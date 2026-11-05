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
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::domain::media::KeyframeIndex;
use crate::domain::time::{Duration, Timestamp};
use crate::media::ffprobe;

use super::chapters::{ChapterSpec, render_ffmetadata};
use super::command::{StreamSelection, SubtitlePlan, VideoEncodeSpec};
use super::planning;
use super::smart_cut::{SmartCutPlan, SmartCutUnavailable};
use super::{ExportError, ExportEvent, ExportProcess, ExportRequest};

/// Placeholder cue used when a part has no subtitle cues.
///
/// It has empty text and lasts one millisecond, so FFmpeg keeps the subtitle
/// stream without rendering anything.
const EMPTY_SUBTITLE: &str = "1\n00:00:00,000 --> 00:00:00,001\n\n";

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

/// Everything a multi-segment export needs to smart cut its parts.
///
/// The caller has already validated that exactly one video stream is being
/// exported and that a software encoder exists for its codec.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SmartCutContext {
    /// Re-encode settings for every encoded head.
    pub encode: VideoEncodeSpec,
    /// Manual bit rate override in bits per second; `None` uses each piece's
    /// probed source bit rate plus headroom.
    pub manual_bit_rate: Option<u64>,
    /// Whether the source has variable frame durations, so smart-cut margins
    /// must use the local PTS spacing instead of the average frame rate.
    pub variable_frame_rate: bool,
    /// Nominal duration of one frame.
    pub frame_duration: Duration,
    /// Keyframes of the exported video stream.
    pub keyframes: KeyframeIndex,
    /// Index of the exported video stream in the source file.
    pub stream_index: usize,
    /// Container start time, which the keyframe index has already subtracted.
    pub start_time: Option<Timestamp>,
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
    smart_cut: Option<SmartCutContext>,
    chapter_titles: Vec<String>,
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
            smart_cut: None,
            chapter_titles: Vec::new(),
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

    /// Enables smart cutting for every segment.
    pub fn with_smart_cut(mut self, smart_cut: Option<SmartCutContext>) -> Self {
        self.smart_cut = smart_cut;
        self
    }

    /// Sets the chapter title of every segment, in output order.
    ///
    /// Missing or blank entries fall back to `Segment N`.
    pub fn with_chapter_titles(mut self, titles: Vec<String>) -> Self {
        self.chapter_titles = titles;
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

/// One temporary part produced by a multi-segment export.
#[derive(Clone, Debug)]
struct PartSpec {
    /// Index of the segment this part belongs to.
    segment: usize,
    start: Timestamp,
    end: Timestamp,
    /// `-ss` target; encoded parts seek to their own start.
    seek: Timestamp,
    encode: bool,
    /// Source average bit rate probed for this piece, when available.
    encode_bit_rate: Option<u64>,
    path: PathBuf,
}

impl PartSpec {
    fn duration(&self) -> Duration {
        self.end.duration_since(self.start).unwrap_or_default()
    }
}

/// First and last video timestamps of one cut part, in its own timeline.
#[derive(Clone, Copy, Debug, Default)]
struct PartTiming {
    /// PTS of the first video packet. Stream copy can leave it above zero.
    first: Timestamp,
    /// End of the last video packet (`PTS + duration`).
    end: Timestamp,
}

impl PartTiming {
    fn span(&self) -> Duration {
        self.end.duration_since(self.first).unwrap_or_default()
    }
}

/// Timings probed from the cut parts, one per part.
struct ProbedTimings {
    parts: Vec<PartTiming>,
}

/// Stage reported while a multi-segment export runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportStage {
    /// A segment is being cut.
    Cutting {
        /// Zero-based segment index.
        index: usize,
        /// Number of segments in the export.
        total: usize,
    },
    /// Parts are being concatenated.
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
    /// The concat list or another temporary file could not be written.
    TempFile(io::Error),
    /// A segment range was invalid.
    InvalidPart {
        /// Zero-based segment index.
        index: usize,
    },
    /// A segment cannot be smart cut.
    SmartCut {
        /// Zero-based segment index.
        index: usize,
        /// Why the segment cannot be smart cut.
        reason: SmartCutUnavailable,
    },
    /// A stream-copy part started before its planned keyframe.
    SmartCutSeek {
        /// Zero-based segment index.
        index: usize,
    },
    /// The export planning thread could not start.
    Planning(io::Error),
    /// A probe needed for planning failed.
    Probe(ffprobe::ProbeError),
    /// An FFmpeg process could not be started.
    Export(ExportError),
}

