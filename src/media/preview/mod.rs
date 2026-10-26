//! Preview backends.
//!
//! The primary backend is `mpv-engine`'s exported-frame path (feature
//! `mpv-preview`): libmpv decodes and renders, and the project imports the
//! resulting GPU texture. The external FFmpeg video/audio path remains as a
//! fallback and is still used until the mpv path is wired into the UI.
//!
//! No process is spawned through a shell. This module is in the Media layer
//! and must not depend on `eframe` or `egui`.

mod audio;
#[cfg(feature = "mpv-preview")]
mod mpv;
mod video;

use std::fmt;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr};

pub use audio::{PreviewAudio, PreviewAudioStatus};
#[cfg(feature = "mpv-preview")]
pub use mpv::{MpvPreview, MpvPreviewConfig, MpvPreviewError, MpvPreviewEvents};
#[cfg(feature = "mpv-preview")]
pub(crate) use video::preview_dimensions;
pub use video::{
    HardwareAcceleration, PreviewVideo, PreviewVideoConfig, PreviewVideoResult, VideoFrame,
};

use crate::domain::time::Timestamp;

/// Errors produced by preview processes.
#[derive(Debug)]
pub enum PreviewError {
    /// A preview process could not be started.
    Spawn {
        /// Executable name.
        program: &'static str,
        /// Media file being previewed.
        path: PathBuf,
        /// Operating-system error from process creation.
        source: std::io::Error,
    },
    /// A preview worker thread could not be started.
    Worker {
        /// Executable name.
        program: &'static str,
        /// Media file being previewed.
        path: PathBuf,
        /// Operating-system error from thread creation.
        source: std::io::Error,
    },
    /// Preview input cannot be decoded with the requested parameters.
    InvalidInput {
        /// Media file being previewed.
        path: PathBuf,
        /// What is unsupported or invalid.
        detail: String,
    },
    /// Audio output could not be initialized.
    AudioDevice {
        /// Description of the audio device failure.
        detail: String,
    },
}

impl PreviewError {
    fn spawn(program: &'static str, path: &Path, source: std::io::Error) -> Self {
        Self::Spawn {
            program,
            path: path.to_path_buf(),
            source,
        }
    }

    fn worker(program: &'static str, path: &Path, source: std::io::Error) -> Self {
        Self::Worker {
            program,
            path: path.to_path_buf(),
            source,
        }
    }

    fn invalid_input(path: &Path, detail: impl Into<String>) -> Self {
        Self::InvalidInput {
            path: path.to_path_buf(),
            detail: detail.into(),
        }
    }

    fn audio_device(detail: impl Into<String>) -> Self {
        Self::AudioDevice {
            detail: detail.into(),
        }
    }
}

impl fmt::Display for PreviewError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn {
                program,
                path,
                source,
            } => write!(
                formatter,
                "failed to start {program} for {}: {source}",
                path.display()
            ),
            Self::Worker {
                program,
                path,
                source,
            } => write!(
                formatter,
                "failed to start the {program} preview worker for {}: {source}",
                path.display()
            ),
            Self::InvalidInput { path, detail } => {
                write!(
                    formatter,
                    "cannot preview {} with the requested parameters: {detail}",
                    path.display()
                )
            }
            Self::AudioDevice { detail } => {
                write!(formatter, "could not initialize audio preview: {detail}")
            }
        }
    }
}

impl std::error::Error for PreviewError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Spawn { source, .. } | Self::Worker { source, .. } => Some(source),
            Self::InvalidInput { .. } | Self::AudioDevice { .. } => None,
        }
    }
}

fn terminate_child(child: &mut Child) {
    if let Err(error) = child.kill()
        && error.kind() != io::ErrorKind::InvalidInput
    {
        log::warn!("failed to stop preview process: {error}");
    }
    if let Err(error) = child.wait() {
        log::warn!("failed to reap preview process: {error}");
    }
}

fn read_stderr(stderr: &mut ChildStderr) -> String {
    let mut bytes = Vec::new();
    match stderr.read_to_end(&mut bytes) {
        Ok(_) => String::from_utf8_lossy(&bytes).trim().to_owned(),
        Err(error) => {
            let text = String::from_utf8_lossy(&bytes).trim().to_owned();
            if text.is_empty() {
                format!("failed to read preview stderr: {error}")
            } else {
                format!("{text}\nfailed to read preview stderr: {error}")
            }
        }
    }
}

fn join_detail(detail: &str, stderr: &str) -> String {
    if stderr.is_empty() {
        detail.to_owned()
    } else {
        format!("{detail}: {stderr}")
    }
}

fn seconds_arg(timestamp: Timestamp) -> String {
    let micros = timestamp.micros();
    let sign = if micros.is_negative() { "-" } else { "" };
    let magnitude = micros.unsigned_abs();
    format!(
        "{sign}{}.{:06}",
        magnitude / 1_000_000,
        magnitude % 1_000_000
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_timestamps_for_ffmpeg_arguments() {
        assert_eq!(seconds_arg(Timestamp::ZERO), "0.000000");
        assert_eq!(seconds_arg(Timestamp::from_micros(1_500_000)), "1.500000");
        assert_eq!(seconds_arg(Timestamp::from_micros(-250_000)), "-0.250000");
    }
}
