//! Manual audio preview test.
//!
//! Ignored by default because CI runners do not provide an audio device.

use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use rossresscut::application::{Application, OpenState};
use rossresscut::domain::time::Timestamp;
use rossresscut::media::preview::{PreviewAudio, PreviewAudioStatus};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
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
#[ignore = "requires a working audio output device"]
fn plays_audio_through_ffplay() {
    let mut audio = PreviewAudio::start(&fixture("sample.mp4"), Timestamp::ZERO)
        .unwrap_or_else(|error| panic!("failed to start audio preview: {error}"));

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match audio.check() {
            PreviewAudioStatus::Running => {
                assert!(
                    Instant::now() < deadline,
                    "ffplay did not exit before the deadline"
                );
                thread::sleep(Duration::from_millis(10));
            }
            PreviewAudioStatus::Ended { success, stderr } => {
                assert!(success, "ffplay failed: {stderr}");
                break;
            }
        }
    }
}

#[test]
#[ignore = "requires a working audio output device"]
fn application_starts_audio_playback() {
    let mut application = Application::new();
    application.open_media(fixture("sample.mp4"));
    wait_until_ready(&mut application);

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
    application.pause();
}
