//! Optional picture behind the main window (Options > General).
//!
//! The picture is drawn first, then a veil in the theme's background colour
//! so text stays readable; the panels themselves become transparent.

use std::path::{Path, PathBuf};

use egui::{Color32, Rect, TextureHandle, Theme, Ui, Visuals, pos2};
use rdm_core::Settings;

/// Bigger pictures are scaled down; a window never needs more.
const MAX_SIDE: u32 = 2560;

#[derive(Default)]
pub struct Background {
    /// The file the current state belongs to (also when loading it failed).
    path: Option<PathBuf>,
    texture: Option<TextureHandle>,
    /// Whether the panels are currently see-through.
    transparent: Option<bool>,
}

impl Background {
    /// Call before any panel is added.
    pub fn paint(&mut self, ui: &mut Ui, settings: &Settings) {
        let ctx = ui.ctx().clone();
        if self.path != settings.background_image {
            self.path = settings.background_image.clone();
            self.texture =
                self.path.as_deref().and_then(load).map(|image| {
                    ctx.load_texture("background", image, egui::TextureOptions::LINEAR)
                });
        }

        let active = self.texture.is_some();
        if self.transparent != Some(active) {
            self.transparent = Some(active);
            for (theme, normal) in [
                (Theme::Light, Visuals::light()),
                (Theme::Dark, Visuals::dark()),
            ] {
                let fill = if active {
                    Color32::TRANSPARENT
                } else {
                    normal.panel_fill
                };
                ctx.style_mut_of(theme, |style| style.visuals.panel_fill = fill);
            }
        }
        let Some(texture) = &self.texture else {
            return;
        };

        let screen = ctx.content_rect();
        ui.painter().image(
            texture.id(),
            screen,
            cover(texture.size_vec2(), screen),
            Color32::WHITE,
        );
        let base = if ui.visuals().dark_mode {
            Visuals::dark()
        } else {
            Visuals::light()
        }
        .panel_fill;
        let veil = 1.0 - f32::from(settings.background_strength.min(100)) / 100.0;
        ui.painter()
            .rect_filled(screen, 0.0, base.gamma_multiply(veil));
    }
}

/// Texture coordinates that fill `screen` without distorting the picture.
fn cover(image: egui::Vec2, screen: Rect) -> Rect {
    let (image_ratio, screen_ratio) = (image.x / image.y, screen.width() / screen.height());
    let (w, h) = if image_ratio > screen_ratio {
        (screen_ratio / image_ratio, 1.0) // picture is wider: crop left and right
    } else {
        (1.0, image_ratio / screen_ratio) // picture is taller: crop top and bottom
    };
    Rect::from_center_size(pos2(0.5, 0.5), egui::vec2(w, h))
}

fn load(path: &Path) -> Option<egui::ColorImage> {
    let bytes = std::fs::read(path).ok()?;
    let image = image::load_from_memory(&bytes).ok()?;
    let image = if image.width().max(image.height()) > MAX_SIDE {
        image.thumbnail(MAX_SIDE, MAX_SIDE)
    } else {
        image
    }
    .to_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    Some(egui::ColorImage::from_rgba_unmultiplied(
        size,
        image.as_raw(),
    ))
}
