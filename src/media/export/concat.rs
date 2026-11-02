//! Multi-segment lossless export.
//!
//! Each segment is first cut into a temporary file with stream copy, then all
//! parts are joined with the concat demuxer. The concat filter is not used
//! because it re-encodes.
//!
//! The temporary directory is removed when the export is dropped, including on
//! failure and cancellation.

use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::domain::time::{Duration, Timestamp};
use crate::media::ffprobe;

use super::chapters::{ChapterSpec, render_ffmetadata};
use super::command::{StreamSelection, SubtitlePlan};
use super::{ExportError, ExportEvent, ExportProcess, ExportRequest};

/// One source-time range in a multi-segment export.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExportSegment {
    start: Timestamp,
    end: Timestamp,
}

impl ExportSegment {
    /// Creates a segment, returning `None` unless `start < end`.
    pub fn new(start: Timestamp, end: Timestamp) -> Option<Self> {
        (start < end).then_some(Self { start, end })
    }

    /// Returns the segment start.
    pub const fn start(self) -> Timestamp {
        self.start
    }

    /// Returns the segment end.
    pub const fn end(self) -> Timestamp {
        self.end
    }

    /// Returns the requested length.
    pub fn duration(self) -> Duration {
        self.end.duration_since(self.start).unwrap_or_default()
    }
}

/// A validated multi-segment export request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MultiExportRequest {
    source: PathBuf,
    segments: Vec<ExportSegment>,
    output: PathBuf,
    chapters: bool,
    title: Option<String>,
    subtitle: Option<SubtitlePlan>,
    streams: Option<StreamSelection>,
}

impl MultiExportRequest {
    /// Creates a request for `segments`, in the given output order.
    ///
    /// Returns `None` when there are no segments, when the output equals the
    /// source, or when the output has no file extension.
    pub fn new(
        source: impl Into<PathBuf>,
        segments: Vec<ExportSegment>,
        output: impl Into<PathBuf>,
    ) -> Option<Self> {
        let source = source.into();
        let output = output.into();
        if segments.is_empty() || source == output || output.extension().is_none() {
            return None;
        }
        Some(Self {
            source,
            segments,
            output,
            chapters: false,
            title: None,
            subtitle: None,
            streams: None,
        })
    }

    /// Enables or disables generated chapters.
    pub const fn with_chapters(mut self, chapters: bool) -> Self {
        self.chapters = chapters;
        self
    }

    /// Returns whether chapters should be generated.
    pub const fn chapters(&self) -> bool {
        self.chapters
    }

    /// Sets the output title metadata. Empty titles are ignored.
    pub fn with_title(mut self, title: Option<String>) -> Self {
        self.title = title.filter(|title| !title.trim().is_empty());
        self
    }

    /// Returns the output title metadata, when set.
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Attaches cropped subtitles to every part.
    pub fn with_subtitle(mut self, subtitle: Option<SubtitlePlan>) -> Self {
        self.subtitle = subtitle;
        self
    }

    /// Restricts every part to the listed streams.
    pub fn with_streams(mut self, streams: Option<StreamSelection>) -> Self {
        self.streams = streams;
        self
    }

    /// Returns the source file.
    pub fn source(&self) -> &Path {
        &self.source
    }

    /// Returns the segments in output order.
    pub fn segments(&self) -> &[ExportSegment] {
        &self.segments
    }

    /// Returns the output path.
    pub fn output(&self) -> &Path {
        &self.output
    }

    /// Returns the sum of the requested segment lengths.
    pub fn total_duration(&self) -> Duration {
        self.segments.iter().fold(Duration::ZERO, |total, segment| {
            total.checked_add(segment.duration()).unwrap_or(total)
        })
    }
}

/// Stage reported while a multi-segment export runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportStage {
    /// A segment is being cut.
    Cutting {
        /// Zero-based segment index.
        index: usize,
        /// Number of segments.
        total: usize,
    },
    /// The parts are being concatenated.
    Concatenating,
}

