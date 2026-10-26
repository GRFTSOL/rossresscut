//! Application layer: user commands, application state, and background jobs.
//!
//! The UI sends commands and polls [`Application::update`]. FFprobe, preview
//! decoding, and audio playback all run outside the UI thread.

use std::fmt;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::Instant;

use crate::domain::media::{KeyframeIndex, MediaFile, MediaStream, VideoStream};
use crate::domain::segment::{Segment, SegmentId, Timeline, TimelineError};
use crate::domain::time::{Duration, Timestamp};
use crate::error::AppError;
use crate::media::ffprobe::{self, ProbeError};
use crate::media::preview::{
    HardwareAcceleration, PreviewAudio, PreviewAudioStatus, PreviewVideo, PreviewVideoConfig,
    PreviewVideoResult, VideoFrame,
};
#[cfg(feature = "mpv-preview")]
use crate::media::preview::{MpvPreview, MpvPreviewConfig, MpvPreviewError};

const PREVIEW_FRAME_RATE: u32 = 30;

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
struct Markers {
    in_point: Option<Timestamp>,
    out_point: Option<Timestamp>,
}

/// Owns application state and coordinates background work.
#[derive(Default)]
pub struct Application {
    media: Option<MediaFile>,
    state: OpenState,
    probe: Option<ProbeJob>,
    keyframes: Option<KeyframeIndex>,
    keyframe_job: Option<KeyframeJob>,
    playback: Playback,
    timeline: Timeline,
    markers: Markers,
    selected_segment: Option<SegmentId>,
}

struct ProbeJob {
    path: PathBuf,
    receiver: Receiver<Result<MediaFile, ProbeError>>,
}

struct KeyframeJob {
    receiver: Receiver<Result<KeyframeIndex, ProbeError>>,
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
        let texture = match self.playback.mpv.as_ref() {
            Some(mpv) => mpv.acquire_texture(device)?,
            None => None,
        };
        if texture.is_some() {
            self.playback.mpv_frame_ready = true;
        }
        Ok(texture)
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

