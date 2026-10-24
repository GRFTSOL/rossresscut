//! Application layer: user commands, application state, and background jobs.
//!
//! The UI sends commands and polls [`Application::update`]. FFprobe, preview
//! decoding, and audio playback all run outside the UI thread.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::Instant;

use crate::domain::media::{MediaFile, MediaStream, VideoStream};
use crate::domain::time::{Duration, Timestamp};
use crate::error::AppError;
use crate::media::ffprobe::{self, ProbeError};
use crate::media::preview::{
    PreviewAudio, PreviewAudioStatus, PreviewVideo, PreviewVideoConfig, PreviewVideoResult,
    VideoFrame,
};

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

/// Owns application state and coordinates background work.
#[derive(Default)]
pub struct Application {
    media: Option<MediaFile>,
    state: OpenState,
    probe: Option<ProbeJob>,
    playback: Playback,
}

struct ProbeJob {
    path: PathBuf,
    receiver: Receiver<Result<MediaFile, ProbeError>>,
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

    /// Returns whether a seek/load preview frame is still being decoded.
    pub fn is_waiting_for_preview(&self) -> bool {
        !self.playback.playing && self.playback.video.is_some()
    }

    /// Returns the latest preview frame.
    pub fn frame(&self) -> Option<&VideoFrame> {
        self.playback.frame.as_ref()
    }

    /// Returns a non-fatal preview warning, if any.
    pub fn warning(&self) -> Option<&str> {
        self.playback.warning.as_deref()
    }

    /// Starts a background probe for `path`.
    ///
    /// This method only spawns a worker; it never waits for FFprobe. Failures
    /// that prevent the worker from starting are exposed through
    /// [`OpenState::Failed`].
    pub fn open_media(&mut self, path: PathBuf) {
        self.playback.reset();

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
                self.media = Some(media);
                self.state = OpenState::Ready;
                true
            }
            Ok(Err(error)) => {
                log::warn!("{error}");
                self.playback.reset();
                self.media = None;
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
                self.media = None;
                self.state = OpenState::Failed {
                    path: job.path,
                    message,
                };
                true
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
        }
    }
}

impl Playback {
    fn update(&mut self, duration: Option<Duration>) -> bool {
        let mut changed = false;
        if self.playing {
            changed |= self.advance(duration);
        }
        changed |= self.poll_video(duration);
        changed |= self.poll_audio();
        changed
    }

    fn play(&mut self, media: &MediaFile) {
        if self.playing {
            return;
        }

        if duration_end(media.duration).is_some_and(|end| self.position.micros() >= end) {
            self.position = Timestamp::ZERO;
            self.frame = None;
        }

        self.pending_frame = None;
        self.warning = None;
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
        self.stop_video();
        self.stop_audio();

        if duration_end(media.duration).is_some_and(|end| target.micros() >= end) {
            self.playing = false;
            self.frame = None;
            self.last_update = Instant::now();
            return;
        }

        self.start_video(media);
        if self.playing {
            self.start_audio(media);
        }
        self.last_update = Instant::now();
    }

    fn load(&mut self, media: &MediaFile) {
        self.reset();
        self.start_video(media);
    }

    fn reset(&mut self) {
        self.stop_video();
        self.stop_audio();
        self.position = Timestamp::ZERO;
        self.playing = false;
        self.pending_frame = None;
        self.frame = None;
        self.warning = None;
        self.last_update = Instant::now();
    }

    fn advance(&mut self, duration: Option<Duration>) -> bool {
        let now = Instant::now();
        let elapsed = now.saturating_duration_since(self.last_update);
        self.last_update = now;
        let elapsed_micros = u64::try_from(elapsed.as_micros()).unwrap_or(u64::MAX);
        if elapsed_micros == 0 {
            return false;
        }

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

        if let Some(end) = duration_end(duration)
            && self.position.micros() >= end
        {
            self.position = Timestamp::from_micros(end);
            self.playing = false;
            self.pending_frame = None;
            self.stop_audio();
            self.stop_video();
        }

        true
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

    fn poll_audio(&mut self) -> bool {
        let Some(mut audio) = self.audio.take() else {
            return false;
        };

        match audio.check() {
            PreviewAudioStatus::Running => {
                self.audio = Some(audio);
                false
            }
            PreviewAudioStatus::Ended { success, stderr } => {
                if !success {
                    let message = if stderr.is_empty() {
                        "audio preview stopped unexpectedly".to_owned()
                    } else {
                        format!("audio preview stopped: {stderr}")
                    };
                    log::warn!("{message}");
                    self.warning = Some(message);
                } else if !stderr.is_empty() {
                    log::debug!("ffplay stderr: {stderr}");
                }
                true
            }
        }
    }

    fn start_video(&mut self, media: &MediaFile) {
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
}
