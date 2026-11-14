//! Small formatting and file-name helpers shared by the engine and GUI.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use percent_encoding::percent_decode_str;

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.2} {}", UNITS[unit])
    }
}

pub fn format_speed(bytes_per_sec: u64) -> String {
    format!("{}/s", format_bytes(bytes_per_sec))
}

pub fn format_duration(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    match (h, m) {
        (0, 0) => format!("{s} sec"),
        (0, _) => format!("{m} min {s} sec"),
        _ => format!("{h} hr {m} min"),
    }
}

/// Unix timestamp as local `YYYY-MM-DD HH:MM`.
pub fn format_timestamp(secs: u64) -> String {
    chrono::DateTime::from_timestamp(secs as i64, 0)
        .map(|t| {
            t.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

/// Removes characters that are invalid in file names on any major OS.
pub fn sanitize_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').trim();
    if trimmed.is_empty() {
        "download".to_owned()
    } else {
        trimmed.chars().take(200).collect()
    }
}

/// Last path segment of a URL, percent-decoded.
pub fn file_name_from_url(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    let last = parsed.path_segments()?.rev().find(|s| !s.is_empty())?;
    let decoded = percent_decode_str(last).decode_utf8_lossy();
    Some(sanitize_file_name(&decoded))
}

/// Extracts the file name from a `Content-Disposition` header.
pub fn parse_content_disposition(header: &str) -> Option<String> {
    let mut plain = None;
    for part in header.split(';').map(str::trim) {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim().to_ascii_lowercase().as_str() {
            // RFC 5987: filename*=UTF-8''na%C3%AFve.txt
            "filename*" => {
                let encoded = value.rsplit("''").next().unwrap_or(value);
                let decoded = percent_decode_str(encoded.trim_matches('"')).decode_utf8_lossy();
                return Some(sanitize_file_name(&decoded));
            }
            "filename" => plain = Some(sanitize_file_name(value.trim_matches('"'))),
            _ => {}
        }
    }
    plain
}

/// Suggests an extension for a MIME type when the URL has none.
pub fn extension_for_mime(mime: &str) -> Option<&'static str> {
    let mime = mime.split(';').next()?.trim().to_ascii_lowercase();
    Some(match mime.as_str() {
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        "video/x-matroska" => "mkv",
        "video/quicktime" => "mov",
        "video/x-flv" => "flv",
        "video/mp2t" => "ts",
        "audio/mpeg" => "mp3",
        "audio/mp4" | "audio/x-m4a" => "m4a",
        "audio/aac" => "aac",
        "audio/ogg" => "ogg",
        "audio/webm" => "weba",
        "audio/flac" => "flac",
        "audio/wav" | "audio/x-wav" => "wav",
        "application/pdf" => "pdf",
        "application/zip" => "zip",
        "application/x-7z-compressed" => "7z",
        "application/vnd.rar" | "application/x-rar-compressed" => "rar",
        "application/gzip" => "gz",
        "application/x-msdownload" => "exe",
        "application/vnd.android.package-archive" => "apk",
        "application/x-iso9660-image" => "iso",
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        _ => return None,
    })
}

/// Returns `name`, or `name (1)`, `name (2)` ... so no existing file in `dir`
/// is overwritten. `taken` lets the caller reserve names of queued downloads.
pub fn unique_file_name(dir: &Path, name: &str, taken: impl Fn(&str) -> bool) -> String {
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem, format!(".{ext}")),
        _ => (name, String::new()),
    };
    let is_free = |candidate: &str| {
        !taken(candidate)
            && !dir.join(candidate).exists()
            && !dir.join(format!("{candidate}.rdm-part")).exists()
    };
    if is_free(name) {
        return name.to_owned();
    }
    (1..)
        .map(|i| format!("{stem} ({i}){ext}"))
        .find(|candidate| is_free(candidate))
        .expect("infinite iterator")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_bytes() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1536), "1.50 KB");
        assert_eq!(format_bytes(5 * 1024 * 1024 * 1024), "5.00 GB");
    }

    #[test]
    fn content_disposition() {
        assert_eq!(
            parse_content_disposition(r#"attachment; filename="report 1.pdf""#).as_deref(),
            Some("report 1.pdf")
        );
        assert_eq!(
            parse_content_disposition(
                "attachment; filename=a.txt; filename*=UTF-8''na%C3%AFve.txt"
            )
            .as_deref(),
            Some("naïve.txt")
        );
        assert_eq!(parse_content_disposition("inline"), None);
    }

    #[test]
    fn url_names() {
        assert_eq!(
            file_name_from_url("https://x.org/dl/My%20File.zip?x=1").as_deref(),
            Some("My File.zip")
        );
        assert_eq!(file_name_from_url("https://x.org/").as_deref(), None);
    }

    #[test]
    fn sanitizes() {
        assert_eq!(sanitize_file_name("a/b:c?.txt"), "a_b_c_.txt");
        assert_eq!(sanitize_file_name("  ..  "), "download");
    }

    #[test]
    fn unique_names() {
        let dir = std::env::temp_dir();
        let taken = |n: &str| n == "file.bin" || n == "file (1).bin";
        assert_eq!(unique_file_name(&dir, "file.bin", taken), "file (2).bin");
    }
}
