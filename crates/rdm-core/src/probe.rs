//! Asks the server about a URL before downloading: name, size, range support.

use reqwest::StatusCode;
use reqwest::header::{CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE};

use crate::http::Ctx;
use crate::util::{extension_for_mime, file_name_from_url, parse_content_disposition};
use crate::{Kind, RequestInfo, Result, hls, ytdlp};

#[derive(Clone, Debug)]
pub struct ProbeResult {
    pub url: String,
    pub file_name: String,
    pub size: Option<u64>,
    pub resumable: bool,
    pub kind: Kind,
    pub content_type: Option<String>,
    /// Number of media segments for HLS streams.
    pub hls_segments: Option<usize>,
    /// Name and files of a torrent.
    pub torrent: Option<crate::TorrentMeta>,
    /// Qualities of a web page video, best first (empty for everything else).
    pub page: Vec<crate::PageOption>,
    /// The page only answered with the browser's cookies (signed-in content).
    pub used_cookies: bool,
}

pub(crate) async fn probe(ctx: &Ctx, url: &str, req: &RequestInfo) -> Result<ProbeResult> {
    if crate::is_torrent_source(url) {
        return ctx.torrents.probe(ctx, url, req).await;
    }
    if req.page {
        return ytdlp::probe(ctx, url, req).await;
    }
    let response = ctx.get(url, req).header("Range", "bytes=0-").send().await?;
    let status = response.status();
    if !status.is_success() {
        return Err(crate::Error::Status(status.as_u16()));
    }
    let headers = response.headers();
    let header = |name| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
    };
    let content_type = header(CONTENT_TYPE);

    if content_type
        .as_deref()
        .is_some_and(|t| t.contains("x-bittorrent"))
    {
        drop(response);
        return ctx.torrents.probe(ctx, url, req).await;
    }

    // A web page: maybe it shows a video yt-dlp can fetch. If not, the page
    // itself is downloaded like any other file.
    if content_type
        .as_deref()
        .is_some_and(|t| t.starts_with("text/html"))
    {
        match ytdlp::probe(ctx, url, req).await {
            Ok(page) => return Ok(page),
            Err(e @ crate::Error::MissingTool(_)) if ytdlp::is_video_site(url) => return Err(e),
            Err(_) => {}
        }
    }

    // DASH streams (separate audio/video pieces) are fetched by yt-dlp.
    let is_dash = content_type
        .as_deref()
        .is_some_and(|t| t.to_ascii_lowercase().contains("dash+xml"))
        || url::Url::parse(url).is_ok_and(|u| u.path().to_ascii_lowercase().ends_with(".mpd"));
    if is_dash {
        drop(response);
        return ytdlp::probe(ctx, url, req).await;
    }

    let is_hls = content_type
        .as_deref()
        .is_some_and(|t| t.to_ascii_lowercase().contains("mpegurl"))
        || url::Url::parse(url).is_ok_and(|u| u.path().to_ascii_lowercase().ends_with(".m3u8"));
    if is_hls {
        drop(response);
        return probe_hls(ctx, url, req, content_type).await;
    }

    // "Content-Range: bytes 0-999/1000" -> total = 1000
    let range_total = header(CONTENT_RANGE).and_then(|v| {
        v.rsplit('/')
            .next()
            .and_then(|t| t.trim().parse::<u64>().ok())
    });
    let content_length = header(CONTENT_LENGTH).and_then(|v| v.parse::<u64>().ok());
    let resumable = status == StatusCode::PARTIAL_CONTENT && range_total.is_some();
    let size = if resumable {
        range_total
    } else {
        content_length
    };

    let mut file_name = header(CONTENT_DISPOSITION)
        .and_then(|v| parse_content_disposition(&v))
        .or_else(|| file_name_from_url(response.url().as_str()))
        .or_else(|| file_name_from_url(url))
        .unwrap_or_else(|| "download".to_owned());
    if !file_name.contains('.')
        && let Some(ext) = content_type.as_deref().and_then(extension_for_mime)
    {
        file_name = format!("{file_name}.{ext}");
    }

    Ok(ProbeResult {
        url: url.to_owned(),
        file_name,
        size,
        resumable,
        kind: Kind::Http,
        content_type,
        hls_segments: None,
        torrent: None,
        page: Vec::new(),
        used_cookies: false,
    })
}

async fn probe_hls(
    ctx: &Ctx,
    url: &str,
    req: &RequestInfo,
    content_type: Option<String>,
) -> Result<ProbeResult> {
    let playlist = match hls::load(ctx, url, req).await {
        Ok(playlist) => playlist,
        // What RDM's own HLS downloader cannot do, yt-dlp often can.
        Err(crate::Error::Unsupported(_)) => return ytdlp::probe(ctx, url, req).await,
        Err(e) => return Err(e),
    };
    let stem = file_name_from_url(url)
        .map(|n| {
            n.rsplit_once('.')
                .map_or(n.clone(), |(stem, _)| stem.to_owned())
        })
        .unwrap_or_else(|| "video".to_owned());
    Ok(ProbeResult {
        url: url.to_owned(),
        file_name: format!("{stem}.{}", playlist.extension()),
        size: None,
        resumable: true,
        kind: Kind::Hls,
        content_type,
        hls_segments: Some(playlist.segments.len()),
        torrent: None,
        page: Vec::new(),
        used_cookies: false,
    })
}
