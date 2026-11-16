//! All popup windows. Each dialog is a small struct with a `show` method that
//! returns whether it stays open; actions go back to the app as [`Command`]s.

mod about;
mod complete;
mod delete;
mod file_info;
mod options;
mod progress;

use egui::{Align2, Context, Id};
use rdm_core::{Download, DownloadId, Engine, RequestInfo, Settings};

use crate::app::Command;
use about::{show_about, show_credits};
use complete::show_complete;
use delete::DeleteConfirm;
use file_info::{AddUrl, FileInfo};
use options::OptionsDialog;
use progress::Progress;

#[derive(Default)]
pub struct Dialogs {
    add_url: Option<AddUrl>,
    file_info: Vec<FileInfo>,
    progress: Vec<Progress>,
    pub complete: Vec<DownloadId>,
    delete: Option<DeleteConfirm>,
    options: Option<OptionsDialog>,
    pub about: bool,
    pub credits: bool,
    next_id: u64,
}

impl Dialogs {
    /// No dialog is waiting for the user.
    pub fn is_empty(&self) -> bool {
        self.add_url.is_none() && self.file_info.is_empty()
    }

    pub fn open_add_url(&mut self) {
        self.add_url = Some(AddUrl::new());
    }

    /// "Download File Info": asks the server about the file, then lets the
    /// user pick name and folder.
    pub fn open_file_info(
        &mut self,
        engine: &Engine,
        url: String,
        request: RequestInfo,
        name: Option<String>,
    ) {
        self.next_id += 1;
        self.file_info
            .push(FileInfo::new(self.next_id, engine, url, request, name));
    }

    pub fn show_progress(&mut self, id: DownloadId) {
        if !self.progress.iter().any(|p| p.id == id) {
            self.progress.push(Progress::new(id));
        }
    }

    pub fn close_progress(&mut self, id: DownloadId) {
        self.progress.retain(|p| p.id != id);
    }

    pub fn confirm_delete(&mut self, ids: Vec<DownloadId>) {
        self.delete = Some(DeleteConfirm::new(ids));
    }

    pub fn open_options(&mut self, settings: Settings) {
        self.options = Some(OptionsDialog::new(settings));
    }

    pub fn show(&mut self, ctx: &Context, engine: &Engine, downloads: &[Download]) -> Vec<Command> {
        let mut commands = Vec::new();

        if let Some(dialog) = &mut self.add_url {
            match dialog.show(ctx) {
                Some(Some(url)) => {
                    self.add_url = None;
                    self.open_file_info(engine, url, RequestInfo::default(), None);
                }
                Some(None) => self.add_url = None,
                None => {}
            }
        }

        self.file_info.retain_mut(|dialog| dialog.show(ctx, engine));

        self.progress.retain_mut(
            |dialog| match downloads.iter().find(|d| d.id == dialog.id) {
                Some(d) => dialog.show(ctx, d, &mut commands),
                None => false,
            },
        );

        self.complete
            .retain(|&id| match downloads.iter().find(|d| d.id == id) {
                Some(d) => show_complete(ctx, engine, d, &mut commands),
                None => false,
            });

        if let Some(dialog) = &mut self.delete
            && !dialog.show(ctx, engine)
        {
            self.delete = None;
        }

        if let Some(dialog) = &mut self.options
            && !dialog.show(ctx, engine)
        {
            self.options = None;
        }

        if self.about {
            self.about = show_about(ctx, &mut self.credits);
        }
        if self.credits {
            self.credits = show_credits(ctx);
        }
        commands
    }
}

/// A fixed size window centered in the main window. `offset` staggers
/// several windows of the same kind so they do not cover each other.
fn window<'a>(ctx: &Context, title: String, id: Id, offset: u64) -> egui::Window<'a> {
    let shift = (offset % 8) as f32 * 24.0;
    egui::Window::new(title)
        .id(id)
        .collapsible(false)
        .resizable(false)
        .pivot(Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center() + egui::vec2(shift, shift))
}

/// Right aligned row of dialog buttons.
fn button_row(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(6.0);
    let size = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
    ui.allocate_ui_with_layout(size, egui::Layout::right_to_left(egui::Align::Center), add);
}