    /// Returns the FFmpeg hardware accelerator in use, or `None` for software
    /// decoding.
    pub fn hardware_decoder(&self) -> Option<&'static str> {
        if self.playback.mpv_present() {
            return Some("mpv-engine");
        }
        self.playback.video.as_ref().map_or_else(
            || self.playback.hardware.name(),
            |video| video.hardware().name(),
        )
    }

    /// Overrides the hardware backend used by subsequently started preview
    /// decoders.
    ///
    /// Intended for diagnostics and future settings. It does not restart a
    /// decoder that is already running; the next play/seek uses the new value.
    pub fn set_preview_hardware(&mut self, hardware: HardwareAcceleration) {
        self.playback.hardware = hardware;
    }

    /// Returns the current editing timeline.
    pub fn timeline(&self) -> &Timeline {
        &self.timeline
    }

    /// Returns the pending in point.
    pub fn in_point(&self) -> Option<Timestamp> {
        self.markers.in_point
    }

    /// Returns the pending out point.
    pub fn out_point(&self) -> Option<Timestamp> {
        self.markers.out_point
    }

    /// Returns the selected segment ID.
    pub fn selected_segment(&self) -> Option<SegmentId> {
        self.selected_segment
    }

    /// Selects a segment by stable ID, or clears the selection with `None`.
    pub fn select_segment(&mut self, id: Option<SegmentId>) {
        self.selected_segment = id.filter(|id| self.timeline.get(*id).is_some());
    }

    /// Sets the in point, clamped to the media bounds.
    pub fn set_in_point(&mut self, point: Timestamp) {
        let Some(media) = self.media.as_ref() else {
            return;
        };
        self.markers.in_point = Some(clamp_position(point, media.duration));
    }

    /// Sets the out point, clamped to the media bounds.
    pub fn set_out_point(&mut self, point: Timestamp) {
        let Some(media) = self.media.as_ref() else {
            return;
        };
        self.markers.out_point = Some(clamp_position(point, media.duration));
    }

    /// Clears both pending markers.
    pub fn clear_markers(&mut self) {
        self.markers = Markers::default();
    }

    /// Creates a segment from the pending in and out points.
    ///
    /// On success the markers are cleared and the new segment is selected.
    /// The segment is appended to the output order.
    pub fn add_segment(&mut self) -> Result<SegmentId, SegmentCommandError> {
        if self.media.is_none() {
            return Err(SegmentCommandError::NoMedia);
        }
        let Some(start) = self.markers.in_point else {
            return Err(SegmentCommandError::MissingInPoint);
        };
        let Some(end) = self.markers.out_point else {
            return Err(SegmentCommandError::MissingOutPoint);
        };

        let id = self
            .timeline
            .push(start, end)
            .map_err(|error| match error {
                TimelineError::InvalidRange => SegmentCommandError::EmptyRange,
                TimelineError::IdExhausted => SegmentCommandError::IdExhausted,
                TimelineError::NotFound => SegmentCommandError::EmptyRange,
            })?;
        self.selected_segment = Some(id);
        self.markers = Markers::default();
        Ok(id)
    }

    /// Removes the selected segment.
    ///
    /// The selection moves to the next segment in output order, or the
    /// previous one when the removed segment was last.
    pub fn remove_selected_segment(&mut self) -> Option<Segment> {
        let id = self.selected_segment?;
        let index = self.timeline.index_of(id)?;
        let removed = self.timeline.remove(id)?;
        self.selected_segment = self
            .timeline
            .segments()
            .get(index)
            .or_else(|| {
                index
                    .checked_sub(1)
                    .and_then(|index| self.timeline.segments().get(index))
            })
            .map(|segment| segment.id());
        Some(removed)
    }

    /// Sets the enabled flag of a segment.
    pub fn set_segment_enabled(&mut self, id: SegmentId, enabled: bool) -> bool {
        self.timeline.set_enabled(id, enabled)
    }

    /// Moves the selected segment one position earlier in output order.
    pub fn move_selected_segment_up(&mut self) -> bool {
        let Some(id) = self.selected_segment else {
            return false;
        };
        self.timeline.move_up(id)
    }

    /// Moves the selected segment one position later in output order.
    pub fn move_selected_segment_down(&mut self) -> bool {
        let Some(id) = self.selected_segment else {
            return false;
        };
        self.timeline.move_down(id)
    }

    /// Moves the selected segment's start to `start`.
    pub fn set_selected_segment_start(
        &mut self,
        start: Timestamp,
    ) -> Result<(), SegmentCommandError> {
        let id = self
            .selected_segment
            .ok_or(SegmentCommandError::NoSelection)?;
        let Some(segment) = self.timeline.get(id).copied() else {
            return Err(SegmentCommandError::NoSelection);
        };
        let start = clamp_position(start, self.duration());
        self.timeline
            .update(id, start, segment.end())
            .map(|_| ())
            .map_err(|error| match error {
                TimelineError::InvalidRange => SegmentCommandError::EmptyRange,
                TimelineError::NotFound => SegmentCommandError::NoSelection,
                TimelineError::IdExhausted => SegmentCommandError::IdExhausted,
            })
    }

    /// Moves the selected segment's end to `end`.
    pub fn set_selected_segment_end(&mut self, end: Timestamp) -> Result<(), SegmentCommandError> {
        let id = self
            .selected_segment
            .ok_or(SegmentCommandError::NoSelection)?;
        let Some(segment) = self.timeline.get(id).copied() else {
            return Err(SegmentCommandError::NoSelection);
        };
        let end = clamp_position(end, self.duration());
        self.timeline
            .update(id, segment.start(), end)
            .map(|_| ())
            .map_err(|error| match error {
                TimelineError::InvalidRange => SegmentCommandError::EmptyRange,
                TimelineError::NotFound => SegmentCommandError::NoSelection,
                TimelineError::IdExhausted => SegmentCommandError::IdExhausted,
            })
    }

    /// Sorts segments by source start time. Selection is preserved because it
    /// is tracked by stable ID.
    pub fn sort_segments(&mut self) {
        self.timeline.sort_by_start();
    }

    /// Starts a background probe for `path`.
    ///
    /// This method only spawns a worker; it never waits for FFprobe. Failures
    /// that prevent the worker from starting are exposed through
    /// [`OpenState::Failed`].
    pub fn open_media(&mut self, path: PathBuf) {
        self.playback.reset();
        self.reset_editing();
        self.keyframes = None;
        self.keyframe_job = None;

        if path.as_os_str().is_empty() {
            self.media = None;
            self.state = OpenState::Failed {
                path,
                message: "no media path was provided".to_owned(),
            };
            return;
        }

        log::info!("probing media file {}", path.display());

        let (sender, receiver) = mpsc::channel();
        let worker_path = path.clone();
        let spawn_result = thread::Builder::new()
            .name("ffprobe".to_owned())
            .spawn(move || {
                let result = ffprobe::probe(&worker_path);
                if sender.send(result).is_err() {
                    log::debug!(
                        "discarding probe result for {}: the request was superseded",
                        worker_path.display()
                    );
                }
            });

        match spawn_result {
            Ok(_handle) => {
                let job_path = path.clone();
                self.media = None;
                self.state = OpenState::Probing { path };
                self.probe = Some(ProbeJob {
                    path: job_path,
                    receiver,
                });
            }
            Err(source) => {
                let error = AppError::io("starting the ffprobe worker thread", source);
                log::error!("{error}");
                self.media = None;
                self.state = OpenState::Failed {
                    path,
                    message: error.to_string(),
                };
            }
        }
    }

    /// Polls background work and advances playback without blocking.
    ///
    /// Returns `true` when application state changed, so the UI can repaint.
    pub fn update(&mut self) -> bool {
        let mut changed = self.poll_probe();
        self.poll_keyframes();
        let duration = self.duration();
        changed |= self.playback.update(duration);
        changed
    }

    /// Starts or resumes playback.
    pub fn play(&mut self) {
        let Some(media) = self.media.as_ref() else {
            return;
        };
        self.playback.play(media);
    }

    /// Pauses playback at the current position.
    pub fn pause(&mut self) {
        let duration = self.duration();
        self.playback.pause(duration);
    }

    /// Toggles between play and pause.
    pub fn toggle_playback(&mut self) {
        if self.playback.playing {
            self.pause();
        } else {
            self.play();
        }
    }

    /// Seeks to `position`, clamped to the media bounds.
    pub fn seek(&mut self, position: Timestamp) {
        let Some(media) = self.media.as_ref() else {
            return;
        };
        let target = clamp_position(position, media.duration);
        self.playback.seek(target, media);
    }

    /// Requests a transient scrub preview at `position`.
    ///
    /// Scrub previews use a fast keyframe seek and may show a frame from before
    /// `position`. Use [`Self::seek`] for an accurate preview and playback
    /// position.
    pub fn scrub(&mut self, position: Timestamp) {
        let Some(media) = self.media.as_ref() else {
            return;
        };
        let target = clamp_position(position, media.duration);
        self.playback.scrub(target, media);
    }

    /// Pauses playback and moves to the previous or next keyframe.
    ///
    /// The keyframe index is built in the background. Until it is available,
    /// this falls back to a one-second step so navigation still responds.
    /// Nothing happens when there is no keyframe in the requested direction.
    pub fn step_keyframe(&mut self, forward: bool) {
        let Some(duration) = self.media.as_ref().map(|media| media.duration) else {
            return;
        };
        let Some(index) = self.keyframes.as_ref() else {
            self.skip(if forward { 1_000_000 } else { -1_000_000 });
            return;
        };

        let position = self.playback.position;
        let target = if forward {
            index.next(position)
        } else {
            index.previous(position)
        };
        let Some(target) = target else {
            return;
        };
        let target = clamp_position(target, duration);

        if self.playback.playing {
            self.pause();
        }
        let Some(media) = self.media.as_ref() else {
            return;
        };
        self.playback.seek(target, media);
    }

    /// Moves the playback position by `delta_micros` and clamps it to the media.
    pub fn skip(&mut self, delta_micros: i64) {
        let Some(media) = self.media.as_ref() else {
            return;
        };
        let Some(position) = self.playback.position.micros().checked_add(delta_micros) else {
            log::warn!("relative seek overflowed; ignoring it");
            return;
        };
        let target = clamp_position(Timestamp::from_micros(position), media.duration);
        self.playback.seek(target, media);
    }

    /// Closes the current media and stops all preview work.
    pub fn close_media(&mut self) {
        self.playback.reset();
        self.reset_editing();
        self.media = None;
        self.probe = None;
        self.keyframes = None;
        self.keyframe_job = None;
        self.state = OpenState::Empty;
    }

    fn reset_editing(&mut self) {
        self.timeline.clear();
        self.markers = Markers::default();
        self.selected_segment = None;
    }

    fn poll_probe(&mut self) -> bool {
        let Some(job) = self.probe.take() else {
            return false;
        };

        match job.receiver.try_recv() {
            Ok(Ok(media)) => {
                log::info!(
                    "opened {} ({} streams, {} chapters)",
                    media.path.display(),
                    media.streams.len(),
                    media.chapters.len()
                );
                self.playback.load(&media);
                self.reset_editing();
                self.timeline.reset(media.path.clone());
                self.start_keyframes(&media);
                self.media = Some(media);
                self.state = OpenState::Ready;
                true
            }
            Ok(Err(error)) => {
                log::warn!("{error}");
                self.playback.reset();
                self.reset_editing();
                self.media = None;
                self.keyframes = None;
                self.keyframe_job = None;
                self.state = OpenState::Failed {
                    path: job.path,
                    message: error.to_string(),
                };
                true
            }
            Err(TryRecvError::Empty) => {
                self.probe = Some(job);
                false
            }
            Err(TryRecvError::Disconnected) => {
                let message = "the ffprobe worker stopped without returning a result".to_owned();
                log::error!("probing {}: {message}", job.path.display());
                self.playback.reset();
                self.reset_editing();
                self.media = None;
                self.keyframes = None;
                self.keyframe_job = None;
                self.state = OpenState::Failed {
                    path: job.path,
                    message,
                };
                true
            }
        }
    }

    fn start_keyframes(&mut self, media: &MediaFile) {
        let Some(video) = preview_video_stream(media) else {
            return;
        };
        let path = media.path.clone();
        let stream_index = video.info.index;
        let start_time = media.start_time;
        let (sender, receiver) = mpsc::channel();

        let spawn_result = thread::Builder::new()
            .name("ffprobe-keyframes".to_owned())
            .spawn(move || {
                let result = ffprobe::probe_keyframes(&path, stream_index, start_time);
                if sender.send(result).is_err() {
                    log::debug!(
                        "discarding keyframe index for {}: the request was superseded",
                        path.display()
                    );
                }
            });

        match spawn_result {
            Ok(_handle) => self.keyframe_job = Some(KeyframeJob { receiver }),
            Err(error) => {
                log::warn!("failed to start the keyframe worker: {error}");
            }
        }
    }

    fn poll_keyframes(&mut self) {
        let Some(job) = self.keyframe_job.take() else {
            return;
        };

        match job.receiver.try_recv() {
            Ok(Ok(index)) => {
                log::info!("indexed {} keyframes", index.len());
                self.keyframes = Some(index);
            }
            Ok(Err(error)) => log::warn!("keyframe indexing failed: {error}"),
            Err(TryRecvError::Empty) => self.keyframe_job = Some(job),
            Err(TryRecvError::Disconnected) => {
                log::warn!("the keyframe worker stopped without a result");
            }
        }
    }
}

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
    mpv_frame_ready: bool,
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
            mpv_frame_ready: false,
        }
    }
}

