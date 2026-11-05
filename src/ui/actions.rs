//! Shortcut dispatch and segment input handling.

use eframe::egui;

use super::shortcuts::{Shortcut, is_modifier_key};
use super::timeline::parse_time_input;
use super::{RossresscutApp, ShortcutAction};

impl RossresscutApp {
    pub(super) fn sync_segment_inputs(&mut self) {
        let selected = self.application.selected_segment();
        if selected == self.segment_input_segment {
            return;
        }
        self.segment_input_segment = selected;
        match selected.and_then(|id| self.application.timeline().get(id).copied()) {
            Some(segment) => {
                self.segment_start_input = segment.start().to_string();
                self.segment_end_input = segment.end().to_string();
            }
            None => {
                self.segment_start_input.clear();
                self.segment_end_input.clear();
            }
        }
    }

    pub(super) fn apply_segment_start_input(&mut self) {
        let Some(time) = parse_time_input(&self.segment_start_input) else {
            self.segment_error = Some("invalid time".to_owned());
            return;
        };
        if let Err(error) = self.application.set_selected_segment_start(time) {
            self.segment_error = Some(error.to_string());
            return;
        }
        self.application.seek(time);
        self.segment_input_segment = None;
        self.segment_error = None;
    }

    pub(super) fn apply_segment_end_input(&mut self) {
        let Some(time) = parse_time_input(&self.segment_end_input) else {
            self.segment_error = Some("invalid time".to_owned());
            return;
        };
        if let Err(error) = self.application.set_selected_segment_end(time) {
            self.segment_error = Some(error.to_string());
            return;
        }
        self.segment_input_segment = None;
        self.segment_error = None;
    }

    pub(super) fn set_selected_start_from_playhead(&mut self) {
        let position = self.application.position();
        if let Err(error) = self.application.set_selected_segment_start(position) {
            self.segment_error = Some(error.to_string());
        } else {
            self.segment_input_segment = None;
            self.segment_error = None;
        }
    }

    pub(super) fn set_selected_end_from_playhead(&mut self) {
        let position = self.application.position();
        if let Err(error) = self.application.set_selected_segment_end(position) {
            self.segment_error = Some(error.to_string());
        } else {
            self.segment_input_segment = None;
            self.segment_error = None;
        }
    }

    pub(super) fn handle_shortcuts(&mut self, context: &egui::Context) {
        if self.recording_shortcut.is_some() {
            self.capture_shortcut(context);
            return;
        }
        if context.egui_wants_keyboard_input() {
            return;
        }

        // Escape closes the topmost floating window before any other binding.
        // Text fields keep Escape for themselves (checked above).
        if context.input(|input| input.key_pressed(egui::Key::Escape)) && self.close_top_window() {
            return;
        }

        let actions: Vec<ShortcutAction> = context.input(|input| {
            ShortcutAction::ALL
                .iter()
                .copied()
                .filter(|action| {
                    self.settings
                        .shortcut(*action)
                        .is_some_and(|shortcut| shortcut.matches(input))
                })
                .collect()
        });
        for action in actions {
            self.run_shortcut(action);
        }
    }

    /// Closes the topmost floating window, if any.
    ///
    /// Order follows the render order in `RossresscutApp::ui`, last window
    /// first. Returns whether a window was closed.
    fn close_top_window(&mut self) -> bool {
        if self.pending_close.is_some() {
            self.pending_close = None;
            return true;
        }
        if self.about_open {
            self.about_open = false;
            return true;
        }
        if self.ffmpeg_command_open {
            self.ffmpeg_command_open = false;
            return true;
        }
        if self.export_panel_open {
            self.export_panel_open = false;
            return true;
        }
        if self.volume_popup_open {
            self.volume_popup_open = false;
            super::settings::save_settings(&self.settings);
            return true;
        }
        if self.random_dialog_open {
            self.random_dialog_open = false;
            return true;
        }
        if self.pending_drop.is_some() {
            self.pending_drop = None;
            return true;
        }
        if self.scene_dialog_open {
            self.scene_dialog_open = false;
            return true;
        }
        if self.silence_dialog_open {
            self.silence_dialog_open = false;
            return true;
        }
        if self.settings_open {
            self.settings_open = false;
            super::settings::save_settings(&self.settings);
            return true;
        }
        false
    }

