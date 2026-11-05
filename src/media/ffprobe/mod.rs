//! FFprobe process integration.
//!
//! FFprobe is executed directly, without a shell, and its JSON output is
//! converted into the domain model before it leaves this module.

mod keyframes;
mod packets;
mod parse;
mod raw;
mod seek;

pub use keyframes::probe_keyframes;
pub use packets::{
    PacketBounds, probe_average_bit_rate, probe_frame_duration, probe_packet_bounds,
};
pub use seek::probe_seek_start;

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::domain::media::MediaFile;

/// Errors produced while probing a media file.
#[derive(Debug)]
pub enum ProbeError {
    /// The FFprobe executable could not be started.
    Spawn {
        /// Media file that was being probed.
        path: PathBuf,
        /// Operating-system error from process creation.
        source: std::io::Error,
    },
    /// FFprobe exited with a non-success status.
    Failed {
        /// Media file that was being probed.
        path: PathBuf,
        /// Human-readable exit status.
        status: String,
        /// Captured standard error, possibly empty.
        stderr: String,
    },
    /// FFprobe wrote output that is not valid UTF-8.
    OutputNotUtf8 {
        /// Media file that was being probed.
        path: PathBuf,
        /// UTF-8 conversion failure.
        source: std::string::FromUtf8Error,
    },
    /// FFprobe output was not valid JSON.
    Json {
        /// Media file that was being probed.
        path: PathBuf,
        /// JSON parser failure.
        source: serde_json::Error,
    },
    /// FFprobe output was valid JSON but did not match the expected model.
    InvalidData {
        /// Media file that was being probed.
        path: PathBuf,
        /// Which field or invariant failed.
        detail: String,
    },
}

impl ProbeError {
    pub(super) fn json(path: &Path, source: serde_json::Error) -> Self {
        Self::Json {
            path: path.to_path_buf(),
            source,
        }
    }

    pub(super) fn invalid_data(path: &Path, detail: impl Into<String>) -> Self {
        Self::InvalidData {
            path: path.to_path_buf(),
            detail: detail.into(),
        }
    }
}

impl fmt::Display for ProbeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn { path, source } => {
                write!(
                    formatter,
                    "failed to start ffprobe for {}: {source}",
                    path.display()
                )
            }
            Self::Failed {
                path,
                status,
                stderr,
            } => {
                if stderr.is_empty() {
                    write!(
                        formatter,
                        "ffprobe failed for {} ({status}) without stderr output",
                        path.display()
                    )
                } else {
                    write!(
                        formatter,
                        "ffprobe failed for {} ({status}): {stderr}",
                        path.display()
                    )
                }
            }
            Self::OutputNotUtf8 { path, source } => {
                write!(
                    formatter,
                    "ffprobe produced non-UTF-8 output for {}: {source}",
                    path.display()
                )
            }
            Self::Json { path, source } => {
                write!(
                    formatter,
                    "failed to parse ffprobe JSON for {}: {source}",
                    path.display()
                )
            }
            Self::InvalidData { path, detail } => {
                write!(
                    formatter,
                    "invalid ffprobe data for {}: {detail}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for ProbeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Spawn { source, .. } => Some(source),
            Self::OutputNotUtf8 { source, .. } => Some(source),
            Self::Json { source, .. } => Some(source),
            Self::Failed { .. } | Self::InvalidData { .. } => None,
        }
    }
}

/// Runs FFprobe on `path` and returns the parsed media model.
///
/// The process is spawned directly with separate arguments; no shell is
/// involved. FFprobe's standard error is preserved in the returned error when
/// the process fails.
pub fn probe(path: &Path) -> Result<MediaFile, ProbeError> {
    let output = command_for(path)
        .output()
        .map_err(|source| ProbeError::Spawn {
            path: path.to_path_buf(),
            source,
        })?;

    if !output.status.success() {
        return Err(ProbeError::Failed {
            path: path.to_path_buf(),
            status: output.status.to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }

    let stdout = String::from_utf8(output.stdout).map_err(|source| ProbeError::OutputNotUtf8 {
        path: path.to_path_buf(),
        source,
    })?;

    parse::parse_output(&stdout, path)
}

fn command_for(path: &Path) -> Command {
    let mut command = Command::new("ffprobe");
    command
        .arg("-v")
        .arg("error")
        .arg("-print_format")
        .arg("json")
        .arg("-show_format")
        .arg("-show_streams")
        .arg("-show_chapters")
        .arg(path);
    command
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::*;

    #[test]
    fn probe_command_passes_each_argument_separately() {
        let command = command_for(Path::new("input file.mp4"));

        assert_eq!(command.get_program(), OsStr::new("ffprobe"));
        let arguments: Vec<&OsStr> = command.get_args().collect();
        assert_eq!(
            arguments,
            vec![
                OsStr::new("-v"),
                OsStr::new("error"),
                OsStr::new("-print_format"),
                OsStr::new("json"),
                OsStr::new("-show_format"),
                OsStr::new("-show_streams"),
                OsStr::new("-show_chapters"),
                OsStr::new("input file.mp4"),
            ]
        );
    }
}
