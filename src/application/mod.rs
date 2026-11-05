//! Application layer: user commands, application state, and background jobs.
//!
//! The UI sends commands and polls [`Application::update`]. FFprobe, preview
//! decoding, and audio playback all run outside the UI thread.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::Instant;

use crate::domain::media::{KeyframeIndex, MediaFile, MediaStream};
use crate::domain::segment::{Segment, SegmentId, Timeline};
use crate::domain::time::{Duration, Timestamp};
use crate::domain::waveform::Waveform;
use crate::media::export::{
    ChapterSpec, ExportProcess, ExportRequest, ExportSegment, MultiExport, MultiExportRequest,
    StreamSelection, SubtitlePlan,
};
use crate::media::ffprobe::ProbeError;
use crate::media::preview::{HardwareAcceleration, PreviewAudio, PreviewVideo, VideoFrame};

mod batch;
mod chapters;
mod editing;
mod export;
mod media;
mod playback;
mod project;
mod scene;
mod silence;
mod transport;
mod waveform;

#[cfg(feature = "mpv-preview")]
use crate::media::preview::{MpvPreview, MpvPreviewError};
pub use batch::{BatchItem, BatchItemStatus, BatchMerge, BatchRequest, BatchSkipReason};
pub use export::{ExportCommandError, ExportStage, ExportStatus};
use export::{ExportJob, JobEvent};
#[cfg(feature = "mpv-preview")]
use playback::{fit_within, preview_video_stream};
pub use project::{PROJECT_EXTENSION, ProjectError};
pub use scene::{SceneCutError, SceneScan};
pub use silence::{SilenceError, SilenceOptions};

/// State of the media-opening workflow.
#[derive(Debug, Default)]
pub enum OpenState {
    /// No media has been requested yet.
    #[default]
    Empty,
    /// A probe is running in the background.
    Probing {
        /// Media file currently being probed.
        path: PathBuf,
    },
    /// The current media was probed successfully.
    Ready,
    /// The most recent open request failed.
    Failed {
        /// Media file that could not be opened.
        path: PathBuf,
        /// User-facing failure description.
        message: String,
    },
}

/// Errors produced by segment editing commands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegmentCommandError {
    /// No media is open.
    NoMedia,
    /// The in point has not been set.
    MissingInPoint,
    /// The out point has not been set.
    MissingOutPoint,
    /// The requested range is empty or reversed.
    EmptyRange,
    /// No segment is selected.
    NoSelection,
    /// The segment identifier counter is exhausted.
    IdExhausted,
}

impl fmt::Display for SegmentCommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NoMedia => "open a media file before editing segments",
            Self::MissingInPoint => "set an in point first",
            Self::MissingOutPoint => "set an out point first",
            Self::EmptyRange => "the segment end must be after its start",
            Self::NoSelection => "select a segment first",
            Self::IdExhausted => "segment identifier space exhausted",
        })
    }
}

impl std::error::Error for SegmentCommandError {}

/// Temporary in / out markers used to create a segment.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Markers {
    pub(super) in_point: Option<Timestamp>,
    pub(super) out_point: Option<Timestamp>,
}

