//! Phase 4A playback diagnostic probe.
//!
//! This is a throwaway measurement tool, not part of the product. It drives the
//! application layer without any UI or GPU work, so it separates the playback
//! clock and frame scheduling from the egui / wgpu presentation path.
//!
//! Usage:
//!
//! ```text
//! cargo run --release --example phase4a_probe -- \
//!     <file> <software|vaapi|d3d11va|videotoolbox> [seconds] [start_seconds]
//! ```
//!
//! Reported values:
//!
//! - `video` is the preview frame timestamp. The preview decoder uses
//!   `fps=30` and synthesises CFR timestamps as `start + index / 30`; it is not
//!   the source PTS.
//! - `audio` is the application playback position. While audio is running it is
//!   derived from consumed device sample frames.
//! - `delta` is `audio - video`. A growing positive delta means video
//!   presentation is falling behind the audio clock.

use std::path::PathBuf;
use std::process::ExitCode;
use std::thread;
use std::time::{Duration, Instant};

use rossresscut::application::{Application, OpenState};
use rossresscut::domain::media::MediaStream;
use rossresscut::domain::time::Timestamp;
use rossresscut::media::preview::HardwareAcceleration;

const USAGE: &str =
    "usage: phase4a_probe <file> <software|vaapi|d3d11va|videotoolbox> [seconds] [start_seconds]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("phase4a_probe: {message}");
            eprintln!("{USAGE}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<(), String> {
    let path = args.get(1).ok_or("missing media file path")?;
    let hardware = parse_hardware(args.get(2).ok_or("missing hardware backend")?)?;
    let seconds = parse_number(args.get(3), 15.0)?;
    let start = parse_number(args.get(4), 60.0)?;

    let mut application = Application::new();
    application.set_preview_hardware(hardware);
    application.open_media(PathBuf::from(path));

    wait_until_ready(&mut application)?;

    let source = application
        .media()
        .ok_or("media became ready without a probed file")?;
    let video = source.streams.iter().find_map(|stream| match stream {
        MediaStream::Video(video) => Some(video),
        _ => None,
    });
    let dimensions = video
        .and_then(|video| Some((video.width?, video.height?)))
        .map_or("unknown".to_owned(), |(width, height)| {
            format!("{width}x{height}")
        });
    let duration = source.duration.map_or("unknown".to_owned(), |duration| {
        format!("{:.3}s", duration.micros() as f64 / 1_000_000.0)
    });
    println!("media: {dimensions} duration={duration}");
    println!("hardware: {hardware:?}");

    application.seek(Timestamp::from_micros(seconds_to_micros(start)));
    application.play();

    let audio_clock = application.uses_audio_clock();
    println!("clock: {}", if audio_clock { "audio" } else { "wall" });
    if !audio_clock {
        println!("WARNING: no audio output; deltas measure the wall clock, not audio sync");
    }

    let mut probe = Probe::new(Instant::now());
    let began = Instant::now();
    let limit = Duration::from_secs_f64(seconds);

    while began.elapsed() < limit {
        application.update();
        let video = application.frame_timestamp();
        let position = application.position();
        probe.observe(video, position);

        if probe.report_due() {
            probe.report();
        }
        thread::sleep(Duration::from_millis(5));
    }

    if let Some(warning) = application.warning() {
        println!("warning: {warning}");
    }

    probe.summarise(seconds);
    Ok(())
}

