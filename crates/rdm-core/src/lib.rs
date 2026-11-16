//! RDM download engine.
//!
//! The [`Engine`] owns a tokio runtime and a list of downloads. The GUI only
//! talks to it through a small synchronous API and polls [`Engine::list`]
//! for snapshots, so no async code leaks into the UI layer.

mod category;
mod download;
mod engine;
mod error;
mod hls;
mod http;
mod limiter;
mod probe;
mod settings;
mod store;
mod tools;
mod torrent;
pub mod util;
mod ytdlp;

pub use category::Category;
pub use download::{
    Download, DownloadId, HlsProgress, Kind, PageOption, RequestInfo, Segment, Status, TorrentFile,
    TorrentMeta,
};
pub use engine::{Engine, Event, NewDownload, ProbeHandle};
pub use error::{Error, Result};
pub use probe::ProbeResult;
pub use settings::Settings;
pub use store::data_dir;
pub use tools::{InstallHandle, Tool};
pub use torrent::is_torrent_source;