impl Playback {
    #[cfg(feature = "mpv-preview")]
    fn start_mpv(&mut self, media: &MediaFile, start_paused: bool) -> bool {
        if !self.mpv_enabled || self.mpv_failed {
            return false;
        }
        let Some(video) = preview_video_stream(media) else {
            return false;
        };
        let (Some(source_width), Some(source_height)) = (video.width, video.height) else {
            return false;
        };
        let (width, height) = match self.preview_bounds {
            Some(bounds) => fit_within((source_width, source_height), bounds),
            None => (source_width, source_height),
        };
        let config = MpvPreviewConfig {
            path: media.path.clone(),
            width,
            height,
            start_paused,
        };
        // The UI polls at 16/50 ms while mpv is active; no egui dependency is
        // allowed in this layer, so no repaint callback is installed here.
        match MpvPreview::start(config, || {}, || {}) {
            Ok(mpv) => {
                log::info!(
                    "using mpv preview backend for {} at {width}x{height}",
                    media.path.display()
                );
                self.mpv = Some(mpv);
                self.mpv_has_audio = has_audio(media);
                self.mpv_size = Some((width, height));
                true
            }
            Err(error) => {
                log::warn!("{error}");
                self.mpv_failed = true;
                self.warning = Some(error.to_string());
                false
            }
        }
    }

