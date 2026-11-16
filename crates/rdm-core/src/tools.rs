//! External helper programs: yt-dlp (video sites such as YouTube) and ffmpeg
//! (joins the separate video and audio streams those sites serve).
//!
//! They are looked up in RDM's own `tools` folder first, then on `PATH`.
//! When missing, RDM can install the official builds - only after the user
//! asked for it - and verifies their SHA-256 checksums.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use futures_util::StreamExt;
use reqwest::Client;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::{Error, Result};

const YTDLP_RELEASE: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download";
const FFMPEG_RELEASE: &str = "https://github.com/yt-dlp/FFmpeg-Builds/releases/latest/download";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    YtDlp,
    Ffmpeg,
}

impl Tool {
    pub fn name(self) -> &'static str {
        match self {
            Self::YtDlp => "yt-dlp",
            Self::Ffmpeg => "ffmpeg",
        }
    }

    fn file_name(self) -> String {
        format!("{}{}", self.name(), std::env::consts::EXE_SUFFIX)
    }

    /// Where the official build for this platform comes from.
    fn source(self) -> Option<Source> {
        use std::env::consts::{ARCH, OS};
        let (base, asset, sums) = match (self, OS, ARCH) {
            (Self::YtDlp, "windows", _) => (YTDLP_RELEASE, "yt-dlp.exe", "SHA2-256SUMS"),
            (Self::YtDlp, "macos", _) => (YTDLP_RELEASE, "yt-dlp_macos", "SHA2-256SUMS"),
            (Self::YtDlp, "linux", "x86_64") => (YTDLP_RELEASE, "yt-dlp_linux", "SHA2-256SUMS"),
            (Self::YtDlp, "linux", "aarch64") => {
                (YTDLP_RELEASE, "yt-dlp_linux_aarch64", "SHA2-256SUMS")
            }
            (Self::Ffmpeg, "windows", "x86_64") => (
                FFMPEG_RELEASE,
                "ffmpeg-master-latest-win64-gpl.zip",
                "checksums.sha256",
            ),
            (Self::Ffmpeg, "linux", "x86_64") => (
                FFMPEG_RELEASE,
                "ffmpeg-master-latest-linux64-gpl.tar.xz",
                "checksums.sha256",
            ),
            (Self::Ffmpeg, "linux", "aarch64") => (
                FFMPEG_RELEASE,
                "ffmpeg-master-latest-linuxarm64-gpl.tar.xz",
                "checksums.sha256",
            ),
            _ => return None,
        };
        Some(Source {
            url: format!("{base}/{asset}"),
            sums_url: format!("{base}/{sums}"),
            asset: asset.to_owned(),
        })
    }

    /// RDM can download this tool itself on the current platform.
    pub fn can_install(self) -> bool {
        self.source().is_some()
    }
}

struct Source {
    url: String,
    sums_url: String,
    asset: String,
}

/// Progress of a running installation, polled by the GUI.
#[derive(Clone, Default)]
pub struct InstallHandle(Arc<Mutex<InstallState>>);

#[derive(Default)]
struct InstallState {
    downloaded: u64,
    total: Option<u64>,
    result: Option<std::result::Result<(), String>>,
}

impl InstallHandle {
    /// `0.0..=1.0` while downloading, if the size is known.
    pub fn progress(&self) -> Option<f32> {
        let state = self.0.lock().unwrap();
        state
            .total
            .filter(|&t| t > 0)
            .map(|t| (state.downloaded as f64 / t as f64).min(1.0) as f32)
    }

    /// `None` while running, then the outcome.
    pub fn result(&self) -> Option<std::result::Result<(), String>> {
        self.0.lock().unwrap().result.clone()
    }

    pub(crate) fn finish(&self, result: Result<()>) {
        self.0.lock().unwrap().result = Some(result.map_err(|e| e.to_string()));
    }
}

pub(crate) struct Tools {
    dir: PathBuf,
}

impl Tools {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// Path of the tool if it is installed.
    pub fn find(&self, tool: Tool) -> Option<PathBuf> {
        let name = tool.file_name();
        let own = self.dir.join(&name);
        if own.is_file() {
            return Some(own);
        }
        let path = std::env::var_os("PATH")?;
        std::env::split_paths(&path)
            .map(|dir| dir.join(&name))
            .find(|p| p.is_file())
    }

    pub fn require(&self, tool: Tool) -> Result<PathBuf> {
        self.find(tool).ok_or(Error::MissingTool(tool))
    }

    pub async fn install(&self, client: &Client, tool: Tool, handle: &InstallHandle) -> Result<()> {
        let source = tool.source().ok_or_else(|| {
            Error::other(format!(
                "{} cannot be installed automatically on this system; please install it yourself",
                tool.name()
            ))
        })?;
        tokio::fs::create_dir_all(&self.dir).await?;
        let download = self.dir.join(format!("{}.download", source.asset));
        let result = async {
            fetch_verified(client, &source, &download, handle).await?;
            match tool {
                Tool::YtDlp => {
                    make_executable(&download).await?;
                    tokio::fs::rename(&download, self.dir.join(tool.file_name())).await?;
                }
                Tool::Ffmpeg => self.unpack_ffmpeg(&download).await?,
            }
            Ok(())
        }
        .await;
        let _ = tokio::fs::remove_file(&download).await;
        result
    }