/// Events emitted by a multi-segment export.
#[derive(Debug)]
pub enum MultiExportEvent {
    /// Overall progress.
    Progress {
        /// Current stage.
        stage: ExportStage,
        /// Overall output timestamp.
        out_time: Timestamp,
        /// Sum of the requested segment lengths.
        duration: Duration,
    },
    /// The export ended. `error` is `None` on success.
    Finished {
        /// Failure description, if the export did not succeed.
        error: Option<String>,
    },
}

/// Errors produced while starting a multi-segment export.
#[derive(Debug)]
pub enum MultiExportError {
    /// The temporary directory could not be created.
    WorkDir(io::Error),
    /// The concat list could not be written.
    TempFile(io::Error),
    /// A segment range was invalid.
    InvalidPart {
        /// Zero-based segment index.
        index: usize,
    },
    /// An FFmpeg process could not be started.
    Export(ExportError),
}

impl fmt::Display for MultiExportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WorkDir(error) => {
                write!(formatter, "could not create an export temp dir: {error}")
            }
            Self::TempFile(error) => write!(formatter, "could not write the concat list: {error}"),
            Self::InvalidPart { index } => {
                write!(formatter, "segment {index} has an invalid range")
            }
            Self::Export(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for MultiExportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::WorkDir(error) | Self::TempFile(error) => Some(error),
            Self::Export(error) => Some(error),
            Self::InvalidPart { .. } => None,
        }
    }
}

/// A running multi-segment export.
pub struct MultiExport {
    source: PathBuf,
    segments: Vec<ExportSegment>,
    output: PathBuf,
    parts: Vec<PathBuf>,
    work_dir: TempDir,
    chapters: bool,
    total_duration: Duration,
    completed_duration: Duration,
    part_durations: Vec<Duration>,
    commands: Vec<Vec<OsString>>,
    stage: Option<Stage>,
    title: Option<String>,
    subtitle: Option<SubtitlePlan>,
    streams: Option<StreamSelection>,
}

enum Stage {
    Cutting {
        index: usize,
        process: ExportProcess,
    },
    ProbingDurations {
        durations: Receiver<Vec<Duration>>,
    },
    Concatenating {
        process: ExportProcess,
    },
    Done,
    Failed,
}

impl MultiExport {
    /// Starts cutting the first segment.
    pub fn start(request: MultiExportRequest) -> Result<Self, MultiExportError> {
        let work_dir = TempDir::create().map_err(MultiExportError::WorkDir)?;
        let extension = request
            .output
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("mkv");
        let parts: Vec<PathBuf> = (0..request.segments.len())
            .map(|index| work_dir.path().join(format!("part-{index:04}.{extension}")))
            .collect();
        let total_duration = request.total_duration();
        let part_durations = vec![Duration::ZERO; parts.len()];
        let command_capacity = parts.len() + 1;

        let mut export = Self {
            source: request.source,
            segments: request.segments,
            output: request.output,
            parts,
            work_dir,
            chapters: request.chapters,
            title: request.title,
            subtitle: request.subtitle,
            streams: request.streams,
            total_duration,
            completed_duration: Duration::ZERO,
            part_durations,
            commands: Vec::with_capacity(command_capacity),
            stage: None,
        };
        export.start_cut(0)?;
        Ok(export)
    }

