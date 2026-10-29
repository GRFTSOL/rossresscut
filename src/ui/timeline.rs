//! Timeline painting and transport controls.

use eframe::egui;

use crate::domain::segment::SegmentId;
use crate::domain::time::{Duration as MediaDuration, Timestamp};
use crate::domain::waveform::{Peak, Waveform};

use super::dialogs::DialogError;
use super::i18n::fill;
use super::{
    RossresscutApp, SEGMENT_COLORS, ShortcutAction, Strings, TIMELINE_HEIGHT,
    TIMELINE_HEIGHT_AUDIO, WHEEL_LINE_POINTS, WHEEL_PAGE_LINES, WHEEL_STEP_POINTS,
};

impl RossresscutApp {
    pub(super) fn timeline_panel(&mut self, ui: &mut egui::Ui) {
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
                &self.shortcut_tooltip(
                    strings.set_start_from_playhead.as_str(),
                    ShortcutAction::SetSegmentStart,
                ),
            ) {
                self.set_selected_start_from_playhead();
            }

            let time_width = time_field_width(ui);
            let start_response = ui
                .add_enabled(
                    has_selection,
                    egui::TextEdit::singleline(&mut self.segment_start_input)
                        .desired_width(time_width),
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
                    egui::TextEdit::singleline(&mut self.segment_end_input)
                        .desired_width(time_width),
                )
                .on_hover_text(strings.segment_end.as_str());
            if end_response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                self.apply_segment_end_input();
            }

            if symbol_button(
                ui,
                "]",
                has_media && has_selection,
                &self.shortcut_tooltip(
                    strings.set_end_from_playhead.as_str(),
                    ShortcutAction::SetSegmentEnd,
                ),
            ) {
                self.set_selected_end_from_playhead();
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

    fn timeline_bar(&mut self, ui: &mut egui::Ui) {
        let height = if self.application.has_audio() {
            TIMELINE_HEIGHT_AUDIO
        } else {
            TIMELINE_HEIGHT
        };
        let response = ui.allocate_response(
            egui::vec2(ui.available_width(), height),
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

        let mut cache = self.waveform_cache.take().unwrap_or_default();
        if let Some(waveform) = self.application.waveform() {
            // The audio does not always cover the whole container, so the
            // waveform occupies only its own source-time span.
            let left = x_for_time(rect, waveform.start(), duration);
            let right = x_for_time(rect, waveform.end(), duration);
            let area = egui::Rect::from_min_max(
                egui::pos2(left, rect.top()),
                egui::pos2(right, rect.bottom()),
            );
            if area.width() > 0.0 {
                let width = area.width().round().max(1.0) as usize;
                let columns = cache.resolve(waveform, width);
                draw_waveform(
                    &painter,
                    area,
                    columns,
                    visuals.weak_text_color().gamma_multiply(1.15),
                );
            }
        }
        self.waveform_cache = Some(cache);

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
            let color = segment_color(
                segment.id(),
                segment.enabled(),
                self.application.is_selected(segment.id()),
                visuals,
            );
            painter.rect_filled(segment_rect, 2.0, color);
            draw_segment_number(&painter, rect, segment_rect, index + 1, color);
            if self.application.is_selected(segment.id()) {
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
            let additive = ui.input(|input| input.modifiers.command || input.modifiers.ctrl);
            // Clicking an empty part of the timeline only moves the playhead;
            // it must not clear the current selection.
            if let Some(id) = selected {
                if additive {
                    self.application.toggle_segment_selection(id);
                } else {
                    self.application.select_segment(Some(id));
                }
            }
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

    pub(super) fn handle_wheel(&mut self, ui: &egui::Ui) {
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
}

pub(super) fn fraction_of(position: Timestamp, duration: MediaDuration) -> f32 {
    if duration.micros() == 0 {
        return 0.0;
    }
    let fraction = position.micros().max(0) as f64 / duration.micros() as f64;
    fraction.clamp(0.0, 1.0) as f32
}

pub(super) fn x_for_time(rect: egui::Rect, time: Timestamp, duration: MediaDuration) -> f32 {
    rect.left() + rect.width() * fraction_of(time, duration)
}

/// Cached pixel columns for the waveform.
///
/// Re-aggregating the peaks on every repaint would cost more than it saves, so
/// the result is reused until the timeline width or the peak count changes.
#[derive(Default)]
pub(super) struct WaveformCache {
    width: usize,
    peaks: usize,
    start: i64,
    columns: Vec<Peak>,
}

impl WaveformCache {
    /// Returns the columns for `waveform` at `width`, recomputing when needed.
    fn resolve(&mut self, waveform: &Waveform, width: usize) -> &[Peak] {
        if self.width != width
            || self.peaks != waveform.len()
            || self.start != waveform.start().micros()
        {
            self.width = width;
            self.peaks = waveform.len();
            self.start = waveform.start().micros();
            self.columns = waveform.columns(width);
        }
        &self.columns
    }
}

pub(super) fn segment_color(
    id: SegmentId,
    enabled: bool,
    selected: bool,
    visuals: &egui::Visuals,
) -> egui::Color32 {
    let base = if enabled {
        SEGMENT_COLORS[(id.get() % SEGMENT_COLORS.len() as u64) as usize]
    } else {
        visuals.weak_text_color()
    };
    // The waveform sits underneath, so every state stays translucent: selected
    // segments are simply more opaque than unselected ones.
    let alpha = match (enabled, selected) {
        (false, _) => 34,
        (true, true) => 130,
        (true, false) => 64,
    };
    egui::Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), alpha)
}

/// Draws one vertical bar per column, centred on the timeline.
///
/// Silence collapses to the centre line instead of disappearing, which is what
/// makes gaps visible at a glance.
fn draw_waveform(
    painter: &egui::Painter,
    rect: egui::Rect,
    columns: &[Peak],
    color: egui::Color32,
) {
    if columns.is_empty() || rect.width() <= 0.0 {
        return;
    }

    let center = rect.center().y;
    let half = (rect.height() * 0.42).max(1.0);
    let step = rect.width() / columns.len() as f32;
    let stroke = egui::Stroke::new(step.max(1.0), color);
    let scale = f32::from(i16::MAX);

    for (index, peak) in columns.iter().enumerate() {
        let x = rect.left() + step * (index as f32 + 0.5);
        let top = center - half * (f32::from(peak.max()) / scale).clamp(0.0, 1.0);
        let bottom = center - half * (f32::from(peak.min()) / scale).clamp(-1.0, 0.0);
        // Keep a hairline for digital silence so the gap is still visible.
        let bottom = bottom.max(top + 0.75);
        painter.line_segment([egui::pos2(x, top), egui::pos2(x, bottom)], stroke);
    }
}

pub(super) fn draw_segment_number(
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

pub(super) fn draw_marker(painter: &egui::Painter, rect: egui::Rect, x: f32, color: egui::Color32) {
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

pub(super) fn timestamp_at_x(rect: egui::Rect, x: f32, duration: MediaDuration) -> Timestamp {
    if rect.width() <= 0.0 {
        return Timestamp::ZERO;
    }
    let fraction = ((x - rect.left()) / rect.width()).clamp(0.0, 1.0) as f64;
    let micros = (fraction * duration.micros() as f64).round();
    Timestamp::from_micros(micros.clamp(0.0, i64::MAX as f64) as i64)
}

/// Maps accumulated wheel points to playback direction.
///
/// Positive egui scroll points mean the content moves down, which is the
/// gesture users perceive as scrolling forward in this application: next
/// keyframe / later position.
pub(super) fn wheel_direction(steps: f32) -> i64 {
    if steps > 0.0 { 1 } else { -1 }
}

/// Width that fits a full `HH:MM:SS.mmm` timestamp in the current font.
pub(super) fn time_field_width(ui: &egui::Ui) -> f32 {
    let font = egui::TextStyle::Monospace.resolve(ui.style());
    let width = ui
        .painter()
        .layout_no_wrap("00:00:00.000".to_owned(), font, egui::Color32::WHITE)
        .size()
        .x;
    (width + 24.0).max(96.0)
}

pub(super) fn symbol_button(ui: &mut egui::Ui, label: &str, enabled: bool, tooltip: &str) -> bool {
    ui.add_enabled(
        enabled,
        egui::Button::new(label).min_size(egui::vec2(32.0, 24.0)),
    )
    .on_hover_text(tooltip)
    .clicked()
}

/// Parses `HH:MM:SS.mmm` or decimal seconds from a segment input field.
pub(super) fn parse_time_input(text: &str) -> Option<Timestamp> {
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