/// Owns application state and coordinates background work.
#[derive(Default)]
pub struct Application {
    media: Option<MediaFile>,
    state: OpenState,
    probe: Option<ProbeJob>,
    keyframes: Option<KeyframeIndex>,
    keyframe_job: Option<KeyframeJob>,
    waveform: Option<Waveform>,
    waveform_job: Option<WaveformJob>,
    scene_scan: Option<SceneScan>,
    scene_job: Option<SceneJob>,
    batch_items: Vec<BatchItem>,
    batch_job: Option<batch::BatchJob>,
    batch_merge: Option<batch::BatchMerge>,
    playback: Playback,
    timeline: Timeline,
    markers: Markers,
    selected_segment: Option<SegmentId>,
    selection: BTreeSet<SegmentId>,
    clipboard: Vec<Segment>,
    chapter_titles: BTreeMap<SegmentId, String>,
    history: Vec<TimelineSnapshot>,
    redo_stack: Vec<TimelineSnapshot>,
    rng_state: u64,
    last_ffmpeg_commands: Vec<String>,
    export: Option<ExportJob>,
    export_status: ExportStatus,
    pending_project: Option<PendingProject>,
    timeline_revision: u64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct TimelineSnapshot {
    segments: Vec<Segment>,
    selected: Option<SegmentId>,
    selection: BTreeSet<SegmentId>,
}

pub(super) struct ProbeJob {
    path: PathBuf,
    receiver: Receiver<Result<MediaFile, ProbeError>>,
}

pub(super) struct KeyframeJob {
    receiver: Receiver<Result<KeyframeIndex, ProbeError>>,
}

pub(super) struct WaveformJob {
    receiver: Receiver<Result<Waveform, crate::media::waveform::WaveformError>>,
    progress: Arc<crate::media::waveform::WaveformProgress>,
}

pub(super) struct SceneJob {
    receiver: Receiver<Result<Vec<Timestamp>, crate::media::scene::SceneError>>,
}

/// Segments waiting to be applied once the project source is probed.
pub(super) struct PendingProject {
    pub(super) segments: Vec<Segment>,
    pub(super) next_id: u64,
    pub(super) chapter_titles: BTreeMap<SegmentId, String>,
}

/// Options applied to one export run.
#[derive(Clone, Debug, Default)]
pub struct ExportOptions {
    /// Whether to generate chapters.
    pub chapters: bool,
    /// Output title metadata.
    pub title: Option<String>,
    /// Whether cues crossing a segment boundary are kept.
    pub subtitle_keep_partial: bool,
    /// Restricts the output to these streams; `None` keeps every stream.
    pub streams: Option<StreamSelection>,
    /// Whether the encoded head of each segment should be re-created.
    pub smart_cut: bool,
    /// Manual re-encode bit rate in kbps; `0` uses the automatic value.
    pub smart_cut_bit_rate_kbps: u32,
}

/// Returns whether `codec_name` is a text subtitle format FFmpeg can convert.
fn is_text_subtitle(codec_name: Option<&str>) -> bool {
    matches!(
        codec_name,
        Some("srt" | "subrip" | "ass" | "ssa" | "mov_text" | "webvtt" | "text")
    )
}

impl Application {
    /// Creates an application with no media loaded.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the currently opened media, if probing succeeded.
    pub fn media(&self) -> Option<&MediaFile> {
        self.media.as_ref()
    }

    /// Returns the current media-opening state.
    pub fn state(&self) -> &OpenState {
        &self.state
    }

    /// Returns the current playback position.
    pub fn position(&self) -> Timestamp {
        self.playback.position
    }

    /// Returns the media duration, when known.
    pub fn duration(&self) -> Option<Duration> {
        self.media.as_ref().and_then(|media| media.duration)
    }

    /// Returns whether playback is running.
    pub fn is_playing(&self) -> bool {
        self.playback.playing
    }

    /// Returns whether the playback clock is currently driven by audio output.
    pub fn uses_audio_clock(&self) -> bool {
        self.playback.audio.is_some() || self.playback.uses_mpv_audio()
    }

    /// Returns whether a seek/load preview frame is still being decoded.
    pub fn is_waiting_for_preview(&self) -> bool {
        !self.playback.playing && (self.playback.video.is_some() || self.playback.mpv_needs_frame())
    }

    /// Returns whether the mpv preview backend is currently driving the preview.
    pub fn uses_mpv_preview(&self) -> bool {
        self.playback.mpv_present()
    }

    /// Enables the mpv preview backend for subsequently opened media.
    ///
    /// When the feature is disabled this is a no-op and the external FFmpeg
    /// preview remains in use.
    pub fn enable_mpv_preview(&mut self) {
        #[cfg(feature = "mpv-preview")]
        {
            self.playback.mpv_enabled = true;
        }
    }

    /// Takes the newest exported mpv frame as a wgpu texture.
    ///
    /// The caller owns the texture for as long as egui needs it. This method
    /// is only meaningful while [`Self::uses_mpv_preview`] is true.
    #[cfg(feature = "mpv-preview")]
    pub fn acquire_mpv_texture(
        &mut self,
        device: &wgpu::Device,
    ) -> Result<Option<wgpu::Texture>, MpvPreviewError> {
        if self.playback.mpv.is_none() {
            self.playback.mpv_seek_gate = None;
        } else if self.playback.mpv_seek_gate.as_ref().is_some_and(|gate| {
            !gate.target_reached
                || gate
                    .target_reached_at
                    .is_none_or(|at| at.elapsed() < SEEK_TARGET_SETTLE)
        }) {
            // Drop nothing and publish nothing: the UI keeps its previous
            // texture until mpv confirms the seek target and has settled.
            return Ok(None);
        }

        let texture = match self.playback.mpv.as_ref() {
            Some(mpv) => mpv.acquire_texture(device)?,
            None => None,
        };
        if texture.is_some() {
            self.playback.mpv_frame_ready = true;
            self.playback.mpv_seek_gate = None;
        }
        Ok(texture)
    }

