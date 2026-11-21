//! Keyboard shortcuts: actions, bindings and parsing.

use std::collections::BTreeMap;

use eframe::egui;

use super::i18n::Strings;

/// Keyboard actions that can be rebound in settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ShortcutAction {
    PlayPause,
    AddCut,
    RemoveCut,
    DeleteSegment,
    Split,
    ClearMarkers,
    CloseMedia,
    OpenSettings,
    OpenMedia,
    ExportProject,
    ImportProject,
    RebindSource,
    Quit,
    GoToStart,
    PreviousSegment,
    SegmentStart,
    PreviousSecond,
    PreviousFrame,
    NextFrame,
    NextSecond,
    SegmentEnd,
    NextSegment,
    GoToEnd,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    RandomSegment,
    SortSegments,
    ShuffleSegments,
    MergeOverlapping,
    InvertSegments,
    ClearSegments,
    ShowLastCommand,
    AutoCutSilence,
    SceneDetection,
    Fullscreen,
    ResetFontSize,
    IncreaseFontSize,
    DecreaseFontSize,
    About,
}

impl ShortcutAction {
    pub(super) const ALL: [Self; 43] = [
        Self::PlayPause,
        Self::AddCut,
        Self::RemoveCut,
        Self::DeleteSegment,
        Self::Split,
        Self::ClearMarkers,
        Self::CloseMedia,
        Self::OpenSettings,
        Self::OpenMedia,
        Self::ExportProject,
        Self::ImportProject,
        Self::RebindSource,
        Self::Quit,
        Self::GoToStart,
        Self::PreviousSegment,
        Self::SegmentStart,
        Self::PreviousSecond,
        Self::PreviousFrame,
        Self::NextFrame,
        Self::NextSecond,
        Self::SegmentEnd,
        Self::NextSegment,
        Self::GoToEnd,
        Self::Undo,
        Self::Redo,
        Self::Cut,
        Self::Copy,
        Self::Paste,
        Self::SelectAll,
        Self::RandomSegment,
        Self::SortSegments,
        Self::ShuffleSegments,
        Self::MergeOverlapping,
        Self::InvertSegments,
        Self::ClearSegments,
        Self::ShowLastCommand,
        Self::AutoCutSilence,
        Self::SceneDetection,
        Self::Fullscreen,
        Self::ResetFontSize,
        Self::IncreaseFontSize,
        Self::DecreaseFontSize,
        Self::About,
    ];

