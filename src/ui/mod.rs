//! egui/eframe desktop layer.
//!
//! The UI renders state and forwards user intent to the application layer. It
//! never runs FFprobe, parses FFprobe output, or mutates domain state directly.
//!
//! The layout follows the LosslessCut direction recorded in `GOALS.md`: a top
//! menu bar, a central preview, a bottom timeline, and a side panel with
//! playback and media information.

mod i18n;

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
#[cfg(feature = "mpv-preview")]
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui;
use serde::{Deserialize, Serialize};

use crate::application::{Application, ExportCommandError, ExportStage, ExportStatus, OpenState};
use crate::domain::media::{MediaFile, MediaStream, VideoStream};
use crate::domain::segment::{Segment, SegmentId};
use crate::domain::time::{Duration as MediaDuration, Timestamp};
use crate::media::preview::HardwareAcceleration;

use i18n::{Language, Strings, fill};

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

/// Color theme selected when the system theme is not followed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum ThemeChoice {
    #[default]
    Dark,
    Light,
}

/// Keyboard actions that can be rebound in settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShortcutAction {
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
}

impl ShortcutAction {
    const ALL: [Self; 17] = [
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
    ];

    const fn key(self) -> &'static str {
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
        }
    }

    fn label(self, strings: &Strings) -> String {
        match self {
            Self::PlayPause => format!("{} / {}", strings.play, strings.pause),
            Self::AddCut => strings.add_cut_point.clone(),
            Self::RemoveCut => strings.remove_cut_point.clone(),
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
        }
    }

    const fn default_binding(self) -> &'static str {
        match self {
            Self::PlayPause => "Space",
            Self::AddCut => "I",
            Self::RemoveCut => "O",
            Self::Split => "Enter",
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
        }
    }
}

/// One parsed keyboard shortcut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Shortcut {
    key: egui::Key,
    command: bool,
    shift: bool,
    alt: bool,
}

impl Shortcut {
    fn parse(text: &str) -> Option<Self> {
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

    fn matches(self, input: &egui::InputState) -> bool {
        // `command` is Ctrl on Linux/Windows and Cmd on macOS; accept a raw Ctrl
        // too so the documented `Ctrl+...` bindings work everywhere.
        let command = input.modifiers.command || input.modifiers.ctrl;
        input.key_pressed(self.key)
            && command == self.command
            && input.modifiers.shift == self.shift
            && input.modifiers.alt == self.alt
    }

    fn label(self) -> String {
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

fn key_label(key: egui::Key) -> &'static str {
    match key {
        // `Key::name` calls this key "Comma"; the punctuation form is what
        // users type and what matches the documented `Ctrl+,` binding.
        egui::Key::Comma => ",",
        other => other.name(),
    }
}

fn default_shortcuts() -> BTreeMap<String, String> {
    ShortcutAction::ALL
        .iter()
        .map(|action| (action.key().to_owned(), action.default_binding().to_owned()))
        .collect()
}

/// UI settings persisted to `config.toml`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct UiSettings {
    follow_system_theme: bool,
    theme: ThemeChoice,
    language: Language,
    #[serde(default = "default_true")]
    export_chapters: bool,
    #[serde(default = "default_font_zoom")]
    font_zoom: f32,
    #[serde(default = "default_shortcuts")]
    shortcuts: BTreeMap<String, String>,
}

fn default_true() -> bool {
    true
}

fn default_font_zoom() -> f32 {
    1.0
}

impl UiSettings {
    fn binding(&self, action: ShortcutAction) -> &str {
        self.shortcuts.get(action.key()).map_or("", String::as_str)
    }

    fn shortcut(&self, action: ShortcutAction) -> Option<Shortcut> {
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
            font_zoom: 1.0,
            shortcuts: default_shortcuts(),
        }
    }
}

enum DialogError {
    Failed(String),
    Stopped,
}

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
    export_dialog: Option<Receiver<Option<PathBuf>>>,
    export_error: Option<String>,
    settings: UiSettings,
    font_zoom_draft: f32,
    recording_shortcut: Option<ShortcutAction>,
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

/// Temporary render-path diagnostics, enabled by setting `ROSS_RESCUT_DIAG`.
///
/// Phase 4A uses this to compare egui frame cadence with preview frame rate
/// and to measure CPU texture upload cost. The struct is only constructed when
/// the environment variable is present, so the default path stays unchanged.
struct RenderDiagnostics {
    last_report: Instant,
    frames: u32,
    uploads: u32,
    upload_total: Duration,
    upload_max: Duration,
}

impl Default for RenderDiagnostics {
    fn default() -> Self {
        Self {
            last_report: Instant::now(),
            frames: 0,
            uploads: 0,
            upload_total: Duration::ZERO,
            upload_max: Duration::ZERO,
        }
    }
}

impl RenderDiagnostics {
    fn from_environment() -> Option<Self> {
        std::env::var_os("ROSS_RESCUT_DIAG").map(|_| Self::default())
    }

    fn frame(&mut self) {
        self.frames += 1;
        self.report_if_due();
    }

    fn upload(&mut self, elapsed: Duration) {
        self.uploads += 1;
        self.upload_total += elapsed;
        self.upload_max = self.upload_max.max(elapsed);
    }

