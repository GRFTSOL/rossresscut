//! Media opening, probing and background keyframe indexing.

use std::path::PathBuf;
use std::sync::mpsc::{self, TryRecvError};
use std::thread;

use crate::domain::media::MediaFile;
use crate::error::AppError;
use crate::media::ffprobe;

use super::playback::preview_video_stream;
use super::{Application, KeyframeJob, Markers, OpenState, ProbeJob};

impl Application {
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
    /// Closes the current media and stops all preview work.
    pub fn close_media(&mut self) {
        self.cancel_export();
        self.playback.reset();
        self.reset_editing();
        self.media = None;
        self.probe = None;
        self.keyframes = None;
        self.keyframe_job = None;
        self.state = OpenState::Empty;
    }

    pub(super) fn reset_editing(&mut self) {
        self.timeline.clear();
        self.markers = Markers::default();
        self.selected_segment = None;
        self.selection.clear();
        self.clipboard.clear();
        self.history.clear();
        self.redo_stack.clear();
    }
    pub(super) fn poll_probe(&mut self) -> bool {
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

    pub(super) fn start_keyframes(&mut self, media: &MediaFile) {
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

    pub(super) fn poll_keyframes(&mut self) {
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
