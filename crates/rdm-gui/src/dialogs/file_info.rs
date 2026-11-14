//! "Enter new address" and "Download File Info" dialogs.

use std::path::PathBuf;

use egui::{Context, Id, RichText};
use rdm_core::util::format_bytes;
use rdm_core::{Category, Engine, Kind, NewDownload, ProbeHandle, ProbeResult, RequestInfo};

use super::{button_row, window};

pub struct AddUrl {
    url: String,
    error: Option<&'static str>,
    focus: bool,
}

impl AddUrl {
    /// Prefills the address from the clipboard, like IDM does.
    pub fn new() -> Self {
        let url = arboard::Clipboard::new()
            .and_then(|mut c| c.get_text())
            .map(|t| t.trim().to_owned())
            .ok()
            .filter(|t| looks_like_url(t) && !t.contains(char::is_whitespace))
            .unwrap_or_default();
        Self {
            url,
            error: None,
            focus: true,
        }
    }

    /// `Some(Some(url))` = OK, `Some(None)` = cancelled, `None` = still open.
    pub fn show(&mut self, ctx: &Context) -> Option<Option<String>> {
        let mut result = None;
        window(
            ctx,
            "Enter new address to download".into(),
            Id::unique("add-url"),
            0,
        )
        .show(ctx, |ui| {
            ui.label("Address:");
            let edit = ui.add(
                egui::TextEdit::singleline(&mut self.url)
                    .desired_width(440.0)
                    .hint_text("https://"),
            );
            if std::mem::take(&mut self.focus) {
                edit.request_focus();
            }
            if let Some(error) = self.error {
                ui.colored_label(egui::Color32::from_rgb(229, 57, 53), error);
            }
            let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            button_row(ui, |ui| {
                if ui.button("Cancel").clicked() {
                    result = Some(None);
                }
                if ui.button("   OK   ").clicked() || enter {
                    let url = self.url.trim();
                    if looks_like_url(url) {
                        result = Some(Some(url.to_owned()));
                    } else {
                        self.error = Some("Please enter a valid http(s) or ftp address.");
                    }
                }
            });
        });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            result = Some(None);
        }
        result
    }
}

fn looks_like_url(text: &str) -> bool {
    ["http://", "https://"]
        .iter()
        .any(|p| text.len() > p.len() && text.to_ascii_lowercase().starts_with(p))
}

enum State {
    Probing(ProbeHandle),
    Failed(String),
    Ready(Box<Form>),
}

struct Form {
    probe: ProbeResult,
    new: NewDownload,
    dir_text: String,
    /// The user picked a folder, so changing the category keeps it.
    custom_dir: bool,
}

pub struct FileInfo {
    id: u64,
    url: String,
    request: RequestInfo,
    suggested_name: Option<String>,
    state: State,
}

impl FileInfo {
    pub fn new(
        id: u64,
        engine: &Engine,
        url: String,
        request: RequestInfo,
        name: Option<String>,
    ) -> Self {
        let state = State::Probing(engine.probe(&url, request.clone()));
        Self {
            id,
            url,
            request,
            suggested_name: name,
            state,
        }
    }

