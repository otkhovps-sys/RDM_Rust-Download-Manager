//! Videos embedded in web pages (YouTube and the many other sites yt-dlp
//! knows). RDM runs the `yt-dlp` program and follows its progress output.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::watch;

use crate::download::SharedDownload;
use crate::http::{Ctx, cancelled};
use crate::tools::{Tool, hidden, stop};
use crate::util::sanitize_file_name;
use crate::{Download, Error, Kind, PageOption, ProbeResult, RequestInfo, Result};

/// One line per progress update: `RDM|downloaded|total|estimate`.
const PROGRESS: &str = "download:RDM|%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s";
/// Printed once with the final file path.
const DONE: &str = "after_move:RDMFILE|%(filepath)s";

/// Sites where a page URL means "the video on this page".
pub(crate) fn is_video_site(url: &str) -> bool {
    const HOSTS: &[&str] = &[
        "youtube.com",
        "youtu.be",
        "youtube-nocookie.com",
        "vimeo.com",
        "dailymotion.com",
        "twitch.tv",
        "tiktok.com",
        "douyin.com",
        "facebook.com",
        "fb.watch",
        "instagram.com",
        "x.com",
        "twitter.com",
        "bilibili.com",
        "soundcloud.com",
        "reddit.com",
        "pinterest.com",
        "vk.com",
        "ok.ru",
        "rumble.com",
        "bitchute.com",
        "odysee.com",
        "streamable.com",
        "nicovideo.jp",
        "weibo.com",
        "tumblr.com",
        "linkedin.com",
        "ted.com",
        "kick.com",
    ];

    let host = url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_ascii_lowercase));
    host.is_some_and(|h| {
        HOSTS
            .iter()
            .any(|site| h == *site || h.ends_with(&format!(".{site}")))
    })
}

#[derive(Deserialize, Default)]
struct Info {
    id: Option<String>,
    title: Option<String>,
    description: Option<String>,
    /// The account's name (on TikTok the "@name").
    uploader: Option<String>,
    channel: Option<String>,
    #[serde(default)]
    formats: Vec<Format>,
}

#[derive(Deserialize)]
struct Format {
    height: Option<u32>,
    vcodec: Option<String>,
    acodec: Option<String>,
    ext: Option<String>,
    filesize: Option<f64>,
    filesize_approx: Option<f64>,
}

impl Format {
    fn has_video(&self) -> bool {
        self.vcodec.as_deref() != Some("none") && self.height.is_some()
    }

    fn has_audio(&self) -> bool {
        self.acodec.as_deref() != Some("none")
    }

    fn size(&self) -> Option<u64> {
        self.filesize.or(self.filesize_approx).map(|s| s as u64)
    }
}

/// Longest name (without extension) a video gets; yt-dlp adds its own
/// suffixes while it works and Windows paths are short.
const MAX_NAME: usize = 80;

/// A readable file name (without extension) for the video.
///
/// Sites with real titles (YouTube, ...) keep them. Short-video sites
/// (TikTok, Reels, ...) have none: there yt-dlp passes the caption, usually
/// a long row of hashtags and emoji that repeats from video to video. Such
/// videos are named "<author> - <caption without hashtags>", or
/// "<author> - <video id>" when nothing readable is left.
fn name_for(info: &Info) -> String {
    fn text(field: &Option<String>) -> Option<&str> {
        field.as_deref().map(str::trim).filter(|t| !t.is_empty())
    }
    let id = text(&info.id);
    // Without a title yt-dlp makes one up: "<site> video #<id>".
    let title = text(&info.title).filter(|t| !id.is_some_and(|id| t.ends_with(&format!("#{id}"))));
    let is_caption = title.is_none() || title == text(&info.description);

    let name = if is_caption {
        let caption = shorten(&plain_words(title.unwrap_or_default()));
        let author = text(&info.uploader).or(text(&info.channel));
        let rest = Some(caption.as_str()).filter(|c| !c.is_empty()).or(id);
        match (author, rest) {
            (Some(author), Some(rest)) => format!("{author} - {rest}"),
            (author, rest) => author.or(rest).unwrap_or("video").to_owned(),
        }
    } else {
        let words: Vec<&str> = title.unwrap_or_default().split_whitespace().collect();
        shorten(&words.join(" "))
    };
    sanitize_file_name(&name)
}

