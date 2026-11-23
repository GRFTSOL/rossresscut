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
mod diagnostics;
mod dialogs;
mod fonts;
mod i18n;
mod icons;
mod menu;
mod panels;
mod preview;
mod settings;
mod shortcuts;
mod timeline;
mod titlebar;
mod windows;

use std::path::PathBuf;
#[cfg(feature = "mpv-preview")]
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use eframe::egui;

use crate::application::{Application, OpenState, SceneScan};
use crate::domain::media::MediaStream;
use crate::domain::segment::SegmentId;
use crate::domain::time::Timestamp;
use crate::media::preview::HardwareAcceleration;

use dialogs::{CloseIntent, DialogError, DirectoryTarget, FileDialogPurpose, ProjectDialogMode};
use fonts::{FontFamilyChoice, scan_fonts};
use i18n::Strings;
use panels::InfoTab;
#[cfg(feature = "mpv-preview")]
use preview::PreviewBounds;
use preview::RenderDiagnostics;
use settings::{ThemeChoice, UiSettings, detect_system_theme, load_settings, save_settings};
use shortcuts::{Shortcut, ShortcutAction};
use timeline::WaveformCache;
use windows::SettingsTab;

const FILE_DIALOG_SUPPORTED: bool = cfg!(any(target_os = "linux", target_os = "windows"));
const WHEEL_STEP_POINTS: f32 = 50.0;
const WHEEL_LINE_POINTS: f32 = 50.0;
const WHEEL_PAGE_LINES: f32 = 10.0;
/// Volume change per wheel step or slider step, in percent.
const VOLUME_STEP: i32 = 5;

/// Steps `volume` by whole [`VOLUME_STEP`] increments, clamped to `0..=100`.
pub(super) fn stepped_volume(volume: u8, steps: i32) -> u8 {
    (i32::from(volume) + steps * VOLUME_STEP).clamp(0, 100) as u8
}
/// Timeline height without an audio stream.
const TIMELINE_HEIGHT: f32 = 42.0;
/// Timeline height with a waveform behind the segments.
const TIMELINE_HEIGHT_AUDIO: f32 = 56.0;
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
    preview_bounds: PreviewBounds,
    preview_source: Option<PathBuf>,
    scrub_target: Option<Timestamp>,
    scrub_was_playing: bool,
    last_scrub_target: Option<i64>,
    last_scrub_start: Option<Instant>,
    file_dialog: Option<(FileDialogPurpose, Receiver<Option<Vec<PathBuf>>>)>,
    dialog_error: Option<DialogError>,
    directory_dialog: Option<(DirectoryTarget, Receiver<Option<PathBuf>>)>,
    project_dialog: Option<(ProjectDialogMode, Receiver<Option<PathBuf>>)>,
    project_error: Option<String>,
    project_file: Option<PathBuf>,
    saved_revision: u64,
    awaiting_project_baseline: bool,
    pending_close: Option<CloseIntent>,
    allow_close: bool,
    last_autosave_revision: u64,
    last_autosave_at: Option<Instant>,
    export_error: Option<String>,
    export_panel_open: bool,
    ffmpeg_command_open: bool,
    random_dialog_open: bool,
    random_count: usize,
    settings: UiSettings,
    font_zoom_draft: f32,
    fonts: Vec<FontFamilyChoice>,
    font_search: String,
    applied_font: Option<(String, u32)>,
    detected_theme: Option<egui::Theme>,
    recording_shortcut: Option<ShortcutAction>,
    recording_pending: Option<(ShortcutAction, egui::Key, egui::Modifiers)>,
    settings_open: bool,
    settings_tab: SettingsTab,
    playback_info_open: bool,
    info_tab: InfoTab,
    about_open: bool,
    silence_dialog_open: bool,
    silence_options: crate::application::SilenceOptions,
    silence_preview: Option<Result<usize, crate::application::SilenceError>>,
    scene_dialog_open: bool,
    scene_threshold: f32,
    batch_panel_open: bool,
    batch_file_dialog: Option<Receiver<Option<Vec<PathBuf>>>>,
    pending_drop: Option<Vec<PathBuf>>,
    /// `None` exports every stream; `Some` holds the chosen stream indices.
    stream_selection: Option<std::collections::BTreeSet<usize>>,
    wheel_accumulator: f32,
    volume_popup_open: bool,
    volume_popup_wheel: f32,
    /// Last pointer position egui reported while the pointer was present.
    ///
    /// Windows reports a cursor leave when Alt is pressed for a shortcut such as
    /// `Alt+I`, after which egui has no pointer position until the mouse moves
    /// again. Keeping the last known position lets wheel gestures keep working
    /// across that gap.
    last_pointer_pos: Option<egui::Pos2>,
    waveform_cache: Option<WaveformCache>,
    segment_error: Option<String>,
    segment_start_input: String,
    segment_end_input: String,
    segment_input_segment: Option<SegmentId>,
    diagnostics: Option<RenderDiagnostics>,
    diagnostic_autoplay: bool,
    diagnostic_autoplay_started: bool,
    diagnostic_scrub: bool,
    diagnostic_scrub_step: u32,
    diagnostic_scrub_start: Option<Instant>,
}

