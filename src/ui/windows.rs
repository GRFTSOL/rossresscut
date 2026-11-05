//! Floating windows: settings, about, export and the last command.

use std::path::PathBuf;

use eframe::egui;

use crate::application::{
    ExportCommandError, ExportOptions, SceneScan, SilenceError, SilenceOptions,
};
use crate::domain::media::MediaStream;
use crate::domain::time::Duration;
use crate::media::export::StreamSelection;

use super::dialogs::DirectoryTarget;
use super::i18n::{Language, fill};
use super::settings::{detect_system_theme, save_settings, theme_label};
use super::{
    FONT_ZOOM_STEP, MAX_FONT_ZOOM, MIN_FONT_ZOOM, RossresscutApp, ShortcutAction, Strings,
    ThemeChoice,
};

impl RossresscutApp {
    fn export_directory_ui(&mut self, ui: &mut egui::Ui, strings: &Strings) {
        ui.checkbox(
            &mut self.settings.export_to_source_dir,
            strings.export_to_source_dir.as_str(),
        )
        .on_hover_text(strings.export_to_source_dir.as_str());
        ui.add_enabled_ui(!self.settings.export_to_source_dir, |ui| {
            ui.horizontal(|ui| {
                if ui.button(strings.choose_directory.as_str()).clicked() {
                    self.start_directory_dialog(DirectoryTarget::Export);
                }
                ui.add(
                    egui::TextEdit::singleline(&mut self.settings.export_directory)
                        .desired_width(360.0)
                        .hint_text(strings.export_directory.as_str()),
                );
            });
        });
    }

    fn export_output_path(&self) -> Option<PathBuf> {
        let media = self.application.media()?;
        let directory = if self.settings.export_to_source_dir {
            media.path.parent()?.to_path_buf()
        } else {
            let directory = self.settings.export_directory.trim();
            if directory.is_empty() {
                return None;
            }
            PathBuf::from(directory)
        };
        Some(directory.join(self.default_export_name()?))
    }

    fn default_export_name(&self) -> Option<String> {
        let path = &self.application.media()?.path;
        let stem = path.file_stem()?.to_string_lossy();
        Some(match path.extension() {
            Some(extension) => format!("{stem}_export.{}", extension.to_string_lossy()),
            None => format!("{stem}_export.mkv"),
        })
    }

    /// Returns the streams to export, or `None` when everything is selected.
    fn current_stream_selection(&self) -> Option<StreamSelection> {
        let set = self.stream_selection.as_ref()?;
        let media = self.application.media()?;
        let mut selection = StreamSelection::default();
        for stream in &media.streams {
            let info = stream.info();
            if !set.contains(&info.index) {
                continue;
            }
            match stream {
                MediaStream::Video(_) => selection.video.push(info.index),
                MediaStream::Audio(_) => selection.audio.push(info.index),
                MediaStream::Subtitle(_) => selection.subtitle.push(info.index),
                MediaStream::Other(_) => {}
            }
        }
        Some(selection)
    }

    /// Renders one checkbox per stream in the current media.
    fn stream_selection_ui(&mut self, ui: &mut egui::Ui, strings: &Strings) {
        let (streams, all): (Vec<(usize, String)>, Vec<usize>) = {
            let Some(media) = self.application.media() else {
                return;
            };
            let streams = media
                .streams
                .iter()
                .map(|stream| {
                    let info = stream.info();
                    let kind = match stream {
                        MediaStream::Video(_) => strings.video.as_str(),
                        MediaStream::Audio(_) => strings.audio.as_str(),
                        MediaStream::Subtitle(_) => strings.subtitle_stream.as_str(),
                        MediaStream::Other(_) => "-",
                    };
                    let codec = info.codec_name.as_deref().unwrap_or("?");
                    (info.index, format!("#{} {kind} {codec}", info.index))
                })
                .collect();
            let all = media
                .streams
                .iter()
                .map(|stream| stream.info().index)
                .collect();
            (streams, all)
        };

        ui.separator();
        ui.label(strings.streams.as_str());
        for (index, label) in streams {
            let mut checked = self
                .stream_selection
                .as_ref()
                .is_none_or(|set| set.contains(&index));
            if ui.checkbox(&mut checked, label).changed() {
                let mut set = self
                    .stream_selection
                    .take()
                    .unwrap_or_else(|| all.iter().copied().collect());
                if checked {
                    set.insert(index);
                } else {
                    set.remove(&index);
                }
                self.stream_selection = Some(set);
            }
        }
    }