    fn report_if_due(&mut self) {
        if self.last_report.elapsed() < Duration::from_secs(2) {
            return;
        }
        let seconds = self.last_report.elapsed().as_secs_f64();
        let upload_average = if self.uploads == 0 {
            0.0
        } else {
            self.upload_total.as_secs_f64() * 1_000.0 / f64::from(self.uploads)
        };
        log::info!(
            "render diag: ui_fps={:.1} uploads={} upload_avg_ms={upload_average:.2} upload_max_ms={:.2}",
            f64::from(self.frames) / seconds,
            self.uploads,
            self.upload_max.as_secs_f64() * 1_000.0,
        );
        self.last_report = Instant::now();
        self.frames = 0;
        self.uploads = 0;
        self.upload_total = Duration::ZERO;
        self.upload_max = Duration::ZERO;
    }
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
        let mut app = Self {
            settings,
            font_zoom_draft,
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

    fn apply_theme(&self, context: &egui::Context) {
        let preference = if self.settings.follow_system_theme {
            egui::ThemePreference::System
        } else {
            match self.settings.theme {
                ThemeChoice::Dark => egui::ThemePreference::Dark,
                ThemeChoice::Light => egui::ThemePreference::Light,
            }
        };
        context.set_theme(preference);
        // `set_zoom_factor` scales text and widgets relative to the system DPI.
        context.set_zoom_factor(self.settings.font_zoom.clamp(MIN_FONT_ZOOM, MAX_FONT_ZOOM));
    }

    fn poll_file_dialog(&mut self) {
        let Some(receiver) = self.file_dialog.take() else {
            return;
        };

        match receiver.try_recv() {
            Ok(Some(path)) => {
                self.reset_scrub();
                self.segment_error = None;
                self.application.open_media(path);
            }
            Ok(None) => {}
            Err(TryRecvError::Empty) => {
                self.file_dialog = Some(receiver);
            }
            Err(TryRecvError::Disconnected) => {
                log::warn!("file dialog thread stopped without a result");
                self.dialog_error = Some(DialogError::Stopped);
            }
        }
    }

    fn poll_export_dialog(&mut self) {
        let Some(receiver) = self.export_dialog.take() else {
            return;
        };

        match receiver.try_recv() {
            Ok(Some(path)) => {
                self.export_error = None;
                if let Err(error) = self
                    .application
                    .start_export(path, self.settings.export_chapters)
                {
                    self.export_error = Some(export_command_message(self.strings(), &error));
                }
            }
            Ok(None) => {}
            Err(TryRecvError::Empty) => {
                self.export_dialog = Some(receiver);
            }
            Err(TryRecvError::Disconnected) => {
                log::warn!("export dialog thread stopped without a result");
                self.export_error = Some(self.strings().export_dialog_failed.clone());
            }
        }
    }

    fn start_export_dialog(&mut self) {
        if self.export_dialog.is_some() {
            return;
        }
        self.export_error = None;

        let Some(default_name) = self.default_export_name() else {
            self.export_error = Some(self.strings().export_no_segment.clone());
            return;
        };
        let dialog_error_message = self.strings().export_dialog_failed.clone();

        let (sender, receiver) = mpsc::channel();
        let spawn = thread::Builder::new()
            .name("export-dialog".to_owned())
            .spawn(move || {
                let path = pick_export_path(&default_name);
                if sender.send(path).is_err() {
                    log::debug!("export dialog result discarded: the window is closing");
                }
            });

        match spawn {
            Ok(_handle) => {
                self.export_dialog = Some(receiver);
            }
            Err(error) => {
                log::error!("failed to start the export dialog thread: {error}");
                self.export_error = Some(dialog_error_message);
            }
        }
    }

    fn default_export_name(&self) -> Option<String> {
        let path = &self.application.media()?.path;
        let stem = path.file_stem()?.to_string_lossy();
        Some(match path.extension() {
            Some(extension) => format!("{stem}_export.{}", extension.to_string_lossy()),
            None => format!("{stem}_export.mkv"),
        })
    }

    fn start_file_dialog(&mut self) {
        if self.file_dialog.is_some() {
            return;
        }
        self.dialog_error = None;

        let (sender, receiver) = mpsc::channel();
        let spawn = thread::Builder::new()
            .name("file-dialog".to_owned())
            .spawn(move || {
                let path = pick_media_file();
                if sender.send(path).is_err() {
                    log::debug!("file dialog result discarded: the window is closing");
                }
            });

        match spawn {
            Ok(_handle) => {
                self.file_dialog = Some(receiver);
            }
            Err(error) => {
                log::error!("failed to start the file dialog thread: {error}");
                self.dialog_error = Some(DialogError::Failed(error.to_string()));
            }
        }
    }

    fn reset_scrub(&mut self) {
        self.scrub_target = None;
        self.scrub_was_playing = false;
        self.last_scrub_target = None;
        self.last_scrub_start = None;
    }

    fn sync_segment_inputs(&mut self) {
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

    fn apply_segment_start_input(&mut self) {
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

    fn apply_segment_end_input(&mut self) {
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

    fn handle_shortcuts(&mut self, context: &egui::Context) {
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

    fn run_shortcut(&mut self, action: ShortcutAction) {
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
            ShortcutAction::OpenSettings => self.settings_open = true,
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
        }
    }

    fn capture_shortcut(&mut self, context: &egui::Context) {
        let Some(action) = self.recording_shortcut else {
            return;
        };
        let captured = context.input(|input| {
            input.events.iter().find_map(|event| match event {
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => Some(Shortcut {
                    key: *key,
                    command: modifiers.command,
                    shift: modifiers.shift,
                    alt: modifiers.alt,
                }),
                _ => None,
            })
        });
        if let Some(shortcut) = captured {
            self.settings
                .shortcuts
                .insert(action.key().to_owned(), shortcut.label());
            self.recording_shortcut = None;
        }
    }

    fn shortcut_tooltip(&self, label: &str, action: ShortcutAction) -> String {
        let binding = self.settings.binding(action);
        if binding.is_empty() {
            label.to_owned()
        } else {
            format!("{label} ({binding})")
        }
    }

    fn set_font_zoom(&mut self, zoom: f32) {
        self.settings.font_zoom = zoom.clamp(MIN_FONT_ZOOM, MAX_FONT_ZOOM);
        self.font_zoom_draft = self.settings.font_zoom;
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

    fn sync_preview_texture(&mut self, context: &egui::Context) {
        if !matches!(self.application.state(), OpenState::Ready) {
            self.clear_preview_textures();
            self.preview_source = None;
            return;
        }

        let source = self.application.media().map(|media| media.path.clone());
        if source != self.preview_source {
            self.clear_preview_textures();
            self.preview_source = source;
        }

        #[cfg(feature = "mpv-preview")]
        {
            if self.application.uses_mpv_preview() {
                self.sync_mpv_texture();
                return;
            }
            if self.preview_mpv_id.is_some() {
                self.clear_mpv_texture();
            }
        }

        let Some(frame) = self.application.frame() else {
            return;
        };
        if self.preview_texture_timestamp == Some(frame.timestamp.micros()) {
            return;
        }

        let width = frame.width as usize;
        let height = frame.height as usize;
        let expected_len = width.saturating_mul(height).saturating_mul(3);
        if frame.rgb.len() != expected_len {
            log::warn!(
                "preview frame has {} bytes, expected {expected_len}",
                frame.rgb.len()
            );
            return;
        }

        let image = egui::ColorImage::from_rgb([width, height], &frame.rgb);
        let upload_start = Instant::now();
        match &mut self.preview_texture {
            Some(texture) => texture.set(image, egui::TextureOptions::LINEAR),
            None => {
                self.preview_texture = Some(context.load_texture(
                    "media_preview",
                    image,
                    egui::TextureOptions::LINEAR,
                ));
            }
        }
        if let Some(diagnostics) = &mut self.diagnostics {
            diagnostics.upload(upload_start.elapsed());
        }
        self.preview_texture_timestamp = Some(frame.timestamp.micros());
    }

    #[cfg(feature = "mpv-preview")]
    fn sync_mpv_texture(&mut self) {
        let Some(render_state) = &self.render_state else {
            return;
        };

        match self.application.acquire_mpv_texture(&render_state.device) {
            Ok(Some(texture)) => {
                let width = texture.width();
                let height = texture.height();
                let view = texture.create_view(&eframe::wgpu::TextureViewDescriptor::default());
                let mut renderer = render_state.renderer.write();
                let first_frame = self.preview_mpv_id.is_none();
                if let Some(id) = self.preview_mpv_id {
                    renderer.update_egui_texture_from_wgpu_texture(
                        &render_state.device,
                        &view,
                        eframe::wgpu::FilterMode::Linear,
                        id,
                    );
                } else {
                    let id = renderer.register_native_texture(
                        &render_state.device,
                        &view,
                        eframe::wgpu::FilterMode::Linear,
                    );
                    self.preview_mpv_id = Some(id);
                }
                drop(renderer);
                if first_frame {
                    log::info!("registered mpv preview texture {width}x{height}");
                }
                let _previous = self.preview_mpv_texture.replace(texture);
                self.preview_texture = None;
                self.preview_texture_timestamp = None;
                self.preview_mpv_size = Some(egui::vec2(width as f32, height as f32));
            }
            Ok(None) => {}
            Err(error) => log::warn!("{error}"),
        }
    }

    #[cfg(feature = "mpv-preview")]
    fn clear_mpv_texture(&mut self) {
        if let (Some(render_state), Some(id)) = (&self.render_state, self.preview_mpv_id.take()) {
            render_state.renderer.write().free_texture(&id);
        }
        self.preview_mpv_texture = None;
        self.preview_mpv_size = None;
    }

    /// Tells the application how large the preview pane is in physical pixels.
    ///
    /// The application fits the source into these bounds, so the exported
    /// texture follows the pane instead of a fixed resolution. A new size is
    /// only forwarded after it repeats for two frames, which avoids
    /// reconfiguring mpv on every frame while a window edge is dragged.
    #[cfg(feature = "mpv-preview")]
    fn request_preview_bounds(&mut self, size: egui::Vec2, context: &egui::Context) {
        if size.x <= 0.0 || size.y <= 0.0 {
            return;
        }
        let pixels_per_point = context.pixels_per_point();
        let width = (size.x * pixels_per_point).round().max(1.0) as u32;
        let height = (size.y * pixels_per_point).round().max(1.0) as u32;
        let bounds = (width, height);
        // Apply the first known pane size immediately so mpv does not allocate
        // a full-resolution render target before the first resize settles.
        if self.preview_bounds.is_none() {
            self.preview_bounds = Some(bounds);
            self.pending_preview_bounds = None;
            self.application.set_preview_bounds(width, height);
            return;
        }
        if self.preview_bounds == Some(bounds) {
            self.pending_preview_bounds = None;
            return;
        }
        if self.pending_preview_bounds == Some(bounds) {
            self.preview_bounds = Some(bounds);
            self.pending_preview_bounds = None;
            self.application.set_preview_bounds(width, height);
        } else {
            self.pending_preview_bounds = Some(bounds);
            context.request_repaint();
        }
    }

    fn clear_preview_textures(&mut self) {
        #[cfg(feature = "mpv-preview")]
        {
            self.clear_mpv_texture();
        }
        self.preview_texture = None;
        self.preview_texture_timestamp = None;
    }

    fn media_title(&self) -> Option<String> {
        let path = match self.application.state() {
            OpenState::Empty => None,
            OpenState::Probing { path } | OpenState::Failed { path, .. } => Some(path),
            OpenState::Ready => self.application.media().map(|media| &media.path),
        }?;
        path.file_stem()
            .or_else(|| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        let strings = self.strings();
        egui::Panel::top("menu_bar").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.spacing_mut().button_padding = egui::vec2(10.0, 6.0);
                ui.menu_button(strings.menu_file.as_str(), |ui| {
                    let dialog_open = self.file_dialog.is_some();
                    let open_enabled = FILE_DIALOG_SUPPORTED && !dialog_open;
                    let open_label = if dialog_open {
                        strings.opening.as_str()
                    } else {
                        strings.open.as_str()
                    };
                    let open = ui
                        .add_enabled(open_enabled, egui::Button::new(open_label))
                        .on_disabled_hover_text(if FILE_DIALOG_SUPPORTED {
                            strings.open_in_progress.as_str()
                        } else {
                            strings.open_disabled.as_str()
                        })
                        .on_hover_text(strings.open.as_str());
                    if open.clicked() {
                        self.start_file_dialog();
                        ui.close();
                    }

                    let has_media = !matches!(self.application.state(), OpenState::Empty);
                    if ui
                        .add_enabled(has_media, egui::Button::new(strings.close_media.as_str()))
                        .on_hover_text(strings.close_media.as_str())
                        .clicked()
                    {
                        self.application.close_media();
                        self.reset_scrub();
                        self.segment_error = None;
                        ui.close();
                    }

                    ui.separator();
                    if ui
                        .button(strings.quit.as_str())
                        .on_hover_text(strings.quit.as_str())
                        .clicked()
                    {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });

                ui.menu_button(strings.menu_view.as_str(), |ui| {
                    let fullscreen = ui
                        .ctx()
                        .input(|input| input.viewport().fullscreen.unwrap_or(false));
                    let fullscreen_label = if fullscreen {
                        strings.exit_fullscreen.as_str()
                    } else {
                        strings.fullscreen.as_str()
                    };
                    if ui
                        .button(fullscreen_label)
                        .on_hover_text(fullscreen_label)
                        .clicked()
                    {
                        ui.ctx()
                            .send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fullscreen));
                        ui.close();
                    }
                    ui.separator();
                    if ui
                        .button(strings.reset_font_size.as_str())
                        .on_hover_text(strings.reset_font_size.as_str())
                        .clicked()
                    {
                        self.set_font_zoom(1.0);
                        ui.close();
                    }
                    if ui
                        .button(strings.increase_font_size.as_str())
                        .on_hover_text(strings.increase_font_size.as_str())
                        .clicked()
                    {
                        let zoom = self.settings.font_zoom + FONT_ZOOM_STEP;
                        self.set_font_zoom(zoom);
                        ui.close();
                    }
                    if ui
                        .button(strings.decrease_font_size.as_str())
                        .on_hover_text(strings.decrease_font_size.as_str())
                        .clicked()
                    {
                        let zoom = self.settings.font_zoom - FONT_ZOOM_STEP;
                        self.set_font_zoom(zoom);
                        ui.close();
                    }
                });

                ui.menu_button(strings.menu_help.as_str(), |ui| {
                    if ui
                        .button(strings.about.as_str())
                        .on_hover_text(strings.about.as_str())
                        .clicked()
                    {
                        self.about_open = true;
                        ui.close();
                    }
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(egui::Button::new("⚙").min_size(egui::vec2(32.0, 28.0)))
                        .on_hover_text(self.shortcut_tooltip(
                            strings.settings.as_str(),
                            ShortcutAction::OpenSettings,
                        ))
                        .clicked()
                    {
                        self.settings_open = true;
                    }
                    let title = self
                        .media_title()
                        .unwrap_or_else(|| strings.no_media.clone());
                    ui.add(egui::Label::new(title).truncate());
                });
            });
        });
    }

    fn settings_window(&mut self, context: &egui::Context) {
        if !self.settings_open {
            return;
        }
        let strings = self.strings();
        let mut open = self.settings_open;

        egui::Window::new(strings.settings.as_str())
            .id(egui::Id::unique("settings_window"))
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .default_width(460.0)
            .show(context, |ui| {
                ui.heading(strings.general.as_str());
                ui.checkbox(
                    &mut self.settings.follow_system_theme,
                    strings.follow_system_theme.as_str(),
                )
                .on_hover_text(strings.follow_system_theme.as_str());
                ui.add_enabled_ui(!self.settings.follow_system_theme, |ui| {
                    egui::ComboBox::from_label(strings.theme.as_str())
                        .selected_text(theme_label(strings, self.settings.theme))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.settings.theme,
                                ThemeChoice::Dark,
                                strings.theme_dark.as_str(),
                            );
                            ui.selectable_value(
                                &mut self.settings.theme,
                                ThemeChoice::Light,
                                strings.theme_light.as_str(),
                            );
                        });
                });

                ui.horizontal(|ui| {
                    ui.label(strings.language.as_str());
                    egui::ComboBox::from_id_salt("language")
                        .selected_text(self.settings.language.label())
                        .show_ui(ui, |ui| {
                            for language in Language::ALL {
                                ui.selectable_value(
                                    &mut self.settings.language,
                                    language,
                                    language.label(),
                                );
                            }
                        });
                });

                ui.horizontal(|ui| {
                    ui.label(strings.font_size.as_str());
                    let slider = ui.add(
                        egui::Slider::new(&mut self.font_zoom_draft, MIN_FONT_ZOOM..=MAX_FONT_ZOOM)
                            .step_by(f64::from(FONT_ZOOM_STEP))
                            .fixed_decimals(2),
                    );
                    // The preview only follows a released slider; this avoids
                    // rebuilding the font atlas on every drag frame.
                    if slider.drag_stopped() || (slider.changed() && !slider.dragged()) {
                        self.settings.font_zoom =
                            self.font_zoom_draft.clamp(MIN_FONT_ZOOM, MAX_FONT_ZOOM);
                    }
                    if ui
                        .button(strings.reset_font_size.as_str())
                        .on_hover_text(strings.reset_font_size.as_str())
                        .clicked()
                    {
                        self.set_font_zoom(1.0);
                    }
                });

                ui.separator();
                ui.heading(strings.export.as_str());
                ui.checkbox(
                    &mut self.settings.export_chapters,
                    strings.export_chapters.as_str(),
                )
                .on_hover_text(strings.export_chapters_hint.as_str());

                ui.separator();
                ui.heading(strings.shortcuts.as_str());
                egui::ScrollArea::vertical()
                    .max_height(320.0)
                    .show(ui, |ui| {
                        for action in ShortcutAction::ALL {
                            ui.horizontal(|ui| {
                                ui.add_sized(
                                    [190.0, 20.0],
                                    egui::Label::new(action.label(strings)).truncate(),
                                );
                                let recording = self.recording_shortcut == Some(action);
                                let binding = self.settings.binding(action).to_owned();
                                let text = if recording {
                                    strings.shortcut_press_keys.clone()
                                } else if binding.is_empty() {
                                    strings.shortcut_unassigned.clone()
                                } else {
                                    binding.clone()
                                };
                                if ui
                                    .add(egui::Button::new(text).min_size(egui::vec2(130.0, 22.0)))
                                    .on_hover_text(strings.shortcut_press_keys.as_str())
                                    .clicked()
                                {
                                    self.recording_shortcut =
                                        if recording { None } else { Some(action) };
                                }
                                if ui
                                    .add_enabled(
                                        !binding.is_empty(),
                                        egui::Button::new("×").min_size(egui::vec2(24.0, 22.0)),
                                    )
                                    .on_hover_text(strings.shortcut_clear.as_str())
                                    .clicked()
                                {
                                    self.settings
                                        .shortcuts
                                        .insert(action.key().to_owned(), String::new());
                                    if recording {
                                        self.recording_shortcut = None;
                                    }
                                }
                            });
                        }
                    });
            });

        self.settings_open = open;
    }

    fn about_window(&mut self, context: &egui::Context) {
        if !self.about_open {
            return;
        }
        let strings = self.strings();
        let mut open = self.about_open;

        egui::Window::new(strings.about.as_str())
            .id(egui::Id::unique("about_window"))
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .show(context, |ui| {
                ui.heading("rossresscut");
                ui.label(strings.about_description.as_str());
                ui.label(fill(&strings.about_version, &[env!("CARGO_PKG_VERSION")]));
                ui.label(fill(&strings.about_commit, &[env!("GIT_COMMIT")]));
                ui.label(fill(&strings.about_phase, &[crate::PHASE]));
            });

        self.about_open = open;
    }

    fn timeline_panel(&mut self, ui: &mut egui::Ui) {
        let strings = self.strings();
        egui::Panel::bottom("media_timeline").show(ui, |ui| {
            ui.add_space(4.0);
            self.timeline_bar(ui);
            self.control_buttons(ui, strings);
            self.export_row(ui, strings);

            if let Some(error) = &self.dialog_error {
                let message = match error {
                    DialogError::Failed(detail) => {
                        fill(&strings.file_dialog_failed, &[detail.as_str()])
                    }
                    DialogError::Stopped => strings.file_dialog_stopped.clone(),
                };
                ui.colored_label(ui.visuals().error_fg_color, message);
            }
            self.export_status(ui);
            if let Some(warning) = self.application.warning() {
                ui.colored_label(ui.visuals().warn_fg_color, warning);
            }
            ui.add_space(4.0);
        });
    }

    fn control_buttons(&mut self, ui: &mut egui::Ui, strings: &'static Strings) {
        let has_media = self.application.media().is_some();
        let has_selection = self.application.selected_segment().is_some();

        // `horizontal_wrapped` shrinks to its content height. A free
        // `with_layout` would claim the panel's remaining height and collapse
        // the preview pane.
        let estimated_row_width = 740.0 * ui.ctx().zoom_factor();
        let padding = ((ui.available_width() - estimated_row_width) * 0.5).max(0.0);
        ui.horizontal_wrapped(|ui| {
            ui.add_space(padding);
            ui.spacing_mut().button_padding = egui::vec2(7.0, 4.0);
            if symbol_button(
                ui,
                "|<",
                has_media,
                &self.shortcut_tooltip(strings.go_to_start.as_str(), ShortcutAction::GoToStart),
            ) {
                self.application.seek_to_start();
            }
            if symbol_button(
                ui,
                "[<",
                has_media && has_selection,
                &self.shortcut_tooltip(
                    strings.select_previous_segment.as_str(),
                    ShortcutAction::PreviousSegment,
                ),
            ) {
                self.application.select_previous_segment();
            }
            if symbol_button(
                ui,
                "<|",
                has_media && has_selection,
                &self.shortcut_tooltip(
                    strings.go_to_segment_start.as_str(),
                    ShortcutAction::SegmentStart,
                ),
            ) {
                self.application.seek_to_selected_segment_start();
            }
            if symbol_button(
                ui,
                "[",
                has_media && has_selection,
                strings.set_start_from_playhead.as_str(),
            ) {
                let position = self.application.position();
                if let Err(error) = self.application.set_selected_segment_start(position) {
                    self.segment_error = Some(error.to_string());
                } else {
                    self.segment_input_segment = None;
                    self.segment_error = None;
                }
            }

            let start_response = ui
                .add_enabled(
                    has_selection,
                    egui::TextEdit::singleline(&mut self.segment_start_input).desired_width(86.0),
                )
                .on_hover_text(strings.segment_start.as_str());
            if start_response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter))
            {
                self.apply_segment_start_input();
            }

            if symbol_button(
                ui,
                "<<",
                has_media,
                &self.shortcut_tooltip(
                    strings.previous_keyframe.as_str(),
                    ShortcutAction::PreviousKeyframe,
                ),
            ) {
                self.application.step_keyframe(false);
            }
            if symbol_button(
                ui,
                "<",
                has_media,
                &self.shortcut_tooltip(
                    strings.previous_frame.as_str(),
                    ShortcutAction::PreviousFrame,
                ),
            ) {
                self.application.step_frame(false);
            }

            let (play_label, play_tooltip) = if self.application.is_playing() {
                ("||", strings.pause.as_str())
            } else {
                ("▶", strings.play.as_str())
            };
            if symbol_button(
                ui,
                play_label,
                has_media,
                &self.shortcut_tooltip(play_tooltip, ShortcutAction::PlayPause),
            ) {
                self.application.toggle_playback();
            }

            if symbol_button(
                ui,
                ">",
                has_media,
                &self.shortcut_tooltip(strings.next_frame.as_str(), ShortcutAction::NextFrame),
            ) {
                self.application.step_frame(true);
            }
            if symbol_button(
                ui,
                ">>",
                has_media,
                &self
                    .shortcut_tooltip(strings.next_keyframe.as_str(), ShortcutAction::NextKeyframe),
            ) {
                self.application.step_keyframe(true);
            }

            let end_response = ui
                .add_enabled(
                    has_selection,
                    egui::TextEdit::singleline(&mut self.segment_end_input).desired_width(86.0),
                )
                .on_hover_text(strings.segment_end.as_str());
            if end_response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                self.apply_segment_end_input();
            }

            if symbol_button(
                ui,
                "]",
                has_media && has_selection,
                strings.set_end_from_playhead.as_str(),
            ) {
                let position = self.application.position();
                if let Err(error) = self.application.set_selected_segment_end(position) {
                    self.segment_error = Some(error.to_string());
                } else {
                    self.segment_input_segment = None;
                    self.segment_error = None;
                }
            }
            if symbol_button(
                ui,
                "|>",
                has_media && has_selection,
                &self.shortcut_tooltip(
                    strings.go_to_segment_end.as_str(),
                    ShortcutAction::SegmentEnd,
                ),
            ) {
                self.application.seek_to_selected_segment_end();
            }
            if symbol_button(
                ui,
                ">]",
                has_media && has_selection,
                &self.shortcut_tooltip(
                    strings.select_next_segment.as_str(),
                    ShortcutAction::NextSegment,
                ),
            ) {
                self.application.select_next_segment();
            }
            if symbol_button(
                ui,
                ">|",
                has_media,
                &self.shortcut_tooltip(strings.go_to_end.as_str(), ShortcutAction::GoToEnd),
            ) {
                self.application.seek_to_end();
            }
        });
    }

    fn export_row(&mut self, ui: &mut egui::Ui, strings: &'static Strings) {
        let position = self
            .scrub_target
            .unwrap_or_else(|| self.application.position());
        let duration = self.application.duration().map_or_else(
            || strings.duration_unavailable.clone(),
            |value| value.to_string(),
        );
        ui.horizontal(|ui| {
            ui.monospace(format!("{position} / {duration}"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let has_enabled_segment = self
                    .application
                    .timeline()
                    .segments()
                    .iter()
                    .any(|segment| segment.enabled());
                let exporting = self.application.export_status().is_running();
                let enabled = FILE_DIALOG_SUPPORTED
                    && self.export_dialog.is_none()
                    && has_enabled_segment
                    && !exporting;
                if ui
                    .add_enabled(
                        enabled,
                        egui::Button::new(strings.export.as_str())
                            .min_size(egui::vec2(120.0, 30.0)),
                    )
                    .on_hover_text(strings.export_segment.as_str())
                    .clicked()
                {
                    self.start_export_dialog();
                }
            });
        });
    }

    fn export_status(&mut self, ui: &mut egui::Ui) {
        let strings = self.strings();
        let status = self.application.export_status().clone();

        match &status {
            ExportStatus::Running { output, stage, .. } => {
                ui.horizontal(|ui| {
                    ui.add(
                        egui::ProgressBar::new(status.progress().unwrap_or_default())
                            .show_percentage(),
                    );
                    if ui
                        .button(strings.cancel_export.as_str())
                        .on_hover_text(strings.cancel_export.as_str())
                        .clicked()
                    {
                        self.application.cancel_export();
                    }
                });
                if let Some(stage) = stage {
                    let label = match stage {
                        ExportStage::Cutting { index, total } => {
                            let index = (*index + 1).to_string();
                            let total = total.to_string();
                            fill(&strings.export_cutting, &[index.as_str(), total.as_str()])
                        }
                        ExportStage::Concatenating => strings.export_concatenating.clone(),
                    };
                    ui.label(label);
                }
                ui.label(format!("{} {}", strings.exporting, output.display()));
            }
            ExportStatus::Completed { output } => {
                let output = output.display().to_string();
                ui.colored_label(
                    ui.visuals().weak_text_color(),
                    fill(&strings.export_completed, &[&output]),
                );
            }
            ExportStatus::Failed { output, message } => {
                let detail = format!("{} ({})", message, output.display());
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    fill(&strings.export_failed, &[&detail]),
                );
            }
            ExportStatus::Cancelled => {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    strings.export_cancelled.as_str(),
                );
            }
            ExportStatus::Idle => {}
        }

        if let Some(error) = &self.export_error {
            ui.colored_label(ui.visuals().error_fg_color, error.as_str());
        }
    }

    fn timeline_bar(&mut self, ui: &mut egui::Ui) {
        let response = ui.allocate_response(
            egui::vec2(ui.available_width(), TIMELINE_HEIGHT),
            egui::Sense::click_and_drag(),
        );
        let rect = response.rect;
        let Some(duration) = self.application.duration() else {
            ui.painter()
                .rect_filled(rect, 4.0, ui.visuals().extreme_bg_color);
            return;
        };

        let position = self
            .scrub_target
            .unwrap_or_else(|| self.application.position());
        let playhead_x = x_for_time(rect, position, duration);

        let visuals = ui.visuals();
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, visuals.extreme_bg_color);

        if let (Some(in_point), Some(out_point)) =
            (self.application.in_point(), self.application.out_point())
        {
            let (start, end) = if in_point <= out_point {
                (in_point, out_point)
            } else {
                (out_point, in_point)
            };
            let region = egui::Rect::from_min_max(
                egui::pos2(x_for_time(rect, start, duration), rect.top()),
                egui::pos2(x_for_time(rect, end, duration), rect.bottom()),
            );
            painter.rect_filled(
                region,
                0.0,
                egui::Color32::from_rgba_unmultiplied(90, 200, 120, 28),
            );
        }

        let selected = self.application.selected_segment();
        for (index, segment) in self.application.timeline().segments().iter().enumerate() {
            let left = x_for_time(rect, segment.start(), duration);
            let right = x_for_time(rect, segment.end(), duration);
            if right <= left {
                continue;
            }
            let segment_rect = egui::Rect::from_min_max(
                egui::pos2(left, rect.top() + 2.0),
                egui::pos2(right, rect.bottom() - 2.0),
            );
            let color = segment_color(segment.id(), segment.enabled(), visuals);
            painter.rect_filled(segment_rect, 2.0, color);
            draw_segment_number(&painter, rect, segment_rect, index + 1, color);
            if selected == Some(segment.id()) {
                painter.rect_stroke(
                    segment_rect,
                    2.0,
                    egui::Stroke::new(1.5, visuals.strong_text_color()),
                    egui::StrokeKind::Inside,
                );
            }
        }

        if let Some(in_point) = self.application.in_point() {
            draw_marker(
                &painter,
                rect,
                x_for_time(rect, in_point, duration),
                egui::Color32::from_rgb(96, 210, 132),
            );
        }
        if let Some(out_point) = self.application.out_point() {
            draw_marker(
                &painter,
                rect,
                x_for_time(rect, out_point, duration),
                egui::Color32::from_rgb(232, 168, 82),
            );
        }

        painter.line_segment(
            [
                egui::pos2(playhead_x, rect.top()),
                egui::pos2(playhead_x, rect.bottom()),
            ],
            egui::Stroke::new(2.0, visuals.strong_text_color()),
        );

        if response.drag_started() {
            self.scrub_was_playing = self.application.is_playing();
            if self.scrub_was_playing {
                self.application.pause();
            }
            self.last_scrub_target = None;
            self.last_scrub_start = None;
        }

        if response.dragged()
            && let Some(pointer) = response.interact_pointer_pos()
        {
            self.scrub_to(timestamp_at_x(rect, pointer.x, duration));
        }

        if response.drag_stopped() {
            // The throttled scrub target can lag behind the pointer, so the
            // release position is the authoritative final target. Dragging
            // only moves the playhead; it does not change the selection.
            let target = response
                .interact_pointer_pos()
                .map(|pointer| timestamp_at_x(rect, pointer.x, duration))
                .or(self.scrub_target)
                .unwrap_or_else(|| self.application.position());
            self.finish_scrub(target);
        } else if response.clicked()
            && let Some(pointer) = response.interact_pointer_pos()
        {
            let target = timestamp_at_x(rect, pointer.x, duration);
            let selected = self
                .application
                .timeline()
                .segments()
                .iter()
                .rev()
                .find(|segment| segment.contains(target))
                .map(|segment| segment.id());
            self.application.select_segment(selected);
            self.application.seek(target);
        }

        if response.hovered() {
            self.handle_wheel(ui);
            if let Some(pointer) = response.hover_pos() {
                let hover_x = pointer.x.clamp(rect.left(), rect.right());
                let hover = timestamp_at_x(rect, hover_x, duration);
                painter.line_segment(
                    [
                        egui::pos2(hover_x, rect.top()),
                        egui::pos2(hover_x, rect.bottom()),
                    ],
                    egui::Stroke::new(1.0, visuals.weak_text_color()),
                );
                let text = self
                    .application
                    .timeline()
                    .segments()
                    .iter()
                    .rev()
                    .find(|segment| segment.contains(hover))
                    .map_or_else(
                        || hover.to_string(),
                        |segment| format!("{} – {}", segment.start(), segment.end()),
                    );
                let _ = response.clone().on_hover_text(text);
            }
        }
    }

    fn preview_panel(&mut self, ui: &mut egui::Ui) {
        let strings = self.strings();
        egui::CentralPanel::default().show(ui, |ui| {
            #[cfg(feature = "mpv-preview")]
            {
                // Use the panel's inner rect so the exported texture matches
                // the area the image is actually painted into.
                let pane = ui.available_rect_before_wrap().size();
                self.request_preview_bounds(pane, ui.ctx());
            }
            match self.application.state() {
                OpenState::Empty => {
                    let response = self.preview_open_response(ui);
                    centered_message(ui, strings.open_media_prompt.as_str());
                    if FILE_DIALOG_SUPPORTED && response.clicked() {
                        self.start_file_dialog();
                    }
                }
                OpenState::Probing { path } => {
                    let title = file_title(path);
                    self.centered_spinner(ui, fill(&strings.probing_file, &[title.as_str()]));
                }
                OpenState::Failed { path, message } => {
                    let title = file_title(path);
                    let response = self.preview_open_response(ui);
                    let mut text =
                        fill(&strings.could_not_open, &[title.as_str(), message.as_str()]);
                    text.push_str("\n\n");
                    text.push_str(&strings.open_media_prompt);
                    centered_message(ui, text);
                    if FILE_DIALOG_SUPPORTED && response.clicked() {
                        self.start_file_dialog();
                    }
                }
                OpenState::Ready => {
                    let mpv_texture: Option<(egui::TextureId, egui::Vec2)> = {
                        #[cfg(feature = "mpv-preview")]
                        {
                            self.preview_mpv_id.zip(self.preview_mpv_size)
                        }
                        #[cfg(not(feature = "mpv-preview"))]
                        {
                            None
                        }
                    };
                    let texture = mpv_texture.or_else(|| {
                        self.preview_texture
                            .as_ref()
                            .map(|texture| (texture.id(), texture.size_vec2()))
                    });
                    if let Some((texture_id, texture_size)) = texture {
                        let rect = ui.available_rect_before_wrap();
                        let response = ui.allocate_rect(rect, egui::Sense::click());

                        // The pane follows the media aspect ratio, so portrait and
                        // landscape sources both keep the image area tight instead
                        // of painting a fixed black rectangle.
                        let size = fit_size(texture_size, rect.size());
                        let image_rect = egui::Rect::from_center_size(rect.center(), size);
                        ui.painter()
                            .rect_filled(image_rect, 0.0, egui::Color32::BLACK);
                        ui.painter().image(
                            texture_id,
                            image_rect,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            egui::Color32::WHITE,
                        );

                        if response.clicked() {
                            self.application.toggle_playback();
                        }
                        if response.hovered() {
                            self.handle_wheel(ui);
                        }
                    } else if self.application.is_waiting_for_preview() {
                        self.centered_spinner(ui, strings.loading_preview.clone());
                    } else {
                        centered_message(ui, strings.no_video_frame.as_str());
                    }
                }
            }
        });
    }

    fn centered_spinner(&self, ui: &mut egui::Ui, message: String) {
        ui.with_layout(
            egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
            |ui| {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(message);
                });
            },
        );
    }

    fn segments_panel(&mut self, ui: &mut egui::Ui, strings: &'static Strings) {
        let has_media = self.application.media().is_some();
        let has_selection = self.application.selected_segment().is_some();
        ui.heading(fill(
            &strings.segments,
            &[&self.application.timeline().len().to_string()],
        ));

        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(has_media, egui::Button::new(strings.add_cut_point.as_str()))
                .on_hover_text(
                    self.shortcut_tooltip(strings.add_cut_point.as_str(), ShortcutAction::AddCut),
                )
                .clicked()
            {
                let position = self.application.position();
                if let Err(error) = self.application.add_cut_point(position) {
                    self.segment_error = Some(error.to_string());
                } else {
                    self.segment_error = None;
                    self.segment_input_segment = None;
                }
            }

            if ui
                .add_enabled(
                    has_media,
                    egui::Button::new(strings.remove_cut_point.as_str()),
                )
                .on_hover_text(
                    self.shortcut_tooltip(
                        strings.remove_cut_point.as_str(),
                        ShortcutAction::RemoveCut,
                    ),
                )
                .clicked()
            {
                self.application.remove_cut_point();
                self.segment_error = None;
                self.segment_input_segment = None;
            }

            if ui
                .add_enabled(has_selection, egui::Button::new(strings.move_up.as_str()))
                .on_hover_text(strings.move_up.as_str())
                .clicked()
            {
                self.application.move_selected_segment_up();
            }

            if ui
                .add_enabled(has_selection, egui::Button::new(strings.move_down.as_str()))
                .on_hover_text(strings.move_down.as_str())
                .clicked()
            {
                self.application.move_selected_segment_down();
            }

            if ui
                .add_enabled(
                    has_selection,
                    egui::Button::new(strings.split_segment.as_str()),
                )
                .on_hover_text(
                    self.shortcut_tooltip(strings.split_segment.as_str(), ShortcutAction::Split),
                )
                .clicked()
            {
                if let Err(error) = self.application.split_selected_segment() {
                    self.segment_error = Some(error.to_string());
                } else {
                    self.segment_error = None;
                    self.segment_input_segment = None;
                }
            }
        });

        if let Some(in_point) = self.application.in_point() {
            ui.label(format!("{}: {in_point}", strings.segment_start));
        }

        let segments: Vec<(usize, Segment)> = self
            .application
            .timeline()
            .segments()
            .iter()
            .copied()
            .enumerate()
            .collect();
        if segments.is_empty() {
            ui.label(strings.no_segments.as_str());
        } else {
            for (index, segment) in segments {
                ui.horizontal(|ui| {
                    let mut enabled = segment.enabled();
                    let checkbox = ui
                        .checkbox(&mut enabled, "")
                        .on_hover_text(strings.enabled.as_str());
                    if checkbox.changed() {
                        self.application.set_segment_enabled(segment.id(), enabled);
                    }

                    let selected = self.application.selected_segment() == Some(segment.id());
                    let label = format!(
                        "#{} {} – {} ({})",
                        index + 1,
                        segment.start(),
                        segment.end(),
                        segment.duration()
                    );
                    let color = segment_color(segment.id(), segment.enabled(), ui.visuals());
                    // Unselected segments keep the same hue at half strength so
                    // the list still maps onto the timeline colours.
                    let fill = if selected {
                        color
                    } else {
                        color.gamma_multiply(0.5)
                    };
                    let mut button = egui::Button::new(label).fill(fill);
                    if selected {
                        button =
                            button.stroke(egui::Stroke::new(1.5, ui.visuals().strong_text_color()));
                    }
                    if ui.add(button).clicked() {
                        self.application.select_segment(Some(segment.id()));
                        self.application.seek(segment.start());
                    }
                });
            }
        }

        if let Some(error) = &self.segment_error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
    }

    fn preview_open_response(&self, ui: &mut egui::Ui) -> egui::Response {
        let rect = ui.available_rect_before_wrap();
        let response = ui.interact(
            rect,
            ui.scope_id().with("preview_open"),
            egui::Sense::click(),
        );
        if FILE_DIALOG_SUPPORTED && self.file_dialog.is_none() {
            response.on_hover_cursor(egui::CursorIcon::PointingHand)
        } else {
            response
        }
    }

    fn handle_wheel(&mut self, ui: &egui::Ui) {
        let (delta, modifiers) = ui.input(|input| {
            let mut delta = 0.0;
            let mut modifiers = egui::Modifiers::NONE;
            for event in &input.events {
                if let egui::Event::MouseWheel {
                    unit,
                    delta: wheel,
                    source,
                    modifiers: event_modifiers,
                    ..
                } = event
                {
                    if *source == egui::MouseWheelSource::Momentum {
                        continue;
                    }
                    let points = match unit {
                        egui::MouseWheelUnit::Point => wheel.y,
                        egui::MouseWheelUnit::Line => wheel.y * WHEEL_LINE_POINTS,
                        egui::MouseWheelUnit::Page => {
                            wheel.y * WHEEL_LINE_POINTS * WHEEL_PAGE_LINES
                        }
                    };
                    delta += points;
                    modifiers = *event_modifiers;
                }
            }
            (delta, modifiers)
        });

        if delta == 0.0 {
            return;
        }
        self.wheel_accumulator += delta;

        let steps = (self.wheel_accumulator / WHEEL_STEP_POINTS).trunc();
        if steps == 0.0 {
            return;
        }
        self.wheel_accumulator -= steps * WHEEL_STEP_POINTS;

        if modifiers.command {
            let count = steps.abs() as i64;
            self.application
                .skip(wheel_direction(steps) * 10_000_000 * count);
        } else {
            self.application.step_keyframe(wheel_direction(steps) > 0);
        }
    }

    fn info_panel(&mut self, ui: &mut egui::Ui) {
        let strings = self.strings();
        egui::Panel::right("media_information")
            .resizable(true)
            .default_size(360.0)
            .show(ui, |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.heading(strings.playback.as_str());
                    render_playback_info(ui, &self.application, strings);
                    ui.separator();

                    self.segments_panel(ui, strings);
                });
            });
    }
}