impl fmt::Display for MultiExportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WorkDir(error) => {
                write!(formatter, "could not create an export temp dir: {error}")
            }
            Self::TempFile(error) => {
                write!(
                    formatter,
                    "could not write a temporary export file: {error}"
                )
            }
            Self::InvalidPart { index } => {
                write!(formatter, "segment {index} has an invalid range")
            }
            Self::SmartCut { index, reason } => {
                write!(formatter, "segment {index} cannot be smart cut: {reason}")
            }
            Self::SmartCutSeek { index } => write!(
                formatter,
                "segment {index} could not be stream copied from its planned keyframe; the source may have sparse seek points"
            ),
            Self::Planning(error) => {
                write!(
                    formatter,
                    "could not start the export planning thread: {error}"
                )
            }
            Self::Probe(error) => write!(formatter, "{error}"),
            Self::Export(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for MultiExportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::WorkDir(error) | Self::TempFile(error) | Self::Planning(error) => Some(error),
            Self::Export(error) => Some(error),
            Self::Probe(error) => Some(error),
            Self::InvalidPart { .. } | Self::SmartCut { .. } | Self::SmartCutSeek { .. } => None,
        }
    }
}

/// A running multi-segment export.
pub struct MultiExport {
    source: PathBuf,
    segments: Vec<ExportSegment>,
    output: PathBuf,
    parts: Vec<PartSpec>,
    work_dir: TempDir,
    chapters: bool,
    total_duration: Duration,
    completed_duration: Duration,
    part_durations: Vec<Duration>,
    /// Actual first and last video timestamps of each cut part.
    part_timings: Vec<PartTiming>,
    commands: Vec<Vec<OsString>>,
    stage: Option<Stage>,
    title: Option<String>,
    subtitle: Option<SubtitlePlan>,
    streams: Option<StreamSelection>,
    smart_cut: Option<SmartCutContext>,
    chapter_titles: Vec<String>,
}

enum Stage {
    Planning {
        plans: Receiver<Result<Vec<SmartCutPlan>, MultiExportError>>,
    },
    Cutting {
        index: usize,
        process: ExportProcess,
    },
    ProbingTimings {
        timings: Receiver<ProbedTimings>,
    },
    Concatenating {
        process: ExportProcess,
    },
    Done,
    Failed,
}

/// Outcome of polling one export stage.
enum StagePoll {
    /// The stage is still running; the caller should return.
    Pending,
    /// An event is ready for the caller.
    Event(MultiExportEvent),
    /// The stage finished and the next one was started.
    Advanced,
}

impl MultiExport {
    /// Starts cutting the first segment.
    pub fn start(request: MultiExportRequest) -> Result<Self, MultiExportError> {
        let work_dir = TempDir::create().map_err(MultiExportError::WorkDir)?;
        let segment_count = request.segments.len();
        let mut export = Self {
            source: request.source,
            segments: request.segments,
            output: request.output,
            parts: Vec::new(),
            work_dir,
            chapters: request.chapters,
            title: request.title,
            subtitle: request.subtitle,
            streams: request.streams,
            smart_cut: request.smart_cut,
            chapter_titles: request.chapter_titles,
            total_duration: Duration::ZERO,
            completed_duration: Duration::ZERO,
            part_durations: vec![Duration::ZERO; segment_count],
            part_timings: Vec::new(),
            commands: Vec::new(),
            stage: None,
        };

        if export.smart_cut.is_some() {
            // Smart cut probes seek points, so planning runs off the UI thread.
            export.start_planning()?;
        } else {
            let plans = planning::plan_segments(&export.source, &export.segments, None)?;
            export.apply_plans(plans);
            export.start_cut(0)?;
        }
        Ok(export)
    }

