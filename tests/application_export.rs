//! Integration test for the application-level export workflow.
//!
//! Requires `ffmpeg` and `ffprobe` in `PATH`. Generates a small 1080p30 fixture
//! and drives `Application` exactly as the UI does: open, mark, add segment,
//! export, poll until completion.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rossresscut::application::{Application, ExportStatus, OpenState};
use rossresscut::domain::time::Timestamp;

fn fixture_directory() -> PathBuf {
    // Tests run in parallel and the clock alone can collide on a fast machine,
    // which used to make one test delete another test's workspace.
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "rossresscut-app-export-{}-{stamp}-{unique}",
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
            "color=c=0xFF0000:s=320x240:d=5:r=30",
            "-f",
            "lavfi",
            "-i",
            "color=c=0x00FF00:s=320x240:d=5:r=30",
            "-f",
            "lavfi",
            "-i",
            "color=c=0x0000FF:s=320x240:d=5:r=30",
            "-filter_complex",
            "[0:v][1:v][2:v]concat=n=3:v=1:a=0[v]",
            "-map",
            "[v]",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-g",
            "30",
            "-keyint_min",
            "30",
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
        .start_export(
            output.clone(),
            rossresscut::application::ExportOptions {
                chapters: true,
                ..Default::default()
            },
        )
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
    assert_eq!(chapter_count(&output), 1, "one chapter for one segment");

    if let Err(error) = fs::remove_dir_all(&directory) {
        eprintln!("could not remove {}: {error}", directory.display());
    }
}

const WIDTH: usize = 320;
const HEIGHT: usize = 240;

fn frame_at(path: &Path, seconds: &str) -> Vec<u8> {
    let output = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(path)
        .args([
            "-ss",
            seconds,
            "-frames:v",
            "1",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgb24",
            "-",
        ])
        .output()
        .expect("run ffmpeg to extract a frame");
    assert!(output.status.success(), "could not extract a frame");
    output.stdout
}

fn centre_colour(frame: &[u8]) -> (u8, u8, u8) {
    let index = ((HEIGHT / 2) * WIDTH + WIDTH / 2) * 3;
    (frame[index], frame[index + 1], frame[index + 2])
}

fn dominant(rgb: (u8, u8, u8)) -> &'static str {
    let (red, green, blue) = rgb;
    if red > 120 && red > green.saturating_add(60) && red > blue.saturating_add(60) {
        "red"
    } else if green > 120 && green > red.saturating_add(60) && green > blue.saturating_add(60) {
        "green"
    } else if blue > 120 && blue > red.saturating_add(60) && blue > green.saturating_add(60) {
        "blue"
    } else {
        "other"
    }
}

fn chapter_count(path: &Path) -> usize {
    let output = Command::new("ffprobe")
        .args(["-v", "error", "-show_chapters", "-of", "json"])
        .arg(path)
        .output()
        .expect("run ffprobe for chapters");
    assert!(output.status.success(), "ffprobe failed on chapters");
    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse chapter json");
    value["chapters"].as_array().map_or(0, Vec::len)
}

fn chapter_titles(path: &Path) -> Vec<String> {
    let output = Command::new("ffprobe")
        .args(["-v", "error", "-show_chapters", "-of", "json"])
        .arg(path)
        .output()
        .expect("run ffprobe for chapters");
    assert!(output.status.success(), "ffprobe failed on chapters");
    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse chapter json");
    value["chapters"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|chapter| chapter["tags"]["title"].as_str().map(str::to_owned))
        .collect()
}

fn chapter_end_times(path: &Path) -> Vec<f64> {
    let output = Command::new("ffprobe")
        .args(["-v", "error", "-show_chapters", "-of", "json"])
        .arg(path)
        .output()
        .expect("run ffprobe for chapters");
    assert!(output.status.success(), "ffprobe failed on chapters");
    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse chapter json");
    value["chapters"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|chapter| chapter["end_time"].as_str()?.parse().ok())
        .collect()
}

fn output_duration(path: &Path) -> f64 {
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
        .expect("run ffprobe for duration");
    assert!(output.status.success(), "ffprobe failed on duration");
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .expect("duration is a number")
}

#[test]
fn cut_points_create_and_split_segments() {
    let directory = fixture_directory();
    let source = directory.join("source.mp4");
    generate_fixture(&source);

    let mut application = Application::new();
    application.open_media(source.clone());
    wait_for_ready(&mut application);

    application
        .add_cut_point(Timestamp::from_micros(2_000_000))
        .expect("add the first cut point");
    assert!(application.timeline().is_empty());
    assert_eq!(
        application.in_point(),
        Some(Timestamp::from_micros(2_000_000))
    );

    application
        .add_cut_point(Timestamp::from_micros(4_000_000))
        .expect("add the second cut point");
    assert_eq!(application.timeline().len(), 1);
    let segment = application.timeline().segments()[0];
    assert_eq!(segment.start(), Timestamp::from_micros(2_000_000));
    assert_eq!(segment.end(), Timestamp::from_micros(4_000_000));

    application.select_segment(Some(segment.id()));
    application.seek(Timestamp::from_micros(3_000_000));
    application
        .split_selected_segment()
        .expect("split at the playhead");
    assert_eq!(application.timeline().len(), 2);

    application.remove_cut_point();
    assert_eq!(application.timeline().len(), 1);

    if let Err(error) = fs::remove_dir_all(&directory) {
        eprintln!("could not remove {}: {error}", directory.display());
    }
}

