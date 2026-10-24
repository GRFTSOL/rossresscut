//! Integration tests for preview decoding and application playback.

use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use rossresscut::application::{Application, OpenState};
use rossresscut::domain::time::Timestamp;
use rossresscut::media::preview::{
    HardwareAcceleration, PreviewVideo, PreviewVideoConfig, PreviewVideoResult, VideoFrame,
};

const FIXTURE_WIDTH: u32 = 160;
const FIXTURE_HEIGHT: u32 = 120;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn start_preview(start: Timestamp) -> PreviewVideo {
    PreviewVideo::start(PreviewVideoConfig {
        path: fixture("sample-video-only.mp4"),
        width: FIXTURE_WIDTH,
        height: FIXTURE_HEIGHT,
        start,
        frame_rate: 30,
        fast_seek: false,
        hardware: HardwareAcceleration::None,
    })
    .unwrap_or_else(|error| panic!("failed to start the preview decoder: {error}"))
}

fn wait_for_frame(video: &mut PreviewVideo) -> VideoFrame {
    let deadline = Instant::now() + Duration::from_secs(10);

    loop {
        match video.next_result() {
            PreviewVideoResult::Frame(frame) => return frame,
            PreviewVideoResult::Pending => {
                assert!(
                    Instant::now() < deadline,
                    "preview decoder did not produce a frame before the deadline"
                );
                thread::sleep(Duration::from_millis(10));
            }
            PreviewVideoResult::Ended { error } => {
                panic!("preview decoder ended before producing a frame: {error:?}");
            }
        }
    }
}

fn wait_until_ready(application: &mut Application) {
    let deadline = Instant::now() + Duration::from_secs(10);

    while Instant::now() < deadline {
        application.update();
        if matches!(application.state(), OpenState::Ready) {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }

    panic!("media did not become ready: {:?}", application.state());
}

#[test]
fn decodes_a_frame_at_the_start() {
    let mut video = start_preview(Timestamp::ZERO);
    let frame = wait_for_frame(&mut video);

    assert_eq!(frame.width, FIXTURE_WIDTH);
    assert_eq!(frame.height, FIXTURE_HEIGHT);
    assert_eq!(
        frame.rgb.len(),
        FIXTURE_WIDTH as usize * FIXTURE_HEIGHT as usize * 3
    );
    assert_eq!(frame.timestamp, Timestamp::ZERO);
}

#[test]
fn decodes_from_a_seek_position() {
    let start = Timestamp::from_micros(500_000);
    let mut video = start_preview(start);
    let frame = wait_for_frame(&mut video);

    assert_eq!(frame.timestamp, start);
}

#[test]
fn reports_decoder_failures() {
    let mut video = PreviewVideo::start(PreviewVideoConfig {
        path: fixture("not-media.bin"),
        width: FIXTURE_WIDTH,
        height: FIXTURE_HEIGHT,
        start: Timestamp::ZERO,
        frame_rate: 30,
        fast_seek: false,
        hardware: HardwareAcceleration::None,
    })
    .unwrap_or_else(|error| panic!("failed to start the preview decoder: {error}"));

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match video.next_result() {
            PreviewVideoResult::Frame(_) => panic!("a non-media fixture produced a frame"),
            PreviewVideoResult::Pending => {
                assert!(
                    Instant::now() < deadline,
                    "preview decoder did not report its failure"
                );
                thread::sleep(Duration::from_millis(10));
            }
            PreviewVideoResult::Ended { error } => {
                let error = error.unwrap_or_else(|| panic!("expected a decoder error"));
                assert!(error.contains("ffmpeg"), "unexpected error: {error}");
                break;
            }
        }
    }
}

#[test]
fn application_scrubs_with_a_fast_preview() {
    let mut application = Application::new();
    application.open_media(fixture("sample-video-only.mp4"));
    wait_until_ready(&mut application);

    let target = Timestamp::from_micros(1_000_000);
    application.scrub(target);
    assert_eq!(application.position(), target);

    let deadline = Instant::now() + Duration::from_secs(5);
    while application.is_waiting_for_preview() && Instant::now() < deadline {
        application.update();
        thread::sleep(Duration::from_millis(10));
    }

    assert!(
        !application.is_waiting_for_preview(),
        "scrub preview did not finish"
    );
    assert!(
        application.frame().is_some(),
        "scrub did not produce a frame"
    );
    assert!(!application.is_playing());
}

#[test]
fn application_steps_between_keyframes() {
    let mut application = Application::new();
    application.open_media(fixture("sample-keyframes.mp4"));
    wait_until_ready(&mut application);

    let deadline = Instant::now() + Duration::from_secs(10);
    while application.keyframes().is_none() && Instant::now() < deadline {
        application.update();
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        application.keyframes().is_some(),
        "keyframe index did not become available"
    );

    application.step_keyframe(true);
    assert_eq!(application.position(), Timestamp::from_micros(1_000_000));

    application.step_keyframe(true);
    assert_eq!(application.position(), Timestamp::from_micros(2_000_000));

    application.step_keyframe(false);
    assert_eq!(application.position(), Timestamp::from_micros(1_000_000));
}

#[test]
fn application_plays_pauses_and_seeks_video() {
    let mut application = Application::new();
    application.open_media(fixture("sample-video-only.mp4"));
    wait_until_ready(&mut application);

    assert!(!application.is_playing());

    application.play();
    assert!(application.is_playing());

    let deadline = Instant::now() + Duration::from_secs(5);
    while application.position() == Timestamp::ZERO && Instant::now() < deadline {
        application.update();
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        application.position().micros() > 0,
        "playback clock did not advance"
    );

    application.pause();
    assert!(!application.is_playing());
    let paused = application.position();
    thread::sleep(Duration::from_millis(50));
    application.update();
    assert_eq!(application.position(), paused, "paused clock advanced");

    application.seek(Timestamp::from_micros(500_000));
    assert_eq!(application.position(), Timestamp::from_micros(500_000));

    let deadline = Instant::now() + Duration::from_secs(5);
    while application.is_waiting_for_preview() && Instant::now() < deadline {
        application.update();
        thread::sleep(Duration::from_millis(10));
    }

    assert!(
        !application.is_waiting_for_preview(),
        "seek preview did not finish"
    );
    assert!(
        application.frame().is_some(),
        "seek did not produce a frame"
    );
}