    /// Builds the temporary parts from the planned pieces.
    fn apply_plans(&mut self, plans: Vec<SmartCutPlan>) {
        let extension = self
            .output
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("mkv");
        let mut parts = Vec::new();
        for (segment, plan) in plans.iter().enumerate() {
            for (piece, part) in plan.pieces().iter().enumerate() {
                parts.push(PartSpec {
                    segment,
                    start: part.start(),
                    end: part.end(),
                    seek: part.seek(),
                    encode: part.encode(),
                    encode_bit_rate: plan.bit_rate(piece),
                    path: self
                        .work_dir
                        .path()
                        .join(format!("part-{segment:04}-{piece}.{extension}")),
                });
            }
        }
        self.total_duration = plans.iter().fold(Duration::ZERO, |total, plan| {
            total.checked_add(plan.total_duration()).unwrap_or(total)
        });
        self.part_durations = vec![Duration::ZERO; self.segments.len()];
        self.part_timings = vec![PartTiming::default(); parts.len()];
        // One cut per part plus the concat process.
        self.commands = Vec::with_capacity(parts.len() + 1);
        self.parts = parts;
    }

    /// Plans smart-cut pieces on a worker thread.
    fn start_planning(&mut self) -> Result<(), MultiExportError> {
        let source = self.source.clone();
        let segments = self.segments.clone();
        let Some(context) = self.smart_cut.clone() else {
            return Ok(());
        };
        let (sender, receiver) = mpsc::channel();
        let spawned = thread::Builder::new()
            .name("export-plan".to_owned())
            .spawn(move || {
                let result = planning::plan_segments(&source, &segments, Some(&context));
                // The receiver is dropped when the export is cancelled.
                let _ = sender.send(result);
            });
        if let Err(error) = spawned {
            return Err(MultiExportError::Planning(error));
        }
        self.stage = Some(Stage::Planning { plans: receiver });
        Ok(())
    }

    /// Polls the current stage without blocking.
    pub fn poll(&mut self) -> Option<MultiExportEvent> {
        loop {
            let stage = self.stage.take()?;
            let polled = match stage {
                Stage::Planning { plans } => self.poll_planning(plans),
                Stage::Cutting { index, process } => self.poll_cutting(index, process),
                Stage::ProbingTimings { timings } => self.poll_timings(timings),
                Stage::Concatenating { process } => self.poll_concatenating(process),
                Stage::Done => {
                    self.stage = Some(Stage::Done);
                    return None;
                }
                Stage::Failed => {
                    self.stage = Some(Stage::Failed);
                    return None;
                }
            };
            match polled {
                StagePoll::Pending => return None,
                StagePoll::Event(event) => return Some(event),
                StagePoll::Advanced => {}
            }
        }
    }

    fn poll_planning(
        &mut self,
        plans: Receiver<Result<Vec<SmartCutPlan>, MultiExportError>>,
    ) -> StagePoll {
        match plans.try_recv() {
            Ok(Ok(plans)) => {
                self.apply_plans(plans);
                match self.start_cut(0) {
                    Ok(()) => StagePoll::Advanced,
                    Err(error) => StagePoll::Event(self.fail(error)),
                }
            }
            Ok(Err(error)) => StagePoll::Event(self.fail(error)),
            Err(TryRecvError::Empty) => {
                self.stage = Some(Stage::Planning { plans });
                StagePoll::Pending
            }
            Err(TryRecvError::Disconnected) => {
                StagePoll::Event(self.fail("the export planning thread stopped unexpectedly"))
            }
        }
    }

