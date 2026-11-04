//! Lossless export through an external FFmpeg process.
//!
//! The media layer owns argument construction, process startup, progress
//! parsing, and cancellation. The application layer owns export job state and
//! user-facing errors. No shell is involved and no file is re-encoded.

mod chapters;
mod command;
mod concat;
mod merge;
mod planning;
mod progress;
mod smart_cut;

use std::ffi::OsString;
use std::fmt;
use std::io::{self, BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::{Duration as StdDuration, Instant};

use crate::domain::time::Timestamp;

use chapters::TempMetadata;

use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

pub use chapters::ChapterSpec;
pub use command::{
    ExportRequest, StreamSelection, SubtitlePlan, VideoColorSpec, VideoEncodeSpec, build_args,
};
pub use concat::{
    ExportSegment, ExportStage, MultiExport, MultiExportError, MultiExportEvent,
    MultiExportRequest, SmartCutContext,
};
pub use merge::{MergeError, merge_files};
pub use progress::{ProgressUpdate, parse_progress_line};
pub use smart_cut::{CopyBoundary, SmartCutPiece, SmartCutPlan, SmartCutUnavailable, encoder_for};

/// Renders an FFmpeg argument list as a copyable shell command.
///
/// This is for display only; execution always passes arguments separately.
pub fn render_command(args: &[OsString]) -> String {
    let mut text = String::from("ffmpeg");
    for arg in args {
        text.push(' ');
        let arg = arg.to_string_lossy();
        let needs_quotes = arg.is_empty()
            || arg
                .chars()
                .any(|character| character.is_whitespace() || "'\"\\$`".contains(character));
        if needs_quotes {
            text.push('\'');
            text.push_str(&arg.replace('\'', "'\\''"));
            text.push('\'');
        } else {
            text.push_str(&arg);
        }
    }
    text
}

const FFMPEG: &str = "ffmpeg";

/// Errors produced while starting or running an export.
#[derive(Debug)]
pub enum ExportError {
    /// The FFmpeg process could not be started.
    Spawn {
        /// Requested output path.
        output: PathBuf,
        /// Operating-system error from process creation.
        source: io::Error,
    },
    /// A worker thread for reading FFmpeg output could not be started.
    Worker {
        /// Requested output path.
        output: PathBuf,
        /// Operating-system error from thread creation.
        source: io::Error,
    },
    /// Chapter metadata could not be written.
    Metadata {
        /// Operating-system error from writing the metadata file.
        source: io::Error,
    },
    /// Cropped subtitles could not be written.
    Subtitle {
        /// Operating-system error from writing the subtitle file.
        source: io::Error,
    },
}

impl fmt::Display for ExportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn { output, source } => {
                write!(
                    formatter,
                    "could not start ffmpeg for {}: {source}",
                    output.display()
                )
            }
            Self::Worker { output, source } => {
                write!(
                    formatter,
                    "could not start an export reader for {}: {source}",
                    output.display()
                )
            }
            Self::Metadata { source } => {
                write!(formatter, "could not write chapter metadata: {source}")
            }
            Self::Subtitle { source } => {
                write!(formatter, "could not write subtitles: {source}")
            }
        }
    }
}

impl std::error::Error for ExportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Spawn { source, .. } | Self::Worker { source, .. } => Some(source),
            Self::Metadata { source } | Self::Subtitle { source } => Some(source),
        }
    }
}

/// Events emitted by a running export process.
#[derive(Debug)]
pub enum ExportEvent {
    /// Muxing progressed to `out_time`.
    Progress {
        /// Last reported output timestamp.
        out_time: Timestamp,
        /// Whether FFmpeg reported `progress=end`.
        done: bool,
    },
    /// The progress stream ended.
    Finished {
        /// FFmpeg stderr, which is only populated when it reported an error.
        stderr: String,
        /// Whether FFmpeg reported `progress=end` before exiting.
        ended: bool,
    },
}

/// Temporary SRT file holding cropped subtitles.
pub(super) struct TempSubtitle {
    path: PathBuf,
}

impl TempSubtitle {
    /// Writes `srt` to a unique temporary file.
    pub(super) fn create(srt: &str) -> io::Result<Self> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?;
        let path = std::env::temp_dir().join(format!(
            "rossresscut-subtitles-{}-{}.srt",
            std::process::id(),
            stamp.as_nanos()
        ));
        fs::write(&path, srt)?;
        Ok(Self { path })
    }

    /// Returns the subtitle file path.
    pub(super) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempSubtitle {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_file(&self.path) {
            log::warn!(
                "could not remove subtitle file {}: {error}",
                self.path.display()
            );
        }
    }
}

/// A running FFmpeg export process.
pub struct ExportProcess {
    child: Child,
    events: Receiver<ExportEvent>,
    reader: Option<JoinHandle<()>>,
    started: Instant,
    args: Vec<OsString>,
    _metadata: Option<TempMetadata>,
    _subtitle: Option<TempSubtitle>,
}

impl ExportProcess {
    /// Starts FFmpeg for `request`.
    pub fn start(request: &ExportRequest) -> Result<Self, ExportError> {
        let metadata = if request.chapters().is_empty() {
            None
        } else {
            Some(
                TempMetadata::create(request.chapters())
                    .map_err(|source| ExportError::Metadata { source })?,
            )
        };
        let subtitle = match request.subtitle() {
            Some(plan) => {
                let text = plan.crop_for(request.start(), request.end());
                if text.trim().is_empty() {
                    None
                } else {
                    Some(
                        TempSubtitle::create(&text)
                            .map_err(|source| ExportError::Subtitle { source })?,
                    )
                }
            }
            None => None,
        };
        let args = build_args(
            request,
            metadata.as_ref().map(TempMetadata::path),
            subtitle.as_ref().map(TempSubtitle::path),
        );
        Self::start_inner(args, request.output(), metadata, subtitle)
    }