impl eframe::App for RossresscutApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if let Some(diagnostics) = &mut self.diagnostics {
            diagnostics.frame();
        }
        self.apply_theme(ui.ctx());
        self.poll_file_dialog();
        self.poll_export_dialog();
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

        self.sync_preview_texture(ui.ctx());
        self.menu_bar(ui);
        self.timeline_panel(ui);
        self.info_panel(ui);
        self.preview_panel(ui);
        self.settings_window(ui.ctx());
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

fn render_playback_info(ui: &mut egui::Ui, application: &Application, strings: &Strings) {
    egui::Grid::new("playback_info")
        .num_columns(2)
        .min_col_width(115.0)
        .striped(true)
        .show(ui, |ui| {
            row(ui, &strings.decoder, decoder_label(application, strings));
            row(ui, &strings.frame, frame_summary(application));
            row(ui, &strings.video, video_summary(application.media()));
            row(ui, &strings.audio, audio_summary(application.media()));
        });
}

fn decoder_label(application: &Application, strings: &Strings) -> String {
    if application.uses_mpv_preview() {
        return match application.decoder_name() {
            Some(name) => format!("mpv ({name})"),
            None => "mpv (libmpv)".to_owned(),
        };
    }
    match application.hardware_decoder() {
        Some(name) => format!("ffmpeg ({name})"),
        None => format!("ffmpeg ({})", strings.decoder_software),
    }
}