    pub(super) fn run_shortcut(&mut self, action: ShortcutAction) {
        match action {
            ShortcutAction::PlayPause => self.application.toggle_playback(),
            ShortcutAction::AddCut => {
                let position = self.application.position();
                if let Err(error) = self.application.add_cut_point_or_set_start(position) {
                    self.segment_error = Some(error.to_string());
                } else {
                    self.segment_error = None;
                    self.segment_input_segment = None;
                }
            }
            ShortcutAction::RemoveCut => {
                let position = self.application.position();
                if let Err(error) = self.application.set_end_or_complete_cut_point(position) {
                    self.segment_error = Some(error.to_string());
                } else {
                    self.segment_error = None;
                    self.segment_input_segment = None;
                }
            }
            ShortcutAction::DeleteSegment => {
                self.application.remove_selected_segment();
                self.segment_error = None;
                self.segment_input_segment = None;
            }
            ShortcutAction::Split => {
                if let Err(error) = self.application.split_selected_segment() {
                    self.segment_error = Some(error.to_string());
                } else {
                    self.segment_error = None;
                    self.segment_input_segment = None;
                }
            }
            ShortcutAction::ClearMarkers => {
                self.application.clear_markers();
                self.segment_error = None;
            }
            ShortcutAction::CloseMedia => self.request_close_media(),
            ShortcutAction::OpenSettings => self.open_settings(),
            ShortcutAction::GoToStart => self.application.seek_to_start(),
            ShortcutAction::PreviousSegment => self.application.select_previous_segment(),
            ShortcutAction::SegmentStart => self.application.seek_to_selected_segment_start(),
            ShortcutAction::PreviousKeyframe => self.application.step_keyframe(false),
            ShortcutAction::PreviousFrame => self.application.step_frame(false),
            ShortcutAction::NextFrame => self.application.step_frame(true),
            ShortcutAction::NextKeyframe => self.application.step_keyframe(true),
            ShortcutAction::SegmentEnd => self.application.seek_to_selected_segment_end(),
            ShortcutAction::NextSegment => self.application.select_next_segment(),
            ShortcutAction::GoToEnd => self.application.seek_to_end(),
            ShortcutAction::Undo => {
                self.application.undo();
                self.segment_input_segment = None;
            }
            ShortcutAction::Redo => {
                self.application.redo();
                self.segment_input_segment = None;
            }
            ShortcutAction::Cut => {
                if self.application.cut_selected_segments() > 0 {
                    self.segment_error = None;
                    self.segment_input_segment = None;
                }
            }
            ShortcutAction::Copy => {
                self.application.copy_selected_segments();
            }
            ShortcutAction::Paste => {
                self.application.paste_segments();
                self.segment_input_segment = None;
            }
            ShortcutAction::SelectAll => self.application.select_all_segments(),
            ShortcutAction::RandomSegment => {
                self.random_dialog_open = true;
            }
            ShortcutAction::SortSegments => {
                self.application.sort_segments();
                self.segment_input_segment = None;
            }
            ShortcutAction::ShuffleSegments => {
                self.application.shuffle_segments();
                self.segment_input_segment = None;
            }
            ShortcutAction::MergeOverlapping => {
                self.application.merge_overlapping_segments();
                self.segment_input_segment = None;
            }
            ShortcutAction::InvertSegments => {
                self.application.invert_segments();
                self.segment_input_segment = None;
            }
            ShortcutAction::ClearSegments => {
                self.application.clear_segments();
                self.segment_input_segment = None;
                self.segment_error = None;
            }
            ShortcutAction::ShowLastCommand => self.ffmpeg_command_open = true,
        }
    }

    fn capture_shortcut(&mut self, context: &egui::Context) {
        let Some(action) = self.recording_shortcut else {
            return;
        };
        let events: Vec<(egui::Key, bool, egui::Modifiers)> = context.input(|input| {
            input
                .events
                .iter()
                .filter_map(|event| match event {
                    egui::Event::Key {
                        key,
                        pressed,
                        modifiers,
                        ..
                    } => Some((*key, *pressed, *modifiers)),
                    _ => None,
                })
                .collect()
        });
        for (key, pressed, modifiers) in events {
            // Modifier keys arrive as ordinary key events; they must not
            // become the recorded key themselves.
            if is_modifier_key(key) {
                continue;
            }
            if pressed {
                self.recording_pending = Some((action, key, modifiers));
            } else if let Some((pending_action, pending_key, pending_modifiers)) =
                self.recording_pending
                && pending_action == action
                && pending_key == key
            {
                let shortcut = Shortcut {
                    key,
                    command: pending_modifiers.command || pending_modifiers.ctrl,
                    shift: pending_modifiers.shift,
                    alt: pending_modifiers.alt,
                };
                self.settings
                    .shortcuts
                    .insert(action.key().to_owned(), shortcut.label());
                self.recording_shortcut = None;
                self.recording_pending = None;
                return;
            }
        }
    }

    pub(super) fn shortcut_tooltip(&self, label: &str, action: ShortcutAction) -> String {
        let binding = self.settings.binding(action);
        if binding.is_empty() {
            label.to_owned()
        } else {
            format!("{label} ({binding})")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_closes_one_window_at_a_time() {
        let mut app = RossresscutApp {
            about_open: true,
            export_panel_open: true,
            ..Default::default()
        };

        assert!(app.close_top_window());
        assert!(!app.about_open, "the last rendered window closes first");
        assert!(app.export_panel_open);

        assert!(app.close_top_window());
        assert!(!app.export_panel_open);
        assert!(!app.close_top_window(), "nothing left to close");
    }

    #[test]
    fn escape_cancels_the_close_prompt_first() {
        let mut app = RossresscutApp {
            about_open: true,
            pending_close: Some(crate::ui::dialogs::CloseIntent::Media),
            ..Default::default()
        };

        assert!(app.close_top_window());
        assert!(app.pending_close.is_none());
        assert!(app.about_open, "the prompt has priority over other windows");
    }
}
