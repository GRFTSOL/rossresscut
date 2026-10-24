//! egui/eframe desktop layer.
//!
//! The UI renders state and forwards user intent to the application layer. It
//! never runs FFprobe, parses FFprobe output, or mutates domain state directly.

use std::path::PathBuf;
use std::time::Duration;

use eframe::egui;

use crate::application::{Application, OpenState};
use crate::domain::media::{
    AudioStream, Disposition, MediaFile, MediaStream, StreamInfo, VideoStream,
};

/// Main application window.
#[derive(Debug, Default)]
pub struct RossresscutApp {
    application: Application,
    path_input: String,
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

    fn open_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("media_open_bar").show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let button_width = 72.0;
                let field_width =
                    (ui.available_width() - button_width - ui.spacing().item_spacing.x).max(80.0);
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
                    self.application
                        .open_media(PathBuf::from(self.path_input.trim()));
                }
            });
            ui.add_space(4.0);
        });
    }
}

impl eframe::App for RossresscutApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.application.poll() {
            ui.ctx().request_repaint();
        }
        if matches!(self.application.state(), OpenState::Probing { .. }) {
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }

        self.open_bar(ui);

        egui::CentralPanel::default().show(ui, |ui| match self.application.state() {
            OpenState::Empty => {
                ui.label("Choose a media file to inspect.");
            }
            OpenState::Probing { path } => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(format!("Probing {}", path.display()));
                });
            }
            OpenState::Failed { path, message } => {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    format!("Could not open {}", path.display()),
                );
                ui.label(message);
            }
            OpenState::Ready => match self.application.media() {
                Some(media) => render_media(ui, media),
                None => {
                    ui.label("No media is loaded.");
                }
            },
        });
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

fn render_media(ui: &mut egui::Ui, media: &MediaFile) {
    egui::ScrollArea::vertical().show(ui, |ui| {
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
    });
}

fn render_stream_details(ui: &mut egui::Ui, stream: &MediaStream) {
    let info = stream.info();

    egui::Grid::new(format!("stream_shared_{}", info.index))
        .num_columns(2)
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
    ui.label(value.into());
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
