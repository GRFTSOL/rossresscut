//! Export job state, command errors, and the single/multi process wrapper.

use std::fmt;
use std::io;
use std::path::PathBuf;
use std::process::ExitStatus;

use crate::domain::media::{MediaFile, VideoStream};
use crate::domain::time::{Duration, FrameRate, Timestamp};
pub use crate::media::export::ExportStage;

use crate::media::export::{
    ExportError, ExportEvent, ExportProcess, MultiExport, MultiExportError, MultiExportEvent,
    SmartCutContext, StreamSelection, VideoColorSpec, VideoEncodeSpec, encoder_for, render_command,
};

use super::Application;

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
    /// Smart cut cannot run for the current media.
    SmartCut(String),
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
            Self::SmartCut(message) => write!(formatter, "smart cut is unavailable: {message}"),
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

// `MultiExport` owns the temporary directory and per-part bookkeeping, so it is
// much larger than a single process handle. Boxing it would move the allocation
// without simplifying the state machine.
#[allow(clippy::large_enum_variant)]
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

    /// Returns the FFmpeg command lines started so far.
    pub(crate) fn commands(&self) -> Vec<String> {
        match &self.process {
            ExportProcessKind::Single(process) => {
                vec![render_command(process.command_line())]
            }
            ExportProcessKind::Multi(process) => process
                .commands()
                .iter()
                .map(|args| render_command(args))
                .collect(),
        }
    }
}

impl Application {
    /// Builds the smart-cut settings for the current media.
    ///
    /// Smart cut only works on the preview video stream, because that is the
    /// stream the keyframe index describes.
    pub(super) fn smart_cut_context(
        &self,
        streams: Option<&StreamSelection>,
        manual_bit_rate_kbps: u32,
    ) -> Result<SmartCutContext, ExportCommandError> {
        let media = self.media.as_ref().ok_or(ExportCommandError::NoMedia)?;
        let keyframes = self
            .keyframes
            .clone()
            .ok_or_else(|| smart_cut_error("the keyframe index is not ready yet"))?;
        let video = smart_cut_video(media, streams)?;

        let codec = video
            .info
            .codec_name
            .as_deref()
            .ok_or_else(|| smart_cut_error("the video codec is unknown"))?;
        let encoder = encoder_for(codec)
            .ok_or_else(|| smart_cut_error(format!("no software encoder for codec '{codec}'")))?;

        let frame_duration = video
            .average_frame_rate
            .or(video.frame_rate)
            .and_then(FrameRate::frame_duration)
            .ok_or_else(|| smart_cut_error("the video frame rate is unknown"))?;

        let source_bit_rate = smart_cut_bit_rate(media, video)?;
        let manual_bit_rate = if manual_bit_rate_kbps > 0 {
            Some(u64::from(manual_bit_rate_kbps) * 1_000)
        } else {
            None
        };
        let bit_rate = smart_cut_target_bit_rate(source_bit_rate, manual_bit_rate)
            .ok_or_else(|| smart_cut_error("the video bit rate overflows"))?;

        let timescale = video
            .info
            .time_base
            .filter(|time_base| time_base.numerator() == 1)
            .map(|time_base| u64::from(time_base.denominator()));

        Ok(SmartCutContext {
            stream_index: video.info.index,
            start_time: media.start_time,
            manual_bit_rate,
            variable_frame_rate: has_variable_frame_rate(video),
            encode: VideoEncodeSpec {
                encoder: encoder.to_owned(),
                bit_rate,
                b_frames: video.has_b_frames,
                timescale,
                color: VideoColorSpec {
                    pixel_format: video.pixel_format.clone(),
                    range: video.color_range.clone(),
                    space: video.color_space.clone(),
                    transfer: video.color_transfer.clone(),
                    primaries: video.color_primaries.clone(),
                    mastering_display: video.mastering_display.clone(),
                    content_light: video.content_light.clone(),
                },
            },
            frame_duration,
            keyframes,
        })
    }
}

/// Returns the video stream smart cut may re-encode.
///
/// It must be the preview stream: the keyframe index describes that one.
fn smart_cut_video<'a>(
    media: &'a MediaFile,
    streams: Option<&StreamSelection>,
) -> Result<&'a VideoStream, ExportCommandError> {
    let preview = super::playback::preview_video_stream(media)
        .ok_or_else(|| smart_cut_error("the file has no video stream"))?;

    let selected = match streams {
        Some(selection) => {
            if selection.video.len() != 1 {
                return Err(smart_cut_error(
                    "smart cut needs exactly one selected video stream",
                ));
            }
            selection.video[0]
        }
        None => preview.info.index,
    };
    if selected != preview.info.index {
        return Err(smart_cut_error(
            "smart cut can only use the preview video stream",
        ));
    }
    Ok(preview)
}

/// Returns the target bit rate for a smart-cut re-encode.
///
/// A manual value wins. Otherwise the source video bit rate gets 50%
/// headroom to absorb encoder differences.
fn smart_cut_target_bit_rate(source_bit_rate: u64, manual: Option<u64>) -> Option<u64> {
    manual.or_else(|| source_bit_rate.checked_add(source_bit_rate / 2))
}

/// Returns whether the nominal and average frame rates differ by more than 0.5%.
fn has_variable_frame_rate(video: &VideoStream) -> bool {
    frame_rates_differ(video.frame_rate, video.average_frame_rate)
}

fn frame_rates_differ(nominal: Option<FrameRate>, average: Option<FrameRate>) -> bool {
    let (Some(nominal), Some(average)) = (nominal, average) else {
        return false;
    };
    let left = u64::from(nominal.numerator()) * u64::from(average.denominator());
    let right = u64::from(average.numerator()) * u64::from(nominal.denominator());
    left.abs_diff(right) * 200 > left.max(right)
}

/// Returns the video bit rate, falling back to the container size and duration.
fn smart_cut_bit_rate(media: &MediaFile, video: &VideoStream) -> Result<u64, ExportCommandError> {
    video
        .info
        .bit_rate
        .or_else(|| {
            media.format.size.and_then(|size| {
                let micros = media.duration?.micros();
                if micros == 0 {
                    return None;
                }
                size.checked_mul(8)?
                    .checked_mul(1_000_000)?
                    .checked_div(micros)
            })
        })
        .ok_or_else(|| smart_cut_error("the video bit rate cannot be determined"))
}

fn smart_cut_error(message: impl Into<String>) -> ExportCommandError {
    ExportCommandError::SmartCut(message.into())
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
    fn smart_cut_bit_rate_defaults_to_source_with_headroom() {
        assert_eq!(smart_cut_target_bit_rate(2_000_000, None), Some(3_000_000));
        assert_eq!(
            smart_cut_target_bit_rate(2_000_000, Some(2_500_000)),
            Some(2_500_000)
        );
        assert_eq!(smart_cut_target_bit_rate(1, None), Some(1));
        assert_eq!(smart_cut_target_bit_rate(u64::MAX, None), None);
    }

    #[test]
    fn detects_variable_frame_rates() {
        let rate = FrameRate::parse;
        assert!(!frame_rates_differ(rate("60000/1001"), rate("60000/1001")));
        assert!(!frame_rates_differ(rate("60000/1001"), rate("120000/2002")));
        assert!(frame_rates_differ(rate("120/1"), rate("52800/719")));
        assert!(!frame_rates_differ(None, rate("30/1")));
    }

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
