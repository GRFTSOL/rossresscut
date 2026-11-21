//! Custom title bar for undecorated windows.
//!
//! The bar mirrors egui's own window title bar: a window-filled strip with a
//! centered title and flat caption buttons. Windows is the only platform where
//! the native decorations are removed; other platforms keep them and the bar is
//! not drawn.

use eframe::egui;

use super::RossresscutApp;

/// Draws the title bar for the current viewport.
///
/// `main_window` adds minimize/maximize buttons and enables double-click to
/// maximize. Dialog viewports only get a close button.
pub(super) fn draw_title_bar(ui: &mut egui::Ui, title: &str, main_window: bool) -> bool {
    #[cfg(target_os = "windows")]
    {
        draw_windows_title_bar(ui, title, main_window)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (ui, title, main_window);
        false
    }
}

/// Height of the merged title/menu bar and of the settings window title bar.
///
/// Windows 10's File Explorer draws its caption at 32 px on a 96 DPI display;
/// matching that height keeps this window visually aligned with the shell.
/// egui points are scaled by the UI zoom, so this follows display scaling too.
#[cfg(target_os = "windows")]
pub(super) const TITLE_BAR_HEIGHT: f32 = 32.0;

/// Frame used by the merged title/menu bar and the settings title bar.
#[cfg(target_os = "windows")]
pub(super) fn bar_frame(style: &egui::Style) -> egui::Frame {
    egui::Frame::window(style)
        .shadow(egui::epaint::Shadow::NONE)
        .corner_radius(egui::CornerRadius::ZERO)
        // No right margin or border: the caption buttons must reach the
        // window corner so the top-right pixel closes the window.
        .inner_margin(egui::Margin {
            left: 8,
            right: 0,
            top: 0,
            bottom: 0,
        })
        .stroke(egui::Stroke::NONE)
}

#[cfg(target_os = "windows")]
fn draw_windows_title_bar(ui: &mut egui::Ui, title: &str, main_window: bool) -> bool {
    let context = ui.ctx().clone();
    if main_window && context.input(|input| input.viewport().fullscreen.unwrap_or(false)) {
        return false;
    }
    let maximized = context.input(|input| input.viewport().maximized.unwrap_or(false));

    let frame = bar_frame(ui.style());

    let mut close_clicked = false;
    egui::Panel::top(("window_title_bar", title))
        .frame(frame)
        .show(ui, |ui| {
            let height = TITLE_BAR_HEIGHT;
            let (bar, _) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), height),
                egui::Sense::hover(),
            );

            // The outer few pixels stay free for the resize border.
            let top_inset = if main_window && !maximized { 5.0 } else { 0.0 };
            let drag_rect = egui::Rect::from_min_max(bar.min + egui::vec2(0.0, top_inset), bar.max);
            let drag = ui.interact(
                drag_rect,
                ui.scope_id().with("window_title_bar_drag"),
                if main_window {
                    egui::Sense::click_and_drag()
                } else {
                    egui::Sense::drag()
                },
            );
            if drag.drag_started() {
                context.send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
            if main_window && drag.double_clicked() {
                context.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
            }

            // Buttons sit along the right edge with a fixed width so all three
            // are exactly the same size regardless of their glyph.
            let button_width = 46.0;
            let mut right = bar.right();
            let close = egui::Rect::from_min_max(
                egui::pos2(right - button_width, bar.top()),
                egui::pos2(right, bar.bottom()),
            );
            right = close.left();
            let maximize = egui::Rect::from_min_max(
                egui::pos2(right - button_width, bar.top()),
                egui::pos2(right, bar.bottom()),
            );
            right = maximize.left();
            let minimize = egui::Rect::from_min_max(
                egui::pos2(right - button_width, bar.top()),
                egui::pos2(right, bar.bottom()),
            );

            if caption_button(ui, close, CaptionButton::Close).clicked() {
                close_clicked = true;
                // The main window must go through the normal close request so
                // unsaved-project handling still runs; dialogs are closed by
                // their caller, which simply stops showing the viewport.
                if main_window {
                    context.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            if main_window {
                if caption_button(
                    ui,
                    maximize,
                    if maximized {
                        CaptionButton::Restore
                    } else {
                        CaptionButton::Maximize
                    },
                )
                .clicked()
                {
                    context.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                }
                if caption_button(ui, minimize, CaptionButton::Minimize).clicked() {
                    context.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                }
            }

            // The title is painted, not laid out, so it stays centered between
            // the window edges like egui's own title bar.
            let font = egui::TextStyle::Heading.resolve(ui.style());
            ui.painter().text(
                bar.center(),
                egui::Align2::CENTER_CENTER,
                title,
                font,
                ui.visuals().strong_text_color(),
            );
        });
    close_clicked
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg(target_os = "windows")]
enum CaptionButton {
    Minimize,
    Maximize,
    Restore,
    Close,
}

#[cfg(target_os = "windows")]
fn caption_button(ui: &mut egui::Ui, rect: egui::Rect, button: CaptionButton) -> egui::Response {
    let response = ui.interact(rect, ui.scope_id().with(button as u8), egui::Sense::click());
    paint_caption(ui, rect, &response, button);
    response
}