impl RossresscutApp {
    /// Creates the window state and optionally opens `initial_path`.
    pub fn new(context: &eframe::CreationContext<'_>, initial_paths: Vec<PathBuf>) -> Self {
        // The timeline uses the command modifier with the wheel to seek, so
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
            diagnostic_scrub: std::env::var_os("ROSS_RESCUT_DIAG_SCRUB").is_some(),
            // A quarter of a frame-to-frame change is the FFmpeg default for
            // scene detection and works well as a starting point.
            scene_threshold: 0.4,
            random_count: 5,
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
        let volume = app.settings.volume;
        app.application.set_volume(volume);
        app.application
            .set_preview_backend(app.settings.preview_backend);
        match initial_paths.as_slice() {
            [] => {}
            [path] => app.application.open_media(path.clone()),
            files => app.pending_drop = Some(files.to_vec()),
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

    /// Opens a single dropped file directly and asks about several at once.
    fn handle_dropped_files(&mut self, context: &egui::Context) {
        let dropped: Vec<PathBuf> = context.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .map(|file| file.path().to_path_buf())
                .collect()
        });
        if dropped.is_empty() {
            return;
        }

        if dropped.len() == 1 {
            self.reset_scrub();
            self.segment_error = None;
            self.stream_selection = None;
            self.detach_project();
            self.application.open_media(dropped[0].clone());
        } else {
            self.pending_drop = Some(dropped);
        }
    }
}
impl eframe::App for RossresscutApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if let Some(position) = ui.ctx().input(|input| input.pointer.hover_pos()) {
            self.last_pointer_pos = Some(position);
        }
        if let Some(diagnostics) = &mut self.diagnostics {
            diagnostics.frame();
        }
        self.apply_theme(ui.ctx());
        self.window_resize_edges(ui.ctx());
        self.sync_project_baseline();
        if ui.ctx().input(|input| input.viewport().close_requested())
            && !self.allow_close
            && self.pending_close.is_none()
            && self.has_unsaved_project()
        {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.pending_close = Some(CloseIntent::Application);
            self.project_error = None;
        }
        self.poll_file_dialog();
        self.poll_batch_file_dialog();
        self.poll_directory_dialog();
        self.poll_project_dialog();
        self.handle_dropped_files(ui.ctx());
        self.sync_segment_inputs();
        self.handle_shortcuts(ui.ctx());

        if self.application.update() {
            ui.ctx().request_repaint();
        }
        self.autosave_project();

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
        if self.diagnostic_scrub && matches!(self.application.state(), OpenState::Ready) {
            self.run_diagnostic_scrub(ui.ctx());
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
        } else if matches!(self.application.scene_scan(), Some(SceneScan::Running(_))) {
            // The worker only posts into a channel; drive repaints until it
            // reports a result, otherwise the bar freezes at its first value.
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        } else if self.application.waveform_progress().is_some() {
            // The worker updates an atomic counter; repaint so the bar moves.
            ui.ctx().request_repaint_after(Duration::from_millis(100));
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
        self.close_prompt_window(ui.ctx());
        self.silence_window(ui.ctx());
        self.random_window(ui.ctx());
        self.scene_window(ui.ctx());
        self.drop_window(ui.ctx());
        self.export_panel(ui.ctx());
        self.ffmpeg_command_window(ui.ctx());
        self.about_window(ui.ctx());
    }

    fn save(&mut self, _storage: &mut dyn eframe::Storage) {
        save_settings(&self.settings);
    }
}

