//! Persistent UI settings, theme detection and theme application.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use eframe::egui;
use serde::{Deserialize, Serialize};

use crate::media::preview::PreviewBackend;

use super::fonts::{font_definitions, install_icon_font};
use super::i18n::{Language, Strings};
use super::shortcuts::default_shortcuts;
use super::{MAX_FONT_ZOOM, MIN_FONT_ZOOM, RossresscutApp, Shortcut, ShortcutAction};

impl RossresscutApp {
    pub(super) fn apply_theme(&mut self, context: &egui::Context) {
        let system_theme = context.system_theme().or(self.detected_theme);
        let preference = if self.settings.follow_system_theme {
            // Some Linux compositors do not report a system theme. Fall back
            // to GTK / Qt settings, then to the saved manual theme instead of
            // egui's dark default, so "follow system" does not force dark.
            match system_theme {
                Some(egui::Theme::Dark) => egui::ThemePreference::Dark,
                Some(egui::Theme::Light) => egui::ThemePreference::Light,
                None => match self.settings.theme {
                    ThemeChoice::Dark => egui::ThemePreference::Dark,
                    ThemeChoice::Light => egui::ThemePreference::Light,
                },
            }
        } else {
            match self.settings.theme {
                ThemeChoice::Dark => egui::ThemePreference::Dark,
                ThemeChoice::Light => egui::ThemePreference::Light,
            }
        };
        context.set_theme(preference);
        // Keep menus, dialogs and the main window at the same control size.
        // The default interact height is too small once the window is scaled up.
        context.all_styles_mut(|style| {
            style.spacing.button_padding = egui::vec2(10.0, 6.0);
            style.spacing.interact_size.y = 28.0;
        });
        // `set_zoom_factor` scales text and widgets relative to the system DPI.
        context.set_zoom_factor(self.settings.font_zoom.clamp(MIN_FONT_ZOOM, MAX_FONT_ZOOM));
        if self.applied_font.as_ref()
            != Some(&(self.settings.font_path.clone(), self.settings.font_index))
        {
            let definitions = if self.settings.font_path.is_empty() {
                Some(egui::FontDefinitions::default())
            } else {
                font_definitions(
                    Path::new(&self.settings.font_path),
                    self.settings.font_index,
                )
            };
            match definitions {
                Some(mut definitions) => {
                    install_icon_font(&mut definitions);
                    context.set_fonts(definitions);
                    self.applied_font =
                        Some((self.settings.font_path.clone(), self.settings.font_index));
                }
                None => log::warn!(
                    "could not load UI font {}; keeping the current font",
                    self.settings.font_path
                ),
            }
        }
    }

    pub(super) fn open_settings(&mut self) {
        self.settings_open = true;
        // niri does not report `system_theme`; refresh the GTK / Qt fallback
        // whenever the user opens settings.
        self.detected_theme = detect_system_theme();
    }

    pub(super) fn set_font_zoom(&mut self, zoom: f32) {
        self.settings.font_zoom = zoom.clamp(MIN_FONT_ZOOM, MAX_FONT_ZOOM);
        self.font_zoom_draft = self.settings.font_zoom;
    }
}

/// Color theme selected when the system theme is not followed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum ThemeChoice {
    #[default]
    Dark,
    Light,
}