#[test]
fn exports_enabled_segments_in_timeline_order() {
    let directory = fixture_directory();
    let source = directory.join("source.mp4");
    let output = directory.join("ordered.mp4");
    generate_fixture(&source);

    let mut application = Application::new();
    application.open_media(source.clone());
    wait_for_ready(&mut application);

    // Source order is red, green, blue. Timeline order is blue, red, green.
    for (start, end) in [(10, 15), (0, 5), (5, 10)] {
        application.set_in_point(Timestamp::from_micros(start * 1_000_000));
        application.set_out_point(Timestamp::from_micros(end * 1_000_000));
        application
            .add_segment()
            .expect("create a segment from the markers");
    }

    application
        .start_export(
            output.clone(),
            rossresscut::application::ExportOptions {
                chapters: true,
                ..Default::default()
            },
        )
        .expect("start multi-segment export");
    wait_for_export(&mut application);

    match application.export_status() {
        ExportStatus::Completed { output: written } => assert_eq!(written, &output),
        other => panic!("expected a completed export, got {other:?}"),
    }

    let first = dominant(centre_colour(&frame_at(&output, "1")));
    let second = dominant(centre_colour(&frame_at(&output, "6")));
    let third = dominant(centre_colour(&frame_at(&output, "11")));
    assert_eq!(
        (first, second, third),
        ("blue", "red", "green"),
        "application export must follow timeline order"
    );
    assert_eq!(chapter_count(&output), 3, "one chapter per segment");

    if let Err(error) = fs::remove_dir_all(&directory) {
        eprintln!("could not remove {}: {error}", directory.display());
    }
}

#[test]
fn exports_custom_chapter_titles() {
    let directory = fixture_directory();
    let source = directory.join("source.mp4");
    let output = directory.join("titled.mp4");
    generate_fixture(&source);

    let mut application = Application::new();
    application.open_media(source);
    wait_for_ready(&mut application);

    let mut ids = Vec::new();
    for (start, end) in [(0, 5), (5, 10)] {
        application.set_in_point(Timestamp::from_micros(start * 1_000_000));
        application.set_out_point(Timestamp::from_micros(end * 1_000_000));
        ids.push(
            application
                .add_segment()
                .expect("create a segment from the markers"),
        );
    }
    application.set_chapter_title(ids[0], "Opening".to_owned());
    application.set_chapter_title(ids[1], "Climax".to_owned());

    application
        .start_export(
            output.clone(),
            rossresscut::application::ExportOptions {
                chapters: true,
                ..Default::default()
            },
        )
        .expect("start multi-segment export");
    wait_for_export(&mut application);

    assert!(
        matches!(application.export_status(), ExportStatus::Completed { .. }),
        "export did not complete: {:?}",
        application.export_status()
    );
    assert_eq!(chapter_titles(&output), vec!["Opening", "Climax"]);

    if let Err(error) = fs::remove_dir_all(&directory) {
        eprintln!("could not remove {}: {error}", directory.display());
    }
}

#[test]
fn single_segment_chapter_uses_the_actual_output_duration() {
    let directory = fixture_directory();
    let source = directory.join("source.mp4");
    let output = directory.join("single.mp4");
    generate_fixture(&source);

    let mut application = Application::new();
    application.open_media(source);
    wait_for_ready(&mut application);
    let history_before = application.executed_ffmpeg_command_lines().len();

    // The fixture has a one-second GOP, so a 1.5s start falls back to 1.0s and
    // the output is longer than the requested 4.5s.
    application.set_in_point(Timestamp::from_micros(1_500_000));
    application.set_out_point(Timestamp::from_micros(6_000_000));
    application
        .add_segment()
        .expect("create a segment from the markers");

    application
        .start_export(
            output.clone(),
            rossresscut::application::ExportOptions {
                chapters: true,
                ..Default::default()
            },
        )
        .expect("start single-segment export");
    wait_for_export(&mut application);

    assert!(
        matches!(application.export_status(), ExportStatus::Completed { .. }),
        "export did not complete: {:?}",
        application.export_status()
    );

    let duration = output_duration(&output);
    let ends = chapter_end_times(&output);
    assert_eq!(ends.len(), 1, "one chapter for the single segment");
    assert!(
        duration > 4.7,
        "stream copy should start before 1.5s, got {duration}s"
    );
    assert!(
        (ends[0] - duration).abs() < 0.1,
        "chapter end {} should follow the output duration {duration}",
        ends[0]
    );
    let history_after = application.executed_ffmpeg_command_lines().len();
    assert!(
        history_after >= history_before + 2,
        "cutting and concatenating are separate processes ({history_before} -> {history_after})"
    );

    if let Err(error) = fs::remove_dir_all(&directory) {
        eprintln!("could not remove {}: {error}", directory.display());
    }
}
