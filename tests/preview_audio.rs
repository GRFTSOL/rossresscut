//! Manual audio preview tests.
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
fn plays_audio_through_system_output() {
    let mut audio = PreviewAudio::start(&fixture("sample.mp4"), Timestamp::ZERO)
        .unwrap_or_else(|error| panic!("failed to start audio preview: {error}"));

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match audio.check() {
            PreviewAudioStatus::Running => {
                assert!(
                    Instant::now() < deadline,
                    "audio preview did not end before the deadline"
                );
                thread::sleep(Duration::from_millis(10));
            }
            PreviewAudioStatus::Ended { success, stderr } => {
                assert!(success, "audio preview failed: {stderr}");
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
    assert!(application.uses_audio_clock(), "audio clock was not active");
    let position = application.position();
    assert!(
        position.micros() > 0,
        "audio clock did not advance: {position}"
    );

    if let Some(frame_timestamp) = application.frame_timestamp() {
        let lag = position.micros() - frame_timestamp.micros();
        assert!(
            (0..250_000).contains(&lag),
            "preview frame is out of sync: frame={frame_timestamp}, position={position}"
        );
    }

    application.pause();
}