    #[cfg(not(feature = "mpv-preview"))]
    fn start_mpv(&mut self, _media: &MediaFile, _start_paused: bool) -> bool {
        false
    }

    fn mpv_present(&self) -> bool {
        #[cfg(feature = "mpv-preview")]
        {
            self.mpv.is_some()
        }
        #[cfg(not(feature = "mpv-preview"))]
        {
            false
        }
    }

    fn uses_mpv_audio(&self) -> bool {
        #[cfg(feature = "mpv-preview")]
        {
            self.mpv_has_audio
        }
        #[cfg(not(feature = "mpv-preview"))]
        {
            false
        }
    }

    fn mpv_needs_frame(&self) -> bool {
        #[cfg(feature = "mpv-preview")]
        {
            self.mpv.is_some() && !self.mpv_frame_ready
        }
        #[cfg(not(feature = "mpv-preview"))]
        {
            false
        }
    }

    fn mpv_frame_size(&self) -> Option<(u32, u32)> {
        #[cfg(feature = "mpv-preview")]
        {
            self.mpv_size
        }
        #[cfg(not(feature = "mpv-preview"))]
        {
            None
        }
    }

    fn stop_mpv(&mut self) {
        #[cfg(feature = "mpv-preview")]
        {
            self.mpv = None;
            self.mpv_has_audio = false;
            self.mpv_size = None;
            self.mpv_pending_seek = None;
            self.mpv_frame_ready = false;
        }
    }