    /// Returns whether the preview is waiting for the first frame after a
    /// precise seek. The UI must not paint the previous texture while true.
    pub fn is_preview_stale(&self) -> bool {
        #[cfg(feature = "mpv-preview")]
        {
            self.playback.mpv_seek_gate.is_some()
        }
        #[cfg(not(feature = "mpv-preview"))]
        {
            false
        }
    }

    /// Sets the preview pane size in physical pixels.
    ///
    /// mpv renders into a texture fitted to these bounds: the source aspect
    /// ratio is preserved and the result never exceeds the source resolution,
    /// so a small pane costs less scaling and a large one is not upscaled.
    /// The size is cached and only forwarded to mpv when it actually changes.
    #[cfg(feature = "mpv-preview")]
    pub fn set_preview_bounds(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        let bounds = (width, height);
        if self.playback.preview_bounds == Some(bounds) {
            return;
        }
        self.playback.preview_bounds = Some(bounds);
        self.apply_mpv_export_size();
    }

    /// Forwards the fitted preview size to mpv when it differs from the size
    /// that is currently rendering.
    #[cfg(feature = "mpv-preview")]
    fn apply_mpv_export_size(&mut self) {
        let Some(size) = self.mpv_target_size() else {
            return;
        };
        let Some(mpv) = self.playback.mpv.as_ref() else {
            return;
        };
        if self.playback.mpv_size == Some(size) {
            return;
        }
        match mpv.set_export_size(size.0, size.1) {
            Ok(()) => {
                log::info!("mpv preview size {}x{}", size.0, size.1);
                self.playback.mpv_size = Some(size);
                self.playback.mpv_frame_ready = false;
            }
            Err(error) => {
                log::warn!(
                    "failed to set mpv preview size to {}x{}: {error}",
                    size.0,
                    size.1
                );
            }
        }
    }

    /// Returns the source size fitted into the current preview bounds.
    #[cfg(feature = "mpv-preview")]
    fn mpv_target_size(&self) -> Option<(u32, u32)> {
        let media = self.media.as_ref()?;
        let video = preview_video_stream(media)?;
        let source = (video.width?, video.height?);
        let bounds = self.playback.preview_bounds?;
        Some(fit_within(source, bounds))
    }

    /// Returns the latest preview frame.
    pub fn frame(&self) -> Option<&VideoFrame> {
        self.playback.frame.as_ref()
    }

    /// Returns the dimensions of the latest preview frame.
    pub fn frame_size(&self) -> Option<(u32, u32)> {
        self.playback
            .frame
            .as_ref()
            .map(|frame| (frame.width, frame.height))
            .or_else(|| self.playback.mpv_frame_size())
    }

    /// Returns the timestamp of the latest preview frame.
    pub fn frame_timestamp(&self) -> Option<Timestamp> {
        self.playback
            .frame
            .as_ref()
            .map(|frame| frame.timestamp)
            .or_else(|| {
                self.playback
                    .mpv_present()
                    .then_some(self.playback.position)
            })
    }

    /// Returns a non-fatal preview warning, if any.
    pub fn warning(&self) -> Option<&str> {
        self.playback.warning.as_deref()
    }

    /// Returns the keyframe index for the preview video stream, once loaded.
    pub fn keyframes(&self) -> Option<&KeyframeIndex> {
        self.keyframes.as_ref()
    }

    /// Returns the decoder backend in use for the preview.
    ///
    /// The mpv path returns `mpv-engine`; the external FFmpeg fallback
    /// returns the requested hardware accelerator or `None` for software.
    pub fn hardware_decoder(&self) -> Option<&'static str> {
        if self.playback.mpv_present() {
            return Some("mpv-engine");
        }
        self.playback.video.as_ref().map_or_else(
            || self.playback.hardware.name(),
            |video| video.hardware().name(),
        )
    }

