//! HLS (`.m3u8`) support: picks the best quality variant, downloads the media
//! segments in parallel (keeping their order) and joins them into one file.
//! AES-128 encrypted streams are decrypted on the fly.

use std::collections::HashMap;
use std::io::SeekFrom;
use std::sync::Arc;

use aes::cipher::{BlockDecryptMut, KeyIvInit, block_padding::Pkcs7};
use futures_util::{StreamExt, stream};
use tokio::fs::OpenOptions;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio::sync::watch;
use url::Url;

use crate::download::SharedDownload;
use crate::http::{Ctx, cancelled};
use crate::{Error, HlsProgress, RequestInfo, Result};

type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;

pub(crate) struct Playlist {
    pub segments: Vec<MediaSegment>,
    /// fMP4 initialization section (`#EXT-X-MAP`).
    pub init: Option<String>,
}

pub(crate) struct MediaSegment {
    url: String,
    sequence: u64,
    key: Option<Key>,
}

#[derive(Clone)]
struct Key {
    url: String,
    iv: Option<[u8; 16]>,
}

impl Playlist {
    pub fn extension(&self) -> &'static str {
        if self.init.is_some() { "mp4" } else { "ts" }
    }
}

/// Downloads a playlist, following a master playlist to its best variant.
pub(crate) async fn load(ctx: &Ctx, url: &str, req: &RequestInfo) -> Result<Playlist> {
    let mut url = Url::parse(url)?;
    for _ in 0..3 {
        let bytes = fetch(ctx, url.as_str(), req).await?;
        let text = String::from_utf8_lossy(&bytes);
        if !text.trim_start().starts_with("#EXTM3U") {
            return Err(Error::other("not an HLS playlist"));
        }
        match best_variant(&text, &url)? {
            Some(variant) => url = variant,
            None => return parse_media(&text, &url),
        }
    }
    Err(Error::other("too many nested playlists"))
}

pub(crate) async fn run(
    ctx: Ctx,
    dl: SharedDownload,
    mut cancel: watch::Receiver<bool>,
) -> Result<()> {
    let (url, req, path) = {
        let d = dl.lock().unwrap();
        (d.url.clone(), d.request.clone(), d.part_path())
    };
    let playlist = ctx
        .retry(&mut cancel.clone(), || load(&ctx, &url, &req))
        .await?;
    let total = playlist.segments.len();
    if total == 0 {
        return Err(Error::other("the playlist has no media segments"));
    }

    // Resume after the last complete segment, or start over if the stream changed.
    let on_disk = tokio::fs::metadata(&path).await.map_or(0, |m| m.len());
    let (start, offset) = {
        let mut d = dl.lock().unwrap();
        let missing_data = on_disk < d.downloaded;
        let progress = d.hls.get_or_insert(HlsProgress::default());
        if progress.total != total || missing_data {
            *progress = HlsProgress { total, done: 0 };
        }
        let done = progress.done;
        if done == 0 {
            d.downloaded = 0;
        }
        (done, d.downloaded)
    };

    if let Some(dir) = path.parent() {
        tokio::fs::create_dir_all(dir).await?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&path)
        .await?;
    file.set_len(offset).await?;
    file.seek(SeekFrom::Start(offset)).await?;

    if start == 0
        && let Some(init) = &playlist.init
    {
        let data = ctx.retry(&mut cancel, || fetch(&ctx, init, &req)).await?;
        file.write_all(&data).await?;
        dl.lock().unwrap().downloaded += data.len() as u64;
    }

    let keys = Arc::new(fetch_keys(&ctx, &req, &playlist, &mut cancel).await?);
    dl.lock().unwrap().connections = ctx.connections;
    // `buffered` runs several requests at once but yields them in order.
    let segment_cancel = cancel.clone();
    let mut parts = stream::iter(playlist.segments.into_iter().skip(start))
        .map(|segment| {
            let (ctx, req, keys) = (ctx.clone(), req.clone(), keys.clone());
            let cancel = segment_cancel.clone();
            async move { fetch_segment(&ctx, &req, &segment, &keys, cancel).await }
        })
        .buffered(ctx.connections);

    let result = loop {
        let next = tokio::select! {
            next = parts.next() => next,
            () = cancelled(&mut cancel) => break Err(Error::Cancelled),
        };
        match next {
            None => break Ok(()),
            Some(Err(e)) => break Err(e),
            Some(Ok(data)) => {
                if let Err(e) = file.write_all(&data).await {
                    break Err(e.into());
                }
                let mut d = dl.lock().unwrap();
                d.downloaded += data.len() as u64;
                if let Some(h) = &mut d.hls {
                    h.done += 1;
                }
            }
        }
    };
    dl.lock().unwrap().connections = 0;
    file.flush().await?;
    result
}