fn wait_until_ready(application: &mut Application) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        application.update();
        match application.state() {
            OpenState::Ready => return Ok(()),
            OpenState::Failed { message, .. } => {
                return Err(format!("could not open media: {message}"));
            }
            _ => {}
        }
        if Instant::now() >= deadline {
            return Err("timed out waiting for ffprobe".to_owned());
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn parse_hardware(value: &str) -> Result<HardwareAcceleration, String> {
    match value {
        "software" | "none" => Ok(HardwareAcceleration::None),
        "vaapi" => Ok(HardwareAcceleration::Vaapi),
        "d3d11va" => Ok(HardwareAcceleration::D3d11va),
        "videotoolbox" => Ok(HardwareAcceleration::VideoToolbox),
        other => Err(format!("unknown hardware backend {other:?}")),
    }
}

fn parse_number(value: Option<&String>, fallback: f64) -> Result<f64, String> {
    match value {
        None => Ok(fallback),
        Some(text) => match text.parse::<f64>() {
            Ok(number) if number.is_finite() && number >= 0.0 => Ok(number),
            _ => Err(format!("invalid non-negative number {text:?}")),
        },
    }
}

/// Converts non-negative seconds from the command line to microseconds.
fn seconds_to_micros(seconds: f64) -> i64 {
    (seconds * 1_000_000.0).round() as i64
}

/// One displayed preview frame.
#[derive(Clone, Copy)]
struct Sample {
    video: Timestamp,
    audio: Timestamp,
}

/// Accumulates presentation cadence and A/V drift samples.
struct Probe {
    began: Instant,
    last_report: Instant,
    last_video: Option<Timestamp>,
    last_present: Option<Instant>,
    last_sample: Option<Sample>,
    samples: Vec<Sample>,
    intervals_ms: Vec<f64>,
    stalls: u32,
    max_stall_ms: f64,
}

impl Probe {
    fn new(began: Instant) -> Self {
        Self {
            began,
            last_report: began,
            last_video: None,
            last_present: None,
            last_sample: None,
            samples: Vec::new(),
            intervals_ms: Vec::new(),
            stalls: 0,
            max_stall_ms: 0.0,
        }
    }

    fn observe(&mut self, video: Option<Timestamp>, audio: Timestamp) {
        let Some(video) = video else {
            return;
        };
        if self.last_video == Some(video) {
            return;
        }

        let now = Instant::now();
        if let Some(previous) = self.last_present {
            let interval = now.duration_since(previous).as_secs_f64() * 1_000.0;
            self.intervals_ms.push(interval);
            if interval > 100.0 {
                self.stalls += 1;
            }
            self.max_stall_ms = self.max_stall_ms.max(interval);
        }

        self.last_video = Some(video);
        self.last_present = Some(now);
        let sample = Sample { video, audio };
        self.last_sample = Some(sample);
        self.samples.push(sample);
    }

    fn report_due(&self) -> bool {
        self.last_report.elapsed() >= Duration::from_millis(250)
    }

    fn report(&mut self) {
        self.last_report = Instant::now();
        let Some(sample) = self.last_sample.as_ref() else {
            return;
        };
        println!(
            "t={:6.2} video={:9.3} audio={:9.3} delta={:+7.3} fps={:5.1}",
            self.began.elapsed().as_secs_f64(),
            micros_to_seconds(sample.video),
            micros_to_seconds(sample.audio),
            delta_ms(sample),
            self.effective_fps(),
        );
    }

    fn effective_fps(&self) -> f64 {
        let elapsed = self.began.elapsed().as_secs_f64();
        if elapsed <= 0.0 {
            return 0.0;
        }
        self.samples.len() as f64 / elapsed
    }

    fn summarise(&self, requested_seconds: f64) {
        let deltas: Vec<f64> = self.samples.iter().map(delta_ms).collect();
        println!("\n--- summary ---");
        println!("requested_seconds: {requested_seconds:.2}");
        println!("presented_frames: {}", self.samples.len());
        println!("effective_fps: {:.2}", self.effective_fps());

        if !self.intervals_ms.is_empty() {
            println!(
                "present_interval_ms: mean={:.2} min={:.2} max={:.2}",
                mean(&self.intervals_ms),
                min(&self.intervals_ms),
                max(&self.intervals_ms),
            );
        }
        println!(
            "stalls_over_100ms: {} max_stall_ms={:.1}",
            self.stalls, self.max_stall_ms
        );

        if deltas.is_empty() {
            println!("delta_ms: no samples");
            return;
        }
        println!(
            "delta_ms: mean={:+.3} min={:+.3} max={:+.3} stddev={:.3}",
            mean(&deltas),
            min(&deltas),
            max(&deltas),
            stddev(&deltas, mean(&deltas)),
        );

        let split = deltas.len() / 3;
        if split > 0 {
            let first = mean(&deltas[..split]);
            let last = mean(&deltas[deltas.len() - split..]);
            println!(
                "delta_drift_ms: first_third={first:+.3} last_third={last:+.3} drift={:+.3}",
                last - first,
            );
        }
    }
}

fn delta_ms(sample: &Sample) -> f64 {
    (sample.audio.micros() - sample.video.micros()) as f64 / 1_000.0
}

fn micros_to_seconds(timestamp: Timestamp) -> f64 {
    timestamp.micros() as f64 / 1_000_000.0
}

fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.iter().sum::<f64>() / values.len() as f64
}

fn min(values: &[f64]) -> f64 {
    values.iter().copied().fold(f64::INFINITY, f64::min)
}

fn max(values: &[f64]) -> f64 {
    values.iter().copied().fold(f64::NEG_INFINITY, f64::max)
}

fn stddev(values: &[f64], mean: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let variance = values
        .iter()
        .map(|value| {
            let difference = value - mean;
            difference * difference
        })
        .sum::<f64>()
        / values.len() as f64;
    variance.sqrt()
}
