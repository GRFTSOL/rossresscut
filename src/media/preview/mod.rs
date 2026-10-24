//! Preview backends built on external FFmpeg processes.
//!
//! Video frames are decoded by `ffmpeg` into RGB24 and delivered through a
//! bounded channel. Audio is played by `ffplay`. Neither process is spawned
//! through a shell.

mod audio;
mod video;

use std::fmt;
use std::path::{Path, PathBuf};

pub use audio::{PreviewAudio, PreviewAudioStatus};
pub use video::{PreviewVideo, PreviewVideoConfig, PreviewVideoResult, VideoFrame};

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
        }
    }
}

impl std::error::Error for PreviewError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Spawn { source, .. } | Self::Worker { source, .. } => Some(source),
            Self::InvalidInput { .. } => None,
        }
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
