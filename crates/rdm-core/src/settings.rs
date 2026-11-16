use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::Category;

/// File types the browser extension hands over to RDM (like IDM's list).
const DEFAULT_CAPTURE_TYPES: &str = "3gp 7z aac ace aif apk arj asf avi bin bz2 deb dmg exe flac \
flv gz gzip img iso jar lzh m4a m4v mkv mov mp3 mp4 mpeg mpg msi msu ogg opus pdf pkg ppt pptx \
qt r00 r01 rar rm rmvb rpm sit tar tgz tif torrent ts wav webm wma wmv xz z zip zst";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Base folder for downloads.
    pub download_dir: PathBuf,
    /// Save each category in its own sub folder (`Downloads/Video`, ...).
    pub category_subfolders: bool,
    /// Connections per download.
    pub connections: usize,
    /// Downloads running at the same time; the rest wait in the queue.
    pub max_active: usize,
    /// Global speed limit in KiB/s, `0` = unlimited.
    pub speed_limit_kib: u64,
    /// Times a broken connection is retried before giving up.
    pub max_retries: u32,
    /// Let the extension take over browser downloads.
    pub browser_integration: bool,
    /// Space separated extensions the extension should capture.
    pub capture_types: String,
    /// Show the "Download complete" dialog.
    pub show_complete_dialog: bool,
    /// Default user agent when the browser did not provide one.
    pub user_agent: String,
    /// Dark window theme instead of the classic light one.
    pub dark_mode: bool,
    /// Closing the window keeps RDM running in the system tray.
    pub close_to_tray: bool,
    /// The "Connect your browser" guide was shown once.
    pub browser_setup_done: bool,
    /// Picture shown behind the main window, if any.
    pub background_image: Option<PathBuf>,
    /// How strongly the picture shows through, in percent.
    pub background_strength: u8,
}

impl Default for Settings {
    fn default() -> Self {
        let download_dir = directories::UserDirs::new()
            .and_then(|d| d.download_dir().map(|p| p.to_path_buf()))
            .or_else(|| directories::UserDirs::new().map(|d| d.home_dir().join("Downloads")))
            .unwrap_or_else(|| PathBuf::from("Downloads"));
        Self {
            download_dir,
            category_subfolders: true,
            connections: 8,
            max_active: 3,
            speed_limit_kib: 0,
            max_retries: 8,
            browser_integration: true,
            capture_types: DEFAULT_CAPTURE_TYPES.to_owned(),
            show_complete_dialog: true,
            user_agent: format!(
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) RDM/{}",
                env!("CARGO_PKG_VERSION")
            ),
            dark_mode: false,
            close_to_tray: true,
            browser_setup_done: false,
            background_image: None,
            background_strength: 30,
        }
    }
}

impl Settings {
    /// Settings saved on disk (defaults when there are none yet).
    pub fn load() -> Self {
        crate::store::Store::new().load_settings().normalized()
    }

    pub fn save_dir_for(&self, category: Category) -> PathBuf {
        if self.category_subfolders {
            self.download_dir.join(category.label())
        } else {
            self.download_dir.clone()
        }
    }

    /// Keeps numbers in sane ranges after the user edited them.
    pub(crate) fn normalized(mut self) -> Self {
        self.connections = self.connections.clamp(1, 32);
        self.max_active = self.max_active.clamp(1, 20);
        self.max_retries = self.max_retries.min(100);
        self.background_strength = self.background_strength.min(100);
        self
    }
}
