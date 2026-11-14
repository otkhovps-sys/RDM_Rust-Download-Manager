//! Small custom widgets: toolbar buttons and the segment map.

use egui::{Align2, Color32, FontId, Rect, Response, Sense, Ui, vec2};
use rdm_core::{Download, Status};

use crate::icons::{self, Icon};

/// IDM style toolbar button: big icon with the label underneath.
pub fn tool_button(ui: &mut Ui, icon: Icon, label: &str, enabled: bool) -> Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(vec2(72.0, 58.0), sense);
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let visuals = ui.visuals();
    if enabled && response.hovered() {
        let fill = if response.is_pointer_button_down_on() {
            visuals.widgets.active.weak_bg_fill
        } else {
            visuals.widgets.hovered.weak_bg_fill
        };
        ui.painter().rect_filled(rect, 4.0, fill);
    }
    let icon_rect = Rect::from_center_size(rect.center_top() + vec2(0.0, 20.0), vec2(30.0, 30.0));
    icons::paint(ui.painter(), icon_rect, icon, enabled);
    let text_color = if enabled {
        visuals.text_color()
    } else {
        visuals.weak_text_color()
    };
    ui.painter().text(
        rect.center_bottom() - vec2(0.0, 5.0),
        Align2::CENTER_BOTTOM,
        label,
        FontId::proportional(12.0),
        text_color,
    );
    response
}

/// Horizontal bar showing which parts of the file are already downloaded,
/// one block per connection - like IDM's progress details.
pub fn segment_map(ui: &mut Ui, d: &Download) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 14.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 2.0, ui.visuals().extreme_bg_color);
    let done_color = Color32::from_rgb(30, 136, 229);
    let active_color = Color32::from_rgb(67, 160, 71);

    let x_at =
        |offset: u64, size: u64| rect.left() + rect.width() * (offset as f64 / size as f64) as f32;
    match d.size.filter(|&s| s > 0 && !d.segments.is_empty()) {
        Some(size) => {
            for s in &d.segments {
                let (x0, x1) = (x_at(s.start, size), x_at(s.pos().min(s.end), size));
                let color = if s.active && d.status == Status::Downloading {
                    active_color
                } else {
                    done_color
                };
                painter.rect_filled(
                    Rect::from_x_y_ranges(x0..=x1.max(x0 + 0.5), rect.y_range()),
                    0.0,
                    color,
                );
            }
        }
        None => {
            let fraction = d.progress().unwrap_or(0.0);
            let x1 = rect.left() + rect.width() * fraction;
            painter.rect_filled(
                Rect::from_x_y_ranges(rect.left()..=x1, rect.y_range()),
                2.0,
                done_color,
            );
        }
    }
    painter.rect_stroke(
        rect,
        2.0,
        ui.visuals().widgets.noninteractive.bg_stroke,
        egui::StrokeKind::Inside,
    );
}
