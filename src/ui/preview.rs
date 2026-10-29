//! Preview surface, texture uploads and render diagnostics.

use std::time::{Duration, Instant};

use eframe::egui;

use crate::application::OpenState;

use super::i18n::fill;
use super::panels::{centered_message, file_title};
use super::{FILE_DIALOG_SUPPORTED, RossresscutApp};

impl RossresscutApp {
    pub(super) fn sync_preview_texture(&mut self, context: &egui::Context) {
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

    pub(super) fn preview_panel(&mut self, ui: &mut egui::Ui) {
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
}

/// Temporary render-path diagnostics, enabled by setting `ROSS_RESCUT_DIAG`.
///
/// Phase 4A uses this to compare egui frame cadence with preview frame rate
/// and to measure CPU texture upload cost. The struct is only constructed when
/// the environment variable is present, so the default path stays unchanged.
pub(super) struct RenderDiagnostics {
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
    pub(super) fn from_environment() -> Option<Self> {
        std::env::var_os("ROSS_RESCUT_DIAG").map(|_| Self::default())
    }

    pub(super) fn frame(&mut self) {
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

pub(super) fn fit_size(source: egui::Vec2, available: egui::Vec2) -> egui::Vec2 {
    if source.x <= 0.0 || source.y <= 0.0 || available.x <= 0.0 || available.y <= 0.0 {
        return egui::Vec2::ZERO;
    }
    let scale = (available.x / source.x).min(available.y / source.y);
    source * scale
}