fn frame_summary(application: &Application) -> String {
    match (application.frame_size(), application.frame_timestamp()) {
        (Some((width, height)), Some(timestamp)) => {
            format!("{width}x{height} @ {timestamp}")
        }
        _ => "-".to_owned(),
    }
}

fn video_summary(media: Option<&MediaFile>) -> String {
    let Some(media) = media else {
        return "-".to_owned();
    };
    let Some(video) = media.streams.iter().find_map(|stream| match stream {
        MediaStream::Video(video) if !video.info.disposition.attached_pic => Some(video),
        _ => None,
    }) else {
        return "-".to_owned();
    };

    let codec = video.info.codec_name.as_deref().unwrap_or("unknown");
    let resolution = resolution_text(video);
    match video.frame_rate {
        Some(rate) => format!("{codec} {resolution} @ {rate} fps"),
        None => format!("{codec} {resolution}"),
    }
}

fn audio_summary(media: Option<&MediaFile>) -> String {
    let Some(media) = media else {
        return "-".to_owned();
    };
    let Some(audio) = media.streams.iter().find_map(|stream| match stream {
        MediaStream::Audio(audio) => Some(audio),
        _ => None,
    }) else {
        return "-".to_owned();
    };

    let codec = audio.info.codec_name.as_deref().unwrap_or("unknown");
    let sample_rate = audio
        .sample_rate
        .map(|rate| format!("{rate} Hz"))
        .unwrap_or_else(|| "-".to_owned());
    let channels = audio
        .channels
        .map(|channels| format!("{channels} ch"))
        .unwrap_or_else(|| "-".to_owned());
    format!("{codec}, {sample_rate}, {channels}")
}

