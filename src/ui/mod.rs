//! egui/eframe desktop layer.
//!
//! The UI renders state and forwards user intent to the application layer. It
//! never runs FFprobe, parses FFprobe output, or mutates domain state directly.
//!
//! The layout follows the LosslessCut direction recorded in `GOALS.md`: a top
//! menu bar, a central preview, a bottom timeline, and a side panel with
//! playback and media information.
//!
//! Submodules own one area each: `shortcuts` and `settings` hold persisted
//! state, `timeline`, `preview`, `panels`, `menu` and `windows` paint it, and
//! `actions` maps input back to application commands.

mod actions;
mod dialogs;
mod fonts;
mod i18n;
mod menu;
mod panels;
mod preview;
mod settings;
mod shortcuts;
mod timeline;
mod windows;

use std::path::PathBuf;
#[cfg(feature = "mpv-preview")]
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use eframe::egui;

use crate::application::{Application, OpenState};
use crate::domain::media::MediaStream;
use crate::domain::segment::SegmentId;
use crate::domain::time::Timestamp;
use crate::media::preview::HardwareAcceleration;

use dialogs::DialogError;
use fonts::{FontFamilyChoice, scan_fonts};
use i18n::Strings;
use preview::RenderDiagnostics;
use settings::{ThemeChoice, UiSettings, detect_system_theme, load_settings, save_settings};
use shortcuts::{Shortcut, ShortcutAction};

const FILE_DIALOG_SUPPORTED: bool = cfg!(target_os = "linux");
const WHEEL_STEP_POINTS: f32 = 50.0;
const WHEEL_LINE_POINTS: f32 = 50.0;
const WHEEL_PAGE_LINES: f32 = 10.0;
const TIMELINE_HEIGHT: f32 = 28.0;
const MIN_FONT_ZOOM: f32 = 0.8;
const MAX_FONT_ZOOM: f32 = 1.5;
const FONT_ZOOM_STEP: f32 = 0.05;
const SEGMENT_COLORS: [egui::Color32; 6] = [
    egui::Color32::from_rgb(86, 132, 176),
    egui::Color32::from_rgb(104, 166, 118),
    egui::Color32::from_rgb(190, 145, 82),
    egui::Color32::from_rgb(148, 112, 176),
    egui::Color32::from_rgb(82, 166, 166),
    egui::Color32::from_rgb(188, 104, 104),
];

/// Main application window.
#[derive(Default)]
pub struct RossresscutApp {
    application: Application,
    preview_texture: Option<egui::TextureHandle>,
    preview_texture_timestamp: Option<i64>,
    #[cfg(feature = "mpv-preview")]
    render_state: Option<eframe::egui_wgpu::RenderState>,
    #[cfg(feature = "mpv-preview")]
    preview_mpv_texture: Option<eframe::wgpu::Texture>,
    #[cfg(feature = "mpv-preview")]
    preview_mpv_id: Option<egui::TextureId>,
    #[cfg(feature = "mpv-preview")]
    preview_mpv_size: Option<egui::Vec2>,
    #[cfg(feature = "mpv-preview")]
    preview_bounds: Option<(u32, u32)>,
    #[cfg(feature = "mpv-preview")]
    pending_preview_bounds: Option<(u32, u32)>,
    preview_source: Option<PathBuf>,
    scrub_target: Option<Timestamp>,
    scrub_was_playing: bool,
    last_scrub_target: Option<i64>,
    last_scrub_start: Option<Instant>,
    file_dialog: Option<Receiver<Option<PathBuf>>>,
    dialog_error: Option<DialogError>,
    directory_dialog: Option<Receiver<Option<PathBuf>>>,
    export_error: Option<String>,
    export_panel_open: bool,
    ffmpeg_command_open: bool,
    settings: UiSettings,
    font_zoom_draft: f32,
    fonts: Vec<FontFamilyChoice>,
    font_search: String,
    applied_font: Option<(String, u32)>,
    detected_theme: Option<egui::Theme>,
    recording_shortcut: Option<ShortcutAction>,
    recording_pending: Option<(ShortcutAction, egui::Key, egui::Modifiers)>,
    settings_open: bool,
    about_open: bool,
    wheel_accumulator: f32,
    segment_error: Option<String>,
    segment_start_input: String,
    segment_end_input: String,
    segment_input_segment: Option<SegmentId>,
    diagnostics: Option<RenderDiagnostics>,
    diagnostic_autoplay: bool,
    diagnostic_autoplay_started: bool,
}

