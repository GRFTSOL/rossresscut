//! Integration tests for multi-segment lossless export.
//!
//! Requires `ffmpeg` and `ffprobe` in `PATH`. The fixture is a 15-second
//! 320x240 30fps clip with red, green and blue five-second sections, so the
//! output order can be verified from frame colours.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rossresscut::domain::time::Timestamp;
use rossresscut::media::export::{
    ExportSegment, MultiExport, MultiExportEvent, MultiExportRequest,
};

const WIDTH: usize = 320;
const HEIGHT: usize = 240;

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
        "rossresscut-multi-{}-{stamp}-{unique}",
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

fn generate_fixture_with_audio(path: &Path) {
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
            "color=c=0xFF0000:s=320x240:d=15:r=30",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=44100:duration=15",
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
            "-c:a",
            "aac",
            "-b:a",
            "128k",
        ])
        .arg(path)
        .status()
        .expect("run ffmpeg to generate the audio fixture");
    assert!(
        status.success(),
        "ffmpeg could not generate the audio fixture"
    );
}

fn segment(start_seconds: i64, end_seconds: i64) -> ExportSegment {
    segment_micros(start_seconds * 1_000_000, end_seconds * 1_000_000)
}

fn segment_micros(start: i64, end: i64) -> ExportSegment {
    ExportSegment::new(Timestamp::from_micros(start), Timestamp::from_micros(end))
        .expect("valid segment")
}

fn run_export(export: &mut MultiExport) -> Option<String> {
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        if let Some(event) = export.poll() {
            match event {
                MultiExportEvent::Progress { .. } => {}
                MultiExportEvent::Finished { error } => return error,
            }
        }
        assert!(
            Instant::now() < deadline,
            "multi-segment export did not finish"
        );
        thread::sleep(Duration::from_millis(10));
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
        .expect("run ffprobe");
    assert!(output.status.success(), "ffprobe failed");
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .expect("numeric duration")
}

fn packet_pts(path: &Path, selector: &str) -> Vec<f64> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            selector,
            "-show_entries",
            "packet=pts_time",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .expect("run ffprobe for packet timestamps");
    assert!(
        output.status.success(),
        "ffprobe failed while reading {selector} packets"
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().split(',').next()?.parse().ok())
        .collect()
}

fn chapters(path: &Path) -> Vec<(f64, f64)> {
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
        .map(|chapters| {
            chapters
                .iter()
                .map(|chapter| {
                    let start = chapter["start_time"]
                        .as_str()
                        .expect("chapter start")
                        .parse()
                        .expect("numeric chapter start");
                    let end = chapter["end_time"]
                        .as_str()
                        .expect("chapter end")
                        .parse()
                        .expect("numeric chapter end");
                    (start, end)
                })
                .collect()
        })
        .unwrap_or_default()
}

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
    assert_eq!(
        output.stdout.len(),
        WIDTH * HEIGHT * 3,
        "unexpected frame size"
    );
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

#[test]
fn chapters_follow_actual_part_lengths() {
    let directory = fixture_directory();
    let source = directory.join("source.mp4");
    let output = directory.join("chapters.mkv");
    generate_fixture(&source);

    // Neither start is a keyframe, so FFmpeg falls back to the previous
    // keyframe and makes each part longer than requested.
    let segments = vec![
        ExportSegment::new(
            Timestamp::from_micros(500_000),
            Timestamp::from_micros(2_500_000),
        )
        .expect("valid segment"),
        ExportSegment::new(
            Timestamp::from_micros(5_200_000),
            Timestamp::from_micros(6_400_000),
        )
        .expect("valid segment"),
    ];
    let request = MultiExportRequest::new(&source, segments, &output)
        .expect("valid request")
        .with_chapters(true);
    let mut export = MultiExport::start(request).expect("start multi export");
    let error = run_export(&mut export);
    assert!(error.is_none(), "multi export failed: {error:?}");
    drop(export);

    let chapters = chapters(&output);
    assert_eq!(chapters.len(), 2, "one chapter per segment");
    assert!(chapters[0].0.abs() < 0.01, "first chapter starts at zero");
    assert!(
        (chapters[1].0 - chapters[0].1).abs() < 0.01,
        "chapters must be contiguous: {chapters:?}"
    );

    let output_duration = duration_seconds(&output);
    assert!(
        (chapters[1].1 - output_duration).abs() < 0.35,
        "last chapter must reach the output end: {chapters:?}, output {output_duration}"
    );
    assert!(
        chapters[0].1 > 2.2,
        "keyframe fallback should lengthen the first part: {chapters:?}"
    );
    assert!(
        chapters[1].1 > 3.2,
        "chapters must use produced lengths, not requested ones: {chapters:?}"
    );

    if let Err(error) = fs::remove_dir_all(&directory) {
        eprintln!("could not remove {}: {error}", directory.display());
    }
}