/// Detects the desktop theme when the compositor does not report one.
pub(super) fn detect_system_theme() -> Option<egui::Theme> {
    #[cfg(target_os = "linux")]
    {
        gtk_theme().or_else(qt_theme)
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

#[cfg(target_os = "linux")]
pub(super) fn gtk_theme() -> Option<egui::Theme> {
    if let Some(theme) = std::env::var_os("GTK_THEME")
        && let Some(theme) = theme_from_label(&theme.to_string_lossy())
    {
        return Some(theme);
    }
    let config = config_directory()?;
    for file in ["gtk-3.0/settings.ini", "gtk-4.0/settings.ini"] {
        let Ok(text) = fs::read_to_string(config.join(file)) else {
            continue;
        };
        if let Some(theme) = gtk_theme_from_ini(&text) {
            return Some(theme);
        }
    }
    None
}

#[cfg(target_os = "linux")]
pub(super) fn gtk_theme_from_ini(text: &str) -> Option<egui::Theme> {
    if let Some(name) = ini_value(text, "gtk-theme-name")
        && let Some(theme) = theme_from_label(name)
    {
        return Some(theme);
    }
    match ini_value(text, "gtk-application-prefer-dark-theme") {
        Some("1" | "true" | "True") => return Some(egui::Theme::Dark),
        Some("0" | "false" | "False") => return Some(egui::Theme::Light),
        _ => {}
    }
    if let Some(name) = ini_value(text, "gtk-icon-theme-name")
        && let Some(theme) = theme_from_label(name)
    {
        return Some(theme);
    }
    None
}

#[cfg(target_os = "linux")]
pub(super) fn qt_theme() -> Option<egui::Theme> {
    if let Some(style) = std::env::var_os("QT_STYLE_OVERRIDE")
        && let Some(theme) = theme_from_label(&style.to_string_lossy())
    {
        return Some(theme);
    }
    let config = config_directory()?;
    for file in ["qt6ct/qt6ct.conf", "qt5ct/qt5ct.conf"] {
        let Ok(text) = fs::read_to_string(config.join(file)) else {
            continue;
        };
        if let Some(theme) = qt_theme_from_ini(&text) {
            return Some(theme);
        }
    }
    None
}

#[cfg(target_os = "linux")]
pub(super) fn qt_theme_from_ini(text: &str) -> Option<egui::Theme> {
    for key in ["color_scheme_path", "icon_theme", "style"] {
        if let Some(value) = ini_value(text, key)
            && let Some(theme) = theme_from_label(value)
        {
            return Some(theme);
        }
    }
    None
}

#[cfg(target_os = "linux")]
pub(super) fn theme_from_label(label: &str) -> Option<egui::Theme> {
    let label = label.to_ascii_lowercase();
    if label.contains("dark") {
        Some(egui::Theme::Dark)
    } else if label.contains("light") {
        Some(egui::Theme::Light)
    } else {
        None
    }
}

#[cfg(target_os = "linux")]
pub(super) fn ini_value<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        let Some((name, value)) = line.split_once('=') else {
            continue;
        };
        if name.trim() != key {
            continue;
        }
        let value = value.trim().trim_matches('"');
        if !value.is_empty() {
            return Some(value);
        }
    }
    None
}

pub(super) fn config_directory() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    if let Some(appdata) = std::env::var_os("APPDATA") {
        return Some(PathBuf::from(appdata));
    }

    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
}

/// Default wheel seek step: 200 frames at 30 fps.
///
/// The wheel step is a user setting; this value is only used when
/// `config.toml` has no `[preview] seek_step_micros` yet.
pub(super) const DEFAULT_PREVIEW_SEEK_MICROS: i64 = 6_666_667;
/// Accepted wheel step range; a hand-edited config outside it is clamped.
const MIN_PREVIEW_SEEK_MICROS: i64 = 100_000;
const MAX_PREVIEW_SEEK_MICROS: i64 = 600_000_000;

/// UI settings used by the application.
///
/// The on-disk layout splits these into `[general]` and `[export]` so
/// application preferences and export preferences can evolve independently.
/// The in-memory type stays flat so call sites do not need to know the split.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct UiSettings {
    pub(super) follow_system_theme: bool,
    pub(super) theme: ThemeChoice,
    pub(super) language: Language,
    pub(super) font_zoom: f32,
    pub(super) font_path: String,
    pub(super) font_index: u32,
    pub(super) shortcuts: BTreeMap<String, String>,
    pub(super) volume: u8,
    pub(super) preview_seek_micros: i64,
    pub(super) preview_backend: PreviewBackend,
    pub(super) autosave_projects: bool,
    pub(super) subtitle_keep_partial: bool,
    pub(super) project_to_source_dir: bool,
    pub(super) project_directory: String,
    pub(super) export_chapters: bool,
    pub(super) export_to_source_dir: bool,
    pub(super) export_directory: String,
    pub(super) export_title: String,
    pub(super) smart_cut: bool,
    pub(super) smart_cut_bit_rate_kbps: u32,
}

/// `config.toml` layout.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct SettingsFile {
    general: GeneralSettings,
    preview: PreviewSettings,
    export: ExportSettings,
}

