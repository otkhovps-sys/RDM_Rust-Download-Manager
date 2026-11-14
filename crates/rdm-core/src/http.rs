//! Multi-connection HTTP downloader with dynamic segment splitting.
//!
//! The file is cut into byte ranges ("segments"). Every connection claims an
//! unfinished segment; when none is left it steals the second half of the
//! biggest segment still in progress, so all connections stay busy until the
//! very end - the same trick that makes IDM fast.

use std::io::SeekFrom;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use reqwest::header::{COOKIE, RANGE, REFERER, USER_AGENT};
use reqwest::{Client, RequestBuilder, StatusCode};
use tokio::fs::OpenOptions;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio::sync::watch;
use tokio::task::JoinSet;

use crate::download::SharedDownload;
use crate::limiter::Limiter;
use crate::{Download, Error, RequestInfo, Result, Segment};

/// Smallest segment we create; splitting needs twice this much left.
const MIN_PIECE: u64 = 1024 * 1024;
/// Bytes buffered per connection before they are written to disk.
const WRITE_BUFFER: usize = 256 * 1024;

/// Everything a running download needs, cheap to clone.
#[derive(Clone)]
pub(crate) struct Ctx {
    pub client: Client,
    pub limiter: Arc<Limiter>,
    pub user_agent: String,
    pub connections: usize,
    pub max_retries: u32,
    pub torrents: Arc<crate::torrent::TorrentClient>,
}

impl Ctx {
    pub fn get(&self, url: &str, req: &RequestInfo) -> RequestBuilder {
        let ua = req.user_agent.as_deref().unwrap_or(&self.user_agent);
        let mut builder = self.client.get(url).header(USER_AGENT, ua);
        if let Some(referer) = &req.referer {
            builder = builder.header(REFERER, referer);
        }
        if let Some(cookies) = req.cookies.as_deref().filter(|c| !c.is_empty()) {
            builder = builder.header(COOKIE, cookies);
        }
        builder
    }

    /// Runs `op` until it succeeds, retrying transient errors with back-off.
    pub async fn retry<T, F, Fut>(&self, cancel: &mut watch::Receiver<bool>, mut op: F) -> Result<T>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        let mut attempt = 0;
        loop {
            match op().await {
                Err(e) if e.is_transient() && attempt < self.max_retries => {
                    attempt += 1;
                    let delay = Duration::from_secs(u64::from(attempt.min(5)) * 2);
                    tokio::select! {
                        () = tokio::time::sleep(delay) => {}
                        () = cancelled(cancel) => return Err(Error::Cancelled),
                    }
                }
                result => return result,
            }
        }
    }
}

/// Resolves once the download has been asked to stop.
pub(crate) async fn cancelled(rx: &mut watch::Receiver<bool>) {
    let _ = rx.wait_for(|stop| *stop).await;
}

pub(crate) async fn run(ctx: Ctx, dl: SharedDownload, cancel: watch::Receiver<bool>) -> Result<()> {
    let (path, size, fresh, workers) = {
        let mut d = dl.lock().unwrap();
        // The partial file was deleted while paused: start over.
        if d.downloaded > 0 && !d.part_path().exists() {
            d.reset_progress();
        }
        prepare(&mut d, ctx.connections);
        let workers = if d.resumable { ctx.connections } else { 1 };
        (d.part_path(), d.size, d.downloaded == 0, workers)
    };

    if let Some(dir) = path.parent() {
        tokio::fs::create_dir_all(dir).await?;
    }
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(fresh)
        .open(&path)
        .await?;
    if let Some(size) = size {
        file.set_len(size).await?;
    }
    drop(file);

    let mut set = JoinSet::new();
    for _ in 0..workers {
        set.spawn(worker(
            ctx.clone(),
            dl.clone(),
            cancel.clone(),
            path.clone(),
        ));
    }
    let mut error = None;
    while let Some(joined) = set.join_next().await {
        match joined {
            Ok(Ok(())) => {}
            Ok(Err(e)) => error = Some(e),
            Err(e) => error = Some(Error::other(e.to_string())),
        }
    }

    if *cancel.borrow() {
        return Err(Error::Cancelled);
    }
    let finished = dl.lock().unwrap().segments.iter().all(Segment::is_finished);
    match (finished, error) {
        (true, _) => Ok(()),
        (false, Some(e)) => Err(e),
        (false, None) => Err(Error::Incomplete),
    }
}