/// The caption's words without hashtags, emoji and line breaks.
fn plain_words(caption: &str) -> String {
    let is_symbol = |c: char| {
        matches!(u32::from(c),
            0x200B..=0x200F | 0x2600..=0x27BF | 0x2B00..=0x2BFF | 0xFE00..=0xFE0F | 0x1F000..=0x1FAFF | 0xE0000..=0xE007F)
    };
    let letters: String = caption.chars().filter(|c| !is_symbol(*c)).collect();
    let words: Vec<&str> = letters
        .split_whitespace()
        .filter(|word| !word.starts_with('#'))
        .collect();
    words.join(" ")
}

/// Cuts `text` to at most [`MAX_NAME`] characters, at a word where possible.
fn shorten(text: &str) -> String {
    if text.chars().count() <= MAX_NAME {
        return text.to_owned();
    }
    let cut: String = text.chars().take(MAX_NAME).collect();
    let whole_words = cut
        .rsplit_once(' ')
        .map_or(cut.as_str(), |(words, _)| words);
    // One very long word (or a language without spaces): keep the plain cut.
    let kept = if whole_words.chars().count() >= MAX_NAME / 2 {
        whole_words
    } else {
        &cut
    };
    kept.trim_end_matches(|c: char| c.is_whitespace() || ",.;:-".contains(c))
        .to_owned()
}

/// Asks yt-dlp what the page offers, without downloading.
pub(crate) async fn probe(ctx: &Ctx, url: &str, req: &RequestInfo) -> Result<ProbeResult> {
    let ytdlp = ctx.tools.require(Tool::YtDlp)?;
    // Anonymous first; the browser's cookies only when the site insists
    // (private, age-restricted or "sign in to continue" pages).
    let (info, used_cookies) = match dump_info(&ytdlp, url, req, None).await {
        Ok(info) => (info, false),
        Err(e) => match CookieFile::create(req).await {
            Some(cookies) => (dump_info(&ytdlp, url, req, Some(&cookies)).await?, true),
            None => return Err(e),
        },
    };

    let options = options(&info.formats, ctx.tools.find(Tool::Ffmpeg).is_some());
    let title = name_for(&info);
    Ok(ProbeResult {
        url: url.to_owned(),
        file_name: format!("{title}.{}", options[0].ext),
        size: options[0].size,
        resumable: true,
        kind: Kind::Page,
        content_type: None,
        hls_segments: None,
        torrent: None,
        page: options,
        used_cookies,
    })
}

async fn dump_info(
    ytdlp: &Path,
    url: &str,
    req: &RequestInfo,
    cookies: Option<&CookieFile>,
) -> Result<Info> {
    let mut command = hidden(Command::new(ytdlp));
    command.args(header_args(req)).args([
        "--dump-single-json",
        "--no-playlist",
        "--no-warnings",
        "--encoding",
        "utf-8",
    ]);
    if let Some(cookies) = cookies {
        command.arg("--cookies").arg(&cookies.0);
    }
    let output = command
        .arg("--")
        .arg(url)
        .stdin(Stdio::null())
        .output()
        .await?;
    if !output.status.success() {
        return Err(Error::other(last_error(&String::from_utf8_lossy(
            &output.stderr,
        ))));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|e| Error::other(format!("unexpected yt-dlp output: {e}")))
}

/// The browser's cookies written to a private temporary file for yt-dlp;
/// the file is deleted when this value is dropped.
struct CookieFile(PathBuf);

