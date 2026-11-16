//! Vector icons drawn with the egui painter, so no image files are shipped.

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, Vec2, pos2, vec2};
use rdm_core::Category;

#[derive(Clone, Copy)]
pub enum Icon {
    Add,
    Resume,
    Stop,
    StopAll,
    Delete,
    DeleteCompleted,
    Options,
    About,
}

/// Icon colours for one theme: saturated with white symbols on a light
/// window, soft pastel with dark symbols on a dark one.
struct Palette {
    blue: Color32,
    green: Color32,
    orange: Color32,
    red: Color32,
    purple: Color32,
    gray: Color32,
    /// The symbol drawn on top of the coloured shape.
    symbol: Color32,
}

const LIGHT: Palette = Palette {
    blue: Color32::from_rgb(30, 136, 229),
    green: Color32::from_rgb(67, 160, 71),
    orange: Color32::from_rgb(251, 140, 0),
    red: Color32::from_rgb(229, 57, 53),
    purple: Color32::from_rgb(94, 53, 177),
    gray: Color32::from_rgb(96, 110, 125),
    symbol: Color32::WHITE,
};

const DARK: Palette = Palette {
    blue: Color32::from_rgb(100, 181, 246),
    green: Color32::from_rgb(129, 199, 132),
    orange: Color32::from_rgb(255, 183, 77),
    red: Color32::from_rgb(239, 110, 105),
    purple: Color32::from_rgb(179, 157, 219),
    gray: Color32::from_rgb(176, 190, 197),
    symbol: Color32::from_rgb(24, 26, 30),
};

/// Draws `icon` in the colours of the current theme (`dark`).
pub fn paint(painter: &Painter, rect: Rect, icon: Icon, enabled: bool, dark: bool) {
    let p = if dark { &DARK } else { &LIGHT };
    let c = rect.center();
    let r = rect.width().min(rect.height()) * 0.45;
    let tint = |color: Color32| {
        if enabled {
            color
        } else {
            color.gamma_multiply(0.35)
        }
    };
    let line = Stroke::new(r * 0.22, p.symbol);
    match icon {
        Icon::Add => {
            painter.circle_filled(c, r, tint(p.blue));
            painter.line_segment([c - vec2(r * 0.5, 0.0), c + vec2(r * 0.5, 0.0)], line);
            painter.line_segment([c - vec2(0.0, r * 0.5), c + vec2(0.0, r * 0.5)], line);
        }
        Icon::Resume => {
            painter.circle_filled(c, r, tint(p.green));
            let pts = vec![
                c + vec2(-r * 0.3, -r * 0.45),
                c + vec2(r * 0.5, 0.0),
                c + vec2(-r * 0.3, r * 0.45),
            ];
            painter.add(Shape::convex_polygon(pts, p.symbol, Stroke::NONE));
        }
        Icon::Stop => {
            painter.circle_filled(c, r, tint(p.orange));
            let bar = vec2(r * 0.22, r * 0.9);
            for dx in [-r * 0.22, r * 0.22] {
                painter.rect_filled(
                    Rect::from_center_size(c + vec2(dx, 0.0), bar),
                    1.0,
                    p.symbol,
                );
            }
        }
        Icon::StopAll => {
            painter.circle_filled(c, r, tint(p.red));
            painter.rect_filled(
                Rect::from_center_size(c, Vec2::splat(r * 0.8)),
                2.0,
                p.symbol,
            );
        }
        Icon::Delete | Icon::DeleteCompleted => {
            let color = tint(p.gray);
            let body = Rect::from_min_max(c + vec2(-r * 0.6, -r * 0.5), c + vec2(r * 0.6, r));
            painter.rect_filled(body, r * 0.15, color);
            painter.rect_filled(
                Rect::from_min_max(c + vec2(-r * 0.85, -r * 0.8), c + vec2(r * 0.85, -r * 0.6)),
                1.0,
                color,
            );
            painter.rect_filled(
                Rect::from_min_max(c + vec2(-r * 0.25, -r), c + vec2(r * 0.25, -r * 0.8)),
                1.0,
                color,
            );
            if matches!(icon, Icon::DeleteCompleted) {
                let badge = c + vec2(r * 0.55, r * 0.55);
                painter.circle_filled(badge, r * 0.42, tint(p.green));
                let check = Stroke::new(r * 0.14, p.symbol);
                let s = r * 0.2;
                painter.line_segment(
                    [badge + vec2(-s, 0.0), badge + vec2(-s * 0.2, s * 0.8)],
                    check,
                );
                painter.line_segment(
                    [
                        badge + vec2(-s * 0.2, s * 0.8),
                        badge + vec2(s * 1.1, -s * 0.8),
                    ],
                    check,
                );
            } else {
                for dx in [-r * 0.25, 0.0, r * 0.25] {
                    painter.line_segment(
                        [c + vec2(dx, -r * 0.25), c + vec2(dx, r * 0.75)],
                        Stroke::new(r * 0.08, p.symbol),
                    );
                }
            }
        }
        Icon::Options => {
            let color = tint(p.gray);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::TAU / 8.0;
                let dir = vec2(a.cos(), a.sin());
                painter.line_segment(
                    [c + dir * r * 0.5, c + dir * r * 0.95],
                    Stroke::new(r * 0.32, color),
                );
            }
            painter.circle_filled(c, r * 0.68, color);
            painter.circle_filled(c, r * 0.28, p.symbol);
        }
        Icon::About => {
            painter.circle_filled(c, r, tint(p.purple));
            painter.circle_filled(c + vec2(0.0, -r * 0.45), r * 0.13, p.symbol);
            painter.line_segment([c + vec2(0.0, -r * 0.15), c + vec2(0.0, r * 0.55)], line);
        }
    }
}

