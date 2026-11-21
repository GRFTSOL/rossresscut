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

/// Six one-second bands encoded with B-frames and a one-second GOP.
fn generate_b_frame_fixture(path: &Path) {
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
            "veryfast",
            "-crf",
            "28",
            "-bf",
            "3",
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
        .expect("run ffmpeg to generate the B-frame fixture");
    assert!(status.success(), "ffmpeg could not generate the fixture");
}

fn generate_vfr_fixture(path: &Path) {
    // 120 fps source reduced to 120/60/40 fps sections without resampling
    // timestamps, so the fixture has genuinely variable frame durations.
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
            "testsrc2=size=640x360:rate=120:duration=12",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000:duration=12",
            "-vf",
            "select='lt(t,4)+between(t,4,8)*not(mod(n,2))+gt(t,8)*not(mod(n,3))'",
            "-fps_mode",
            "vfr",
            "-c:v",
            "libx264",
            "-preset",
            "veryfast",
            "-bf",
            "2",
            "-g",
            "30",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-b:a",
            "128k",
        ])
        .arg(path)
        .status()
        .expect("run ffmpeg to generate the VFR fixture");
    assert!(
        status.success(),
        "ffmpeg could not generate the VFR fixture"
    );
}

fn generate_hdr_fixture(path: &Path) {
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
            "testsrc2=size=320x180:rate=10:duration=4",
            "-vf",
            "format=yuv420p10le",
            "-c:v",
            "libx265",
            "-preset",
            "ultrafast",
            "-x265-params",
            "keyint=10:min-keyint=10:scenecut=0:colorprim=bt2020:transfer=smpte2084:colormatrix=bt2020nc:master-display=G(13250,34500)B(7500,3000)R(34000,16000)WP(15635,16450)L(10000000,1):max-cll=1000,400",
            "-pix_fmt",
            "yuv420p10le",
            "-color_range",
            "tv",
            "-color_primaries",
            "bt2020",
            "-color_trc",
            "smpte2084",
            "-colorspace",
            "bt2020nc",
        ])
        .arg(path)
        .status()
        .expect("run ffmpeg to generate the HDR fixture");
    assert!(
        status.success(),
        "ffmpeg could not generate the HDR fixture"
    );
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

fn probe_video_color(path: &Path) -> String {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=pix_fmt,color_range,color_space,color_transfer,color_primaries",
            "-of",
            "default=nw=1",
        ])
        .arg(path)
        .output()
        .expect("run ffprobe for color metadata");
    assert!(output.status.success(), "ffprobe failed");
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Sorted presentation timestamps for one stream selector.
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
    assert!(output.status.success(), "ffprobe failed");
    let mut values: Vec<f64> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().split(',').next()?.parse().ok())
        .collect();
    values.sort_by(|a: &f64, b: &f64| a.partial_cmp(b).expect("finite timestamps"));
    values
}

/// Sorted video presentation timestamps.
fn video_pts(path: &Path) -> Vec<f64> {
    packet_pts(path, "v:0")
}