    fn poll_cutting(&mut self, index: usize, mut process: ExportProcess) -> StagePoll {
        match process.poll() {
            None => {
                self.stage = Some(Stage::Cutting { index, process });
                StagePoll::Pending
            }
            Some(ExportEvent::Progress { out_time, .. }) => {
                self.stage = Some(Stage::Cutting { index, process });
                StagePoll::Event(self.progress(
                    out_time,
                    ExportStage::Cutting {
                        index: self.parts[index].segment,
                        total: self.segments.len(),
                    },
                ))
            }
            Some(ExportEvent::Finished { stderr, ended }) => {
                let exit = process.wait();
                if !(ended && exit.map(|status| status.success()).unwrap_or(false)) {
                    return StagePoll::Event(self.fail(failure(stderr)));
                }
                self.completed_duration = self
                    .completed_duration
                    .checked_add(self.parts[index].duration())
                    .unwrap_or(self.completed_duration);

                let next = if index + 1 < self.parts.len() {
                    self.start_cut(index + 1)
                } else {
                    self.start_probing_timings()
                };
                match next {
                    Ok(()) => StagePoll::Advanced,
                    Err(error) => StagePoll::Event(self.fail(error)),
                }
            }
        }
    }

    fn poll_timings(&mut self, timings: Receiver<ProbedTimings>) -> StagePoll {
        match timings.try_recv() {
            Ok(found) => {
                self.part_timings = found.parts;
                if let Err(error) = self.refresh_durations() {
                    return StagePoll::Event(self.fail(error));
                }
                match self.start_concat() {
                    Ok(()) => StagePoll::Advanced,
                    Err(error) => StagePoll::Event(self.fail(error)),
                }
            }
            Err(TryRecvError::Empty) => {
                self.stage = Some(Stage::ProbingTimings { timings });
                StagePoll::Pending
            }
            Err(TryRecvError::Disconnected) => {
                log::warn!("part probe failed; using requested lengths");
                self.use_requested_timings();
                if let Err(error) = self.refresh_durations() {
                    return StagePoll::Event(self.fail(error));
                }
                match self.start_concat() {
                    Ok(()) => StagePoll::Advanced,
                    Err(error) => StagePoll::Event(self.fail(error)),
                }
            }
        }
    }

