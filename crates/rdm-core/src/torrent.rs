//! BitTorrent downloads (magnet links, `.torrent` files and URLs) on top of
//! librqbit. The torrent session is only started when it is first needed.

use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use librqbit::dht::{DhtPersistenceConfig, Id20};
use librqbit::limits::LimitsConfig;
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, DhtSessionConfig, ListenerOptions, Session,
    SessionOptions, TorrentStatsState,
};
use tokio::sync::{OnceCell, watch};

use crate::download::SharedDownload;
use crate::http::{Ctx, cancelled};
use crate::util::sanitize_file_name;
use crate::{Error, Kind, ProbeResult, RequestInfo, Result, TorrentFile, TorrentMeta};

/// Magnet links can take a while: the metadata comes from other peers.
const METADATA_TIMEOUT: Duration = Duration::from_secs(180);

/// `true` for magnet links and `.torrent` files or URLs.
pub fn is_torrent_source(source: &str) -> bool {
    let lower = source.trim().to_ascii_lowercase();
    let path = lower.split(['?', '#']).next().unwrap_or_default();
    lower.starts_with("magnet:") || path.ends_with(".torrent")
}

pub(crate) struct TorrentClient {
    /// Holds cached `.torrent` files and the DHT state.
    dir: PathBuf,
    session: OnceCell<Arc<Session>>,
}

