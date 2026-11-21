//! Floating windows: settings, about, export and the last command.

use std::path::PathBuf;

use eframe::egui;

use crate::application::{
    ExportCommandError, ExportOptions, SceneScan, SilenceError, SilenceOptions,
};
use crate::domain::media::MediaStream;
use crate::domain::time::Duration;
use crate::media::export::StreamSelection;
use crate::media::preview::PreviewBackend;
use crate::media::process::CommandPurpose;

use super::dialogs::DirectoryTarget;
use super::i18n::{Language, fill};
use super::settings::{detect_system_theme, save_settings, theme_label};
use super::shortcuts::SETTINGS_ROWS;
use super::{
    FONT_ZOOM_STEP, MAX_FONT_ZOOM, MIN_FONT_ZOOM, RossresscutApp, ShortcutAction, Strings,
    ThemeChoice,
};

const SHORTCUT_LABEL_WIDTH: f32 = 180.0;
const SHORTCUT_BUTTON_WIDTH: f32 = 115.0;
/// Label + binding button, including the horizontal spacing.
const SHORTCUT_CELL_WIDTH: f32 = SHORTCUT_LABEL_WIDTH + SHORTCUT_BUTTON_WIDTH + 8.0;
/// Height of one shortcut row, matching the button height.
const SHORTCUT_ROW_HEIGHT: f32 = 26.0;

/// Third-party components shown in the About window.
///
/// Mirrors the short table in `ATTRIBUTION.md`; keep both in sync when a
/// runtime dependency changes. `FFmpeg` and `libmpv` are not linked into this
/// binary, but the packaged builds ship them next to the executable, so their
/// licenses are relevant to distribution.
const THIRD_PARTY_COMPONENTS: &[(&str, &str)] = &[
    ("FFmpeg", "LGPL-2.1-or-later / GPL-2.0-or-later"),
    ("libmpv", "GPL-2.0-or-later"),
    ("egui / eframe", "MIT OR Apache-2.0"),
    ("wgpu", "MIT OR Apache-2.0"),
    ("cpal", "Apache-2.0"),
    ("rfd / fontdb", "MIT"),
    ("serde / serde_json / toml / log", "MIT OR Apache-2.0"),
    ("JetBrains Mono NF", "OFL-1.1"),
];

/// Pages shown as tabs in the independent settings window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum SettingsTab {
    /// Theme, language, fonts and project saving.
    #[default]
    General,
    /// Decoder backend and wheel step.
    Preview,
    /// Output directory, chapters and encoding.
    Export,
    /// Keyboard bindings.
    Shortcuts,
}

impl SettingsTab {
    const ALL: [Self; 4] = [Self::General, Self::Preview, Self::Export, Self::Shortcuts];

    fn label(self, strings: &Strings) -> &str {
        match self {
            Self::General => strings.general.as_str(),
            Self::Preview => strings.preview.as_str(),
            Self::Export => strings.export.as_str(),
            Self::Shortcuts => strings.shortcuts.as_str(),
        }
    }
}

/// Floating windows that can be the topmost dialog for keyboard input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DialogWindow {
    ClosePrompt,
    Silence,
    Random,
    Scene,
    Drop,
    Export,
    FfmpegCommand,
    About,
}

/// Localized purpose for one command-history entry.
fn command_purpose_label(strings: &Strings, purpose: CommandPurpose) -> &str {
    match purpose {
        CommandPurpose::Export => strings.ffmpeg_purpose_export.as_str(),
        CommandPurpose::SceneDetection => strings.ffmpeg_purpose_scene.as_str(),
        CommandPurpose::Waveform => strings.ffmpeg_purpose_waveform.as_str(),
        CommandPurpose::Subtitle => strings.ffmpeg_purpose_subtitle.as_str(),
        CommandPurpose::Merge => strings.ffmpeg_purpose_merge.as_str(),
        CommandPurpose::Probe => strings.ffmpeg_purpose_probe.as_str(),
        CommandPurpose::Keyframes => strings.ffmpeg_purpose_keyframes.as_str(),
        CommandPurpose::Preview => strings.ffmpeg_purpose_other.as_str(),
    }
}