    /// The archive holds `<name>/bin/ffmpeg` and `ffprobe`; the system's `tar`
    /// (part of Windows 10+ too) unpacks both `.zip` and `.tar.xz`.
    async fn unpack_ffmpeg(&self, archive: &Path) -> Result<()> {
        let temp = self.dir.join("ffmpeg-unpack");
        let _ = tokio::fs::remove_dir_all(&temp).await;
        tokio::fs::create_dir_all(&temp).await?;
        let result = async {
            let status = hidden(Command::new("tar"))
                .arg("-xf")
                .arg(archive)
                .arg("-C")
                .arg(&temp)
                .status()
                .await?;
            if !status.success() {
                return Err(Error::other("could not unpack the ffmpeg archive"));
            }
            let mut roots = tokio::fs::read_dir(&temp).await?;
            let root = roots
                .next_entry()
                .await?
                .ok_or_else(|| Error::other("empty ffmpeg archive"))?;
            for program in ["ffmpeg", "ffprobe"] {
                let name = format!("{program}{}", std::env::consts::EXE_SUFFIX);
                tokio::fs::rename(root.path().join("bin").join(&name), self.dir.join(&name))
                    .await?;
            }
            Ok(())
        }
        .await;
        let _ = tokio::fs::remove_dir_all(&temp).await;
        result
    }
}

/// Downloads `source` to `dest` and checks it against the published SHA-256.
async fn fetch_verified(
    client: &Client,
    source: &Source,
    dest: &Path,
    handle: &InstallHandle,
) -> Result<()> {
    let sums = get(client, &source.sums_url).await?.text().await?;
    let expected = sums
        .lines()
        .filter_map(|line| line.split_once(char::is_whitespace))
        .find(|(_, name)| name.trim().trim_start_matches('*') == source.asset)
        .map(|(hash, _)| hash.to_ascii_lowercase())
        .ok_or_else(|| Error::other(format!("no checksum published for {}", source.asset)))?;

    let response = get(client, &source.url).await?;
    handle.0.lock().unwrap().total = response.content_length();
    let mut file = tokio::fs::File::create(dest).await?;
    let mut hasher = Sha256::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
        handle.0.lock().unwrap().downloaded += chunk.len() as u64;
    }
    file.flush().await?;

    let actual: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if actual != expected {
        return Err(Error::other(format!(
            "checksum mismatch for {}, download discarded",
            source.asset
        )));
    }
    Ok(())
}

async fn get(client: &Client, url: &str) -> Result<reqwest::Response> {
    let response = client.get(url).send().await?;
    if !response.status().is_success() {
        return Err(Error::Status(response.status().as_u16()));
    }
    Ok(response)
}

async fn make_executable(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).await?;
    }
    let _ = path;
    Ok(())
}

/// Stops a helper program together with the child processes it started
/// (the official yt-dlp builds are a launcher plus the real program).
pub(crate) async fn stop(child: &mut tokio::process::Child) {
    let Some(pid) = child.id() else {
        return; // already gone
    };
    #[cfg(unix)]
    {
        // SAFETY: plain signal to the process group created in `hidden`.
        unsafe { libc::killpg(pid as libc::pid_t, libc::SIGTERM) };
        let graceful = tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await;
        if graceful.is_err() {
            unsafe { libc::killpg(pid as libc::pid_t, libc::SIGKILL) };
        }
    }
    #[cfg(windows)]
    {
        let _ = hidden(Command::new("taskkill"))
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .status()
            .await;
    }
    let _ = child.wait().await;
}

/// Helper programs must not flash a console window on Windows.
pub(crate) fn hidden(mut command: Command) -> Command {
    #[cfg(windows)]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    #[cfg(unix)]
    command.process_group(0); // own group, so `stop` reaches its children
    command.kill_on_drop(true);
    command
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;

    /// Serves `/tool` and `/sums` once each on a local port.
    async fn serve(body: &'static [u8], sums: String) -> u16 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let (mut sock, _) = listener.accept().await.unwrap();
                let mut buf = vec![0; 2048];
                let n = sock.read(&mut buf).await.unwrap();
                let request = String::from_utf8_lossy(&buf[..n]).to_string();
                let payload = if request.starts_with("GET /sums") {
                    sums.as_bytes()
                } else {
                    body
                };
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    payload.len()
                );
                let _ = sock.write_all(head.as_bytes()).await;
                let _ = sock.write_all(payload).await;
            }
        });
        port
    }

    #[tokio::test]
    async fn verifies_checksums() {
        let body: &[u8] = b"fake tool binary";
        let good: String = Sha256::digest(body)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let dir = std::env::temp_dir().join(format!("rdm-tools-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let client = Client::new();

        for (hash, ok) in [(good.clone(), true), ("0".repeat(64), false)] {
            let port = serve(body, format!("abc  other\n{hash}  tool\n")).await;
            let source = Source {
                url: format!("http://127.0.0.1:{port}/tool"),
                sums_url: format!("http://127.0.0.1:{port}/sums"),
                asset: "tool".into(),
            };
            let handle = InstallHandle::default();
            let result = fetch_verified(&client, &source, &dir.join("tool"), &handle).await;
            assert_eq!(result.is_ok(), ok, "{result:?}");
            assert_eq!(handle.progress(), Some(1.0));
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
