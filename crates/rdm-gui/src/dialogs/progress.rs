//! Per-download progress window: what is being downloaded, how far along and
//! how fast it is, and - on request - what every connection is doing.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use egui::{Color32, Context, Id, RichText, Sense, Ui, vec2};
use rdm_core::util::{format_bytes, format_duration, format_speed};
use rdm_core::{Download, DownloadId, Kind, Status};

use super::primary_button;
use crate::app::Command;
use crate::icons;
use crate::widgets::segment_map;

const WIDTH: f32 = 480.0;
/// Space between the window's edge and its content.
const MARGIN: f32 = 14.0;
/// The speed graph holds one sample per half second: the last minute.
const SAMPLE_EVERY: Duration = Duration::from_millis(500);
const MAX_SAMPLES: usize = 120;

const BLUE: Color32 = Color32::from_rgb(30, 136, 229);
const GREEN: Color32 = Color32::from_rgb(67, 160, 71);
const ORANGE: Color32 = Color32::from_rgb(245, 124, 0);
const RED: Color32 = Color32::from_rgb(229, 57, 53);

pub struct Progress {
    pub id: DownloadId,
    /// Recent transfer rates, oldest first.
    speeds: VecDeque<u64>,
    sampled: Instant,
    /// Size of the window; follows its content.
    size: egui::Vec2,
}

impl Progress {
    pub fn new(id: DownloadId) -> Self {
        Self {
            id,
            speeds: VecDeque::new(),
            sampled: Instant::now(),
            size: egui::vec2(WIDTH + 2.0 * MARGIN, 330.0),
        }
    }

    /// Shows the download in a window of its own (or, where the system
    /// cannot open more windows, inside the main one). Returns `false` once
    /// it is closed.
    pub fn show(&mut self, ctx: &Context, d: &Download, commands: &mut Vec<Command>) -> bool {
        self.sample(d);
        let title = match percent(d) {
            Some(percent) => format!("{percent}% {}", d.file_name),
            None => d.file_name.clone(),
        };
        let builder = egui::ViewportBuilder::default()
            .with_title(title)
            .with_inner_size(self.size)
            .with_resizable(false)
            .with_maximize_button(false)
            .with_icon(icons::app_icon());
        let id = egui::ViewportId::from_hash_of(("progress", d.id));
        ctx.show_viewport_immediate(id, builder, |ui, class| {
            if class == egui::ViewportClass::EmbeddedWindow {
                ui.set_width(WIDTH);
                return !self.content(ui, d, commands);
            }
            let frame = egui::Frame::new()
                .fill(ui.visuals().window_fill())
                .inner_margin(MARGIN);
            let mut cancel = false;
            egui::CentralPanel::default().frame(frame).show(ui, |ui| {
                ui.set_width(WIDTH);
                // (The panel itself fills the window, so the content is
                // measured in a scope of its own.)
                let content = ui.scope(|ui| cancel = self.content(ui, d, commands));
                // The window is exactly as tall as what it shows.
                let height = (content.response.rect.height() + 2.0 * MARGIN).ceil();
                if height != self.size.y {
                    self.size.y = height;
                    ui.ctx().request_repaint();
                }
            });
            // Closing the window leaves the download running.
            let closed = ui.input(|i| i.viewport().close_requested());
            !cancel && !closed
        })
    }

    /// Everything inside the window. Returns whether "Cancel" was clicked.
    fn content(&mut self, ui: &mut Ui, d: &Download, commands: &mut Vec<Command>) -> bool {
        header(ui, d);
        ui.add_space(10.0);
        amount(ui, d);
        ui.add_space(4.0);
        segment_map(ui, d, state_color(d));
        ui.add_space(6.0);
        status_line(ui, d);
        ui.add_space(10.0);
        tiles(ui, d);
        ui.add_space(8.0);
        speed_graph(ui, &self.speeds, state_color(d));

        // Whether the details are open is remembered for all progress
        // windows until RDM is closed.
        let details_id = Id::unique("progress-details");
        let mut details = ui.data(|data| data.get_temp(details_id).unwrap_or(false));
        let mut cancel = false;
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let toggle = if details {
                "Hide details"
            } else {
                "Show details"
            };
            if ui.button(toggle).clicked() {
                details = !details;
                ui.data_mut(|data| data.insert_temp(details_id, details));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                cancel = buttons(ui, d, commands);
            });
        });
        if details {
            ui.separator();
            self::details(ui, d);
        }
        cancel
    }

    /// Adds the current transfer rate to the graph.
    fn sample(&mut self, d: &Download) {
        if self.sampled.elapsed() < SAMPLE_EVERY && !self.speeds.is_empty() {
            return;
        }
        self.sampled = Instant::now();
        let running = d.status == Status::Downloading;
        self.speeds.push_back(if running { d.speed } else { 0 });
        if self.speeds.len() > MAX_SAMPLES {
            self.speeds.pop_front();
        }
    }
}

