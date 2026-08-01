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
            vec!["mp4", "mkv", "avi", "mov", "webm", "flv", "m4v"]
                .into_iter()
                .map(String::from)
                .collect(),
        );
        map.insert(
            FileCategory::Music,
            vec!["mp3", "flac", "wav", "aac", "ogg", "m4a", "opus"]
                .into_iter()
                .map(String::from)
                .collect(),
        );
        map.insert(
            FileCategory::Document,
            vec!["pdf", "doc", "docx", "txt", "epub", "odt", "rtf"]
                .into_iter()
                .map(String::from)
                .collect(),
        );
        map.insert(
            FileCategory::Archive,
            vec!["zip", "rar", "7z", "tar", "gz", "xz", "bz2"]
                .into_iter()
                .map(String::from)
                .collect(),
        );
        map.insert(
            FileCategory::Program,
            vec![
                "exe", "msi", "apk", "deb", "rpm", "app", "dmg", "pkg", "bin", "run", "com", "jar",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
        );
        map
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