impl CookieFile {
    async fn create(req: &RequestInfo) -> Option<Self> {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let text = req
            .cookie_file
            .as_deref()
            .filter(|t| !t.trim().is_empty())?;
        let name = format!(
            "rdm-cookies-{}-{}.txt",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let file = Self(std::env::temp_dir().join(name));
        let mut options = tokio::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut handle = options.open(&file.0).await.ok()?;
        handle.write_all(text.as_bytes()).await.ok()?;
        handle.flush().await.ok()?;
        Some(file)
    }
}

impl Drop for CookieFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Builds the quality list, best first. Without ffmpeg only formats that
/// already contain both video and audio can be offered.
fn options(formats: &[Format], ffmpeg: bool) -> Vec<PageOption> {
    let usable = |f: &&Format| f.has_video() && (ffmpeg || f.has_audio());
    let best_audio = formats
        .iter()
        .filter(|f| !f.has_video() && f.has_audio())
        .filter_map(Format::size)
        .max();
    let heights: BTreeSet<u32> = formats
        .iter()
        .filter(usable)
        .filter_map(|f| f.height)
        .collect();

    let mut options: Vec<PageOption> = heights
        .into_iter()
        .rev()
        .map(|height| {
            let best = formats
                .iter()
                .filter(usable)
                .filter(|f| f.height == Some(height))
                .max_by_key(|f| f.size().unwrap_or(0))
                .expect("height comes from these formats");
            let audio = if best.has_audio() {
                Some(0)
            } else {
                best_audio
            };
            PageOption {
                label: format!("{height}p"),
                height: Some(height),
                audio_only: false,
                ext: if ffmpeg {
                    "mp4".into()
                } else {
                    best.ext.clone().unwrap_or_else(|| "mp4".into())
                },
                size: best.size().zip(audio).map(|(v, a)| v + a),
            }
        })
        .collect();
    if options.is_empty() {
        options.push(PageOption {
            label: "Best available".into(),
            height: None,
            audio_only: false,
            ext: "mp4".into(),
            size: None,
        });
    }
    let (label, ext) = if ffmpeg {
        ("Audio only (MP3)", "mp3")
    } else {
        ("Audio only (M4A)", "m4a")
    };
    options.push(PageOption {
        label: label.into(),
        height: None,
        audio_only: true,
        ext: ext.into(),
        size: best_audio,
    });
    options
}

/// For streams found by the browser extension, yt-dlp sends what the
/// browser sent. Video sites (`req.page`) are left to yt-dlp's own handling.
fn header_args(req: &RequestInfo) -> Vec<String> {
    let mut args = Vec::new();
    if req.page {
        return args;
    }
    if let Some(referer) = &req.referer {
        args.extend(["--referer".to_owned(), referer.clone()]);
    }
    if let Some(agent) = &req.user_agent {
        args.extend(["--user-agent".to_owned(), agent.clone()]);
    }
    if let Some(cookies) = req.cookies.as_deref().filter(|c| !c.is_empty()) {
        args.extend(["--add-header".to_owned(), format!("Cookie:{cookies}")]);
    }
    args
}

/// yt-dlp arguments that select `option`.
fn format_args(option: &PageOption, ffmpeg: bool) -> Vec<String> {
    let limit = option
        .height
        .map(|h| format!("[height<={h}]"))
        .unwrap_or_default();
    // Resolution decides first; among equal ones MP4-friendly streams win.
    let sort = match option.height {
        Some(h) => format!("res:{h},ext:mp4:m4a"),
        None => "res,ext:mp4:m4a".to_owned(),
    };
    let args: &[&str] = match (option.audio_only, ffmpeg) {
        (true, true) => &["-f", "ba/b", "-x", "--audio-format", "mp3"],
        (true, false) => &["-f", "ba[ext=m4a]/ba/b"],
        // Best video up to the chosen height plus best audio, joined as MP4.
        (false, true) => &[
            "-f",
            "bv*{limit}+ba/b{limit}/b",
            "-S",
            "{sort}",
            "--merge-output-format",
            "mp4",
        ],
        (false, false) => &["-f", "b{limit}/b"],
    };
    args.iter()
        .map(|a| a.replace("{limit}", &limit).replace("{sort}", &sort))
        .collect()
}

pub(crate) async fn run(
    ctx: Ctx,
    dl: SharedDownload,
    mut cancel: watch::Receiver<bool>,
) -> Result<()> {
    let ytdlp = ctx.tools.require(Tool::YtDlp)?;
    let ffmpeg = ctx.tools.find(Tool::Ffmpeg);
    let (url, option, dir, stem, request) = {
        let d = dl.lock().unwrap();
        let option = d
            .page
            .clone()
            .ok_or_else(|| Error::other("missing quality selection"))?;
        (
            d.url.clone(),
            option,
            d.save_dir.clone(),
            stem(&d.file_name).to_owned(),
            d.request.clone(),
        )
    };
    tokio::fs::create_dir_all(&dir).await?;

    let mut command = hidden(Command::new(ytdlp));
    command
        .args(header_args(&request))
        .args([
            "--no-playlist",
            "--no-warnings",
            "--no-mtime",
            "--encoding",
            "utf-8",
        ])
        .args([
            "--newline",
            "--progress",
            "--progress-template",
            PROGRESS,
            "--print",
            DONE,
        ])
        .args(format_args(&option, ffmpeg.is_some()))
        .args(["-N", &ctx.connections.clamp(1, 16).to_string()])
        .arg("-o")
        // `%` starts a template field in yt-dlp, so it is doubled in the name.
        .arg(dir.join(format!("{}.%(ext)s", stem.replace('%', "%%"))));
    if let Some(ffmpeg) = ffmpeg {
        command.arg("--ffmpeg-location").arg(ffmpeg);
    }
    // Kept only when the probe needed them (see `NewDownload::from_probe`).
    let cookies = CookieFile::create(&request).await;
    if let Some(cookies) = &cookies {
        command.arg("--cookies").arg(&cookies.0);
    }
    if ctx.limiter.rate() > 0 {
        command.args(["-r", &ctx.limiter.rate().to_string()]);
    }
    command.arg("--").arg(&url);
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let stderr = child.stderr.take().expect("piped");
    let errors = tokio::spawn(async move {
        let mut text = String::new();
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            text.push_str(&line);
            text.push('\n');
        }
        text
    });

