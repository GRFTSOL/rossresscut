//! Persistent UI settings, theme detection and theme application.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use eframe::egui;
use serde::{Deserialize, Serialize};

use super::fonts::font_definitions;
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
        // `set_zoom_factor` scales text and widgets relative to the system DPI.
        context.set_zoom_factor(self.settings.font_zoom.clamp(MIN_FONT_ZOOM, MAX_FONT_ZOOM));
        if self.applied_font.as_ref()
            != Some(&(self.settings.font_path.clone(), self.settings.font_index))
        {
            if self.settings.font_path.is_empty() {
                context.set_fonts(egui::FontDefinitions::default());
            } else if let Some(definitions) = font_definitions(
                Path::new(&self.settings.font_path),
                self.settings.font_index,
            ) {
                context.set_fonts(definitions);
            } else {
                log::warn!(
                    "could not load UI font {}; keeping the current font",
                    self.settings.font_path
                );
            }
            self.applied_font = Some((self.settings.font_path.clone(), self.settings.font_index));
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
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
}

/// UI settings persisted to `config.toml`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct UiSettings {
    pub(super) follow_system_theme: bool,
    pub(super) theme: ThemeChoice,
    pub(super) language: Language,
    #[serde(default = "default_true")]
    pub(super) export_chapters: bool,
    #[serde(default = "default_true")]
    pub(super) export_to_source_dir: bool,
    #[serde(default)]
    pub(super) export_directory: String,
    #[serde(default = "default_font_zoom")]
    pub(super) font_zoom: f32,
    #[serde(default)]
    pub(super) font_path: String,
    #[serde(default)]
    pub(super) font_index: u32,
    #[serde(default = "default_shortcuts")]
    pub(super) shortcuts: BTreeMap<String, String>,
}

pub(super) fn default_true() -> bool {
    true
}

pub(super) fn default_font_zoom() -> f32 {
    1.0
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
        Ok(text) => match toml::from_str(&text) {
            Ok(settings) => settings,
            Err(error) => {
                log::warn!("could not parse {}: {error}", path.display());
                UiSettings::default()
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => UiSettings::default(),
        Err(error) => {
            log::warn!("could not read {}: {error}", path.display());
            UiSettings::default()
        }
    }
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
    match toml::to_string_pretty(settings) {
        Ok(text) => {
            if let Err(error) = fs::write(&path, text) {
                log::warn!("could not write {}: {error}", path.display());
            }
        }
        Err(error) => log::warn!("could not serialize settings: {error}"),
    }
}