/// Restores progress after a restart or plans fresh segments.
fn prepare(d: &mut Download, connections: usize) {
    if !d.resumable {
        d.reset_progress();
    }
    if d.segments.is_empty() {
        d.segments = plan(d.size, d.resumable, connections);
    }
    for s in &mut d.segments {
        s.active = false;
    }
    d.downloaded = d.segments.iter().map(|s| s.done).sum();
}

fn plan(size: Option<u64>, resumable: bool, connections: usize) -> Vec<Segment> {
    match size {
        Some(size) if resumable && size > 0 => {
            let n = (size / MIN_PIECE).clamp(1, connections as u64);
            let step = size / n;
            (0..n)
                .map(|i| Segment::new(i * step, if i == n - 1 { size } else { (i + 1) * step }))
                .collect()
        }
        Some(size) => vec![Segment::new(0, size)],
        None => vec![Segment::new(0, u64::MAX)],
    }
}

/// Picks the next segment for a connection, splitting a busy one if needed.
fn claim(d: &mut Download) -> Option<usize> {
    if let Some(i) = d
        .segments
        .iter()
        .position(|s| !s.active && !s.is_finished())
    {
        d.segments[i].active = true;
        return Some(i);
    }
    if !d.resumable {
        return None;
    }
    let (i, remaining) = d
        .segments
        .iter()
        .enumerate()
        .filter(|(_, s)| s.active)
        .map(|(i, s)| (i, s.remaining()))
        .max_by_key(|&(_, remaining)| remaining)?;
    if remaining < 2 * MIN_PIECE {
        return None;
    }
    let busy = &mut d.segments[i];
    let mid = busy.pos() + remaining / 2;
    let mut stolen = Segment::new(mid, busy.end);
    stolen.active = true;
    busy.end = mid;
    d.segments.push(stolen);
    Some(d.segments.len() - 1)
}

/// One connection: keeps claiming segments until nothing is left.
async fn worker(
    ctx: Ctx,
    dl: SharedDownload,
    cancel: watch::Receiver<bool>,
    path: PathBuf,
) -> Result<()> {
    let mut retry_cancel = cancel.clone();
    loop {
        let Some(i) = claim(&mut dl.lock().unwrap()) else {
            return Ok(());
        };
        let result = ctx
            .retry(&mut retry_cancel, || {
                fetch(&ctx, &dl, i, cancel.clone(), &path)
            })
            .await;
        dl.lock().unwrap().segments[i].active = false;
        // On error the segment stays unfinished, so other connections retry it.
        result?;
    }
}