    #[cfg(feature = "mpv-preview")]
    fn update_mpv(&mut self, duration: Option<Duration>) -> bool {
        let Some(mpv) = self.mpv.as_ref() else {
            return false;
        };
        let events = mpv.poll_events();
        let position = mpv.position();
        let mut changed = false;

        if let Some(position) = position
            && position != self.position
        {
            self.position = position;
            changed = true;
        }
        if events.loaded || events.restarted {
            changed = true;
            if let Some((target, precise)) = self.mpv_pending_seek.take() {
                log::info!("applying deferred mpv seek to {target}");
                self.mpv_frame_ready = false;
                let _ = mpv
                    .set_precise_seek(precise)
                    .and_then(|()| mpv.seek(target));
            }
        }
        let failed = events.failed.clone();
        if let Some(message) = failed.as_deref() {
            log::warn!("mpv preview failed: {message}");
            self.warning = Some(format!("mpv preview failed: {message}"));
            self.playing = false;
            changed = true;
        }
        if events.ended {
            self.playing = false;
            if let Some(end) = duration_end(duration) {
                self.position = Timestamp::from_micros(end);
            }
            changed = true;
        }
        if failed.is_some() {
            self.stop_mpv();
        }
        changed
    }

    fn update(&mut self, duration: Option<Duration>) -> bool {
        #[cfg(feature = "mpv-preview")]
        if self.mpv.is_some() {
            return self.update_mpv(duration);
        }

        let mut changed = false;
        if self.playing {
            changed |= self.poll_audio(duration);
            changed |= self.advance(duration);
        }
        changed |= self.poll_video(duration);
        changed
    }

