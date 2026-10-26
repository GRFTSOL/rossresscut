//! Export job state, command errors, and the single/multi process wrapper.

use std::fmt;
use std::io;
use std::path::PathBuf;
use std::process::ExitStatus;

use crate::domain::time::{Duration, Timestamp};
pub use crate::media::export::ExportStage;

use crate::media::export::{
    ExportError, ExportEvent, ExportProcess, MultiExport, MultiExportError, MultiExportEvent,
};

/// State of the current export, if any.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum ExportStatus {
    /// No export has run in this session.
    #[default]
    Idle,
    /// An export is running.
    Running {
        /// Requested output path.
        output: PathBuf,
        /// Last output timestamp reported by FFmpeg.
        out_time: Timestamp,
        /// Total requested length.
        duration: Duration,
        /// Multi-segment stage, or `None` for a single segment.
        stage: Option<ExportStage>,
    },
    /// The export finished successfully.
    Completed {
        /// Written output path.
        output: PathBuf,
    },
    /// The export failed.
    Failed {
        /// Requested output path.
        output: PathBuf,
        /// User-facing failure description.
        message: String,
    },
    /// The user cancelled the export.
    Cancelled,
}

impl ExportStatus {
    /// Returns the progress in `0.0..=1.0` while an export is running.
    pub fn progress(&self) -> Option<f32> {
        match self {
            Self::Running {
                out_time, duration, ..
            } => {
                let total = duration.micros();
                if total == 0 {
                    return Some(0.0);
                }
                let done = u64::try_from(out_time.micros()).unwrap_or(0);
                Some((done as f32 / total as f32).clamp(0.0, 1.0))
            }
            _ => None,
        }
    }

    /// Returns whether an export is currently running.
    pub fn is_running(&self) -> bool {
        matches!(self, Self::Running { .. })
    }
}

/// Errors produced by export commands.
#[derive(Debug)]
pub enum ExportCommandError {
    /// No media is open.
    NoMedia,
    /// The timeline has no enabled segment.
    NoSegment,
    /// Another export is already running.
    AlreadyRunning,
    /// The output path is invalid or equals the source path.
    InvalidOutput,
    /// The FFmpeg process could not be started.
    Start(ExportError),
    /// A multi-segment export could not be started.
    StartMulti(MultiExportError),
}

impl fmt::Display for ExportCommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoMedia => formatter.write_str("open a media file before exporting"),
            Self::NoSegment => formatter.write_str("add an enabled segment before exporting"),
            Self::AlreadyRunning => formatter.write_str("an export is already running"),
            Self::InvalidOutput => {
                formatter.write_str("the output path must differ from the source file")
            }
            Self::Start(error) => write!(formatter, "{error}"),
            Self::StartMulti(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for ExportCommandError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Start(error) => Some(error),
            Self::StartMulti(error) => Some(error),
            _ => None,
        }
    }
}

/// A running export and its progress bookkeeping.
pub(crate) struct ExportJob {
    pub(crate) output: PathBuf,
    pub(crate) duration: Duration,
    pub(crate) out_time: Timestamp,
    pub(crate) cancel_requested: bool,
    process: ExportProcessKind,
}

pub(crate) enum ExportProcessKind {
    Single(ExportProcess),
    Multi(MultiExport),
}

/// Normalised event from either export path.
pub(crate) enum JobEvent {
    Progress {
        out_time: Timestamp,
        duration: Duration,
        stage: Option<ExportStage>,
    },
    Finished {
        error: Option<String>,
    },
}

impl ExportJob {
    /// Creates a job for one segment.
    pub(crate) fn single(output: PathBuf, duration: Duration, process: ExportProcess) -> Self {
        Self {
            output,
            duration,
            out_time: Timestamp::ZERO,
            cancel_requested: false,
            process: ExportProcessKind::Single(process),
        }
    }

    /// Creates a job for multiple segments.
    pub(crate) fn multi(output: PathBuf, duration: Duration, process: MultiExport) -> Self {
        Self {
            output,
            duration,
            out_time: Timestamp::ZERO,
            cancel_requested: false,
            process: ExportProcessKind::Multi(process),
        }
    }

    /// Polls the underlying process without blocking.
    pub(crate) fn poll(&mut self) -> Option<JobEvent> {
        let duration = self.duration;
        match &mut self.process {
            ExportProcessKind::Single(process) => match process.poll()? {
                ExportEvent::Progress { out_time, .. } => Some(JobEvent::Progress {
                    out_time,
                    duration,
                    stage: None,
                }),
                ExportEvent::Finished { stderr, ended } => {
                    let exit = process.wait();
                    Some(JobEvent::Finished {
                        error: single_error(stderr, ended, exit),
                    })
                }
            },
            ExportProcessKind::Multi(process) => match process.poll()? {
                MultiExportEvent::Progress {
                    stage,
                    out_time,
                    duration,
                } => Some(JobEvent::Progress {
                    out_time,
                    duration,
                    stage: Some(stage),
                }),
                MultiExportEvent::Finished { error } => Some(JobEvent::Finished { error }),
            },
        }
    }

    /// Requests cancellation.
    pub(crate) fn cancel(&mut self) {
        match &mut self.process {
            ExportProcessKind::Single(process) => process.cancel(),
            ExportProcessKind::Multi(process) => process.cancel(),
        }
    }
}

fn single_error(stderr: String, ended: bool, exit: io::Result<ExitStatus>) -> Option<String> {
    match exit {
        Ok(status) if ended && status.success() => None,
        Ok(status) => {
            let details = stderr.trim();
            Some(if details.is_empty() {
                format!("ffmpeg exited with {status}")
            } else {
                format!("ffmpeg exited with {status}: {details}")
            })
        }
        Err(error) => Some(format!("could not wait for ffmpeg: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_is_clamped_to_the_requested_duration() {
        let status = ExportStatus::Running {
            output: PathBuf::from("out.mkv"),
            out_time: Timestamp::from_micros(2_000_000),
            duration: Duration::from_micros(4_000_000),
            stage: None,
        };
        assert_eq!(status.progress(), Some(0.5));

        let status = ExportStatus::Running {
            output: PathBuf::from("out.mkv"),
            out_time: Timestamp::from_micros(9_000_000),
            duration: Duration::from_micros(4_000_000),
            stage: Some(ExportStage::Concatenating),
        };
        assert_eq!(status.progress(), Some(1.0));
    }

    #[test]
    fn idle_status_has_no_progress() {
        assert_eq!(ExportStatus::Idle.progress(), None);
        assert!(!ExportStatus::Idle.is_running());
    }
}