    dl.lock().unwrap().connections = 1;
    let mut lines = BufReader::new(child.stdout.take().expect("piped")).lines();
    let mut progress = Progress::default();
    loop {
        let line = tokio::select! {
            line = lines.next_line() => line?,
            () = cancelled(&mut cancel) => {
                stop(&mut child).await;
                return Err(Error::Cancelled);
            }
        };
        let Some(line) = line else { break };
        if let Some(path) = line.strip_prefix("RDMFILE|") {
            finished(&dl, path).await;
        } else if let Some(fields) = line.strip_prefix("RDM|") {
            progress.update(&mut dl.lock().unwrap(), fields);
        }
    }

    if child.wait().await?.success() {
        Ok(())
    } else {
        Err(Error::other(last_error(&errors.await.unwrap_or_default())))
    }
}

/// Adds up the progress of the files yt-dlp downloads one after another
/// (usually a video stream, then an audio stream).
#[derive(Default)]
struct Progress {
    /// Bytes of the files that are already complete.
    finished: u64,
    current: u64,
}

impl Progress {
    fn update(&mut self, d: &mut Download, fields: &str) {
        let mut numbers = fields
            .split('|')
            .map(|f| f.trim().parse::<f64>().ok().map(|n| n as u64));
        let downloaded = numbers.next().flatten().unwrap_or(0);
        let total = numbers
            .next()
            .flatten()
            .or_else(|| numbers.next().flatten());
        if downloaded < self.current {
            self.finished += self.current; // the next file started
        }
        self.current = downloaded;
        d.downloaded = self.finished + downloaded;
        let estimate = self.finished + total.unwrap_or(downloaded);
        d.size = Some(d.size.unwrap_or(0).max(estimate).max(d.downloaded));
    }
}

/// Records the real name and size of the finished file.
async fn finished(dl: &SharedDownload, path: &str) {
    let path = std::path::Path::new(path.trim());
    let size = tokio::fs::metadata(path).await.map(|m| m.len()).ok();
    let mut d = dl.lock().unwrap();
    if let Some(name) = path.file_name() {
        d.file_name = name.to_string_lossy().into_owned();
    }
    if let Some(size) = size {
        d.size = Some(size);
        d.downloaded = size;
    }
}

/// Deletes what yt-dlp left behind for an unfinished download.
pub(crate) async fn remove_temp_files(d: &Download) {
    let prefix = format!("{}.", stem(&d.file_name));
    let Ok(mut entries) = tokio::fs::read_dir(&d.save_dir).await else {
        return;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        let temporary =
            name.ends_with(".part") || name.ends_with(".ytdl") || name.contains(".part-Frag");
        if name.starts_with(&prefix) && temporary {
            let _ = tokio::fs::remove_file(entry.path()).await;
        }
    }
}

fn stem(file_name: &str) -> &str {
    file_name
        .rsplit_once('.')
        .map_or(file_name, |(stem, _)| stem)
}