    /// Decoder reported by the active preview backend.
    ///
    /// The mpv path returns what libmpv actually selected, for example
    /// `vaapi`, `vaapi-copy`, `d3d11va`, `videotoolbox` or `no` for software.
    /// The external FFmpeg fallback returns the requested accelerator.
    pub fn decoder_name(&self) -> Option<String> {
        #[cfg(feature = "mpv-preview")]
        if self.playback.mpv_present() {
            return Some(
                self.playback
                    .mpv_decoder
                    .clone()
                    .unwrap_or_else(|| "libmpv".to_owned()),
            );
        }
        self.playback.video.as_ref().map_or_else(
            || self.playback.hardware.name().map(str::to_owned),
            |video| video.hardware().name().map(str::to_owned),
        )
    }

    /// Overrides the hardware backend used by subsequently started preview
    /// decoders.
    ///
    /// Intended for diagnostics and future settings. It only affects the
    /// external FFmpeg fallback path; mpv selects its own hardware decoder.
    /// It does not restart a decoder that is already running; the next
    /// play/seek uses the new value.
    pub fn set_preview_hardware(&mut self, hardware: HardwareAcceleration) {
        self.playback.hardware = hardware;
    }

    /// Playback volume in percent.
    pub fn volume(&self) -> u8 {
        self.playback.volume
    }

    /// Sets the playback volume in percent, clamped to `0..=100`.
    ///
    /// The value is stored even when no mpv backend is running yet; the next
    /// mpv preview start applies it.
    pub fn set_volume(&mut self, volume: u8) {
        self.playback.set_volume(volume);
    }

    /// Returns the current editing timeline.
    pub fn timeline(&self) -> &Timeline {
        &self.timeline
    }

    /// Returns the export state.
    pub fn export_status(&self) -> &ExportStatus {
        &self.export_status
    }

    /// Extracts the first text subtitle track for cropping.
    ///
    /// Returns `None` when the media has no text subtitle, or when FFmpeg
    /// cannot convert it. FFmpeg runs synchronously here because subtitle
    /// tracks are small.
    fn prepare_subtitles(&self, keep_partial: bool) -> Option<SubtitlePlan> {
        let media = self.media.as_ref()?;
        let index = media.streams.iter().find_map(|stream| match stream {
            MediaStream::Subtitle(subtitle)
                if is_text_subtitle(subtitle.info.codec_name.as_deref()) =>
            {
                Some(subtitle.info.index)
            }
            _ => None,
        })?;

        match crate::media::subtitle::extract(&media.path, index) {
            Ok(srt) if !srt.trim().is_empty() => Some(SubtitlePlan { srt, keep_partial }),
            Ok(_) => None,
            Err(error) => {
                log::warn!("{error}");
                None
            }
        }
    }

