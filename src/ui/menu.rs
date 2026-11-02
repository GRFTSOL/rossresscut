//! Menu bar and its entries.

use eframe::egui;

use crate::application::OpenState;

use super::dialogs::{FileDialogPurpose, ProjectDialogMode};
use super::{FILE_DIALOG_SUPPORTED, FONT_ZOOM_STEP, RossresscutApp, ShortcutAction};

impl RossresscutApp {
    pub(super) fn menu_bar(&mut self, ui: &mut egui::Ui) {
        let strings = self.strings();
        egui::Panel::top("menu_bar").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.spacing_mut().button_padding = egui::vec2(10.0, 6.0);
                ui.menu_button(strings.menu_file.as_str(), |ui| {
                    let dialog_open = self.file_dialog.is_some();
                    let open_enabled = FILE_DIALOG_SUPPORTED && !dialog_open;
                    let open_label = if dialog_open {
                        strings.opening.as_str()
                    } else {
                        strings.open.as_str()
                    };
                    let open = ui
                        .add_enabled(open_enabled, egui::Button::new(open_label))
                        .on_disabled_hover_text(if FILE_DIALOG_SUPPORTED {
                            strings.open_in_progress.as_str()
                        } else {
                            strings.open_disabled.as_str()
                        })
                        .on_hover_text(strings.open.as_str());
                    if open.clicked() {
                        self.start_file_dialog(FileDialogPurpose::OpenMedia);
                        ui.close();
                    }

                    let has_media = !matches!(self.application.state(), OpenState::Empty);
                    if ui
                        .add_enabled(has_media, egui::Button::new(strings.close_media.as_str()))
                        .on_hover_text(strings.close_media.as_str())
                        .clicked()
                    {
                        self.application.close_media();
                        self.reset_scrub();
                        self.segment_error = None;
                        ui.close();
                    }

                    ui.separator();
                    let has_source = self.application.timeline().source().is_some();
                    if ui
                        .add_enabled(
                            has_source,
                            egui::Button::new(strings.export_project.as_str()),
                        )
                        .on_disabled_hover_text(strings.no_media.as_str())
                        .on_hover_text(strings.export_project.as_str())
                        .clicked()
                    {
                        self.start_project_dialog(ProjectDialogMode::Save);
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            FILE_DIALOG_SUPPORTED && self.project_dialog.is_none(),
                            egui::Button::new(strings.import_project.as_str()),
                        )
                        .on_hover_text(strings.import_project.as_str())
                        .clicked()
                    {
                        self.start_project_dialog(ProjectDialogMode::Open);
                        ui.close();
                    }
                    if has_source
                        && matches!(self.application.state(), OpenState::Failed { .. })
                        && ui
                            .button(strings.rebind_source.as_str())
                            .on_hover_text(strings.rebind_source.as_str())
                            .clicked()
                    {
                        self.start_file_dialog(FileDialogPurpose::RebindSource);
                        ui.close();
                    }

                    ui.separator();
                    if ui
                        .button(strings.quit.as_str())
                        .on_hover_text(strings.quit.as_str())
                        .clicked()
                    {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });

                ui.menu_button(strings.menu_edit.as_str(), |ui| {
                    let undo_tooltip =
                        self.shortcut_tooltip(strings.undo.as_str(), ShortcutAction::Undo);
                    if menu_button(
                        ui,
                        strings.undo.as_str(),
                        &undo_tooltip,
                        self.application.can_undo(),
                    ) {
                        self.run_shortcut(ShortcutAction::Undo);
                        ui.close();
                    }
                    let redo_tooltip =
                        self.shortcut_tooltip(strings.redo.as_str(), ShortcutAction::Redo);
                    if menu_button(
                        ui,
                        strings.redo.as_str(),
                        &redo_tooltip,
                        self.application.can_redo(),
                    ) {
                        self.run_shortcut(ShortcutAction::Redo);
                        ui.close();
                    }
                    ui.separator();
                    let has_selection = self.application.selected_segment().is_some();
                    let cut_tooltip =
                        self.shortcut_tooltip(strings.cut.as_str(), ShortcutAction::Cut);
                    if menu_button(ui, strings.cut.as_str(), &cut_tooltip, has_selection) {
                        self.run_shortcut(ShortcutAction::Cut);
                        ui.close();
                    }
                    let copy_tooltip =
                        self.shortcut_tooltip(strings.copy.as_str(), ShortcutAction::Copy);
                    if menu_button(ui, strings.copy.as_str(), &copy_tooltip, has_selection) {
                        self.run_shortcut(ShortcutAction::Copy);
                        ui.close();
                    }
                    let paste_tooltip =
                        self.shortcut_tooltip(strings.paste.as_str(), ShortcutAction::Paste);
                    if menu_button(
                        ui,
                        strings.paste.as_str(),
                        &paste_tooltip,
                        self.application.media().is_some(),
                    ) {
                        self.run_shortcut(ShortcutAction::Paste);
                        ui.close();
                    }
                    ui.separator();
                    let select_all_tooltip = self
                        .shortcut_tooltip(strings.select_all.as_str(), ShortcutAction::SelectAll);
                    if menu_button(
                        ui,
                        strings.select_all.as_str(),
                        &select_all_tooltip,
                        !self.application.timeline().is_empty(),
                    ) {
                        self.run_shortcut(ShortcutAction::SelectAll);
                        ui.close();
                    }
                });

                ui.menu_button(strings.menu_segment.as_str(), |ui| {
                    let has_media = self.application.media().is_some();
                    if menu_button(
                        ui,
                        strings.auto_cut_silence.as_str(),
                        strings.auto_cut_silence.as_str(),
                        has_media,
                    ) {
                        self.open_silence_dialog();
                        ui.close();
                    }
                    let random_tooltip = self.shortcut_tooltip(
                        strings.random_segment.as_str(),
                        ShortcutAction::RandomSegment,
                    );
                    if menu_button(
                        ui,
                        strings.random_segment.as_str(),
                        &random_tooltip,
                        has_media,
                    ) {
                        self.run_shortcut(ShortcutAction::RandomSegment);
                        ui.close();
                    }
                    ui.separator();
                    let sort_tooltip = self.shortcut_tooltip(
                        strings.sort_segments.as_str(),
                        ShortcutAction::SortSegments,
                    );
                    if menu_button(
                        ui,
                        strings.sort_segments.as_str(),
                        &sort_tooltip,
                        self.application.timeline().len() > 1,
                    ) {
                        self.run_shortcut(ShortcutAction::SortSegments);
                        ui.close();
                    }
                    let shuffle_tooltip = self.shortcut_tooltip(
                        strings.shuffle_segments.as_str(),
                        ShortcutAction::ShuffleSegments,
                    );
                    if menu_button(
                        ui,
                        strings.shuffle_segments.as_str(),
                        &shuffle_tooltip,
                        self.application.timeline().len() > 1,
                    ) {
                        self.run_shortcut(ShortcutAction::ShuffleSegments);
                        ui.close();
                    }
                    ui.separator();
                    let merge_tooltip = self.shortcut_tooltip(
                        strings.merge_overlapping.as_str(),
                        ShortcutAction::MergeOverlapping,
                    );
                    if menu_button(
                        ui,
                        strings.merge_overlapping.as_str(),
                        &merge_tooltip,
                        self.application.timeline().len() > 1,
                    ) {
                        self.run_shortcut(ShortcutAction::MergeOverlapping);
                        ui.close();
                    }
                    let reverse_tooltip = self.shortcut_tooltip(
                        strings.invert_segments.as_str(),
                        ShortcutAction::InvertSegments,
                    );
                    if menu_button(
                        ui,
                        strings.invert_segments.as_str(),
                        &reverse_tooltip,
                        self.application.timeline().len() > 1,
                    ) {
                        self.run_shortcut(ShortcutAction::InvertSegments);
                        ui.close();
                    }
                    ui.separator();
                    let clear_tooltip = self.shortcut_tooltip(
                        strings.clear_segments.as_str(),
                        ShortcutAction::ClearSegments,
                    );
                    if menu_button(
                        ui,
                        strings.clear_segments.as_str(),
                        &clear_tooltip,
                        !self.application.timeline().is_empty(),
                    ) {
                        self.run_shortcut(ShortcutAction::ClearSegments);
                        ui.close();
                    }
                });

                ui.menu_button(strings.menu_tools.as_str(), |ui| {
                    let command_tooltip = self.shortcut_tooltip(
                        strings.last_ffmpeg_command.as_str(),
                        ShortcutAction::ShowLastCommand,
                    );
                    if menu_button(
                        ui,
                        strings.last_ffmpeg_command.as_str(),
                        &command_tooltip,
                        true,
                    ) {
                        self.run_shortcut(ShortcutAction::ShowLastCommand);
                        ui.close();
                    }
                });

                ui.menu_button(strings.menu_view.as_str(), |ui| {
                    let fullscreen = ui
                        .ctx()
                        .input(|input| input.viewport().fullscreen.unwrap_or(false));
                    let fullscreen_label = if fullscreen {
                        strings.exit_fullscreen.as_str()
                    } else {
                        strings.fullscreen.as_str()
                    };
                    if ui
                        .button(fullscreen_label)
                        .on_hover_text(fullscreen_label)
                        .clicked()
                    {
                        ui.ctx()
                            .send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fullscreen));
                        ui.close();
                    }
                    ui.separator();
                    if ui
                        .button(strings.reset_font_size.as_str())
                        .on_hover_text(strings.reset_font_size.as_str())
                        .clicked()
                    {
                        self.set_font_zoom(1.0);
                        ui.close();
                    }
                    if ui
                        .button(strings.increase_font_size.as_str())
                        .on_hover_text(strings.increase_font_size.as_str())
                        .clicked()
                    {
                        let zoom = self.settings.font_zoom + FONT_ZOOM_STEP;
                        self.set_font_zoom(zoom);
                        ui.close();
                    }
                    if ui
                        .button(strings.decrease_font_size.as_str())
                        .on_hover_text(strings.decrease_font_size.as_str())
                        .clicked()
                    {
                        let zoom = self.settings.font_zoom - FONT_ZOOM_STEP;
                        self.set_font_zoom(zoom);
                        ui.close();
                    }
                });

                ui.menu_button(strings.menu_help.as_str(), |ui| {
                    if ui
                        .button(strings.about.as_str())
                        .on_hover_text(strings.about.as_str())
                        .clicked()
                    {
                        self.about_open = true;
                        ui.close();
                    }
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(egui::Button::new("⚙").min_size(egui::vec2(32.0, 28.0)))
                        .on_hover_text(self.shortcut_tooltip(
                            strings.settings.as_str(),
                            ShortcutAction::OpenSettings,
                        ))
                        .clicked()
                    {
                        self.open_settings();
                    }
                    let title = self
                        .media_title()
                        .unwrap_or_else(|| strings.no_media.clone());
                    ui.add(egui::Label::new(title).truncate());
                });
            });
        });
    }
}

/// Menu item with an enable flag and a tooltip that carries the shortcut.
pub(super) fn menu_button(ui: &mut egui::Ui, label: &str, tooltip: &str, enabled: bool) -> bool {
    ui.add_enabled(enabled, egui::Button::new(label))
        .on_hover_text(tooltip)
        .clicked()
}
