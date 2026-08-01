use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    enums::{DownloadStatus, FileCategory, SortField, SourceType},
    finetune::FineTune,
};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Download {
    pub id: i64,
    /// aria2's own GID for this download
    pub aria2_gid: Option<String>,
    pub url: String,
    pub filename: Option<String>,
    pub destination_path: String,
    pub source_type: SourceType,
    pub category: FileCategory,
    pub status: DownloadStatus,
    pub paused_by_scheduler: bool,
    pub size: Option<u64>,
    pub queue_id: i64,
    pub position_in_queue: i32,
    pub finetune: FineTune,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

/// Live status merged from aria2
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DownloadLiveStatus {
    pub download: Download,
    pub completed_length: u64,
    pub download_speed: u64,
    pub eta_seconds: Option<u64>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum AddDownloadInput {
    /// A plain HTTP(S) or magnet URL.
    Url(String),
    /// Raw .torrent file bytes
    TorrentFile { filename: String, data: Vec<u8> },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AddDownloadsRequest {
    pub inputs: Vec<AddDownloadInput>,
    pub queue_id: i64,
    /// `None` = use the queue's `default_finetune` as-is.
    pub finetune_override: Option<FineTune>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct DownloadFilter {
    pub queue_id: Option<i64>,
    pub status: Option<DownloadStatus>,
    pub category: Option<FileCategory>,
    pub sort_by: Option<SortField>,
    #[serde(default)]
    pub sort_desc: bool,
}