/// Application-level preferences (`[general]`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct GeneralSettings {
    follow_system_theme: bool,
    theme: ThemeChoice,
    language: Language,
    font_zoom: f32,
    font_path: String,
    font_index: u32,
    autosave_projects: bool,
    volume: u8,
    subtitle_keep_partial: bool,
    project_to_source_dir: bool,
    project_directory: String,
    shortcuts: BTreeMap<String, String>,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        let defaults = UiSettings::default();
        Self {
            follow_system_theme: defaults.follow_system_theme,
            theme: defaults.theme,
            language: defaults.language,
            font_zoom: defaults.font_zoom,
            font_path: defaults.font_path,
            font_index: defaults.font_index,
            autosave_projects: defaults.autosave_projects,
            volume: defaults.volume,
            subtitle_keep_partial: defaults.subtitle_keep_partial,
            project_to_source_dir: defaults.project_to_source_dir,
            project_directory: defaults.project_directory,
            shortcuts: defaults.shortcuts,
        }
    }
}

/// Preview preferences (`[preview]`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct PreviewSettings {
    seek_step_micros: i64,
    backend: String,
}

impl Default for PreviewSettings {
    fn default() -> Self {
        Self {
            seek_step_micros: UiSettings::default().preview_seek_micros,
            backend: PreviewBackend::default().key().to_owned(),
        }
    }
}

/// Export preferences (`[export]`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct ExportSettings {
    chapters: bool,
    to_source_dir: bool,
    directory: String,
    title: String,
    smart_cut: bool,
    smart_cut_bit_rate: u32,
}

impl Default for ExportSettings {
    fn default() -> Self {
        let defaults = UiSettings::default();
        Self {
            chapters: defaults.export_chapters,
            to_source_dir: defaults.export_to_source_dir,
            directory: defaults.export_directory,
            title: defaults.export_title,
            smart_cut: defaults.smart_cut,
            smart_cut_bit_rate: defaults.smart_cut_bit_rate_kbps,
        }
    }
}

impl From<SettingsFile> for UiSettings {
    fn from(file: SettingsFile) -> Self {
        let general = file.general;
        let preview = file.preview;
        let export = file.export;
        let mut shortcuts = general.shortcuts;
        migrate_legacy_shortcut_defaults(&mut shortcuts);
        fill_missing_shortcuts(&mut shortcuts);
        Self {
            follow_system_theme: general.follow_system_theme,
            theme: general.theme,
            language: general.language,
            font_zoom: general.font_zoom,
            font_path: general.font_path,
            font_index: general.font_index,
            shortcuts,
            volume: general.volume,
            preview_seek_micros: preview
                .seek_step_micros
                .clamp(MIN_PREVIEW_SEEK_MICROS, MAX_PREVIEW_SEEK_MICROS),
            preview_backend: PreviewBackend::from_key(&preview.backend).unwrap_or_default(),
            autosave_projects: general.autosave_projects,
            subtitle_keep_partial: general.subtitle_keep_partial,
            project_to_source_dir: general.project_to_source_dir,
            project_directory: general.project_directory,
            export_chapters: export.chapters,
            export_to_source_dir: export.to_source_dir,
            export_directory: export.directory,
            export_title: export.title,
            smart_cut: export.smart_cut,
            smart_cut_bit_rate_kbps: export.smart_cut_bit_rate,
        }
    }
}

impl From<&UiSettings> for SettingsFile {
    fn from(settings: &UiSettings) -> Self {
        Self {
            general: GeneralSettings {
                follow_system_theme: settings.follow_system_theme,
                theme: settings.theme,
                language: settings.language,
                font_zoom: settings.font_zoom,
                font_path: settings.font_path.clone(),
                font_index: settings.font_index,
                autosave_projects: settings.autosave_projects,
                subtitle_keep_partial: settings.subtitle_keep_partial,
                project_to_source_dir: settings.project_to_source_dir,
                project_directory: settings.project_directory.clone(),
                shortcuts: settings.shortcuts.clone(),
                volume: settings.volume,
            },
            preview: PreviewSettings {
                seek_step_micros: settings.preview_seek_micros,
                backend: settings.preview_backend.key().to_owned(),
            },
            export: ExportSettings {
                chapters: settings.export_chapters,
                to_source_dir: settings.export_to_source_dir,
                directory: settings.export_directory.clone(),
                title: settings.export_title.clone(),
                smart_cut: settings.smart_cut,
                smart_cut_bit_rate: settings.smart_cut_bit_rate_kbps,
            },
        }
    }
}

