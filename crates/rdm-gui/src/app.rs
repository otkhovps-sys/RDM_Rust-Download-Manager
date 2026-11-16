//! Main window: menu, toolbar, category tree, download list and status bar.

use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

/// egui redraws on every mouse move; more than this is wasted CPU for a
/// download manager (and high refresh monitors would double or triple it).
const MIN_FRAME_TIME: Duration = Duration::from_millis(33);
/// How often the download list snapshot is taken from the engine.
const LIST_REFRESH: Duration = Duration::from_millis(250);

use egui::{Color32, RichText, Ui};
use rdm_core::util::format_speed;
use rdm_core::{Category, Download, DownloadId, Engine, Event, Status};

use crate::dialogs::Dialogs;
use crate::icons::{self, Icon};
use crate::server::{self, Message};
use crate::table::DownloadTable;
use crate::tray::{Tray, TrayAction};
use crate::widgets::tool_button;

/// Everything the user can trigger from the menu, toolbar, list or dialogs.
pub enum Command {
    AddUrl,
    OpenTorrent,
    Resume(Vec<DownloadId>),
    Pause(Vec<DownloadId>),
    PauseAll,
    Delete(Vec<DownloadId>),
    DeleteCompleted,
    Redownload(DownloadId),
    ShowProgress(DownloadId),
    OpenFile(DownloadId),
    OpenFolder(DownloadId),
    CopyUrl(DownloadId),
    Options,
    About,
    Credits,
    Quit,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Group {
    All,
    Unfinished,
    Finished,
}

/// Which downloads the list shows (the selected node of the category tree).
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Filter {
    pub group: Group,
    pub category: Option<Category>,
}

impl Filter {
    pub fn matches(&self, d: &Download) -> bool {
        let done = d.status == Status::Completed;
        let group_ok = match self.group {
            Group::All => true,
            Group::Unfinished => !done,
            Group::Finished => done,
        };
        group_ok && self.category.is_none_or(|c| c == d.category)
    }
}

pub struct RdmApp {
    engine: Engine,
    messages: Option<Receiver<Message>>,
    server_error: Option<String>,
    downloads: Vec<Download>,
    filter: Filter,
    table: DownloadTable,
    dialogs: Dialogs,
    last_frame: Instant,
    /// `None` forces a refresh on the next frame.
    list_taken: Option<Instant>,
    /// `None` when the desktop has no system tray.
    tray: Option<Tray>,
    /// Set by "Exit": the next close request really ends the program.
    quit_requested: Option<Instant>,
}

impl RdmApp {
    pub fn new(
        cc: &eframe::CreationContext,
        engine: Engine,
        url: Option<String>,
        start_hidden: bool,
    ) -> Self {
        crate::fonts::install(&cc.egui_ctx);
        apply_theme(&cc.egui_ctx, engine.settings().dark_mode);
        let ctx = cc.egui_ctx.clone();
        engine.set_waker(move || ctx.request_repaint());

        let port = engine.settings().server_port;
        let (messages, server_error) =
            match server::spawn(port, engine.clone(), cc.egui_ctx.clone()) {
                Ok(rx) => (Some(rx), None),
                Err(e) => (
                    None,
                    Some(format!(
                        "Browser integration off: port {port} unavailable ({e})"
                    )),
                ),
            };

        let mut dialogs = Dialogs::default();
        if let Some(url) = url {
            dialogs.open_file_info(&engine, url, Default::default(), None);
        }
        let tray = Tray::new(&cc.egui_ctx);
        // Starting hidden only makes sense when the tray can bring RDM back.
        if start_hidden && (tray.is_none() || !dialogs.is_empty()) {
            show_window(&cc.egui_ctx);
        }
        Self {
            engine,
            messages,
            server_error,
            downloads: Vec::new(),
            last_frame: Instant::now(),
            list_taken: None,
            tray,
            quit_requested: None,
            filter: Filter {
                group: Group::All,
                category: None,
            },
            table: DownloadTable::default(),
            dialogs,
        }
    }

