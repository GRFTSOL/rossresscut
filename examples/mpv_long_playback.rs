//! Long-playback A/V drift probe for the mpv preview path (OQ-9).
//!
//! Drives libmpv through the same exported-frame backend the preview uses,
//! without the egui / wgpu presentation path, and samples `time-pos`,
//! `audio-pts`, frame-drop counters and RSS.
//!
//! Usage:
//!
//! ```text
//! cargo run --release --features mpv-preview --example mpv_long_playback -- \
//!     <file> [seconds] [width] [height]
//! ```
//!
//! Reported values:
//!
//! - `position` is mpv's `time-pos`, the playback clock the UI reads.
//! - `audio` is mpv's `audio-pts`, the PTS of the audio frame being played.
//! - `delta` is `position - audio`; a growing value means the playback clock
//!   is drifting away from the audio device.
//! - drop counters come from mpv: `frame-drop-count`,
//!   `mistimed-frame-count`, `decoder-frame-drop-count`.
//! - RSS is the probe process's resident set (Linux only), which includes
//!   libmpv and its decoder / export buffers.

use std::process::ExitCode;
use std::thread;
use std::time::{Duration, Instant};

use mpv_engine::{Engine, ExportOptions};

const USAGE: &str = "usage: mpv_long_playback <file> [seconds] [width] [height]";

const SAMPLE_INTERVAL: Duration = Duration::from_secs(1);
const POLL_INTERVAL: Duration = Duration::from_millis(5);

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("mpv_long_playback: {message}");
            eprintln!("{USAGE}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<(), String> {
    let path = args.get(1).ok_or("missing media file path")?;
    let seconds = parse_number(args.get(2), 600.0)?;
    let width = parse_dimension(args.get(3), 1280)?;
    let height = parse_dimension(args.get(4), 720)?;

    let engine = Engine::video()
        .property("keep-open", "yes")
        .property("hr-seek", "yes")
        .property("hwdec", "auto-safe")
        .build()
        .map_err(|error| format!("could not create the mpv engine: {error}"))?;

    engine
        .attach_exported_render(ExportOptions::new(width, height), || {})
        .map_err(|error| format!("could not attach the exported render: {error}"))?;

    engine
        .load_when_ready(path)
        .map_err(|error| format!("could not load {path}: {error}"))?;

    wait_for_position(&engine)?;

    println!("file: {path}");
    println!("requested_seconds: {seconds:.1}");
    println!(
        "hwdec: {}",
        engine
            .get_property::<String>("hwdec-current")
            .unwrap_or_else(|_| "unknown".to_owned())
    );
    println!(
        "audio_out: {}",
        engine
            .get_property::<String>("current-ao")
            .unwrap_or_else(|_| "unknown".to_owned())
    );
    println!(
        "audio_device: {}",
        engine
            .get_property::<String>("audio-device")
            .unwrap_or_else(|_| "unknown".to_owned())
    );
    println!(
        "audio_params: {}",
        engine
            .get_property::<String>("audio-out-params")
            .unwrap_or_else(|_| "unknown".to_owned())
    );
    println!(
        "video_sync: {}",
        engine
            .get_property::<String>("video-sync")
            .unwrap_or_else(|_| "unknown".to_owned())
    );

    let began = Instant::now();
    let limit = Duration::from_secs_f64(seconds);
    let mut next_sample = Instant::now();
    let mut samples = Vec::new();
    let mut last_report = 0.0_f64;

    while began.elapsed() < limit {
        engine.pump_events();
        engine
            .acquire_frame()
            .map_err(|error| format!("frame acquisition failed: {error}"))?;

        if Instant::now() >= next_sample {
            let sample = Sample::read(&engine, began.elapsed().as_secs_f64());
            if let Some(sample) = sample {
                if sample.at - last_report >= 30.0 {
                    last_report = sample.at;
                    sample.report();
                }
                samples.push(sample);
            }
            if engine.get_property::<bool>("eof-reached").unwrap_or(false) {
                println!("reached the end of the file");
                break;
            }
            next_sample += SAMPLE_INTERVAL;
        }

        thread::sleep(POLL_INTERVAL);
    }

    summarise(&samples, began.elapsed().as_secs_f64(), seconds);
    Ok(())
}

fn wait_for_position(engine: &Engine) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        engine.pump_events();
        engine
            .acquire_frame()
            .map_err(|error| format!("frame acquisition failed: {error}"))?;
        if engine.position().is_some() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("mpv did not report a position".to_owned());
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn parse_number(value: Option<&String>, fallback: f64) -> Result<f64, String> {
    match value {
        None => Ok(fallback),
        Some(text) => match text.parse::<f64>() {
            Ok(number) if number.is_finite() && number > 0.0 => Ok(number),
            _ => Err(format!("invalid positive number {text:?}")),
        },
    }
}