async fn fetch(ctx: &Ctx, url: &str, req: &RequestInfo) -> Result<bytes::Bytes> {
    let response = ctx.get(url, req).send().await?;
    if !response.status().is_success() {
        return Err(Error::Status(response.status().as_u16()));
    }
    Ok(response.bytes().await?)
}

async fn fetch_keys(
    ctx: &Ctx,
    req: &RequestInfo,
    playlist: &Playlist,
    cancel: &mut watch::Receiver<bool>,
) -> Result<HashMap<String, [u8; 16]>> {
    let mut keys = HashMap::new();
    for key in playlist.segments.iter().filter_map(|s| s.key.as_ref()) {
        if keys.contains_key(&key.url) {
            continue;
        }
        let data = ctx.retry(cancel, || fetch(ctx, &key.url, req)).await?;
        let key_bytes: [u8; 16] = data
            .as_ref()
            .try_into()
            .map_err(|_| Error::other("invalid AES-128 key"))?;
        keys.insert(key.url.clone(), key_bytes);
    }
    Ok(keys)
}

async fn fetch_segment(
    ctx: &Ctx,
    req: &RequestInfo,
    segment: &MediaSegment,
    keys: &HashMap<String, [u8; 16]>,
    mut cancel: watch::Receiver<bool>,
) -> Result<Vec<u8>> {
    let data = ctx
        .retry(&mut cancel, || fetch(ctx, &segment.url, req))
        .await?;
    ctx.limiter.acquire(data.len()).await;
    let Some(key) = &segment.key else {
        return Ok(data.to_vec());
    };
    let iv = key
        .iv
        .unwrap_or_else(|| u128::from(segment.sequence).to_be_bytes());
    Aes128CbcDec::new(&keys[&key.url].into(), &iv.into())
        .decrypt_padded_vec_mut::<Pkcs7>(&data)
        .map_err(|_| Error::other("failed to decrypt an HLS segment"))
}

/// Returns the highest bandwidth variant of a master playlist.
fn best_variant(text: &str, base: &Url) -> Result<Option<Url>> {
    // Sound delivered as its own playlist would be missing from the file.
    let separate_audio = text.lines().any(|line| {
        line.trim()
            .strip_prefix("#EXT-X-MEDIA:")
            .is_some_and(|attrs| {
                attribute(attrs, "TYPE").as_deref() == Some("AUDIO")
                    && attribute(attrs, "URI").is_some()
            })
    });
    if separate_audio {
        return Err(Error::Unsupported(
            "this HLS stream has separate audio and video tracks".into(),
        ));
    }
    let mut best: Option<(u64, &str)> = None;
    let mut lines = text.lines().map(str::trim);
    while let Some(line) = lines.next() {
        let Some(attrs) = line.strip_prefix("#EXT-X-STREAM-INF:") else {
            continue;
        };
        let bandwidth = attribute(attrs, "BANDWIDTH")
            .and_then(|b| b.parse().ok())
            .unwrap_or(0);
        let Some(uri) = lines
            .by_ref()
            .find(|l| !l.is_empty() && !l.starts_with('#'))
        else {
            break;
        };
        if best.is_none_or(|(b, _)| bandwidth > b) {
            best = Some((bandwidth, uri));
        }
    }
    Ok(match best {
        Some((_, uri)) => Some(base.join(uri)?),
        None => None,
    })
}

