//! Native file and directory pickers.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui;

use crate::application::{OpenState, PROJECT_EXTENSION};

use super::RossresscutApp;
use super::i18n::fill;
use super::settings::config_directory;

impl RossresscutApp {
    pub(super) fn poll_file_dialog(&mut self) {
        let Some((purpose, receiver)) = self.file_dialog.take() else {
            return;
        };

        match receiver.try_recv() {
            Ok(Some(path)) => {
                self.reset_scrub();
                self.segment_error = None;
                match purpose {
                    FileDialogPurpose::OpenMedia => {
                        self.stream_selection = None;
                        self.detach_project();
                        self.application.open_media(path);
                    }
                    FileDialogPurpose::RebindSource => self.application.rebind_source(path),
                }
            }
            Ok(None) => {}
            Err(TryRecvError::Empty) => {
                self.file_dialog = Some((purpose, receiver));
            }
            Err(TryRecvError::Disconnected) => {
                log::warn!("file dialog thread stopped without a result");
                self.dialog_error = Some(DialogError::Stopped);
            }
        }
    }

    pub(super) fn poll_directory_dialog(&mut self) {
        let Some((target, receiver)) = self.directory_dialog.take() else {
            return;
        };

        match receiver.try_recv() {
            Ok(Some(path)) => match target {
                DirectoryTarget::Export => {
                    self.settings.export_directory = path.display().to_string();
                    self.settings.export_to_source_dir = false;
                    self.export_error = None;
                }
                DirectoryTarget::Project => {
                    self.settings.project_directory = path.display().to_string();
                    self.settings.project_to_source_dir = false;
                    self.project_error = None;
                }
            },
            Ok(None) => {}
            Err(TryRecvError::Empty) => {
                self.directory_dialog = Some((target, receiver));
            }
            Err(TryRecvError::Disconnected) => {
                log::warn!("directory dialog thread stopped without a result");
                self.dialog_error = Some(DialogError::Stopped);
            }
        }
    }

