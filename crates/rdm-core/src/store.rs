//! JSON persistence of settings and the download list.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::{Download, Settings};

pub(crate) struct Store {
    dir: PathBuf,
}

impl Store {
    /// Data folder, in order of preference:
    /// 1. `RDM_DATA_DIR` environment variable,
    /// 2. `data/` next to the executable when a `portable` file is there,
    /// 3. the per-user data folder of the OS.
    pub fn new() -> Self {
        let portable = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|p| p.to_path_buf()))
            .filter(|dir| dir.join("portable").exists())
            .map(|dir| dir.join("data"));
        let dir = std::env::var_os("RDM_DATA_DIR")
            .map(PathBuf::from)
            .or(portable)
            .or_else(|| {
                directories::ProjectDirs::from("", "", "rdm").map(|d| d.data_dir().to_path_buf())
            })
            .unwrap_or_else(|| PathBuf::from(".rdm"));
        Self { dir }
    }

    pub fn load_settings(&self) -> Settings {
        load(&self.dir.join("settings.json")).unwrap_or_default()
    }

    pub fn save_settings(&self, settings: &Settings) {
        save(&self.dir.join("settings.json"), settings);
    }

    pub fn load_downloads(&self) -> Vec<Download> {
        load(&self.dir.join("downloads.json")).unwrap_or_default()
    }

    pub fn save_downloads(&self, downloads: &[Download]) {
        save(&self.dir.join("downloads.json"), &downloads);
    }
}

fn load<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text)
        .inspect_err(|e| eprintln!("rdm: ignoring corrupt {}: {e}", path.display()))
        .ok()
}

/// Writes to a temp file first so a crash never leaves a half written file.
fn save<T: Serialize + ?Sized>(path: &Path, value: &T) {
    let result = (|| -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(value)?)?;
        fs::rename(tmp, path)
    })();
    if let Err(e) = result {
        eprintln!("rdm: failed to save {}: {e}", path.display());
    }
}
