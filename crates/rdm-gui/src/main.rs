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

fn main() {
    install_crash_handler();

    // `rdm <url>`: hand the URL to an already running instance (single instance).
    let url_arg = std::env::args()
        .nth(1)
        .and_then(|arg| source_argument(&arg));
    let port = rdm_core::Settings::load().server_port;
    if server::forward_to_running(port, url_arg.as_deref()) {
        return;
    }

    let engine = Engine::new().unwrap_or_else(|e| fatal(&format!("Failed to start: {e}")));
    // Try each renderer until one works on this machine's graphics driver.
    let mut errors = Vec::new();
    for renderer in renderers() {
        let options = eframe::NativeOptions {
            renderer,
            #[cfg(windows)]
            wgpu_options: low_power_gpu(),
            viewport: egui::ViewportBuilder::default()
                .with_title("RDM - Rust Download Manager")
                .with_app_id("rdm")
                .with_inner_size([960.0, 560.0])
                .with_min_inner_size([640.0, 360.0])
                .with_icon(icons::app_icon()),
            ..Default::default()
        };
        let (engine, url) = (engine.clone(), url_arg.clone());
        let result = eframe::run_native(
            "RDM",
            options,
            Box::new(move |cc| Ok(Box::new(app::RdmApp::new(cc, engine, url)))),
        );
        match result {
            Ok(()) => return,
            Err(e) => errors.push(format!("{renderer:?}: {e}")),
        }
    }
    fatal(&format!(
        "Could not open the window. Please update your graphics driver.\n\n{}",
        errors.join("\n")
    ));
}

/// Windows: DirectX/Vulkan first, OpenGL as fallback. Elsewhere: OpenGL.
fn renderers() -> Vec<eframe::Renderer> {
    #[cfg(windows)]
    return vec![eframe::Renderer::Wgpu, eframe::Renderer::Glow];
    #[cfg(not(windows))]
    return vec![eframe::Renderer::Glow];
}

/// A download manager never needs the discrete GPU of a laptop; the
/// integrated one saves power (still overridable with `WGPU_POWER_PREF`).
#[cfg(windows)]
fn low_power_gpu() -> eframe::WgpuConfiguration {
    let mut config = eframe::WgpuConfiguration::default();
    if std::env::var_os("WGPU_POWER_PREF").is_none()
        && let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut config.wgpu_setup
    {
        setup.power_preference = eframe::wgpu::PowerPreference::LowPower;
    }
    config
}

/// The release build has no console, so errors go to `crash.log` in the data
/// folder and to a message box instead of disappearing silently.
fn install_crash_handler() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_hook(info);
        let backtrace = std::backtrace::Backtrace::force_capture();
        report(&format!("RDM crashed: {info}"), &backtrace.to_string());
    }));
}

fn fatal(message: &str) -> ! {
    report(message, "");
    std::process::exit(1);
}

fn report(message: &str, details: &str) {
    eprintln!("rdm: {message}");
    let log = rdm_core::data_dir().join("crash.log");
    let _ = std::fs::create_dir_all(rdm_core::data_dir());
    let _ = std::fs::write(
        &log,
        format!("RDM {}\n{message}\n\n{details}", env!("CARGO_PKG_VERSION")),
    );
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title("RDM")
        .set_description(format!(
            "{message}\n\nDetails were saved to:\n{}",
            log.display()
        ))
        .show();
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