fn parse_media(text: &str, base: &Url) -> Result<Playlist> {
    let mut playlist = Playlist {
        segments: Vec::new(),
        init: None,
    };
    let mut sequence = 0;
    let mut key = None;
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if let Some(value) = line.strip_prefix("#EXT-X-MEDIA-SEQUENCE:") {
            sequence = value.parse().unwrap_or(0);
        } else if let Some(attrs) = line.strip_prefix("#EXT-X-KEY:") {
            key = parse_key(attrs, base)?;
        } else if let Some(attrs) = line.strip_prefix("#EXT-X-MAP:") {
            if attribute(attrs, "BYTERANGE").is_some() {
                return Err(Error::Unsupported(
                    "HLS byte-range init sections are not supported".into(),
                ));
            }
            let uri =
                attribute(attrs, "URI").ok_or_else(|| Error::other("EXT-X-MAP without URI"))?;
            playlist.init = Some(base.join(&uri)?.to_string());
        } else if line.starts_with("#EXT-X-BYTERANGE") {
            return Err(Error::Unsupported(
                "HLS byte-range playlists are not supported".into(),
            ));
        } else if !line.starts_with('#') {
            playlist.segments.push(MediaSegment {
                url: base.join(line)?.to_string(),
                sequence,
                key: key.clone(),
            });
            sequence += 1;
        }
    }
    Ok(playlist)
}

fn parse_key(attrs: &str, base: &Url) -> Result<Option<Key>> {
    match attribute(attrs, "METHOD").as_deref() {
        None | Some("NONE") => Ok(None),
        Some("AES-128") => {
            let uri =
                attribute(attrs, "URI").ok_or_else(|| Error::other("EXT-X-KEY without URI"))?;
            let iv = attribute(attrs, "IV").and_then(|hex| parse_iv(&hex));
            Ok(Some(Key {
                url: base.join(&uri)?.to_string(),
                iv,
            }))
        }
        Some(other) => Err(Error::Unsupported(format!(
            "HLS encryption {other} is not supported (DRM)"
        ))),
    }
}

fn parse_iv(hex: &str) -> Option<[u8; 16]> {
    let hex = hex.trim_start_matches("0x").trim_start_matches("0X");
    u128::from_str_radix(hex, 16).ok().map(u128::to_be_bytes)
}

/// Reads `NAME=value` or `NAME="value"` from an attribute list.
fn attribute(list: &str, name: &str) -> Option<String> {
    let mut rest = list;
    while !rest.is_empty() {
        let (key, after) = rest.split_once('=')?;
        let (value, next) = match after.strip_prefix('"') {
            Some(quoted) => {
                let (value, next) = quoted.split_once('"')?;
                (value, next.trim_start_matches(','))
            }
            None => after.split_once(',').unwrap_or((after, "")),
        };
        if key.trim().eq_ignore_ascii_case(name) {
            return Some(value.to_owned());
        }
        rest = next;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_attributes() {
        let list = r#"METHOD=AES-128,URI="https://k.example/key?a=1,b=2",IV=0x01"#;
        assert_eq!(attribute(list, "METHOD").as_deref(), Some("AES-128"));
        assert_eq!(
            attribute(list, "URI").as_deref(),
            Some("https://k.example/key?a=1,b=2")
        );
        assert_eq!(attribute(list, "IV").as_deref(), Some("0x01"));
        assert_eq!(parse_iv("0x01").unwrap()[15], 1);
    }

    #[test]
    fn picks_best_variant() {
        let base = Url::parse("https://v.example/live/master.m3u8").unwrap();
        let master = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=800000\nlow.m3u8\n\
                      #EXT-X-STREAM-INF:BANDWIDTH=5000000,RESOLUTION=1920x1080\nhd/index.m3u8\n";
        let best = best_variant(master, &base).unwrap().unwrap();
        assert_eq!(best.as_str(), "https://v.example/live/hd/index.m3u8");
    }

    #[test]
    fn parses_media_playlist() {
        let base = Url::parse("https://v.example/a/index.m3u8").unwrap();
        let text = "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:7\n#EXT-X-KEY:METHOD=AES-128,URI=\"k.bin\"\n\
                    #EXTINF:4,\ns1.ts\n#EXTINF:4,\nhttps://cdn.example/s2.ts\n#EXT-X-ENDLIST\n";
        let p = parse_media(text, &base).unwrap();
        assert_eq!(p.segments.len(), 2);
        assert_eq!(p.segments[0].url, "https://v.example/a/s1.ts");
        assert_eq!(p.segments[1].sequence, 8);
        assert_eq!(
            p.segments[0].key.as_ref().unwrap().url,
            "https://v.example/a/k.bin"
        );
        assert_eq!(p.extension(), "ts");
    }
}