    fn play(&mut self, media: &MediaFile) {
        if self.playing {
            return;
        }

        if duration_end(media.duration).is_some_and(|end| self.position.micros() >= end) {
            self.position = Timestamp::ZERO;
            self.frame = None;
            #[cfg(feature = "mpv-preview")]
            if let Some(mpv) = &self.mpv {
                let _ = mpv.set_precise_seek(true);
                let _ = mpv.seek(Timestamp::ZERO);
            }
        }

        self.pending_frame = None;
        self.warning = None;

        #[cfg(feature = "mpv-preview")]
        if let Some(mpv) = &self.mpv {
            if let Err(error) = mpv.set_paused(false) {
                log::warn!("{error}");
                self.warning = Some(error.to_string());
                return;
            }
            self.playing = true;
            self.last_update = Instant::now();
            return;
        }

        if self.start_mpv(media, false) {
            self.playing = true;
            self.last_update = Instant::now();
            return;
        }

        self.stop_video();
        self.stop_audio();
        self.start_video(media);
        self.start_audio(media);
        self.playing = true;
        self.last_update = Instant::now();
    }

    fn pause(&mut self, duration: Option<Duration>) {
        if !self.playing {
            return;
        }

        #[cfg(feature = "mpv-preview")]
        if let Some(mpv) = &self.mpv {
            if let Err(error) = mpv.set_paused(true) {
                log::warn!("{error}");
                self.warning = Some(error.to_string());
            }
            if let Some(position) = mpv.position() {
                self.position = position;
            }
            self.playing = false;
            self.pending_frame = None;
            self.last_update = Instant::now();
            return;
        }

        self.advance(duration);
        self.playing = false;
        self.pending_frame = None;
        self.stop_video();
        self.stop_audio();
        self.last_update = Instant::now();
    }

    fn seek(&mut self, target: Timestamp, media: &MediaFile) {
        self.position = target;
        self.pending_frame = None;
        self.warning = None;

        if duration_end(media.duration).is_some_and(|end| target.micros() >= end) {
            self.playing = false;
            self.frame = None;
            self.stop_video();
            self.stop_audio();
            self.stop_mpv();
            self.last_update = Instant::now();
            return;
        }

        #[cfg(feature = "mpv-preview")]
        if let Some(mpv) = &self.mpv {
            self.mpv_frame_ready = false;
            if let Err(error) = mpv.set_precise_seek(true).and_then(|()| mpv.seek(target)) {
                log::warn!("{error}; deferring seek until mpv is ready");
                self.mpv_pending_seek = Some((target, true));
            } else {
                self.mpv_pending_seek = None;
            }
            self.last_update = Instant::now();
            return;
        }

        self.stop_video();
        self.stop_audio();
        self.start_video(media);
        if self.playing {
            self.start_audio(media);
        }
        self.last_update = Instant::now();
    }

    fn scrub(&mut self, target: Timestamp, media: &MediaFile) {
        self.position = target;
        self.playing = false;
        self.pending_frame = None;
        self.warning = None;

        if duration_end(media.duration).is_some_and(|end| target.micros() >= end) {
            self.frame = None;
            self.stop_video();
            self.stop_audio();
            self.stop_mpv();
            self.last_update = Instant::now();
            return;
        }

        #[cfg(feature = "mpv-preview")]
        if let Some(mpv) = &self.mpv {
            self.mpv_frame_ready = false;
            let _ = mpv.set_paused(true);
            if let Err(error) = mpv.set_precise_seek(false).and_then(|()| mpv.seek(target)) {
                log::warn!("{error}; deferring scrub until mpv is ready");
                self.mpv_pending_seek = Some((target, false));
            } else {
                self.mpv_pending_seek = None;
            }
            self.last_update = Instant::now();
            return;
        }

        self.stop_video();
        self.stop_audio();
        self.start_video_with_seek(media, true);
        self.last_update = Instant::now();
    }

    fn load(&mut self, media: &MediaFile) {
        self.reset();
        if self.start_mpv(media, true) {
            #[cfg(feature = "mpv-preview")]
            {
                self.mpv_frame_ready = false;
            }
            #[cfg(feature = "mpv-preview")]
            if let Some(mpv) = &self.mpv {
                let _ = mpv.set_paused(true);
            }
            self.position = Timestamp::ZERO;
            self.playing = false;
            self.frame = None;
            return;
        }
        self.start_video(media);
    }

