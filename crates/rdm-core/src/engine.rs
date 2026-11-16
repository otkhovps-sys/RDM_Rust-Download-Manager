//! The download manager: queue, scheduling, persistence and the public API.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use reqwest::Client;
use tokio::runtime::{Handle, Runtime};
use tokio::sync::{oneshot, watch};
use tokio::task::JoinHandle;

use crate::download::SharedDownload;
use crate::http::Ctx;
use crate::limiter::Limiter;
use crate::store::Store;
use crate::tools::{InstallHandle, Tool, Tools};
use crate::torrent::TorrentClient;
use crate::util::{now_secs, sanitize_file_name, unique_file_name};
use crate::{
    Category, Download, DownloadId, Error, Kind, PageOption, ProbeResult, RequestInfo, Result,
    Settings, Status, TorrentMeta,
};

const TICK: Duration = Duration::from_millis(500);
const SAVE_EVERY_TICKS: u32 = 4;

/// Something the GUI may want to react to (e.g. "Download complete" dialog).
#[derive(Clone, Debug)]
pub enum Event {
    Completed(DownloadId),
    Failed(DownloadId, String),
}

/// What the user confirmed in the "Download File Info" dialog.
#[derive(Clone, Debug)]
pub struct NewDownload {
    pub url: String,
    pub file_name: String,
    pub save_dir: PathBuf,
    pub size: Option<u64>,
    pub resumable: bool,
    pub kind: Kind,
    pub category: Category,
    pub request: RequestInfo,
    pub description: String,
    /// Torrent files and which of them to download.
    pub torrent: Option<TorrentMeta>,
    /// Chosen quality of a web page video.
    pub page: Option<PageOption>,
    /// `false` = "Download Later" (added paused).
    pub start: bool,
}

impl NewDownload {
    pub fn from_probe(probe: &ProbeResult, mut request: RequestInfo, settings: &Settings) -> Self {
        // Browser cookies are kept only for pages that need them.
        if !probe.used_cookies {
            request.cookie_file = None;
        }
        let category = match (probe.kind, &probe.torrent) {
            (Kind::Hls | Kind::Page, _) => Category::Video,
            // A torrent is filed under the type of its biggest file.
            (Kind::Torrent, Some(meta)) => meta
                .files
                .iter()
                .max_by_key(|f| f.size)
                .map_or(Category::Other, |f| Category::from_file_name(&f.path)),
            _ => Category::from_file_name(&probe.file_name),
        };
        let mut new = Self {
            url: probe.url.clone(),
            file_name: probe.file_name.clone(),
            save_dir: settings.save_dir_for(category),
            size: probe.size,
            resumable: probe.resumable,
            kind: probe.kind,
            category,
            request,
            description: String::new(),
            torrent: probe.torrent.clone(),
            page: None,
            start: true,
        };
        // Full HD is a sensible default for videos; 4K files are huge.
        let default = probe
            .page
            .iter()
            .find(|o| o.height.is_none_or(|h| h <= 1080))
            .or(probe.page.first());
        if let Some(option) = default {
            new.set_quality(option.clone());
            new.save_dir = settings.save_dir_for(new.category);
        }
        new
    }

    /// Picks the quality of a web page video; name, size and category follow.
    pub fn set_quality(&mut self, option: PageOption) {
        // Keep the name (the user may have edited it), only swap the extension.
        let stem = self
            .file_name
            .rsplit_once('.')
            .map_or(self.file_name.as_str(), |(stem, _)| stem);
        self.file_name = format!("{stem}.{}", option.ext);
        self.size = option.size;
        self.category = if option.audio_only {
            Category::Music
        } else {
            Category::Video
        };
        self.page = Some(option);
    }
}

/// Result of [`Engine::probe`], polled by the GUI without blocking.
pub struct ProbeHandle(oneshot::Receiver<Result<ProbeResult>>);

impl ProbeHandle {
    /// `None` while the server is still being asked.
    pub fn try_take(&mut self) -> Option<Result<ProbeResult>> {
        match self.0.try_recv() {
            Ok(result) => Some(result),
            Err(oneshot::error::TryRecvError::Empty) => None,
            Err(oneshot::error::TryRecvError::Closed) => Some(Err(Error::Cancelled)),
        }
    }
}

