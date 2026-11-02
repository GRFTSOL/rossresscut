//! Integration tests for smart-cut export.
//!
//! The fixture is six one-second colour bands (red, green, blue, red, green,
//! blue) at 30 fps with a keyframe every two seconds. Segment starts inside a
//! band therefore differ visibly between smart cut and plain stream copy.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rossresscut::application::{Application, ExportOptions, ExportStatus, OpenState};
use rossresscut::domain::time::Timestamp;

fn fixture_directory() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "rossresscut-smart-cut-{}-{stamp}-{unique}",
        std::process::id()
    ));
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
            "color=c=0xFF0000:s=160x120:d=1:r=30",
            "-f",
            "lavfi",
            "-i",
            "color=c=0x00FF00:s=160x120:d=1:r=30",
            "-f",
            "lavfi",
            "-i",
            "color=c=0x0000FF:s=160x120:d=1:r=30",
            "-f",
            "lavfi",
            "-i",
            "color=c=0xFF0000:s=160x120:d=1:r=30",
            "-f",
            "lavfi",
            "-i",
            "color=c=0x00FF00:s=160x120:d=1:r=30",
            "-f",
            "lavfi",
            "-i",
            "color=c=0x0000FF:s=160x120:d=1:r=30",
            "-filter_complex",
            "[0:v][1:v][2:v][3:v][4:v][5:v]concat=n=6:v=1:a=0[v]",
            "-map",
            "[v]",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-crf",
            "30",
            "-g",
            "60",
            "-keyint_min",
            "60",
            "-sc_threshold",
            "0",
            "-pix_fmt",
            "yuv420p",
        ])
        .arg(path)
        .status()
        .expect("run ffmpeg to generate the fixture");
    assert!(status.success(), "ffmpeg could not generate the fixture");
}

fn wait_for_ready(application: &mut Application) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        application.update();
        if matches!(application.state(), OpenState::Ready) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "media did not become ready before the deadline"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn wait_for_keyframes(application: &mut Application) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        application.update();
        if application.keyframes().is_some() {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "keyframes were not indexed before the deadline"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn wait_for_export(application: &mut Application) {
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        application.update();
        if !application.export_status().is_running() {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "export did not finish before the deadline"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

const WIDTH: usize = 160;
const HEIGHT: usize = 120;

/// Reads the centre pixel of the frame shown at `at_seconds`.
fn centre_pixel(path: &Path, at_seconds: &str) -> (u8, u8, u8) {
    let output = Command::new("ffmpeg")
        .args(["-v", "error", "-ss", at_seconds, "-i"])
        .arg(path)
        .args(["-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"])
        .output()
        .expect("run ffmpeg to read a frame");
    assert!(
        output.status.success(),
        "ffmpeg could not read a frame: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected = WIDTH * HEIGHT * 3;
    assert!(
        output.stdout.len() >= expected,
        "ffmpeg returned {} bytes, expected {expected}",
        output.stdout.len()
    );
    let offset = ((HEIGHT / 2) * WIDTH + WIDTH / 2) * 3;
    (
        output.stdout[offset],
        output.stdout[offset + 1],
        output.stdout[offset + 2],
    )
}

fn probe_duration(path: &Path) -> f64 {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .expect("run ffprobe");
    assert!(output.status.success(), "ffprobe failed");
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .expect("duration is a number")
}

fn open_with_keyframes(source: &Path) -> Application {
    let mut application = Application::new();
    application.open_media(source.to_path_buf());
    wait_for_ready(&mut application);
    wait_for_keyframes(&mut application);
    application
}

fn add_segment(application: &mut Application, start_micros: i64, end_micros: i64) {
    application.set_in_point(Timestamp::from_micros(start_micros));
    application.set_out_point(Timestamp::from_micros(end_micros));
    application
        .add_segment()
        .expect("create a segment from the markers");
}

#[test]
fn smart_cut_starts_at_the_requested_frame() {
    let directory = fixture_directory();
    let source = directory.join("source.mp4");
    let output = directory.join("smart.mp4");
    generate_fixture(&source);

    let mut application = open_with_keyframes(&source);
    // 1.5s is inside the green band; the next keyframe is at 2.0s.
    add_segment(&mut application, 1_500_000, 3_500_000);
    application
        .start_export(
            output.clone(),
            ExportOptions {
                smart_cut: true,
                ..Default::default()
            },
        )
        .expect("smart cut starts");
    wait_for_export(&mut application);

    assert_eq!(
        application.export_status(),
        &ExportStatus::Completed {
            output: output.clone()
        }
    );
    let (red, green, blue) = centre_pixel(&output, "0");
    assert!(
        green > 100 && green > red && green > blue,
        "first frame should be green, got ({red}, {green}, {blue})"
    );
}

#[test]
fn plain_copy_falls_back_to_the_previous_keyframe() {
    let directory = fixture_directory();
    let source = directory.join("source.mp4");
    let output = directory.join("copy.mp4");
    generate_fixture(&source);

    let mut application = open_with_keyframes(&source);
    add_segment(&mut application, 1_500_000, 3_500_000);
    application
        .start_export(output.clone(), ExportOptions::default())
        .expect("copy export starts");
    wait_for_export(&mut application);

    assert_eq!(
        application.export_status(),
        &ExportStatus::Completed {
            output: output.clone()
        }
    );
    let (red, green, blue) = centre_pixel(&output, "0");
    assert!(
        red > 100 && red > green && red > blue,
        "stream copy should start at the red keyframe, got ({red}, {green}, {blue})"
    );
}

#[test]
fn smart_cut_keeps_timeline_order_across_multiple_segments() {
    let directory = fixture_directory();
    let source = directory.join("source.mp4");
    let output = directory.join("ordered.mp4");
    generate_fixture(&source);

    let mut application = open_with_keyframes(&source);
    // [1.5, 2.5) starts in green, [3.5, 4.5) starts in red.
    add_segment(&mut application, 1_500_000, 2_500_000);
    add_segment(&mut application, 3_500_000, 4_500_000);
    application
        .start_export(
            output.clone(),
            ExportOptions {
                smart_cut: true,
                ..Default::default()
            },
        )
        .expect("smart cut starts");
    wait_for_export(&mut application);

    assert_eq!(
        application.export_status(),
        &ExportStatus::Completed {
            output: output.clone()
        }
    );
    assert!(
        (probe_duration(&output) - 2.0).abs() < 0.2,
        "output should be about two seconds long"
    );

    let (red, green, blue) = centre_pixel(&output, "0");
    assert!(
        green > 100 && green > red && green > blue,
        "first segment should start in green, got ({red}, {green}, {blue})"
    );
    let (red, green, blue) = centre_pixel(&output, "1.0");
    assert!(
        red > 100 && red > green && red > blue,
        "second segment should start in red, got ({red}, {green}, {blue})"
    );
}

#[test]
fn smart_cut_rejects_a_codec_without_a_software_encoder() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample.wmv");
    assert!(source.exists(), "the wmv fixture is required");
    let directory = fixture_directory();
    let output = directory.join("smart.wmv");

    let mut application = open_with_keyframes(&source);
    add_segment(&mut application, 200_000, 800_000);
    let error = application
        .start_export(
            output,
            ExportOptions {
                smart_cut: true,
                ..Default::default()
            },
        )
        .expect_err("wmv2 has no mapped software encoder");

    assert!(
        error.to_string().contains("smart cut is unavailable"),
        "unexpected error: {error}"
    );
}
