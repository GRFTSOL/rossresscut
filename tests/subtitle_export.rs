//! Integration test for subtitle-aware export.
//!
//! FFmpeg's `-ss` input seek does not rewrite subtitle packet timestamps, so a
//! plain stream copy keeps cues outside the range and stretches the container.
//! This test covers the SRT cropping path that replaces the source track.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration as StdDuration, Instant, SystemTime, UNIX_EPOCH};

use rossresscut::domain::time::Timestamp;
use rossresscut::media::export::{ExportEvent, ExportProcess, ExportRequest, SubtitlePlan};
use rossresscut::media::subtitle;

const SOURCE: &str = "tests/fixtures/sample.mkv";
const SUBTITLE_STREAM: usize = 3;

fn temporary_output(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "rossresscut-subtitle-{}-{stamp}-{name}",
        std::process::id()
    ))
}

fn run_export(request: &ExportRequest) {
    let mut process = ExportProcess::start(request).expect("start export");
    let deadline = Instant::now() + StdDuration::from_secs(60);

    loop {
        match process.poll() {
            Some(ExportEvent::Progress { .. }) => {}
            Some(ExportEvent::Finished { stderr, ended }) => {
                let exit = process.wait().expect("wait for ffmpeg");
                assert!(ended && exit.success(), "ffmpeg failed: {stderr}; {exit}");
                return;
            }
            None => {
                assert!(Instant::now() < deadline, "export timed out");
                thread::sleep(StdDuration::from_millis(20));
            }
        }
    }
}

/// Matroska reports `N/A` for stream durations, so the last packet timestamp
/// is the reliable way to check what the export actually contains.
fn probe_last_video_pts(path: &Path) -> f64 {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v",
            "-show_entries",
            "packet=pts_time",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .expect("ffprobe");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().parse::<f64>().ok())
        .next_back()
        .expect("video packets")
}

fn probe_subtitle_timestamps(path: &Path) -> Vec<String> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "s",
            "-show_entries",
            "packet=pts_time",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .expect("ffprobe");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| line.trim().to_owned())
        .filter(|line| !line.is_empty())
        .collect()
}

#[test]
fn crops_subtitles_and_keeps_the_container_duration() {
    let srt = subtitle::extract(Path::new(SOURCE), SUBTITLE_STREAM).expect("extract subtitles");
    assert!(srt.contains("Hello"), "fixture subtitles: {srt}");

    let output = temporary_output("cropped.mkv");
    let plan = SubtitlePlan {
        srt,
        keep_partial: true,
    };
    let request = ExportRequest::new(
        SOURCE,
        Timestamp::from_micros(500_000),
        Timestamp::from_micros(1_500_000),
        &output,
    )
    .expect("request")
    .with_subtitle(Some(plan));

    run_export(&request);

    let last_pts = probe_last_video_pts(&output);
    let timestamps = probe_subtitle_timestamps(&output);
    fs::remove_file(&output).ok();

    // The input seek falls back to the previous keyframe, so the video covers
    // at least the requested second rather than exactly one second.
    assert!(
        last_pts >= 0.9,
        "the exported video should cover the requested second, last pts {last_pts}"
    );
    // Both cues overlap the range and are clipped, so exactly two remain.
    assert_eq!(
        timestamps.len(),
        2,
        "unexpected subtitle packets: {timestamps:?}"
    );
}

#[test]
fn dropping_partial_cues_removes_the_crossing_one() {
    let srt = subtitle::extract(Path::new(SOURCE), SUBTITLE_STREAM).expect("extract subtitles");

    let output = temporary_output("dropped.mkv");
    let plan = SubtitlePlan {
        srt,
        keep_partial: false,
    };
    let request = ExportRequest::new(
        SOURCE,
        Timestamp::from_micros(500_000),
        Timestamp::from_micros(1_500_000),
        &output,
    )
    .expect("request")
    .with_subtitle(Some(plan));

    run_export(&request);

    let timestamps = probe_subtitle_timestamps(&output);
    fs::remove_file(&output).ok();

    // "Hello" starts before the cut and "World" extends past its end, so both
    // are dropped and only the output container remains.
    assert!(
        timestamps.is_empty(),
        "expected no subtitle packets, got {timestamps:?}"
    );
}
