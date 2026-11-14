//! The download list with sortable columns and multi-selection.

use std::cmp::Ordering;
use std::collections::BTreeSet;

use egui::{Align, Layout, Modifiers, RichText, Sense, Ui};
use egui_extras::{Column, TableBuilder};
use rdm_core::util::{format_bytes, format_duration, format_speed, format_timestamp};
use rdm_core::{Download, DownloadId, Status};

use crate::app::Command;
use crate::icons;

#[derive(Clone, Copy, PartialEq, Eq)]
enum SortKey {
    Name,
    Size,
    Status,
    TimeLeft,
    Speed,
    LastTry,
    Description,
    /// Default order: as added (no column for it).
    Added,
}

const COLUMNS: [(&str, SortKey); 7] = [
    ("File Name", SortKey::Name),
    ("Size", SortKey::Size),
    ("Status", SortKey::Status),
    ("Time left", SortKey::TimeLeft),
    ("Transfer rate", SortKey::Speed),
    ("Last Try Date", SortKey::LastTry),
    ("Description", SortKey::Description),
];

pub struct DownloadTable {
    selected: BTreeSet<DownloadId>,
    /// Row where a shift-click range starts.
    anchor: Option<DownloadId>,
    sort: SortKey,
    ascending: bool,
}

impl Default for DownloadTable {
    fn default() -> Self {
        Self {
            selected: BTreeSet::new(),
            anchor: None,
            sort: SortKey::Added,
            ascending: true,
        }
    }
}

impl DownloadTable {
    pub fn selected(&self) -> Vec<DownloadId> {
        self.selected.iter().copied().collect()
    }

    pub fn retain_existing(&mut self, downloads: &[Download]) {
        self.selected
            .retain(|id| downloads.iter().any(|d| d.id == *id));
    }

