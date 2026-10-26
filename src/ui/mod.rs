//! egui/eframe desktop layer.
//!
//! The UI renders state and forwards user intent to the application layer. It
//! never runs FFprobe, parses FFprobe output, or mutates domain state directly.
//!
//! The layout follows the LosslessCut direction recorded in `GOALS.md`: a top
//! menu bar, a central preview, a bottom timeline, and a side panel with
//! playback and media information.

mod i18n;

use std::path::PathBuf;
#[cfg(feature = "mpv-preview")]
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui;
use serde::{Deserialize, Serialize};

use crate::application::{Application, OpenState};
use crate::domain::media::{
    AudioStream, Disposition, MediaFile, MediaStream, StreamInfo, VideoStream,
};
use crate::domain::segment::{Segment, SegmentId};
use crate::domain::time::{Duration as MediaDuration, Timestamp};
use crate::media::preview::HardwareAcceleration;

use i18n::{Language, Strings, fill};

const FILE_DIALOG_SUPPORTED: bool = cfg!(target_os = "linux");
const SETTINGS_KEY: &str = "ui_settings";
const WHEEL_STEP_POINTS: f32 = 50.0;
const WHEEL_LINE_POINTS: f32 = 50.0;
const WHEEL_PAGE_LINES: f32 = 10.0;
const TIMELINE_HEIGHT: f32 = 28.0;
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

