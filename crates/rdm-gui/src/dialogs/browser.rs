//! "Connect your browser": a short guide for adding the extension, with a
//! live indicator that turns green as soon as the extension reaches RDM.
//!
//! Browsers install extensions on their own only from their web stores, so
//! for now the user loads the folder once; no port or other setting is needed.

use std::path::PathBuf;
use std::time::Duration;

use egui::{Color32, Context, Id, RichText};

use super::{button_row, window};

/// A Chromium based browser and the address of its extensions page.
struct Browser {
    name: &'static str,
    page: &'static str,
    /// Program names to try, per platform.
    programs: &'static [&'static str],
}

#[cfg(target_os = "windows")]
const BROWSERS: [Browser; 3] = [
    Browser {
        name: "Chrome",
        page: "chrome://extensions",
        programs: &["chrome"],
    },
    Browser {
        name: "Edge",
        page: "edge://extensions",
        programs: &["msedge"],
    },
    Browser {
        name: "Brave",
        page: "brave://extensions",
        programs: &["brave"],
    },
];
#[cfg(target_os = "macos")]
const BROWSERS: [Browser; 3] = [
    Browser {
        name: "Chrome",
        page: "chrome://extensions",
        programs: &["Google Chrome"],
    },
    Browser {
        name: "Edge",
        page: "edge://extensions",
        programs: &["Microsoft Edge"],
    },
    Browser {
        name: "Brave",
        page: "brave://extensions",
        programs: &["Brave Browser"],
    },
];
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
const BROWSERS: [Browser; 3] = [
    Browser {
        name: "Chrome",
        page: "chrome://extensions",
        programs: &[
            "google-chrome",
            "google-chrome-stable",
            "chromium",
            "chromium-browser",
        ],
    },
    Browser {
        name: "Edge",
        page: "edge://extensions",
        programs: &["microsoft-edge", "microsoft-edge-stable"],
    },
    Browser {
        name: "Brave",
        page: "brave://extensions",
        programs: &["brave-browser", "brave"],
    },
];

#[derive(Default)]
pub struct BrowserSetup {
    /// Shown when a browser could not be started.
    note: Option<String>,
}

impl BrowserSetup {
    /// `extension` is the connected extension's version and age, if any.
    pub fn show(&mut self, ctx: &Context, extension: Option<(String, Duration)>) -> bool {
        let mut open = true;
        window(ctx, "Connect your browser".into(), Id::unique("browser-setup"), 0).show(ctx, |ui| {
            ui.set_width(500.0);
            status(ui, extension);
            ui.separator();

            ui.label(RichText::new("Chrome, Edge, Brave, Opera").strong());
            ui.horizontal(|ui| {
                ui.label("1. Open the extensions page:");
                for browser in &BROWSERS {
                    if ui.button(browser.name).clicked() {
                        self.note = (!open_page(browser)).then(|| {
                            format!("Could not start {}. Open {} in it yourself.", browser.name, browser.page)
                        });
                    }
                }
            });
            ui.label("2. Switch on \"Developer mode\" (top right corner).");
            ui.label("3. Click \"Load unpacked\" and choose this folder:");
            match extension_dir() {
                Some(dir) => {
                    let path = dir.display().to_string();
                    ui.horizontal(|ui| {
                        ui.add(egui::Label::new(RichText::new(&path).monospace()).truncate());
                    });
                    if ui.button("Copy folder path").clicked() {
                        ctx.copy_text(path);
                    }
                }
                None => {
                    ui.weak("The `extension` folder that came with RDM (it is also on the releases page).");
                }
            }
            if let Some(note) = &self.note {
                ui.colored_label(Color32::from_rgb(229, 57, 53), note);
            }

            ui.add_space(6.0);
            ui.label(RichText::new("Firefox").strong());
            ui.label(
                "Open about:debugging, \"This Firefox\", \"Load Temporary Add-on\" and pick \
                 rdm-extension-firefox.zip from the releases page.",
            );
            ui.add_space(6.0);
            ui.weak(
                "That is all - RDM and the extension find each other by themselves. \
                 (Browsers only install extensions automatically from their web stores.)",
            );
            button_row(ui, |ui| open = !ui.button("Close").clicked());
        });
        // Keep the indicator live while the guide is open.
        ctx.request_repaint_after(Duration::from_secs(1));
        open
    }
}

fn status(ui: &mut egui::Ui, extension: Option<(String, Duration)>) {
    let (color, text) = match extension {
        Some((version, _)) => (
            Color32::from_rgb(67, 160, 71),
            RichText::new(format!("Connected - browser extension {version}")).strong(),
        ),
        None => (
            Color32::from_rgb(229, 57, 53),
            RichText::new("No browser extension has connected yet. Add it once like this:"),
        ),
    };
    ui.horizontal(|ui| {
        let (dot, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
        ui.painter().circle_filled(dot.center(), 5.0, color);
        ui.label(text);
    });
}

/// The `extension` folder next to the executable (installer and archives).
fn extension_dir() -> Option<PathBuf> {
    let dir = std::env::current_exe().ok()?.parent()?.join("extension");
    dir.join("manifest.json").is_file().then_some(dir)
}

/// Starts the browser on its extensions page.
fn open_page(browser: &Browser) -> bool {
    browser
        .programs
        .iter()
        .any(|program| launch(program, browser.page))
}

#[cfg(target_os = "windows")]
fn launch(program: &str, page: &str) -> bool {
    use std::os::windows::process::CommandExt;
    // `start` finds browsers through the registry (App Paths), not just PATH.
    std::process::Command::new("cmd")
        .args(["/C", "start", "", program, page])
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(target_os = "macos")]
fn launch(program: &str, page: &str) -> bool {
    std::process::Command::new("open")
        .args(["-a", program, page])
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn launch(program: &str, page: &str) -> bool {
    std::process::Command::new(program)
        .arg(page)
        .spawn()
        .is_ok()
}
