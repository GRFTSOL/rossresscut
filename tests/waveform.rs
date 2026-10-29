//! Integration tests for waveform extraction.
//!
//! These tests run FFmpeg against the synthetic fixtures, so they exercise the
//! real process pipeline instead of a mock.

use std::path::Path;

use rossresscut::media::waveform;

#[test]
fn extracts_peaks_from_a_real_audio_stream() {
    let waveform = waveform::extract(Path::new("tests/fixtures/sample.mp4"), 1).expect("waveform");

    assert!(!waveform.is_empty());
    let micros = waveform.duration().micros();
    assert!(
        (900_000..=1_200_000).contains(&micros),
        "unexpected waveform duration: {micros} us"
    );

    let columns = waveform.columns(200);
    assert_eq!(columns.len(), 200);
    assert!(
        columns.iter().any(|peak| peak.amplitude() > 0.1),
        "a 440 Hz tone should produce visible peaks"
    );
}

#[test]
fn reports_a_stream_without_audio() {
    let error = waveform::extract(Path::new("tests/fixtures/sample-keyframes.mp4"), 0)
        .expect_err("video-only media has no waveform");

    let message = error.to_string();
    assert!(
        message.contains("no audio samples") || message.contains("ffmpeg failed"),
        "unexpected error: {message}"
    );
}

#[test]
fn reports_a_missing_file() {
    let error = waveform::extract(Path::new("tests/fixtures/does-not-exist.mp4"), 1)
        .expect_err("missing files cannot be scanned");

    assert!(
        error.to_string().contains("ffmpeg failed"),
        "unexpected error: {error}"
    );
}