fn probe_frame_side_data(path: &Path) -> String {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-read_intervals",
            "%+#1",
            "-show_frames",
            "-of",
            "json",
        ])
        .arg(path)
        .output()
        .expect("run ffprobe for side data");
    assert!(output.status.success(), "ffprobe failed");
    String::from_utf8_lossy(&output.stdout).into_owned()
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
fn smart_cut_preserves_10_bit_hdr_metadata() {
    let directory = fixture_directory();
    let source = directory.join("hdr-source.mp4");
    let output = directory.join("hdr-smart.mp4");
    generate_hdr_fixture(&source);

    let source_color = probe_video_color(&source);
    assert!(
        source_color.contains("yuv420p10le"),
        "fixture is not 10-bit: {source_color}"
    );

    let mut application = open_with_keyframes(&source);
    // 1.5s is inside the first GOP; the next keyframe is at 2.0s.
    add_segment(&mut application, 1_500_000, 2_500_000);
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

    let output_color = probe_video_color(&output);
    for expected in ["yuv420p10le", "bt2020", "bt2020nc", "smpte2084", "tv"] {
        assert!(
            output_color.contains(expected),
            "smart cut lost {expected}: {output_color}"
        );
    }
    let side_data = probe_frame_side_data(&output);
    for expected in ["Mastering display metadata", "Content light level metadata"] {
        assert!(
            side_data.contains(expected),
            "smart cut lost HDR10 {expected}: {side_data}"
        );
    }
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

#[test]
fn smart_cut_ends_on_the_requested_frame_without_a_gap() {
    let directory = fixture_directory();
    let source = directory.join("b-frames.mp4");
    let output = directory.join("b-frames-smart.mp4");
    generate_b_frame_fixture(&source);

    let mut application = open_with_keyframes(&source);
    // 1.5s is in the green band and 3.5s in the red band. Keyframes sit every
    // second, so the segment needs an encoded head, a copied middle and an
    // encoded tail.
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

    let pts = video_pts(&output);
    assert!(pts.len() > 40, "unexpected frame count: {}", pts.len());
    for window in pts.windows(2) {
        let delta = window[1] - window[0];
        assert!(
            delta > 0.0,
            "video timestamps must increase: {} → {}",
            window[0],
            window[1]
        );
        // 30fps frames are 33.3ms; a dropped B-frame group used to leave a
        // 100ms+ hold at the segment end.
        assert!(
            delta < 0.05,
            "frame gap of {:.1}ms after {:.3}s",
            delta * 1000.0,
            window[0]
        );
    }

    let (red, green, blue) = centre_pixel(&output, "1.93");
    assert!(
        red > 100 && red > green && red > blue,
        "the end of the segment should be red, got ({red}, {green}, {blue})"
    );
    let duration = probe_duration(&output);
    assert!(
        (duration - 2.0).abs() < 0.12,
        "expected about two seconds, got {duration}"
    );
}

#[test]
fn smart_cut_uses_the_fast_preset_and_manual_bit_rate() {
    let directory = fixture_directory();
    let source = directory.join("source.mp4");
    let output = directory.join("preset.mp4");
    generate_fixture(&source);

    let mut application = open_with_keyframes(&source);
    add_segment(&mut application, 1_500_000, 3_500_000);
    application
        .start_export(
            output.clone(),
            ExportOptions {
                smart_cut: true,
                smart_cut_bit_rate_kbps: 2_500,
                ..Default::default()
            },
        )
        .expect("smart cut starts");
    wait_for_export(&mut application);
    assert!(
        matches!(application.export_status(), ExportStatus::Completed { .. }),
        "smart cut export failed: {:?}",
        application.export_status()
    );

    let source_text = source.to_string_lossy();
    let encoded = application
        .executed_ffmpeg_command_lines()
        .into_iter()
        .find(|command| command.contains(source_text.as_ref()) && command.contains("-c:v"))
        .expect("an encoded piece");
    assert!(
        encoded.contains("-preset faster"),
        "unexpected preset: {encoded}"
    );
    assert!(
        encoded.contains("-b:v 2500000"),
        "manual bit rate was not applied: {encoded}"
    );

    if let Err(error) = fs::remove_dir_all(&directory) {
        eprintln!("could not remove {}: {error}", directory.display());
    }
}

#[test]
fn smart_cut_preserves_vfr_timestamps_and_audio_cadence() {
    let directory = fixture_directory();
    let source = directory.join("vfr.mp4");
    let output = directory.join("vfr-smart.mkv");
    generate_vfr_fixture(&source);

    let source_pts = video_pts(&source);
    let source_deltas: std::collections::BTreeSet<i64> = source_pts
        .windows(2)
        .map(|window| ((window[1] - window[0]) * 1_000_000.0).round() as i64)
        .collect();
    assert!(
        source_deltas.len() >= 2,
        "the fixture must be VFR: {source_deltas:?}"
    );

    let mut application = open_with_keyframes(&source);
    // The first segment is in the 60 fps section and the second in the 40 fps
    // section; neither boundary is a keyframe.
    add_segment(&mut application, 4_100_000, 7_200_000);
    add_segment(&mut application, 8_100_000, 10_200_000);
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
    assert!(
        matches!(application.export_status(), ExportStatus::Completed { .. }),
        "smart cut export failed: {:?}",
        application.export_status()
    );

    let video = video_pts(&output);
    assert!(video.len() > 200, "unexpected frame count: {}", video.len());
    let mut largest_video = 0.0_f64;
    for window in video.windows(2) {
        let delta = window[1] - window[0];
        assert!(
            delta > 0.0,
            "duplicate or reversed frame at {} → {}",
            window[0],
            window[1]
        );
        largest_video = largest_video.max(delta);
    }
    // The slowest source section is 40 fps (25 ms).
    assert!(
        largest_video < 0.030,
        "video gap of {:.1}ms is a stall",
        largest_video * 1000.0
    );

    let audio = packet_pts(&output, "a:0");
    assert!(!audio.is_empty(), "the output must keep audio");
    let mut largest_audio = 0.0_f64;
    let mut repeated = 0;
    for window in audio.windows(2) {
        let delta = window[1] - window[0];
        if delta.abs() < 1.0e-6 {
            repeated += 1;
            continue;
        }
        assert!(
            delta > 0.0,
            "audio must not go backwards: {} → {}",
            window[0],
            window[1]
        );
        largest_audio = largest_audio.max(delta);
    }
    // AAC packets are 21.3ms. At a copy/encoded seam the packet grid can leave
    // up to about two packets of silence; video cadence remains the strict
    // check. B-frame VFR measured 72ms, no-B-frame VFR measured 27ms.
    assert!(
        largest_audio < 0.080,
        "audio gap of {:.1}ms at a smart-cut seam",
        largest_audio * 1000.0
    );

    println!(
        "VFR smart cut: {} video packets, max video gap {:.1}ms, max audio gap {:.1}ms, repeated audio packets {repeated}",
        video.len(),
        largest_video * 1000.0,
        largest_audio * 1000.0
    );

    if let Err(error) = fs::remove_dir_all(&directory) {
        eprintln!("could not remove {}: {error}", directory.display());
    }
}

#[test]
fn smart_cut_keeps_graphical_subtitles_in_range() {
    let source = PathBuf::from("tests/fixtures/dvb_subtitles.mkv");
    let directory = fixture_directory();
    let output = directory.join("dvb-smart.mkv");

    let mut application = open_with_keyframes(&source);
    // 0.2s starts mid-GOP and 2.8s forces an encoded head, a copied middle
    // and an encoded tail, so subtitle packets cross both smart-cut seams.
    add_segment(&mut application, 200_000, 2_800_000);
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
    assert!(
        matches!(application.export_status(), ExportStatus::Completed { .. }),
        "smart cut export failed: {:?}",
        application.export_status()
    );

    let codec = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "s:0",
            "-show_entries",
            "stream=codec_name",
            "-of",
            "csv=p=0",
        ])
        .arg(&output)
        .output()
        .expect("run ffprobe for the subtitle codec");
    assert_eq!(
        String::from_utf8_lossy(&codec.stdout).trim(),
        "dvb_subtitle",
        "the graphical subtitle stream must be kept"
    );

    let duration = probe_duration(&output);
    let subtitles = packet_pts(&output, "s:0");
    assert!(
        subtitles.len() >= 20,
        "unexpected subtitle packet count: {}",
        subtitles.len()
    );
    assert!(
        subtitles[0] >= 0.0,
        "subtitle timestamps must be rebased: {:?}",
        &subtitles[..subtitles.len().min(5)]
    );
    assert!(
        subtitles.last().copied().unwrap_or_default() <= duration + 0.05,
        "subtitle packets extend past the output: {duration}"
    );
    for window in subtitles.windows(2) {
        let delta = window[1] - window[0];
        assert!(
            delta > 0.0,
            "subtitle timestamps must increase: {} → {}",
            window[0],
            window[1]
        );
        assert!(
            delta < 0.2,
            "subtitle gap of {:.1}ms after {:.3}s",
            delta * 1000.0,
            window[0]
        );
    }

    let video = video_pts(&output);
    assert!(video.len() > 20, "unexpected frame count: {}", video.len());
    for window in video.windows(2) {
        assert!(
            window[1] > window[0],
            "video timestamps must increase: {} → {}",
            window[0],
            window[1]
        );
        // The fixture is 10 fps, so one frame is 100ms.
        assert!(
            window[1] - window[0] < 0.15,
            "video gap of {:.1}ms after {:.3}s",
            (window[1] - window[0]) * 1000.0,
            window[0]
        );
    }

    if let Err(error) = fs::remove_dir_all(&directory) {
        eprintln!("could not remove {}: {error}", directory.display());
    }
}
