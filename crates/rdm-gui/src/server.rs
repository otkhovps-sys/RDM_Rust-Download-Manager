//! Local HTTP endpoint for the browser extension (127.0.0.1 only).
//!
//! * `GET  /ping`     -> app info and the file types to capture
//! * `POST /download` -> `{url, referer?, cookies?, userAgent?, fileName?}`
//! * `POST /show`     -> bring the main window to the front

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

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
}

/// Starts the server thread. Fails when the port is already taken.
pub fn spawn(port: u16, engine: Engine, ctx: egui::Context) -> std::io::Result<Receiver<Message>> {
    let server = Server::http(("127.0.0.1", port)).map_err(std::io::Error::other)?;
    let (tx, rx) = channel();
    std::thread::Builder::new()
        .name("rdm-server".into())
        .spawn(move || {
            for request in server.incoming_requests() {
                handle(request, &engine, &tx, &ctx);
            }
        })?;
    Ok(rx)
}

fn handle(mut request: Request, engine: &Engine, tx: &Sender<Message>, ctx: &egui::Context) {
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
    let response = match (request.method(), request.url()) {
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

/// Web pages must not be able to push downloads; only extensions (or local
/// programs, which send no `Origin`) may talk to us.
fn origin_allowed(request: &Request) -> bool {
    let origin = request.headers().iter().find(|h| h.field.equiv("Origin"));
    origin.is_none_or(|h| {
        let value = h.value.as_str();
        [
            "chrome-extension://",
            "moz-extension://",
            "safari-web-extension://",
        ]
        .iter()
        .any(|prefix| value.starts_with(prefix))
    })
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
        .filter(|b| b.url.contains("://"))
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
pub fn forward_to_running(port: u16, url: Option<&str>) -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(300)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let (path, body) = match url {
        Some(url) => ("/download", json!({ "url": url }).to_string()),
        None => ("/show", String::new()),
    };
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }
    let mut response = String::new();
    let _ = stream.read_to_string(&mut response);
    response.contains(r#""ok":true"#)
}
