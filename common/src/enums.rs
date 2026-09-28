use std::collections::HashMap;

use chrono::{DateTime, NaiveTime, Utc, Weekday};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum DownloadStatus {
    Pending,
    Active,
    Paused,
    Completed,
    Error(String),
    Removed,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum QueueStatus {
    Active,
    Paused,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
pub enum FileCategory {
    Video,
    Music,
    Document,
    Archive,
    Program,
    Other,
}

pub type CategoryExtensions = HashMap<FileCategory, Vec<String>>;

impl FileCategory {
    pub fn infer_from_filename(filename: &str, map: &CategoryExtensions) -> FileCategory {
        let ext = filename.rsplit('.').next().unwrap_or("").to_lowercase();

        for (category, extensions) in map {
            if extensions.iter().any(|e| e == &ext) {
                return category.clone();
            }
        }
        FileCategory::Other
    }

    pub fn default_extensions() -> CategoryExtensions {
        let mut map = HashMap::new();
        map.insert(
            FileCategory::Video,
            vec![
                "mp4", "mkv", "avi", "mov", "webm", "flv", "m4v", "wmv", "mpg", "mpeg", "m2v",
                "3gp", "3g2", "ogv", "ts", "mts", "m2ts", "vob", "asf",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
        );
        map.insert(
            FileCategory::Music,
            vec![
                "mp3", "flac", "wav", "aac", "ogg", "m4a", "opus", "wma", "aiff", "aif", "alac",
                "ape", "amr", "oga", "mka", "mid", "midi",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
        );
        map.insert(
            FileCategory::Document,
            vec![
                "pdf", "doc", "docx", "txt", "epub", "odt", "rtf", "xls", "xlsx", "ods", "csv",
                "ppt", "pptx", "odp", "html", "htm", "md", "tex", "mobi", "azw", "azw3", "djvu",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
        );
        map.insert(
            FileCategory::Archive,
            vec![
                "zip", "zipx", "rar", "7z", "tar", "gz", "xz", "bz2", "tgz", "tbz", "tbz2", "txz",
                "zst", "cab", "iso",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
        );
        map.insert(
            FileCategory::Program,
            vec![
                "exe",
                "msi",
                "apk",
                "deb",
                "rpm",
                "app",
                "dmg",
                "pkg",
                "bin",
                "run",
                "com",
                "jar",
                "appimage",
                "ipa",
                "appx",
                "msix",
                "msixbundle",
                "flatpak",
                "snap",
                "sh",
                "bat",
                "cmd",
                "ps1",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
        );
        map
    }
}

#[cfg(test)]
mod file_category_tests {
    use super::FileCategory;

    #[test]
    fn common_extensions_are_inferred_from_the_defaults() {
        let extensions = FileCategory::default_extensions();

        for (filename, expected) in [
            ("movie.MKV", FileCategory::Video),
            ("recording.m2ts", FileCategory::Video),
            ("album.aiff", FileCategory::Music),
            ("spreadsheet.xlsx", FileCategory::Document),
            ("slides.pptx", FileCategory::Document),
            ("backup.tgz", FileCategory::Archive),
            ("package.AppImage", FileCategory::Program),
        ] {
            assert_eq!(
                FileCategory::infer_from_filename(filename, &extensions),
                expected,
                "unexpected category for {filename}"
            );
        }
    }

    #[test]
    fn unknown_or_missing_extensions_remain_other() {
        let extensions = FileCategory::default_extensions();

        assert_eq!(
            FileCategory::infer_from_filename("README", &extensions),
            FileCategory::Other
        );
        assert_eq!(
            FileCategory::infer_from_filename("image.png", &extensions),
            FileCategory::Other
        );
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum SourceType {
    Http,
    Torrent,
    Magnet,
}

/// aria2 `--file-allocation`
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum AllocStrategy {
    None,
    Prealloc,
    Falloc,
    Trunc,
}

/// aria2 `--stream-piece-selector`
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum StreamPieceSelector {
    Default,
    InOrder,
    Random,
    Geom,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum SortField {
    CreatedAt,
    Size,
    Name,
    QueuePosition,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum Recurrence {
    Once {
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    },
    Weekly {
        days: Vec<Weekday>,
        start_time: NaiveTime,
        end_time: NaiveTime,
    },
}
