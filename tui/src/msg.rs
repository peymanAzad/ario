use std::path::PathBuf;

use chrono::{DateTime, Utc};
use common::{
    download::{
        AddDownloadsRequest, DeleteDownloadFilesResult, DownloadLiveStatus, TorrentUploadMetadata,
    },
    finetune::{Aria2GlobalOptions, FineTune},
    queue::{CreateQueueRequest, Queue, UpdateQueueRequest},
};

use crate::{
    api::DeleteQueueOutcome,
    app::{Focus, LifecycleState, PendingConfirmationAction},
    modal::QueueModalMode,
    toast::ToastLevel,
};

#[derive(Debug)]
pub enum Msg {
    Tick,
    Paste(String),
    Action(Action),
    Lifecycle(LifecycleState),
    Api(ApiResult),
    Toast {
        message: String,
        level: ToastLevel,
    },
}

#[derive(Clone, Debug)]
#[allow(clippy::large_enum_variant)]
pub enum Action {
    Quit,
    FocusNext,
    FocusPrev,
    Focus(Focus),
    SelectNext,
    SelectPrev,
    OpenHelp,
    OpenClipboardImport,
    OpenTorrentFile,
    OpenCreateQueue,
    OpenEditQueue,
    PauseDownload,
    ResumeDownload,
    DeleteDownload,
    DeleteDownloadFiles,
    OpenDownloadFolder,
    ActivateDownload,
    PauseQueue,
    ResumeQueue,
    DeleteQueue,
    RemoveCompleted,
    SubmitDownloads(AddDownloadsRequest),
    SubmitTorrent {
        path: PathBuf,
        metadata: TorrentUploadMetadata,
    },
    SaveQueue {
        mode: QueueModalMode,
        create: Option<CreateQueueRequest>,
        update: Option<(i64, UpdateQueueRequest)>,
        ordered_ids: Vec<i64>,
    },
    SaveDownloadEdit {
        id: i64,
        finetune: FineTune,
        queue_id: i64,
        original_queue_id: i64,
    },
    Confirm(PendingConfirmationAction),
}

#[derive(Debug)]
pub enum ApiResult {
    Refreshed {
        downloads: anyhow::Result<Vec<DownloadLiveStatus>>,
        queues: anyhow::Result<Vec<Queue>>,
        server_reachable: bool,
        aria2_reachable: bool,
        download_speed: u64,
        active_downloads: u64,
        aria2_global_options: Option<Aria2GlobalOptions>,
        lifecycle_revision: u64,
    },
    QueueDownloadsLoaded {
        queue_id: i64,
        result: anyhow::Result<Vec<DownloadLiveStatus>>,
    },
    QueueSaved(anyhow::Result<()>),
    DownloadPaused {
        download_id: i64,
        result: anyhow::Result<DownloadLiveStatus>,
    },
    DownloadFilesDeleted(anyhow::Result<DeleteDownloadFilesResult>),
    TorrentAdded(anyhow::Result<DownloadLiveStatus>),
    QueueDeleteResolved {
        queue_id: i64,
        queue_name: String,
        result: anyhow::Result<DeleteQueueOutcome>,
    },
    QueueResumed {
        #[allow(dead_code)]
        queue_id: i64,
        result: anyhow::Result<Option<DateTime<Utc>>>,
    },
    Failed {
        error: String,
    },
    /// Successful fire-and-forget call that should trigger a refresh.
    Done,
}

impl ApiResult {
    pub fn from_unit(result: anyhow::Result<()>) -> Self {
        match result {
            Ok(()) => Self::Done,
            Err(error) => Self::Failed {
                error: error.to_string(),
            },
        }
    }
}