    /// Exports every enabled segment in timeline order to `output`.
    ///
    /// A single segment uses one FFmpeg process. Multiple segments cut each
    /// part to a temporary file and join them with the concat demuxer. Both
    /// paths use stream copy. Seeking uses an input seek, so each segment may
    /// start at the keyframe before its requested start.
    pub fn start_export(
        &mut self,
        output: PathBuf,
        options: ExportOptions,
    ) -> Result<(), ExportCommandError> {
        if !matches!(self.state, OpenState::Ready) {
            return Err(ExportCommandError::NoMedia);
        }
        if self.export.is_some() {
            return Err(ExportCommandError::AlreadyRunning);
        }

        let source = self
            .timeline
            .source()
            .map(Path::to_path_buf)
            .ok_or(ExportCommandError::NoMedia)?;
        let subtitle = self.prepare_subtitles(options.subtitle_keep_partial);
        let smart_cut = if options.smart_cut {
            Some(self.smart_cut_context(options.streams.as_ref(), options.smart_cut_bit_rate_kbps)?)
        } else {
            None
        };

        let segments: Vec<_> = self
            .timeline
            .segments()
            .iter()
            .copied()
            .filter(|segment| segment.enabled())
            .collect();
        if segments.is_empty() {
            return Err(ExportCommandError::NoSegment);
        }

        // Chapters use the actual container length of each produced part, which
        // is only known after cutting. A single segment therefore takes the
        // multi-segment path when chapters are requested.
        if segments.len() == 1 && smart_cut.is_none() && !options.chapters {
            let segment = segments[0];
            let request =
                ExportRequest::new(source, segment.start(), segment.end(), output.clone())
                    .ok_or(ExportCommandError::InvalidOutput)?
                    .with_title(options.title)
                    .with_subtitle(subtitle)
                    .with_streams(options.streams.clone());
            let duration = request.duration();
            let request = if options.chapters {
                let title = self.export_chapter_title(segment.id(), 0);
                request.with_chapters(vec![ChapterSpec {
                    start: Duration::ZERO,
                    end: duration,
                    title,
                }])
            } else {
                request
            };
            let process = ExportProcess::start(&request).map_err(ExportCommandError::Start)?;

            self.export_status = ExportStatus::Running {
                output: output.clone(),
                out_time: Timestamp::ZERO,
                duration,
                stage: None,
            };
            self.export = Some(ExportJob::single(output, duration, process));
            return Ok(());
        }

        let export_segments: Vec<ExportSegment> = segments
            .iter()
            .map(|segment| {
                ExportSegment::new(segment.start(), segment.end())
                    .ok_or(ExportCommandError::InvalidOutput)
            })
            .collect::<Result<_, _>>()?;
        let chapter_titles: Vec<String> = segments
            .iter()
            .enumerate()
            .map(|(index, segment)| self.export_chapter_title(segment.id(), index))
            .collect();
        let request = MultiExportRequest::new(source, export_segments, output.clone())
            .ok_or(ExportCommandError::InvalidOutput)?
            .with_chapters(options.chapters)
            .with_chapter_titles(chapter_titles)
            .with_title(options.title)
            .with_subtitle(subtitle)
            .with_streams(options.streams)
            .with_smart_cut(smart_cut);
        let duration = request.total_duration();
        let process = MultiExport::start(request).map_err(ExportCommandError::StartMulti)?;

        self.export_status = ExportStatus::Running {
            output: output.clone(),
            out_time: Timestamp::ZERO,
            duration,
            stage: None,
        };
        self.export = Some(ExportJob::multi(output, duration, process));
        Ok(())
    }

    /// Requests cancellation of the running export.
    ///
    /// The status becomes [`ExportStatus::Cancelled`] after the process exits.
    pub fn cancel_export(&mut self) {
        if let Some(job) = self.export.as_mut() {
            job.cancel_requested = true;
            job.cancel();
        }
    }

    /// Returns the FFmpeg command lines produced by the latest export.
    pub fn last_ffmpeg_commands(&self) -> &[String] {
        &self.last_ffmpeg_commands
    }

    /// Polls background work and advances playback without blocking.
    ///
    /// Returns `true` when application state changed, so the UI can repaint.
    pub fn update(&mut self) -> bool {
        let mut changed = self.poll_probe();
        self.poll_keyframes();
        self.poll_waveform();
        self.poll_scene_scan();
        self.poll_batch();
        changed |= self.poll_export();
        let duration = self.duration();
        changed |= self.playback.update(duration);
        changed
    }

    fn poll_export(&mut self) -> bool {
        let mut changed = false;
        let mut progress = None;
        let mut finished = None;
        let mut commands = Vec::new();

        if let Some(job) = self.export.as_mut() {
            loop {
                match job.poll() {
                    None => break,
                    Some(JobEvent::Progress {
                        out_time,
                        duration,
                        stage,
                    }) => {
                        job.out_time = out_time;
                        progress = Some((job.output.clone(), out_time, duration, stage));
                    }
                    Some(JobEvent::Finished { error }) => {
                        finished = Some((job.output.clone(), job.cancel_requested, error));
                        break;
                    }
                }
            }
            commands = job.commands();
        }

        if !commands.is_empty() {
            self.last_ffmpeg_commands = commands;
        }

        if let Some((output, out_time, duration, stage)) = progress {
            self.export_status = ExportStatus::Running {
                output,
                out_time,
                duration,
                stage,
            };
            changed = true;
        }

        if let Some((output, cancelled, error)) = finished {
            self.export = None;
            self.export_status = if cancelled {
                ExportStatus::Cancelled
            } else {
                match error {
                    None => ExportStatus::Completed { output },
                    Some(message) => ExportStatus::Failed { output, message },
                }
            };
            changed = true;
        }

        changed
    }
}

