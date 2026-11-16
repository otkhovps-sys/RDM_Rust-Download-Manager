//! Small custom widgets: toolbar buttons and the segment map.

use egui::{Color32, FontId, Rect, Response, Sense, Ui, vec2};
use rdm_core::Download;

use crate::icons::{self, Icon};

/// IDM style toolbar button: big icon with the label underneath.
pub fn tool_button(ui: &mut Ui, icon: Icon, label: &str, enabled: bool) -> Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let text_color = if enabled {
        ui.visuals().text_color()
    } else {
        ui.visuals().weak_text_color()
    };
    // Wide enough for the label in whatever font the system provides.
    let text =
        ui.painter()
            .layout_no_wrap(label.to_owned(), FontId::proportional(12.0), text_color);
    let width = (text.size().x + 14.0).max(72.0);
    let (rect, response) = ui.allocate_exact_size(vec2(width, 58.0), sense);
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
    icons::paint(ui.painter(), icon_rect, icon, enabled, visuals.dark_mode);
    let text_pos = rect.center_bottom() - vec2(text.size().x / 2.0, 5.0 + text.size().y);
    ui.painter().galley(text_pos, text, text_color);
    response
}

/// A small rotating arc. Unlike `egui::Spinner`, which redraws the whole
/// window at full frame rate, this one asks for only 10 frames per second -
/// much cheaper on machines that render in software.
pub fn busy_indicator(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
    let t = ui.input(|i| i.time) as f32;
    let start = t * std::f32::consts::TAU;
    let points: Vec<_> = (0..=12)
        .map(|i| {
            let a = start + i as f32 / 12.0 * std::f32::consts::PI * 1.5;
            rect.center() + 6.0 * egui::vec2(a.cos(), a.sin())
        })
        .collect();
    let stroke = egui::Stroke::new(2.0, ui.visuals().strong_text_color());
    ui.painter().add(egui::Shape::line(points, stroke));
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(100));
}

/// The progress bar. For a file fetched over several connections every
/// connection's part fills in on its own, like IDM's progress details.
pub fn segment_map(ui: &mut Ui, d: &Download, color: Color32) {
    const RADIUS: u8 = 6;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 12.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, RADIUS, ui.visuals().extreme_bg_color);

    // Only the ends of the bar are round, so parts in the middle join up.
    let fill = |x0: f32, x1: f32| {
        let round = |at_end: bool| if at_end { RADIUS } else { 0 };
        let (left, right) = (
            round(x0 <= rect.left() + 0.5),
            round(x1 >= rect.right() - 0.5),
        );
        let corners = egui::CornerRadius {
            nw: left,
            sw: left,
            ne: right,
            se: right,
        };
        let part = Rect::from_x_y_ranges(x0..=x1.max(x0 + 0.5), rect.y_range());
        painter.rect_filled(part, corners, color);
    };
    let x_at =
        |offset: u64, size: u64| rect.left() + rect.width() * (offset as f64 / size as f64) as f32;
    match d.size.filter(|&s| s > 0 && !d.segments.is_empty()) {
        Some(size) => {
            for s in &d.segments {
                fill(x_at(s.start, size), x_at(s.pos().min(s.end), size));
            }
        }
        None => {
            let fraction = d.progress().unwrap_or(0.0);
            if fraction > 0.0 {
                fill(rect.left(), rect.left() + rect.width() * fraction);
            }
        }
    }
}
