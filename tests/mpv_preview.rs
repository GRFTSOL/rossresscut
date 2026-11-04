//! Manual integration tests for the mpv preview backend.
//!
//! Ignored by default because they need libmpv, a GPU render backend and an
//! audio output device. Run with `cargo test --features mpv-preview -- --ignored`.

#![cfg(feature = "mpv-preview")]

use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use rossresscut::application::{Application, OpenState};
use rossresscut::domain::time::Timestamp;
use rossresscut::media::preview::{MpvPreview, MpvPreviewConfig};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn start(name: &str, start_paused: bool) -> MpvPreview {
    MpvPreview::start(
        MpvPreviewConfig {
            path: fixture(name),
            width: 160,
            height: 120,
            start_paused,
        },
        || {},
        || {},
    )
    .unwrap_or_else(|error| panic!("failed to start the mpv preview: {error}"))
}

/// Opens `name` with the mpv preview backend and waits until it is ready.
fn open_ready(name: &str) -> Application {
    let mut application = Application::new();
    application.enable_mpv_preview();
    application.open_media(fixture(name));

    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        application.update();
        if matches!(application.state(), OpenState::Ready) {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        matches!(application.state(), OpenState::Ready),
        "media did not become ready: {:?}",
        application.state()
    );
    assert!(
        application.uses_mpv_preview(),
        "the mpv preview backend did not start"
    );
    application
}

/// Polls the application until its position is within `tolerance` micros of
/// `expected`.
fn wait_for_position(application: &mut Application, expected: Timestamp, tolerance: u64) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        application.update();
        if application.position().micros().abs_diff(expected.micros()) <= tolerance {
            return true;
        }
        thread::sleep(Duration::from_millis(10));
    }
    false
}

/// Polls mpv until `predicate` is true or the deadline expires.
fn wait_for(mpv: &MpvPreview, deadline: Duration, mut predicate: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + deadline;
    while Instant::now() < end {
        let events = mpv.poll_events();
        assert!(
            events.failed.is_none(),
            "mpv reported a failure: {:?}",
            events.failed
        );
        if predicate() {
            return true;
        }
        thread::sleep(Duration::from_millis(10));
    }
    false
}

#[test]
#[ignore = "requires libmpv, a GPU render backend and an audio device"]
fn playback_advances_the_mpv_clock() {
    let mpv = start("sample.mp4", false);

    let advanced = wait_for(&mpv, Duration::from_secs(5), || {
        mpv.position()
            .is_some_and(|position| position.micros() >= 200_000)
    });

    assert!(
        advanced,
        "mpv position did not advance: {:?}",
        mpv.position()
    );
}

#[test]
#[ignore = "requires libmpv and a GPU render backend"]
fn fast_seek_lands_on_the_previous_keyframe() {
    let mpv = start("sample-keyframes.mp4", true);
    assert!(
        wait_for(&mpv, Duration::from_secs(5), || mpv.position().is_some()),
        "mpv did not report an initial position"
    );

    mpv.set_precise_seek(false)
        .unwrap_or_else(|error| panic!("failed to disable precise seeking: {error}"));
    mpv.seek(Timestamp::from_micros(2_500_000))
        .unwrap_or_else(|error| panic!("fast seek failed: {error}"));

    let landed = wait_for(&mpv, Duration::from_secs(5), || {
        mpv.position()
            .is_some_and(|position| (1_900_000..=2_200_000).contains(&position.micros()))
    });
    assert!(
        landed,
        "fast seek did not land on the 2 s keyframe: {:?}",
        mpv.position()
    );
}

#[test]
#[ignore = "requires libmpv and a GPU render backend"]
fn precise_seek_lands_on_the_requested_position() {
    let mpv = start("sample-keyframes.mp4", true);
    assert!(
        wait_for(&mpv, Duration::from_secs(5), || mpv.position().is_some()),
        "mpv did not report an initial position"
    );

    mpv.set_precise_seek(true)
        .unwrap_or_else(|error| panic!("failed to enable precise seeking: {error}"));
    mpv.seek(Timestamp::from_micros(2_500_000))
        .unwrap_or_else(|error| panic!("precise seek failed: {error}"));

    let landed = wait_for(&mpv, Duration::from_secs(5), || {
        mpv.position()
            .is_some_and(|position| (2_400_000..=2_600_000).contains(&position.micros()))
    });
    assert!(
        landed,
        "precise seek did not land near 2.5 s: {:?}",
        mpv.position()
    );
}