/// Tracks a precise mpv seek that has not delivered its target frame yet.
///
/// While the gate is closed, the UI keeps showing the previous texture instead
/// of frames mpv publishes mid-seek. The gate opens once mpv reports the target
/// position or `PlaybackRestart`, and the first texture acquired after that is
/// accepted; the timeout is a safety net so a failed seek cannot freeze the
/// preview indefinitely.
#[cfg(feature = "mpv-preview")]
struct MpvSeekGate {
    target: Timestamp,
    started: Instant,
    target_reached: bool,
    target_reached_at: Option<Instant>,
    position_matched_at: Option<Instant>,
}

/// Delay between confirming the seek target and accepting a texture. mpv can
/// publish a pre-seek frame between `PlaybackRestart` and the target frame.
#[cfg(feature = "mpv-preview")]
const SEEK_TARGET_SETTLE: std::time::Duration = std::time::Duration::from_millis(50);

struct Playback {
    position: Timestamp,
    playing: bool,
    last_update: Instant,
    video: Option<PreviewVideo>,
    frame: Option<VideoFrame>,
    pending_frame: Option<VideoFrame>,
    audio: Option<PreviewAudio>,
    warning: Option<String>,
    hardware: HardwareAcceleration,
    volume: u8,
    #[cfg(feature = "mpv-preview")]
    mpv: Option<MpvPreview>,
    #[cfg(feature = "mpv-preview")]
    mpv_enabled: bool,
    #[cfg(feature = "mpv-preview")]
    mpv_failed: bool,
    #[cfg(feature = "mpv-preview")]
    mpv_has_audio: bool,
    #[cfg(feature = "mpv-preview")]
    mpv_size: Option<(u32, u32)>,
    #[cfg(feature = "mpv-preview")]
    preview_bounds: Option<(u32, u32)>,
    #[cfg(feature = "mpv-preview")]
    mpv_pending_seek: Option<(Timestamp, bool)>,
    #[cfg(feature = "mpv-preview")]
    mpv_seek_gate: Option<MpvSeekGate>,
    #[cfg(feature = "mpv-preview")]
    mpv_frame_ready: bool,
    #[cfg(feature = "mpv-preview")]
    mpv_decoder: Option<String>,
    #[cfg(feature = "mpv-preview")]
    mpv_decoder_checked_at: Option<Instant>,
}