    /// Starts FFmpeg with a caller-provided argument list.
    ///
    /// Used by the concat stage, which does not fit [`ExportRequest`].
    pub fn start_with_args(args: Vec<OsString>, output: &Path) -> Result<Self, ExportError> {
        Self::start_inner(args, output, None, None)
    }

    fn start_inner(
        args: Vec<OsString>,
        output: &Path,
        metadata: Option<TempMetadata>,
        subtitle: Option<TempSubtitle>,
    ) -> Result<Self, ExportError> {
        let mut child = Command::new(FFMPEG)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| ExportError::Spawn {
                output: output.to_path_buf(),
                source,
            })?;

        let stdout = child.stdout.take().ok_or_else(|| ExportError::Worker {
            output: output.to_path_buf(),
            source: io::Error::other("ffmpeg stdout was not piped"),
        })?;
        let stderr = child.stderr.take().ok_or_else(|| ExportError::Worker {
            output: output.to_path_buf(),
            source: io::Error::other("ffmpeg stderr was not piped"),
        })?;

        let (sender, events) = mpsc::channel();
        let reader = thread::Builder::new()
            .name("ffmpeg-export".to_owned())
            .spawn(move || read_export_output(stdout, stderr, sender))
            .map_err(|source| ExportError::Worker {
                output: output.to_path_buf(),
                source,
            })?;

        Ok(Self {
            child,
            events,
            reader: Some(reader),
            started: Instant::now(),
            args,
            _metadata: metadata,
            _subtitle: subtitle,
        })
    }

    /// Returns the FFmpeg argument list, excluding the executable path.
    pub fn command_line(&self) -> &[OsString] {
        &self.args
    }

    /// Polls for the next event without blocking.
    pub fn poll(&mut self) -> Option<ExportEvent> {
        match self.events.try_recv() {
            Ok(event) => Some(event),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => None,
        }
    }

    /// Waits for the process to exit and reaps it.
    ///
    /// Call this after [`ExportEvent::Finished`].
    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        self.join_reader();
        self.child.wait()
    }

    /// Stops the process.
    pub fn cancel(&mut self) {
        match self.child.try_wait() {
            Ok(Some(_)) => {}
            Ok(None) => {
                if let Err(error) = self.child.kill() {
                    log::warn!("could not kill the ffmpeg export process: {error}");
                }
            }
            Err(error) => log::warn!("could not query the ffmpeg export process: {error}"),
        }
    }

    /// Returns how long the process has been running.
    pub fn elapsed(&self) -> StdDuration {
        self.started.elapsed()
    }

    fn join_reader(&mut self) {
        let Some(handle) = self.reader.take() else {
            return;
        };
        if let Err(error) = handle.join() {
            log::warn!("ffmpeg export reader thread panicked: {error:?}");
        }
    }
}

impl Drop for ExportProcess {
    fn drop(&mut self) {
        self.cancel();
        self.join_reader();
        if let Err(error) = self.child.wait() {
            log::warn!("could not reap the ffmpeg export process: {error}");
        }
    }
}

/// Reads progress from stdout and the error text from stderr.
fn read_export_output(
    stdout: impl Read,
    stderr: impl Read + Send + 'static,
    sender: mpsc::Sender<ExportEvent>,
) {
    let stderr_handle = thread::Builder::new()
        .name("ffmpeg-export-stderr".to_owned())
        .spawn(move || read_stderr(stderr));

    let mut stdout = BufReader::new(stdout);
    let mut line = String::new();
    let mut out_time = Timestamp::ZERO;
    let mut ended = false;

    loop {
        line.clear();
        match stdout.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                let Some(update) = parse_progress_line(&line) else {
                    continue;
                };
                match update {
                    ProgressUpdate::OutTime(time) => {
                        out_time = out_time.max(time);
                        if sender
                            .send(ExportEvent::Progress {
                                out_time,
                                done: false,
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                    ProgressUpdate::Ended => {
                        ended = true;
                        if sender
                            .send(ExportEvent::Progress {
                                out_time,
                                done: true,
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
            Err(error) => {
                log::warn!("could not read ffmpeg export progress: {error}");
                break;
            }
        }
    }

    let stderr_text = match stderr_handle {
        Ok(handle) => match handle.join() {
            Ok(text) => text,
            Err(_) => "ffmpeg stderr reader panicked".to_owned(),
        },
        Err(error) => format!("could not start the ffmpeg stderr reader: {error}"),
    };

    // The receiver is dropped when the application closes or cancels the job.
    // Losing the final event in that case is expected.
    let _ = sender.send(ExportEvent::Finished {
        stderr: stderr_text,
        ended,
    });
}

fn read_stderr(mut stderr: impl Read) -> String {
    let mut text = String::new();
    match stderr.read_to_string(&mut text) {
        Ok(_) => text,
        Err(error) => format!("could not read ffmpeg stderr: {error}\n{text}"),
    }
}

#[cfg(test)]
mod command_render_tests {
    use super::*;

    #[test]
    fn renders_arguments_for_display() {
        let args: Vec<OsString> = vec![
            "-ss".into(),
            "1.500000".into(),
            "-i".into(),
            "/tmp/a file.mkv".into(),
            "-map".into(),
            "0".into(),
        ];
        assert_eq!(
            render_command(&args),
            "ffmpeg -ss 1.500000 -i '/tmp/a file.mkv' -map 0"
        );
    }

    #[test]
    fn escapes_single_quotes() {
        let args: Vec<OsString> = vec!["it's".into()];
        assert_eq!(render_command(&args), "ffmpeg 'it'\\''s'");
    }
}