impl RossresscutApp {
    /// Floating windows that can be the topmost dialog for keyboard input.
    ///
    /// Order matches the render order in `RossresscutApp::ui`; later windows
    /// are on top.
    fn topmost_dialog(&self) -> Option<DialogWindow> {
        if self.about_open {
            Some(DialogWindow::About)
        } else if self.ffmpeg_command_open {
            Some(DialogWindow::FfmpegCommand)
        } else if self.export_panel_open {
            Some(DialogWindow::Export)
        } else if self.pending_drop.is_some() {
            Some(DialogWindow::Drop)
        } else if self.scene_dialog_open {
            Some(DialogWindow::Scene)
        } else if self.random_dialog_open {
            Some(DialogWindow::Random)
        } else if self.silence_dialog_open {
            Some(DialogWindow::Silence)
        } else if self.pending_close.is_some() {
            Some(DialogWindow::ClosePrompt)
        } else {
            None
        }
    }

    /// Whether Enter should activate `dialog`'s primary action.
    ///
    /// Text fields keep Enter for themselves; only the topmost window reacts,
    /// so stacked dialogs cannot all confirm at once.
    fn dialog_enter(&self, context: &egui::Context, dialog: DialogWindow) -> bool {
        self.topmost_dialog() == Some(dialog)
            && !context.text_edit_focused()
            && context.input(|input| input.key_pressed(egui::Key::Enter))
    }

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
        let confirm = self.dialog_enter(context, DialogWindow::Export);

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
                    let export_clicked = ui
                        .add_enabled(can_export, egui::Button::new(strings.export.as_str()))
                        .clicked()
                        || (confirm && can_export);
                    if export_clicked && let Some(output) = output.clone() {
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
        let commands = self.application.executed_ffmpeg_commands();

        egui::Window::new(strings.executed_ffmpeg_commands.as_str())
            .id(egui::Id::unique("ffmpeg_command_window"))
            .open(&mut open)
            .resizable(true)
            .collapsible(false)
            .default_size(egui::vec2(760.0, 420.0))
            .show(context, |ui| {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(strings.command_history_hint.as_str()).small(),
                    )
                    .wrap(),
                );
                if commands.is_empty() {
                    ui.label(strings.no_ffmpeg_command.as_str());
                    return;
                }
                if ui.button(strings.copy_command.as_str()).clicked() {
                    let text = commands
                        .iter()
                        .map(|entry| entry.command_line.as_str())
                        .collect::<Vec<_>>()
                        .join("\n\n");
                    ui.ctx().copy_text(text);
                }
                ui.separator();
                egui::ScrollArea::vertical()
                    .id_salt("ffmpeg_command_history")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for entry in &commands {
                            ui.label(
                                egui::RichText::new(command_purpose_label(strings, entry.purpose))
                                    .small()
                                    .weak(),
                            );
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(entry.command_line.as_str()).monospace(),
                                )
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
        let mut open = true;
        let builder = egui::ViewportBuilder::default()
            .with_title(strings.settings.as_str())
            .with_inner_size([760.0, 620.0])
            .with_min_inner_size([560.0, 400.0])
            .with_decorations(!cfg!(target_os = "windows"))
            .with_icon(super::window_icon());
        context.show_viewport_immediate(
            egui::ViewportId::from_hash_of("settings_window"),
            builder,
            |ui, _class| {
                // The immediate viewport starts from a fresh Ui; re-apply the
                // theme and font zoom so it matches the main window.
                self.apply_theme(ui.ctx());
                self.window_resize_edges(ui.ctx());
                if self.settings_viewport(ui, strings) {
                    open = false;
                }
                if ui.ctx().input(|input| input.viewport().close_requested()) {
                    open = false;
                }
            },
        );

        if !open {
            self.settings_open = false;
            // `eframe` only calls `App::save` when persistence is configured, so
            // settings are written when the window is closed instead.
            save_settings(&self.settings);
        }
    }