    pub(super) fn start_directory_dialog(&mut self, target: DirectoryTarget) {
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
                self.directory_dialog = Some((target, receiver));
            }
            Err(error) => {
                log::error!("failed to start the directory dialog thread: {error}");
                self.dialog_error = Some(DialogError::Failed(error.to_string()));
            }
        }
    }

    pub(super) fn start_file_dialog(&mut self, purpose: FileDialogPurpose) {
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
                self.file_dialog = Some((purpose, receiver));
            }
            Err(error) => {
                log::error!("failed to start the file dialog thread: {error}");
                self.dialog_error = Some(DialogError::Failed(error.to_string()));
            }
        }
    }

    /// Returns where autosave and the project dialog write for `source`.
    pub(super) fn project_path_for(&self, source: &Path) -> PathBuf {
        let stem = source.file_stem().map_or_else(
            || "project".to_owned(),
            |stem| stem.to_string_lossy().into_owned(),
        );
        let file_name = format!("{stem}.{PROJECT_EXTENSION}");

        let directory = if self.settings.project_to_source_dir {
            source.parent().map(Path::to_path_buf)
        } else if !self.settings.project_directory.is_empty() {
            Some(PathBuf::from(&self.settings.project_directory))
        } else {
            config_directory().map(|directory| directory.join("rossresscut").join("projects"))
        };

        match directory {
            Some(directory) => directory.join(file_name),
            None => PathBuf::from(file_name),
        }
    }

    pub(super) fn start_project_dialog(&mut self, mode: ProjectDialogMode) {
        if self.project_dialog.is_some() {
            return;
        }
        self.dialog_error = None;

        let default = self
            .application
            .timeline()
            .source()
            .map(|source| self.project_path_for(source))
            .unwrap_or_else(|| PathBuf::from("project"));
        let (sender, receiver) = mpsc::channel();
        let spawn = thread::Builder::new()
            .name("project-dialog".to_owned())
            .spawn(move || {
                let path = match mode {
                    ProjectDialogMode::Open => pick_project_file(),
                    ProjectDialogMode::Save => pick_project_save_path(&default),
                };
                if sender.send(path).is_err() {
                    log::debug!("project dialog result discarded: the window is closing");
                }
            });

        match spawn {
            Ok(_handle) => self.project_dialog = Some((mode, receiver)),
            Err(error) => {
                log::error!("failed to start the project dialog thread: {error}");
                self.dialog_error = Some(DialogError::Failed(error.to_string()));
            }
        }
    }

    pub(super) fn poll_project_dialog(&mut self) {
        let Some((mode, receiver)) = self.project_dialog.take() else {
            return;
        };

        match receiver.try_recv() {
            Ok(Some(path)) => {
                let result = match mode {
                    ProjectDialogMode::Open => self.application.load_project(&path),
                    ProjectDialogMode::Save => self.application.save_project(&path),
                };
                if let Err(error) = result {
                    let strings = self.strings();
                    let template = match mode {
                        ProjectDialogMode::Open => &strings.project_load_failed,
                        ProjectDialogMode::Save => &strings.project_save_failed,
                    };
                    self.project_error = Some(fill(template, &[&error.to_string()]));
                } else {
                    self.project_error = None;
                    self.project_file = Some(path.clone());
                    if mode == ProjectDialogMode::Open {
                        self.stream_selection = None;
                        // The probe bumps the revision again; take the baseline
                        // once the media is ready.
                        self.awaiting_project_baseline = true;
                    } else {
                        self.saved_revision = self.application.timeline_revision();
                    }
                    self.reset_scrub();
                    self.segment_error = None;
                    self.sync_segment_inputs();
                    self.last_autosave_revision = self.application.timeline_revision();
                }
            }
            Ok(None) => {}
            Err(TryRecvError::Empty) => self.project_dialog = Some((mode, receiver)),
            Err(TryRecvError::Disconnected) => {
                log::warn!("project dialog thread stopped without a result");
                self.dialog_error = Some(DialogError::Stopped);
            }
        }
    }

    /// Writes the timeline to its project file when autosave is due.
    ///
    /// Edits are coalesced: a save happens at most once every two seconds and
    /// only after the timeline revision changed.
    pub(super) fn autosave_project(&mut self) {
        if !self.settings.autosave_projects {
            return;
        }
        let revision = self.application.timeline_revision();
        if revision == self.last_autosave_revision {
            return;
        }

        let now = Instant::now();
        if let Some(last) = self.last_autosave_at
            && now.duration_since(last) < Duration::from_secs(2)
        {
            return;
        }

        let Some(path) = self
            .application
            .timeline()
            .source()
            .map(|source| self.project_path_for(source))
        else {
            return;
        };

        match self.application.save_project(&path) {
            Ok(()) => {
                self.last_autosave_revision = revision;
                self.last_autosave_at = Some(now);
            }
            Err(error) => log::warn!("{error}"),
        }
    }

    /// Forgets the project binding after importing a plain media file.
    pub(super) fn detach_project(&mut self) {
        self.project_file = None;
        self.saved_revision = 0;
        self.awaiting_project_baseline = false;
    }

    /// Takes the saved-revision baseline once a loaded project is ready.
    pub(super) fn sync_project_baseline(&mut self) {
        if !self.awaiting_project_baseline
            || matches!(
                self.application.state(),
                OpenState::Probing { .. } | OpenState::Empty
            )
        {
            return;
        }
        self.saved_revision = self.application.timeline_revision();
        self.awaiting_project_baseline = false;
    }

    /// Returns whether closing would lose edits to a bound project file.
    pub(super) fn has_unsaved_project(&self) -> bool {
        self.project_file.is_some()
            && !self.awaiting_project_baseline
            && self.application.timeline_revision() != self.saved_revision
    }

    /// Closes the media, asking to save a bound project first.
    pub(super) fn request_close_media(&mut self) {
        if matches!(self.application.state(), OpenState::Empty) {
            return;
        }
        if self.has_unsaved_project() {
            self.pending_close = Some(CloseIntent::Media);
            self.project_error = None;
        } else {
            self.close_media_now();
        }
    }

    fn close_media_now(&mut self) {
        self.application.close_media();
        self.reset_scrub();
        self.segment_error = None;
        self.detach_project();
    }

    /// Saves or discards pending edits, then performs the requested close.
    pub(super) fn finish_close(
        &mut self,
        intent: CloseIntent,
        save: bool,
        context: &egui::Context,
    ) {
        if save {
            let Some(path) = self.project_file.clone() else {
                self.project_error = Some("no project file to save".to_owned());
                return;
            };
            if let Err(error) = self.application.save_project(&path) {
                self.project_error = Some(error.to_string());
                return;
            }
            self.saved_revision = self.application.timeline_revision();
        }

        self.pending_close = None;
        self.project_error = None;
        match intent {
            CloseIntent::Media => self.close_media_now(),
            CloseIntent::Application => {
                self.allow_close = true;
                context.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
}

pub(super) enum DialogError {
    Failed(String),
    Stopped,
}

/// What the media dialog should do with the chosen path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FileDialogPurpose {
    OpenMedia,
    RebindSource,
}

/// Which setting a directory picker writes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DirectoryTarget {
    Export,
    Project,
}

/// Whether the project dialog opens an existing file or picks a save path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ProjectDialogMode {
    Open,
    Save,
}

/// What the user asked to close while a project has unsaved edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CloseIntent {
    Media,
    Application,
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
pub(super) fn pick_project_file() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter("Project files", &[crate::application::PROJECT_EXTENSION])
        .pick_file()
}

#[cfg(not(target_os = "linux"))]
pub(super) fn pick_project_file() -> Option<PathBuf> {
    None
}

#[cfg(target_os = "linux")]
pub(super) fn pick_project_save_path(default: &std::path::Path) -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter("Project files", &[crate::application::PROJECT_EXTENSION])
        .set_file_name(format!(
            "{}.{}",
            default
                .file_stem()
                .map_or("project", |stem| stem.to_str().unwrap_or("project")),
            crate::application::PROJECT_EXTENSION
        ))
        .save_file()
}

#[cfg(not(target_os = "linux"))]
pub(super) fn pick_project_save_path(default: &std::path::Path) -> Option<PathBuf> {
    let _ = default;
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