#[test]
#[ignore = "requires libmpv, a GPU render backend and an audio device"]
fn application_uses_the_mpv_audio_clock() {
    let mut application = open_ready("sample.mp4");

    application.play();
    assert!(application.is_playing());

    let deadline = Instant::now() + Duration::from_millis(700);
    while Instant::now() < deadline {
        application.update();
        thread::sleep(Duration::from_millis(10));
    }

    assert!(
        application.warning().is_none(),
        "application reported a preview warning: {:?}",
        application.warning()
    );
    assert!(
        application.uses_audio_clock(),
        "the mpv audio path was not reported as the clock source"
    );
    assert!(
        application.position().micros() > 0,
        "the mpv clock did not advance: {}",
        application.position()
    );
    application.pause();
}

#[test]
#[ignore = "requires libmpv, a GPU render backend and an audio device"]
fn rapid_scrub_with_audio_reaches_the_final_target() {
    let mut application = open_ready("sample.mkv");
    assert!(
        application.uses_audio_clock(),
        "the mpv audio path was not reported as the clock source"
    );
    application.play();

    let started = Instant::now();
    for index in 0..20 {
        let target = Timestamp::from_micros(200_000 + index * 70_000);
        application.scrub(target);
        application.update();
        thread::sleep(Duration::from_millis(50));
    }
    let scrub_elapsed = started.elapsed();

    let target = Timestamp::from_micros(1_500_000);
    application.seek(target);
    assert!(
        wait_for_position(&mut application, target, 100_000),
        "precise seek after scrubbing did not land near {target}: {}",
        application.position()
    );
    assert!(
        scrub_elapsed < Duration::from_secs(3),
        "scrubbing blocked the caller for {scrub_elapsed:?}"
    );
    application.pause();
}

#[test]
#[ignore = "requires libmpv, a GPU render backend and an audio device"]
fn frame_steps_use_source_frame_boundaries() {
    let mut application = open_ready("sample-keyframes.mp4");
    application.pause();

    let start = Timestamp::from_micros(500_000);
    application.seek(start);
    assert!(
        wait_for_position(&mut application, start, 50_000),
        "initial seek did not land near {start}: {}",
        application.position()
    );

    for expected in [600_000, 700_000, 800_000] {
        application.step_frame(true);
        let expected = Timestamp::from_micros(expected);
        assert!(
            wait_for_position(&mut application, expected, 30_000),
            "forward step did not land near {expected}: {}",
            application.position()
        );
    }

    for expected in [700_000, 600_000, 500_000] {
        application.step_frame(false);
        let expected = Timestamp::from_micros(expected);
        assert!(
            wait_for_position(&mut application, expected, 30_000),
            "backward step did not land near {expected}: {}",
            application.position()
        );
    }
}

#[test]
#[ignore = "requires libmpv and a GPU render backend"]
fn pause_and_resume_keep_the_position() {
    let mpv = start("sample-keyframes.mp4", false);
    assert!(
        wait_for(&mpv, Duration::from_secs(5), || {
            mpv.position()
                .is_some_and(|position| position.micros() >= 200_000)
        }),
        "mpv position did not advance"
    );

    mpv.set_paused(true)
        .unwrap_or_else(|error| panic!("failed to pause: {error}"));
    thread::sleep(Duration::from_millis(100));
    let paused = mpv
        .position()
        .unwrap_or_else(|| panic!("mpv lost its position while paused"));

    thread::sleep(Duration::from_millis(250));
    let still_paused = mpv
        .position()
        .unwrap_or_else(|| panic!("mpv lost its position while paused"));
    assert_eq!(
        paused, still_paused,
        "position advanced while paused: {paused} -> {still_paused}"
    );

    mpv.set_paused(false)
        .unwrap_or_else(|error| panic!("failed to resume: {error}"));
    let resumed = wait_for(&mpv, Duration::from_secs(5), || {
        mpv.position()
            .is_some_and(|position| position > still_paused)
    });
    assert!(resumed, "mpv did not resume from {still_paused}");
}