    /// Polls the current stage without blocking.
    pub fn poll(&mut self) -> Option<MultiExportEvent> {
        loop {
            let stage = self.stage.take()?;
            match stage {
                Stage::Cutting { index, mut process } => match process.poll() {
                    None => {
                        self.stage = Some(Stage::Cutting { index, process });
                        return None;
                    }
                    Some(ExportEvent::Progress { out_time, .. }) => {
                        let current = u64::try_from(out_time.micros()).unwrap_or(0);
                        let overall = self
                            .completed_duration
                            .checked_add(Duration::from_micros(current))
                            .unwrap_or(self.completed_duration);
                        self.stage = Some(Stage::Cutting { index, process });
                        return Some(MultiExportEvent::Progress {
                            stage: ExportStage::Cutting {
                                index,
                                total: self.segments.len(),
                            },
                            out_time: Timestamp::from_micros(
                                i64::try_from(overall.micros()).unwrap_or(i64::MAX),
                            ),
                            duration: self.total_duration,
                        });
                    }
                    Some(ExportEvent::Finished { stderr, ended }) => {
                        let exit = process.wait();
                        if !(ended && exit.map(|status| status.success()).unwrap_or(false)) {
                            self.stage = Some(Stage::Failed);
                            return Some(MultiExportEvent::Finished {
                                error: Some(failure(stderr)),
                            });
                        }
                        self.completed_duration = self
                            .completed_duration
                            .checked_add(self.segments[index].duration())
                            .unwrap_or(self.completed_duration);

                        if index + 1 < self.segments.len() {
                            if let Err(error) = self.start_cut(index + 1) {
                                self.stage = Some(Stage::Failed);
                                return Some(MultiExportEvent::Finished {
                                    error: Some(error.to_string()),
                                });
                            }
                        } else {
                            if let Err(error) = self.start_probing_durations() {
                                self.stage = Some(Stage::Failed);
                                return Some(MultiExportEvent::Finished {
                                    error: Some(error.to_string()),
                                });
                            }
                        }
                    }
                },
                Stage::ProbingDurations { durations } => match durations.try_recv() {
                    Ok(found) => {
                        self.part_durations = found;
                        if let Err(error) = self.start_concat() {
                            self.stage = Some(Stage::Failed);
                            return Some(MultiExportEvent::Finished {
                                error: Some(error.to_string()),
                            });
                        }
                    }
                    Err(TryRecvError::Empty) => {
                        self.stage = Some(Stage::ProbingDurations { durations });
                        return None;
                    }
                    Err(TryRecvError::Disconnected) => {
                        // The probe thread panicked or was torn down. Keep the
                        // requested lengths rather than failing the export.
                        if let Err(error) = self.start_concat() {
                            self.stage = Some(Stage::Failed);
                            return Some(MultiExportEvent::Finished {
                                error: Some(error.to_string()),
                            });
                        }
                    }
                },
                Stage::Concatenating { mut process } => match process.poll() {
                    None => {
                        self.stage = Some(Stage::Concatenating { process });
                        return None;
                    }
                    Some(ExportEvent::Progress { out_time, .. }) => {
                        self.stage = Some(Stage::Concatenating { process });
                        return Some(MultiExportEvent::Progress {
                            stage: ExportStage::Concatenating,
                            out_time,
                            duration: self.total_duration,
                        });
                    }
                    Some(ExportEvent::Finished { stderr, ended }) => {
                        let exit = process.wait();
                        self.stage = Some(Stage::Done);
                        let succeeded =
                            ended && exit.map(|status| status.success()).unwrap_or(false);
                        return Some(MultiExportEvent::Finished {
                            error: (!succeeded).then(|| failure(stderr)),
                        });
                    }
                },
                Stage::Done => {
                    self.stage = Some(Stage::Done);
                    return None;
                }
                Stage::Failed => {
                    self.stage = Some(Stage::Failed);
                    return None;
                }
            }
        }
    }

    /// Requests cancellation of the running FFmpeg process.
    pub fn cancel(&mut self) {
        if let Some(Stage::Cutting { process, .. } | Stage::Concatenating { process }) =
            self.stage.as_mut()
        {
            process.cancel();
        }
    }

    /// Returns the temporary directory, for tests and diagnostics.
    pub fn work_dir(&self) -> &Path {
        self.work_dir.path()
    }