impl RossresscutApp {
    /// Creates the window state and optionally opens `initial_path`.
    pub fn new(context: &eframe::CreationContext<'_>, initial_path: Option<PathBuf>) -> Self {
        // The preview uses the command modifier with the wheel for seeking, so
        // egui must not also treat that gesture as a UI zoom.
        context
            .egui_ctx
            .options_mut(|options| options.input_options.zoom_modifier = egui::Modifiers::NONE);

        let settings = load_settings();
        let font_zoom_draft = settings.font_zoom;
        let font_scan_start = Instant::now();
        let fonts = scan_fonts();
        log::info!(
            "indexed {} font families in {:?}",
            fonts.len(),
            font_scan_start.elapsed()
        );
        let mut app = Self {
            settings,
            font_zoom_draft,
            fonts,
            detected_theme: detect_system_theme(),
            diagnostics: RenderDiagnostics::from_environment(),
            diagnostic_autoplay: std::env::var_os("ROSS_RESCUT_DIAG_AUTOPLAY").is_some(),
            ..Self::default()
        };
        #[cfg(feature = "mpv-preview")]
        match context.wgpu_render_state.clone() {
            Some(render_state) => {
                app.render_state = Some(render_state);
                app.application.enable_mpv_preview();
            }
            None => log::warn!(
                "no wgpu render state is available; falling back to the external FFmpeg preview"
            ),
        }
        // The diagnostic run overrides the decoder backend without a settings UI.
        if let Ok(value) = std::env::var("ROSS_RESCUT_PREVIEW_HW") {
            let hardware = match value.as_str() {
                "software" | "none" => HardwareAcceleration::None,
                "vaapi" => HardwareAcceleration::Vaapi,
                "d3d11va" => HardwareAcceleration::D3d11va,
                "videotoolbox" => HardwareAcceleration::VideoToolbox,
                other => {
                    log::warn!("ignoring unknown ROSS_RESCUT_PREVIEW_HW value {other:?}");
                    HardwareAcceleration::default_for_platform()
                }
            };
            app.application.set_preview_hardware(hardware);
        }
        if let Some(path) = initial_path {
            app.application.open_media(path);
        }
        app
    }