/// Raw RGBA window icon generated by `build.rs`.
fn window_icon() -> egui::IconData {
    use std::sync::LazyLock;

    const SIZE: u32 = 256;
    static ICON: LazyLock<egui::IconData> = LazyLock::new(|| egui::IconData {
        rgba: include_bytes!(concat!(env!("OUT_DIR"), "/window-icon.rgba")).to_vec(),
        width: SIZE,
        height: SIZE,
    });
    ICON.clone()
}

/// Starts the native event loop and blocks until the window closes.
///
/// Must be called from the main thread.
pub fn run(initial_paths: Vec<PathBuf>) -> Result<(), eframe::Error> {
    // `options.wgpu_options` is only assigned when mpv-preview is enabled.
    #[allow(unused_mut)]
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([960.0, 640.0])
            .with_min_inner_size([720.0, 480.0])
            // The Windows window draws its own title bar so the menu and the
            // window controls share one visual style.
            .with_decorations(!cfg!(target_os = "windows"))
            .with_icon(window_icon()),
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
                initial_paths,
            )))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::i18n::Language;
    use super::panels::file_title;
    use super::preview::fit_size;
    #[cfg(feature = "mpv-preview")]
    use super::preview::{PreviewBounds, physical_bounds};
    #[cfg(target_os = "linux")]
    use super::settings::{gtk_theme_from_ini, ini_value, qt_theme_from_ini, theme_from_label};
    use super::shortcuts::default_shortcuts;
    use super::timeline::{
        fraction_of, timestamp_at_x, wheel_direction, wheel_seek_micros, wheel_target,
    };
    use super::*;
    use crate::domain::time::Duration as MediaDuration;

    #[test]
    fn tracks_unsaved_project_edits() {
        let mut app = RossresscutApp {
            project_file: Some(PathBuf::from("/tmp/clip.rrc")),
            ..Default::default()
        };
        app.saved_revision = app.application.timeline_revision();

        assert!(!app.has_unsaved_project());

        app.application
            .set_chapter_title(SegmentId::new(1), "Intro".to_owned());
        assert!(app.has_unsaved_project());

        app.saved_revision = app.application.timeline_revision();
        assert!(!app.has_unsaved_project());

        app.detach_project();
        assert!(
            !app.has_unsaved_project(),
            "plain media imports are never prompted about saving"
        );
    }

    #[test]
    fn a_project_being_loaded_is_not_dirty() {
        let mut app = RossresscutApp {
            project_file: Some(PathBuf::from("/tmp/clip.rrc")),
            awaiting_project_baseline: true,
            saved_revision: 0,
            ..Default::default()
        };
        app.application
            .set_chapter_title(SegmentId::new(1), "Intro".to_owned());

        assert!(!app.has_unsaved_project());
    }

    #[test]
    fn closing_with_save_writes_the_bound_project() {
        let directory =
            std::env::temp_dir().join(format!("rossresscut-close-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("temp directory");
        let project = directory.join("clip.rrc");

        let mut app = RossresscutApp {
            project_file: Some(project.clone()),
            ..Default::default()
        };
        app.application.open_media(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample.mkv"),
        );
        let deadline = Instant::now() + Duration::from_secs(30);
        while !matches!(app.application.state(), OpenState::Ready) {
            app.application.update();
            assert!(Instant::now() < deadline, "media did not become ready");
            std::thread::sleep(Duration::from_millis(10));
        }

        let id = app.application.timeline().segments()[0].id();
        app.project_file = Some(project.clone());
        app.saved_revision = app.application.timeline_revision();
        app.application
            .set_chapter_title(id, "Opening (edited)".to_owned());
        assert!(app.has_unsaved_project());

        let context = egui::Context::default();
        app.finish_close(CloseIntent::Media, true, &context);

        assert!(project.is_file(), "the project file should be written");
        assert!(!app.has_unsaved_project());
        assert!(
            matches!(app.application.state(), OpenState::Empty),
            "the media should be closed after saving"
        );
        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn closing_without_saving_keeps_the_project_untouched() {
        let directory =
            std::env::temp_dir().join(format!("rossresscut-discard-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("temp directory");
        let project = directory.join("clip.rrc");

        let mut app = RossresscutApp {
            project_file: Some(project.clone()),
            saved_revision: 0,
            ..Default::default()
        };
        app.application
            .set_chapter_title(SegmentId::new(1), "Intro".to_owned());

        let context = egui::Context::default();
        app.finish_close(CloseIntent::Media, false, &context);

        assert!(!project.exists(), "discarding must not write a project");
        std::fs::remove_dir_all(&directory).ok();
    }

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

    #[cfg(feature = "mpv-preview")]
    #[test]
    fn preview_resize_waits_for_a_repeated_size() {
        let mut bounds = PreviewBounds::default();
        assert_eq!(bounds.observe((800, 600)), Some((800, 600)));
        assert!(!bounds.is_pending());

        assert_eq!(bounds.observe((810, 600)), None);
        assert!(bounds.is_pending());
        assert_eq!(bounds.observe((820, 600)), None);
        assert_eq!(bounds.observe((820, 600)), Some((820, 600)));
        assert!(!bounds.is_pending());

        assert_eq!(bounds.observe((820, 600)), None);
        assert!(!bounds.is_pending());
    }

    #[cfg(feature = "mpv-preview")]
    #[test]
    fn preview_bounds_use_physical_pixels() {
        assert_eq!(
            physical_bounds(egui::vec2(640.0, 360.0), 1.0),
            Some((640, 360))
        );
        assert_eq!(
            physical_bounds(egui::vec2(640.0, 360.0), 1.5),
            Some((960, 540))
        );
        assert_eq!(physical_bounds(egui::vec2(0.0, 360.0), 1.0), None);
        assert_eq!(physical_bounds(egui::vec2(640.0, 360.0), 0.0), None);
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
        use super::settings::SettingsFile;

        let mut settings = UiSettings {
            follow_system_theme: false,
            theme: ThemeChoice::Light,
            ..UiSettings::default()
        };

        for language in Language::ALL {
            settings.language = language;
            let file = SettingsFile::from(&settings);
            let text = toml::to_string(&file).expect("serialize settings");

            assert!(text.contains("[general]"), "{text}");
            assert!(text.contains("[export]"), "{text}");

            let parsed =
                UiSettings::from(toml::from_str::<SettingsFile>(&text).expect("parse settings"));
            assert_eq!(parsed, settings);
        }
    }

    #[test]
    fn migrates_the_flat_settings_layout() {
        use super::settings::parse_settings;

        let legacy = "follow_system_theme = false\n\
                      theme = \"light\"\n\
                      language = \"zh_hans\"\n\
                      export_chapters = false\n\
                      export_to_source_dir = false\n\
                      export_directory = \"/tmp/out\"\n\
                      font_zoom = 1.2\n\
                      font_path = \"/fonts/test.ttf\"\n\
                      font_index = 2\n";

        let settings = parse_settings(legacy).expect("legacy settings");

        assert!(!settings.follow_system_theme);
        assert_eq!(settings.theme, ThemeChoice::Light);
        assert_eq!(settings.language, Language::SimplifiedChinese);
        assert!(!settings.export_chapters);
        assert_eq!(settings.export_directory, "/tmp/out");
        assert_eq!(settings.font_zoom, 1.2);
        assert_eq!(settings.font_index, 2);
        // New fields fall back to their defaults.
        assert!(settings.autosave_projects);
        assert!(settings.export_title.is_empty());
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
        assert_eq!(settings.binding(ShortcutAction::DeleteSegment), "Delete");
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
    fn wheel_seek_moves_by_the_configured_duration_per_step() {
        let none = egui::Modifiers::NONE;
        let step = 2_500_000;

        assert_eq!(wheel_seek_micros(1.0, none, step), 2_500_000);
        assert_eq!(wheel_seek_micros(-1.0, none, step), -2_500_000);
        assert_eq!(wheel_seek_micros(3.0, none, step), 7_500_000);
        assert_eq!(wheel_seek_micros(-2.0, none, step), -5_000_000);
        assert_eq!(
            wheel_seek_micros(1.0, egui::Modifiers::COMMAND, step),
            10_000_000
        );
        // A fractional remainder is not a whole step and must not seek.
        assert_eq!(wheel_seek_micros(0.4, none, step), 0);
    }

    #[test]
    fn wheel_target_uses_the_last_pointer_position_after_a_cursor_leave() {
        let context = egui::Context::default();
        context.begin_pass(egui::RawInput {
            events: vec![egui::Event::PointerGone],
            ..Default::default()
        });
        let rect = egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(100.0, 20.0));

        assert!(wheel_target(
            &context,
            rect,
            false,
            Some(egui::pos2(20.0, 20.0))
        ));
        assert!(!wheel_target(
            &context,
            rect,
            false,
            Some(egui::pos2(200.0, 20.0))
        ));
        assert!(!wheel_target(&context, rect, false, None));

        let mut output = context.end_pass();
        output.textures_delta.clear();
    }

    #[test]
    fn wheel_target_defers_to_egui_while_the_pointer_is_known() {
        let context = egui::Context::default();
        context.begin_pass(egui::RawInput {
            events: vec![egui::Event::PointerMoved(egui::pos2(200.0, 200.0))],
            ..Default::default()
        });
        let rect = egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(100.0, 20.0));

        assert!(!wheel_target(
            &context,
            rect,
            false,
            Some(egui::pos2(20.0, 20.0))
        ));
        assert!(wheel_target(
            &context,
            rect,
            true,
            Some(egui::pos2(200.0, 200.0))
        ));

        let mut output = context.end_pass();
        output.textures_delta.clear();
    }

    #[test]
    fn stepped_volume_moves_by_five_and_clamps() {
        assert_eq!(super::stepped_volume(100, 1), 100);
        assert_eq!(super::stepped_volume(100, -4), 80);
        assert_eq!(super::stepped_volume(80, 1), 85);
        assert_eq!(super::stepped_volume(0, -1), 0);
        assert_eq!(super::stepped_volume(0, 1), 5);
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
        assert_eq!(app.settings.binding(ShortcutAction::AddCut), "Alt+I");

        run_shortcut_events(
            &context,
            &mut app,
            vec![egui::Event::Key {
                key: egui::Key::J,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::ALT,
            }],
        );
        assert_eq!(app.settings.binding(ShortcutAction::AddCut), "Alt+I");

        run_shortcut_events(
            &context,
            &mut app,
            vec![egui::Event::Key {
                key: egui::Key::J,
                physical_key: None,
                pressed: false,
                repeat: false,
                modifiers: egui::Modifiers::ALT,
            }],
        );
        assert_eq!(app.settings.binding(ShortcutAction::AddCut), "Alt+J");
        assert!(app.recording_shortcut.is_none());
    }

    #[test]
    fn backspace_clears_the_binding_while_recording() {
        let mut app = RossresscutApp {
            recording_shortcut: Some(ShortcutAction::AddCut),
            ..Default::default()
        };
        let context = egui::Context::default();

        run_shortcut_events(
            &context,
            &mut app,
            vec![egui::Event::Key {
                key: egui::Key::Backspace,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );

        assert_eq!(app.settings.binding(ShortcutAction::AddCut), "");
        assert!(app.recording_shortcut.is_none());
    }
}