fn resolution_text(video: &VideoStream) -> String {
    match (video.width, video.height) {
        (Some(width), Some(height)) => format!("{width}x{height}"),
        _ => "-".to_owned(),
    }
}

fn row(ui: &mut egui::Ui, label: &str, value: impl Into<String>) {
    ui.label(label);
    ui.add(egui::Label::new(value.into()).wrap());
    ui.end_row();
}

fn centered_message(ui: &mut egui::Ui, message: impl Into<String>) {
    ui.with_layout(
        egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
        |ui| {
            ui.add(egui::Label::new(message.into()).wrap().selectable(false));
        },
    );
}

fn fit_size(source: egui::Vec2, available: egui::Vec2) -> egui::Vec2 {
    if source.x <= 0.0 || source.y <= 0.0 || available.x <= 0.0 || available.y <= 0.0 {
        return egui::Vec2::ZERO;
    }
    let scale = (available.x / source.x).min(available.y / source.y);
    source * scale
}

fn fraction_of(position: Timestamp, duration: MediaDuration) -> f32 {
    if duration.micros() == 0 {
        return 0.0;
    }
    let fraction = position.micros().max(0) as f64 / duration.micros() as f64;
    fraction.clamp(0.0, 1.0) as f32
}

fn x_for_time(rect: egui::Rect, time: Timestamp, duration: MediaDuration) -> f32 {
    rect.left() + rect.width() * fraction_of(time, duration)
}

