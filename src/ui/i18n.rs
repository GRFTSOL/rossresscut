//! Localized UI strings.
//!
//! Strings live in `locales/*.json` and are embedded at compile time. Lookups
//! select a typed structure, so a missing or misspelled key fails the locale
//! unit tests instead of producing a runtime fallback.
//!
//! Only the UI layer is localized. Application warnings and media-layer errors
//! remain English until they are modeled as structured error kinds.

use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

/// UI language.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    /// English.
    #[default]
    #[serde(rename = "en")]
    English,
    /// Simplified Chinese.
    #[serde(rename = "zh_hans")]
    SimplifiedChinese,
}

impl Language {
    /// Languages offered by the settings page.
    pub const ALL: [Self; 2] = [Self::English, Self::SimplifiedChinese];

    /// Returns the language name written in that language.
    pub const fn label(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::SimplifiedChinese => "简体中文",
        }
    }

    /// Returns the UI strings for this language.
    pub fn strings(self) -> &'static Strings {
        match self {
            Self::English => &ENGLISH,
            Self::SimplifiedChinese => &CHINESE,
        }
    }
}

static ENGLISH: LazyLock<Strings> = LazyLock::new(|| load(include_str!("../../locales/en.json")));

static CHINESE: LazyLock<Strings> =
    LazyLock::new(|| load(include_str!("../../locales/zh-CN.json")));

fn load(json: &'static str) -> Strings {
    // INVARIANT: locale files are embedded at compile time and parsed by
    // `locales_parse` below, so a malformed file is a programming error.
    serde_json::from_str(json).expect("embedded locale file is valid")
}

/// Replaces each `{}` in `template` with the next value.
pub fn fill(template: &str, values: &[&str]) -> String {
    let mut result = template.to_owned();
    for value in values {
        result = result.replacen("{}", value, 1);
    }
    result
}

/// UI strings for one language.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Strings {
    pub menu_file: String,
    pub menu_view: String,
    pub menu_help: String,
    pub open: String,
    pub opening: String,
    pub close_media: String,
    pub quit: String,
    pub settings: String,
    pub about: String,
    pub open_disabled: String,
    pub open_in_progress: String,
    pub file_dialog_failed: String,
    pub file_dialog_stopped: String,
    pub play: String,
    pub pause: String,
    pub no_media: String,
    pub open_media_prompt: String,
    pub probing_file: String,
    pub loading_preview: String,
    pub no_video_frame: String,
    pub could_not_open: String,
    pub duration_unavailable: String,
    pub playback: String,
    pub decoder: String,
    pub frame: String,
    pub video: String,
    pub audio: String,
    pub language: String,
    pub theme: String,
    pub theme_dark: String,
    pub theme_light: String,
    pub follow_system_theme: String,
    pub about_description: String,
    pub about_version: String,
    pub about_commit: String,
    pub about_phase: String,
    pub decoder_software: String,
    pub segments: String,
    pub move_up: String,
    pub move_down: String,
    pub no_segments: String,
    pub set_start_from_playhead: String,
    pub set_end_from_playhead: String,
    pub enabled: String,
    pub export_segment: String,
    pub exporting: String,
    pub cancel_export: String,
    pub export_completed: String,
    pub export_failed: String,
    pub export_cancelled: String,
    pub export_no_segment: String,
    pub export_invalid_output: String,
    pub export_cutting: String,
    pub export_concatenating: String,
    pub export_chapters: String,
    pub export_chapters_hint: String,
    pub go_to_start: String,
    pub select_previous_segment: String,
    pub go_to_segment_start: String,
    pub previous_keyframe: String,
    pub previous_frame: String,
    pub next_frame: String,
    pub next_keyframe: String,
    pub go_to_segment_end: String,
    pub select_next_segment: String,
    pub go_to_end: String,
    pub segment_start: String,
    pub segment_end: String,
    pub add_cut_point: String,
    pub remove_cut_point: String,
    pub split_segment: String,
    pub export: String,
    pub general: String,
    pub font_size: String,
    pub shortcuts: String,
    pub font_family: String,
    pub font_search: String,
    pub default_font: String,
    pub export_to_source_dir: String,
    pub choose_directory: String,
    pub export_directory: String,
    pub export_output_path: String,
    pub cancel: String,
    pub menu_edit: String,
    pub menu_segment: String,
    pub menu_tools: String,
    pub undo: String,
    pub redo: String,
    pub cut: String,
    pub copy: String,
    pub paste: String,
    pub select_all: String,
    pub random_segment: String,
    pub sort_segments: String,
    pub shuffle_segments: String,
    pub merge_overlapping: String,
    pub invert_segments: String,
    pub clear_segments: String,
    pub last_ffmpeg_command: String,
    pub no_ffmpeg_command: String,
    pub copy_command: String,
    pub clear_markers: String,
    pub shortcut_press_keys: String,
    pub shortcut_unassigned: String,
    pub shortcut_clear: String,
    pub reset_font_size: String,
    pub increase_font_size: String,
    pub decrease_font_size: String,
    pub fullscreen: String,
    pub exit_fullscreen: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locales_parse() {
        for language in Language::ALL {
            let strings = language.strings();
            assert!(!strings.menu_file.is_empty());
            assert!(!strings.settings.is_empty());
            assert!(!strings.about_description.is_empty());
        }
    }

    #[test]
    fn language_labels_are_distinct() {
        assert_ne!(
            Language::English.label(),
            Language::SimplifiedChinese.label()
        );
    }

    #[test]
    fn fills_templates_in_order() {
        assert_eq!(fill("{} / {}", &["a", "b"]), "a / b");
        assert_eq!(fill("Streams ({})", &["3"]), "Streams (3)");
        assert_eq!(fill("no placeholder", &["x"]), "no placeholder");
    }
}