/// Cheap to clone handle to the download manager.
#[derive(Clone)]
pub struct Engine {
    inner: Arc<Inner>,
    _runtime: Arc<Runtime>,
}

struct Inner {
    handle: Handle,
    client: Client,
    limiter: Arc<Limiter>,
    store: Store,
    torrents: Arc<TorrentClient>,
    tools: Arc<Tools>,
    settings: Mutex<Settings>,
    jobs: Mutex<Vec<Job>>,
    next_id: AtomicU64,
    events: Mutex<Vec<Event>>,
    waker: Mutex<Option<Box<dyn Fn() + Send + Sync>>>,
    dirty: AtomicBool,
}

struct Job {
    data: SharedDownload,
    running: Option<Running>,
    /// Bytes at the previous tick, for the speed meter.
    last_bytes: u64,
}

struct Running {
    cancel: watch::Sender<bool>,
    task: JoinHandle<()>,
}

impl Engine {
    pub fn new() -> Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .thread_name("rdm-worker")
            .enable_all()
            .build()?;
        let client = Client::builder()
            // Separate TCP connections are the whole point of segmenting,
            // so never let HTTP/2 multiplex them into one.
            .http1_only()
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()?;

        let store = Store::new();
        let settings = store.load_settings().normalized();
        let downloads = store.load_downloads();
        let next_id = downloads.iter().map(|d| d.id).max().unwrap_or(0) + 1;
        let jobs = downloads.into_iter().map(Job::restored).collect();