/// Flat layout used by `config.toml` before the `[general]` / `[export]` split.
#[derive(Deserialize)]
#[serde(default)]
struct LegacySettings {
    follow_system_theme: bool,
    theme: ThemeChoice,
    language: Language,
    export_chapters: bool,
    export_to_source_dir: bool,
    export_directory: String,
    font_zoom: f32,
    font_path: String,
    font_index: u32,
    shortcuts: BTreeMap<String, String>,
}

impl Default for LegacySettings {
    fn default() -> Self {
        Self {
            follow_system_theme: true,
            theme: ThemeChoice::Dark,
            language: Language::English,
            export_chapters: true,
            export_to_source_dir: true,
            export_directory: String::new(),
            font_zoom: 1.0,
            font_path: String::new(),
            font_index: 0,
            shortcuts: default_shortcuts(),
        }
    }
}

impl From<LegacySettings> for UiSettings {
    fn from(legacy: LegacySettings) -> Self {
        let mut shortcuts = legacy.shortcuts;
        migrate_legacy_shortcut_defaults(&mut shortcuts);
        fill_missing_shortcuts(&mut shortcuts);
        Self {
            follow_system_theme: legacy.follow_system_theme,
            theme: legacy.theme,
            language: legacy.language,
            font_zoom: legacy.font_zoom,
            font_path: legacy.font_path,
            font_index: legacy.font_index,
            shortcuts,
            volume: 100,
            preview_seek_micros: DEFAULT_PREVIEW_SEEK_MICROS,
            preview_backend: PreviewBackend::default(),
            autosave_projects: true,
            subtitle_keep_partial: false,
            project_to_source_dir: true,
            project_directory: String::new(),
            export_chapters: legacy.export_chapters,
            export_to_source_dir: legacy.export_to_source_dir,
            export_directory: legacy.export_directory,
            export_title: String::new(),
            smart_cut: false,
            smart_cut_bit_rate_kbps: 0,
        }
    }
}

/// Rewrites the old default cut bindings to the contextual Alt bindings.
///
/// Only applies when all three bindings still hold their old defaults, so a
/// user who mapped anything by hand keeps their choices.
fn migrate_legacy_shortcut_defaults(shortcuts: &mut BTreeMap<String, String>) {
    let legacy = shortcuts
        .get(ShortcutAction::AddCut.key())
        .map(String::as_str)
        == Some("I")
        && shortcuts
            .get(ShortcutAction::RemoveCut.key())
            .map(String::as_str)
            == Some("O")
        && shortcuts
            .get(ShortcutAction::Split.key())
            .map(String::as_str)
            == Some("Enter");
    if legacy {
        shortcuts.insert(ShortcutAction::AddCut.key().to_owned(), "Alt+I".to_owned());
        shortcuts.insert(
            ShortcutAction::RemoveCut.key().to_owned(),
            "Alt+O".to_owned(),
        );
        shortcuts.insert(ShortcutAction::Split.key().to_owned(), "Alt+B".to_owned());
    }

    // The keyframe step became a one-second step; keep the user's binding.
    for (old, new) in [
        ("previous_keyframe", "previous_second"),
        ("next_keyframe", "next_second"),
    ] {
        if let Some(value) = shortcuts.remove(old) {
            shortcuts.entry(new.to_owned()).or_insert(value);
        }
    }

    // These actions were unbound in older builds; fill only empty values so a
    // binding the user set stays untouched.
    for (key, binding) in [
        ("previous_segment", "ArrowUp"),
        ("next_segment", "ArrowDown"),
        ("previous_second", "Alt+Left"),
        ("next_second", "Alt+Right"),
        ("previous_frame", ","),
        ("next_frame", "."),
    ] {
        if shortcuts.get(key).is_some_and(String::is_empty) {
            shortcuts.insert(key.to_owned(), binding.to_owned());
        }
    }

    // The delete action now removes the selected segment instead of a marker.
    if let Some(value) = shortcuts.remove("delete_cut_point") {
        shortcuts
            .entry(ShortcutAction::DeleteSegment.key().to_owned())
            .or_insert(value);
    }
    shortcuts.remove("segment_set_start");
    shortcuts.remove("segment_set_end");
}

/// Adds defaults for actions that did not exist when the file was written.
///
/// Explicitly cleared bindings are empty strings, not missing keys, so they are
/// preserved.
fn fill_missing_shortcuts(shortcuts: &mut BTreeMap<String, String>) {
    for (key, value) in default_shortcuts() {
        shortcuts.entry(key).or_insert(value);
    }
}