    fn reset(&mut self) {
        self.stop_video();
        self.stop_audio();
        self.stop_mpv();
        #[cfg(feature = "mpv-preview")]
        {
            self.mpv_failed = false;
        }
        self.position = Timestamp::ZERO;
        self.playing = false;
        self.pending_frame = None;
        self.frame = None;
        self.warning = None;
        self.last_update = Instant::now();
    }

    fn advance(&mut self, duration: Option<Duration>) -> bool {
        let now = Instant::now();
        let mut changed = false;

        if let Some(audio) = &self.audio {
            let audio_position = audio.position();
            if audio_position > self.position {
                self.position = audio_position;
                changed = true;
            }
            self.last_update = now;
        } else {
            let elapsed = now.saturating_duration_since(self.last_update);
            self.last_update = now;
            let elapsed_micros = u64::try_from(elapsed.as_micros()).unwrap_or(u64::MAX);
            if elapsed_micros > 0 {
                let Some(position) = self
                    .position
                    .checked_add(Duration::from_micros(elapsed_micros))
                else {
                    log::warn!("playback position overflowed; pausing");
                    self.playing = false;
                    self.pending_frame = None;
                    self.stop_audio();
                    self.stop_video();
                    return true;
                };
                self.position = position;
                changed = true;
            }
        }

        if let Some(end) = duration_end(duration)
            && self.position.micros() >= end
        {
            self.position = Timestamp::from_micros(end);
            self.playing = false;
            self.pending_frame = None;
            self.stop_audio();
            self.stop_video();
            changed = true;
        }

        changed
    }

    fn poll_video(&mut self, duration: Option<Duration>) -> bool {
        let was_playing = self.playing;
        let mut changed = false;
        let mut stop: Option<Option<String>> = None;

        if was_playing && let Some(frame) = self.pending_frame.take() {
            if frame.timestamp <= self.position {
                self.frame = Some(frame);
                changed = true;
            } else {
                self.pending_frame = Some(frame);
                return changed;
            }
        }

        if let Some(video) = &mut self.video {
            loop {
                match video.next_result() {
                    PreviewVideoResult::Frame(frame) => {
                        if was_playing {
                            if frame.timestamp <= self.position {
                                self.frame = Some(frame);
                                changed = true;
                            } else {
                                self.pending_frame = Some(frame);
                                break;
                            }
                        } else {
                            self.frame = Some(frame);
                            changed = true;
                            stop = Some(None);
                            break;
                        }
                    }
                    PreviewVideoResult::Pending => break,
                    PreviewVideoResult::Ended { error } => {
                        stop = Some(error);
                        break;
                    }
                }
            }
        }

        if let Some(error) = stop {
            self.stop_video();
            self.pending_frame = None;
            changed = true;

            if was_playing {
                if let Some(error) = error {
                    log::warn!("video preview stopped: {error}");
                    self.warning = Some(error);
                    self.playing = false;
                    self.stop_audio();
                } else {
                    // Keep the clock and audio running when only the video
                    // stream ended, so an audio tail is not cut off.
                    let end = duration_end(duration);
                    let keep_playing =
                        end.is_some_and(|end| self.audio.is_some() && self.position.micros() < end);
                    if !keep_playing {
                        self.playing = false;
                        self.stop_audio();
                        if let Some(end) = end
                            && self.position.micros() < end
                        {
                            self.position = Timestamp::from_micros(end);
                        }
                    }
                }
            } else if let Some(error) = error {
                log::warn!("video preview stopped: {error}");
                self.warning = Some(error);
                self.frame = None;
            }
        }

        changed
    }

    fn poll_audio(&mut self, duration: Option<Duration>) -> bool {
        let Some(mut audio) = self.audio.take() else {
            return false;
        };

        match audio.check() {
            PreviewAudioStatus::Running => {
                self.audio = Some(audio);
                false
            }
            PreviewAudioStatus::Ended { success, stderr } => {
                let audio_position = audio.position();
                if audio_position > self.position {
                    self.position = audio_position;
                }
                self.last_update = Instant::now();

                if !success {
                    let message = if stderr.is_empty() {
                        "audio preview stopped unexpectedly".to_owned()
                    } else {
                        format!("audio preview stopped: {stderr}")
                    };
                    log::warn!("{message}");
                    self.warning = Some(message);
                } else if !stderr.is_empty() {
                    log::debug!("audio decoder stderr: {stderr}");
                }

                if let Some(end) = duration_end(duration)
                    && self.position.micros() >= end
                {
                    self.position = Timestamp::from_micros(end);
                    self.playing = false;
                    self.pending_frame = None;
                    self.stop_video();
                }
                true
            }
        }
    }

