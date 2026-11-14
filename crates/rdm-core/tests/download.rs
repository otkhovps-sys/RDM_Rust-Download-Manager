//! End-to-end tests against a tiny local HTTP server.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Once};
use std::time::{Duration, Instant};

use rdm_core::{Engine, NewDownload, RequestInfo, Settings, Status};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Serves `data` at any path. Honors `Range` when `ranges` is true.
fn serve(data: Arc<Vec<u8>>, ranges: bool, connections: Arc<AtomicUsize>) -> u16 {
    serve_routes(move |_| Some(data.clone()), ranges, connections)
}

/// Serves whatever `route` returns for a path (404 for `None`). Responses are
/// sent slowly in small pieces so pausing can be tested.
fn serve_routes(
    route: impl Fn(&str) -> Option<Arc<Vec<u8>>> + Send + Sync + 'static,
    ranges: bool,
    connections: Arc<AtomicUsize>,
) -> u16 {
    let route = Arc::new(route);
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let listener = rt.block_on(TcpListener::bind("127.0.0.1:0")).unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        rt.block_on(async move {
            loop {
                let (mut sock, _) = listener.accept().await.unwrap();
                let (route, connections) = (route.clone(), connections.clone());
                tokio::spawn(async move {
                    connections.fetch_add(1, Ordering::SeqCst);
                    let mut buf = vec![0; 4096];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let request = String::from_utf8_lossy(&buf[..n]).to_lowercase();
                    let path = request.split_whitespace().nth(1).unwrap_or("/").to_owned();
                    let Some(data) = route(&path) else {
                        let _ = sock.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                        return;
                    };
                    let range = request
                        .lines()
                        .find_map(|l| l.strip_prefix("range: bytes="))
                        .filter(|_| ranges)
                        .map(|r| {
                            let (a, b) = r.trim().split_once('-').unwrap();
                            let end = if b.is_empty() { data.len() - 1 } else { b.parse().unwrap() };
                            (a.parse::<usize>().unwrap(), end)
                        });
                    let (status, start, end) = match range {
                        Some((a, b)) => ("206 Partial Content", a, b),
                        None => ("200 OK", 0, data.len() - 1),
                    };
                    let mut head = format!(
                        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nContent-Disposition: attachment; filename=\"test.bin\"\r\nConnection: close\r\n",
                        end - start + 1
                    );
                    if range.is_some() {
                        head += &format!("Content-Range: bytes {start}-{end}/{}\r\n", data.len());
                    }
                    head += "\r\n";
                    if sock.write_all(head.as_bytes()).await.is_err() {
                        return;
                    }
                    for chunk in data[start..=end].chunks(64 * 1024) {
                        if sock.write_all(chunk).await.is_err() {
                            return;
                        }
                        tokio::time::sleep(Duration::from_millis(2)).await;
                    }
                });
            }
        });
    });
    port
}

