//! "Download complete" dialog.

use egui::{Context, Id, RichText};
use rdm_core::util::format_bytes;
use rdm_core::{Download, Engine, Settings};

use super::{button_row, window};
use crate::app::Command;

pub fn show_complete(
    ctx: &Context,
    engine: &Engine,
    d: &Download,
    commands: &mut Vec<Command>,
) -> bool {
    let mut open = true;
    window(
        ctx,
        "Download complete".into(),
        Id::unique(("complete", d.id)),
        d.id,
    )
    .show(ctx, |ui| {
        ui.set_width(440.0);
        ui.label(RichText::new(&d.file_name).strong());
        egui::Grid::new("complete-grid")
            .num_columns(2)
            .spacing([12.0, 4.0])
            .show(ui, |ui| {
                ui.label("Downloaded");
                ui.label(d.size.map(format_bytes).unwrap_or_default());
                ui.end_row();
                ui.label("Address");
                ui.add(egui::Label::new(RichText::new(&d.url).weak()).truncate());
                ui.end_row();
                ui.label("Saved as");
                ui.add(egui::Label::new(d.path().display().to_string()).truncate());
                ui.end_row();
            });
        let settings = engine.settings();
        let mut dont_show = !settings.show_complete_dialog;
        ui.add_space(4.0);
        if ui
            .checkbox(&mut dont_show, "Don't show this dialog again")
            .changed()
        {
            engine.set_settings(Settings {
                show_complete_dialog: !dont_show,
                ..settings
            });
        }
        button_row(ui, |ui| {
            if ui.button("Close").clicked() {
                open = false;
            }
            if ui.button("Open folder").clicked() {
                commands.push(Command::OpenFolder(d.id));
                open = false;
            }
            if ui.button(RichText::new("Open").strong()).clicked() {
                commands.push(Command::OpenFile(d.id));
                open = false;
            }
        });
    });
    open
}