    pub(super) const fn key(self) -> &'static str {
        match self {
            Self::PlayPause => "play_pause",
            Self::AddCut => "add_cut",
            Self::RemoveCut => "remove_cut",
            Self::DeleteSegment => "delete_segment",
            Self::Split => "split",
            Self::ClearMarkers => "clear_markers",
            Self::CloseMedia => "close_media",
            Self::OpenSettings => "open_settings",
            Self::OpenMedia => "open_media",
            Self::ExportProject => "export_project",
            Self::ImportProject => "import_project",
            Self::RebindSource => "rebind_source",
            Self::Quit => "quit",
            Self::GoToStart => "go_to_start",
            Self::PreviousSegment => "previous_segment",
            Self::SegmentStart => "segment_start",
            Self::PreviousSecond => "previous_second",
            Self::PreviousFrame => "previous_frame",
            Self::NextFrame => "next_frame",
            Self::NextSecond => "next_second",
            Self::SegmentEnd => "segment_end",
            Self::NextSegment => "next_segment",
            Self::GoToEnd => "go_to_end",
            Self::Undo => "undo",
            Self::Redo => "redo",
            Self::Cut => "cut",
            Self::Copy => "copy",
            Self::Paste => "paste",
            Self::SelectAll => "select_all",
            Self::RandomSegment => "random_segment",
            Self::SortSegments => "sort_segments",
            Self::ShuffleSegments => "shuffle_segments",
            Self::MergeOverlapping => "merge_overlapping",
            Self::InvertSegments => "invert_segments",
            Self::ClearSegments => "clear_segments",
            Self::ShowLastCommand => "last_ffmpeg_command",
            Self::AutoCutSilence => "auto_cut_silence",
            Self::SceneDetection => "scene_detection",
            Self::Fullscreen => "fullscreen",
            Self::ResetFontSize => "reset_font_size",
            Self::IncreaseFontSize => "increase_font_size",
            Self::DecreaseFontSize => "decrease_font_size",
            Self::About => "about",
        }
    }

    pub(super) fn label(self, strings: &Strings) -> String {
        match self {
            Self::PlayPause => format!("{} / {}", strings.play, strings.pause),
            Self::AddCut => strings.add_cut_or_start.clone(),
            Self::RemoveCut => strings.set_cut_end_or_finish.clone(),
            Self::DeleteSegment => strings.delete_segment.clone(),
            Self::Split => strings.split_segment.clone(),
            Self::ClearMarkers => strings.clear_markers.clone(),
            Self::CloseMedia => strings.close_media.clone(),
            Self::OpenSettings => strings.settings.clone(),
            Self::OpenMedia => strings.open.clone(),
            Self::ExportProject => strings.export_project.clone(),
            Self::ImportProject => strings.import_project.clone(),
            Self::RebindSource => strings.rebind_source.clone(),
            Self::Quit => strings.quit.clone(),
            Self::GoToStart => strings.go_to_start.clone(),
            Self::PreviousSegment => strings.select_previous_segment.clone(),
            Self::SegmentStart => strings.go_to_segment_start.clone(),
            Self::PreviousSecond => strings.previous_second.clone(),
            Self::PreviousFrame => strings.previous_frame.clone(),
            Self::NextFrame => strings.next_frame.clone(),
            Self::NextSecond => strings.next_second.clone(),
            Self::SegmentEnd => strings.go_to_segment_end.clone(),
            Self::NextSegment => strings.select_next_segment.clone(),
            Self::GoToEnd => strings.go_to_end.clone(),
            Self::Undo => strings.undo.clone(),
            Self::Redo => strings.redo.clone(),
            Self::Cut => strings.cut.clone(),
            Self::Copy => strings.copy.clone(),
            Self::Paste => strings.paste.clone(),
            Self::SelectAll => strings.select_all.clone(),
            Self::RandomSegment => strings.random_segment.clone(),
            Self::SortSegments => strings.sort_segments.clone(),
            Self::ShuffleSegments => strings.shuffle_segments.clone(),
            Self::MergeOverlapping => strings.merge_overlapping.clone(),
            Self::InvertSegments => strings.invert_segments.clone(),
            Self::ClearSegments => strings.clear_segments.clone(),
            Self::ShowLastCommand => strings.executed_ffmpeg_commands.clone(),
            Self::AutoCutSilence => strings.auto_cut_silence.clone(),
            Self::SceneDetection => strings.scene_detection.clone(),
            Self::Fullscreen => strings.fullscreen.clone(),
            Self::ResetFontSize => strings.reset_font_size.clone(),
            Self::IncreaseFontSize => strings.increase_font_size.clone(),
            Self::DecreaseFontSize => strings.decrease_font_size.clone(),
            Self::About => strings.about.clone(),
        }
    }

    pub(super) const fn default_binding(self) -> &'static str {
        match self {
            Self::PlayPause => "Space",
            Self::AddCut => "Alt+I",
            Self::RemoveCut => "Alt+O",
            Self::DeleteSegment => "Delete",
            Self::Split => "Alt+B",
            Self::ClearMarkers => "Escape",
            Self::CloseMedia => "Ctrl+W",
            Self::OpenSettings => "Ctrl+,",
            Self::PreviousSegment => "ArrowUp",
            Self::NextSegment => "ArrowDown",
            Self::PreviousSecond => "Alt+Left",
            Self::NextSecond => "Alt+Right",
            Self::PreviousFrame => ",",
            Self::NextFrame => ".",
            Self::AutoCutSilence => "Alt+S",
            Self::Undo => "Ctrl+Z",
            Self::Redo => "Ctrl+Shift+Z",
            Self::Cut => "Ctrl+X",
            Self::Copy => "Ctrl+C",
            Self::Paste => "Ctrl+V",
            Self::SelectAll => "Ctrl+A",
            Self::OpenMedia
            | Self::ExportProject
            | Self::ImportProject
            | Self::RebindSource
            | Self::Quit
            | Self::GoToStart
            | Self::SegmentStart
            | Self::SegmentEnd
            | Self::GoToEnd
            | Self::RandomSegment
            | Self::SortSegments
            | Self::ShuffleSegments
            | Self::MergeOverlapping
            | Self::InvertSegments
            | Self::ClearSegments
            | Self::ShowLastCommand
            | Self::SceneDetection
            | Self::Fullscreen
            | Self::ResetFontSize
            | Self::IncreaseFontSize
            | Self::DecreaseFontSize
            | Self::About => "",
        }
    }
}

