//! Integration test for the application-level export workflow.
//!
//! Requires `ffmpeg` and `ffprobe` in `PATH`. Generates a small 1080p30 fixture
//! and drives `Application` exactly as the UI does: open, mark, add segment,
//! export, poll until completion.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rossresscut::application::{Application, ExportStatus, OpenState};
use rossresscut::domain::time::Timestamp;

fn fixture_directory() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "rossresscut-app-export-{}-{stamp}",
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

fn probe_codec(path: &Path) -> (String, u32, u32) {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=codec_name,width,height",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .expect("run ffprobe");
    assert!(output.status.success(), "ffprobe failed");

    let text = String::from_utf8_lossy(&output.stdout);
    let mut fields = text.trim().split(',');
    let codec = fields.next().unwrap_or_default().to_owned();
    let width = fields
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or_default();
    let height = fields
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or_default();
    (codec, width, height)
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

fn wait_for_export(application: &mut Application) {
    let deadline = Instant::now() + Duration::from_secs(90);
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

#[test]
fn exports_the_first_enabled_segment_through_the_application() {
    let directory = fixture_directory();
    let source = directory.join("source.mp4");
    let output = directory.join("output.mp4");
    generate_fixture(&source);

    let mut application = Application::new();
    application.open_media(source.clone());
    wait_for_ready(&mut application);

    application.set_in_point(Timestamp::from_micros(2_000_000));
    application.set_out_point(Timestamp::from_micros(6_000_000));
    application
        .add_segment()
        .expect("create a segment from the markers");

    application
        .start_export(output.clone())
        .expect("start export");

    wait_for_export(&mut application);

    match application.export_status() {
        ExportStatus::Completed { output: written } => {
            assert_eq!(written, &output);
        }
        other => panic!("expected a completed export, got {other:?}"),
    }

    let before = probe_codec(&source);
    let after = probe_codec(&output);
    assert_eq!(after.0, before.0);
    assert_eq!(after.1, before.1);
    assert_eq!(after.2, before.2);

    if let Err(error) = fs::remove_dir_all(&directory) {
        eprintln!("could not remove {}: {error}", directory.display());
    }
}