/// Adds the minimize/maximize/close buttons to the current right-to-left
/// layout. The caller is responsible for sending the viewport commands.
#[cfg(target_os = "windows")]
pub(super) fn caption_buttons(ui: &mut egui::Ui, height: f32, maximized: bool) {
    use egui::ViewportCommand;

    let buttons = [
        CaptionButton::Close,
        if maximized {
            CaptionButton::Restore
        } else {
            CaptionButton::Maximize
        },
        CaptionButton::Minimize,
    ];
    for button in buttons {
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(46.0, height), egui::Sense::click());
        paint_caption(ui, rect, &response, button);
        if !response.clicked() {
            continue;
        }
        match button {
            CaptionButton::Close => ui.ctx().send_viewport_cmd(ViewportCommand::Close),
            CaptionButton::Minimize => {
                ui.ctx().send_viewport_cmd(ViewportCommand::Minimized(true));
            }
            CaptionButton::Maximize | CaptionButton::Restore => {
                ui.ctx()
                    .send_viewport_cmd(ViewportCommand::Maximized(!maximized));
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn paint_caption(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    response: &egui::Response,
    button: CaptionButton,
) {
    let hovered = response.hovered();
    let close = button == CaptionButton::Close;
    if hovered {
        let fill = if close {
            egui::Color32::from_rgb(232, 17, 35)
        } else {
            ui.visuals().widgets.hovered.weak_bg_fill
        };
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::ZERO, fill);
    }
    let color = if close && hovered {
        egui::Color32::WHITE
    } else {
        ui.visuals().widgets.inactive.fg_stroke.color
    };
    let stroke = egui::Stroke::new(1.0, color);
    let center = rect.center();
    let painter = ui.painter();
    match button {
        CaptionButton::Minimize => {
            painter.line_segment(
                [
                    egui::pos2(center.x - 5.0, center.y),
                    egui::pos2(center.x + 5.0, center.y),
                ],
                stroke,
            );
        }
        CaptionButton::Maximize => {
            painter.rect_stroke(
                egui::Rect::from_center_size(center, egui::vec2(9.0, 9.0)),
                egui::CornerRadius::ZERO,
                stroke,
                egui::StrokeKind::Inside,
            );
        }
        CaptionButton::Restore => {
            let front =
                egui::Rect::from_center_size(center + egui::vec2(1.5, 1.5), egui::vec2(8.0, 8.0));
            let back =
                egui::Rect::from_center_size(center - egui::vec2(1.5, 1.5), egui::vec2(8.0, 8.0));
            painter.rect_stroke(
                back,
                egui::CornerRadius::ZERO,
                stroke,
                egui::StrokeKind::Inside,
            );
            painter.rect_filled(front, egui::CornerRadius::ZERO, ui.visuals().window_fill());
            painter.rect_stroke(
                front,
                egui::CornerRadius::ZERO,
                stroke,
                egui::StrokeKind::Inside,
            );
        }
        CaptionButton::Close => {
            painter.line_segment(
                [
                    egui::pos2(center.x - 5.0, center.y - 5.0),
                    egui::pos2(center.x + 5.0, center.y + 5.0),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    egui::pos2(center.x + 5.0, center.y - 5.0),
                    egui::pos2(center.x - 5.0, center.y + 5.0),
                ],
                stroke,
            );
        }
    }
}

impl RossresscutApp {
    #[cfg(target_os = "windows")]
    pub(super) fn window_resize_edges(&mut self, context: &egui::Context) {
        if context.input(|input| input.viewport().maximized.unwrap_or(false))
            || context.input(|input| input.viewport().fullscreen.unwrap_or(false))
        {
            return;
        }
        let Some(position) = context.input(|input| input.pointer.hover_pos()) else {
            return;
        };
        let screen = context.content_rect();
        if !screen.contains(position) {
            return;
        }
        // Winit removes WS_SIZEBOX for undecorated windows, so the OS no
        // longer offers resize borders; detect the client edges ourselves.
        const BORDER: f32 = 6.0;
        let left = position.x - screen.left() <= BORDER;
        let right = screen.right() - position.x <= BORDER;
        let top = position.y - screen.top() <= BORDER;
        let bottom = screen.bottom() - position.y <= BORDER;
        let direction = match (left, right, top, bottom) {
            (true, _, true, _) => egui::ResizeDirection::NorthWest,
            (_, true, true, _) => egui::ResizeDirection::NorthEast,
            (true, _, _, true) => egui::ResizeDirection::SouthWest,
            (_, true, _, true) => egui::ResizeDirection::SouthEast,
            (true, _, _, _) => egui::ResizeDirection::West,
            (_, true, _, _) => egui::ResizeDirection::East,
            (_, _, true, _) => egui::ResizeDirection::North,
            (_, _, _, true) => egui::ResizeDirection::South,
            _ => return,
        };
        context.set_cursor_icon(match direction {
            egui::ResizeDirection::North | egui::ResizeDirection::South => {
                egui::CursorIcon::ResizeVertical
            }
            egui::ResizeDirection::East | egui::ResizeDirection::West => {
                egui::CursorIcon::ResizeHorizontal
            }
            egui::ResizeDirection::NorthEast | egui::ResizeDirection::SouthWest => {
                egui::CursorIcon::ResizeNeSw
            }
            egui::ResizeDirection::NorthWest | egui::ResizeDirection::SouthEast => {
                egui::CursorIcon::ResizeNwSe
            }
        });
        if context.input(|input| input.pointer.primary_pressed()) {
            context.send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
        }
    }

    #[cfg(not(target_os = "windows"))]
    pub(super) fn window_resize_edges(&mut self, _context: &egui::Context) {}
}