/// One parsed keyboard shortcut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Shortcut {
    pub(super) key: egui::Key,
    pub(super) command: bool,
    pub(super) shift: bool,
    pub(super) alt: bool,
}

impl Shortcut {
    pub(super) fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        let mut command = false;
        let mut shift = false;
        let mut alt = false;
        let mut key = None;
        for part in text.split('+') {
            let part = part.trim();
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "cmd" | "command" | "super" => command = true,
                "shift" => shift = true,
                "alt" | "option" => alt = true,
                _ => {
                    key = Some(key_from_name(part)?);
                }
            }
        }
        Some(Self {
            key: key?,
            command,
            shift,
            alt,
        })
    }

    pub(super) fn matches(self, input: &egui::InputState) -> bool {
        // `command` is Ctrl on Linux/Windows and Cmd on macOS; accept a raw Ctrl
        // too so the documented `Ctrl+...` bindings work everywhere.
        let command = input.modifiers.command || input.modifiers.ctrl;
        input.key_pressed(self.key)
            && command == self.command
            && input.modifiers.shift == self.shift
            && input.modifiers.alt == self.alt
    }

    pub(super) fn label(self) -> String {
        let mut parts: Vec<&str> = Vec::with_capacity(4);
        if self.command {
            parts.push("Ctrl");
        }
        if self.shift {
            parts.push("Shift");
        }
        if self.alt {
            parts.push("Alt");
        }
        parts.push(key_label(self.key));
        parts.join("+")
    }
}

pub(super) fn is_modifier_key(key: egui::Key) -> bool {
    matches!(
        key,
        egui::Key::ShiftLeft
            | egui::Key::ShiftRight
            | egui::Key::ControlLeft
            | egui::Key::ControlRight
            | egui::Key::AltLeft
            | egui::Key::AltRight
            | egui::Key::SuperLeft
            | egui::Key::SuperRight
    )
}

/// Keys that should not become a shortcut while recording.
///
/// Besides modifiers, lock keys only change keyboard state and would otherwise
/// replace a binding by accident.
pub(super) fn is_recording_ignored_key(key: egui::Key) -> bool {
    is_modifier_key(key)
        || matches!(
            key,
            egui::Key::CapsLock | egui::Key::NumLock | egui::Key::ScrollLock
        )
}

pub(super) fn key_label(key: egui::Key) -> &'static str {
    match key {
        // `Key::name` calls this key "Comma"; the punctuation form is what
        // users type and what matches the documented `Ctrl+,` binding.
        egui::Key::Comma => ",",
        other => other.name(),
    }
}

fn key_from_name(name: &str) -> Option<egui::Key> {
    match name {
        "," => Some(egui::Key::Comma),
        "." => Some(egui::Key::Period),
        other => egui::Key::from_name(other),
    }
}

pub(super) fn default_shortcuts() -> BTreeMap<String, String> {
    ShortcutAction::ALL
        .iter()
        .map(|action| (action.key().to_owned(), action.default_binding().to_owned()))
        .collect()
}