    fn strings(&self) -> &'static Strings {
        self.settings.language.strings()
    }

    fn reset_scrub(&mut self) {
        self.scrub_target = None;
        self.scrub_was_playing = false;
        self.last_scrub_target = None;
        self.last_scrub_start = None;
    }

    fn scrub_to(&mut self, target: Timestamp) {
        self.scrub_target = Some(target);
        let now = Instant::now();
        let should_scrub = !self.application.is_waiting_for_preview()
            && self.last_scrub_target != Some(target.micros())
            && self
                .last_scrub_start
                .is_none_or(|start| now.duration_since(start) >= Duration::from_millis(80));
        if should_scrub {
            self.application.scrub(target);
            self.last_scrub_target = Some(target.micros());
            self.last_scrub_start = Some(now);
        }
    }

    fn finish_scrub(&mut self, target: Timestamp) {
        self.application.seek(target);
        let resume = self.scrub_was_playing;
        self.reset_scrub();
        if resume {
            self.application.play();
        }
    }
}
impl eframe::App for RossresscutApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if let Some(diagnostics) = &mut self.diagnostics {
            diagnostics.frame();
        }
        self.apply_theme(ui.ctx());
        self.poll_file_dialog();
        self.poll_directory_dialog();
        self.sync_segment_inputs();
        self.handle_shortcuts(ui.ctx());

        if self.application.update() {
            ui.ctx().request_repaint();
        }

        // A diagnostic run has no input injection, so it seeks and starts
        // playback once the media is ready. Only enabled by an environment
        // variable; normal sessions never take this path.
        if self.diagnostic_autoplay
            && !self.diagnostic_autoplay_started
            && matches!(self.application.state(), OpenState::Ready)
        {
            self.application.seek(Timestamp::from_micros(60_000_000));
            self.application.play();
            self.diagnostic_autoplay_started = true;
        }

        let has_video = self.application.media().is_some_and(|media| {
            media
                .streams
                .iter()
                .any(|stream| matches!(stream, MediaStream::Video(_)))
        });
        if self.application.is_playing() {
            let repaint_millis = if has_video { 16 } else { 100 };
            ui.ctx()
                .request_repaint_after(Duration::from_millis(repaint_millis));
        } else if self.application.export_status().is_running() {
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        } else if self.application.is_waiting_for_preview() {
            ui.ctx().request_repaint_after(Duration::from_millis(50));
        } else if matches!(self.application.state(), OpenState::Probing { .. })
            || self.file_dialog.is_some()
        {
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }

        // niri and some other compositors never report `system_theme`, so the
        // fallback is re-detected when settings are opened or changed.

        self.sync_preview_texture(ui.ctx());
        self.menu_bar(ui);
        self.timeline_panel(ui);
        self.info_panel(ui);
        self.preview_panel(ui);
        self.settings_window(ui.ctx());
        self.export_panel(ui.ctx());
        self.ffmpeg_command_window(ui.ctx());
        self.about_window(ui.ctx());
    }

    fn save(&mut self, _storage: &mut dyn eframe::Storage) {
        save_settings(&self.settings);
    }
}