/// UI settings persisted by eframe.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct UiSettings {
    follow_system_theme: bool,
    theme: ThemeChoice,
    language: Language,
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            follow_system_theme: true,
            theme: ThemeChoice::Dark,
            language: Language::English,
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
    settings: UiSettings,
    settings_open: bool,
    about_open: bool,
    wheel_accumulator: f32,
    segment_error: Option<String>,
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

        let settings = context
            .storage
            .and_then(|storage| eframe::get_value(storage, SETTINGS_KEY))
            .unwrap_or_default();
        let mut app = Self {
            settings,
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

    fn create_segment(&mut self) {
        match self.application.add_segment() {
            Ok(_) => self.segment_error = None,
            Err(error) => self.segment_error = Some(error.to_string()),
        }
    }

    fn handle_shortcuts(&mut self, context: &egui::Context) {
        if context.egui_wants_keyboard_input() {
            return;
        }

        let (play_pause, set_in, set_out, add, delete, escape) = context.input(|input| {
            (
                input.key_pressed(egui::Key::Space),
                input.key_pressed(egui::Key::I),
                input.key_pressed(egui::Key::O),
                input.key_pressed(egui::Key::Enter),
                input.key_pressed(egui::Key::Delete),
                input.key_pressed(egui::Key::Escape),
            )
        });

        let position = self.application.position();
        if set_in {
            self.application.set_in_point(position);
            self.segment_error = None;
        }
        if set_out {
            self.application.set_out_point(position);
            self.segment_error = None;
        }
        if play_pause {
            self.application.toggle_playback();
        }
        if add && self.application.media().is_some() {
            self.create_segment();
        }
        if delete {
            self.application.remove_selected_segment();
            self.segment_error = None;
        }
        if escape {
            self.application.clear_markers();
            self.segment_error = None;
        }
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
                        });
                    if open.clicked() {
                        self.start_file_dialog();
                        ui.close();
                    }

                    let has_media = !matches!(self.application.state(), OpenState::Empty);
                    if ui
                        .add_enabled(has_media, egui::Button::new(strings.close_media.as_str()))
                        .clicked()
                    {
                        self.application.close_media();
                        self.reset_scrub();
                        self.segment_error = None;
                        ui.close();
                    }

                    ui.separator();
                    if ui.button(strings.quit.as_str()).clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });

                ui.menu_button(strings.menu_view.as_str(), |ui| {
                    if ui.button(strings.settings.as_str()).clicked() {
                        self.settings_open = true;
                        ui.close();
                    }
                });

                ui.menu_button(strings.menu_help.as_str(), |ui| {
                    if ui.button(strings.about.as_str()).clicked() {
                        self.about_open = true;
                        ui.close();
                    }
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .button("⚙")
                        .on_hover_text(strings.settings.as_str())
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
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .show(context, |ui| {
                ui.heading(strings.appearance.as_str());
                ui.checkbox(
                    &mut self.settings.follow_system_theme,
                    strings.follow_system_theme.as_str(),
                );
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

                ui.separator();
                ui.heading(strings.language.as_str());
                egui::ComboBox::from_label(strings.language.as_str())
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

        self.settings_open = open;
    }

    fn about_window(&mut self, context: &egui::Context) {
        if !self.about_open {
            return;
        }
        let strings = self.strings();
        let mut open = self.about_open;

        egui::Window::new(strings.about.as_str())
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .show(context, |ui| {
                ui.heading("rossresscut");
                ui.label(strings.about_description.as_str());
                ui.label(fill(&strings.about_version, &[env!("CARGO_PKG_VERSION")]));
            });

        self.about_open = open;
    }

    fn timeline_panel(&mut self, ui: &mut egui::Ui) {
        let strings = self.strings();
        egui::Panel::bottom("media_timeline").show(ui, |ui| {
            ui.add_space(4.0);
            self.timeline_bar(ui);

            ui.horizontal(|ui| {
                let can_play = self.application.media().is_some();
                let playing = self.application.is_playing();
                let label = if playing {
                    strings.pause.as_str()
                } else {
                    strings.play.as_str()
                };
                if ui.add_enabled(can_play, egui::Button::new(label)).clicked() {
                    self.application.toggle_playback();
                }

                let position = self
                    .scrub_target
                    .unwrap_or_else(|| self.application.position());
                let duration = self.application.duration().map_or_else(
                    || strings.duration_unavailable.clone(),
                    |value| value.to_string(),
                );
                ui.monospace(format!("{position} / {duration}"));
            });

            if let Some(error) = &self.dialog_error {
                let message = match error {
                    DialogError::Failed(detail) => {
                        fill(&strings.file_dialog_failed, &[detail.as_str()])
                    }
                    DialogError::Stopped => strings.file_dialog_stopped.clone(),
                };
                ui.colored_label(ui.visuals().error_fg_color, message);
            }
            if let Some(warning) = self.application.warning() {
                ui.colored_label(ui.visuals().warn_fg_color, warning);
            }
            ui.add_space(4.0);
        });
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
        for segment in self.application.timeline().segments().iter() {
            let left = x_for_time(rect, segment.start(), duration);
            let right = x_for_time(rect, segment.end(), duration);
            if right <= left {
                continue;
            }
            let segment_rect = egui::Rect::from_min_max(
                egui::pos2(left, rect.top() + 2.0),
                egui::pos2(right, rect.bottom() - 2.0),
            );
            painter.rect_filled(
                segment_rect,
                2.0,
                segment_color(segment.id(), segment.enabled(), visuals),
            );
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
            // release position is the authoritative final target.
            let target = response
                .interact_pointer_pos()
                .map(|pointer| timestamp_at_x(rect, pointer.x, duration))
                .or(self.scrub_target)
                .unwrap_or_else(|| self.application.position());
            let selected = self
                .application
                .timeline()
                .segments()
                .iter()
                .rev()
                .find(|segment| segment.contains(target))
                .map(|segment| segment.id());
            self.finish_scrub(target);
            self.application.select_segment(selected);
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
                    if response.clicked() {
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
                    if response.clicked() {
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
        ui.heading(fill(
            &strings.segments,
            &[&self.application.timeline().len().to_string()],
        ));

        ui.horizontal_wrapped(|ui| {
            let set_in = ui
                .add_enabled(has_media, egui::Button::new(strings.set_in.as_str()))
                .on_hover_text(format!("{} (I)", strings.set_in));
            if set_in.clicked() {
                let position = self.application.position();
                self.application.set_in_point(position);
                self.segment_error = None;
            }

            let set_out = ui
                .add_enabled(has_media, egui::Button::new(strings.set_out.as_str()))
                .on_hover_text(format!("{} (O)", strings.set_out));
            if set_out.clicked() {
                let position = self.application.position();
                self.application.set_out_point(position);
                self.segment_error = None;
            }

            let add = ui
                .add_enabled(has_media, egui::Button::new(strings.add_segment.as_str()))
                .on_hover_text(format!("{} (Enter)", strings.add_segment));
            if add.clicked() {
                self.create_segment();
            }

            let has_markers =
                self.application.in_point().is_some() || self.application.out_point().is_some();
            if ui
                .add_enabled(
                    has_markers,
                    egui::Button::new(strings.clear_markers.as_str()),
                )
                .clicked()
            {
                self.application.clear_markers();
                self.segment_error = None;
            }

            let can_sort = self.application.timeline().len() > 1;
            if ui
                .add_enabled(can_sort, egui::Button::new(strings.sort_segments.as_str()))
                .clicked()
            {
                self.application.sort_segments();
            }
        });

        ui.horizontal(|ui| {
            ui.label(format!(
                "{}: {}",
                strings.in_point,
                dash(self.application.in_point())
            ));
            ui.label(format!(
                "{}: {}",
                strings.out_point,
                dash(self.application.out_point())
            ));
        });

        if self.application.selected_segment().is_some() {
            ui.horizontal_wrapped(|ui| {
                if ui
                    .button(strings.set_start_from_playhead.as_str())
                    .clicked()
                {
                    let position = self.application.position();
                    if let Err(error) = self.application.set_selected_segment_start(position) {
                        self.segment_error = Some(error.to_string());
                    }
                }
                if ui.button(strings.set_end_from_playhead.as_str()).clicked() {
                    let position = self.application.position();
                    if let Err(error) = self.application.set_selected_segment_end(position) {
                        self.segment_error = Some(error.to_string());
                    }
                }
                if ui.button(strings.delete_segment.as_str()).clicked() {
                    self.application.remove_selected_segment();
                    self.segment_error = None;
                }
                if ui.button(strings.move_up.as_str()).clicked() {
                    self.application.move_selected_segment_up();
                }
                if ui.button(strings.move_down.as_str()).clicked() {
                    self.application.move_selected_segment_down();
                }
            });
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
                    if ui.selectable_label(selected, label).clicked() {
                        self.application.select_segment(Some(segment.id()));
                        self.application.seek(segment.start());
                    }
                });
            }
        }

        ui.small(strings.segment_shortcuts.as_str());

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
                    render_playback_info(ui, &self.application, self.scrub_target, strings);
                    ui.separator();

                    self.segments_panel(ui, strings);
                    ui.separator();

                    ui.heading(strings.media.as_str());
                    ui.separator();
                    match self.application.state() {
                        OpenState::Empty => {
                            ui.label(strings.no_media_loaded.as_str());
                        }
                        OpenState::Probing { path } => {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(strings.reading_media.as_str());
                            });
                            ui.monospace(path.display().to_string());
                        }
                        OpenState::Failed { path, message } => {
                            ui.colored_label(
                                ui.visuals().error_fg_color,
                                path.display().to_string(),
                            );
                            ui.label(message);
                        }
                        OpenState::Ready => {
                            if let Some(media) = self.application.media() {
                                render_media_info(ui, media, strings);
                            }
                        }
                    }
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

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, SETTINGS_KEY, &self.settings);
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

fn render_playback_info(
    ui: &mut egui::Ui,
    application: &Application,
    scrub_target: Option<Timestamp>,
    strings: &Strings,
) {
    let scrubbing = scrub_target.is_some();
    let displayed_position = scrub_target.unwrap_or_else(|| application.position());

    egui::Grid::new("playback_info")
        .num_columns(2)
        .min_col_width(115.0)
        .striped(true)
        .show(ui, |ui| {
            row(
                ui,
                &strings.state,
                playback_state(application, scrubbing, strings),
            );
            row(
                ui,
                &strings.position,
                format!("{} / {}", displayed_position, dash(application.duration())),
            );
            row(
                ui,
                &strings.sync,
                playback_sync(application, scrubbing, strings),
            );
            row(ui, &strings.decoder, decoder_label(application, strings));
            row(ui, &strings.preview, strings.preview_summary.clone());
            row(ui, &strings.frame, frame_summary(application));
            row(ui, &strings.video, video_summary(application.media()));
            row(ui, &strings.audio, audio_summary(application.media()));
        });
}

fn decoder_label(application: &Application, strings: &Strings) -> String {
    if application.uses_mpv_preview() {
        return "mpv (libmpv)".to_owned();
    }
    match application.hardware_decoder() {
        Some(name) => format!("ffmpeg ({name})"),
        None => format!("ffmpeg ({})", strings.decoder_software),
    }
}

fn playback_state(application: &Application, scrubbing: bool, strings: &Strings) -> String {
    if scrubbing {
        return strings.state_scrubbing.clone();
    }
    match application.state() {
        OpenState::Empty => strings.state_no_media.clone(),
        OpenState::Probing { .. } => strings.state_probing.clone(),
        OpenState::Failed { .. } => strings.state_failed.clone(),
        OpenState::Ready => {
            if application.is_playing() {
                strings.state_playing.clone()
            } else {
                strings.state_paused.clone()
            }
        }
    }
}

fn playback_sync(application: &Application, scrubbing: bool, strings: &Strings) -> String {
    if scrubbing {
        strings.sync_scrub.clone()
    } else if application.is_playing() {
        if application.uses_audio_clock() {
            strings.sync_audio_clock.clone()
        } else {
            strings.sync_wall_clock.clone()
        }
    } else {
        "-".to_owned()
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

fn render_media_info(ui: &mut egui::Ui, media: &MediaFile, strings: &Strings) {
    let file_name = media.path.file_name().map_or_else(
        || media.path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    ui.heading(file_name);
    ui.monospace(media.path.display().to_string());
    ui.separator();

    ui.heading(strings.format.as_str());
    egui::Grid::new("media_format")
        .num_columns(2)
        .min_col_width(115.0)
        .striped(true)
        .show(ui, |ui| {
            row(ui, &strings.container, media.format.name.clone());
            row(
                ui,
                &strings.long_name,
                dash(media.format.long_name.as_deref()),
            );
            row(ui, &strings.duration, dash(media.duration));
            row(ui, &strings.start_time, dash(media.start_time));
            row(
                ui,
                &strings.size,
                dash(media.format.size.map(|size| format!("{size} bytes"))),
            );
            row(
                ui,
                &strings.bit_rate,
                dash(media.format.bit_rate.map(|rate| format!("{rate} bit/s"))),
            );
        });

    ui.separator();
    ui.heading(fill(&strings.streams, &[&media.streams.len().to_string()]));
    for stream in &media.streams {
        egui::CollapsingHeader::new(stream_title(stream, strings))
            .default_open(false)
            .id_salt(("stream", stream.info().index))
            .show(ui, |ui| render_stream_details(ui, stream, strings));
    }

    if !media.chapters.is_empty() {
        ui.separator();
        ui.heading(fill(
            &strings.chapters,
            &[&media.chapters.len().to_string()],
        ));
        egui::Grid::new("media_chapters")
            .num_columns(4)
            .min_col_width(45.0)
            .striped(true)
            .show(ui, |ui| {
                ui.label(strings.id.as_str());
                ui.label(strings.start_time.as_str());
                ui.label(strings.end.as_str());
                ui.label(strings.title.as_str());
                ui.end_row();

                for chapter in &media.chapters {
                    ui.label(chapter.id.to_string());
                    ui.label(chapter.start.to_string());
                    ui.label(chapter.end.to_string());
                    ui.label(chapter.title.as_deref().unwrap_or("-"));
                    ui.end_row();
                }
            });
    }
}

fn render_stream_details(ui: &mut egui::Ui, stream: &MediaStream, strings: &Strings) {
    let info = stream.info();

    egui::Grid::new(format!("stream_shared_{}", info.index))
        .num_columns(2)
        .min_col_width(115.0)
        .striped(true)
        .show(ui, |ui| {
            row(ui, &strings.codec, dash(info.codec_name.as_deref()));
            row(
                ui,
                &strings.codec_name,
                dash(info.codec_long_name.as_deref()),
            );
            row(ui, &strings.profile, dash(info.profile.as_deref()));
            row(ui, &strings.language, dash(info.language.as_deref()));
            row(ui, &strings.title, dash(info.title.as_deref()));
            row(
                ui,
                &strings.bit_rate,
                dash(info.bit_rate.map(|rate| format!("{rate} bit/s"))),
            );
            row(ui, &strings.time_base, dash(info.time_base));
            row(ui, &strings.start_time, dash(info.start_time));
            row(ui, &strings.duration, dash(info.duration));
            row(
                ui,
                &strings.disposition,
                disposition_text(&info.disposition, strings),
            );
        });

    match stream {
        MediaStream::Video(video) => render_video_details(ui, video, strings),
        MediaStream::Audio(audio) => render_audio_details(ui, audio, strings),
        MediaStream::Subtitle(_) | MediaStream::Other(_) => {}
    }
}

fn render_video_details(ui: &mut egui::Ui, video: &VideoStream, strings: &Strings) {
    egui::Grid::new(format!("stream_video_{}", video.info.index))
        .num_columns(2)
        .min_col_width(115.0)
        .striped(true)
        .show(ui, |ui| {
            row(ui, &strings.resolution, resolution_text(video));
            row(
                ui,
                &strings.pixel_format,
                dash(video.pixel_format.as_deref()),
            );
            row(ui, &strings.frame_rate, dash(video.frame_rate));
            row(
                ui,
                &strings.average_frame_rate,
                dash(video.average_frame_rate),
            );
            row(ui, &strings.color_range, dash(video.color_range.as_deref()));
            row(ui, &strings.color_space, dash(video.color_space.as_deref()));
            row(
                ui,
                &strings.color_transfer,
                dash(video.color_transfer.as_deref()),
            );
            row(
                ui,
                &strings.color_primaries,
                dash(video.color_primaries.as_deref()),
            );
        });
}

fn render_audio_details(ui: &mut egui::Ui, audio: &AudioStream, strings: &Strings) {
    egui::Grid::new(format!("stream_audio_{}", audio.info.index))
        .num_columns(2)
        .min_col_width(115.0)
        .striped(true)
        .show(ui, |ui| {
            row(
                ui,
                &strings.sample_rate,
                dash(audio.sample_rate.map(|rate| format!("{rate} Hz"))),
            );
            row(ui, &strings.channels, dash(audio.channels));
            row(
                ui,
                &strings.channel_layout,
                dash(audio.channel_layout.as_deref()),
            );
            row(
                ui,
                &strings.sample_format,
                dash(audio.sample_format.as_deref()),
            );
        });
}

fn row(ui: &mut egui::Ui, label: &str, value: impl Into<String>) {
    ui.label(label);
    ui.add(egui::Label::new(value.into()).wrap());
    ui.end_row();
}

fn dash<T: std::fmt::Display>(value: Option<T>) -> String {
    value.map_or_else(|| "-".to_owned(), |value| value.to_string())
}

fn stream_title(stream: &MediaStream, strings: &Strings) -> String {
    let info = stream.info();
    let kind = match stream {
        MediaStream::Video(_) => &strings.stream_video,
        MediaStream::Audio(_) => &strings.stream_audio,
        MediaStream::Subtitle(_) => &strings.stream_subtitle,
        MediaStream::Other(_) => &strings.stream_other,
    };
    format!("{kind} #{} - {}", info.index, codec_label(info))
}

fn codec_label(info: &StreamInfo) -> &str {
    info.codec_name.as_deref().unwrap_or("unknown")
}

fn resolution_text(video: &VideoStream) -> String {
    match (video.width, video.height) {
        (Some(width), Some(height)) => format!("{width}x{height}"),
        _ => "-".to_owned(),
    }
}

fn disposition_text(disposition: &Disposition, strings: &Strings) -> String {
    let mut flags = Vec::new();
    if disposition.default {
        flags.push(strings.flag_default.as_str());
    }
    if disposition.forced {
        flags.push(strings.flag_forced.as_str());
    }
    if disposition.hearing_impaired {
        flags.push(strings.flag_hearing_impaired.as_str());
    }
    if disposition.visual_impaired {
        flags.push(strings.flag_visual_impaired.as_str());
    }
    if disposition.attached_pic {
        flags.push(strings.flag_attached_pic.as_str());
    }

    if flags.is_empty() {
        "-".to_owned()
    } else {
        flags.join(", ")
    }
}

fn centered_message(ui: &mut egui::Ui, message: impl Into<String>) {
    ui.with_layout(
        egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
        |ui| {
            ui.add(egui::Label::new(message.into()).wrap());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct MemoryStorage(std::collections::BTreeMap<String, String>);

    impl eframe::Storage for MemoryStorage {
        fn get_string(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }

        fn set_string(&mut self, key: &str, value: String) {
            self.0.insert(key.to_owned(), value);
        }

        fn remove_string(&mut self, key: &str) {
            self.0.remove(key);
        }

        fn flush(&mut self) {}
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
        };

        for language in Language::ALL {
            settings.language = language;
            let mut storage = MemoryStorage::default();
            eframe::set_value(&mut storage, SETTINGS_KEY, &settings);
            let parsed: Option<UiSettings> = eframe::get_value(&storage, SETTINGS_KEY);

            assert_eq!(parsed, Some(settings));
        }
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
