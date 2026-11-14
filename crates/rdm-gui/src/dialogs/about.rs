use egui::{Context, Id, RichText};

use super::{button_row, window};

pub fn show_about(ctx: &Context) -> bool {
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
        button_row(ui, |ui| open = !ui.button("Close").clicked());
    });
    open
}