/// Whole percent done, rounded down so 100% means finished.
fn percent(d: &Download) -> Option<u32> {
    d.progress().map(|p| (p * 100.0).floor() as u32)
}

/// The colour that stands for the download's state.
fn state_color(d: &Download) -> Color32 {
    match d.status {
        Status::Downloading | Status::Queued => BLUE,
        Status::Paused => ORANGE,
        Status::Completed => GREEN,
        Status::Failed(_) => RED,
    }
}

/// File icon, name and address.
fn header(ui: &mut Ui, d: &Download) {
    ui.horizontal(|ui| {
        let (icon, _) = ui.allocate_exact_size(vec2(36.0, 40.0), Sense::hover());
        icons::paint_file(ui.painter(), icon, icons::category_color(d.category));
        ui.vertical(|ui| {
            ui.add(egui::Label::new(RichText::new(&d.file_name).size(15.0).strong()).truncate());
            ui.add(egui::Label::new(RichText::new(&d.url).weak().size(11.5)).truncate());
        });
    });
}

/// "37%" in large figures and "12.4 MB of 33.0 MB" beside it.
fn amount(ui: &mut Ui, d: &Download) {
    ui.horizontal(|ui| {
        let percent = match percent(d) {
            Some(percent) => format!("{percent}%"),
            None => format_bytes(d.downloaded),
        };
        ui.label(RichText::new(percent).size(28.0).strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::BOTTOM), |ui| {
            let text = match d.size {
                Some(size) => format!("{} of {}", format_bytes(d.downloaded), format_bytes(size)),
                None => "size unknown".to_owned(),
            };
            ui.label(text);
        });
    });
}

/// A dot in the state's colour and what is happening right now.
fn status_line(ui: &mut Ui, d: &Download) {
    ui.horizontal_wrapped(|ui| {
        let (dot, _) = ui.allocate_exact_size(vec2(10.0, 14.0), Sense::hover());
        ui.painter()
            .circle_filled(dot.center(), 4.0, state_color(d));
        ui.label(status_text(d));
    });
}

fn status_text(d: &Download) -> String {
    match &d.status {
        Status::Downloading if d.kind == Kind::Page => "Receiving data (yt-dlp)".into(),
        Status::Downloading if d.kind == Kind::Torrent => {
            format!("Downloading from {} peers", d.connections)
        }
        Status::Downloading if d.connections > 0 => "Receiving data".into(),
        Status::Downloading => "Connecting...".into(),
        Status::Queued => "Waiting in queue".into(),
        Status::Paused => "Paused".into(),
        Status::Completed => "Complete".into(),
        Status::Failed(e) => format!("Error: {e}"),
    }
}

/// Speed, time left and connections side by side.
fn tiles(ui: &mut Ui, d: &Download) {
    let running = d.status == Status::Downloading;
    let speed = if running {
        format_speed(d.speed)
    } else {
        "-".to_owned()
    };
    let left = d
        .eta()
        .filter(|_| running)
        .map_or_else(|| "-".to_owned(), format_duration);
    let (links_name, links) = match d.kind {
        Kind::Torrent => ("Peers", d.connections.to_string()),
        Kind::Page => (
            "Quality",
            d.page.as_ref().map_or("-", |o| &o.label).to_owned(),
        ),
        _ => ("Connections", d.connections.to_string()),
    };
    ui.columns(3, |columns| {
        tile(&mut columns[0], "Speed", &speed);
        tile(&mut columns[1], "Time left", &left);
        tile(&mut columns[2], links_name, &links);
    });
}

fn tile(ui: &mut Ui, name: &str, value: &str) {
    egui::Frame::new()
        .fill(ui.visuals().faint_bg_color)
        .corner_radius(8.0)
        .inner_margin(egui::Margin::symmetric(10, 7))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(name).weak().small());
            ui.add(egui::Label::new(RichText::new(value).size(15.0).strong()).truncate());
        });
}

