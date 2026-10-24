//! Application layer: user commands, application state, and background jobs.
//!
//! The UI only sends commands such as [`Application::open_media`] and polls
//! [`Application::poll`]. FFprobe never runs on the UI thread.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use crate::domain::media::MediaFile;
use crate::error::AppError;
use crate::media::ffprobe::{self, ProbeError};

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
#[derive(Debug, Default)]
pub struct Application {
    media: Option<MediaFile>,
    state: OpenState,
    probe: Option<ProbeJob>,
}

#[derive(Debug)]
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

    /// Starts a background probe for `path`.
    ///
    /// This method only spawns a worker; it never waits for FFprobe. Failures
    /// that prevent the worker from starting are exposed through
    /// [`OpenState::Failed`].
    pub fn open_media(&mut self, path: PathBuf) {
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

    /// Checks for a completed background probe without blocking.
    ///
    /// Returns `true` when application state changed, so the UI can repaint.
    pub fn poll(&mut self) -> bool {
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
                self.media = Some(media);
                self.state = OpenState::Ready;
                true
            }
            Ok(Err(error)) => {
                log::warn!("{error}");
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
    fn polling_without_a_job_does_not_change_state() {
        let mut application = Application::new();

        assert!(!application.poll());
        assert!(matches!(application.state(), OpenState::Empty));
    }
}