        let inner = Arc::new(Inner {
            handle: runtime.handle().clone(),
            client,
            limiter: Arc::new(Limiter::new(settings.speed_limit_kib * 1024)),
            torrents: Arc::new(TorrentClient::new(store.torrent_dir())),
            tools: Arc::new(Tools::new(store.tools_dir())),
            store,
            settings: Mutex::new(settings),
            jobs: Mutex::new(jobs),
            next_id: AtomicU64::new(next_id),
            events: Mutex::new(Vec::new()),
            waker: Mutex::new(None),
            dirty: AtomicBool::new(false),
        });
        runtime.spawn(scheduler(inner.clone()));
        let updater = inner.clone();
        runtime.spawn(async move { updater.tools.update_ytdlp().await });
        Ok(Self {
            inner,
            _runtime: Arc::new(runtime),
        })
    }

    /// Called (from any thread) whenever an [`Event`] is queued.
    pub fn set_waker(&self, waker: impl Fn() + Send + Sync + 'static) {
        *self.inner.waker.lock().unwrap() = Some(Box::new(waker));
    }

    pub fn settings(&self) -> Settings {
        self.inner.settings.lock().unwrap().clone()
    }

    pub fn set_settings(&self, settings: Settings) {
        let settings = settings.normalized();
        self.inner.limiter.set_rate(settings.speed_limit_kib * 1024);
        self.inner.store.save_settings(&settings);
        *self.inner.settings.lock().unwrap() = settings;
    }

    /// Asks the server for name, size and resume support in the background.
    pub fn probe(&self, url: &str, request: RequestInfo) -> ProbeHandle {
        let (tx, rx) = oneshot::channel();
        let ctx = self.inner.ctx();
        let url = url.trim().to_owned();
        self.inner.handle.spawn(async move {
            let _ = tx.send(crate::probe::probe(&ctx, &url, &request).await);
        });
        ProbeHandle(rx)
    }

    /// Path of a helper program (yt-dlp, ffmpeg) if it is installed.
    pub fn tool_path(&self, tool: Tool) -> Option<PathBuf> {
        self.inner.tools.find(tool)
    }

    /// Downloads the official build of a helper program in the background.
    pub fn install_tool(&self, tool: Tool) -> InstallHandle {
        let handle = InstallHandle::default();
        let (inner, task_handle) = (self.inner.clone(), handle.clone());
        self.inner.handle.spawn(async move {
            let result = inner.tools.install(&inner.client, tool, &task_handle).await;
            task_handle.finish(result);
        });
        handle
    }

    pub fn add(&self, new: NewDownload) -> DownloadId {
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let mut jobs = self.inner.jobs.lock().unwrap();
        let taken = |name: &str| {
            jobs.iter().any(|j| {
                let d = j.data.lock().unwrap();
                d.save_dir == new.save_dir && d.file_name == name && d.status != Status::Completed
            })
        };
        let file_name = match new.kind {
            // Torrent names come from the torrent and an existing folder means
            // the data is already partly there, so never rename them.
            Kind::Torrent | Kind::Page => sanitize_file_name(&new.file_name),
            _ => unique_file_name(&new.save_dir, &sanitize_file_name(&new.file_name), taken),
        };
        let download = Download {
            id,
            url: new.url,
            category: new.category,
            file_name,
            save_dir: new.save_dir,
            kind: new.kind,
            status: if new.start {
                Status::Queued
            } else {
                Status::Paused
            },
            size: new.size,
            resumable: new.resumable,
            downloaded: 0,
            segments: Vec::new(),
            hls: None,
            torrent: new.torrent,
            page: new.page,
            request: new.request,
            added_at: now_secs(),
            last_try: None,
            description: new.description,
            speed: 0,
            connections: 0,
        };
        jobs.push(Job::new(download));
        drop(jobs);
        self.inner.changed();
        self.inner.schedule();
        id
    }

    /// Snapshot of all downloads, oldest first.
    pub fn list(&self) -> Vec<Download> {
        let jobs = self.inner.jobs.lock().unwrap();
        jobs.iter()
            .map(|j| j.data.lock().unwrap().clone())
            .collect()
    }

    pub fn get(&self, id: DownloadId) -> Option<Download> {
        self.inner.with_job(id, |j| j.data.lock().unwrap().clone())
    }

    pub fn poll_events(&self) -> Vec<Event> {
        std::mem::take(&mut *self.inner.events.lock().unwrap())
    }

    /// Puts paused or failed downloads back into the queue.
    pub fn resume(&self, id: DownloadId) {
        self.inner.with_job(id, |j| {
            let mut d = j.data.lock().unwrap();
            if matches!(d.status, Status::Paused | Status::Failed(_)) {
                d.status = Status::Queued;
            }
        });
        self.inner.changed();
        self.inner.schedule();
    }

    pub fn pause(&self, id: DownloadId) {
        self.inner.with_job(id, Job::pause);
        self.inner.changed();
    }

    pub fn resume_all(&self) {
        for d in self.list() {
            self.resume(d.id);
        }
    }

    pub fn pause_all(&self) {
        self.inner
            .jobs
            .lock()
            .unwrap()
            .iter_mut()
            .for_each(Job::pause);
        self.inner.changed();
    }

    /// Starts the file again from zero.
    pub fn redownload(&self, id: DownloadId) {
        let inner = self.inner.clone();
        self.inner.handle.spawn(async move {
            let Some((data, task)) = inner.with_job(id, |j| {
                j.pause();
                (j.data.clone(), j.running.take().map(|r| r.task))
            }) else {
                return;
            };
            if let Some(task) = task {
                let _ = task.await;
            }
            let snapshot = data.lock().unwrap().clone();
            discard_data(&inner, &snapshot, true).await;
            {
                let mut d = data.lock().unwrap();
                d.reset_progress();
                d.status = Status::Queued;
            }
            inner.changed();
            inner.schedule();
        });
    }

    /// Removes downloads from the list, optionally deleting their files.
    pub fn remove(&self, ids: &[DownloadId], delete_files: bool) {
        let removed: Vec<Job> = {
            let mut jobs = self.inner.jobs.lock().unwrap();
            let (removed, kept) = std::mem::take(&mut *jobs)
                .into_iter()
                .partition(|j| ids.contains(&j.data.lock().unwrap().id));
            *jobs = kept;
            removed
        };
        for mut job in removed {
            job.pause();
            let inner = self.inner.clone();
            self.inner.handle.spawn(async move {
                // Wait for the connections to close before touching the files.
                if let Some(running) = job.running.take() {
                    let _ = running.task.await;
                }
                let d = job.data.lock().unwrap().clone();
                discard_data(&inner, &d, delete_files).await;
            });
        }
        self.inner.changed();
    }

    /// Stops everything and writes the state to disk. Call before exiting.
    pub fn shutdown(&self) {
        self.pause_all();
        let tasks: Vec<_> = {
            let mut jobs = self.inner.jobs.lock().unwrap();
            jobs.iter_mut()
                .filter_map(|j| j.running.take())
                .map(|r| r.task)
                .collect()
        };
        self.inner.handle.block_on(async {
            let wait_all = futures_util::future::join_all(tasks);
            let _ = tokio::time::timeout(Duration::from_secs(5), wait_all).await;
        });
        self.inner.save();
    }
}

