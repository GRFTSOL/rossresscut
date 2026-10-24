//! egui/eframe desktop layer.
//!
//! The UI renders state and forwards user intent to the application layer. It
//! never runs FFprobe, parses FFprobe output, or mutates domain state directly.
//!
//! The layout follows the LosslessCut direction recorded in `GOALS.md`: a top
//! toolbar, a central preview, a bottom timeline, and a side panel for media
//! information.

use std::path::PathBuf;
use std::time::Duration;

use eframe::egui;

use crate::application::{Application, OpenState};
use crate::domain::media::{
    AudioStream, Disposition, MediaFile, MediaStream, StreamInfo, VideoStream,
};
use crate::domain::time::Timestamp;

/// Main application window.
#[derive(Default)]
pub struct RossresscutApp {
    application: Application,
    path_input: String,
    preview_texture: Option<egui::TextureHandle>,
    preview_texture_timestamp: Option<i64>,
    preview_source: Option<PathBuf>,
    scrub_seconds: Option<f64>,
}

impl RossresscutApp {
    /// Creates the window state and optionally opens `initial_path`.
    pub fn new(initial_path: Option<PathBuf>) -> Self {
        let mut app = Self::default();
        if let Some(path) = initial_path {
            app.path_input = path.display().to_string();
            app.application.open_media(path);
        }
        app
    }

    fn sync_preview_texture(&mut self, context: &egui::Context) {
        if !matches!(self.application.state(), OpenState::Ready) {
            self.preview_texture = None;
            self.preview_texture_timestamp = None;
            self.preview_source = None;
            return;
        }

        let source = self.application.media().map(|media| media.path.clone());
        if source != self.preview_source {
            self.preview_texture = None;
            self.preview_texture_timestamp = None;
            self.preview_source = source;
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
        self.preview_texture_timestamp = Some(frame.timestamp.micros());
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("media_toolbar").show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let field_width = (ui.available_width() - 330.0).max(80.0);
                let response = ui.add_sized(
                    [field_width, 22.0],
                    egui::TextEdit::singleline(&mut self.path_input)
                        .hint_text("/path/to/media.mp4"),
                );
                let submitted =
                    response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
                let has_path = !self.path_input.trim().is_empty();
                let clicked = ui
                    .add_enabled(has_path, egui::Button::new("Open"))
                    .clicked();
                if has_path && (submitted || clicked) {
                    self.scrub_seconds = None;
                    self.application
                        .open_media(PathBuf::from(self.path_input.trim()));
                }

                ui.separator();

                let can_play = self.application.media().is_some();
                let playing = self.application.is_playing();
                let label = if playing { "Pause" } else { "Play" };
                if ui.add_enabled(can_play, egui::Button::new(label)).clicked() {
                    self.application.toggle_playback();
                }

                let shown_position = self
                    .scrub_seconds
                    .map(seconds_to_timestamp)
                    .unwrap_or_else(|| self.application.position());
                ui.monospace(format!(
                    "{} / {}",
                    shown_position,
                    dash(self.application.duration())
                ));
            });
            ui.add_space(4.0);
        });
    }

    fn timeline_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("media_timeline").show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let position = self.application.position();
                if let Some(duration) = self.application.duration() {
                    let max_seconds = duration.micros() as f64 / 1_000_000.0;
                    if max_seconds > 0.0 {
                        let mut value = self
                            .scrub_seconds
                            .unwrap_or_else(|| position.micros() as f64 / 1_000_000.0)
                            .clamp(0.0, max_seconds);
                        let width = (ui.available_width() - 230.0).max(80.0);
                        ui.spacing_mut().slider_width = width;
                        let response = ui.add(
                            egui::Slider::new(&mut value, 0.0..=max_seconds)
                                .show_value(false)
                                .trailing_fill(true),
                        );

                        if response.drag_stopped() {
                            self.application.seek(seconds_to_timestamp(value));
                            self.scrub_seconds = None;
                        } else if response.dragged() {
                            self.scrub_seconds = Some(value);
                        } else if response.changed() {
                            self.application.seek(seconds_to_timestamp(value));
                        }
                    }

                    let shown = self
                        .scrub_seconds
                        .map(seconds_to_timestamp)
                        .unwrap_or(position);
                    ui.monospace(format!("{shown} / {duration}"));
                } else {
                    ui.label("Duration unavailable");
                    ui.monospace(position.to_string());
                }
            });

            if let Some(warning) = self.application.warning() {
                ui.colored_label(ui.visuals().warn_fg_color, warning);
            }
            ui.add_space(4.0);
        });
    }

    fn info_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::right("media_information")
            .resizable(true)
            .default_size(340.0)
            .show(ui, |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.heading("Media");
                    ui.separator();

                    match self.application.state() {
                        OpenState::Empty => {
                            ui.label("No media loaded.");
                        }
                        OpenState::Probing { path } => {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label("Reading media information...");
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
                                render_media_info(ui, media);
                            }
                        }
                    }
                });
            });
    }

    fn preview_panel(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.painter()
                .rect_filled(ui.max_rect(), 0.0, egui::Color32::BLACK);

            match self.application.state() {
                OpenState::Empty => {
                    centered_message(ui, "Open a media file to preview.");
                }
                OpenState::Probing { path } => {
                    ui.with_layout(
                        egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                        |ui| {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(format!("Probing {}", path.display()));
                            });
                        },
                    );
                }
                OpenState::Failed { path, message } => {
                    centered_message(ui, format!("Could not open {}\n{message}", path.display()));
                }
                OpenState::Ready => {
                    if let Some(texture) = &self.preview_texture {
                        let size = fit_size(texture.size_vec2(), ui.available_size());
                        ui.with_layout(
                            egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                            |ui| {
                                ui.image(egui::load::SizedTexture::new(texture.id(), size));
                            },
                        );
                    } else if self.application.is_waiting_for_preview() {
                        ui.with_layout(
                            egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                            |ui| {
                                ui.horizontal(|ui| {
                                    ui.spinner();
                                    ui.label("Loading preview...");
                                });
                            },
                        );
                    } else {
                        centered_message(ui, "No video frame available.");
                    }
                }
            }
        });
    }
}

