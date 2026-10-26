//! Integration tests for lossless segment export.
//!
//! These tests require `ffmpeg` and `ffprobe` in `PATH`. They generate a small
//! 1080p30 H.264/AAC fixture so the suite stays fast and does not depend on
//! external media files.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rossresscut::domain::time::Timestamp;
use rossresscut::media::export::{ExportEvent, ExportProcess, ExportRequest};

fn fixture_directory() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let directory =
        std::env::temp_dir().join(format!("rossresscut-export-{}-{stamp}", std::process::id()));
    fs::create_dir_all(&directory).expect("create fixture directory");
    directory
}

fn generate_fixture(path: &Path) {
    let status = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=1920x1080:rate=30",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000",
            "-t",
            "10",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-pix_fmt",
            "yuv420p",
            "-g",
            "60",
            "-c:a",
            "aac",
            "-b:a",
            "128k",
        ])
        .arg(path)
        .status()
        .expect("run ffmpeg to generate the fixture");
    assert!(status.success(), "ffmpeg could not generate the fixture");
}

struct StreamFacts {
    codec_name: String,
    width: u32,
    height: u32,
    sample_rate: u32,
    channels: u32,
}

fn probe(path: &Path) -> StreamFacts {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=codec_name,width,height",
            "-of",
            "json",
        ])
        .arg(path)
        .output()
        .expect("run ffprobe");
    assert!(
        output.status.success(),
        "ffprobe failed on the video stream"
    );

    let video: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse ffprobe video json");
    let video = &video["streams"][0];

    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "a:0",
            "-show_entries",
            "stream=codec_name,sample_rate,channels",
            "-of",
            "json",
        ])
        .arg(path)
        .output()
        .expect("run ffprobe");
    assert!(
        output.status.success(),
        "ffprobe failed on the audio stream"
    );

    let audio: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse ffprobe audio json");
    let audio = &audio["streams"][0];

    StreamFacts {
        codec_name: video["codec_name"].as_str().unwrap_or_default().to_owned(),
        width: video["width"].as_u64().unwrap_or_default() as u32,
        height: video["height"].as_u64().unwrap_or_default() as u32,
        sample_rate: audio["sample_rate"]
            .as_str()
            .and_then(|value| value.parse().ok())
            .unwrap_or_default(),
        channels: audio["channels"].as_u64().unwrap_or_default() as u32,
    }
}

fn duration_seconds(path: &Path) -> f64 {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
        ])
        .arg(path)
        .output()
        .expect("run ffprobe for duration");
    assert!(output.status.success(), "ffprobe failed on duration");
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .expect("numeric duration")
}

fn run_export(request: &ExportRequest) -> (bool, String) {
    let mut process = ExportProcess::start(request).expect("start export");
    let deadline = Instant::now() + Duration::from_secs(90);
    let mut finished = false;
    let mut ended = false;
    let mut stderr = String::new();

    while Instant::now() < deadline {
        while let Some(event) = process.poll() {
            match event {
                ExportEvent::Progress { .. } => {}
                ExportEvent::Finished {
                    stderr: text,
                    ended: reported_end,
                } => {
                    stderr = text;
                    ended = reported_end;
                    finished = true;
                }
            }
        }
        if finished {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }

    assert!(finished, "export did not finish before the deadline");
    let status = process.wait().expect("wait for ffmpeg");
    (ended && status.success(), stderr)
}

#[test]
fn exports_a_segment_without_reencoding() {
    let directory = fixture_directory();
    let source = directory.join("source.mp4");
    let output = directory.join("output.mp4");
    generate_fixture(&source);

    let request = ExportRequest::new(
        &source,
        Timestamp::from_micros(2_000_000),
        Timestamp::from_micros(6_000_000),
        &output,
    )
    .expect("valid export request");

    let (succeeded, stderr) = run_export(&request);
    assert!(succeeded, "ffmpeg export failed: {stderr}");

    let input = probe(&source);
    let exported = probe(&output);
    assert_eq!(exported.codec_name, input.codec_name);
    assert_eq!(exported.width, input.width);
    assert_eq!(exported.height, input.height);
    assert_eq!(exported.sample_rate, input.sample_rate);
    assert_eq!(exported.channels, input.channels);

    let duration = duration_seconds(&output);
    assert!(
        (3.5..=4.5).contains(&duration),
        "expected about 4 seconds, got {duration}"
    );

    if let Err(error) = fs::remove_dir_all(&directory) {
        eprintln!("could not remove {}: {error}", directory.display());
    }
}

#[test]
fn cancel_is_idempotent_and_reaps_the_process() {
    let directory = fixture_directory();
    let source = directory.join("source.mp4");
    let output = directory.join("cancelled.mp4");
    generate_fixture(&source);

    let request = ExportRequest::new(
        &source,
        Timestamp::from_micros(0),
        Timestamp::from_micros(9_000_000),
        &output,
    )
    .expect("valid export request");

    let mut process = ExportProcess::start(&request).expect("start export");
    process.cancel();
    process.cancel();

    // Stream copy of a short fixture can finish before the cancel reaches the
    // process. Both outcomes are valid; the test only requires that cancel is
    // idempotent and that the process is reaped without hanging.
    let status = process.wait().expect("wait for cancelled ffmpeg");
    let _ = status;

    if let Err(error) = fs::remove_dir_all(&directory) {
        eprintln!("could not remove {}: {error}", directory.display());
    }
}
