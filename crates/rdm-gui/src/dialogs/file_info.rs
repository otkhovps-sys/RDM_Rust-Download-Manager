//! "Enter new address" and "Download File Info" dialogs.

use std::path::PathBuf;

use egui::{Context, Id, RichText};
use rdm_core::util::format_bytes;
use rdm_core::{
    Category, Engine, InstallHandle, Kind, NewDownload, ProbeHandle, ProbeResult, RequestInfo,
    Tool, TorrentMeta,
};

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
                        self.error = Some("Please enter an http(s) address or a magnet link.");
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
    ["http://", "https://", "magnet:"]
        .iter()
        .any(|p| text.len() > p.len() && text.to_ascii_lowercase().starts_with(p))
}

enum State {
    Probing(ProbeHandle),
    /// A helper program (yt-dlp, ffmpeg) is being downloaded.
    Installing(Tool, InstallHandle),
    Failed(Failure),
    Ready(Box<Form>),
}

struct Failure {
    message: String,
    /// The helper program whose absence caused the failure.
    missing: Option<Tool>,
}

/// What the form asks the dialog to do.
enum FormAction {
    Keep,
    Close,
    /// Ask the server again.
    Retry,
    Install(Tool),
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
                Err(e) => State::Failed(Failure {
                    message: e.to_string(),
                    missing: match e {
                        rdm_core::Error::MissingTool(tool) => Some(tool),
                        _ => None,
                    },
                }),
            };
        }
        if let State::Installing(_, handle) = &self.state
            && let Some(result) = handle.result()
        {
            self.state = match result {
                Ok(()) => State::Probing(engine.probe(&self.url, self.request.clone())),
                Err(message) => State::Failed(Failure {
                    message,
                    missing: None,
                }),
            };
        }

        let mut open = true;
        let mut action = FormAction::Keep;
        let id = Id::unique(("file-info", self.id));
        window(ctx, "Download File Info".into(), id, self.id).show(ctx, |ui| {
            ui.set_width(480.0);
            match &mut self.state {
                State::Probing(_) => {
                    ui.horizontal(|ui| {
                        crate::widgets::busy_indicator(ui);
                        ui.label("Getting file information...");
                    });
                    if rdm_core::is_torrent_source(&self.url) {
                        ui.weak(
                            "Magnet links can take a minute: the details come from other peers.",
                        );
                    }
                    ui.weak(&self.url);
                    ctx.request_repaint_after(std::time::Duration::from_millis(100));
                    button_row(ui, |ui| open &= !ui.button("Cancel").clicked());
                }
                State::Installing(tool, handle) => {
                    ui.label(format!("Downloading {}...", tool.name()));
                    ui.add(
                        egui::ProgressBar::new(handle.progress().unwrap_or(0.0)).show_percentage(),
                    );
                    ui.weak("The official build is downloaded and its checksum verified.");
                    ctx.request_repaint_after(std::time::Duration::from_millis(200));
                    button_row(ui, |ui| open &= !ui.button("Close").clicked());
                }
                State::Failed(failure) => {
                    ui.colored_label(
                        egui::Color32::from_rgb(229, 57, 53),
                        format!("Cannot download: {}", failure.message),
                    );
                    ui.weak(&self.url);
                    let installable = failure.missing.filter(|tool| tool.can_install());
                    if installable.is_some() {
                        ui.add_space(4.0);
                        ui.label(
                            "Video sites such as YouTube need yt-dlp, a free open source \
                             program. RDM can download the official build from \
                             github.com/yt-dlp (about 35 MB).",
                        );
                    }
                    button_row(ui, |ui| {
                        open &= !ui.button("Close").clicked();
                        if ui.button("Try again").clicked() {
                            action = FormAction::Retry;
                        }
                        if let Some(tool) = installable
                            && ui
                                .button(RichText::new(format!("Install {}", tool.name())).strong())
                                .clicked()
                        {
                            action = FormAction::Install(tool);
                        }
                    });
                }
                State::Ready(form) => {
                    action = form.show(ui, engine);
                    open &= !matches!(action, FormAction::Close);
                }
            }
        });
        match action {
            FormAction::Install(tool) => {
                self.state = State::Installing(tool, engine.install_tool(tool))
            }
            FormAction::Retry => {
                self.state = State::Probing(engine.probe(&self.url, self.request.clone()));
            }
            _ => {}
        }
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
            } else if probe.kind == Kind::Http && name.contains('.') {
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
    fn show(&mut self, ui: &mut egui::Ui, engine: &Engine) -> FormAction {
        let mut action = FormAction::Keep;
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

                if !self.probe.page.is_empty() {
                    ui.label("Quality:");
                    self.quality(ui, engine);
                    ui.end_row();
                }

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
                // Torrent names come from the torrent itself.
                let editable = self.new.kind != Kind::Torrent;
                ui.add_enabled(
                    editable,
                    egui::TextEdit::singleline(&mut self.new.file_name).desired_width(400.0),
                );
                ui.end_row();

                ui.label("Description:");
                ui.add(egui::TextEdit::singleline(&mut self.new.description).desired_width(400.0));
                ui.end_row();

                ui.label("Size:");
                ui.label(match (&self.new.torrent, &self.new.page) {
                    (Some(meta), _) => format_bytes(meta.selected_size()),
                    (_, Some(option)) => option.size.map_or("Unknown".to_owned(), |s| {
                        format!("about {}", format_bytes(s))
                    }),
                    _ => size_text(&self.probe),
                });
                ui.end_row();

                ui.label("Resume:");
                ui.label(match (self.new.kind, self.probe.resumable) {
                    (Kind::Torrent, _) => "Yes - BitTorrent (peer to peer)",
                    (Kind::Page, _) => "Yes - video site (yt-dlp)",
                    (_, true) => "Yes - multiple connections",
                    (_, false) => "No - single connection",
                });
                ui.end_row();
            });

        if let Some(meta) = &mut self.new.torrent {
            torrent_files(ui, meta);
        }
        if self.new.kind == Kind::Page && engine.tool_path(Tool::Ffmpeg).is_none() {
            ui.add_space(4.0);
            ui.weak(
                "ffmpeg is not installed, so only qualities that come as one ready-made file \
                 are listed (often 360p). With ffmpeg RDM can fetch every quality and MP3.",
            );
            if Tool::Ffmpeg.can_install() {
                if ui.button("Install ffmpeg (about 130 MB)").clicked() {
                    action = FormAction::Install(Tool::Ffmpeg);
                }
            } else {
                ui.weak("Install it with your package manager, e.g. `brew install ffmpeg`.");
            }
        }

        let mut add = None;
        button_row(ui, |ui| {
            if ui.button("Cancel").clicked() {
                action = FormAction::Close;
            }
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
        let nothing_selected = self
            .new
            .torrent
            .as_ref()
            .is_some_and(|m| m.files.iter().all(|f| !f.selected));
        if nothing_selected && add.is_some() {
            add = None;
        }
        if let Some(start) = add {
            let mut new = self.new.clone();
            new.save_dir = PathBuf::from(self.dir_text.trim());
            new.start = start;
            engine.add(new);
            action = FormAction::Close;
        }
        action
    }

    /// Quality list of a web page video.
    fn quality(&mut self, ui: &mut egui::Ui, engine: &Engine) {
        let current = self.new.page.clone();
        egui::ComboBox::from_id_salt("quality")
            .selected_text(current.as_ref().map_or("", |o| o.label.as_str()))
            .show_ui(ui, |ui| {
                for option in &self.probe.page {
                    let text = match option.size {
                        Some(size) => format!("{}  ({})", option.label, format_bytes(size)),
                        None => option.label.clone(),
                    };
                    ui.selectable_value(&mut self.new.page, Some(option.clone()), text);
                }
            });
        let Some(option) = self
            .new
            .page
            .clone()
            .filter(|o| Some(o) != current.as_ref())
        else {
            return;
        };
        self.new.set_quality(option);
        if !self.custom_dir {
            self.new.save_dir = engine.settings().save_dir_for(self.new.category);
            self.dir_text = self.new.save_dir.display().to_string();
        }
    }
}

/// Checklist of the files inside a torrent.
fn torrent_files(ui: &mut egui::Ui, meta: &mut TorrentMeta) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("Files ({})", meta.files.len())).strong());
        if ui.small_button("All").clicked() {
            meta.files.iter_mut().for_each(|f| f.selected = true);
        }
        if ui.small_button("None").clicked() {
            meta.files.iter_mut().for_each(|f| f.selected = false);
        }
    });
    egui::ScrollArea::vertical()
        .max_height(160.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            for file in &mut meta.files {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut file.selected, "");
                    ui.add(egui::Label::new(&file.path).truncate());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.weak(format_bytes(file.size));
                    });
                });
            }
        });
    if meta.files.iter().all(|f| !f.selected) {
        ui.colored_label(
            egui::Color32::from_rgb(229, 57, 53),
            "Select at least one file.",
        );
    }
}

fn size_text(probe: &ProbeResult) -> String {
    match (probe.size, probe.hls_segments) {
        (Some(size), _) => format_bytes(size),
        (None, Some(parts)) => format!("Unknown (HLS stream, {parts} segments)"),
        (None, None) => "Unknown".to_owned(),
    }
}