    pub fn show(&mut self, ui: &mut Ui, rows: &[&Download]) -> Vec<Command> {
        let mut rows = rows.to_vec();
        rows.sort_by(|a, b| {
            let order = compare(self.sort, a, b);
            if self.ascending {
                order
            } else {
                order.reverse()
            }
        });
        let order: Vec<DownloadId> = rows.iter().map(|d| d.id).collect();
        if !ui.ctx().egui_wants_keyboard_input()
            && ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::A))
        {
            self.selected = order.iter().copied().collect();
        }

        let mut commands = Vec::new();
        let table = TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .sense(Sense::click())
            .cell_layout(Layout::left_to_right(Align::Center))
            .column(Column::initial(230.0).at_least(120.0).clip(true))
            .column(Column::initial(75.0).at_least(50.0))
            .column(Column::initial(100.0).at_least(60.0).clip(true))
            .column(Column::initial(80.0).at_least(50.0))
            .column(Column::initial(85.0).at_least(50.0))
            .column(Column::initial(110.0).at_least(60.0))
            .column(Column::remainder().at_least(60.0).clip(true));

        table
            .header(22.0, |mut header| {
                for (title, key) in COLUMNS {
                    header.col(|ui| {
                        let arrow = match (self.sort == key, self.ascending) {
                            (true, true) => " ⏶",
                            (true, false) => " ⏷",
                            _ => "",
                        };
                        let label =
                            egui::Label::new(RichText::new(format!("{title}{arrow}")).strong())
                                .sense(Sense::click())
                                .selectable(false);
                        if ui.add(label).clicked() {
                            if self.sort == key {
                                self.ascending = !self.ascending;
                            } else {
                                (self.sort, self.ascending) = (key, true);
                            }
                        }
                    });
                }
            })
            .body(|body| {
                body.rows(22.0, rows.len(), |mut row| {
                    let d = rows[row.index()];
                    row.set_selected(self.selected.contains(&d.id));
                    row.col(|ui| {
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(14.0, 16.0), Sense::hover());
                        icons::paint_file(ui.painter(), rect, icons::category_color(d.category));
                        ui.add(egui::Label::new(&d.file_name).truncate().selectable(false));
                    });
                    row.col(|ui| {
                        ui.label(d.size.map(format_bytes).unwrap_or_default());
                    });
                    row.col(|ui| {
                        status_label(ui, d);
                    });
                    row.col(|ui| {
                        if d.status == Status::Downloading {
                            ui.label(d.eta().map(format_duration).unwrap_or_default());
                        }
                    });
                    row.col(|ui| {
                        if d.status == Status::Downloading {
                            ui.label(format_speed(d.speed));
                        }
                    });
                    row.col(|ui| {
                        ui.label(d.last_try.map(format_timestamp).unwrap_or_default());
                    });
                    row.col(|ui| {
                        ui.add(
                            egui::Label::new(&d.description)
                                .truncate()
                                .selectable(false),
                        );
                    });

                    let response = row.response();
                    let modifiers = response.ctx.input(|i| i.modifiers);
                    if response.clicked() {
                        self.click(d.id, &order, modifiers);
                    }
                    if response.secondary_clicked() && !self.selected.contains(&d.id) {
                        self.click(d.id, &order, Modifiers::NONE);
                    }
                    if response.double_clicked() {
                        commands.push(match d.status {
                            Status::Completed => Command::OpenFile(d.id),
                            _ => Command::ShowProgress(d.id),
                        });
                    }
                    response.context_menu(|ui| self.context_menu(ui, d, &mut commands));
                });
            });

        if rows.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.weak("No downloads here yet.\nClick \"Add URL\" or start a download in your browser.");
            });
        }
        commands
    }

    fn click(&mut self, id: DownloadId, order: &[DownloadId], modifiers: Modifiers) {
        let position = |id| order.iter().position(|&x| x == id);
        match (
            modifiers.shift,
            self.anchor.and_then(position),
            position(id),
        ) {
            (true, Some(a), Some(b)) => {
                self.selected = order[a.min(b)..=a.max(b)].iter().copied().collect();
                return;
            }
            _ if modifiers.command => {
                if !self.selected.remove(&id) {
                    self.selected.insert(id);
                }
            }
            _ => self.selected = BTreeSet::from([id]),
        }
        self.anchor = Some(id);
    }

    fn context_menu(&self, ui: &mut Ui, d: &Download, commands: &mut Vec<Command>) {
        let selected = self.selected();
        let done = d.status == Status::Completed;
        if ui.add_enabled(done, egui::Button::new("Open")).clicked() {
            commands.push(Command::OpenFile(d.id));
        }
        if ui.button("Open folder").clicked() {
            commands.push(Command::OpenFolder(d.id));
        }
        ui.separator();
        if ui.add_enabled(!done, egui::Button::new("Resume")).clicked() {
            commands.push(Command::Resume(selected.clone()));
        }
        if ui
            .add_enabled(d.status.is_active(), egui::Button::new("Stop"))
            .clicked()
        {
            commands.push(Command::Pause(selected.clone()));
        }
        if ui.button("Redownload").clicked() {
            commands.push(Command::Redownload(d.id));
        }
        ui.separator();
        if ui.button("Copy address").clicked() {
            commands.push(Command::CopyUrl(d.id));
        }
        if ui.button("Properties / progress").clicked() {
            commands.push(Command::ShowProgress(d.id));
        }
        ui.separator();
        if ui.button("Delete...").clicked() {
            commands.push(Command::Delete(selected));
        }
    }
}

fn status_label(ui: &mut Ui, d: &Download) {
    let percent = d.progress().map(|p| format!("{:.1}%", p * 100.0));
    match &d.status {
        Status::Completed => ui.label("Complete"),
        Status::Downloading => ui.label(percent.unwrap_or_else(|| format_bytes(d.downloaded))),
        Status::Queued => ui.label("Queued"),
        Status::Paused => {
            ui.label(percent.map_or("Paused".to_owned(), |p| format!("{p} (Paused)")))
        }
        Status::Failed(e) => ui
            .colored_label(egui::Color32::from_rgb(229, 57, 53), "Error")
            .on_hover_text(e),
    };
}

fn compare(key: SortKey, a: &Download, b: &Download) -> Ordering {
    match key {
        SortKey::Name => a.file_name.to_lowercase().cmp(&b.file_name.to_lowercase()),
        SortKey::Size => a.size.cmp(&b.size),
        SortKey::Status => {
            let p = |d: &Download| (d.status == Status::Completed, d.progress().unwrap_or(0.0));
            p(a).partial_cmp(&p(b)).unwrap_or(Ordering::Equal)
        }
        SortKey::TimeLeft => a.eta().cmp(&b.eta()),
        SortKey::Speed => a.speed.cmp(&b.speed),
        SortKey::LastTry => a.last_try.cmp(&b.last_try),
        SortKey::Description => a.description.cmp(&b.description),
        SortKey::Added => a.added_at.cmp(&b.added_at).then(a.id.cmp(&b.id)),
    }
}
