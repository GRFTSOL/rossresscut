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
    Split,
    ClearMarkers,
    CloseMedia,
    OpenSettings,
    GoToStart,
    PreviousSegment,
    SegmentStart,
    PreviousKeyframe,
    PreviousFrame,
    NextFrame,
    NextKeyframe,
    SegmentEnd,
    NextSegment,
    GoToEnd,
    SetSegmentStart,
    SetSegmentEnd,
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
}

impl ShortcutAction {
    pub(super) const ALL: [Self; 32] = [
        Self::PlayPause,
        Self::AddCut,
        Self::RemoveCut,
        Self::Split,
        Self::ClearMarkers,
        Self::CloseMedia,
        Self::OpenSettings,
        Self::GoToStart,
        Self::PreviousSegment,
        Self::SegmentStart,
        Self::PreviousKeyframe,
        Self::PreviousFrame,
        Self::NextFrame,
        Self::NextKeyframe,
        Self::SegmentEnd,
        Self::NextSegment,
        Self::GoToEnd,
        Self::SetSegmentStart,
        Self::SetSegmentEnd,
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
    ];

    pub(super) const fn key(self) -> &'static str {
        match self {
            Self::PlayPause => "play_pause",
            Self::AddCut => "add_cut",
            Self::RemoveCut => "remove_cut",
            Self::Split => "split",
            Self::ClearMarkers => "clear_markers",
            Self::CloseMedia => "close_media",
            Self::OpenSettings => "open_settings",
            Self::GoToStart => "go_to_start",
            Self::PreviousSegment => "previous_segment",
            Self::SegmentStart => "segment_start",
            Self::PreviousKeyframe => "previous_keyframe",
            Self::PreviousFrame => "previous_frame",
            Self::NextFrame => "next_frame",
            Self::NextKeyframe => "next_keyframe",
            Self::SegmentEnd => "segment_end",
            Self::NextSegment => "next_segment",
            Self::GoToEnd => "go_to_end",
            Self::SetSegmentStart => "segment_set_start",
            Self::SetSegmentEnd => "segment_set_end",
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
        }
    }

    pub(super) fn label(self, strings: &Strings) -> String {
        match self {
            Self::PlayPause => format!("{} / {}", strings.play, strings.pause),
            Self::AddCut => strings.add_cut_or_start.clone(),
            Self::RemoveCut => strings.remove_cut_or_end.clone(),
            Self::Split => strings.split_segment.clone(),
            Self::ClearMarkers => strings.clear_markers.clone(),
            Self::CloseMedia => strings.close_media.clone(),
            Self::OpenSettings => strings.settings.clone(),
            Self::GoToStart => strings.go_to_start.clone(),
            Self::PreviousSegment => strings.select_previous_segment.clone(),
            Self::SegmentStart => strings.go_to_segment_start.clone(),
            Self::PreviousKeyframe => strings.previous_keyframe.clone(),
            Self::PreviousFrame => strings.previous_frame.clone(),
            Self::NextFrame => strings.next_frame.clone(),
            Self::NextKeyframe => strings.next_keyframe.clone(),
            Self::SegmentEnd => strings.go_to_segment_end.clone(),
            Self::NextSegment => strings.select_next_segment.clone(),
            Self::GoToEnd => strings.go_to_end.clone(),
            Self::SetSegmentStart => strings.set_start_from_playhead.clone(),
            Self::SetSegmentEnd => strings.set_end_from_playhead.clone(),
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
            Self::ShowLastCommand => strings.last_ffmpeg_command.clone(),
        }
    }

    pub(super) const fn default_binding(self) -> &'static str {
        match self {
            Self::PlayPause => "Space",
            Self::AddCut => "Alt+I",
            Self::RemoveCut => "Alt+O",
            Self::Split => "Alt+B",
            Self::ClearMarkers => "Escape",
            Self::CloseMedia => "Ctrl+W",
            Self::OpenSettings => "Ctrl+,",
            Self::GoToStart
            | Self::PreviousSegment
            | Self::SegmentStart
            | Self::PreviousKeyframe
            | Self::PreviousFrame
            | Self::NextFrame
            | Self::NextKeyframe
            | Self::SegmentEnd
            | Self::NextSegment
            | Self::GoToEnd => "",
            Self::SetSegmentStart | Self::SetSegmentEnd => "",
            Self::Undo => "Ctrl+Z",
            Self::Redo => "Ctrl+Shift+Z",
            Self::Cut => "Ctrl+X",
            Self::Copy => "Ctrl+C",
            Self::Paste => "Ctrl+V",
            Self::SelectAll => "Ctrl+A",
            Self::RandomSegment
            | Self::SortSegments
            | Self::ShuffleSegments
            | Self::MergeOverlapping
            | Self::InvertSegments
            | Self::ClearSegments
            | Self::ShowLastCommand => "",
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
                    key = Some(egui::Key::from_name(part)?);
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

pub(super) fn key_label(key: egui::Key) -> &'static str {
    match key {
        // `Key::name` calls this key "Comma"; the punctuation form is what
        // users type and what matches the documented `Ctrl+,` binding.
        egui::Key::Comma => ",",
        other => other.name(),
    }
}

pub(super) fn default_shortcuts() -> BTreeMap<String, String> {
    ShortcutAction::ALL
        .iter()
        .map(|action| (action.key().to_owned(), action.default_binding().to_owned()))
        .collect()
}