/// Streams one segment into the file.
async fn fetch(
    ctx: &Ctx,
    dl: &SharedDownload,
    i: usize,
    mut cancel: watch::Receiver<bool>,
    path: &Path,
) -> Result<()> {
    let (url, req, pos, end, resumable) = {
        let mut d = dl.lock().unwrap();
        if !d.resumable && d.segments[i].done > 0 {
            // Without range support a broken transfer restarts from zero.
            d.segments[i].done = 0;
            d.downloaded = 0;
        }
        let s = d.segments[i];
        (
            d.url.clone(),
            d.request.clone(),
            s.pos(),
            s.end,
            d.resumable,
        )
    };
    if pos >= end {
        return Ok(());
    }

    let mut request = ctx.get(&url, &req);
    if resumable {
        let range = match end {
            u64::MAX => format!("bytes={pos}-"),
            end => format!("bytes={pos}-{}", end - 1),
        };
        request = request.header(RANGE, range);
    }
    let response = tokio::select! {
        r = request.send() => r?,
        () = cancelled(&mut cancel) => return Err(Error::Cancelled),
    };
    let status = response.status();
    if !status.is_success() {
        return Err(Error::Status(status.as_u16()));
    }
    if resumable && status != StatusCode::PARTIAL_CONTENT {
        return Err(Error::other("the server no longer supports resuming"));
    }

    let mut file = OpenOptions::new().write(true).open(path).await?;
    file.seek(SeekFrom::Start(pos)).await?;
    let mut writer = SegmentWriter {
        file,
        dl,
        i,
        pos,
        buf: Vec::with_capacity(WRITE_BUFFER),
    };

    dl.lock().unwrap().connections += 1;
    let mut stream = response.bytes_stream();
    let result = loop {
        let chunk = tokio::select! {
            chunk = stream.next() => chunk,
            () = cancelled(&mut cancel) => break Err(Error::Cancelled),
        };
        match chunk {
            Some(Ok(bytes)) => {
                ctx.limiter.acquire(bytes.len()).await;
                match writer.push(&bytes).await {
                    Ok(false) => {}
                    Ok(true) => break Ok(()),
                    Err(e) => break Err(e),
                }
            }
            Some(Err(e)) => break Err(e.into()),
            None => break writer.end_of_stream().await,
        }
    };
    dl.lock().unwrap().connections -= 1;
    // Always keep what was received, even when stopping or failing.
    let flushed = writer.finish().await;
    result.and(flushed)
}

/// Buffers incoming bytes and records progress once they are written.
struct SegmentWriter<'a> {
    file: tokio::fs::File,
    dl: &'a SharedDownload,
    i: usize,
    /// File offset where `buf` starts.
    pos: u64,
    buf: Vec<u8>,
}

impl SegmentWriter<'_> {
    fn end(&self) -> u64 {
        self.dl.lock().unwrap().segments[self.i].end
    }

    /// Returns `true` once the segment is complete. The end may move closer
    /// while we download because another connection stole part of it.
    async fn push(&mut self, bytes: &[u8]) -> Result<bool> {
        let end = self.end();
        let room = end.saturating_sub(self.pos + self.buf.len() as u64);
        let take = (bytes.len() as u64).min(room) as usize;
        self.buf.extend_from_slice(&bytes[..take]);
        let reached = self.pos + self.buf.len() as u64 >= end;
        if reached || self.buf.len() >= WRITE_BUFFER {
            self.write_buffer().await?;
        }
        Ok(reached)
    }

    async fn write_buffer(&mut self) -> Result<()> {
        if self.buf.is_empty() {
            return Ok(());
        }
        self.file.write_all(&self.buf).await?;
        let n = self.buf.len() as u64;
        self.buf.clear();
        self.pos += n;
        let mut d = self.dl.lock().unwrap();
        d.segments[self.i].done += n;
        d.downloaded += n;
        Ok(())
    }

    /// The server closed the stream: fine for unknown sizes, an error otherwise.
    async fn end_of_stream(&mut self) -> Result<()> {
        self.write_buffer().await?;
        let mut d = self.dl.lock().unwrap();
        let segment = &mut d.segments[self.i];
        if segment.end == u64::MAX {
            segment.end = segment.pos();
            d.size = Some(d.downloaded);
        }
        if d.segments[self.i].is_finished() {
            Ok(())
        } else {
            Err(Error::Incomplete)
        }
    }

    async fn finish(mut self) -> Result<()> {
        self.write_buffer().await?;
        self.file.flush().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans_segments() {
        let segs = plan(Some(10 * MIN_PIECE), true, 4);
        assert_eq!(segs.len(), 4);
        assert_eq!(segs[0].start, 0);
        assert_eq!(segs[3].end, 10 * MIN_PIECE);
        assert!(segs.windows(2).all(|w| w[0].end == w[1].start));
        assert_eq!(plan(Some(100), true, 8).len(), 1);
        assert_eq!(plan(None, false, 8)[0].end, u64::MAX);
    }
}
