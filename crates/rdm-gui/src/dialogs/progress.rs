//! Per-download progress window with the connection details (IDM style).

use egui::{Context, Id, RichText};
use rdm_core::util::{format_bytes, format_duration, format_speed};
use rdm_core::{Download, DownloadId, Kind, Status};

use super::{button_row, window};
use crate::app::Command;
use crate::widgets::segment_map;

pub struct Progress {
    pub id: DownloadId,
    details: bool,
}

impl Progress {
    pub fn new(id: DownloadId) -> Self {
        Self { id, details: true }
    }

    pub fn show(&mut self, ctx: &Context, d: &Download, commands: &mut Vec<Command>) -> bool {
        let mut open = true;
        let mut cancel = false;
        let percent = d.progress().map(|p| p * 100.0);
        let title = match percent {
            Some(p) => format!("{p:.0}% {}", d.file_name),
            None => d.file_name.clone(),
        };
        window(ctx, title, Id::unique(("progress", d.id)), d.id)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_width(460.0);
                ui.label(RichText::new("Download status").strong());
                ui.separator();
                egui::Grid::new("progress-grid")
                    .num_columns(2)
                    .spacing([12.0, 4.0])
                    .show(ui, |ui| {
                        row(ui, "URL", &d.url);
                        row(ui, "Status", &status_text(d));
                        row(
                            ui,
                            "File size",
                            &d.size.map(format_bytes).unwrap_or_else(|| "Unknown".into()),
                        );
                        let downloaded = match percent {
                            Some(p) => format!("{} ({p:.2}%)", format_bytes(d.downloaded)),
                            None => format_bytes(d.downloaded),
                        };
                        row(ui, "Downloaded", &downloaded);
                        let running = d.status == Status::Downloading;
                        row(
                            ui,
                            "Transfer rate",
                            &if running {
                                format_speed(d.speed)
                            } else {
                                String::new()
                            },
                        );
                        let eta = d
                            .eta()
                            .filter(|_| running)
                            .map(format_duration)
                            .unwrap_or_default();
                        row(ui, "Time left", &eta);
                        row(
                            ui,
                            "Resume capability",
                            if d.resumable { "Yes" } else { "No" },
                        );
                    });
                ui.add_space(6.0);
                ui.add(egui::ProgressBar::new(d.progress().unwrap_or(0.0)).desired_height(16.0));

                let active = d.status.is_active();
                button_row(ui, |ui| {
                    if ui
                        .button("Cancel")
                        .on_hover_text("Stop and close")
                        .clicked()
                    {
                        commands.push(Command::Pause(vec![d.id]));
                        cancel = true;
                    }
                    let (label, command) = if active {
                        ("Pause", Command::Pause(vec![d.id]))
                    } else {
                        ("Resume", Command::Resume(vec![d.id]))
                    };
                    if ui
                        .add_enabled(d.status != Status::Completed, egui::Button::new(label))
                        .clicked()
                    {
                        commands.push(command);
                    }
                    let toggle = if self.details {
                        "<< Hide details"
                    } else {
                        "Show details >>"
                    };
                    if ui.button(toggle).clicked() {
                        self.details = !self.details;
                    }
                });

                if self.details {
                    ui.separator();
                    details(ui, d);
                }
            });
        open && !cancel
    }
}

fn row(ui: &mut egui::Ui, name: &str, value: &str) {
    ui.label(name);
    ui.add(egui::Label::new(value).truncate());
    ui.end_row();
}

fn status_text(d: &Download) -> String {
    match &d.status {
        Status::Downloading if d.kind == Kind::Page => "Receiving data (yt-dlp)...".into(),
        Status::Downloading if d.kind == Kind::Torrent => {
            format!("Downloading from {} peers", d.connections)
        }
        Status::Downloading if d.connections > 0 => {
            format!("Receiving data... ({} connections)", d.connections)
        }
        Status::Downloading => "Connecting...".into(),
        Status::Queued => "Waiting in queue".into(),
        Status::Paused => "Paused".into(),
        Status::Completed => "Complete".into(),
        Status::Failed(e) => format!("Error: {e}"),
    }
}

/// Segment map plus one line per connection.
fn details(ui: &mut egui::Ui, d: &Download) {
    segment_map(ui, d);
    ui.add_space(4.0);
    if d.kind == Kind::Hls {
        if let Some(h) = d.hls {
            ui.label(format!("HLS segments: {} / {}", h.done, h.total));
        }
        return;
    }
    if let Some(option) = &d.page {
        ui.label(format!("Video page, quality: {}", option.label));
        return;
    }
    if let Some(meta) = &d.torrent {
        let selected: Vec<_> = meta.files.iter().filter(|f| f.selected).collect();
        ui.label(format!(
            "Torrent files: {} of {} selected",
            selected.len(),
            meta.files.len()
        ));
        egui::ScrollArea::vertical()
            .max_height(140.0)
            .show(ui, |ui| {
                for file in selected {
                    ui.horizontal(|ui| {
                        ui.add(egui::Label::new(&file.path).truncate());
                        ui.weak(format_bytes(file.size));
                    });
                }
            });
        return;
    }
    let mut segments = d.segments.clone();
    segments.sort_by_key(|s| s.start);
    egui::ScrollArea::vertical()
        .max_height(140.0)
        .show(ui, |ui| {
            egui::Grid::new("segments")
                .num_columns(3)
                .striped(true)
                .spacing([16.0, 2.0])
                .show(ui, |ui| {
                    ui.label(RichText::new("N.").strong());
                    ui.label(RichText::new("Downloaded").strong());
                    ui.label(RichText::new("Info").strong());
                    ui.end_row();
                    for (i, s) in segments.iter().enumerate() {
                        ui.label(format!("{}", i + 1));
                        ui.label(format_bytes(s.done));
                        let info = if s.is_finished() {
                            "Finished"
                        } else if s.active && d.status == Status::Downloading {
                            "Receiving data..."
                        } else {
                            "Waiting"
                        };
                        ui.label(info);
                        ui.end_row();
                    }
                });
        });
}