impl Job {
    fn new(download: Download) -> Self {
        let last_bytes = download.downloaded;
        Self {
            data: Arc::new(Mutex::new(download)),
            running: None,
            last_bytes,
        }
    }

    /// A download loaded from disk: nothing is running right after start-up.
    fn restored(mut download: Download) -> Self {
        if download.status.is_active() {
            download.status = Status::Paused;
        }
        Self::new(download)
    }

    fn pause(&mut self) {
        let mut d = self.data.lock().unwrap();
        if d.status.is_active() {
            d.status = Status::Paused;
        }
        if let Some(running) = &self.running {
            let _ = running.cancel.send(true);
        }
    }

    fn is_busy(&self) -> bool {
        self.running.as_ref().is_some_and(|r| !r.task.is_finished())
    }
}

impl Inner {
    fn ctx(&self) -> Ctx {
        let settings = self.settings.lock().unwrap();
        Ctx {
            client: self.client.clone(),
            limiter: self.limiter.clone(),
            user_agent: settings.user_agent.clone(),
            connections: settings.connections,
            max_retries: settings.max_retries,
            torrents: self.torrents.clone(),
            tools: self.tools.clone(),
        }
    }

    fn with_job<T>(&self, id: DownloadId, f: impl FnOnce(&mut Job) -> T) -> Option<T> {
        let mut jobs = self.jobs.lock().unwrap();
        jobs.iter_mut()
            .find(|j| j.data.lock().unwrap().id == id)
            .map(f)
    }

    fn changed(&self) {
        self.dirty.store(true, Ordering::Relaxed);
    }

    fn emit(&self, event: Event) {
        self.events.lock().unwrap().push(event);
        if let Some(waker) = &*self.waker.lock().unwrap() {
            waker();
        }
    }

    /// Starts queued downloads while there are free slots.
    fn schedule(self: &Arc<Self>) {
        let max_active = self.settings.lock().unwrap().max_active;
        let mut jobs = self.jobs.lock().unwrap();
        let mut active = jobs.iter().filter(|j| j.is_busy()).count();
        for job in jobs.iter_mut() {
            if active >= max_active {
                break;
            }
            if job.is_busy() || job.data.lock().unwrap().status != Status::Queued {
                continue;
            }
            self.start(job);
            active += 1;
        }
    }

    fn start(self: &Arc<Self>, job: &mut Job) {
        let (cancel, cancel_rx) = watch::channel(false);
        let kind = {
            let mut d = job.data.lock().unwrap();
            d.status = Status::Downloading;
            d.last_try = Some(now_secs());
            d.kind
        };
        let ctx = self.ctx();
        let data = job.data.clone();
        let inner = self.clone();
        let task = self.handle.spawn(async move {
            let result = match kind {
                Kind::Http => crate::http::run(ctx, data.clone(), cancel_rx).await,
                Kind::Hls => crate::hls::run(ctx, data.clone(), cancel_rx).await,
                Kind::Torrent => {
                    let torrents = ctx.torrents.clone();
                    torrents.run(ctx, data.clone(), cancel_rx).await
                }
                Kind::Page => crate::ytdlp::run(ctx, data.clone(), cancel_rx).await,
            };
            let result = match result {
                Ok(()) if !kind.writes_final_files() => finalize(&data).await,
                other => other,
            };
            inner.finished(&data, result);
        });
        job.running = Some(Running { cancel, task });
    }

