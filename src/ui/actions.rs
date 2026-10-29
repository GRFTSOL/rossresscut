//! Shortcut dispatch and segment input handling.

use eframe::egui;

use crate::application::OpenState;

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

    pub(super) fn run_shortcut(&mut self, action: ShortcutAction) {
        match action {
            ShortcutAction::PlayPause => self.application.toggle_playback(),
            ShortcutAction::AddCut => {
                let position = self.application.position();
                if let Err(error) = self.application.add_cut_point(position) {
                    self.segment_error = Some(error.to_string());
                } else {
                    self.segment_error = None;
                    self.segment_input_segment = None;
                }
            }
            ShortcutAction::RemoveCut => {
                self.application.remove_cut_point();
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
            ShortcutAction::CloseMedia => {
                if !matches!(self.application.state(), OpenState::Empty) {
                    self.application.close_media();
                    self.reset_scrub();
                    self.segment_error = None;
                }
            }
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
            ShortcutAction::SetSegmentStart => self.set_selected_start_from_playhead(),
            ShortcutAction::SetSegmentEnd => self.set_selected_end_from_playhead(),
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
                if let Err(error) = self.application.add_random_segment() {
                    self.segment_error = Some(error.to_string());
                } else {
                    self.segment_error = None;
                    self.segment_input_segment = None;
                }
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