/// Settings-page layout: each row holds two related actions side by side.
///
/// Related bindings stay next to each other (cut start / end, previous / next
/// keyframe) so the two-column list reads as pairs instead of a long sequence.
pub(super) const SETTINGS_ROWS: &[[Option<ShortcutAction>; 2]] = &[
    [
        Some(ShortcutAction::PlayPause),
        Some(ShortcutAction::ClearMarkers),
    ],
    [
        Some(ShortcutAction::AddCut),
        Some(ShortcutAction::RemoveCut),
    ],
    [
        Some(ShortcutAction::SegmentStart),
        Some(ShortcutAction::SegmentEnd),
    ],
    [
        Some(ShortcutAction::PreviousSecond),
        Some(ShortcutAction::NextSecond),
    ],
    [
        Some(ShortcutAction::PreviousFrame),
        Some(ShortcutAction::NextFrame),
    ],
    [
        Some(ShortcutAction::PreviousSegment),
        Some(ShortcutAction::NextSegment),
    ],
    [
        Some(ShortcutAction::GoToStart),
        Some(ShortcutAction::GoToEnd),
    ],
    [
        Some(ShortcutAction::DeleteSegment),
        Some(ShortcutAction::Split),
    ],
    [Some(ShortcutAction::Undo), Some(ShortcutAction::Redo)],
    [Some(ShortcutAction::Cut), Some(ShortcutAction::Copy)],
    [Some(ShortcutAction::Paste), Some(ShortcutAction::SelectAll)],
    [
        Some(ShortcutAction::AutoCutSilence),
        Some(ShortcutAction::SceneDetection),
    ],
    [
        Some(ShortcutAction::RandomSegment),
        Some(ShortcutAction::ShowLastCommand),
    ],
    [
        Some(ShortcutAction::SortSegments),
        Some(ShortcutAction::ShuffleSegments),
    ],
    [
        Some(ShortcutAction::MergeOverlapping),
        Some(ShortcutAction::InvertSegments),
    ],
    [Some(ShortcutAction::ClearSegments), None],
    [
        Some(ShortcutAction::OpenSettings),
        Some(ShortcutAction::About),
    ],
    [
        Some(ShortcutAction::OpenMedia),
        Some(ShortcutAction::CloseMedia),
    ],
    [
        Some(ShortcutAction::ExportProject),
        Some(ShortcutAction::ImportProject),
    ],
    [
        Some(ShortcutAction::RebindSource),
        Some(ShortcutAction::Quit),
    ],
    [
        Some(ShortcutAction::Fullscreen),
        Some(ShortcutAction::ResetFontSize),
    ],
    [
        Some(ShortcutAction::IncreaseFontSize),
        Some(ShortcutAction::DecreaseFontSize),
    ],
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_layout_covers_every_action_once() {
        let layout: Vec<ShortcutAction> =
            SETTINGS_ROWS.iter().flatten().flatten().copied().collect();

        assert_eq!(layout.len(), ShortcutAction::ALL.len());
        for action in ShortcutAction::ALL {
            assert_eq!(
                layout.iter().filter(|entry| **entry == action).count(),
                1,
                "{action:?} must appear exactly once in the settings layout"
            );
        }
    }

    #[test]
    fn silence_cut_has_a_default_binding() {
        assert_eq!(ShortcutAction::AutoCutSilence.default_binding(), "Alt+S");
        assert_eq!(default_shortcuts()["auto_cut_silence"], "Alt+S");
    }

    #[test]
    fn parses_punctuation_and_frame_defaults() {
        assert_eq!(
            Shortcut::parse(",").map(|shortcut| shortcut.key),
            Some(egui::Key::Comma)
        );
        assert_eq!(
            Shortcut::parse(".").map(|shortcut| shortcut.key),
            Some(egui::Key::Period)
        );
        assert_eq!(ShortcutAction::PreviousFrame.default_binding(), ",");
        assert_eq!(ShortcutAction::NextFrame.default_binding(), ".");
        assert_eq!(ShortcutAction::PreviousSecond.default_binding(), "Alt+Left");
    }
}
