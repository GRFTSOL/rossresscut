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
    pub about: String,
    pub about_description: String,
    pub about_version: String,
    pub add_cut_point: String,
    pub add_cut_or_start: String,
    pub apply: String,
    pub audio: String,
    pub auto_cut_silence: String,
    pub autosave_projects: String,
    pub autosave_projects_hint: String,
    pub batch_cancel: String,
    pub batch_clear: String,
    pub batch_add_files: String,
    pub batch_completed: String,
    pub batch_empty: String,

    pub batch_failed: String,
    pub batch_hint: String,
    pub batch_merge_failed: String,
    pub batch_merged: String,
    pub batch_no_segments: String,
    pub batch_pending: String,
    pub batch_queue: String,
    pub batch_running: String,
    pub batch_skipped: String,
    pub batch_skipped_cancelled: String,
    pub batch_skipped_no_duration: String,
    pub batch_skipped_no_segments: String,
    pub batch_start: String,
    pub cancel: String,
    pub cancel_export: String,
    pub chapter_hint: String,
    pub chapters: String,
    pub chapters_empty: String,
    pub choose_directory: String,
    pub choose_project_directory: String,
    pub clear_markers: String,
    pub clear_segments: String,
    pub close_media: String,
    pub command_history_hint: String,
    pub copy: String,
    pub copy_command: String,
    pub create: String,
    pub could_not_open: String,
    pub cut: String,
    pub decoder: String,
    pub decoder_software: String,
    pub decrease_font_size: String,
    pub default_font: String,
    pub delete_segment: String,
    pub discard: String,
    pub drop_batch: String,
    pub drop_files: String,
    pub drop_merge: String,
    pub duration_unavailable: String,
    pub enabled: String,
    pub executed_ffmpeg_commands: String,
    pub exit_fullscreen: String,
    pub export: String,
    pub export_cancelled: String,
    pub export_chapters: String,
    pub export_chapters_hint: String,
    pub export_completed: String,
    pub export_concatenating: String,
    pub export_cutting: String,
    pub export_directory: String,
    pub export_failed: String,
    pub export_invalid_output: String,
    pub export_join_hint: String,
    pub export_no_segment: String,
    pub export_output_path: String,
    pub export_project: String,
    pub export_segment: String,
    pub export_title: String,
    pub export_title_hint: String,
    pub export_to_source_dir: String,
    pub exporting: String,
    pub ffmpeg_purpose_export: String,
    pub ffmpeg_purpose_keyframes: String,
    pub ffmpeg_purpose_merge: String,
    pub ffmpeg_purpose_other: String,
    pub ffmpeg_purpose_probe: String,
    pub ffmpeg_purpose_scene: String,
    pub ffmpeg_purpose_subtitle: String,
    pub ffmpeg_purpose_waveform: String,
    pub file_dialog_failed: String,
    pub file_dialog_stopped: String,
    pub follow_system_theme: String,
    pub font_family: String,
    pub font_search: String,
    pub font_size: String,
    pub frame: String,
    pub fullscreen: String,
    pub general: String,
    pub go_to_end: String,
    pub go_to_segment_end: String,
    pub go_to_segment_start: String,
    pub go_to_start: String,
    pub import_project: String,
    pub increase_font_size: String,
    pub invert_segments: String,
    pub language: String,
    pub loading_preview: String,
    pub menu_edit: String,
    pub menu_file: String,
    pub menu_help: String,
    pub menu_scene: String,
    pub menu_segment: String,
    pub menu_tools: String,
    pub menu_view: String,
    pub merge_overlapping: String,
    pub move_down: String,
    pub move_up: String,
    pub next_frame: String,
    pub next_second: String,
    pub no_ffmpeg_command: String,
    pub no_media: String,
    pub no_segments: String,
    pub no_video_frame: String,
    pub open: String,
    pub open_disabled: String,
    pub open_in_progress: String,
    pub open_media_prompt: String,
    pub opening: String,
    pub paste: String,
    pub pause: String,
    pub play: String,
    pub playback: String,

    pub previous_frame: String,
    pub previous_second: String,
    pub preview: String,
    pub preview_backend: String,
    pub preview_backend_hint: String,
    pub preview_seek_step: String,
    pub preview_seek_step_hint: String,
    pub probing_file: String,
    pub project_directory: String,
    pub project_load_failed: String,
    pub project_missing_source: String,
    pub project_save_failed: String,
    pub project_to_source_dir: String,
    pub quit: String,
    pub random_segment: String,
    pub random_segments_count: String,
    pub rebind_source: String,
    pub redo: String,
    pub remove_cut_point: String,
    pub set_cut_end_or_finish: String,
    pub reset_font_size: String,
    pub save: String,
    pub scene_cuts: String,
    pub scene_detection: String,
    pub scene_no_cuts: String,
    pub scene_rescan: String,
    pub scene_scanning: String,
    pub scene_start_hint: String,
    pub scene_threshold: String,
    pub segment_end: String,
    pub segment_start: String,
    pub segments: String,
    pub select_all: String,
    pub select_next_segment: String,
    pub select_previous_segment: String,
    pub set_end_from_playhead: String,
    pub set_start_from_playhead: String,
    pub settings: String,
    pub shortcut_hint: String,
    pub shortcut_press_keys: String,
    pub shortcut_unassigned: String,
    pub shortcuts: String,
    pub shuffle_segments: String,
    pub show_batch_panel: String,
    pub show_playback_info: String,
    pub silence_detection: String,
    pub silence_min_duration: String,
    pub silence_no_audible: String,
    pub silence_no_waveform: String,
    pub silence_preview: String,
    pub silence_replaces: String,
    pub silence_threshold: String,
    pub smart_cut: String,
    pub smart_cut_hint: String,
    pub smart_cut_bit_rate: String,
    pub smart_cut_bit_rate_hint: String,
    pub sort_segments: String,
    pub split_segment: String,
    pub start: String,
    pub streams: String,
    pub subtitle_keep_partial: String,
    pub subtitle_keep_partial_hint: String,
    pub subtitle_stream: String,
    pub theme: String,
    pub theme_dark: String,
    pub theme_light: String,
    pub total_duration: String,
    pub undo: String,
    pub unsaved_changes: String,
    pub unsaved_changes_hint: String,
    pub volume: String,
    pub video: String,
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
