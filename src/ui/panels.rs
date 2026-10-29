//! Side panels: playback information, segments and export status.

use eframe::egui;

use crate::application::{Application, ExportStage, ExportStatus, OpenState};
use crate::domain::media::{MediaFile, MediaStream, VideoStream};
use crate::domain::segment::Segment;

use super::i18n::fill;
use super::timeline::segment_color;
use super::{FILE_DIALOG_SUPPORTED, RossresscutApp, ShortcutAction, Strings};

impl RossresscutApp {
    pub(super) fn media_title(&self) -> Option<String> {
        let path = match self.application.state() {
            OpenState::Empty => None,
            OpenState::Probing { path } | OpenState::Failed { path, .. } => Some(path),
            OpenState::Ready => self.application.media().map(|media| &media.path),
        }?;
        path.file_stem()
            .or_else(|| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
    }

    pub(super) fn export_row(&mut self, ui: &mut egui::Ui, strings: &'static Strings) {
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
                let enabled = FILE_DIALOG_SUPPORTED && has_enabled_segment && !exporting;
                if ui
                    .add_enabled(
                        enabled,
                        egui::Button::new(format!("{}…", strings.export))
                            .min_size(egui::vec2(120.0, 30.0)),
                    )
                    .on_hover_text(strings.export_segment.as_str())
                    .clicked()
                {
                    self.export_panel_open = true;
                }
            });
        });
    }

    pub(super) fn export_status(&mut self, ui: &mut egui::Ui) {
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

                    let selected = self.application.is_selected(segment.id());
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
                        let additive =
                            ui.input(|input| input.modifiers.command || input.modifiers.ctrl);
                        if additive {
                            self.application.toggle_segment_selection(segment.id());
                        } else {
                            self.application.select_segment(Some(segment.id()));
                        }
                        self.application.seek(segment.start());
                    }
                });
            }
        }

        if let Some(error) = &self.segment_error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
    }

    pub(super) fn info_panel(&mut self, ui: &mut egui::Ui) {
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

pub(super) fn render_playback_info(
    ui: &mut egui::Ui,
    application: &Application,
    strings: &Strings,
) {
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

pub(super) fn decoder_label(application: &Application, strings: &Strings) -> String {
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

pub(super) fn frame_summary(application: &Application) -> String {
    match (application.frame_size(), application.frame_timestamp()) {
        (Some((width, height)), Some(timestamp)) => {
            format!("{width}x{height} @ {timestamp}")
        }
        _ => "-".to_owned(),
    }
}

pub(super) fn video_summary(media: Option<&MediaFile>) -> String {
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

pub(super) fn audio_summary(media: Option<&MediaFile>) -> String {
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

pub(super) fn resolution_text(video: &VideoStream) -> String {
    match (video.width, video.height) {
        (Some(width), Some(height)) => format!("{width}x{height}"),
        _ => "-".to_owned(),
    }
}

pub(super) fn row(ui: &mut egui::Ui, label: &str, value: impl Into<String>) {
    ui.label(label);
    ui.add(egui::Label::new(value.into()).wrap());
    ui.end_row();
}

pub(super) fn centered_message(ui: &mut egui::Ui, message: impl Into<String>) {
    ui.with_layout(
        egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
        |ui| {
            ui.add(egui::Label::new(message.into()).wrap().selectable(false));
        },
    );
}

pub(super) fn file_title(path: &std::path::Path) -> String {
    path.file_stem()
        .or_else(|| path.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}
