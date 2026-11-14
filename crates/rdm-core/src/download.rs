use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::Category;

pub type DownloadId = u64;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    Queued,
    Downloading,
    Paused,
    Completed,
    Failed(String),
}

impl Status {
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Queued | Self::Downloading)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    /// Plain HTTP(S) file, downloaded in byte-range segments.
    #[default]
    Http,
    /// HLS stream (`.m3u8`), media segments are fetched and joined.
    Hls,
}

/// One byte range `[start, end)` of the file; `done` bytes are on disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Segment {
    pub start: u64,
    pub end: u64,
    pub done: u64,
    /// A connection is currently working on this segment (runtime only).
    #[serde(skip)]
    pub active: bool,
}

impl Segment {
    pub fn new(start: u64, end: u64) -> Self {
        Self {
            start,
            end,
            done: 0,
            active: false,
        }
    }

    /// Absolute file offset of the next byte to write.
    pub fn pos(&self) -> u64 {
        self.start + self.done
    }

    pub fn remaining(&self) -> u64 {
        self.end.saturating_sub(self.pos())
    }

    pub fn is_finished(&self) -> bool {
        self.remaining() == 0
    }
}

/// Extra request data, usually forwarded by the browser extension.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestInfo {
    pub referer: Option<String>,
    pub cookies: Option<String>,
    pub user_agent: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HlsProgress {
    pub total: usize,
    pub done: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Download {
    pub id: DownloadId,
    pub url: String,
    pub file_name: String,
    pub save_dir: PathBuf,
    pub kind: Kind,
    pub category: Category,
    pub status: Status,
    /// Total size in bytes when the server tells us.
    pub size: Option<u64>,
    /// The server accepts range requests, so we can pause/resume and split.
    pub resumable: bool,
    /// Bytes written to disk so far.
    pub downloaded: u64,
    pub segments: Vec<Segment>,
    pub hls: Option<HlsProgress>,
    pub request: RequestInfo,
    /// Unix timestamps (seconds).
    pub added_at: u64,
    pub last_try: Option<u64>,
    pub description: String,

    /// Current transfer rate in bytes/s (runtime only).
    #[serde(skip)]
    pub speed: u64,
    /// Open connections (runtime only).
    #[serde(skip)]
    pub connections: usize,
}

impl Download {
    pub fn path(&self) -> PathBuf {
        self.save_dir.join(&self.file_name)
    }

    /// Temporary file used while downloading.
    pub fn part_path(&self) -> PathBuf {
        self.save_dir.join(format!("{}.rdm-part", self.file_name))
    }

    /// Progress in `0.0..=1.0` if it can be known.
    pub fn progress(&self) -> Option<f32> {
        if self.status == Status::Completed {
            return Some(1.0);
        }
        if let Some(h) = self.hls.filter(|h| h.total > 0) {
            return Some(h.done as f32 / h.total as f32);
        }
        match self.size {
            Some(0) => None,
            Some(size) => Some((self.downloaded as f64 / size as f64).min(1.0) as f32),
            None => None,
        }
    }

    /// Estimated seconds left at the current speed.
    pub fn eta(&self) -> Option<u64> {
        if self.speed == 0 {
            return None;
        }
        if let Some(size) = self.size {
            return Some(size.saturating_sub(self.downloaded) / self.speed);
        }
        let h = self.hls.filter(|h| h.done > 0)?;
        let per_part = self.downloaded / h.done as u64;
        Some(per_part * (h.total - h.done) as u64 / self.speed)
    }

    /// Resets all progress so the file is fetched again from scratch.
    pub(crate) fn reset_progress(&mut self) {
        self.downloaded = 0;
        self.segments.clear();
        if let Some(h) = &mut self.hls {
            h.done = 0;
        }
    }
}

/// A download shared between the engine, its connections and the GUI.
pub(crate) type SharedDownload = std::sync::Arc<std::sync::Mutex<Download>>;