impl eframe::App for RossresscutApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.application.update() {
            ui.ctx().request_repaint();
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
        } else if matches!(self.application.state(), OpenState::Probing { .. }) {
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }

        self.sync_preview_texture(ui.ctx());
        self.toolbar(ui);
        self.timeline_panel(ui);
        self.info_panel(ui);
        self.preview_panel(ui);
    }
}

/// Starts the native event loop and blocks until the window closes.
///
/// Must be called from the main thread.
pub fn run(initial_path: Option<PathBuf>) -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([960.0, 640.0])
            .with_min_inner_size([480.0, 320.0]),
        ..Default::default()
    };

    eframe::run_native(
        "rossresscut",
        options,
        Box::new(move |_creation_context| Ok(Box::new(RossresscutApp::new(initial_path)))),
    )
}

fn render_media_info(ui: &mut egui::Ui, media: &MediaFile) {
    let file_name = media.path.file_name().map_or_else(
        || media.path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    ui.heading(file_name);
    ui.monospace(media.path.display().to_string());
    ui.separator();

    ui.heading("Format");
    egui::Grid::new("media_format")
        .num_columns(2)
        .min_col_width(115.0)
        .striped(true)
        .show(ui, |ui| {
            row(ui, "Container", media.format.name.clone());
            row(ui, "Long name", dash(media.format.long_name.as_deref()));
            row(ui, "Duration", dash(media.duration));
            row(ui, "Start time", dash(media.start_time));
            row(
                ui,
                "Size",
                dash(media.format.size.map(|size| format!("{size} bytes"))),
            );
            row(
                ui,
                "Bit rate",
                dash(media.format.bit_rate.map(|rate| format!("{rate} bit/s"))),
            );
        });

    ui.separator();
    ui.heading(format!("Streams ({})", media.streams.len()));
    for stream in &media.streams {
        egui::CollapsingHeader::new(stream_title(stream))
            .default_open(true)
            .show(ui, |ui| render_stream_details(ui, stream));
    }

    if !media.chapters.is_empty() {
        ui.separator();
        ui.heading(format!("Chapters ({})", media.chapters.len()));
        egui::Grid::new("media_chapters")
            .num_columns(4)
            .min_col_width(45.0)
            .striped(true)
            .show(ui, |ui| {
                ui.label("ID");
                ui.label("Start");
                ui.label("End");
                ui.label("Title");
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

fn render_stream_details(ui: &mut egui::Ui, stream: &MediaStream) {
    let info = stream.info();

    egui::Grid::new(format!("stream_shared_{}", info.index))
        .num_columns(2)
        .min_col_width(115.0)
        .striped(true)
        .show(ui, |ui| {
            row(ui, "Codec", dash(info.codec_name.as_deref()));
            row(ui, "Codec name", dash(info.codec_long_name.as_deref()));
            row(ui, "Profile", dash(info.profile.as_deref()));
            row(ui, "Language", dash(info.language.as_deref()));
            row(ui, "Title", dash(info.title.as_deref()));
            row(
                ui,
                "Bit rate",
                dash(info.bit_rate.map(|rate| format!("{rate} bit/s"))),
            );
            row(ui, "Time base", dash(info.time_base));
            row(ui, "Start time", dash(info.start_time));
            row(ui, "Duration", dash(info.duration));
            row(ui, "Disposition", disposition_text(&info.disposition));
        });

    match stream {
        MediaStream::Video(video) => render_video_details(ui, video),
        MediaStream::Audio(audio) => render_audio_details(ui, audio),
        MediaStream::Subtitle(_) | MediaStream::Other(_) => {}
    }
}

fn render_video_details(ui: &mut egui::Ui, video: &VideoStream) {
    egui::Grid::new(format!("stream_video_{}", video.info.index))
        .num_columns(2)
        .min_col_width(115.0)
        .striped(true)
        .show(ui, |ui| {
            row(ui, "Resolution", resolution_text(video));
            row(ui, "Pixel format", dash(video.pixel_format.as_deref()));
            row(ui, "Frame rate", dash(video.frame_rate));
            row(ui, "Average frame rate", dash(video.average_frame_rate));
            row(ui, "Color range", dash(video.color_range.as_deref()));
            row(ui, "Color space", dash(video.color_space.as_deref()));
            row(ui, "Color transfer", dash(video.color_transfer.as_deref()));
            row(
                ui,
                "Color primaries",
                dash(video.color_primaries.as_deref()),
            );
        });
}

fn render_audio_details(ui: &mut egui::Ui, audio: &AudioStream) {
    egui::Grid::new(format!("stream_audio_{}", audio.info.index))
        .num_columns(2)
        .min_col_width(115.0)
        .striped(true)
        .show(ui, |ui| {
            row(
                ui,
                "Sample rate",
                dash(audio.sample_rate.map(|rate| format!("{rate} Hz"))),
            );
            row(ui, "Channels", dash(audio.channels));
            row(ui, "Channel layout", dash(audio.channel_layout.as_deref()));
            row(ui, "Sample format", dash(audio.sample_format.as_deref()));
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

fn stream_title(stream: &MediaStream) -> String {
    let info = stream.info();
    let kind = match stream {
        MediaStream::Video(_) => "Video",
        MediaStream::Audio(_) => "Audio",
        MediaStream::Subtitle(_) => "Subtitle",
        MediaStream::Other(_) => "Other",
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

fn disposition_text(disposition: &Disposition) -> String {
    let mut flags = Vec::new();
    if disposition.default {
        flags.push("default");
    }
    if disposition.forced {
        flags.push("forced");
    }
    if disposition.hearing_impaired {
        flags.push("hearing impaired");
    }
    if disposition.visual_impaired {
        flags.push("visually impaired");
    }
    if disposition.attached_pic {
        flags.push("attached picture");
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

fn seconds_to_timestamp(seconds: f64) -> Timestamp {
    if !seconds.is_finite() {
        return Timestamp::ZERO;
    }
    let micros = (seconds.max(0.0) * 1_000_000.0).round();
    Timestamp::from_micros(micros.clamp(0.0, i64::MAX as f64) as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_timeline_seconds_to_microseconds() {
        assert_eq!(seconds_to_timestamp(1.5), Timestamp::from_micros(1_500_000));
        assert_eq!(seconds_to_timestamp(-1.0), Timestamp::ZERO);
        assert_eq!(seconds_to_timestamp(f64::NAN), Timestamp::ZERO);
        assert_eq!(seconds_to_timestamp(0.000_000_5), Timestamp::from_micros(1));
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
}