    pub(super) fn export_panel(&mut self, context: &egui::Context) {
        if !self.export_panel_open {
            return;
        }
        let strings = self.strings();
        let mut open = self.export_panel_open;
        let mut close_requested = false;
        let output = self.export_output_path();
        let running = self.application.export_status().is_running();
        let has_enabled_segment = self
            .application
            .timeline()
            .segments()
            .iter()
            .any(|segment| segment.enabled());
        let can_export = output.is_some() && has_enabled_segment && !running;

        egui::Window::new(strings.export.as_str())
            .id(egui::Id::unique("export_panel"))
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .default_pos(context.content_rect().center() - egui::vec2(270.0, 140.0))
            .default_width(540.0)
            .show(context, |ui| {
                self.export_directory_ui(ui, strings);
                self.stream_selection_ui(ui, strings);
                ui.separator();
                ui.label(strings.export_output_path.as_str());
                let path_text = output
                    .as_ref()
                    .map_or_else(|| "-".to_owned(), |path| path.display().to_string());
                ui.add(egui::Label::new(path_text).wrap().selectable(false));
                if let Some(error) = &self.export_error {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(can_export, egui::Button::new(strings.export.as_str()))
                        .clicked()
                        && let Some(output) = output.clone()
                    {
                        match self.application.start_export(
                            output,
                            ExportOptions {
                                chapters: self.settings.export_chapters,
                                title: Some(self.settings.export_title.clone()),
                                subtitle_keep_partial: self.settings.subtitle_keep_partial,
                                streams: self.current_stream_selection(),
                                smart_cut: self.settings.smart_cut,
                                smart_cut_bit_rate_kbps: self.settings.smart_cut_bit_rate_kbps,
                            },
                        ) {
                            Ok(()) => {
                                self.export_error = None;
                                close_requested = true;
                            }
                            Err(error) => {
                                self.export_error = Some(export_command_message(strings, &error));
                            }
                        }
                    }
                    if ui.button(strings.cancel.as_str()).clicked() {
                        close_requested = true;
                    }
                });
            });
        if close_requested {
            open = false;
        }
        self.export_panel_open = open;
    }

    pub(super) fn ffmpeg_command_window(&mut self, context: &egui::Context) {
        if !self.ffmpeg_command_open {
            return;
        }
        let strings = self.strings();
        let mut open = self.ffmpeg_command_open;
        let commands = self.application.last_ffmpeg_commands().to_vec();

        egui::Window::new(strings.last_ffmpeg_command.as_str())
            .id(egui::Id::unique("ffmpeg_command_window"))
            .open(&mut open)
            .resizable(true)
            .collapsible(false)
            .default_size(egui::vec2(720.0, 360.0))
            .show(context, |ui| {
                if commands.is_empty() {
                    ui.label(strings.no_ffmpeg_command.as_str());
                    return;
                }
                if ui.button(strings.copy_command.as_str()).clicked() {
                    ui.ctx().copy_text(commands.join("\n\n"));
                }
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for command in &commands {
                        ui.add(
                            egui::Label::new(egui::RichText::new(command).monospace())
                                .wrap()
                                .selectable(true),
                        );
                        ui.separator();
                    }
                });
            });
        self.ffmpeg_command_open = open;
    }

    pub(super) fn settings_window(&mut self, context: &egui::Context) {
        if !self.settings_open {
            return;
        }
        let strings = self.strings();
        let mut open = self.settings_open;

        egui::Window::new(strings.settings.as_str())
            .id(egui::Id::unique("settings_window"))
            .open(&mut open)
            .resizable(true)
            .collapsible(false)
            .min_width(760.0)
            .min_height(560.0)
            .default_size(egui::vec2(820.0, 700.0))
            .show(context, |ui| {
                egui::CollapsingHeader::new(strings.general.as_str())
                    .default_open(true)
                    .show(ui, |ui| {
                        let follow = ui
                            .checkbox(
                                &mut self.settings.follow_system_theme,
                                strings.follow_system_theme.as_str(),
                            )
                            .on_hover_text(strings.follow_system_theme.as_str());
                        if follow.changed() {
                            // Re-detect immediately instead of reusing the
                            // value captured at startup.
                            self.detected_theme = detect_system_theme();
                        }
                        ui.add_enabled_ui(!self.settings.follow_system_theme, |ui| {
                            egui::ComboBox::from_label(strings.theme.as_str())
                                .selected_text(theme_label(strings, self.settings.theme))
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut self.settings.theme,
                                        ThemeChoice::Dark,
                                        strings.theme_dark.as_str(),
                                    );
                                    ui.selectable_value(
                                        &mut self.settings.theme,
                                        ThemeChoice::Light,
                                        strings.theme_light.as_str(),
                                    );
                                });
                        });

                        ui.horizontal(|ui| {
                            ui.label(strings.language.as_str());
                            egui::ComboBox::from_id_salt("language")
                                .selected_text(self.settings.language.label())
                                .show_ui(ui, |ui| {
                                    for language in Language::ALL {
                                        ui.selectable_value(
                                            &mut self.settings.language,
                                            language,
                                            language.label(),
                                        );
                                    }
                                });
                        });

                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label(strings.font_size.as_str());
                            let slider = ui.add(
                                egui::Slider::new(
                                    &mut self.font_zoom_draft,
                                    MIN_FONT_ZOOM..=MAX_FONT_ZOOM,
                                )
                                .step_by(f64::from(FONT_ZOOM_STEP))
                                .fixed_decimals(2),
                            );
                            // The preview only follows a released slider; this
                            // avoids rebuilding the font atlas on every drag
                            // frame.
                            if slider.drag_stopped() || (slider.changed() && !slider.dragged()) {
                                self.settings.font_zoom =
                                    self.font_zoom_draft.clamp(MIN_FONT_ZOOM, MAX_FONT_ZOOM);
                            }
                            if ui
                                .button(strings.reset_font_size.as_str())
                                .on_hover_text(strings.reset_font_size.as_str())
                                .clicked()
                            {
                                self.set_font_zoom(1.0);
                            }
                        });
                        self.font_family_ui(ui, strings);

                        ui.separator();
                        ui.checkbox(
                            &mut self.settings.subtitle_keep_partial,
                            strings.subtitle_keep_partial.as_str(),
                        )
                        .on_hover_text(strings.subtitle_keep_partial_hint.as_str());
                        ui.checkbox(
                            &mut self.settings.autosave_projects,
                            strings.autosave_projects.as_str(),
                        )
                        .on_hover_text(strings.autosave_projects_hint.as_str());
                        ui.add_enabled_ui(self.settings.autosave_projects, |ui| {
                            ui.checkbox(
                                &mut self.settings.project_to_source_dir,
                                strings.project_to_source_dir.as_str(),
                            )
                            .on_hover_text(strings.project_to_source_dir.as_str());
                            ui.add_enabled_ui(!self.settings.project_to_source_dir, |ui| {
                                ui.horizontal(|ui| {
                                    if ui
                                        .button(strings.choose_project_directory.as_str())
                                        .clicked()
                                    {
                                        self.start_directory_dialog(DirectoryTarget::Project);
                                    }
                                    ui.add(
                                        egui::TextEdit::singleline(
                                            &mut self.settings.project_directory,
                                        )
                                        .desired_width(360.0)
                                        .hint_text(strings.project_directory.as_str()),
                                    );
                                });
                            });
                        });
                    });

                egui::CollapsingHeader::new(strings.export.as_str())
                    .default_open(true)
                    .show(ui, |ui| {
                        ui.checkbox(
                            &mut self.settings.export_chapters,
                            strings.export_chapters.as_str(),
                        )
                        .on_hover_text(strings.export_chapters_hint.as_str());
                        self.export_directory_ui(ui, strings);

                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label(strings.export_title.as_str());
                            ui.add(
                                egui::TextEdit::singleline(&mut self.settings.export_title)
                                    .desired_width(320.0),
                            )
                            .on_hover_text(strings.export_title_hint.as_str());
                        });
                        ui.checkbox(&mut self.settings.smart_cut, strings.smart_cut.as_str())
                            .on_hover_text(strings.smart_cut_hint.as_str());
                        ui.add_enabled_ui(self.settings.smart_cut, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(strings.smart_cut_bit_rate.as_str());
                                ui.add(
                                    egui::DragValue::new(
                                        &mut self.settings.smart_cut_bit_rate_kbps,
                                    )
                                    .range(0..=1_000_000)
                                    .speed(50)
                                    .suffix(" kbps"),
                                )
                                .on_hover_text(strings.smart_cut_bit_rate_hint.as_str());
                            });
                        });
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(strings.export_join_hint.as_str()).small(),
                            )
                            .wrap(),
                        );
                    });

                egui::CollapsingHeader::new(strings.shortcuts.as_str())
                    .default_open(false)
                    .show(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .max_height(360.0)
                            .show(ui, |ui| {
                                for action in ShortcutAction::ALL {
                                    ui.horizontal(|ui| {
                                        ui.add_sized(
                                            [210.0, 20.0],
                                            egui::Label::new(action.label(strings)).truncate(),
                                        );
                                        let recording = self.recording_shortcut == Some(action);
                                        let binding = self.settings.binding(action).to_owned();
                                        let text = if recording {
                                            strings.shortcut_press_keys.clone()
                                        } else if binding.is_empty() {
                                            strings.shortcut_unassigned.clone()
                                        } else {
                                            binding.clone()
                                        };
                                        if ui
                                            .add(
                                                egui::Button::new(text)
                                                    .min_size(egui::vec2(140.0, 26.0)),
                                            )
                                            .on_hover_text(strings.shortcut_press_keys.as_str())
                                            .clicked()
                                        {
                                            self.recording_shortcut =
                                                if recording { None } else { Some(action) };
                                            self.recording_pending = None;
                                        }
                                        if ui
                                            .add_enabled(
                                                !binding.is_empty(),
                                                egui::Button::new("×")
                                                    .min_size(egui::vec2(28.0, 26.0)),
                                            )
                                            .on_hover_text(strings.shortcut_clear.as_str())
                                            .clicked()
                                        {
                                            self.settings
                                                .shortcuts
                                                .insert(action.key().to_owned(), String::new());
                                            if recording {
                                                self.recording_shortcut = None;
                                                self.recording_pending = None;
                                            }
                                        }
                                    });
                                }
                            });
                    });
            });

        self.settings_open = open;
        // `eframe` only calls `App::save` when persistence is configured, so
        // settings are written when the window is closed instead.
        if !open {
            save_settings(&self.settings);
        }
    }

    /// Opens the scene detection dialog and starts a scan.
    pub(super) fn open_scene_dialog(&mut self) {
        self.scene_dialog_open = true;
        self.application.start_scene_scan(self.scene_threshold);
    }

    pub(super) fn scene_window(&mut self, context: &egui::Context) {
        if !self.scene_dialog_open {
            return;
        }
        let strings = self.strings();
        let mut open = self.scene_dialog_open;
        let mut close_requested = false;
        let status = self.application.scene_scan().cloned();

        egui::Window::new(strings.scene_detection.as_str())
            .id(egui::Id::unique("scene_window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(360.0)
            .show(context, |ui| {
                ui.label(strings.silence_replaces.as_str());

                ui.add(
                    egui::Slider::new(&mut self.scene_threshold, 0.1..=0.9)
                        .text(strings.scene_threshold.as_str())
                        .fixed_decimals(2),
                );

                match &status {
                    Some(SceneScan::Running) => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(strings.scene_scanning.as_str());
                        });
                    }
                    Some(SceneScan::Ready(cuts)) => {
                        if cuts.is_empty() {
                            ui.label(strings.scene_no_cuts.as_str());
                        } else {
                            ui.label(fill(
                                &strings.scene_cuts,
                                &[cuts.len().to_string().as_str()],
                            ));
                        }
                    }
                    Some(SceneScan::Failed(message)) => {
                        ui.colored_label(ui.visuals().error_fg_color, message);
                    }
                    None => {
                        ui.label(strings.scene_no_cuts.as_str());
                    }
                }

                ui.horizontal(|ui| {
                    let ready = matches!(status, Some(SceneScan::Ready(_)));
                    if ui
                        .add_enabled(ready, egui::Button::new(strings.apply.as_str()))
                        .clicked()
                    {
                        match self.application.segments_from_scene_changes() {
                            Ok(count) => {
                                log::info!("created {count} segments from scene changes");
                                self.segment_error = None;
                                close_requested = true;
                            }
                            Err(error) => self.segment_error = Some(error.to_string()),
                        }
                    }
                    if ui.button(strings.scene_rescan.as_str()).clicked() {
                        self.application.start_scene_scan(self.scene_threshold);
                    }
                    if ui.button(strings.cancel.as_str()).clicked() {
                        close_requested = true;
                    }
                });
            });

        if close_requested {
            open = false;
        }
        self.scene_dialog_open = open;
    }

    /// Opens the silence detection dialog and refreshes its preview.
    pub(super) fn open_silence_dialog(&mut self) {
        self.silence_dialog_open = true;
        self.refresh_silence_preview();
    }

    fn refresh_silence_preview(&mut self) {
        self.silence_preview = Some(self.silence_preview_for(self.silence_options));
    }

    fn silence_preview_for(&self, options: SilenceOptions) -> Result<usize, String> {
        let strings = self.strings();
        self.application
            .count_silence_segments(options)
            .map_err(|error| match error {
                SilenceError::NoWaveform => strings.silence_no_waveform.clone(),
                SilenceError::NoAudibleRange | SilenceError::NoDuration => {
                    strings.silence_no_audible.clone()
                }
            })
    }

    pub(super) fn silence_window(&mut self, context: &egui::Context) {
        if !self.silence_dialog_open {
            return;
        }
        let strings = self.strings();
        let mut open = self.silence_dialog_open;
        let mut recompute = false;
        let mut close_requested = false;

        egui::Window::new(strings.silence_detection.as_str())
            .id(egui::Id::unique("silence_window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(360.0)
            .show(context, |ui| {
                ui.label(strings.silence_replaces.as_str());

                let threshold = ui.add(
                    egui::Slider::new(&mut self.silence_options.threshold_db, -80.0..=-20.0)
                        .text(strings.silence_threshold.as_str())
                        .suffix(" dB")
                        .fixed_decimals(0),
                );
                if threshold.drag_stopped() || (threshold.changed() && !threshold.dragged()) {
                    recompute = true;
                }

                let mut seconds = self.silence_options.min_duration.micros() as f64 / 1_000_000.0;
                let minimum = ui.add(
                    egui::Slider::new(&mut seconds, 0.1..=10.0)
                        .text(strings.silence_min_duration.as_str())
                        .suffix(" s")
                        .step_by(0.1),
                );
                if minimum.drag_stopped() || (minimum.changed() && !minimum.dragged()) {
                    self.silence_options.min_duration =
                        Duration::from_micros((seconds * 1_000_000.0).round() as u64);
                    recompute = true;
                }

                if let Some(result) = &self.silence_preview {
                    match result {
                        Ok(count) => {
                            ui.label(fill(
                                &strings.silence_preview,
                                &[count.to_string().as_str()],
                            ));
                        }
                        Err(message) => {
                            ui.colored_label(ui.visuals().error_fg_color, message);
                        }
                    }
                }

                ui.horizontal(|ui| {
                    if ui.button(strings.apply.as_str()).clicked() {
                        match self.application.segments_from_silence(self.silence_options) {
                            Ok(count) => {
                                log::info!("created {count} segments from silence");
                                self.segment_error = None;
                                close_requested = true;
                            }
                            Err(error) => self.segment_error = Some(error.to_string()),
                        }
                    }
                    if ui.button(strings.cancel.as_str()).clicked() {
                        close_requested = true;
                    }
                });
            });

        if recompute {
            self.refresh_silence_preview();
        }
        if close_requested {
            open = false;
        }
        self.silence_dialog_open = open;
    }

    /// Asks how many random, non-overlapping segments to create.
    pub(super) fn random_window(&mut self, context: &egui::Context) {
        if !self.random_dialog_open {
            return;
        }
        let strings = self.strings();
        let mut open = self.random_dialog_open;
        let mut create = false;
        let mut close_requested = false;

        egui::Window::new(strings.random_segment.as_str())
            .id(egui::Id::unique("random_window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .default_width(320.0)
            .show(context, |ui| {
                ui.horizontal(|ui| {
                    ui.label(strings.random_segments_count.as_str());
                    ui.add(egui::DragValue::new(&mut self.random_count).range(1..=100));
                });
                if let Some(error) = &self.segment_error {
                    ui.colored_label(ui.visuals().error_fg_color, error.as_str());
                }
                ui.horizontal(|ui| {
                    if ui.button(strings.create.as_str()).clicked() {
                        create = true;
                    }
                    if ui.button(strings.cancel.as_str()).clicked() {
                        close_requested = true;
                    }
                });
            });

        if create {
            match self.application.add_random_segments(self.random_count) {
                Ok(_) => {
                    self.segment_error = None;
                    self.segment_input_segment = None;
                    open = false;
                }
                Err(error) => self.segment_error = Some(error.to_string()),
            }
        }
        if close_requested {
            open = false;
        }
        self.random_dialog_open = open;
    }

    /// Asks what to do with several files dropped at once.
    pub(super) fn drop_window(&mut self, context: &egui::Context) {
        let Some(files) = self.pending_drop.clone() else {
            return;
        };
        let strings = self.strings();
        let mut close = false;

        egui::Window::new(fill(
            &strings.drop_files,
            &[files.len().to_string().as_str()],
        ))
        .id(egui::Id::unique("drop_window"))
        .collapsible(false)
        .resizable(false)
        .default_width(380.0)
        .show(context, |ui| {
            egui::ScrollArea::vertical()
                .max_height(180.0)
                .show(ui, |ui| {
                    for file in &files {
                        ui.label(super::panels::file_title(file));
                    }
                });
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button(strings.drop_merge.as_str()).clicked() {
                    self.start_merge(files.clone());
                    close = true;
                }
                if ui.button(strings.drop_batch.as_str()).clicked() {
                    self.application.queue_batch(files.clone());
                    self.batch_panel_open = true;
                    close = true;
                }
                if ui.button(strings.cancel.as_str()).clicked() {
                    close = true;
                }
            });
        });

        if close {
            self.pending_drop = None;
        }
    }

    pub(super) fn about_window(&mut self, context: &egui::Context) {
        if !self.about_open {
            return;
        }
        let strings = self.strings();
        let mut open = self.about_open;

        egui::Window::new(strings.about.as_str())
            .id(egui::Id::unique("about_window"))
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .show(context, |ui| {
                ui.heading("rossresscut");
                ui.label(strings.about_description.as_str());
                ui.label(fill(&strings.about_version, &[env!("CARGO_PKG_VERSION")]));
                ui.label(fill(&strings.about_commit, &[env!("GIT_COMMIT")]));
            });

        self.about_open = open;
    }

    /// Asks whether to save a bound project before closing.
    pub(super) fn close_prompt_window(&mut self, context: &egui::Context) {
        let Some(intent) = self.pending_close else {
            return;
        };
        let strings = self.strings();
        let mut cancel = false;

        egui::Window::new(strings.unsaved_changes.as_str())
            .id(egui::Id::unique("close_prompt_window"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(context, |ui| {
                ui.label(strings.unsaved_changes_hint.as_str());
                if let Some(error) = &self.project_error {
                    ui.colored_label(ui.visuals().error_fg_color, error.as_str());
                }
                ui.horizontal(|ui| {
                    if ui.button(strings.save.as_str()).clicked() {
                        self.finish_close(intent, true, ui.ctx());
                    }
                    if ui.button(strings.discard.as_str()).clicked() {
                        self.finish_close(intent, false, ui.ctx());
                    }
                    if ui.button(strings.cancel.as_str()).clicked() {
                        cancel = true;
                    }
                });
            });

        if cancel {
            self.pending_close = None;
        }
    }
}

pub(super) fn export_command_message(strings: &Strings, error: &ExportCommandError) -> String {
    match error {
        ExportCommandError::NoSegment => strings.export_no_segment.clone(),
        ExportCommandError::InvalidOutput => strings.export_invalid_output.clone(),
        other => other.to_string(),
    }
}
