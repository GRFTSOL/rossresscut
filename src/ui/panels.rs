//! Side panels: playback information, segments and export status.

use std::path::PathBuf;

use eframe::egui;

use crate::application::{
    Application, BatchItemStatus, BatchSkipReason, ExportStage, ExportStatus, OpenState,
};
use crate::domain::media::{MediaFile, MediaStream, VideoStream};
use crate::domain::segment::Segment;
use crate::domain::time::{Duration, Timestamp};

use super::i18n::fill;
use super::icons;
use super::timeline::segment_color;
use super::{FILE_DIALOG_SUPPORTED, RossresscutApp, ShortcutAction, Strings};

/// Tabs in the right-hand information panel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum InfoTab {
    /// Timeline segments, the primary editing view.
    #[default]
    Segments,
    /// Chapter titles derived from enabled segments.
    Chapters,
}

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
        let time = format!("{position} / {duration}");
        let title = self
            .media_title()
            .unwrap_or_else(|| strings.no_media.clone());
        let waveform = self
            .application
            .waveform_progress()
            .map(|progress| (progress.determinate(), progress.fraction()));
        let has_enabled_segment = self
            .application
            .timeline()
            .segments()
            .iter()
            .any(|segment| segment.enabled());
        let exporting = self.application.export_status().is_running();
        let enabled = FILE_DIALOG_SUPPORTED && has_enabled_segment && !exporting;

        let row_height = 30.0;
        let row = egui::Rect::from_min_size(
            ui.cursor().min,
            egui::vec2(ui.available_width(), row_height),
        );
        // The time is painted so it stays exactly on the window centre line,
        // the same axis as the play button above it.
        ui.painter().text(
            row.center(),
            egui::Align2::CENTER_CENTER,
            time,
            egui::TextStyle::Monospace.resolve(ui.style()),
            ui.visuals().text_color(),
        );
        ui.allocate_ui_with_layout(
            row.size(),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.columns(3, |columns| {
                    columns[0].with_layout(
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.add(egui::Label::new(title).truncate());
                            if let Some((determinate, fraction)) = waveform {
                                let bar = if determinate {
                                    egui::ProgressBar::new(fraction)
                                        .desired_width(90.0)
                                        .show_percentage()
                                } else {
                                    egui::ProgressBar::new(0.0)
                                        .desired_width(90.0)
                                        .animate(true)
                                };
                                ui.add(bar);
                            }
                        },
                    );
                    columns[2].with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
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
                        },
                    );
                });
            },
        );
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
        // Keep custom backgrounds and outlines the same size while hovering:
        // egui expands hovered/active widgets by one point by default, which
        // made the selected segment frame visibly grow.
        let widgets = &mut ui.style_mut().visuals.widgets;
        widgets.hovered.expansion = 0.0;
        widgets.active.expansion = 0.0;
        widgets.hovered.bg_stroke = egui::Stroke::NONE;
        widgets.active.bg_stroke = egui::Stroke::NONE;

        let has_media = self.application.media().is_some();
        let has_selection = self.application.selected_segment().is_some();
        let has_pending_cut = self.application.in_point().is_some();
        let total = self
            .application
            .timeline()
            .segments()
            .iter()
            .fold(Duration::ZERO, |total, segment| {
                total.checked_add(segment.duration()).unwrap_or(total)
            });
        ui.horizontal(|ui| {
            if let Some(in_point) = self.application.in_point() {
                ui.label(format!("{}: {in_point}", strings.segment_start));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(format!("{} {total}", strings.total_duration));
            });
        });

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
                    let color = segment_color(
                        segment.id(),
                        segment.enabled(),
                        self.application.is_selected(segment.id()),
                        ui.visuals(),
                    );
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

        // Icon controls sit under the list and are horizontally centered.
        let size = egui::vec2(36.0, 32.0);
        let gap = ui.spacing().item_spacing.x;
        let row_width = 5.0 * size.x + 4.0 * gap;
        let padding = ((ui.available_width() - row_width) * 0.5).max(0.0);
        ui.horizontal(|ui| {
            ui.add_space(padding);

            let (cut_icon, cut_tooltip) = if has_pending_cut {
                (icons::REMOVE_CUT, strings.remove_cut_point.clone())
            } else {
                (
                    icons::ADD_CUT,
                    self.shortcut_tooltip(strings.add_cut_point.as_str(), ShortcutAction::AddCut),
                )
            };
            if ui
                .add_enabled(has_media, egui::Button::new(cut_icon).min_size(size))
                .on_hover_text(cut_tooltip)
                .clicked()
            {
                if has_pending_cut {
                    self.application.clear_markers();
                    self.segment_error = None;
                    self.segment_input_segment = None;
                } else {
                    let position = self.application.position();
                    if let Err(error) = self.application.add_cut_point(position) {
                        self.segment_error = Some(error.to_string());
                    } else {
                        self.segment_error = None;
                        self.segment_input_segment = None;
                    }
                }
            }

            if ui
                .add_enabled(
                    has_selection,
                    egui::Button::new(icons::DELETE_SEGMENT).min_size(size),
                )
                .on_hover_text(self.shortcut_tooltip(
                    strings.delete_segment.as_str(),
                    ShortcutAction::DeleteSegment,
                ))
                .clicked()
            {
                self.application.remove_selected_segment();
                self.segment_error = None;
                self.segment_input_segment = None;
            }

            if ui
                .add_enabled(
                    has_selection,
                    egui::Button::new(icons::MOVE_UP).min_size(size),
                )
                .on_hover_text(strings.move_up.as_str())
                .clicked()
            {
                self.application.move_selected_segment_up();
            }

            if ui
                .add_enabled(
                    has_selection,
                    egui::Button::new(icons::MOVE_DOWN).min_size(size),
                )
                .on_hover_text(strings.move_down.as_str())
                .clicked()
            {
                self.application.move_selected_segment_down();
            }

            if ui
                .add_enabled(
                    has_selection,
                    egui::Button::new(icons::SPLIT).min_size(size),
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

        if let Some(error) = &self.segment_error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
    }

    fn chapters_panel(&mut self, ui: &mut egui::Ui, strings: &'static Strings) {
        let enabled: Vec<Segment> = self
            .application
            .timeline()
            .segments()
            .iter()
            .copied()
            .filter(|segment| segment.enabled())
            .collect();

        if enabled.is_empty() {
            ui.label(strings.chapters_empty.as_str());
            return;
        }

        let mut output_position = Timestamp::ZERO;
        for (index, segment) in enabled.iter().enumerate() {
            let start = output_position;
            let end = start.checked_add(segment.duration()).unwrap_or(start);
            output_position = end;

            let id = segment.id();
            let default_title = format!("Segment {}", index + 1);
            let mut title = self
                .application
                .chapter_title(id)
                .unwrap_or_default()
                .to_owned();

            ui.horizontal(|ui| {
                ui.label(format!("#{}", index + 1));
                ui.monospace(format!("{start} – {end}"));
            });
            let changed = ui
                .add(
                    egui::TextEdit::singleline(&mut title)
                        .hint_text(default_title)
                        .desired_width(f32::INFINITY),
                )
                .changed();
            if changed {
                self.application.set_chapter_title(id, title);
            }
        }
        ui.label(egui::RichText::new(strings.chapter_hint.as_str()).weak());
    }

    pub(super) fn info_panel(&mut self, ui: &mut egui::Ui) {
        let strings = self.strings();
        egui::Panel::right("media_information")
            .resizable(true)
            .default_size(360.0)
            .show(ui, |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    if self.batch_panel_open {
                        self.batch_panel(ui, strings);
                        ui.separator();
                    }

                    if self.playback_info_open {
                        ui.heading(strings.playback.as_str());
                        render_playback_info(ui, &self.application, strings);
                        ui.separator();
                    }

                    let segment_label = fill(
                        &strings.segments,
                        &[&self.application.timeline().len().to_string()],
                    );
                    // The tab row is centred in the panel. The labels are sized
                    // by the current font, so they are measured instead of
                    // offset by a constant that would drift with zoom.
                    let tab_size = |ui: &egui::Ui, text: &str| -> f32 {
                        let font = egui::TextStyle::Button.resolve(ui.style());
                        let width = ui
                            .painter()
                            .layout_no_wrap(text.to_owned(), font, egui::Color32::WHITE)
                            .size()
                            .x;
                        width + ui.spacing().button_padding.x * 2.0
                    };
                    let tabs_width = tab_size(ui, &segment_label)
                        + ui.spacing().item_spacing.x
                        + tab_size(ui, strings.chapters.as_str());
                    let offset = ((ui.available_width() - tabs_width) * 0.5).max(0.0);
                    // Leave the same gap above the tabs as the separator leaves
                    // below them, so the selected background sits centred between
                    // the panel edge and the divider.
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.add_space(offset);
                        ui.selectable_value(&mut self.info_tab, InfoTab::Segments, segment_label);
                        ui.selectable_value(
                            &mut self.info_tab,
                            InfoTab::Chapters,
                            strings.chapters.as_str(),
                        );
                    });
                    ui.separator();
                    match self.info_tab {
                        InfoTab::Segments => self.segments_panel(ui, strings),
                        InfoTab::Chapters => self.chapters_panel(ui, strings),
                    }
                });
            });
    }

    /// Returns the output directory for batch exports.
    ///
    /// `None` means each file is written next to its source.
    pub(super) fn batch_output_directory(&self) -> Option<PathBuf> {
        if self.settings.export_to_source_dir || self.settings.export_directory.is_empty() {
            return None;
        }
        Some(PathBuf::from(&self.settings.export_directory))
    }

    /// Returns the enabled segments of the current timeline.
    pub(super) fn enabled_segments(&self) -> Vec<(Timestamp, Timestamp)> {
        self.application
            .timeline()
            .segments()
            .iter()
            .filter(|segment| segment.enabled())
            .map(|segment| (segment.start(), segment.end()))
            .collect()
    }

    /// Starts a merge of `files`, sorted by file name.
    pub(super) fn start_merge(&mut self, mut files: Vec<PathBuf>) -> bool {
        files.sort_by(|left, right| left.file_name().cmp(&right.file_name()));
        let Some(output) = self.merge_output_path(&files) else {
            return false;
        };

        let segments = self.enabled_segments();
        self.application.queue_batch(files.clone());
        self.batch_panel_open = true;
        self.application
            .start_batch(crate::application::BatchRequest {
                files,
                segments,
                output_directory: self.batch_output_directory(),
                // Chapters of separate files cannot be concatenated reliably.
                chapters: false,
                title: Some(self.settings.export_title.clone()),
                merge_output: Some(output),
            })
    }

    fn merge_output_path(&self, files: &[PathBuf]) -> Option<PathBuf> {
        let first = files.first()?;
        let stem = first.file_stem()?.to_string_lossy();
        let extension = first.extension()?.to_string_lossy();
        let name = format!("{stem}-merged.{extension}");
        match self.batch_output_directory() {
            Some(directory) => Some(directory.join(name)),
            None => first.parent().map(|parent| parent.join(name)),
        }
    }

    /// Renders the batch queue and its controls.
    pub(super) fn batch_panel(&mut self, ui: &mut egui::Ui, strings: &'static Strings) {
        ui.horizontal(|ui| {
            ui.heading(strings.batch_queue.as_str());
            ui.label(format!("({})", self.application.batch_items().len()));
            if ui
                .add_enabled(
                    FILE_DIALOG_SUPPORTED && !self.application.is_batching(),
                    egui::Button::new(strings.batch_add_files.as_str()),
                )
                .on_hover_text(strings.batch_add_files.as_str())
                .clicked()
            {
                self.start_batch_file_dialog();
            }
        });

        ui.label(egui::RichText::new(strings.batch_hint.as_str()).weak());

        if self.application.batch_items().is_empty() {
            ui.label(strings.batch_empty.as_str());
            return;
        }

        egui::ScrollArea::vertical()
            .max_height(160.0)
            .id_salt("batch_items")
            .show(ui, |ui| {
                for item in self.application.batch_items() {
                    let status = match item.status() {
                        BatchItemStatus::Pending => strings.batch_pending.clone(),
                        BatchItemStatus::Running => strings.batch_running.clone(),
                        BatchItemStatus::Completed { output } => {
                            fill(&strings.batch_completed, &[&file_title(output)])
                        }
                        BatchItemStatus::Failed { message } => {
                            fill(&strings.batch_failed, &[message.as_str()])
                        }
                        BatchItemStatus::Skipped { reason } => {
                            let reason = match reason {
                                BatchSkipReason::NoDuration => {
                                    strings.batch_skipped_no_duration.clone()
                                }
                                BatchSkipReason::NoSegments => {
                                    strings.batch_skipped_no_segments.clone()
                                }
                                BatchSkipReason::Cancelled => {
                                    strings.batch_skipped_cancelled.clone()
                                }
                            };
                            fill(&strings.batch_skipped, &[reason.as_str()])
                        }
                    };
                    ui.horizontal(|ui| {
                        ui.add(egui::Label::new(file_title(item.path())).truncate());
                        ui.add(egui::Label::new(status).truncate());
                    });
                }
            });

        let running = self.application.is_batching();
        let segments = self.enabled_segments();
        if segments.is_empty() {
            ui.label(strings.batch_no_segments.as_str());
        }

        ui.horizontal(|ui| {
            let can_start = !running && !self.application.batch_items().is_empty();
            if ui
                .add_enabled(can_start, egui::Button::new(strings.batch_start.as_str()))
                .on_disabled_hover_text(strings.batch_empty.as_str())
                .clicked()
            {
                let files: Vec<PathBuf> = self
                    .application
                    .batch_items()
                    .iter()
                    .map(|item| item.path().to_path_buf())
                    .collect();
                self.application
                    .start_batch(crate::application::BatchRequest {
                        files,
                        segments,
                        output_directory: self.batch_output_directory(),
                        chapters: self.settings.export_chapters,
                        title: Some(self.settings.export_title.clone()),
                        merge_output: None,
                    });
            }
            if ui
                .add_enabled(running, egui::Button::new(strings.batch_cancel.as_str()))
                .clicked()
            {
                self.application.cancel_batch();
            }
            if ui
                .add_enabled(!running, egui::Button::new(strings.batch_clear.as_str()))
                .clicked()
            {
                self.application.clear_batch();
            }
        });

        if let Some(merge) = self.application.batch_merge() {
            match &merge.error {
                None => {
                    ui.label(fill(&strings.batch_merged, &[&file_title(&merge.output)]));
                }
                Some(message) => {
                    ui.colored_label(
                        ui.visuals().error_fg_color,
                        fill(&strings.batch_merge_failed, &[message.as_str()]),
                    );
                }
            }
        }
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