impl UiSettings {
    pub(super) fn binding(&self, action: ShortcutAction) -> &str {
        self.shortcuts.get(action.key()).map_or("", String::as_str)
    }

    pub(super) fn shortcut(&self, action: ShortcutAction) -> Option<Shortcut> {
        Shortcut::parse(self.binding(action))
    }
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            follow_system_theme: true,
            theme: ThemeChoice::Dark,
            language: Language::English,
            font_zoom: 1.0,
            font_path: String::new(),
            font_index: 0,
            shortcuts: default_shortcuts(),
            volume: 100,
            preview_seek_micros: DEFAULT_PREVIEW_SEEK_MICROS,
            preview_backend: PreviewBackend::default(),
            autosave_projects: true,
            subtitle_keep_partial: false,
            project_to_source_dir: true,
            project_directory: String::new(),
            export_chapters: true,
            export_to_source_dir: true,
            export_directory: String::new(),
            export_title: String::new(),
            smart_cut: false,
            smart_cut_bit_rate_kbps: 0,
        }
    }
}

pub(super) fn theme_label(strings: &Strings, theme: ThemeChoice) -> String {
    match theme {
        ThemeChoice::Dark => strings.theme_dark.clone(),
        ThemeChoice::Light => strings.theme_light.clone(),
    }
}

pub(super) fn settings_path() -> Option<PathBuf> {
    Some(config_directory()?.join("rossresscut").join("config.toml"))
}

