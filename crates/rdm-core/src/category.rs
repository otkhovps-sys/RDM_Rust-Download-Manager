use serde::{Deserialize, Serialize};

/// File categories, the same set IDM uses for its sidebar and save folders.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Category {
    Compressed,
    Documents,
    Music,
    Programs,
    Video,
    Other,
}

impl Category {
    pub const ALL: [Category; 6] = [
        Self::Compressed,
        Self::Documents,
        Self::Music,
        Self::Programs,
        Self::Video,
        Self::Other,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Compressed => "Compressed",
            Self::Documents => "Documents",
            Self::Music => "Music",
            Self::Programs => "Programs",
            Self::Video => "Video",
            Self::Other => "Other",
        }
    }

    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Compressed => &[
                "zip", "rar", "7z", "tar", "gz", "tgz", "bz2", "xz", "zst", "lz", "lzma", "cab",
                "iso", "img", "arj", "z",
            ],
            Self::Documents => &[
                "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "odt", "ods", "odp", "rtf",
                "txt", "csv", "epub", "mobi", "djvu", "md",
            ],
            Self::Music => &[
                "mp3", "m4a", "aac", "flac", "wav", "ogg", "oga", "opus", "wma", "aif", "aiff",
                "mid", "midi", "ape",
            ],
            Self::Programs => &[
                "exe", "msi", "msix", "appx", "apk", "dmg", "pkg", "deb", "rpm", "appimage",
                "flatpak", "snap", "bin", "run", "jar",
            ],
            Self::Video => &[
                "mp4", "m4v", "mkv", "webm", "avi", "mov", "wmv", "flv", "mpg", "mpeg", "3gp",
                "ts", "m2ts", "ogv", "m3u8", "vob", "rm", "rmvb",
            ],
            Self::Other => &[],
        }
    }

    /// Picks a category from the file name's extension.
    pub fn from_file_name(name: &str) -> Self {
        let Some((_, ext)) = name.rsplit_once('.') else {
            return Self::Other;
        };
        let ext = ext.to_ascii_lowercase();
        Self::ALL
            .into_iter()
            .find(|c| c.extensions().contains(&ext.as_str()))
            .unwrap_or(Self::Other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_categories() {
        assert_eq!(Category::from_file_name("a.ZIP"), Category::Compressed);
        assert_eq!(Category::from_file_name("movie.mkv"), Category::Video);
        assert_eq!(Category::from_file_name("song.flac"), Category::Music);
        assert_eq!(Category::from_file_name("setup.exe"), Category::Programs);
        assert_eq!(Category::from_file_name("paper.pdf"), Category::Documents);
        assert_eq!(Category::from_file_name("noext"), Category::Other);
    }
}