/// The last `ERROR:` line yt-dlp wrote, without the prefix.
fn last_error(stderr: &str) -> String {
    stderr
        .lines()
        .rev()
        .find_map(|line| line.trim().strip_prefix("ERROR:"))
        .map(|e| e.trim().to_owned())
        .unwrap_or_else(|| "yt-dlp could not download this page".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn format(height: Option<u32>, vcodec: &str, acodec: &str, size: u64) -> Format {
        Format {
            height,
            vcodec: Some(vcodec.into()),
            acodec: Some(acodec.into()),
            ext: Some("mp4".into()),
            filesize: Some(size as f64),
            filesize_approx: None,
        }
    }

    #[test]
    fn builds_quality_list() {
        let formats = [
            format(None, "none", "opus", 3_000),
            format(Some(360), "avc1", "mp4a", 10_000),
            format(Some(720), "avc1", "none", 50_000),
            format(Some(1080), "vp9", "none", 90_000),
        ];
        let with = options(&formats, true);
        let labels: Vec<_> = with.iter().map(|o| o.label.as_str()).collect();
        assert_eq!(labels, ["1080p", "720p", "360p", "Audio only (MP3)"]);
        assert_eq!(with[0].size, Some(93_000)); // video + best audio
        assert_eq!(with[2].size, Some(10_000)); // already has audio

        // Without ffmpeg only the pre-merged 360p file is possible.
        let without = options(&formats, false);
        let labels: Vec<_> = without.iter().map(|o| o.label.as_str()).collect();
        assert_eq!(labels, ["360p", "Audio only (M4A)"]);
    }

    fn info(title: &str, description: &str, uploader: &str, id: &str) -> Info {
        let field = |text: &str| Some(text.to_owned()).filter(|t| !t.is_empty());
        Info {
            id: field(id),
            title: field(title),
            description: field(description),
            uploader: field(uploader),
            ..Info::default()
        }
    }

    #[test]
    fn names_videos() {
        // A real title stays, whatever the description says.
        let video = info(
            "Rust in 100 Seconds  #shorts",
            "Learn Rust...",
            "Fireship",
            "5C_HPTJg5ek",
        );
        assert_eq!(name_for(&video), "Rust in 100 Seconds #shorts");

        // TikTok: the "title" is the caption.
        let caption = "Mèo con đi lạc 🐱✨ về nhà rồi\n#meo #fyp #xuhuong #viral";
        let video = info(caption, caption, "meo.cute", "7706387837360311573");
        assert_eq!(name_for(&video), "meo.cute - Mèo con đi lạc về nhà rồi");

        // Nothing but hashtags, or no caption at all: the id tells videos apart.
        let video = info(
            "#fyp #viral",
            "#fyp #viral",
            "meo.cute",
            "7706387837360311573",
        );
        assert_eq!(name_for(&video), "meo.cute - 7706387837360311573");
        let video = info(
            "TikTok video #7706387837360311573",
            "",
            "meo.cute",
            "7706387837360311573",
        );
        assert_eq!(name_for(&video), "meo.cute - 7706387837360311573");
        assert_eq!(name_for(&Info::default()), "video");

        // Long captions end at a word.
        let caption = "word ".repeat(60);
        let video = info(&caption, &caption, "someone", "1");
        let name = name_for(&video);
        assert!(name.starts_with("someone - word word") && name.ends_with("word"));
        assert_eq!(name.chars().count(), "someone - ".len() + 79);
    }

    #[test]
    fn selects_formats() {
        let option = PageOption {
            label: "720p".into(),
            height: Some(720),
            audio_only: false,
            ext: "mp4".into(),
            size: None,
        };
        assert_eq!(
            format_args(&option, true)[1],
            "bv*[height<=720]+ba/b[height<=720]/b"
        );
        assert_eq!(format_args(&option, false), ["-f", "b[height<=720]/b"]);
    }

    #[test]
    fn adds_up_progress_of_two_streams() {
        let mut d: Download = serde_json::from_str(
            r#"{"id":1,"url":"u","file_name":"a.mp4","save_dir":"/tmp","kind":"Page","category":"Video",
                "status":"Downloading","size":null,"resumable":true,"downloaded":0,"segments":[],
                "hls":null,"request":{"referer":null,"cookies":null,"user_agent":null},
                "added_at":0,"last_try":null,"description":""}"#,
        )
        .unwrap();
        let mut p = Progress::default();
        p.update(&mut d, "500|1000|NA");
        assert_eq!((d.downloaded, d.size), (500, Some(1000)));
        p.update(&mut d, "1000|1000|NA");
        p.update(&mut d, "100|NA|300.5"); // audio stream starts
        assert_eq!((d.downloaded, d.size), (1100, Some(1300)));
    }

    #[test]
    fn forwards_browser_headers_for_streams_only() {
        let mut req = RequestInfo {
            referer: Some("https://site.example/watch".into()),
            cookies: Some("a=1".into()),
            user_agent: Some("UA".into()),
            page: false,
            cookie_file: None,
        };
        assert_eq!(
            header_args(&req),
            [
                "--referer",
                "https://site.example/watch",
                "--user-agent",
                "UA",
                "--add-header",
                "Cookie:a=1"
            ]
        );
        req.page = true;
        assert!(header_args(&req).is_empty());
    }

    #[test]
    fn recognizes_video_sites() {
        assert!(is_video_site("https://www.youtube.com/watch?v=abc"));
        assert!(is_video_site("https://youtu.be/abc"));
        assert!(!is_video_site("https://notyoutube.com/watch"));
        assert_eq!(
            last_error("WARNING: x\nERROR: Video unavailable\n"),
            "Video unavailable"
        );
    }
}