    fn start_video(&mut self, media: &MediaFile) {
        self.start_video_with_seek(media, false);
    }

    fn start_video_with_seek(&mut self, media: &MediaFile, fast_seek: bool) {
        let Some(video) = preview_video_stream(media) else {
            return;
        };
        let (Some(width), Some(height)) = (video.width, video.height) else {
            self.warning = Some("video stream does not report frame dimensions".to_owned());
            return;
        };

        let config = PreviewVideoConfig {
            path: media.path.clone(),
            width,
            height,
            start: self.position,
            frame_rate: PREVIEW_FRAME_RATE,
            fast_seek,
            hardware: self.hardware,
        };
        match PreviewVideo::start(config) {
            Ok(video) => self.video = Some(video),
            Err(error) => {
                log::warn!("{error}");
                self.warning = Some(error.to_string());
            }
        }
    }

    fn start_audio(&mut self, media: &MediaFile) {
        if !has_audio(media) {
            return;
        }

        match PreviewAudio::start(&media.path, self.position) {
            Ok(audio) => self.audio = Some(audio),
            Err(error) => {
                log::warn!("{error}");
                self.warning = Some(error.to_string());
            }
        }
    }

    fn stop_video(&mut self) {
        self.video = None;
    }

    fn stop_audio(&mut self) {
        self.audio = None;
    }
}

fn preview_video_stream(media: &MediaFile) -> Option<&VideoStream> {
    media.streams.iter().find_map(|stream| match stream {
        MediaStream::Video(video) if !video.info.disposition.attached_pic => Some(video),
        _ => None,
    })
}

fn has_audio(media: &MediaFile) -> bool {
    media
        .streams
        .iter()
        .any(|stream| matches!(stream, MediaStream::Audio(_)))
}

fn duration_end(duration: Option<Duration>) -> Option<i64> {
    duration.map(|duration| i64::try_from(duration.micros()).unwrap_or(i64::MAX))
}

fn clamp_position(position: Timestamp, duration: Option<Duration>) -> Timestamp {
    let position = position.micros().max(0);
    let position = match duration_end(duration) {
        Some(end) => position.min(end),
        None => position,
    };
    Timestamp::from_micros(position)
}

/// Fits `source` into `bounds` while preserving its aspect ratio.
///
/// The result never exceeds the source size, so the preview is not upscaled
/// when the pane is larger than the media.
#[cfg(feature = "mpv-preview")]
fn fit_within(source: (u32, u32), bounds: (u32, u32)) -> (u32, u32) {
    let (source_width, source_height) = source;
    let (bounds_width, bounds_height) = bounds;
    if source_width == 0 || source_height == 0 || bounds_width == 0 || bounds_height == 0 {
        return source;
    }
    if bounds_width >= source_width && bounds_height >= source_height {
        return source;
    }

    let source_width = u64::from(source_width);
    let source_height = u64::from(source_height);
    let bounds_width = u64::from(bounds_width);
    let bounds_height = u64::from(bounds_height);

    // Compare `bounds_width / source_width` and `bounds_height / source_height`
    // by cross-multiplication to avoid floating point rounding.
    let (width, height) = if bounds_width * source_height <= bounds_height * source_width {
        let height = source_height * bounds_width / source_width;
        (bounds_width, height)
    } else {
        let width = source_width * bounds_height / source_height;
        (width, bounds_height)
    };
    (
        u32::try_from(width.max(1)).unwrap_or(u32::MAX),
        u32::try_from(height.max(1)).unwrap_or(u32::MAX),
    )
}

#[cfg(test)]
mod tests {
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
}
