//! Preview playback engine: clocks, decoding and the mpv backend.

use std::time::Instant;

use crate::domain::media::{MediaFile, MediaStream, VideoStream};
use crate::domain::time::{Duration, Timestamp};
use crate::media::preview::{
    PreviewAudio, PreviewAudioStatus, PreviewVideo, PreviewVideoConfig, PreviewVideoResult,
};

#[cfg(feature = "mpv-preview")]
use super::MpvSeekGate;
use super::Playback;

#[cfg(feature = "mpv-preview")]
use crate::media::preview::{MpvPreview, MpvPreviewConfig};

const PREVIEW_FRAME_RATE: u32 = 30;

/// Distance at which a precise mpv seek is considered to have landed even if
/// the `PlaybackRestart` event was dropped.
#[cfg(feature = "mpv-preview")]
const SEEK_ARRIVAL_TOLERANCE_MICROS: u64 = 100_000;

/// Maximum time the preview may hide the previous frame while waiting for a
/// precise seek target.
#[cfg(feature = "mpv-preview")]
const SEEK_GATE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// How long the position must stay at the seek target before the gate opens
/// without a `PlaybackRestart` event. `time-pos` can reach the target before
/// the target texture has been published.
#[cfg(feature = "mpv-preview")]
const SEEK_POSITION_SETTLE: std::time::Duration = std::time::Duration::from_millis(100);

impl Playback {
    /// Stores the playback volume and forwards it to the active backend.
    pub(super) fn set_volume(&mut self, volume: u8) {
        let volume = volume.min(100);
        if self.volume == volume {
            return;
        }
        self.volume = volume;
        self.apply_mpv_volume();
    }

    /// Applies the stored volume to the mpv backend when it is running.
    ///
    /// The external FFmpeg preview fallback has no gain stage; volume only
    /// affects the mpv backend.
    #[cfg(feature = "mpv-preview")]
    fn apply_mpv_volume(&mut self) {
        let Some(mpv) = self.mpv.as_ref() else {
            return;
        };
        if let Err(error) = mpv.set_volume(self.volume) {
            log::warn!("could not apply the preview volume: {error}");
        }
    }

    #[cfg(not(feature = "mpv-preview"))]
    fn apply_mpv_volume(&mut self) {}

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
                self.apply_mpv_volume();
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

    pub(super) fn mpv_present(&self) -> bool {
        #[cfg(feature = "mpv-preview")]
        {
            self.mpv.is_some()
        }
        #[cfg(not(feature = "mpv-preview"))]
        {
            false
        }
    }

    pub(super) fn uses_mpv_audio(&self) -> bool {
        #[cfg(feature = "mpv-preview")]
        {
            self.mpv_has_audio
        }
        #[cfg(not(feature = "mpv-preview"))]
        {
            false
        }
    }

    pub(super) fn mpv_needs_frame(&self) -> bool {
        #[cfg(feature = "mpv-preview")]
        {
            self.mpv.is_some() && !self.mpv_frame_ready
        }
        #[cfg(not(feature = "mpv-preview"))]
        {
            false
        }
    }