    /// Returns `false` once the dialog is closed.
    pub fn show(&mut self, ctx: &Context, engine: &Engine) -> bool {
        if let State::Probing(handle) = &mut self.state
            && let Some(result) = handle.try_take()
        {
            self.state = match result {
                Ok(probe) => State::Ready(Box::new(self.form(probe, engine))),
                Err(e) => State::Failed(e.to_string()),
            };
        }

        let mut open = true;
        let id = Id::unique(("file-info", self.id));
        window(ctx, "Download File Info".into(), id, self.id).show(ctx, |ui| {
            ui.set_width(480.0);
            match &mut self.state {
                State::Probing(_) => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Getting file information...");
                    });
                    ui.weak(&self.url);
                    ctx.request_repaint_after(std::time::Duration::from_millis(100));
                    button_row(ui, |ui| open &= !ui.button("Cancel").clicked());
                }
                State::Failed(error) => {
                    ui.colored_label(
                        egui::Color32::from_rgb(229, 57, 53),
                        format!("Cannot download: {error}"),
                    );
                    ui.weak(&self.url);
                    let mut retry = false;
                    button_row(ui, |ui| {
                        open &= !ui.button("Close").clicked();
                        retry = ui.button("Try again").clicked();
                    });
                    if retry {
                        self.state = State::Probing(engine.probe(&self.url, self.request.clone()));
                    }
                }
                State::Ready(form) => open &= form.show(ui, engine),
            }
        });
        open
    }

    fn form(&self, probe: ProbeResult, engine: &Engine) -> Form {
        let settings = engine.settings();
        let mut new = NewDownload::from_probe(&probe, self.request.clone(), &settings);
        if let Some(name) = self
            .suggested_name
            .as_deref()
            .map(str::trim)
            .filter(|n| !n.is_empty())
        {
            if probe.kind == Kind::Hls {
                let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
                let ext = probe
                    .file_name
                    .rsplit_once('.')
                    .map_or("ts", |(_, ext)| ext);
                new.file_name = format!("{stem}.{ext}");
            } else if name.contains('.') {
                new.file_name = name.to_owned();
                new.category = Category::from_file_name(name);
                new.save_dir = settings.save_dir_for(new.category);
            }
        }
        let dir_text = new.save_dir.display().to_string();
        Form {
            probe,
            new,
            dir_text,
            custom_dir: false,
        }
    }
}

impl Form {
    fn show(&mut self, ui: &mut egui::Ui, engine: &Engine) -> bool {
        let mut open = true;
        egui::Grid::new("file-info-grid")
            .num_columns(2)
            .spacing([8.0, 6.0])
            .show(ui, |ui| {
                ui.label("URL:");
                ui.add(egui::Label::new(RichText::new(&self.new.url).weak()).truncate());
                ui.end_row();

                ui.label("Category:");
                let before = self.new.category;
                egui::ComboBox::from_id_salt("category")
                    .selected_text(self.new.category.label())
                    .show_ui(ui, |ui| {
                        for category in Category::ALL {
                            ui.selectable_value(&mut self.new.category, category, category.label());
                        }
                    });
                if before != self.new.category && !self.custom_dir {
                    self.new.save_dir = engine.settings().save_dir_for(self.new.category);
                    self.dir_text = self.new.save_dir.display().to_string();
                }
                ui.end_row();

                ui.label("Save to:");
                ui.horizontal(|ui| {
                    if ui
                        .add(egui::TextEdit::singleline(&mut self.dir_text).desired_width(340.0))
                        .changed()
                    {
                        self.custom_dir = true;
                    }
                    if ui.button("Browse...").clicked()
                        && let Some(dir) = rfd::FileDialog::new()
                            .set_directory(&self.dir_text)
                            .pick_folder()
                    {
                        self.dir_text = dir.display().to_string();
                        self.custom_dir = true;
                    }
                });
                ui.end_row();

                ui.label("File name:");
                ui.add(egui::TextEdit::singleline(&mut self.new.file_name).desired_width(400.0));
                ui.end_row();

                ui.label("Description:");
                ui.add(egui::TextEdit::singleline(&mut self.new.description).desired_width(400.0));
                ui.end_row();

                ui.label("Size:");
                ui.label(size_text(&self.probe));
                ui.end_row();

                ui.label("Resume:");
                ui.label(if self.probe.resumable {
                    "Yes - multiple connections"
                } else {
                    "No - single connection"
                });
                ui.end_row();
            });

        let mut add = None;
        button_row(ui, |ui| {
            open &= !ui.button("Cancel").clicked();
            if ui.button("Download Later").clicked() {
                add = Some(false);
            }
            if ui
                .button(RichText::new("Start Download").strong())
                .clicked()
            {
                add = Some(true);
            }
        });
        if let Some(start) = add {
            let mut new = self.new.clone();
            new.save_dir = PathBuf::from(self.dir_text.trim());
            new.start = start;
            engine.add(new);
            open = false;
        }
        open
    }
}

fn size_text(probe: &ProbeResult) -> String {
    match (probe.size, probe.hls_segments) {
        (Some(size), _) => format_bytes(size),
        (None, Some(parts)) => format!("Unknown (HLS stream, {parts} segments)"),
        (None, None) => "Unknown".to_owned(),
    }
}