    fn execute(&mut self, ctx: &egui::Context, command: Command) {
        let engine = &self.engine;
        match command {
            Command::AddUrl => self.dialogs.open_add_url(),
            Command::OpenTorrent => {
                let file = rfd::FileDialog::new()
                    .add_filter("Torrent", &["torrent"])
                    .set_title("Open torrent file")
                    .pick_file();
                if let Some(path) = file {
                    let source = path.display().to_string();
                    self.dialogs
                        .open_file_info(engine, source, Default::default(), None);
                }
            }
            Command::Resume(ids) => ids.into_iter().for_each(|id| engine.resume(id)),
            Command::Pause(ids) => ids.into_iter().for_each(|id| engine.pause(id)),
            Command::PauseAll => engine.pause_all(),
            Command::Delete(ids) if !ids.is_empty() => self.dialogs.confirm_delete(ids),
            Command::Delete(_) => {}
            Command::DeleteCompleted => {
                let done: Vec<_> = self
                    .downloads
                    .iter()
                    .filter(|d| d.status == Status::Completed)
                    .map(|d| d.id)
                    .collect();
                engine.remove(&done, false);
            }
            Command::Redownload(id) => engine.redownload(id),
            Command::ShowProgress(id) => self.dialogs.show_progress(id),
            Command::OpenFile(id) => {
                if let Some(d) = engine.get(id) {
                    let _ = open::that_detached(d.path());
                }
            }
            Command::OpenFolder(id) => {
                if let Some(d) = engine.get(id) {
                    reveal(&d);
                }
            }
            Command::CopyUrl(id) => {
                if let Some(d) = engine.get(id) {
                    ctx.copy_text(d.url);
                }
            }
            Command::Options => self.dialogs.open_options(engine.settings()),
            Command::About => self.dialogs.about = true,
            Command::Credits => self.dialogs.credits = true,
            Command::Quit => {
                self.quit_requested = Some(Instant::now());
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    /// Browser extension requests, tray menu and engine events. Runs even
    /// while the window is hidden in the tray.
    fn poll(&mut self, ctx: &egui::Context) {
        let actions = self.tray.as_ref().map(Tray::poll).unwrap_or_default();
        for action in actions {
            self.list_taken = None;
            match action {
                TrayAction::Show => show_window(ctx),
                TrayAction::AddUrl => {
                    show_window(ctx);
                    self.execute(ctx, Command::AddUrl);
                }
                TrayAction::ResumeAll => self.engine.resume_all(),
                TrayAction::PauseAll => self.engine.pause_all(),
                TrayAction::Quit => self.execute(ctx, Command::Quit),
            }
        }
        while let Some(message) = self.messages.as_ref().and_then(|rx| rx.try_recv().ok()) {
            if let Message::Download(incoming) = message {
                self.dialogs.open_file_info(
                    &self.engine,
                    incoming.url,
                    incoming.request,
                    incoming.file_name,
                );
            }
            show_window(ctx);
        }
        for event in self.engine.poll_events() {
            self.list_taken = None;
            match event {
                Event::Completed(id) => {
                    self.dialogs.close_progress(id);
                    if self.engine.settings().show_complete_dialog {
                        self.dialogs.complete.push(id);
                        show_window(ctx);
                    }
                }
                Event::Failed(..) => {}
            }
        }
    }

    /// The close button hides RDM in the tray instead of ending it.
    fn close_to_tray(&mut self, ctx: &egui::Context) {
        if !ctx.input(|i| i.viewport().close_requested()) {
            return;
        }
        let stay = self.quit_requested.is_none()
            && self.tray.is_some()
            && self.engine.settings().close_to_tray;
        if stay {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
    }

    fn update_tray_tooltip(&mut self) {
        let Some(tray) = &mut self.tray else {
            return;
        };
        let running: Vec<_> = self
            .downloads
            .iter()
            .filter(|d| d.status == Status::Downloading)
            .collect();
        let text = match running.len() {
            0 => "RDM".to_owned(),
            n => format!(
                "RDM - {n} downloading, {}",
                format_speed(running.iter().map(|d| d.speed).sum())
            ),
        };
        tray.set_tooltip(text);
    }

    fn shortcuts(&self, ctx: &egui::Context) -> Vec<Command> {
        // Text fields in dialogs keep their keys.
        if ctx.egui_wants_keyboard_input() {
            return Vec::new();
        }
        let mut commands = Vec::new();
        ctx.input(|i| {
            if i.modifiers.command && i.key_pressed(egui::Key::N) {
                commands.push(Command::AddUrl);
            }
            if i.modifiers.command && i.key_pressed(egui::Key::O) {
                commands.push(Command::OpenTorrent);
            }
            if i.key_pressed(egui::Key::Delete) {
                commands.push(Command::Delete(self.table.selected()));
            }
        });
        commands
    }

    fn menu_bar(&self, ui: &mut Ui, commands: &mut Vec<Command>) {
        let selected = self.table.selected();
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("Tasks", |ui| {
                if ui.button("Add new download...").clicked() {
                    commands.push(Command::AddUrl);
                }
                if ui.button("Open torrent file...").clicked() {
                    commands.push(Command::OpenTorrent);
                }
                ui.separator();
                if ui.button("Exit").clicked() {
                    commands.push(Command::Quit);
                }
            });
            ui.menu_button("Downloads", |ui| {
                if ui.button("Resume").clicked() {
                    commands.push(Command::Resume(selected.clone()));
                }
                if ui.button("Stop").clicked() {
                    commands.push(Command::Pause(selected.clone()));
                }
                if ui.button("Resume all").clicked() {
                    let ids = self.downloads.iter().map(|d| d.id).collect();
                    commands.push(Command::Resume(ids));
                }
                if ui.button("Stop all").clicked() {
                    commands.push(Command::PauseAll);
                }
                ui.separator();
                if ui.button("Delete...").clicked() {
                    commands.push(Command::Delete(selected.clone()));
                }
                if ui.button("Delete completed").clicked() {
                    commands.push(Command::DeleteCompleted);
                }
                ui.separator();
                if ui.button("Options...").clicked() {
                    commands.push(Command::Options);
                }
            });
            ui.menu_button("Help", |ui| {
                if ui.button("Credits").clicked() {
                    commands.push(Command::Credits);
                }
                if ui.button("About RDM").clicked() {
                    commands.push(Command::About);
                }
            });
        });
    }

    fn toolbar(&self, ui: &mut Ui, commands: &mut Vec<Command>) {
        let selected = self.table.selected();
        let has = |pred: fn(&Status) -> bool| {
            self.downloads
                .iter()
                .any(|d| selected.contains(&d.id) && pred(&d.status))
        };
        let can_resume = has(|s| matches!(s, Status::Paused | Status::Failed(_)));
        let can_stop = has(Status::is_active);
        let any_active = self.downloads.iter().any(|d| d.status.is_active());
        let any_done = self.downloads.iter().any(|d| d.status == Status::Completed);

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let mut button = |icon, label, enabled, command: Command| {
                if tool_button(ui, icon, label, enabled).clicked() {
                    commands.push(command);
                }
            };
            button(Icon::Add, "Add URL", true, Command::AddUrl);
            button(
                Icon::Resume,
                "Resume",
                can_resume,
                Command::Resume(selected.clone()),
            );
            button(
                Icon::Stop,
                "Stop",
                can_stop,
                Command::Pause(selected.clone()),
            );
            button(Icon::StopAll, "Stop All", any_active, Command::PauseAll);
            button(
                Icon::Delete,
                "Delete",
                !selected.is_empty(),
                Command::Delete(selected.clone()),
            );
            button(
                Icon::DeleteCompleted,
                "Del. Completed",
                any_done,
                Command::DeleteCompleted,
            );
            button(Icon::Options, "Options", true, Command::Options);
            button(Icon::About, "About", true, Command::About);
        });
    }

