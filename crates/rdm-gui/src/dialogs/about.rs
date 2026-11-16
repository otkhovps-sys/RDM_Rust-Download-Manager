use egui::{Context, Id, RichText};

use super::{button_row, window};

/// The credits are kept in one place: `CREDITS.md` at the repository root.
const CREDITS: &str = include_str!("../../../../CREDITS.md");

pub fn show_about(ctx: &Context, credits: &mut bool) -> bool {
    let mut open = true;
    window(ctx, "About RDM".into(), Id::unique("about"), 0).show(ctx, |ui| {
        ui.set_width(340.0);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new("RDM - Rust Download Manager")
                    .heading()
                    .strong(),
            );
            ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
            ui.add_space(6.0);
            ui.label("Fast multi-connection download manager.");
            ui.label("Free and open source software (MIT license).");
            ui.hyperlink(env!("CARGO_PKG_REPOSITORY"));
        });
        button_row(ui, |ui| {
            open = !ui.button("Close").clicked();
            if ui.button("Credits").clicked() {
                *credits = true;
            }
        });
    });
    open
}

/// Shows `CREDITS.md`: `#` lines are headings, `- name - https://...` lines
/// become a name with a link.
pub fn show_credits(ctx: &Context) -> bool {
    let mut open = true;
    window(ctx, "Credits".into(), Id::unique("credits"), 1).show(ctx, |ui| {
        ui.set_width(460.0);
        egui::ScrollArea::vertical()
            .max_height(360.0)
            .show(ui, |ui| {
                for line in CREDITS.lines() {
                    let line = line.trim();
                    if let Some(heading) = line.strip_prefix('#') {
                        ui.add_space(6.0);
                        ui.label(RichText::new(heading.trim_start_matches('#').trim()).strong());
                    } else if let Some((text, url)) = line.rsplit_once(" - https://") {
                        ui.horizontal(|ui| {
                            ui.label(text.trim_start_matches("- "));
                            ui.hyperlink_to("link", format!("https://{url}"));
                        });
                    } else if !line.is_empty() {
                        ui.label(line);
                    }
                }
            });
        button_row(ui, |ui| open = !ui.button("Close").clicked());
    });
    open
}