    fn settings_viewport(&mut self, ui: &mut egui::Ui, strings: &'static Strings) -> bool {
        // The root Ui of an immediate viewport has no background of its own;
        // the central-panel frame supplies the theme fill instead of the
        // backend's black clear colour.
        let close = super::titlebar::draw_title_bar(ui, strings.settings.as_str(), false);
        egui::Frame::central_panel(ui.style()).show(ui, |ui| {
            self.settings_viewport_contents(ui, strings);
        });
        close
    }

    fn settings_viewport_contents(&mut self, ui: &mut egui::Ui, strings: &'static Strings) {
        // Seven backends fit without scrolling in the preview tab.
        ui.spacing_mut().combo_height = 320.0;
        // Key recording happens in this viewport: when the settings window has
        // focus, the main window receives no key events.
        if self.recording_shortcut.is_some() {
            self.capture_shortcut(ui.ctx());
        }
        #[cfg(not(target_os = "windows"))]
        ui.horizontal(|ui| {
            ui.heading(strings.settings.as_str());
        });
        ui.separator();
        ui.horizontal(|ui| {
            for tab in SettingsTab::ALL {
                ui.selectable_value(&mut self.settings_tab, tab, tab.label(strings));
            }
        });
        ui.separator();

        egui::ScrollArea::vertical()
            .id_salt("settings_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| match self.settings_tab {
                SettingsTab::General => self.general_settings_ui(ui, strings),
                SettingsTab::Preview => self.preview_settings_ui(ui, strings),
                SettingsTab::Export => self.export_settings_ui(ui, strings),
                SettingsTab::Shortcuts => self.shortcut_settings_ui(ui, strings),
            });
    }

    fn general_settings_ui(&mut self, ui: &mut egui::Ui, strings: &Strings) {
        let follow = ui
            .checkbox(
                &mut self.settings.follow_system_theme,
                strings.follow_system_theme.as_str(),
            )
            .on_hover_text(strings.follow_system_theme.as_str());
        if follow.changed() {
            // Re-detect immediately instead of reusing the value captured at
            // startup.
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
                egui::Slider::new(&mut self.font_zoom_draft, MIN_FONT_ZOOM..=MAX_FONT_ZOOM)
                    .step_by(f64::from(FONT_ZOOM_STEP))
                    .fixed_decimals(2),
            );
            // The preview only follows a released slider; this avoids rebuilding
            // the font atlas on every drag frame.
            if slider.drag_stopped() || (slider.changed() && !slider.dragged()) {
                self.settings.font_zoom = self.font_zoom_draft.clamp(MIN_FONT_ZOOM, MAX_FONT_ZOOM);
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
                        egui::TextEdit::singleline(&mut self.settings.project_directory)
                            .desired_width(360.0)
                            .hint_text(strings.project_directory.as_str()),
                    );
                });
            });
        });
    }

    fn preview_settings_ui(&mut self, ui: &mut egui::Ui, strings: &Strings) {
        ui.horizontal(|ui| {
            ui.label(strings.preview_backend.as_str());
            let mut backend = self.settings.preview_backend;
            egui::ComboBox::from_id_salt("preview_backend")
                .selected_text(backend.label())
                .show_ui(ui, |ui| {
                    for candidate in PreviewBackend::available() {
                        ui.selectable_value(&mut backend, *candidate, candidate.label());
                    }
                });
            if backend != self.settings.preview_backend {
                self.settings.preview_backend = backend;
                self.application.set_preview_backend(backend);
            }
        })
        .response
        .on_hover_text(strings.preview_backend_hint.as_str());

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(strings.preview_seek_step.as_str());
            let mut seconds = self.settings.preview_seek_micros as f64 / 1_000_000.0;
            let response = ui.add(
                egui::DragValue::new(&mut seconds)
                    .range(0.1..=600.0)
                    .speed(0.1)
                    .fixed_decimals(2)
                    .suffix(" s"),
            );
            let response = response.on_hover_text(strings.preview_seek_step_hint.as_str());
            if response.changed() {
                self.settings.preview_seek_micros = (seconds * 1_000_000.0).round() as i64;
            }
        });
    }

    fn export_settings_ui(&mut self, ui: &mut egui::Ui, strings: &Strings) {
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
                egui::TextEdit::singleline(&mut self.settings.export_title).desired_width(320.0),
            )
            .on_hover_text(strings.export_title_hint.as_str());
        });
        ui.checkbox(&mut self.settings.smart_cut, strings.smart_cut.as_str())
            .on_hover_text(strings.smart_cut_hint.as_str());
        ui.add_enabled_ui(self.settings.smart_cut, |ui| {
            ui.horizontal(|ui| {
                ui.label(strings.smart_cut_bit_rate.as_str());
                ui.add(
                    egui::DragValue::new(&mut self.settings.smart_cut_bit_rate_kbps)
                        .range(0..=1_000_000)
                        .speed(50)
                        .suffix(" kbps"),
                )
                .on_hover_text(strings.smart_cut_bit_rate_hint.as_str());
            });
        });
        ui.add(
            egui::Label::new(egui::RichText::new(strings.export_join_hint.as_str()).small()).wrap(),
        );
    }

    fn shortcut_settings_ui(&mut self, ui: &mut egui::Ui, strings: &Strings) {
        let row_width = ui.available_width();
        ui.add(
            egui::Label::new(egui::RichText::new(strings.shortcut_hint.as_str()).small()).wrap(),
        );
        let clip_rect = ui.clip_rect();
        let row_visible = |ui: &egui::Ui| {
            let top = ui.cursor().min.y;
            top + SHORTCUT_ROW_HEIGHT > clip_rect.top() && top < clip_rect.bottom()
        };
        let skip_row = |ui: &mut egui::Ui| {
            ui.allocate_space(egui::vec2(row_width, SHORTCUT_ROW_HEIGHT));
        };
        if row_width >= SHORTCUT_CELL_WIDTH * 2.0 {
            for row in SETTINGS_ROWS {
                if !row_visible(ui) {
                    skip_row(ui);
                    continue;
                }
                ui.allocate_ui_with_layout(
                    egui::vec2(row_width, SHORTCUT_ROW_HEIGHT),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        for action in row {
                            match action {
                                Some(action) => {
                                    self.shortcut_editor(ui, *action, strings);
                                }
                                None => {
                                    ui.add_space(SHORTCUT_CELL_WIDTH);
                                }
                            }
                        }
                    },
                );
            }
        } else {
            for action in SETTINGS_ROWS.iter().flatten().flatten() {
                if !row_visible(ui) {
                    skip_row(ui);
                    continue;
                }
                ui.allocate_ui_with_layout(
                    egui::vec2(row_width, SHORTCUT_ROW_HEIGHT),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| self.shortcut_editor(ui, *action, strings),
                );
            }
        }
    }

    /// One shortcut cell: a label followed by its binding button.
    ///
    /// Clicking the button starts recording; Backspace clears the binding while
    /// recording. No per-row clear button is drawn, because reserving space for
    /// it would push the second column out of alignment.
    fn shortcut_editor(&mut self, ui: &mut egui::Ui, action: ShortcutAction, strings: &Strings) {
        ui.add_sized(
            [SHORTCUT_LABEL_WIDTH, 20.0],
            egui::Label::new(action.label(strings)).truncate(),
        );

        let recording = self.recording_shortcut == Some(action);
        let binding = self.settings.binding(action).to_owned();
        let text = if recording {
            strings.shortcut_press_keys.clone()
        } else if binding.is_empty() {
            strings.shortcut_unassigned.clone()
        } else {
            binding
        };
        if ui
            .add(egui::Button::new(text).min_size(egui::vec2(SHORTCUT_BUTTON_WIDTH, 26.0)))
            .on_hover_text(strings.shortcut_press_keys.as_str())
            .clicked()
        {
            self.recording_shortcut = if recording { None } else { Some(action) };
            self.recording_pending = None;
        }
    }

    /// Opens the scene detection dialog.
    ///
    /// Analysis starts when the user presses the primary button, so the same
    /// button can read "Start" first and "Apply" after the scan completes.
    pub(super) fn open_scene_dialog(&mut self) {
        self.scene_dialog_open = true;
        self.application.reset_scene_scan();
    }

    pub(super) fn scene_window(&mut self, context: &egui::Context) {
        if !self.scene_dialog_open {
            return;
        }
        let strings = self.strings();
        let mut open = self.scene_dialog_open;
        let mut close_requested = false;
        let status = self.application.scene_scan().cloned();
        let confirm = self.dialog_enter(context, DialogWindow::Scene);

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
                    Some(SceneScan::Running(percent)) => {
                        ui.add(
                            egui::ProgressBar::new(f32::from(*percent) / 100.0)
                                .text(format!("{} {percent}%", strings.scene_scanning)),
                        );
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
                        ui.label(strings.scene_start_hint.as_str());
                    }
                }

                ui.horizontal(|ui| {
                    let ready = matches!(status, Some(SceneScan::Ready(_)));
                    let running = matches!(status, Some(SceneScan::Running(_)));
                    let label = if ready {
                        strings.apply.as_str()
                    } else {
                        strings.start.as_str()
                    };
                    let primary = ui.add_enabled(!running, egui::Button::new(label)).clicked()
                        || (confirm && !running);
                    if primary {
                        if ready {
                            match self.application.segments_from_scene_changes() {
                                Ok(count) => {
                                    log::info!("created {count} segments from scene changes");
                                    self.segment_error = None;
                                    close_requested = true;
                                }
                                Err(error) => self.segment_error = Some(error.to_string()),
                            }
                        } else {
                            self.application.start_scene_scan(self.scene_threshold);
                        }
                    }
                    if status.is_some() && ui.button(strings.scene_rescan.as_str()).clicked() {
                        self.application.start_scene_scan(self.scene_threshold);
                    }
                    if ui.button(strings.cancel.as_str()).clicked() {
                        close_requested = true;
                    }
                });
            });

        if close_requested || !open {
            // Closing with X, Escape or Cancel must stop the FFmpeg process;
            // dropping the result channel alone leaves it running.
            self.application.cancel_scene_scan();
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

    fn silence_preview_for(&self, options: SilenceOptions) -> Result<usize, SilenceError> {
        self.application.count_silence_segments(options)
    }

    fn silence_error_text(&self, error: SilenceError) -> String {
        let strings = self.strings();
        match error {
            SilenceError::NoWaveform => strings.silence_no_waveform.clone(),
            SilenceError::NoAudibleRange | SilenceError::NoDuration => {
                strings.silence_no_audible.clone()
            }
        }
    }

    pub(super) fn silence_window(&mut self, context: &egui::Context) {
        if !self.silence_dialog_open {
            return;
        }
        let strings = self.strings();
        let mut open = self.silence_dialog_open;
        let mut recompute = false;
        let mut close_requested = false;
        // The dialog can be opened before the waveform worker finishes. Once the
        // waveform arrives, replace the stale "no waveform" result.
        if matches!(self.silence_preview, Some(Err(SilenceError::NoWaveform)))
            && self.application.waveform().is_some()
        {
            recompute = true;
        }
        let confirm = self.dialog_enter(context, DialogWindow::Silence);

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
                        Err(error) => {
                            let message = self.silence_error_text(*error);
                            ui.colored_label(ui.visuals().error_fg_color, message);
                        }
                    }
                }

                ui.horizontal(|ui| {
                    if ui.button(strings.apply.as_str()).clicked() || confirm {
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
        let confirm = self.dialog_enter(context, DialogWindow::Random);

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
                    if ui.button(strings.create.as_str()).clicked() || confirm {
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
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(strings.about_license.as_str());
                    ui.monospace("GPL-3.0-or-later");
                });
                egui::CollapsingHeader::new(strings.about_third_party.as_str())
                    .id_salt("about_third_party")
                    .default_open(false)
                    .show(ui, |ui| {
                        egui::Grid::new("about_third_party_grid")
                            .num_columns(2)
                            .spacing([16.0, 4.0])
                            .show(ui, |ui| {
                                for (component, license) in THIRD_PARTY_COMPONENTS {
                                    ui.monospace(*component);
                                    ui.monospace(*license);
                                    ui.end_row();
                                }
                            });
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(strings.about_third_party_hint.as_str())
                                    .small(),
                            )
                            .wrap(),
                        );
                    });
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
        let confirm = self.dialog_enter(context, DialogWindow::ClosePrompt);

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
                    if ui.button(strings.save.as_str()).clicked() || confirm {
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