    pub(super) fn mpv_frame_size(&self) -> Option<(u32, u32)> {
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
            self.mpv_seek_gate = None;
            self.mpv_frame_ready = false;
            self.mpv_decoder = None;
            self.mpv_decoder_checked_at = None;
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
        let mut applied_deferred = false;
        if events.loaded || events.restarted {
            changed = true;
            if let Some((target, precise)) = self.mpv_pending_seek.take() {
                log::info!("applying deferred mpv seek to {target}");
                self.mpv_frame_ready = false;
                if let Err(error) = mpv.discard_published_frame() {
                    log::warn!("could not discard the frame before the deferred seek: {error}");
                }
                let _ = mpv
                    .set_precise_seek(precise)
                    .and_then(|()| mpv.seek(target));
                applied_deferred = true;
            }
        }
        // `PlaybackRestart` can be dropped by the event queue; a precise seek
        // that already landed means the deferred command is no longer needed.
        if let Some((target, true)) = self.mpv_pending_seek
            && position.is_some_and(|position| {
                position.micros().abs_diff(target.micros()) <= SEEK_ARRIVAL_TOLERANCE_MICROS
            })
        {
            self.mpv_pending_seek = None;
        }
        if let Some(gate) = &mut self.mpv_seek_gate
            && !events.loaded
            && !applied_deferred
        {
            let near = position.is_some_and(|position| {
                position.micros().abs_diff(gate.target.micros()) <= SEEK_ARRIVAL_TOLERANCE_MICROS
            });
            if near {
                let since = *gate.position_matched_at.get_or_insert_with(Instant::now);
                if events.restarted || since.elapsed() >= SEEK_POSITION_SETTLE {
                    if !gate.target_reached {
                        gate.target_reached_at = Some(Instant::now());
                    }
                    gate.target_reached = true;
                }
            } else {
                gate.position_matched_at = None;
            }
        }
        if self
            .mpv_seek_gate
            .as_ref()
            .is_some_and(|gate| gate.started.elapsed() >= SEEK_GATE_TIMEOUT)
        {
            log::warn!("mpv seek gate timed out; showing the last available frame");
            self.mpv_seek_gate = None;
        }
        let failed = events.failed.clone();
        // `hwdec-current` is stable for the lifetime of a decoder, so a slow
        // poll is enough and keeps libmpv property reads out of the frame path.
        let now = Instant::now();
        if self
            .mpv_decoder_checked_at
            .is_none_or(|last| now.duration_since(last) >= std::time::Duration::from_millis(500))
        {
            self.mpv_decoder = mpv.hardware_decoder();
            self.mpv_decoder_checked_at = Some(now);
        }
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

    pub(super) fn update(&mut self, duration: Option<Duration>) -> bool {
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

    pub(super) fn play(&mut self, media: &MediaFile) {
        if self.playing {
            return;
        }

        if duration_end(media.duration).is_some_and(|end| self.position.micros() >= end) {
            self.position = Timestamp::ZERO;
            self.frame = None;
            #[cfg(feature = "mpv-preview")]
            if let Some(mpv) = &self.mpv {
                self.mpv_seek_gate = Some(MpvSeekGate {
                    target: Timestamp::ZERO,
                    started: Instant::now(),
                    target_reached: false,
                    target_reached_at: None,
                    position_matched_at: None,
                });
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

    pub(super) fn pause(&mut self, duration: Option<Duration>) {
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

    pub(super) fn seek(&mut self, target: Timestamp, media: &MediaFile) {
        self.position = target;
        self.pending_frame = None;
        self.warning = None;
        #[cfg(feature = "mpv-preview")]
        {
            self.mpv_seek_gate = None;
        }

        if duration_end(media.duration).is_some_and(|end| target.micros() >= end) {
            self.playing = false;
            // Do not tear down mpv here: `keep-open=yes` keeps the file
            // loaded, so dragging away from the end stays responsive.
            #[cfg(feature = "mpv-preview")]
            if let Some(mpv) = &self.mpv {
                self.mpv_frame_ready = false;
                let _ = mpv.set_paused(true);
                let _ = mpv.set_precise_seek(false).and_then(|()| mpv.seek(target));
                self.last_update = Instant::now();
                return;
            }
            self.frame = None;
            self.stop_video();
            self.stop_audio();
            self.last_update = Instant::now();
            return;
        }

        #[cfg(feature = "mpv-preview")]
        if let Some(mpv) = &self.mpv {
            self.mpv_frame_ready = false;
            // Discard load/restart events queued before this seek so the gate
            // cannot open on an event that predates it.
            let pending_events = mpv.poll_events();
            if let Some(message) = pending_events.failed {
                log::warn!("mpv preview failed before seek: {message}");
                self.warning = Some(format!("mpv preview failed: {message}"));
            }
            if let Err(error) = mpv.discard_published_frame() {
                log::warn!("could not discard the pre-seek frame: {error}");
            }
            self.mpv_seek_gate = Some(MpvSeekGate {
                target,
                started: Instant::now(),
                target_reached: false,
                target_reached_at: None,
                position_matched_at: None,
            });
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

    pub(super) fn scrub(&mut self, target: Timestamp, media: &MediaFile) {
        self.position = target;
        self.playing = false;
        self.pending_frame = None;
        self.warning = None;
        #[cfg(feature = "mpv-preview")]
        {
            self.mpv_seek_gate = None;
        }

        if duration_end(media.duration).is_some_and(|end| target.micros() >= end) {
            // Same as `seek`: scrub previews must not destroy the mpv backend.
            #[cfg(feature = "mpv-preview")]
            if let Some(mpv) = &self.mpv {
                self.mpv_frame_ready = false;
                let _ = mpv.set_paused(true);
                let _ = mpv.set_precise_seek(false).and_then(|()| mpv.seek(target));
                self.last_update = Instant::now();
                return;
            }
            self.frame = None;
            self.stop_video();
            self.stop_audio();
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

    pub(super) fn load(&mut self, media: &MediaFile) {
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

    pub(super) fn reset(&mut self) {
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

pub(super) fn preview_video_stream(media: &MediaFile) -> Option<&VideoStream> {
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

pub(super) fn clamp_position(position: Timestamp, duration: Option<Duration>) -> Timestamp {
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
pub(super) fn fit_within(source: (u32, u32), bounds: (u32, u32)) -> (u32, u32) {
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