fn segment_color(id: SegmentId, enabled: bool, visuals: &egui::Visuals) -> egui::Color32 {
    if enabled {
        SEGMENT_COLORS[(id.get() % SEGMENT_COLORS.len() as u64) as usize]
    } else {
        visuals.weak_text_color().gamma_multiply(0.5)
    }
}

fn draw_segment_number(
    painter: &egui::Painter,
    timeline: egui::Rect,
    segment_rect: egui::Rect,
    number: usize,
    background: egui::Color32,
) {
    let luminance = 0.2126 * f32::from(background.r())
        + 0.7152 * f32::from(background.g())
        + 0.0722 * f32::from(background.b());
    let color = if luminance > 140.0 {
        egui::Color32::from_rgb(24, 24, 24)
    } else {
        egui::Color32::from_rgb(236, 236, 236)
    };
    let galley =
        painter.layout_no_wrap(number.to_string(), egui::FontId::proportional(11.0), color);
    let size = galley.size();
    let center = segment_rect.center();
    // Short segments cannot fit their number, so it moves to the left of the
    // block while staying inside the timeline.
    let x = if segment_rect.width() >= size.x + 8.0 {
        center.x - size.x / 2.0
    } else {
        (segment_rect.left() - size.x - 2.0).max(timeline.left() + 1.0)
    };
    painter.galley(egui::pos2(x, center.y - size.y / 2.0), galley, color);
}

