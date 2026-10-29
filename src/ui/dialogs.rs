//! Native file and directory pickers.

use std::path::PathBuf;
use std::sync::mpsc::{self, TryRecvError};
use std::thread;

use super::RossresscutApp;

impl RossresscutApp {
    pub(super) fn poll_file_dialog(&mut self) {
        let Some(receiver) = self.file_dialog.take() else {
            return;
        };

        match receiver.try_recv() {
            Ok(Some(path)) => {
                self.reset_scrub();
                self.segment_error = None;
                self.application.open_media(path);
            }
            Ok(None) => {}
            Err(TryRecvError::Empty) => {
                self.file_dialog = Some(receiver);
            }
            Err(TryRecvError::Disconnected) => {
                log::warn!("file dialog thread stopped without a result");
                self.dialog_error = Some(DialogError::Stopped);
            }
        }
    }

    pub(super) fn poll_directory_dialog(&mut self) {
        let Some(receiver) = self.directory_dialog.take() else {
            return;
        };

        match receiver.try_recv() {
            Ok(Some(path)) => {
                self.settings.export_directory = path.display().to_string();
                self.settings.export_to_source_dir = false;
                self.export_error = None;
            }
            Ok(None) => {}
            Err(TryRecvError::Empty) => {
                self.directory_dialog = Some(receiver);
            }
            Err(TryRecvError::Disconnected) => {
                log::warn!("directory dialog thread stopped without a result");
                self.dialog_error = Some(DialogError::Stopped);
            }
        }
    }

    pub(super) fn start_directory_dialog(&mut self) {
        if self.directory_dialog.is_some() {
            return;
        }
        self.dialog_error = None;

        let (sender, receiver) = mpsc::channel();
        let spawn = thread::Builder::new()
            .name("directory-dialog".to_owned())
            .spawn(move || {
                let path = pick_directory();
                if sender.send(path).is_err() {
                    log::debug!("directory dialog result discarded: the window is closing");
                }
            });

        match spawn {
            Ok(_handle) => {
                self.directory_dialog = Some(receiver);
            }
            Err(error) => {
                log::error!("failed to start the directory dialog thread: {error}");
                self.dialog_error = Some(DialogError::Failed(error.to_string()));
            }
        }
    }

    pub(super) fn start_file_dialog(&mut self) {
        if self.file_dialog.is_some() {
            return;
        }
        self.dialog_error = None;

        let (sender, receiver) = mpsc::channel();
        let spawn = thread::Builder::new()
            .name("file-dialog".to_owned())
            .spawn(move || {
                let path = pick_media_file();
                if sender.send(path).is_err() {
                    log::debug!("file dialog result discarded: the window is closing");
                }
            });

        match spawn {
            Ok(_handle) => {
                self.file_dialog = Some(receiver);
            }
            Err(error) => {
                log::error!("failed to start the file dialog thread: {error}");
                self.dialog_error = Some(DialogError::Failed(error.to_string()));
            }
        }
    }
}

pub(super) enum DialogError {
    Failed(String),
    Stopped,
}

#[cfg(target_os = "linux")]
pub(super) fn pick_media_file() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter(
            "Media files",
            &[
                "mp4", "mkv", "mov", "avi", "wmv", "ts", "m2ts", "webm", "flv", "mpg", "mpeg",
                "m4v", "mp3", "aac", "flac", "wav", "ogg", "opus", "mka", "srt", "ass",
            ],
        )
        .pick_file()
}

#[cfg(not(target_os = "linux"))]
pub(super) fn pick_media_file() -> Option<PathBuf> {
    None
}

#[cfg(target_os = "linux")]
pub(super) fn pick_directory() -> Option<PathBuf> {
    rfd::FileDialog::new().pick_folder()
}

#[cfg(not(target_os = "linux"))]
pub(super) fn pick_directory() -> Option<PathBuf> {
    None
}
