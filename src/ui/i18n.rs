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
    pub no_media_loaded: String,
    pub open_media_prompt: String,
    pub probing_file: String,
    pub reading_media: String,
    pub loading_preview: String,
    pub no_video_frame: String,
    pub could_not_open: String,
    pub duration_unavailable: String,
    pub playback: String,
    pub media: String,
    pub state: String,
    pub position: String,
    pub sync: String,
    pub decoder: String,
    pub preview: String,
    pub frame: String,
    pub video: String,
    pub audio: String,
    pub state_no_media: String,
    pub state_probing: String,
    pub state_failed: String,
    pub state_playing: String,
    pub state_paused: String,
    pub state_scrubbing: String,
    pub sync_scrub: String,
    pub sync_audio_clock: String,
    pub sync_wall_clock: String,
    pub preview_summary: String,
    pub format: String,
    pub container: String,
    pub long_name: String,
    pub duration: String,
    pub start_time: String,
    pub size: String,
    pub bit_rate: String,
    pub streams: String,
    pub chapters: String,
    pub id: String,
    pub end: String,
    pub title: String,
    pub codec: String,
    pub codec_name: String,
    pub profile: String,
    pub language: String,
    pub time_base: String,
    pub disposition: String,
    pub resolution: String,
    pub pixel_format: String,
    pub frame_rate: String,
    pub average_frame_rate: String,
    pub color_range: String,
    pub color_space: String,
    pub color_transfer: String,
    pub color_primaries: String,
    pub sample_rate: String,
    pub channels: String,
    pub channel_layout: String,
    pub sample_format: String,
    pub stream_video: String,
    pub stream_audio: String,
    pub stream_subtitle: String,
    pub stream_other: String,
    pub flag_default: String,
    pub flag_forced: String,
    pub flag_hearing_impaired: String,
    pub flag_visual_impaired: String,
    pub flag_attached_pic: String,
    pub appearance: String,
    pub theme: String,
    pub theme_dark: String,
    pub theme_light: String,
    pub follow_system_theme: String,
    pub about_description: String,
    pub about_version: String,
    pub decoder_software: String,
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