fn draw_marker(painter: &egui::Painter, rect: egui::Rect, x: f32, color: egui::Color32) {
    painter.line_segment(
        [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
        egui::Stroke::new(1.5, color),
    );
    painter.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(x - 4.0, rect.top()),
            egui::pos2(x + 4.0, rect.top()),
            egui::pos2(x, rect.top() + 5.0),
        ],
        color,
        egui::Stroke::new(0.0, color),
    ));
}

fn timestamp_at_x(rect: egui::Rect, x: f32, duration: MediaDuration) -> Timestamp {
    if rect.width() <= 0.0 {
        return Timestamp::ZERO;
    }
    let fraction = ((x - rect.left()) / rect.width()).clamp(0.0, 1.0) as f64;
    let micros = (fraction * duration.micros() as f64).round();
    Timestamp::from_micros(micros.clamp(0.0, i64::MAX as f64) as i64)
}

fn file_title(path: &std::path::Path) -> String {
    path.file_stem()
        .or_else(|| path.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn theme_label(strings: &Strings, theme: ThemeChoice) -> String {
    match theme {
        ThemeChoice::Dark => strings.theme_dark.clone(),
        ThemeChoice::Light => strings.theme_light.clone(),
    }
}

/// Maps accumulated wheel points to playback direction.
///
/// Positive egui scroll points mean the content moves down, which is the
/// gesture users perceive as scrolling forward in this application: next
/// keyframe / later position.
fn wheel_direction(steps: f32) -> i64 {
    if steps > 0.0 { 1 } else { -1 }
}

fn settings_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join("rossresscut").join("config.toml"))
}

