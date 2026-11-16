//! Options dialog (tabs like IDM: Connection, Save To, Browser, General).

use std::path::PathBuf;

use egui::{Context, Id, Ui};
use rdm_core::{Category, Engine, Settings};

use super::{button_row, window};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Connection,
    SaveTo,
    Browser,
    General,
}

pub struct OptionsDialog {
    settings: Settings,
    dir_text: String,
    tab: Tab,
}

impl OptionsDialog {
    pub fn new(settings: Settings) -> Self {
        let dir_text = settings.download_dir.display().to_string();
        Self {
            settings,
            dir_text,
            tab: Tab::Connection,
        }
    }

    pub fn show(&mut self, ctx: &Context, engine: &Engine) -> bool {
        let mut open = true;
        window(ctx, "Options".into(), Id::unique("options"), 0).show(ctx, |ui| {
            ui.set_width(500.0);
            ui.horizontal(|ui| {
                for (tab, label) in [
                    (Tab::Connection, "Connection"),
                    (Tab::SaveTo, "Save To"),
                    (Tab::Browser, "Browser"),
                    (Tab::General, "General"),
                ] {
                    ui.selectable_value(&mut self.tab, tab, label);
                }
            });
            ui.separator();
            ui.set_min_height(230.0);
            match self.tab {
                Tab::Connection => self.connection(ui),
                Tab::SaveTo => self.save_to(ui),
                Tab::Browser => self.browser(ui),
                Tab::General => self.general(ui),
            }
            button_row(ui, |ui| {
                if ui.button("Cancel").clicked() {
                    open = false;
                }
                if ui.button("   OK   ").clicked() {
                    self.settings.download_dir = PathBuf::from(self.dir_text.trim());
                    engine.set_settings(self.settings.clone());
                    crate::app::apply_theme(ui.ctx(), self.settings.dark_mode);
                    open = false;
                }
            });
        });
        open
    }

    fn connection(&mut self, ui: &mut Ui) {
        let s = &mut self.settings;
        egui::Grid::new("opt-connection")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                ui.label("Connections per download:");
                ui.add(egui::Slider::new(&mut s.connections, 1..=32));
                ui.end_row();
                ui.label("Simultaneous downloads:");
                ui.add(egui::Slider::new(&mut s.max_active, 1..=20));
                ui.end_row();
                ui.label("Speed limit (KB/s):");
                ui.horizontal(|ui| {
                    ui.add(
                        egui::DragValue::new(&mut s.speed_limit_kib)
                            .range(0..=10_000_000)
                            .speed(16),
                    );
                    ui.weak("0 = unlimited");
                });
                ui.end_row();
                ui.label("Retries per connection:");
                ui.add(egui::Slider::new(&mut s.max_retries, 0..=50));
                ui.end_row();
            });
        ui.add_space(8.0);
        ui.weak(
            "New values apply to downloads started from now on. The speed limit applies at once.",
        );
    }

    fn save_to(&mut self, ui: &mut Ui) {
        ui.label("Default download folder:");
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.dir_text).desired_width(380.0));
            if ui.button("Browse...").clicked()
                && let Some(dir) = rfd::FileDialog::new()
                    .set_directory(&self.dir_text)
                    .pick_folder()
            {
                self.dir_text = dir.display().to_string();
            }
        });
        ui.checkbox(
            &mut self.settings.category_subfolders,
            "Save each category in its own sub folder",
        );
        ui.add_space(6.0);
        let preview = Settings {
            download_dir: PathBuf::from(self.dir_text.trim()),
            ..self.settings.clone()
        };
        egui::Grid::new("opt-folders")
            .num_columns(2)
            .spacing([12.0, 2.0])
            .show(ui, |ui| {
                for category in Category::ALL {
                    ui.label(category.label());
                    ui.weak(preview.save_dir_for(category).display().to_string());
                    ui.end_row();
                }
            });
    }

    fn browser(&mut self, ui: &mut Ui) {
        let s = &mut self.settings;
        ui.checkbox(
            &mut s.browser_integration,
            "Take over downloads from the browser extension",
        );
        ui.add_space(6.0);
        ui.label("Automatically download these file types:");
        ui.add(
            egui::TextEdit::multiline(&mut s.capture_types)
                .desired_rows(4)
                .desired_width(f32::INFINITY),
        );
        ui.add_space(6.0);
        ui.weak(
            "RDM and the extension find each other automatically. To add the extension to a \
             browser, use Tasks > Connect browser.",
        );
    }

    fn general(&mut self, ui: &mut Ui) {
        let s = &mut self.settings;
        ui.checkbox(&mut s.dark_mode, "Dark theme");
        ui.checkbox(
            &mut s.close_to_tray,
            "Closing the window keeps RDM running in the system tray",
        );
        ui.checkbox(
            &mut s.show_complete_dialog,
            "Show the \"Download complete\" dialog",
        );
        ui.add_space(6.0);
        ui.label("User agent used when the browser does not send one:");
        ui.add(egui::TextEdit::singleline(&mut s.user_agent).desired_width(f32::INFINITY));
    }
}