impl TorrentClient {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            session: OnceCell::new(),
        }
    }

    async fn session(&self) -> Result<&Arc<Session>> {
        self.session
            .get_or_try_init(|| async {
                tokio::fs::create_dir_all(&self.dir).await?;
                let opts = SessionOptions {
                    dht: Some(DhtSessionConfig {
                        persistence: Some(DhtPersistenceConfig {
                            dump_interval: None,
                            config_filename: Some(self.dir.join("dht.json")),
                        }),
                        ..Default::default()
                    }),
                    listen: Some(ListenerOptions::default()),
                    client_name_and_version: Some(format!("RDM {}", env!("CARGO_PKG_VERSION"))),
                    ..Default::default()
                };
                Session::new_with_opts(self.dir.clone(), opts)
                    .await
                    .map_err(torrent_error)
            })
            .await
    }

    fn cached_file(&self, info_hash: &str) -> PathBuf {
        self.dir.join(format!("{info_hash}.torrent"))
    }

    /// Reads name and file list without downloading anything.
    pub async fn probe(&self, ctx: &Ctx, source: &str, req: &RequestInfo) -> Result<ProbeResult> {
        let add = load_source(ctx, source, req).await?;
        let session = self.session().await?;
        let options = AddTorrentOptions {
            list_only: true,
            ..Default::default()
        };
        let response =
            tokio::time::timeout(METADATA_TIMEOUT, session.add_torrent(add, Some(options)))
                .await
                .map_err(|_| Error::other("no peers answered with the torrent metadata"))?
                .map_err(torrent_error)?;
        let AddTorrentResponse::ListOnly(list) = response else {
            return Err(Error::other("unexpected answer from the torrent session"));
        };

        let info_hash = list.info_hash.as_string();
        tokio::fs::write(self.cached_file(&info_hash), &list.torrent_bytes).await?;
        let files: Vec<TorrentFile> = list
            .info
            .iter_file_details()
            .enumerate()
            .filter(|(_, f)| !f.attrs().padding)
            .map(|(index, f)| TorrentFile {
                index,
                path: f.filename.to_pathbuf().to_string_lossy().into_owned(),
                size: f.len,
                selected: true,
            })
            .collect();
        let name = list
            .info
            .name()
            .map_or_else(|| info_hash.clone(), |n| n.into_owned());
        let meta = TorrentMeta {
            info_hash,
            files,
            multi_file: list.info.info().files.is_some(),
        };
        Ok(ProbeResult {
            url: source.trim().to_owned(),
            file_name: sanitize_file_name(&name),
            size: Some(meta.selected_size()),
            resumable: true,
            kind: Kind::Torrent,
            content_type: Some("application/x-bittorrent".into()),
            hls_segments: None,
            torrent: Some(meta),
            page: Vec::new(),
        })
    }

    pub async fn run(
        &self,
        ctx: Ctx,
        dl: SharedDownload,
        mut cancel: watch::Receiver<bool>,
    ) -> Result<()> {
        let (url, req, meta, output) = {
            let d = dl.lock().unwrap();
            let meta = d
                .torrent
                .clone()
                .ok_or_else(|| Error::other("missing torrent data"))?;
            let output = if meta.multi_file {
                d.path()
            } else {
                d.save_dir.clone()
            };
            (d.url.clone(), d.request.clone(), meta, output)
        };
        let id: Id20 = meta.info_hash.parse().map_err(torrent_error)?;
        let session = self.session().await?;

        // A torrent paused earlier in this session is still known to it.
        let handle = match session.get(id.into()) {
            Some(handle) => {
                if handle.is_paused() {
                    session.unpause(&handle).await.map_err(torrent_error)?;
                }
                handle
            }
            None => {
                let add = match tokio::fs::read(self.cached_file(&meta.info_hash)).await {
                    Ok(bytes) => AddTorrent::from_bytes(bytes),
                    Err(_) => load_source(&ctx, &url, &req).await?,
                };
                let options = AddTorrentOptions {
                    output_folder: Some(output.to_string_lossy().into_owned()),
                    only_files: Some(
                        meta.files
                            .iter()
                            .filter(|f| f.selected)
                            .map(|f| f.index)
                            .collect(),
                    ),
                    // Lets a restarted download continue on the data already on disk.
                    overwrite: true,
                    ratelimits: LimitsConfig {
                        download_bps: NonZeroU32::new(
                            ctx.limiter.rate().min(u64::from(u32::MAX)) as u32
                        ),
                        upload_bps: None,
                    },
                    ..Default::default()
                };
                session
                    .add_torrent(add, Some(options))
                    .await
                    .map_err(torrent_error)?
                    .into_handle()
                    .ok_or_else(|| Error::other("the torrent could not be started"))?
            }
        };

        loop {
            let stats = handle.stats();
            {
                let mut d = dl.lock().unwrap();
                d.downloaded = stats.progress_bytes;
                d.size = Some(stats.total_bytes);
                d.connections = stats
                    .live
                    .as_ref()
                    .map_or(0, |l| l.snapshot.peer_stats.live as usize);
            }
            if stats.finished {
                // Downloads stop when complete, like every other download.
                let _ = session.delete(id.into(), false).await;
                return Ok(());
            }
            if matches!(stats.state, TorrentStatsState::Error) {
                let _ = session.delete(id.into(), false).await;
                return Err(Error::other(
                    stats.error.unwrap_or_else(|| "torrent error".into()),
                ));
            }
            tokio::select! {
                () = tokio::time::sleep(Duration::from_millis(500)) => {}
                () = cancelled(&mut cancel) => {
                    let _ = session.pause(&handle).await;
                    return Err(Error::Cancelled);
                }
            }
        }
    }

    /// Drops a torrent from the session (files stay on disk).
    pub async fn forget(&self, info_hash: &str) {
        if let (Some(session), Ok(id)) = (self.session.get(), info_hash.parse::<Id20>()) {
            let _ = session.delete(id.into(), false).await;
        }
    }
}

/// Magnet link, `.torrent` URL or local `.torrent` file.
async fn load_source(ctx: &Ctx, source: &str, req: &RequestInfo) -> Result<AddTorrent<'static>> {
    let source = source.trim();
    if source.to_ascii_lowercase().starts_with("magnet:") {
        return Ok(AddTorrent::from_url(source.to_owned()));
    }
    if source.starts_with("http://") || source.starts_with("https://") {
        let response = ctx.get(source, req).send().await?;
        if !response.status().is_success() {
            return Err(Error::Status(response.status().as_u16()));
        }
        return Ok(AddTorrent::from_bytes(response.bytes().await?));
    }
    Ok(AddTorrent::from_bytes(tokio::fs::read(source).await?))
}

/// Keeps the whole cause chain ("a: b: c") so the user sees the real reason.
fn torrent_error(e: impl std::fmt::Display) -> Error {
    Error::other(format!("torrent: {e:#}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_sources() {
        assert!(is_torrent_source("magnet:?xt=urn:btih:abc"));
        assert!(is_torrent_source("https://x.org/ubuntu.iso.torrent?k=1"));
        assert!(is_torrent_source("C:\\\\Users\\\\me\\\\file.TORRENT"));
        assert!(!is_torrent_source("https://x.org/file.zip"));
    }
}
