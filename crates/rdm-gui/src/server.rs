//! Local HTTP endpoint for the browser extension (127.0.0.1 only), on the
//! first free port of [`PORTS`].
//!
//! * `GET  /ping`     -> app info and the file types to capture
//! * `POST /download` -> `{url, referer?, cookies?, userAgent?, fileName?, page?}`
//!   (`url` may also be a magnet link; `page: true` marks a video page for yt-dlp)
//! * `POST /show`     -> bring the main window to the front

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rdm_core::{Engine, RequestInfo};
use serde::Deserialize;
use serde_json::json;
use tiny_http::{Header, Method, Request, Response, Server};

/// A download handed over by the browser.
pub struct Incoming {
    pub url: String,
    pub request: RequestInfo,
    pub file_name: Option<String>,
}

pub enum Message {
    Download(Incoming),
    Show,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DownloadBody {
    url: String,
    referer: Option<String>,
    cookies: Option<String>,
    user_agent: Option<String>,
    file_name: Option<String>,
    /// `url` is a web page with a video (YouTube, ...), not a file.
    #[serde(default)]
    page: bool,
    /// The page's cookies as `cookies.txt` text, for signed-in video pages.
    cookie_file: Option<String>,
}

/// RDM listens on the first free port of this range and the extension looks
/// for it there, so nobody has to configure a port.
pub const PORTS: std::ops::RangeInclusive<u16> = 9614..=9623;

/// When the browser extension last talked to RDM, and its version.
type LastSeen = Arc<Mutex<Option<(Instant, String)>>>;

/// The running endpoint, as seen by the GUI.
pub struct Link {
    pub port: u16,
    pub messages: Receiver<Message>,
    seen: LastSeen,
}

impl Link {
    /// Version of the connected extension and how long ago it was heard from.
    pub fn extension(&self) -> Option<(String, Duration)> {
        let seen = self.seen.lock().unwrap();
        seen.as_ref()
            .map(|(at, version)| (version.clone(), at.elapsed()))
    }
}

/// Starts the server thread on the first free port of [`PORTS`].
pub fn spawn(engine: Engine, ctx: egui::Context) -> std::io::Result<Link> {
    let (port, server) = PORTS
        .filter_map(|port| Some((port, Server::http(("127.0.0.1", port)).ok()?)))
        .next()
        .ok_or_else(|| std::io::Error::other("no free local port"))?;
    let (tx, messages) = channel();
    let seen = LastSeen::default();
    let thread_seen = seen.clone();
    std::thread::Builder::new()
        .name("rdm-server".into())
        .spawn(move || {
            for request in server.incoming_requests() {
                handle(request, &engine, &tx, &ctx, &thread_seen);
            }
        })?;
    Ok(Link {
        port,
        messages,
        seen,
    })
}

fn handle(
    mut request: Request,
    engine: &Engine,
    tx: &Sender<Message>,
    ctx: &egui::Context,
    seen: &LastSeen,
) {
    if !origin_allowed(&request) {
        let _ = request.respond(reply(
            403,
            json!({ "ok": false, "error": "forbidden origin" }),
        ));
        return;
    }
    let send = |message| {
        let _ = tx.send(message);
        ctx.request_repaint();
    };
    // `/ping?ext=1.3.0`: the extension says hello with its version.
    let url = request.url().to_owned();
    let (path, query) = url.split_once('?').unwrap_or((&url, ""));
    // (Browsers send no `Origin` with an extension's GET requests, so this
    // cannot be tied to one; a false "connected" would be harmless anyway.)
    if let Some(version) = query.strip_prefix("ext=") {
        *seen.lock().unwrap() = Some((Instant::now(), version.to_owned()));
        ctx.request_repaint();
    }
    let response = match (request.method(), path) {
        (Method::Options, _) => reply(204, json!({})),
        (Method::Get, "/ping") => {
            let settings = engine.settings();
            reply(
                200,
                json!({
                    "ok": true,
                    "app": "rdm",
                    "version": env!("CARGO_PKG_VERSION"),
                    "enabled": settings.browser_integration,
                    "captureTypes": settings.capture_types.split_whitespace().collect::<Vec<_>>(),
                }),
            )
        }
        (Method::Post, "/show") => {
            send(Message::Show);
            reply(200, json!({ "ok": true }))
        }
        (Method::Post, "/download") => match read_body(&mut request) {
            Some(body) => {
                send(Message::Download(Incoming {
                    url: body.url,
                    request: RequestInfo {
                        referer: body.referer,
                        cookies: body.cookies,
                        user_agent: body.user_agent,
                        page: body.page,
                        cookie_file: body.cookie_file,
                    },
                    file_name: body.file_name,
                }));
                reply(200, json!({ "ok": true }))
            }
            None => reply(400, json!({ "ok": false, "error": "bad request" })),
        },
        _ => reply(404, json!({ "ok": false, "error": "not found" })),
    };
    let _ = request.respond(response);
}

fn origin(request: &Request) -> Option<&str> {
    let header = request.headers().iter().find(|h| h.field.equiv("Origin"))?;
    Some(header.value.as_str())
}

fn from_extension(request: &Request) -> bool {
    origin(request).is_some_and(|value| {
        [
            "chrome-extension://",
            "moz-extension://",
            "safari-web-extension://",
        ]
        .iter()
        .any(|prefix| value.starts_with(prefix))
    })
}

/// Web pages must not be able to push downloads; only extensions (or local
/// programs, which send no `Origin`) may talk to us.
fn origin_allowed(request: &Request) -> bool {
    origin(request).is_none() || from_extension(request)
}

fn read_body(request: &mut Request) -> Option<DownloadBody> {
    let mut body = String::new();
    request
        .as_reader()
        .take(256 * 1024)
        .read_to_string(&mut body)
        .ok()?;
    serde_json::from_str::<DownloadBody>(&body)
        .ok()
        .filter(|b| b.url.contains("://") || b.url.starts_with("magnet:"))
}

fn reply(status: u16, body: serde_json::Value) -> Response<std::io::Cursor<Vec<u8>>> {
    let header = |k: &str, v: &str| Header::from_bytes(k, v).expect("valid header");
    Response::from_string(body.to_string())
        .with_status_code(status)
        .with_header(header("Content-Type", "application/json"))
        .with_header(header("Access-Control-Allow-Origin", "*"))
        .with_header(header("Access-Control-Allow-Methods", "GET, POST, OPTIONS"))
        .with_header(header("Access-Control-Allow-Headers", "Content-Type"))
}

/// If RDM already runs, passes it the URL (or asks it to show itself) and
/// returns `true` so this second process can exit.
pub fn forward_to_running(url: Option<&str>) -> bool {
    let (path, body) = match url {
        Some(url) => ("/download", json!({ "url": url }).to_string()),
        None => ("/show", String::new()),
    };
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    PORTS.into_iter().any(|port| {
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(300)) else {
            return false;
        };
        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
        if stream.write_all(request.as_bytes()).is_err() {
            return false;
        }
        let mut response = String::new();
        let _ = stream.read_to_string(&mut response);
        response.contains(r#""ok":true"#)
    })
}