    fn category_tree(&mut self, ui: &mut Ui) {
        ui.add_space(4.0);
        ui.label(RichText::new("Categories").strong());
        ui.add_space(2.0);
        for (group, title) in [
            (Group::All, "All Downloads"),
            (Group::Unfinished, "Unfinished"),
            (Group::Finished, "Finished"),
        ] {
            let header = egui::CollapsingHeader::new(title)
                .id_salt(title)
                .default_open(group == Group::All);
            header.show(ui, |ui| {
                self.tree_node(
                    ui,
                    Filter {
                        group,
                        category: None,
                    },
                    "All",
                );
                for category in Category::ALL {
                    self.tree_node(
                        ui,
                        Filter {
                            group,
                            category: Some(category),
                        },
                        category.label(),
                    );
                }
            });
        }
    }

    fn tree_node(&mut self, ui: &mut Ui, filter: Filter, label: &str) {
        let count = self.downloads.iter().filter(|d| filter.matches(d)).count();
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 16.0), egui::Sense::hover());
            let color = filter
                .category
                .map_or(Color32::from_rgb(255, 193, 7), icons::category_color);
            icons::paint_file(ui.painter(), rect, color);
            let text = if count > 0 {
                format!("{label} ({count})")
            } else {
                label.to_owned()
            };
            if ui.selectable_label(self.filter == filter, text).clicked() {
                self.filter = filter;
            }
        });
    }

    fn status_bar(&self, ui: &mut Ui) {
        let running: Vec<_> = self
            .downloads
            .iter()
            .filter(|d| d.status == Status::Downloading)
            .collect();
        let speed: u64 = running.iter().map(|d| d.speed).sum();
        let queued = self
            .downloads
            .iter()
            .filter(|d| d.status == Status::Queued)
            .count();
        ui.horizontal(|ui| {
            ui.label(format!("Downloading: {}", running.len()));
            ui.separator();
            ui.label(format!("Queued: {queued}"));
            ui.separator();
            ui.label(format!("Total speed: {}", format_speed(speed)));
            ui.separator();
            match &self.server_error {
                Some(error) => ui.colored_label(Color32::from_rgb(229, 57, 53), error),
                None => ui.weak(format!(
                    "Browser integration: 127.0.0.1:{}",
                    self.engine.settings().server_port
                )),
            };
        });
    }
}