/// The transfer rate of the last minute as a filled curve.
fn speed_graph(ui: &mut Ui, speeds: &VecDeque<u64>, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 54.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8.0, ui.visuals().faint_bg_color);
    let peak = speeds.iter().copied().max().unwrap_or(0);
    if peak == 0 {
        return;
    }
    let area = rect.shrink2(vec2(0.0, 6.0));
    // The newest sample sits at the right edge; a full minute fills the width.
    let step = area.width() / (MAX_SAMPLES - 1) as f32;
    let points: Vec<egui::Pos2> = speeds
        .iter()
        .enumerate()
        .map(|(i, &speed)| {
            let x = area.right() - (speeds.len() - 1 - i) as f32 * step;
            let y = area.bottom() - area.height() * (speed as f32 / peak as f32);
            egui::pos2(x, y)
        })
        .collect();

    let mut fill = egui::Mesh::default();
    for (i, point) in points.iter().enumerate() {
        fill.colored_vertex(*point, color.gamma_multiply(0.28));
        fill.colored_vertex(
            egui::pos2(point.x, rect.bottom()),
            color.gamma_multiply(0.05),
        );
        if i > 0 {
            let n = 2 * i as u32;
            fill.add_triangle(n - 2, n - 1, n);
            fill.add_triangle(n - 1, n, n + 1);
        }
    }
    painter.add(egui::Shape::mesh(fill));
    painter.add(egui::Shape::line(points, egui::Stroke::new(1.5, color)));
    painter.text(
        rect.left_top() + vec2(8.0, 5.0),
        egui::Align2::LEFT_TOP,
        format!("peak {}", format_speed(peak)),
        egui::FontId::proportional(11.0),
        ui.visuals().weak_text_color(),
    );
}

/// Pause/Resume and Cancel (laid out right to left). Returns whether the
/// window should close.
fn buttons(ui: &mut Ui, d: &Download, commands: &mut Vec<Command>) -> bool {
    let size = vec2(96.0, 28.0);
    let (label, command) = if d.status.is_active() {
        ("Pause", Command::Pause(vec![d.id]))
    } else {
        ("Resume", Command::Resume(vec![d.id]))
    };
    let main = primary_button(label).min_size(size);
    if ui
        .add_enabled(d.status != Status::Completed, main)
        .clicked()
    {
        commands.push(command);
    }
    let cancel = ui
        .add(egui::Button::new("Cancel").min_size(size))
        .on_hover_text("Stop the download and close this window")
        .clicked();
    if cancel {
        commands.push(Command::Pause(vec![d.id]));
    }
    cancel
}

/// Where the file goes and what every connection is doing.
fn details(ui: &mut Ui, d: &Download) {
    egui::Grid::new("progress-facts")
        .num_columns(2)
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            ui.weak("Save to");
            ui.add(egui::Label::new(d.save_dir.display().to_string()).truncate());
            ui.end_row();
            ui.weak("Resume");
            ui.label(if d.resumable {
                "Supported"
            } else {
                "Not supported - pausing starts over"
            });
            ui.end_row();
        });
    ui.add_space(4.0);

    if d.kind == Kind::Hls {
        if let Some(h) = d.hls {
            ui.label(format!("HLS segments: {} of {}", h.done, h.total));
        }
        return;
    }
    if d.kind == Kind::Page {
        ui.weak("Downloaded with yt-dlp; its connections are not shown here.");
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
            .max_height(112.0)
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
        .max_height(112.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            egui::Grid::new("segments")
                .num_columns(3)
                .striped(true)
                .spacing([24.0, 3.0])
                .show(ui, |ui| {
                    ui.weak("Part");
                    ui.weak("Downloaded");
                    ui.weak("State");
                    ui.end_row();
                    for (i, s) in segments.iter().enumerate() {
                        ui.label(format!("{}", i + 1));
                        ui.label(format_bytes(s.done));
                        if s.is_finished() {
                            ui.colored_label(GREEN, "Finished");
                        } else if s.active && d.status == Status::Downloading {
                            ui.colored_label(BLUE, "Receiving data");
                        } else {
                            ui.weak("Waiting");
                        }
                        ui.end_row();
                    }
                });
        });
}