    fn poll_concatenating(&mut self, mut process: ExportProcess) -> StagePoll {
        match process.poll() {
            None => {
                self.stage = Some(Stage::Concatenating { process });
                StagePoll::Pending
            }
            Some(ExportEvent::Progress { out_time, .. }) => {
                self.stage = Some(Stage::Concatenating { process });
                StagePoll::Event(MultiExportEvent::Progress {
                    stage: ExportStage::Concatenating,
                    out_time,
                    duration: self.total_duration,
                })
            }
            Some(ExportEvent::Finished { stderr, ended }) => {
                let exit = process.wait();
                self.stage = Some(Stage::Done);
                let succeeded = ended && exit.map(|status| status.success()).unwrap_or(false);
                StagePoll::Event(MultiExportEvent::Finished {
                    error: (!succeeded).then(|| failure(stderr)),
                })
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

    /// Builds an overall progress event for the running stage.
    fn progress(&self, out_time: Timestamp, stage: ExportStage) -> MultiExportEvent {
        let elapsed = u64::try_from(out_time.micros()).unwrap_or(0);
        let overall = self
            .completed_duration
            .checked_add(Duration::from_micros(elapsed))
            .unwrap_or(self.completed_duration);
        MultiExportEvent::Progress {
            stage,
            out_time: Timestamp::from_micros(i64::try_from(overall.micros()).unwrap_or(i64::MAX)),
            duration: self.total_duration,
        }
    }

    /// Marks the export as failed and reports `error`.
    fn fail(&mut self, error: impl ToString) -> MultiExportEvent {
        self.stage = Some(Stage::Failed);
        MultiExportEvent::Finished {
            error: Some(error.to_string()),
        }
    }

    fn start_cut(&mut self, index: usize) -> Result<(), MultiExportError> {
        let part = &self.parts[index];
        let (start, end, seek, path, encode, segment) = (
            part.start,
            part.end,
            part.seek,
            part.path.clone(),
            part.encode,
            part.segment,
        );
        let encode_bit_rate = part.encode_bit_rate;
        let mut request = ExportRequest::new(&self.source, start, end, &path)
            .ok_or(MultiExportError::InvalidPart { index: segment })?
            .with_seek(seek)
            .with_streams(self.streams.clone())
            // Concatenated parts keep their timestamps; rebasing would open a
            // hole at the join.
            .with_rebase_negative(self.smart_cut.is_none());
        if encode {
            let context = self
                .smart_cut
                .as_ref()
                .ok_or(MultiExportError::InvalidPart { index: segment })?;
            let mut spec = context.encode.clone();
            if context.manual_bit_rate.is_none()
                && let Some(source) = encode_bit_rate
            {
                spec.bit_rate = source.checked_add(source / 2).unwrap_or(spec.bit_rate);
            }
            request = request.with_encode(Some(spec));
        }
        let subtitle_text = self.subtitle.as_ref().map(|plan| {
            let text = plan.crop_for(start, end);
            if text.trim().is_empty() {
                // An empty SRT is not a valid input. A single empty cue keeps
                // the subtitle stream present so every part maps the same
                // streams during concat.
                EMPTY_SUBTITLE.to_owned()
            } else {
                text
            }
        });
        let subtitle_path = match subtitle_text {
            Some(text) => {
                let path = self.work_dir.path().join(format!("part-{index:04}.srt"));
                fs::write(&path, text).map_err(MultiExportError::TempFile)?;
                Some(path)
            }
            None => None,
        };
        if self.subtitle.is_some() {
            // The SRT input is only mapped when the request says subtitles are
            // replaced. Without this the cut part keeps the source subtitle
            // packets, whose timestamps are still absolute.
            request = request.with_subtitle(self.subtitle.clone());
        }
        let args = super::build_args(&request, None, subtitle_path.as_deref());
        let process = ExportProcess::start_with_args(args.clone(), &path)
            .map_err(MultiExportError::Export)?;
        self.commands.push(args);
        self.stage = Some(Stage::Cutting { index, process });
        Ok(())
    }

    /// Probes the cut parts for their first and last video timestamps.
    ///
    /// Stream copy lands on packet boundaries, so a part's span is usually
    /// longer than its request. The concat offsets use these real timestamps.
    fn start_probing_timings(&mut self) -> Result<(), MultiExportError> {
        let parts: Vec<(PathBuf, Duration)> = self
            .parts
            .iter()
            .map(|part| (part.path.clone(), part.duration()))
            .collect();
        let (sender, receiver) = mpsc::channel();
        let spawned = thread::Builder::new()
            .name("export-timing-probe".to_owned())
            .spawn(move || {
                let parts = parts
                    .into_iter()
                    .map(|(path, requested)| MultiExport::probe_timing(&path, requested))
                    .collect();
                // The receiver is dropped when the export is cancelled.
                let _ = sender.send(ProbedTimings { parts });
            });
        if let Err(error) = spawned {
            log::warn!("could not start the timing probe thread: {error}; using requested lengths");
            self.use_requested_timings();
            self.refresh_durations()?;
            return self.start_concat();
        }
        self.stage = Some(Stage::ProbingTimings { timings: receiver });
        Ok(())
    }

    fn probe_timing(path: &Path, fallback: Duration) -> PartTiming {
        let bounds = ffprobe::probe_packet_bounds(path, "v:0")
            .ok()
            .flatten()
            .or_else(|| ffprobe::probe_packet_bounds(path, "a:0").ok().flatten());
        match bounds {
            Some(bounds) if bounds.end > bounds.first => PartTiming {
                first: bounds.first,
                end: bounds.end,
            },
            _ => PartTiming {
                first: Timestamp::ZERO,
                end: Timestamp::from_micros(i64::try_from(fallback.micros()).unwrap_or(0)),
            },
        }
    }

    fn use_requested_timings(&mut self) {
        for (timing, part) in self.part_timings.iter_mut().zip(&self.parts) {
            *timing = PartTiming {
                first: Timestamp::ZERO,
                end: Timestamp::from_micros(i64::try_from(part.duration().micros()).unwrap_or(0)),
            };
        }
    }

    /// Concat offset for one part: the distance from the next part's first
    /// video packet to this part's end, so the next frame follows immediately.
    fn concat_duration(&self, index: usize) -> Duration {
        let timing = self.part_timings[index];
        let Some(next) = self.part_timings.get(index + 1) else {
            return timing.span();
        };
        timing
            .end
            .duration_since(next.first)
            .filter(|duration| duration.micros() > 0)
            .unwrap_or_else(|| timing.span())
    }

    /// Recomputes per-segment and total durations from the cut parts.
    ///
    /// Smart cut plans rely on a copy piece starting at its planned keyframe.
    /// Sparse Matroska cue points make FFmpeg seek to an earlier keyframe, which
    /// silently duplicates content, so fail instead of exporting a wrong cut.
    fn refresh_durations(&mut self) -> Result<(), MultiExportError> {
        if let Some(context) = self.smart_cut.as_ref() {
            // Copy cuts can end a few frames past the request because the
            // packet that crosses the boundary is kept. A sparse seek point
            // lands seconds earlier, so a generous tolerance still separates
            // the two cases.
            let tolerance =
                Duration::from_micros(context.frame_duration.micros().saturating_mul(4));
            for (index, part) in self.parts.iter().enumerate() {
                if part.encode {
                    continue;
                }
                let span = self.part_timings[index].span();
                if span.micros() > part.duration().micros().saturating_add(tolerance.micros()) {
                    return Err(MultiExportError::SmartCutSeek {
                        index: part.segment,
                    });
                }
            }
        }

        self.part_durations = vec![Duration::ZERO; self.segments.len()];
        for index in 0..self.parts.len() {
            let segment = self.parts[index].segment;
            let duration = self.concat_duration(index);
            self.part_durations[segment] = self.part_durations[segment]
                .checked_add(duration)
                .unwrap_or(self.part_durations[segment]);
        }
        self.total_duration = self
            .part_durations
            .iter()
            .fold(Duration::ZERO, |total, duration| {
                total.checked_add(*duration).unwrap_or(total)
            });
        self.completed_duration = Duration::ZERO;
        Ok(())
    }

    fn start_concat(&mut self) -> Result<(), MultiExportError> {
        let list_path = self.work_dir.path().join("concat.txt");
        let mut list = String::new();
        for (index, part) in self.parts.iter().enumerate() {
            list.push_str(&concat_line(&part.path));
            list.push('\n');
            list.push_str(&format!(
                "duration {}\n",
                format_seconds(self.concat_duration(index))
            ));
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
                let title = self
                    .chapter_titles
                    .get(index)
                    .filter(|title| !title.trim().is_empty())
                    .cloned()
                    .unwrap_or_else(|| format!("Segment {}", index + 1));
                chapters.push(ChapterSpec { start, end, title });
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
            self.smart_cut.is_none(),
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
                Stage::Done
                | Stage::Failed
                | Stage::Planning { .. }
                | Stage::ProbingTimings { .. } => {}
            }
        }
    }
}

fn concat_args(
    list: &Path,
    output: &Path,
    metadata: Option<&Path>,
    title: Option<&str>,
    rebase_negative: bool,
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

    args.extend(["-ignore_unknown".into()]);
    if rebase_negative {
        args.extend(["-avoid_negative_ts".into(), "make_zero".into()]);
    }
    args.extend(["-y".into(), output.as_os_str().to_owned()]);

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

fn format_seconds(duration: Duration) -> String {
    let micros = duration.micros();
    let seconds = micros / 1_000_000;
    let fraction = micros % 1_000_000;
    format!("{seconds}.{fraction:06}")
}

pub(super) struct TempDir {
    path: PathBuf,
}

impl TempDir {
    pub(super) fn create() -> io::Result<Self> {
        // Parallel exports in one process can share a nanosecond stamp.
        static COUNTER: AtomicU64 = AtomicU64::new(0);

        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?;
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "rossresscut-export-{}-{}-{unique}",
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
