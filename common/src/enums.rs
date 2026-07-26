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