    /// Returns the FFmpeg argument lists started by this export, in order.
    pub fn commands(&self) -> &[Vec<OsString>] {
        &self.commands
    }

    fn start_cut(&mut self, index: usize) -> Result<(), MultiExportError> {
        let segment = self.segments[index];
        let request = ExportRequest::new(
            &self.source,
            segment.start(),
            segment.end(),
            &self.parts[index],
        )
        .ok_or(MultiExportError::InvalidPart { index })?
        .with_streams(self.streams.clone());
        let subtitle_path = self
            .subtitle
            .as_ref()
            .map(|plan| plan.crop_for(segment.start(), segment.end()))
            .filter(|text| !text.trim().is_empty())
            .map(|text| {
                let path = self.work_dir.path().join(format!("part-{index:04}.srt"));
                (path, text)
            })
            .and_then(|(path, text)| std::fs::write(&path, text).ok().map(|_| path));
        let args = super::build_args(&request, None, subtitle_path.as_deref());
        let process = ExportProcess::start_with_args(args.clone(), &self.parts[index])
            .map_err(MultiExportError::Export)?;
        self.commands.push(args);
        self.stage = Some(Stage::Cutting { index, process });
        Ok(())
    }

    /// Probes the produced parts on a worker thread.
    ///
    /// Chapters must use the real part lengths: `-ss` falls back to the
    /// previous keyframe, so a part is usually longer than its request.
    fn start_probing_durations(&mut self) -> Result<(), MultiExportError> {
        let parts = self.parts.clone();
        let requested: Vec<Duration> = self
            .segments
            .iter()
            .map(|segment| segment.duration())
            .collect();
        let (sender, receiver) = mpsc::channel();
        let spawned = thread::Builder::new()
            .name("export-chapter-probe".to_owned())
            .spawn(move || {
                let durations = parts
                    .iter()
                    .zip(requested)
                    .map(|(part, requested)| match ffprobe::probe(part) {
                        Ok(media) => media.duration.unwrap_or(requested),
                        Err(error) => {
                            log::warn!(
                                "could not probe {} for chapter timing: {error}",
                                part.display()
                            );
                            requested
                        }
                    })
                    .collect();
                // The receiver is dropped when the export is cancelled.
                let _ = sender.send(durations);
            });
        if let Err(error) = spawned {
            log::warn!(
                "could not start the chapter probe thread: {error}; using requested lengths"
            );
            return self.start_concat();
        }
        self.stage = Some(Stage::ProbingDurations {
            durations: receiver,
        });
        Ok(())
    }

    fn start_concat(&mut self) -> Result<(), MultiExportError> {
        let list_path = self.work_dir.path().join("concat.txt");
        let mut list = String::new();
        for part in &self.parts {
            list.push_str(&concat_line(part));
            list.push('\n');
        }
        fs::write(&list_path, list).map_err(MultiExportError::TempFile)?;

        let metadata_path = if self.chapters {
            let mut chapters = Vec::with_capacity(self.segments.len());
            let mut start = Duration::ZERO;
            for (index, _segment) in self.segments.iter().enumerate() {
                let duration = self.part_durations[index];
                // A part whose progress stream reported nothing falls back to
                // the requested length so chapters never collapse.
                let duration = if duration.micros() == 0 {
                    self.segments[index].duration()
                } else {
                    duration
                };
                let end = start.checked_add(duration).unwrap_or(start);
                chapters.push(ChapterSpec {
                    start,
                    end,
                    title: format!("Segment {}", index + 1),
                });
                start = end;
            }
            let path = self.work_dir.path().join("chapters.txt");
            fs::write(&path, render_ffmetadata(&chapters)).map_err(MultiExportError::TempFile)?;
            Some(path)
        } else {
            None
        };

        let args = concat_args(
            &list_path,
            &self.output,
            metadata_path.as_deref(),
            self.title.as_deref(),
        );
        let process = ExportProcess::start_with_args(args.clone(), &self.output)
            .map_err(MultiExportError::Export)?;
        self.commands.push(args);
        self.stage = Some(Stage::Concatenating { process });
        Ok(())
    }
}