/// Starts the native event loop and blocks until the window closes.
///
/// Must be called from the main thread.
pub fn run(initial_path: Option<PathBuf>) -> Result<(), eframe::Error> {
    // `options.wgpu_options` is only assigned when mpv-preview is enabled.
    #[allow(unused_mut)]
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([960.0, 640.0])
            .with_min_inner_size([720.0, 480.0]),
        ..Default::default()
    };

    #[cfg(feature = "mpv-preview")]
    {
        let mut create_new = eframe::egui_wgpu::WgpuSetupCreateNew::without_display_handle();
        create_new.instance_descriptor.backends = mpv_engine::WGPU_BACKEND;
        create_new.device_descriptor = Arc::new(|_adapter| eframe::wgpu::DeviceDescriptor {
            label: Some("rossresscut preview device"),
            required_features: mpv_engine::REQUIRED_WGPU_FEATURES,
            required_limits: eframe::wgpu::Limits {
                max_texture_dimension_2d: 8192,
                ..Default::default()
            },
            ..Default::default()
        });
        options.wgpu_options = eframe::egui_wgpu::WgpuConfiguration {
            wgpu_setup: eframe::egui_wgpu::WgpuSetup::CreateNew(create_new),
            ..Default::default()
        };
    }

    eframe::run_native(
        "rossresscut",
        options,
        Box::new(move |creation_context| {
            Ok(Box::new(RossresscutApp::new(
                creation_context,
                initial_path,
            )))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::i18n::Language;
    use super::panels::file_title;
    use super::preview::fit_size;
    use super::settings::{gtk_theme_from_ini, ini_value, qt_theme_from_ini, theme_from_label};
    use super::shortcuts::default_shortcuts;
    use super::timeline::{fraction_of, timestamp_at_x, wheel_direction};
    use super::*;
    use crate::domain::time::Duration as MediaDuration;

    #[test]
    fn converts_timeline_fractions_to_microseconds() {
        let duration = MediaDuration::from_micros(10_000_000);
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 20.0));

        assert_eq!(
            timestamp_at_x(rect, 50.0, duration),
            Timestamp::from_micros(5_000_000)
        );
        assert_eq!(timestamp_at_x(rect, -10.0, duration), Timestamp::ZERO);
        assert_eq!(
            timestamp_at_x(rect, 200.0, duration),
            Timestamp::from_micros(10_000_000)
        );
        assert_eq!(
            fraction_of(Timestamp::from_micros(5_000_000), duration),
            0.5
        );
    }

    #[test]
    fn fits_preview_inside_available_space() {
        let small = fit_size(egui::vec2(160.0, 120.0), egui::vec2(640.0, 480.0));
        assert!((small.x - 640.0).abs() < 0.01);
        assert!((small.y - 480.0).abs() < 0.01);

        let large = fit_size(egui::vec2(1920.0, 1080.0), egui::vec2(640.0, 480.0));
        assert!((large.x - 640.0).abs() < 0.01);
        assert!((large.y - 360.0).abs() < 0.01);
    }

    #[test]
    fn applies_theme_to_context() {
        let context = egui::Context::default();

        context.set_theme(egui::ThemePreference::Light);
        assert_eq!(context.theme(), egui::Theme::Light);

        context.set_theme(egui::ThemePreference::Dark);
        assert_eq!(context.theme(), egui::Theme::Dark);
    }

    #[test]
    fn persists_ui_settings() {
        let mut settings = UiSettings {
            follow_system_theme: false,
            theme: ThemeChoice::Light,
            language: Language::English,
            export_chapters: true,
            export_to_source_dir: true,
            export_directory: String::new(),
            font_zoom: 1.0,
            font_path: String::new(),
            font_index: 0,
            shortcuts: default_shortcuts(),
        };

        for language in Language::ALL {
            settings.language = language;
            let text = toml::to_string(&settings).expect("serialize settings");
            let parsed: UiSettings = toml::from_str(&text).expect("parse settings");

            assert_eq!(parsed, settings);
        }
    }

    #[test]
    fn parses_and_renders_shortcuts() {
        let cases = [
            ("Space", egui::Key::Space, false, false, false),
            ("Ctrl+W", egui::Key::W, true, false, false),
            ("Ctrl+,", egui::Key::Comma, true, false, false),
            ("Ctrl+Shift+I", egui::Key::I, true, true, false),
            ("Alt+Left", egui::Key::ArrowLeft, false, false, true),
        ];
        for (text, key, command, shift, alt) in cases {
            let shortcut = Shortcut::parse(text).expect("parse shortcut");
            assert_eq!(shortcut.key, key);
            assert_eq!(shortcut.command, command);
            assert_eq!(shortcut.shift, shift);
            assert_eq!(shortcut.alt, alt);
            assert_eq!(shortcut.label(), text);
        }
        assert!(Shortcut::parse("").is_none());
        assert!(Shortcut::parse("Ctrl+NotAKey").is_none());
    }

    #[test]
    fn default_shortcuts_cover_every_action() {
        let shortcuts = default_shortcuts();
        for action in ShortcutAction::ALL {
            assert!(
                shortcuts.contains_key(action.key()),
                "missing default for {}",
                action.key()
            );
        }
        let settings = UiSettings::default();
        assert_eq!(settings.binding(ShortcutAction::PlayPause), "Space");
        assert_eq!(
            settings.shortcut(ShortcutAction::OpenSettings),
            Shortcut::parse("Ctrl+,")
        );
    }

    fn press_shortcut(
        context: &egui::Context,
        app: &mut RossresscutApp,
        key: egui::Key,
        modifiers: egui::Modifiers,
    ) {
        let mut input = egui::RawInput::default();
        input.events.push(egui::Event::ModifiersChanged(modifiers));
        input.events.push(egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        });
        context
            .run_pass(input, |context| {
                app.handle_shortcuts(context);
            })
            .drop_without_applying_deltas();
    }

    #[test]
    fn open_settings_shortcut_uses_the_configured_binding() {
        let mut app = RossresscutApp::default();
        let context = egui::Context::default();

        press_shortcut(&context, &mut app, egui::Key::Comma, egui::Modifiers::CTRL);
        assert!(app.settings_open, "default Ctrl+, must open settings");

        app.settings_open = false;
        app.settings
            .shortcuts
            .insert("open_settings".to_owned(), "F1".to_owned());
        press_shortcut(&context, &mut app, egui::Key::Comma, egui::Modifiers::CTRL);
        assert!(!app.settings_open, "the old binding must stop working");

        press_shortcut(&context, &mut app, egui::Key::F1, egui::Modifiers::NONE);
        assert!(app.settings_open, "the new binding must work immediately");

        app.settings_open = false;
        app.settings
            .shortcuts
            .insert("open_settings".to_owned(), String::new());
        press_shortcut(&context, &mut app, egui::Key::F1, egui::Modifiers::NONE);
        assert!(!app.settings_open, "an empty binding must do nothing");
    }

    #[test]
    fn follow_system_falls_back_to_the_manual_theme() {
        let mut app = RossresscutApp {
            settings: UiSettings {
                follow_system_theme: true,
                theme: ThemeChoice::Light,
                ..UiSettings::default()
            },
            ..Default::default()
        };
        let context = egui::Context::default();

        app.apply_theme(&context);

        assert_eq!(context.theme(), egui::Theme::Light);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn reads_gtk_and_qt_theme_preferences() {
        let gtk = "[Settings]\ngtk-theme-name=adw-gtk3\ngtk-icon-theme-name=Papirus-Light\ngtk-application-prefer-dark-theme=0\n";
        assert_eq!(gtk_theme_from_ini(gtk), Some(egui::Theme::Light));

        let dark = "[Settings]\ngtk-theme-name=Adwaita-dark\n";
        assert_eq!(gtk_theme_from_ini(dark), Some(egui::Theme::Dark));

        let qt = "[Appearance]\ncolor_scheme_path=/usr/share/color-schemes/BreezeDark.colors\n";
        assert_eq!(qt_theme_from_ini(qt), Some(egui::Theme::Dark));
        assert_eq!(theme_from_label("Adwaita"), None);
        assert_eq!(ini_value(gtk, "gtk-icon-theme-name"), Some("Papirus-Light"));
    }

    #[test]
    fn defaults_to_system_theme() {
        let settings = UiSettings::default();

        assert!(settings.follow_system_theme);
        assert_eq!(settings.theme, ThemeChoice::Dark);
        assert_eq!(settings.language, Language::English);
    }

    #[test]
    fn wheel_direction_maps_scroll_to_playback_direction() {
        assert_eq!(wheel_direction(1.0), 1);
        assert_eq!(wheel_direction(-1.0), -1);
        assert_eq!(wheel_direction(0.0), -1);
    }

    #[test]
    fn strips_extensions_from_titles() {
        assert_eq!(
            file_title(std::path::Path::new("/tmp/sample.mkv")),
            "sample"
        );
        assert_eq!(file_title(std::path::Path::new("/tmp/archive")), "archive");
    }
    fn run_shortcut_events(
        context: &egui::Context,
        app: &mut RossresscutApp,
        events: Vec<egui::Event>,
    ) {
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        context
            .run_pass(input, |context| {
                app.handle_shortcuts(context);
            })
            .drop_without_applying_deltas();
    }

    #[test]
    fn shortcut_recording_waits_for_release_and_ignores_modifiers() {
        let mut app = RossresscutApp {
            recording_shortcut: Some(ShortcutAction::AddCut),
            ..Default::default()
        };
        let context = egui::Context::default();

        run_shortcut_events(
            &context,
            &mut app,
            vec![
                egui::Event::ModifiersChanged(egui::Modifiers::ALT),
                egui::Event::Key {
                    key: egui::Key::AltLeft,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::ALT,
                },
            ],
        );
        assert_eq!(app.settings.binding(ShortcutAction::AddCut), "I");

        run_shortcut_events(
            &context,
            &mut app,
            vec![egui::Event::Key {
                key: egui::Key::I,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::ALT,
            }],
        );
        assert_eq!(app.settings.binding(ShortcutAction::AddCut), "I");

        run_shortcut_events(
            &context,
            &mut app,
            vec![egui::Event::Key {
                key: egui::Key::I,
                physical_key: None,
                pressed: false,
                repeat: false,
                modifiers: egui::Modifiers::ALT,
            }],
        );
        assert_eq!(app.settings.binding(ShortcutAction::AddCut), "Alt+I");
        assert!(app.recording_shortcut.is_none());
    }
}