    fn finished(&self, data: &SharedDownload, result: Result<()>) {
        let event = {
            let mut d = data.lock().unwrap();
            d.speed = 0;
            d.connections = 0;
            if d.status != Status::Downloading {
                None // paused or removed by the user meanwhile
            } else {
                match result {
                    Ok(()) => {
                        d.status = Status::Completed;
                        Some(Event::Completed(d.id))
                    }
                    Err(Error::Cancelled) => {
                        d.status = Status::Paused;
                        None
                    }
                    Err(e) => {
                        d.status = Status::Failed(e.to_string());
                        Some(Event::Failed(d.id, e.to_string()))
                    }
                }
            }
        };
        self.changed();
        if let Some(event) = event {
            self.emit(event);
        }
    }

    fn update_speeds(&self, elapsed: f64) {
        let mut jobs = self.jobs.lock().unwrap();
        for job in jobs.iter_mut() {
            let mut d = job.data.lock().unwrap();
            let delta = d.downloaded.saturating_sub(job.last_bytes);
            job.last_bytes = d.downloaded;
            if d.status == Status::Downloading {
                // Smooth the value a little so the number does not jump around.
                let now = delta as f64 / elapsed;
                d.speed = (d.speed as f64 * 0.6 + now * 0.4) as u64;
            }
        }
    }

    fn any_running(&self) -> bool {
        self.jobs.lock().unwrap().iter().any(Job::is_busy)
    }

    fn save(&self) {
        self.dirty.store(false, Ordering::Relaxed);
        let downloads: Vec<Download> = {
            let jobs = self.jobs.lock().unwrap();
            jobs.iter()
                .map(|j| j.data.lock().unwrap().clone())
                .collect()
        };
        self.store.save_downloads(&downloads);
    }
}

/// Deletes the temporary data of a download, and with `everything` also the
/// finished file (or torrent folder).
async fn discard_data(inner: &Inner, d: &Download, everything: bool) {
    if let Some(meta) = &d.torrent {
        inner.torrents.forget(&meta.info_hash).await;
        if everything {
            let path = d.path();
            let _ = if meta.multi_file {
                tokio::fs::remove_dir_all(path).await
            } else {
                tokio::fs::remove_file(path).await
            };
        }
        return;
    }
    if d.kind == Kind::Page {
        crate::ytdlp::remove_temp_files(d).await;
    }
    let _ = tokio::fs::remove_file(d.part_path()).await;
    if everything && d.status == Status::Completed {
        let _ = tokio::fs::remove_file(d.path()).await;
    }
}

/// Moves the finished `.rdm-part` file to its real name.
async fn finalize(data: &SharedDownload) -> Result<()> {
    let (part, dir, name) = {
        let d = data.lock().unwrap();
        (d.part_path(), d.save_dir.clone(), d.file_name.clone())
    };
    let name = if dir.join(&name).exists() {
        unique_file_name(&dir, &name, |_| false)
    } else {
        name
    };
    tokio::fs::rename(&part, dir.join(&name)).await?;
    let mut d = data.lock().unwrap();
    d.file_name = name;
    if d.size.is_none() || d.kind == Kind::Hls {
        d.size = Some(d.downloaded);
    }
    Ok(())
}

async fn scheduler(inner: Arc<Inner>) {
    let mut ticks = 0u32;
    let mut last = Instant::now();
    loop {
        tokio::time::sleep(TICK).await;
        let elapsed = last.elapsed().as_secs_f64();
        last = Instant::now();
        inner.update_speeds(elapsed);
        inner.schedule();
        ticks += 1;
        if ticks % SAVE_EVERY_TICKS == 0
            && (inner.dirty.load(Ordering::Relaxed) || inner.any_running())
        {
            let inner = inner.clone();
            // File IO stays off the async worker threads.
            let _ = tokio::task::spawn_blocking(move || inner.save()).await;
        }
    }
}
