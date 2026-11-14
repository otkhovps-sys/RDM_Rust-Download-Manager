//! Confirmation before removing downloads.

use egui::{Context, Id};
use rdm_core::{DownloadId, Engine};

use super::button_row;

pub struct DeleteConfirm {
    ids: Vec<DownloadId>,
    delete_files: bool,
}

impl DeleteConfirm {
    pub fn new(ids: Vec<DownloadId>) -> Self {
        Self {
            ids,
            delete_files: false,
        }
    }

    pub fn show(&mut self, ctx: &Context, engine: &Engine) -> bool {
        let mut open = true;
        let modal = egui::Modal::new(Id::unique("delete-confirm")).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.heading("Delete downloads");
            ui.label(format!(
                "Remove {} download(s) from the list?",
                self.ids.len()
            ));
            ui.checkbox(
                &mut self.delete_files,
                "Also delete the downloaded files from disk",
            );
            button_row(ui, |ui| {
                if ui.button("No").clicked() {
                    open = false;
                }
                if ui.button("Yes").clicked() {
                    engine.remove(&self.ids, self.delete_files);
                    open = false;
                }
            });
        });
        open && !modal.should_close()
    }
}