pub(super) fn load_settings() -> UiSettings {
    let Some(path) = settings_path() else {
        return UiSettings::default();
    };
    match fs::read_to_string(&path) {
        Ok(text) => parse_settings(&text).unwrap_or_else(|| {
            log::warn!("could not parse {}; using defaults", path.display());
            UiSettings::default()
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => UiSettings::default(),
        Err(error) => {
            log::warn!("could not read {}: {error}", path.display());
            UiSettings::default()
        }
    }
}

/// Parses `config.toml`, accepting the flat layout written by older builds.
pub(super) fn parse_settings(text: &str) -> Option<UiSettings> {
    let value: toml::Value = toml::from_str(text).ok()?;
    let table = value.as_table()?;
    if table.contains_key("general")
        || table.contains_key("preview")
        || table.contains_key("export")
    {
        return toml::from_str::<SettingsFile>(text)
            .ok()
            .map(UiSettings::from);
    }
    toml::from_str::<LegacySettings>(text)
        .ok()
        .map(UiSettings::from)
}

pub(super) fn save_settings(settings: &UiSettings) {
    let Some(path) = settings_path() else {
        log::warn!("no config directory; settings will not persist");
        return;
    };
    if let Some(parent) = path.parent()
        && let Err(error) = fs::create_dir_all(parent)
    {
        log::warn!("could not create {}: {error}", parent.display());
        return;
    }
    let file = SettingsFile::from(settings);
    match toml::to_string_pretty(&file) {
        Ok(text) => {
            if let Err(error) = fs::write(&path, text) {
                log::warn!("could not write {}: {error}", path.display());
            }
        }
        Err(error) => log::warn!("could not serialize settings: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn legacy_shortcuts() -> BTreeMap<String, String> {
        let mut shortcuts = default_shortcuts();
        shortcuts.insert("add_cut".to_owned(), "I".to_owned());
        shortcuts.insert("remove_cut".to_owned(), "O".to_owned());
        shortcuts.insert("split".to_owned(), "Enter".to_owned());
        shortcuts
    }

    #[test]
    fn migrates_the_old_cut_shortcuts_to_alt() {
        let mut shortcuts = legacy_shortcuts();
        migrate_legacy_shortcut_defaults(&mut shortcuts);

        assert_eq!(shortcuts["add_cut"], "Alt+I");
        assert_eq!(shortcuts["remove_cut"], "Alt+O");
        assert_eq!(shortcuts["split"], "Alt+B");
    }

    #[test]
    fn keeps_custom_shortcuts_when_only_one_matches_the_old_default() {
        let mut shortcuts = legacy_shortcuts();
        shortcuts.insert("split".to_owned(), "Ctrl+B".to_owned());
        migrate_legacy_shortcut_defaults(&mut shortcuts);

        assert_eq!(shortcuts["add_cut"], "I");
        assert_eq!(shortcuts["remove_cut"], "O");
        assert_eq!(shortcuts["split"], "Ctrl+B");
    }

    #[test]
    fn migrates_keyframe_steps_to_one_second_steps() {
        let mut shortcuts = legacy_shortcuts();
        shortcuts.remove("previous_second");
        shortcuts.remove("next_second");
        shortcuts.insert("previous_keyframe".to_owned(), "Alt+Left".to_owned());
        shortcuts.insert("next_keyframe".to_owned(), "Alt+Right".to_owned());
        shortcuts.insert("previous_frame".to_owned(), String::new());
        shortcuts.insert("next_frame".to_owned(), String::new());
        migrate_legacy_shortcut_defaults(&mut shortcuts);

        assert_eq!(shortcuts["previous_second"], "Alt+Left");
        assert_eq!(shortcuts["next_second"], "Alt+Right");
        assert_eq!(shortcuts["previous_frame"], ",");
        assert_eq!(shortcuts["next_frame"], ".");
        assert!(!shortcuts.contains_key("previous_keyframe"));
        assert!(!shortcuts.contains_key("next_keyframe"));
    }

    #[test]
    fn round_trips_the_smart_cut_bit_rate() {
        let settings = UiSettings {
            smart_cut_bit_rate_kbps: 2_500,
            ..Default::default()
        };
        let text = toml::to_string_pretty(&SettingsFile::from(&settings)).expect("serialize");
        let parsed = parse_settings(&text).expect("parse");

        assert_eq!(parsed.smart_cut_bit_rate_kbps, 2_500);
        assert_eq!(
            parse_settings("[general]\n[export]\n")
                .expect("parse")
                .smart_cut_bit_rate_kbps,
            0
        );
    }

    #[test]
    fn round_trips_the_preview_backend() {
        let settings = UiSettings {
            preview_backend: PreviewBackend::MpvNvdec,
            ..Default::default()
        };
        let text = toml::to_string_pretty(&SettingsFile::from(&settings)).expect("serialize");
        assert!(text.contains("backend = \"mpv-nvdec\""), "{text}");
        assert_eq!(
            parse_settings(&text).expect("parse").preview_backend,
            PreviewBackend::MpvNvdec
        );

        // Unknown or old values fall back to the default backend.
        assert_eq!(
            parse_settings("[general]\n[preview]\nbackend = \"bogus\"\n")
                .expect("parse")
                .preview_backend,
            PreviewBackend::MpvAutoSafe
        );
    }

    #[test]
    fn round_trips_the_preview_seek_step() {
        let settings = UiSettings {
            preview_seek_micros: 2_500_000,
            ..Default::default()
        };
        let text = toml::to_string_pretty(&SettingsFile::from(&settings)).expect("serialize");
        assert!(text.contains("[preview]"));
        assert_eq!(
            parse_settings(&text).expect("parse").preview_seek_micros,
            2_500_000
        );

        // Config files written before the key existed fall back to the default.
        assert_eq!(
            parse_settings("[general]\n[export]\n")
                .expect("parse")
                .preview_seek_micros,
            DEFAULT_PREVIEW_SEEK_MICROS
        );
    }

    #[test]
    fn clamps_a_hand_edited_preview_seek_step() {
        let too_small = parse_settings("[general]\n[preview]\nseek_step_micros = 0\n[export]\n")
            .expect("parse");
        assert_eq!(too_small.preview_seek_micros, MIN_PREVIEW_SEEK_MICROS);

        let too_large =
            parse_settings("[general]\n[preview]\nseek_step_micros = 999999999999\n[export]\n")
                .expect("parse");
        assert_eq!(too_large.preview_seek_micros, MAX_PREVIEW_SEEK_MICROS);
    }

    #[test]
    fn round_trips_the_playback_volume() {
        let settings = UiSettings {
            volume: 45,
            ..Default::default()
        };
        let text = toml::to_string_pretty(&SettingsFile::from(&settings)).expect("serialize");
        assert_eq!(parse_settings(&text).expect("parse").volume, 45);

        // Config files written before the key existed fall back to 100%.
        assert_eq!(
            parse_settings("[general]\n[export]\n")
                .expect("parse")
                .volume,
            100
        );
    }
}