impl Drop for MultiExport {
    fn drop(&mut self) {
        if let Some(stage) = self.stage.take() {
            match stage {
                Stage::Cutting { mut process, .. } | Stage::Concatenating { mut process } => {
                    process.cancel();
                    if let Err(error) = process.wait() {
                        log::warn!("could not reap the ffmpeg part process: {error}");
                    }
                }
                Stage::Done | Stage::Failed | Stage::ProbingDurations { .. } => {}
            }
        }
    }
}

fn concat_args(
    list: &Path,
    output: &Path,
    metadata: Option<&Path>,
    title: Option<&str>,
) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec![
        "-nostdin".into(),
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-progress".into(),
        "pipe:1".into(),
        "-nostats".into(),
        "-f".into(),
        "concat".into(),
        "-safe".into(),
        "0".into(),
        "-i".into(),
        list.as_os_str().to_owned(),
    ];

    if let Some(metadata) = metadata {
        args.push("-i".into());
        args.push(metadata.as_os_str().to_owned());
    }

    args.extend(["-map".into(), "0".into(), "-c".into(), "copy".into()]);

    if let Some(title) = title {
        args.push("-metadata".into());
        args.push(format!("title={title}").into());
    }

    if metadata.is_some() {
        args.push("-map_chapters".into());
        args.push("1".into());
    }

    args.extend([
        "-ignore_unknown".into(),
        "-avoid_negative_ts".into(),
        "make_zero".into(),
        "-y".into(),
        output.as_os_str().to_owned(),
    ]);

    args
}

pub(super) fn concat_line(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\'', "'\\''");
    format!("file '{text}'")
}

fn failure(stderr: String) -> String {
    let details = stderr.trim();
    if details.is_empty() {
        "ffmpeg exited with an error".to_owned()
    } else {
        details.to_owned()
    }
}

pub(super) struct TempDir {
    path: PathBuf,
}

impl TempDir {
    pub(super) fn create() -> io::Result<Self> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?;
        let path = std::env::temp_dir().join(format!(
            "rossresscut-export-{}-{}",
            std::process::id(),
            stamp.as_nanos()
        ));
        fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.path) {
            log::warn!(
                "could not remove export temp dir {}: {error}",
                self.path.display()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_segments_and_missing_extension() {
        let start = Timestamp::from_micros(1_000_000);
        let end = Timestamp::from_micros(2_000_000);
        assert!(MultiExportRequest::new("in.mp4", Vec::new(), "out.mp4").is_none());
        assert!(
            MultiExportRequest::new(
                "in.mp4",
                vec![ExportSegment::new(start, end).unwrap()],
                "out"
            )
            .is_none()
        );
        assert!(
            MultiExportRequest::new(
                "in.mp4",
                vec![ExportSegment::new(start, end).unwrap()],
                "in.mp4"
            )
            .is_none()
        );
    }

    #[test]
    fn sums_requested_durations_in_order() {
        let segments = vec![
            ExportSegment::new(
                Timestamp::from_micros(3_000_000),
                Timestamp::from_micros(5_000_000),
            )
            .unwrap(),
            ExportSegment::new(
                Timestamp::from_micros(1_000_000),
                Timestamp::from_micros(2_000_000),
            )
            .unwrap(),
        ];
        let request =
            MultiExportRequest::new("in.mp4", segments, "out.mp4").expect("valid request");
        assert_eq!(request.total_duration(), Duration::from_micros(3_000_000));
    }

    #[test]
    fn escapes_single_quotes_in_concat_paths() {
        assert_eq!(
            concat_line(Path::new("/tmp/a'b.mkv")),
            "file '/tmp/a'\\''b.mkv'"
        );
    }
}