pub fn category_color(category: Category) -> Color32 {
    match category {
        Category::Compressed => Color32::from_rgb(255, 160, 0),
        Category::Documents => Color32::from_rgb(30, 136, 229),
        Category::Music => Color32::from_rgb(216, 27, 96),
        Category::Programs => Color32::from_rgb(94, 53, 177),
        Category::Video => Color32::from_rgb(229, 57, 53),
        Category::Other => Color32::from_rgb(120, 130, 140),
    }
}

/// A small "file" glyph with a folded corner in the category color.
pub fn paint_file(painter: &Painter, rect: Rect, color: Color32) {
    let h = rect.height() * 0.8;
    let w = h * 0.75;
    let min = rect.center() - vec2(w, h) / 2.0;
    let fold = w * 0.35;
    let pts = vec![
        min,
        min + vec2(w - fold, 0.0),
        min + vec2(w, fold),
        min + vec2(w, h),
        min + vec2(0.0, h),
    ];
    painter.add(Shape::convex_polygon(pts, color, Stroke::NONE));
    painter.add(Shape::convex_polygon(
        vec![
            min + vec2(w - fold, 0.0),
            min + vec2(w - fold, fold),
            min + vec2(w, fold),
        ],
        color.gamma_multiply(0.6),
        Stroke::NONE,
    ));
}

/// The window icon: a white download arrow on a blue rounded square.
pub fn app_icon() -> egui::IconData {
    const SIZE: u32 = 64;
    let s = SIZE as f32;
    let mut rgba = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let p = pos2(x as f32 + 0.5, y as f32 + 0.5);
            let bg = rounded_square_coverage(p, s, 12.0);
            let fg = arrow_coverage(p, s);
            let t = y as f32 / s;
            let blue = [
                lerp(66.0, 21.0, t),
                lerp(165.0, 101.0, t),
                lerp(245.0, 192.0, t),
            ];
            let mix = |c: f32| (c * (1.0 - fg) + 255.0 * fg) as u8;
            rgba.extend_from_slice(&[mix(blue[0]), mix(blue[1]), mix(blue[2]), (bg * 255.0) as u8]);
        }
    }
    egui::IconData {
        rgba,
        width: SIZE,
        height: SIZE,
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn rounded_square_coverage(p: Pos2, size: f32, radius: f32) -> f32 {
    let half = size / 2.0 - 1.0;
    let q = (p - pos2(size / 2.0, size / 2.0)).abs() - Vec2::splat(half - radius);
    let outside = q.max(Vec2::ZERO).length() + q.x.max(q.y).min(0.0) - radius;
    (0.5 - outside).clamp(0.0, 1.0)
}

fn arrow_coverage(p: Pos2, s: f32) -> f32 {
    let (x, y) = (p.x / s, p.y / s);
    let shaft = (0.42..0.58).contains(&x) && (0.16..0.52).contains(&y);
    let head = (0.46..=0.72).contains(&y) && (x - 0.5).abs() <= (0.72 - y) * 0.95;
    let base = (0.24..0.76).contains(&x) && (0.78..0.86).contains(&y);
    if shaft || head || base { 1.0 } else { 0.0 }
}