#[test]
fn preserves_timeline_order_instead_of_source_order() {
    let directory = fixture_directory();
    let source = directory.join("source.mp4");
    let output = directory.join("output.mp4");
    generate_fixture(&source);

    // Source order is red, green, blue. Timeline order is blue, red, green.
    let request = MultiExportRequest::new(
        &source,
        vec![segment(10, 15), segment(0, 5), segment(5, 10)],
        &output,
    )
    .expect("valid request");
    let mut export = MultiExport::start(request).expect("start multi export");
    let error = run_export(&mut export);
    assert!(error.is_none(), "multi export failed: {error:?}");
    let work_dir = export.work_dir().to_path_buf();
    drop(export);

    assert!(
        !work_dir.exists(),
        "temporary export directory was not removed"
    );

    let duration = duration_seconds(&output);
    assert!(
        (14.5..=15.5).contains(&duration),
        "expected about 15 seconds, got {duration}"
    );

    let first = dominant(centre_colour(&frame_at(&output, "1")));
    let second = dominant(centre_colour(&frame_at(&output, "6")));
    let third = dominant(centre_colour(&frame_at(&output, "11")));
    assert_eq!(
        (first, second, third),
        ("blue", "red", "green"),
        "output must follow timeline order"
    );

    if let Err(error) = fs::remove_dir_all(&directory) {
        eprintln!("could not remove {}: {error}", directory.display());
    }
}

#[test]
fn cancellation_removes_temporary_files() {
    let directory = fixture_directory();
    let source = directory.join("source.mp4");
    let output = directory.join("cancelled.mp4");
    generate_fixture(&source);

    let request = MultiExportRequest::new(
        &source,
        vec![segment(0, 5), segment(5, 10), segment(10, 15)],
        &output,
    )
    .expect("valid request");
    let mut export = MultiExport::start(request).expect("start multi export");
    let work_dir = export.work_dir().to_path_buf();
    assert!(work_dir.exists(), "temporary directory should exist");

    export.cancel();
    drop(export);

    assert!(
        !work_dir.exists(),
        "temporary export directory was not removed after cancellation"
    );

    if let Err(error) = fs::remove_dir_all(&directory) {
        eprintln!("could not remove {}: {error}", directory.display());
    }
}

#[test]
fn joins_parts_without_a_frame_gap() {
    let directory = fixture_directory();
    let source = directory.join("source-av.mp4");
    let output = directory.join("gapless.mkv");
    generate_fixture_with_audio(&source);

    // Neither start is a keyframe, so each part is longer than requested and
    // its first packet is not at zero. The join offset must still keep a normal
    // frame cadence instead of inserting a one-frame gap.
    let request = MultiExportRequest::new(
        &source,
        vec![
            segment_micros(3_300_000, 7_300_000),
            segment_micros(10_700_000, 13_700_000),
        ],
        &output,
    )
    .expect("valid request");
    let mut export = MultiExport::start(request).expect("start multi export");
    let error = run_export(&mut export);
    assert!(error.is_none(), "multi export failed: {error:?}");
    drop(export);

    // Packets are listed in muxing order; B-frames can reorder PTS.
    let mut video = packet_pts(&output, "v:0");
    video.sort_by(|a, b| a.partial_cmp(b).expect("finite video timestamps"));
    assert!(
        video.len() > 200,
        "unexpected video packet count: {}",
        video.len()
    );
    for window in video.windows(2) {
        let delta = window[1] - window[0];
        assert!(
            delta > 0.0,
            "video timestamps must increase: {} → {}",
            window[0],
            window[1]
        );
        // One 30fps frame is 33.3ms. A one-frame gap at the join used to
        // measure 66.7ms, so anything below 50ms is a normal cadence.
        assert!(
            delta < 0.05,
            "video gap of {:.1}ms after {:.3}s is a stall",
            delta * 1000.0,
            window[0]
        );
    }

    let mut audio = packet_pts(&output, "a:0");
    audio.sort_by(|a, b| a.partial_cmp(b).expect("finite audio timestamps"));
    assert!(!audio.is_empty(), "the output must keep audio");
    let mut repeated = 0;
    for window in audio.windows(2) {
        let delta = window[1] - window[0];
        // A part can begin with a few leading audio packets from before the
        // video keyframe. The join keeps video cadence exact, so such packets
        // can overlap the previous part and the muxer repeats one of them.
        if delta.abs() < 1.0e-6 {
            repeated += 1;
            continue;
        }
        assert!(
            delta > 0.0,
            "audio timestamps must not go backwards: {} → {}",
            window[0],
            window[1]
        );
        assert!(
            delta < 0.06,
            "audio gap of {:.1}ms after {:.3}s",
            delta * 1000.0,
            window[0]
        );
    }
    assert!(
        repeated <= 1,
        "the join may repeat at most one AAC packet, got {repeated}"
    );

    let duration = duration_seconds(&output);
    assert!(
        (7.5..=8.5).contains(&duration),
        "expected about 8 seconds, got {duration}"
    );

    if let Err(error) = fs::remove_dir_all(&directory) {
        eprintln!("could not remove {}: {error}", directory.display());
    }
}