fn test_data(len: usize) -> Arc<Vec<u8>> {
    let mut x: u32 = 12345;
    Arc::new(
        (0..len)
            .map(|_| {
                x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
                (x >> 16) as u8
            })
            .collect(),
    )
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rdm-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn engine(name: &str) -> (Engine, PathBuf) {
    static DATA_DIR: Once = Once::new();
    // SAFETY: set once, before any engine (and its threads) exists.
    DATA_DIR.call_once(|| unsafe { std::env::set_var("RDM_DATA_DIR", temp_dir("data")) });
    let dir = temp_dir(name);
    let engine = Engine::new().unwrap();
    engine.set_settings(Settings {
        connections: 8,
        download_dir: dir.clone(),
        ..Settings::default()
    });
    (engine, dir)
}

fn wait_for(engine: &Engine, id: u64, done: impl Fn(&Status) -> bool) -> rdm_core::Download {
    let start = Instant::now();
    loop {
        let d = engine.get(id).unwrap();
        if done(&d.status) {
            return d;
        }
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "timeout, status {:?}",
            d.status
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn add(engine: &Engine, port: u16, dir: &std::path::Path) -> u64 {
    let url = format!("http://127.0.0.1:{port}/files/test.bin");
    let mut handle = engine.probe(&url, RequestInfo::default());
    let start = Instant::now();
    let probe = loop {
        if let Some(result) = handle.try_take() {
            break result.unwrap();
        }
        assert!(start.elapsed() < Duration::from_secs(10), "probe timeout");
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(probe.file_name, "test.bin");
    let mut new = NewDownload::from_probe(&probe, RequestInfo::default(), &engine.settings());
    new.save_dir = dir.to_path_buf();
    engine.add(new)
}

#[test]
fn segmented_download_with_pause_and_resume() {
    let data = test_data(24 * 1024 * 1024 + 123);
    let connections = Arc::new(AtomicUsize::new(0));
    let port = serve(data.clone(), true, connections.clone());
    let (engine, dir) = engine("segmented");

    let id = add(&engine, port, &dir);
    let d = engine.get(id).unwrap();
    assert!(d.resumable);
    assert_eq!(d.size, Some(data.len() as u64));

    // Pause in the middle, then resume.
    wait_for(&engine, id, |_| {
        engine.get(id).unwrap().downloaded > 4 * 1024 * 1024
    });
    engine.pause(id);
    std::thread::sleep(Duration::from_millis(300));
    let paused = engine.get(id).unwrap();
    assert_eq!(paused.status, Status::Paused);
    assert!(paused.downloaded < data.len() as u64);
    engine.resume(id);

    let d = wait_for(&engine, id, |s| {
        matches!(s, Status::Completed | Status::Failed(_))
    });
    assert_eq!(d.status, Status::Completed);
    assert!(
        d.segments.len() >= 8,
        "expected parallel segments, got {}",
        d.segments.len()
    );
    assert!(connections.load(Ordering::SeqCst) > 8);
    assert_eq!(std::fs::read(d.path()).unwrap(), *data);
    assert!(!d.part_path().exists());
    engine.shutdown();
}

#[test]
fn single_connection_without_range_support() {
    let data = test_data(3 * 1024 * 1024);
    let port = serve(data.clone(), false, Arc::new(AtomicUsize::new(0)));
    let (engine, dir) = engine("single");

    let id = add(&engine, port, &dir);
    assert!(!engine.get(id).unwrap().resumable);
    let d = wait_for(&engine, id, |s| {
        matches!(s, Status::Completed | Status::Failed(_))
    });
    assert_eq!(d.status, Status::Completed);
    assert_eq!(std::fs::read(d.path()).unwrap(), *data);
    engine.shutdown();
}

#[test]
fn hls_stream_with_aes128() {
    use aes::cipher::{BlockEncryptMut, KeyIvInit, block_padding::Pkcs7};
    type Enc = cbc::Encryptor<aes::Aes128>;

    let key = *b"0123456789abcdef";
    let parts: Vec<Vec<u8>> = (0..6)
        .map(|i| test_data(200_000 + i * 1000).to_vec())
        .collect();
    let playlist = {
        let mut p = "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:3\n#EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\"\n"
            .to_owned();
        for i in 0..parts.len() {
            p += &format!("#EXTINF:4,\nseg{i}.ts\n");
        }
        p + "#EXT-X-ENDLIST\n"
    };
    let master = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=100\nlow/index.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=900\nhd/index.m3u8\n";
    let encrypted: Vec<Arc<Vec<u8>>> = parts
        .iter()
        .enumerate()
        .map(|(i, part)| {
            let iv = (3 + i as u128).to_be_bytes(); // IV = media sequence number
            Arc::new(Enc::new(&key.into(), &iv.into()).encrypt_padded_vec_mut::<Pkcs7>(part))
        })
        .collect();

    let port = serve_routes(
        move |path| match path {
            "/video/master.m3u8" => Some(Arc::new(master.as_bytes().to_vec())),
            "/video/hd/index.m3u8" => Some(Arc::new(playlist.clone().into_bytes())),
            "/video/hd/key.bin" => Some(Arc::new(key.to_vec())),
            _ => path
                .strip_prefix("/video/hd/seg")
                .and_then(|rest| rest.strip_suffix(".ts"))
                .and_then(|n| encrypted.get(n.parse::<usize>().ok()?).cloned()),
        },
        false,
        Arc::new(AtomicUsize::new(0)),
    );
    let (engine, dir) = engine("hls");
    let url = format!("http://127.0.0.1:{port}/video/master.m3u8");
    let mut handle = engine.probe(&url, RequestInfo::default());
    let probe = loop {
        if let Some(result) = handle.try_take() {
            break result.unwrap();
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(probe.kind, rdm_core::Kind::Hls);
    assert_eq!(probe.hls_segments, Some(6));
    assert_eq!(probe.file_name, "master.ts");

    let mut new = NewDownload::from_probe(&probe, RequestInfo::default(), &engine.settings());
    new.save_dir = dir.clone();
    let id = engine.add(new);
    let d = wait_for(&engine, id, |s| {
        matches!(s, Status::Completed | Status::Failed(_))
    });
    assert_eq!(d.status, Status::Completed);
    assert_eq!(std::fs::read(d.path()).unwrap(), parts.concat());
    engine.shutdown();
}