fn parse_dimension(value: Option<&String>, fallback: u32) -> Result<u32, String> {
    match value {
        None => Ok(fallback),
        Some(text) => match text.parse::<u32>() {
            Ok(number) if number > 0 => Ok(number),
            _ => Err(format!("invalid positive dimension {text:?}")),
        },
    }
}

#[derive(Clone, Copy)]
struct Sample {
    at: f64,
    position: f64,
    audio: f64,
    frame_drops: i64,
    mistimed_frames: i64,
    decoder_drops: i64,
    rss_kib: Option<u64>,
}

impl Sample {
    fn read(engine: &Engine, at: f64) -> Option<Self> {
        let position = match engine.get_property::<f64>("time-pos") {
            Ok(value) => value,
            Err(_) => return None,
        };
        let audio = match engine.get_property::<f64>("audio-pts") {
            Ok(value) => value,
            Err(_) => return None,
        };
        Some(Self {
            at,
            position,
            audio,
            frame_drops: integer(engine, "frame-drop-count"),
            mistimed_frames: integer(engine, "mistimed-frame-count"),
            decoder_drops: integer(engine, "decoder-frame-drop-count"),
            rss_kib: resident_kib(),
        })
    }

    fn delta_ms(self) -> f64 {
        (self.position - self.audio) * 1_000.0
    }

    fn report(self) {
        println!(
            "t={:7.1} position={:9.3} audio={:9.3} delta={:+8.3}ms \
             drops={} mistimed={} decoder_drops={} rss={}",
            self.at,
            self.position,
            self.audio,
            self.delta_ms(),
            self.frame_drops,
            self.mistimed_frames,
            self.decoder_drops,
            self.rss_kib
                .map_or_else(|| "n/a".to_owned(), |kib| format!("{}MiB", kib / 1024)),
        );
    }
}

fn integer(engine: &Engine, name: &str) -> i64 {
    engine.get_property::<i64>(name).unwrap_or_default()
}

fn summarise(samples: &[Sample], elapsed: f64, requested: f64) {
    println!("\n--- summary ---");
    println!("requested_seconds: {requested:.1}");
    println!("elapsed_seconds: {elapsed:.2}");
    println!("samples: {}", samples.len());

    let Some(first) = samples.first() else {
        println!("delta_ms: no samples (audio-pts unavailable?)");
        return;
    };
    let last = samples.last().expect("non-empty samples");
    println!(
        "played_seconds: {:.3}",
        (last.position - first.position).max(0.0)
    );

    let deltas: Vec<f64> = samples.iter().map(|sample| sample.delta_ms()).collect();
    let average = mean(&deltas);
    println!(
        "delta_ms: mean={average:+.3} min={:+.3} max={:+.3} stddev={:.3}",
        min(&deltas),
        max(&deltas),
        stddev(&deltas, average),
    );

    let split = deltas.len() / 3;
    if split > 0 {
        let first_third = mean(&deltas[..split]);
        let last_third = mean(&deltas[deltas.len() - split..]);
        println!("delta_ms_first_third: {first_third:+.3}");
        println!("delta_ms_last_third: {last_third:+.3}");
        println!("delta_drift_ms: {:+.3}", last_third - first_third);
    }

    println!(
        "frame_drops: {} mistimed_frames: {} decoder_drops: {}",
        last.frame_drops - first.frame_drops,
        last.mistimed_frames - first.mistimed_frames,
        last.decoder_drops - first.decoder_drops,
    );

    let rss: Vec<u64> = samples.iter().filter_map(|sample| sample.rss_kib).collect();
    if let (Some(first), Some(last)) = (rss.first(), rss.last()) {
        let peak = rss.iter().copied().max().unwrap_or_default();
        println!(
            "rss_mib: first={} last={} peak={} growth={}",
            first / 1024,
            last / 1024,
            peak / 1024,
            last.saturating_sub(*first) / 1024,
        );
    }
}

#[cfg(target_os = "linux")]
fn resident_kib() -> Option<u64> {
    let status = match std::fs::read_to_string("/proc/self/status") {
        Ok(status) => status,
        Err(_) => return None,
    };
    status.lines().find_map(|line| {
        line.strip_prefix("VmRSS:")
            .and_then(|value| value.trim().trim_end_matches(" kB").trim().parse().ok())
    })
}

#[cfg(not(target_os = "linux"))]
fn resident_kib() -> Option<u64> {
    None
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