fn load_settings() -> UiSettings {
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

fn save_settings(settings: &UiSettings) {
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

fn symbol_button(ui: &mut egui::Ui, label: &str, enabled: bool, tooltip: &str) -> bool {
    ui.add_enabled(
        enabled,
        egui::Button::new(label).min_size(egui::vec2(32.0, 24.0)),
    )
    .on_hover_text(tooltip)
    .clicked()
}

/// Parses `HH:MM:SS.mmm` or decimal seconds from a segment input field.
fn parse_time_input(text: &str) -> Option<Timestamp> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if let Some(time) = Timestamp::from_seconds(text) {
        return Some(time);
    }

    let (clock, fraction) = text.split_once('.').unwrap_or((text, "0"));
    let parts: Vec<&str> = clock.split(':').collect();
    if parts.len() != 3 {
        return None;
    }
    let hours: i64 = parts[0].parse().ok()?;
    let minutes: i64 = parts[1].parse().ok()?;
    let seconds: i64 = parts[2].parse().ok()?;
    if hours < 0 || minutes < 0 || seconds < 0 || minutes >= 60 || seconds >= 60 {
        return None;
    }
    let fraction: String = fraction.chars().take(3).collect();
    let millis: i64 = format!("{fraction:0<3}").parse().ok()?;
    let total_seconds = hours
        .checked_mul(3600)?
        .checked_add(minutes.checked_mul(60)?)?
        .checked_add(seconds)?;
    let micros = total_seconds
        .checked_mul(1_000_000)?
        .checked_add(millis.checked_mul(1_000)?)?;
    Some(Timestamp::from_micros(micros))
}

#[cfg(target_os = "linux")]
fn pick_media_file() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter(
            "Media files",
            &[
                "mp4", "mkv", "mov", "avi", "wmv", "ts", "m2ts", "webm", "flv", "mpg", "mpeg",
                "m4v", "mp3", "aac", "flac", "wav", "ogg", "opus", "mka", "srt", "ass",
            ],
        )
        .pick_file()
}

#[cfg(not(target_os = "linux"))]
fn pick_media_file() -> Option<PathBuf> {
    None
}

#[cfg(target_os = "linux")]
fn pick_export_path(default_name: &str) -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_file_name(default_name)
        .add_filter("Matroska", &["mkv"])
        .add_filter("MP4", &["mp4"])
        .save_file()
}

#[cfg(not(target_os = "linux"))]
fn pick_export_path(_default_name: &str) -> Option<PathBuf> {
    None
}

fn export_command_message(strings: &Strings, error: &ExportCommandError) -> String {
    match error {
        ExportCommandError::NoSegment => strings.export_no_segment.clone(),
        ExportCommandError::InvalidOutput => strings.export_invalid_output.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            font_zoom: 1.0,
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
}