impl Default for Playback {
    fn default() -> Self {
        Self {
            position: Timestamp::ZERO,
            playing: false,
            last_update: Instant::now(),
            video: None,
            frame: None,
            pending_frame: None,
            audio: None,
            warning: None,
            hardware: HardwareAcceleration::default_for_platform(),
            volume: 100,
            #[cfg(feature = "mpv-preview")]
            mpv: None,
            #[cfg(feature = "mpv-preview")]
            mpv_enabled: false,
            #[cfg(feature = "mpv-preview")]
            mpv_failed: false,
            #[cfg(feature = "mpv-preview")]
            mpv_has_audio: false,
            #[cfg(feature = "mpv-preview")]
            mpv_size: None,
            #[cfg(feature = "mpv-preview")]
            preview_bounds: None,
            #[cfg(feature = "mpv-preview")]
            mpv_pending_seek: None,
            #[cfg(feature = "mpv-preview")]
            mpv_seek_gate: None,
            #[cfg(feature = "mpv-preview")]
            mpv_frame_ready: false,
            #[cfg(feature = "mpv-preview")]
            mpv_decoder: None,
            #[cfg(feature = "mpv-preview")]
            mpv_decoder_checked_at: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::playback::clamp_position;
    use super::*;

    #[test]
    fn empty_path_fails_without_starting_a_worker() {
        let mut application = Application::new();

        application.open_media(PathBuf::new());

        match application.state() {
            OpenState::Failed { message, .. } => {
                assert!(message.contains("no media path"));
            }
            other => panic!("expected a failure state, got {other:?}"),
        }
        assert!(application.media().is_none());
    }

    #[test]
    fn updating_without_media_does_not_change_state() {
        let mut application = Application::new();

        assert!(!application.update());
        assert!(matches!(application.state(), OpenState::Empty));
    }

    #[test]
    fn playback_commands_without_media_are_no_ops() {
        let mut application = Application::new();

        application.play();
        application.seek(Timestamp::from_micros(1_000_000));

        assert!(!application.is_playing());
        assert_eq!(application.position(), Timestamp::ZERO);
    }

    #[test]
    fn clamps_seek_positions_to_media_bounds() {
        let duration = Some(Duration::from_micros(2_000_000));

        assert_eq!(
            clamp_position(Timestamp::from_micros(-5), duration),
            Timestamp::ZERO
        );
        assert_eq!(
            clamp_position(Timestamp::from_micros(1_000_000), duration),
            Timestamp::from_micros(1_000_000)
        );
        assert_eq!(
            clamp_position(Timestamp::from_micros(3_000_000), duration),
            Timestamp::from_micros(2_000_000)
        );
        assert_eq!(
            clamp_position(Timestamp::from_micros(3_000_000), None),
            Timestamp::from_micros(3_000_000)
        );
    }

    #[cfg(feature = "mpv-preview")]
    #[test]
    fn fits_landscape_sources_into_the_preview_pane() {
        assert_eq!(fit_within((1920, 1080), (960, 540)), (960, 540));
        assert_eq!(fit_within((1920, 1080), (800, 600)), (800, 450));
        assert_eq!(fit_within((3840, 2160), (1000, 1000)), (1000, 562));
    }

    #[cfg(feature = "mpv-preview")]
    #[test]
    fn fits_portrait_sources_into_the_preview_pane() {
        assert_eq!(fit_within((1080, 1920), (800, 600)), (337, 600));
        assert_eq!(fit_within((1080, 1920), (2000, 2000)), (1080, 1920));
    }

    #[cfg(feature = "mpv-preview")]
    #[test]
    fn never_upscales_beyond_the_source() {
        assert_eq!(fit_within((640, 480), (1920, 1080)), (640, 480));
        assert_eq!(fit_within((640, 480), (640, 480)), (640, 480));
    }

    #[cfg(feature = "mpv-preview")]
    #[test]
    fn degenerate_sizes_are_returned_unchanged() {
        assert_eq!(fit_within((0, 480), (100, 100)), (0, 480));
        assert_eq!(fit_within((640, 0), (100, 100)), (640, 0));
        assert_eq!(fit_within((640, 480), (0, 100)), (640, 480));
        assert_eq!(fit_within((640, 480), (100, 0)), (640, 480));
    }

    #[test]
    fn segment_commands_require_media() {
        let mut application = Application::new();

        assert_eq!(application.add_segment(), Err(SegmentCommandError::NoMedia));
        assert_eq!(application.remove_selected_segment(), None);
        assert_eq!(
            application.set_selected_segment_start(Timestamp::from_micros(1_000)),
            Err(SegmentCommandError::NoSelection)
        );

        application.set_in_point(Timestamp::from_micros(1_000_000));
        assert_eq!(application.in_point(), None);
        assert!(application.timeline().is_empty());
    }

    #[test]
    fn select_segment_ignores_unknown_ids() {
        let mut application = Application::new();

        application.select_segment(Some(SegmentId::new(3)));

        assert_eq!(application.selected_segment(), None);
    }

    #[test]
    fn preview_hardware_can_be_overridden() {
        let mut application = Application::new();

        application.set_preview_hardware(HardwareAcceleration::None);
        assert_eq!(application.hardware_decoder(), None);

        application.set_preview_hardware(HardwareAcceleration::Vaapi);
        assert_eq!(application.hardware_decoder(), Some("vaapi"));
    }

    #[test]
    fn volume_is_clamped_between_zero_and_one_hundred() {
        let mut application = Application::new();
        assert_eq!(application.volume(), 100);

        application.set_volume(42);
        assert_eq!(application.volume(), 42);

        application.set_volume(250);
        assert_eq!(application.volume(), 100);
    }

    #[test]
    fn export_without_media_is_rejected() {
        let mut application = Application::new();

        let error = application
            .start_export(PathBuf::from("out.mkv"), ExportOptions::default())
            .expect_err("export must fail without media");

        assert!(matches!(error, ExportCommandError::NoMedia));
        assert!(!application.export_status().is_running());
    }
}
