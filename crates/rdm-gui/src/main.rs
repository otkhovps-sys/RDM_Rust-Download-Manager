//! RDM - Rust Download Manager.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod dialogs;
mod fonts;
mod icons;
mod server;
mod table;
mod widgets;

use rdm_core::Engine;

fn main() -> eframe::Result {
    // `rdm <url>`: hand the URL to an already running instance (single instance).
    let url_arg = std::env::args()
        .nth(1)
        .and_then(|arg| source_argument(&arg));
    let port = rdm_core::Settings::load().server_port;
    if server::forward_to_running(port, url_arg.as_deref()) {
        return Ok(());
    }

    let engine = match Engine::new() {
        Ok(engine) => engine,
        Err(e) => {
            eprintln!("rdm: failed to start: {e}");
            std::process::exit(1);
        }
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("RDM - Rust Download Manager")
            .with_app_id("rdm")
            .with_inner_size([960.0, 560.0])
            .with_min_inner_size([640.0, 360.0])
            .with_icon(icons::app_icon()),
        ..Default::default()
    };
    eframe::run_native(
        "RDM",
        options,
        Box::new(move |cc| Ok(Box::new(app::RdmApp::new(cc, engine, url_arg)))),
    )
}

/// Accepts URLs, magnet links and paths to `.torrent` files (e.g. when RDM is
/// the default app for them).
fn source_argument(arg: &str) -> Option<String> {
    if arg.contains("://") || arg.to_ascii_lowercase().starts_with("magnet:") {
        return Some(arg.to_owned());
    }
    let path = std::path::Path::new(arg);
    (rdm_core::is_torrent_source(arg) && path.is_file()).then(|| {
        path.canonicalize()
            .unwrap_or(path.to_path_buf())
            .display()
            .to_string()
    })
}