impl eframe::App for RdmApp {
    /// Called before every frame and also while the window is hidden.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll(ctx);
        self.close_to_tray(ctx);
        if self.list_taken.is_none_or(|t| t.elapsed() >= LIST_REFRESH) {
            self.downloads = self.engine.list();
            self.table.retain_existing(&self.downloads);
            self.list_taken = Some(Instant::now());
            self.update_tray_tooltip();
        }
        // Keeps the tray tooltip current while downloads run in the background.
        if self.downloads.iter().any(|d| d.status.is_active()) {
            ctx.request_repaint_after(Duration::from_secs(1));
        }
        // "Exit" must work even if the hidden window ignores the close request.
        if self
            .quit_requested
            .is_some_and(|t| t.elapsed() > Duration::from_secs(2))
        {
            self.engine.shutdown();
            std::process::exit(0);
        }
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // Cap the frame rate: input that arrives meanwhile is simply handled
        // together in the next frame.
        if let Some(wait) = MIN_FRAME_TIME.checked_sub(self.last_frame.elapsed()) {
            std::thread::sleep(wait);
        }
        self.last_frame = Instant::now();

        let mut commands = self.shortcuts(&ctx);
        egui::Panel::top("menu").show(ui, |ui| self.menu_bar(ui, &mut commands));
        egui::Panel::top("toolbar").show(ui, |ui| self.toolbar(ui, &mut commands));
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        egui::Panel::left("categories")
            .resizable(true)
            .default_size(180.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.category_tree(ui))
            });
        egui::CentralPanel::default().show(ui, |ui| {
            let visible: Vec<&Download> = self
                .downloads
                .iter()
                .filter(|d| self.filter.matches(d))
                .collect();
            commands.extend(self.table.show(ui, &visible));
        });
        commands.extend(self.dialogs.show(&ctx, &self.engine, &self.downloads));

        if !commands.is_empty() {
            self.list_taken = None; // show the effect right away
        }
        for command in commands {
            self.execute(&ctx, command);
        }
        // Refresh while something moves; otherwise sleep until an event arrives.
        if self.downloads.iter().any(|d| d.status.is_active()) {
            ctx.request_repaint_after(Duration::from_millis(400));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.engine.shutdown();
    }
}

/// Brings the window back from the tray or the task bar.
fn show_window(ctx: &egui::Context) {
    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
}

pub fn apply_theme(ctx: &egui::Context, dark: bool) {
    ctx.set_theme(if dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    });
}

/// Opens the folder of a download, selecting the file where the OS can.
fn reveal(d: &Download) {
    let path = if d.status == Status::Completed {
        d.path()
    } else {
        d.save_dir.clone()
    };
    #[cfg(target_os = "windows")]
    if path.is_file() {
        let _ = std::process::Command::new("explorer")
            .arg("/select,")
            .arg(&path)
            .spawn();
        return;
    }
    #[cfg(target_os = "macos")]
    if path.is_file() {
        let _ = std::process::Command::new("open")
            .arg("-R")
            .arg(&path)
            .spawn();
        return;
    }
    let folder = if path.is_file() {
        d.save_dir.clone()
    } else {
        path
    };
    let _ = open::that_detached(folder);
}
