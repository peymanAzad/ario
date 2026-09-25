use crate::aria2::Aria2AddMode;
use crate::db::{DownloadArtifact, DownloadArtifactKind};
use crate::error::AppError;
use crate::state::AppState;
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Multipart, Path, Query, State},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use chrono::Utc;
use common::{
    download::{
        AddDownloadInput, AddDownloadsRequest, DeleteDownloadFilesResult, Download, DownloadFilter,
        DownloadLiveStatus, TorrentUploadMetadata,
    },
    enums::{DownloadStatus, FileCategory, SourceType},
    finetune::FineTune,
    queue::QueueSettings,
};
use std::{collections::HashSet, fs, io::ErrorKind, path::PathBuf};

const MAX_TORRENT_BYTES: usize = 16 * 1024 * 1024;
const MAX_TORRENT_REQUEST_BYTES: usize = 17 * 1024 * 1024;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/downloads", get(list_downloads).post(add_downloads))
        .route(
            "/downloads/torrent",
            post(add_torrent_download).layer(DefaultBodyLimit::max(MAX_TORRENT_REQUEST_BYTES)),
        )
        .route("/downloads/completed", delete(delete_completed_downloads))
        .route("/downloads/{id}", get(get_download).delete(delete_download))
        .route(
            "/downloads/{id}/finetune",
            axum::routing::put(update_finetune),
        )
        .route(
            "/downloads/{id}/queue",
            axum::routing::put(update_download_queue),
        )
        .route("/downloads/{id}/pause", axum::routing::post(pause_download))
        .route(
            "/downloads/{id}/resume",
            axum::routing::post(resume_download),
        )
        .route(
            "/queues/{queue_id}/reorder",
            axum::routing::put(reorder_queue),
        )
}

async fn merge_live(state: &AppState, download: Download) -> DownloadLiveStatus {
    let live = state.live_status.read().await.get(&download.id).copied();
    let completed_length = match live {
        Some(stats) => crate::live_status::coalesce_completed_length(
            stats.completed_length,
            download.completed_length,
        ),
        None => download.completed_length.unwrap_or(0),
    };
    let download_speed = if download.status == DownloadStatus::Active {
        live.map(|stats| stats.download_speed).unwrap_or(0)
    } else {
        0
    };

    let eta_seconds = match (download.size, download_speed) {
        (Some(total), speed) if speed > 0 && total > completed_length => {
            Some((total - completed_length) / speed)
        }
        _ => None,
    };

    DownloadLiveStatus {
        completed_length,
        download_speed,
        eta_seconds,
        download,
    }
}

fn resolve_finetune(settings: &QueueSettings, overrides: Option<FineTune>) -> FineTune {
    let mut finetune = settings.default_finetune.clone();
    finetune.max_retries = Some(settings.max_retries);
    finetune.retry_wait_seconds = Some(settings.retry_wait_seconds);
    if let Some(overrides) = overrides {
        finetune.apply_override(overrides);
    }
    finetune
}

/// `GET /downloads?queue_id=&status=&category=&sort_by=&sort_desc=`
async fn list_downloads(
    State(state): State<AppState>,
    Query(filter): Query<DownloadFilter>,
) -> Result<Json<Vec<DownloadLiveStatus>>, AppError> {
    let downloads = state.db.list_downloads(&filter)?;
    let mut result = Vec::with_capacity(downloads.len());
    for d in downloads {
        result.push(merge_live(&state, d).await);
    }
    Ok(Json(result))
}

async fn get_download(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<DownloadLiveStatus>, AppError> {
    let download = state
        .db
        .get_download(id)?
        .ok_or_else(|| AppError::NotFound(format!("download {id}")))?;
    Ok(Json(merge_live(&state, download).await))
}

async fn add_downloads(
    State(state): State<AppState>,
    Json(req): Json<AddDownloadsRequest>,
) -> Result<Json<Vec<DownloadLiveStatus>>, AppError> {
    let _activity = state.activity_guard().await?;
    if req.inputs.is_empty() {
        return Err(AppError::BadRequest("inputs must not be empty".into()));
    }

    let queue = state
        .db
        .get_queue(req.queue_id)?
        .ok_or_else(|| AppError::BadRequest(format!("queue {} does not exist", req.queue_id)))?;

    let finetune = resolve_finetune(&queue.settings, req.finetune_override);
    let next_position = state.db.next_position_in_queue(queue.id)?;

    let mut created = Vec::with_capacity(req.inputs.len());

    for (position, input) in (next_position..).zip(req.inputs) {
        let download = create_download(
            &state,
            input,
            queue.id,
            position,
            finetune.clone(),
            req.start_immediately,
        )
        .await?;
        created.push(download);
    }

    Ok(Json(created))
}

async fn add_torrent_download(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<DownloadLiveStatus>, AppError> {
    let _activity = state.activity_guard().await?;
    let mut metadata = None;
    let mut file = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|error| AppError::BadRequest(format!("invalid multipart body: {error}")))?
    {
        match field.name() {
            Some("metadata") => {
                let bytes = field.bytes().await.map_err(|error| {
                    AppError::BadRequest(format!("invalid metadata part: {error}"))
                })?;
                metadata = Some(
                    serde_json::from_slice::<TorrentUploadMetadata>(&bytes).map_err(|error| {
                        AppError::BadRequest(format!("invalid metadata JSON: {error}"))
                    })?,
                );
            }
            Some("file") => {
                let filename = field
                    .file_name()
                    .and_then(safe_filename)
                    .ok_or_else(|| AppError::BadRequest("torrent filename is required".into()))?;
                let bytes = field.bytes().await.map_err(|error| {
                    AppError::BadRequest(format!("invalid torrent file part: {error}"))
                })?;
                if bytes.len() > MAX_TORRENT_BYTES {
                    return Err(AppError::PayloadTooLarge(format!(
                        "torrent file exceeds the {MAX_TORRENT_BYTES} byte limit"
                    )));
                }
                file = Some((filename, bytes.to_vec()));
            }
            _ => {}
        }
    }

    let metadata =
        metadata.ok_or_else(|| AppError::BadRequest("metadata part is required".into()))?;
    let (filename, data) =
        file.ok_or_else(|| AppError::BadRequest("file part is required".into()))?;
    let queue = state.db.get_queue(metadata.queue_id)?.ok_or_else(|| {
        AppError::BadRequest(format!("queue {} does not exist", metadata.queue_id))
    })?;
    let finetune = resolve_finetune(&queue.settings, metadata.finetune_override);
    let position = state.db.next_position_in_queue(queue.id)?;
    let created = create_download(
        &state,
        AddDownloadInput::TorrentFile { filename, data },
        queue.id,
        position,
        finetune,
        metadata.start_immediately,
    )
    .await?;
    Ok(Json(created))
}

fn safe_filename(filename: &str) -> Option<String> {
    filename
        .rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty() && *name != "." && *name != "..")
        .map(str::to_owned)
}

async fn create_download(
    state: &AppState,
    input: AddDownloadInput,
    queue_id: i64,
    position: i32,
    finetune: FineTune,
    start_immediately: bool,
) -> Result<DownloadLiveStatus, AppError> {
    let (url, filename, source_type, torrent_data) = match input {
        AddDownloadInput::Url(url) => {
            let filename = url.rsplit('/').next().map(str::to_string);
            let source_type = if url.starts_with("magnet:") {
                SourceType::Magnet
            } else {
                SourceType::Http
            };
            (url, filename, source_type, None)
        }
        AddDownloadInput::TorrentFile { filename, data } => {
            if data.is_empty() {
                return Err(AppError::BadRequest(
                    "torrent file must not be empty".into(),
                ));
            }
            if data.len() > MAX_TORRENT_BYTES {
                return Err(AppError::PayloadTooLarge(format!(
                    "torrent file exceeds the {MAX_TORRENT_BYTES} byte limit"
                )));
            }
            let filename = safe_filename(&filename)
                .ok_or_else(|| AppError::BadRequest("torrent filename is required".into()))?;
            (
                String::new(),
                Some(filename),
                SourceType::Torrent,
                Some(data),
            )
        }
    };

    let category = filename
        .as_deref()
        .map(|filename| {
            FileCategory::infer_from_filename(filename, &state.config.settings.category_extensions)
        })
        .unwrap_or(FileCategory::Other);
    let configured_path = state
        .config
        .settings
        .category_locations
        .get(&category)
        .unwrap_or(&state.config.settings.default_download_location);
    let destination_path = crate::config::expand_tilde(configured_path)?
        .to_string_lossy()
        .to_string();
    let mut download = Download {
        id: 0,
        aria2_gid: None,
        url,
        filename,
        destination_path,
        source_type,
        category,
        status: DownloadStatus::Pending,
        paused_by_scheduler: false,
        manually_started: false,
        size: None,
        completed_length: None,
        queue_id,
        position_in_queue: position,
        finetune,
        created_at: Utc::now(),
        started_at: None,
        completed_at: None,
    };

    download.id = match torrent_data.as_deref() {
        Some(data) => state.db.insert_download_with_torrent(&download, data)?,
        None => state.db.insert_download(&download)?,
    };

    if start_immediately {
        state.db.set_manually_started(download.id, true)?;
        state.db.set_paused_by_scheduler(download.id, false)?;
        match start_in_aria2(state, &download, Aria2AddMode::Fresh).await {
            Ok(gid) => {
                state.db.update_download_gid(download.id, &gid)?;
                state
                    .db
                    .update_download_status(download.id, &DownloadStatus::Active)?;
            }
            Err(error) => {
                state.db.set_manually_started(download.id, false)?;
                state.db.update_download_status(
                    download.id,
                    &DownloadStatus::Error(error.to_string()),
                )?;
            }
        }
    }

    let row = state
        .db
        .get_download(download.id)?
        .ok_or_else(|| AppError::NotFound(format!("download {}", download.id)))?;
    Ok(merge_live(state, row).await)
}

#[derive(Default, serde::Deserialize)]
struct DeleteDownloadQuery {
    #[serde(default)]
    delete_files: bool,
}

async fn delete_download(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(query): Query<DeleteDownloadQuery>,
) -> Result<Response, AppError> {
    let _activity = state.activity_guard().await?;
    let download = state
        .db
        .get_download(id)?
        .ok_or_else(|| AppError::NotFound(format!("download {id}")))?;

    if query.delete_files {
        return delete_download_with_files(&state, download)
            .await
            .map(|result| Json(result).into_response());
    }

    delete_download_record(&state, download).await?;
    Ok(axum::http::StatusCode::NO_CONTENT.into_response())
}

/// Remove a download from aria2 and application state while retaining every
/// downloaded file. This is the shared implementation of the TUI's lowercase
/// `d` action and confirmed queue deletion.
pub(crate) async fn delete_download_record(
    state: &AppState,
    download: Download,
) -> Result<(), AppError> {
    if let Some(gid) = &download.aria2_gid {
        let _ = state.aria2.remove(gid).await;
    }

    state.db.delete_download(download.id)?;
    state.live_status.write().await.remove(&download.id);
    Ok(())
}

async fn delete_download_with_files(
    state: &AppState,
    download: Download,
) -> Result<DeleteDownloadFilesResult, AppError> {
    let mut artifacts = state.db.list_download_artifacts(download.id)?;
    let mut metadata_complete = artifacts
        .iter()
        .any(|artifact| artifact.kind == DownloadArtifactKind::Payload);

    if let Some(gid) = &download.aria2_gid
        && let Ok(status) = state.aria2.tell_status(gid).await
    {
        let (payloads, controls) = status.artifact_paths();
        if !payloads.is_empty() {
            metadata_complete = true;
            artifacts.extend(payloads.into_iter().map(|path| DownloadArtifact {
                path,
                kind: DownloadArtifactKind::Payload,
            }));
            artifacts.extend(controls.into_iter().map(|path| DownloadArtifact {
                path,
                kind: DownloadArtifactKind::Control,
            }));
        }
    }

    if !metadata_complete
        && (download.aria2_gid.is_some() || matches!(download.status, DownloadStatus::Completed))
        && let Some(filename) = &download.filename
    {
        let path = PathBuf::from(&download.destination_path).join(filename);
        artifacts.push(DownloadArtifact {
            path: path.to_string_lossy().into_owned(),
            kind: DownloadArtifactKind::Payload,
        });
        artifacts.push(DownloadArtifact {
            path: format!("{}.aria2", path.to_string_lossy()),
            kind: DownloadArtifactKind::Control,
        });
        // HTTP(S) downloads have one payload, so the resolved legacy
        // filename is a complete fallback. Torrent and magnet rows may
        // represent many payloads and remain explicitly incomplete.
        if download.source_type == SourceType::Http {
            metadata_complete = true;
        }
    }

    let was_running = matches!(
        download.status,
        DownloadStatus::Pending | DownloadStatus::Active | DownloadStatus::Paused
    );
    // Reserve the row before stopping aria2 so the scheduler cannot start a
    // pending/paused item while destructive deletion is in progress.
    state
        .db
        .update_download_status(download.id, &DownloadStatus::Removed)?;
    if let Some(gid) = &download.aria2_gid {
        if was_running && let Err(error) = state.aria2.remove(gid).await {
            state
                .db
                .update_download_status(download.id, &download.status)?;
            return Err(error.into());
        }
        let _ = state.aria2.remove_download_result(gid).await;
    }

    let result = remove_download_artifacts(&artifacts, metadata_complete)?;
    state.db.delete_download(download.id)?;
    state.live_status.write().await.remove(&download.id);
    Ok(result)
}

fn remove_download_artifacts(
    artifacts: &[DownloadArtifact],
    metadata_complete: bool,
) -> Result<DeleteDownloadFilesResult, AppError> {
    let mut payloads = HashSet::new();
    let mut controls = HashSet::new();
    for artifact in artifacts {
        match artifact.kind {
            DownloadArtifactKind::Payload => {
                payloads.insert(PathBuf::from(&artifact.path));
            }
            DownloadArtifactKind::Control => {
                controls.insert(PathBuf::from(&artifact.path));
            }
        }
    }

    let mut removed_payloads = 0;
    let mut missing_payloads = 0;
    if payloads.is_empty() {
        missing_payloads = 1;
    }
    for path in payloads {
        match fs::remove_file(&path) {
            Ok(()) => removed_payloads += 1,
            Err(error) if error.kind() == ErrorKind::NotFound => missing_payloads += 1,
            Err(error) => {
                return Err(AppError::Internal(format!(
                    "failed to remove {}: {error}",
                    path.display()
                )));
            }
        }
    }

    for path in controls {
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => {
                return Err(AppError::Internal(format!(
                    "failed to remove aria2 control file {}: {error}",
                    path.display()
                )));
            }
        }
    }

    Ok(DeleteDownloadFilesResult {
        removed_payloads,
        missing_payloads,
        metadata_complete,
    })
}

#[derive(serde::Deserialize)]
struct DeleteCompletedQuery {
    queue_id: Option<i64>,
}

async fn delete_completed_downloads(
    State(state): State<AppState>,
    Query(query): Query<DeleteCompletedQuery>,
) -> Result<axum::http::StatusCode, AppError> {
    let _activity = state.activity_guard().await?;
    let filter = DownloadFilter {
        queue_id: query.queue_id,
        status: Some(DownloadStatus::Completed),
        ..DownloadFilter::default()
    };
    let completed = state.db.list_downloads(&filter)?;

    for download in &completed {
        if let Some(gid) = &download.aria2_gid {
            let _ = state.aria2.remove_download_result(gid).await;
        }
    }

    state.db.delete_completed_downloads(query.queue_id)?;
    let mut live_status = state.live_status.write().await;
    for download in completed {
        live_status.remove(&download.id);
    }

    Ok(axum::http::StatusCode::NO_CONTENT)
}

async fn update_finetune(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(finetune): Json<FineTune>,
) -> Result<Json<DownloadLiveStatus>, AppError> {
    let _activity = state.activity_guard().await?;
    let download = state
        .db
        .get_download(id)?
        .ok_or_else(|| AppError::NotFound(format!("download {id}")))?;
    let queue = state
        .db
        .get_queue(download.queue_id)?
        .ok_or_else(|| AppError::NotFound(format!("queue {}", download.queue_id)))?;
    let finetune = resolve_finetune(&queue.settings, Some(finetune));
    state.db.update_download_finetune(id, &finetune)?;
    let updated = state
        .db
        .get_download(id)?
        .ok_or_else(|| AppError::NotFound(format!("download {id}")))?;
    Ok(Json(merge_live(&state, updated).await))
}

#[derive(serde::Deserialize)]
struct UpdateDownloadQueueRequest {
    queue_id: i64,
}

async fn update_download_queue(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<UpdateDownloadQueueRequest>,
) -> Result<Json<DownloadLiveStatus>, AppError> {
    let _activity = state.activity_guard().await?;
    let download = state
        .db
        .get_download(id)?
        .ok_or_else(|| AppError::NotFound(format!("download {id}")))?;
    state
        .db
        .get_queue(req.queue_id)?
        .ok_or_else(|| AppError::BadRequest(format!("queue {} does not exist", req.queue_id)))?;

    if download.queue_id != req.queue_id {
        let position = state.db.next_position_in_queue(req.queue_id)?;
        state.db.update_download_queue(id, req.queue_id, position)?;
    }

    let updated = state
        .db
        .get_download(id)?
        .ok_or_else(|| AppError::NotFound(format!("download {id}")))?;
    Ok(Json(merge_live(&state, updated).await))
}

async fn pause_download(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<DownloadLiveStatus>, AppError> {
    let _activity = state.activity_guard().await?;
    let download = state
        .db
        .get_download(id)?
        .ok_or_else(|| AppError::NotFound(format!("download {id}")))?;
    let gid = download
        .aria2_gid
        .ok_or_else(|| AppError::BadRequest("download has not been started in aria2 yet".into()))?;

    state.aria2.pause(&gid).await?;
    state.db.set_paused_by_scheduler(id, false)?;
    state.db.set_manually_started(id, false)?;

    let updated = state
        .db
        .get_download(id)?
        .ok_or_else(|| AppError::NotFound(format!("download {id}")))?;
    Ok(Json(merge_live(&state, updated).await))
}

pub(crate) async fn start_in_aria2(
    state: &AppState,
    download: &Download,
    mode: Aria2AddMode,
) -> Result<String, AppError> {
    match download.source_type {
        SourceType::Torrent => {
            use base64::Engine;
            let data = state
                .db
                .get_download_torrent_data(download.id)?
                .ok_or_else(|| {
                    AppError::BadRequest(
                        "torrent metainfo is unavailable; add the .torrent file again".into(),
                    )
                })?;
            let encoded = base64::engine::general_purpose::STANDARD.encode(data);
            Ok(state
                .aria2
                .add_torrent(
                    &encoded,
                    &download.finetune,
                    &download.destination_path,
                    mode,
                )
                .await?)
        }
        SourceType::Http | SourceType::Magnet => Ok(state
            .aria2
            .add_uri(
                &download.url,
                &download.finetune,
                &download.destination_path,
                mode,
            )
            .await?),
    }
}

async fn drop_old_aria2_gid(state: &AppState, gid: &str) {
    let _ = state.aria2.remove(gid).await;
    let _ = state.aria2.remove_download_result(gid).await;
}

async fn readd_in_aria2(
    state: &AppState,
    download: &Download,
    mode: Aria2AddMode,
) -> Result<(), AppError> {
    if let Some(gid) = &download.aria2_gid {
        drop_old_aria2_gid(state, gid).await;
    }
    let gid = start_in_aria2(state, download, mode).await?;
    state.db.update_download_gid(download.id, &gid)?;
    state
        .db
        .update_download_status(download.id, &DownloadStatus::Active)?;
    state.live_status.write().await.remove(&download.id);
    Ok(())
}

async fn resume_download(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<DownloadLiveStatus>, AppError> {
    let _activity = state.activity_guard().await?;
    let download = state
        .db
        .get_download(id)?
        .ok_or_else(|| AppError::NotFound(format!("download {id}")))?;

    if download.status == DownloadStatus::Active {
        return Err(AppError::Conflict(
            "download is active or still finishing a pause".into(),
        ));
    }

    // Mark this before talking to aria2 so a concurrent scheduler tick cannot
    // reclaim and pause the item after it becomes active.
    state.db.set_manually_started(id, true)?;
    state.db.set_paused_by_scheduler(id, false)?;

    let start_result: Result<(), AppError> = async {
        match &download.status {
            DownloadStatus::Error(_) | DownloadStatus::Removed => {
                readd_in_aria2(&state, &download, Aria2AddMode::Retry).await?;
            }
            DownloadStatus::Completed => {
                readd_in_aria2(&state, &download, Aria2AddMode::Restart).await?;
                state.db.clear_completed_at(id)?;
                state.db.update_download_completed_length(id, 0)?;
            }
            _ => match download.aria2_gid {
                Some(gid) => {
                    state.aria2.unpause(&gid).await?;
                    state
                        .db
                        .update_download_status(id, &DownloadStatus::Active)?;
                }
                None => {
                    let gid = start_in_aria2(&state, &download, Aria2AddMode::Fresh).await?;
                    state.db.update_download_gid(download.id, &gid)?;
                    state
                        .db
                        .update_download_status(download.id, &DownloadStatus::Active)?;
                }
            },
        }
        Ok(())
    }
    .await;

    if let Err(error) = start_result {
        state.db.set_manually_started(id, false)?;
        return Err(error);
    }

    let updated = state
        .db
        .get_download(id)?
        .ok_or_else(|| AppError::NotFound(format!("download {id}")))?;
    Ok(Json(merge_live(&state, updated).await))
}

#[derive(serde::Deserialize)]
pub struct ReorderRequest {
    pub ordered_ids: Vec<i64>,
}

async fn reorder_queue(
    State(state): State<AppState>,
    Path(queue_id): Path<i64>,
    Json(req): Json<ReorderRequest>,
) -> Result<axum::http::StatusCode, AppError> {
    let _activity = state.activity_guard().await?;
    state.db.reorder_queue(queue_id, &req.ordered_ids)?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod pause_tests {
    use super::*;
    use crate::{aria2::Aria2Client, config::ServerConfig, db::Database, live_status::LiveStats};
    use std::sync::Arc;
    use tokio::sync::Mutex;

    fn insert_active_download(state: &AppState) -> Download {
        let mut download = Download {
            id: 0,
            aria2_gid: Some("gid".into()),
            url: "https://example.test/file.bin".into(),
            filename: Some("file.bin".into()),
            destination_path: "/tmp".into(),
            source_type: SourceType::Http,
            category: FileCategory::Other,
            status: DownloadStatus::Active,
            paused_by_scheduler: true,
            manually_started: true,
            size: Some(100),
            completed_length: Some(20),
            queue_id: 1,
            position_in_queue: 0,
            finetune: FineTune::default(),
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
        };
        download.id = state.db.insert_download(&download).unwrap();
        download
    }

    async fn rpc_state(
        response: serde_json::Value,
    ) -> (
        AppState,
        Arc<Mutex<Vec<serde_json::Value>>>,
        tokio::task::JoinHandle<()>,
    ) {
        async fn rpc(
            State((requests, response)): State<(
                Arc<Mutex<Vec<serde_json::Value>>>,
                serde_json::Value,
            )>,
            Json(request): Json<serde_json::Value>,
        ) -> Json<serde_json::Value> {
            requests.lock().await.push(request);
            Json(response)
        }

        let requests = Arc::new(Mutex::new(Vec::new()));
        let rpc_app = Router::new()
            .route("/jsonrpc", post(rpc))
            .with_state((Arc::clone(&requests), response));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, rpc_app).await.unwrap();
        });
        let state = AppState::new(
            Database::open(":memory:").unwrap(),
            Aria2Client::new(format!("http://{address}/jsonrpc"), None),
            ServerConfig::default(),
            false,
        );
        (state, requests, server)
    }

    #[tokio::test]
    async fn successful_pause_stays_active_until_poller_confirms_it() {
        let (state, requests, server) = rpc_state(serde_json::json!({
            "jsonrpc": "2.0",
            "id": "ario",
            "result": "OK"
        }))
        .await;
        let download = insert_active_download(&state);
        state.live_status.write().await.insert(
            download.id,
            LiveStats {
                completed_length: 20,
                download_speed: 50,
            },
        );

        let Json(response) = pause_download(State(state.clone()), Path(download.id))
            .await
            .unwrap();

        assert_eq!(requests.lock().await[0]["method"], "aria2.pause");
        assert_eq!(response.download.status, DownloadStatus::Active);
        assert_eq!(response.download_speed, 50);
        assert!(!response.download.paused_by_scheduler);
        assert!(!response.download.manually_started);
        assert!(state.live_status.read().await.contains_key(&download.id));
        server.abort();
    }

    #[tokio::test]
    async fn failed_pause_leaves_database_and_live_state_unchanged() {
        let (state, _requests, server) = rpc_state(serde_json::json!({
            "jsonrpc": "2.0",
            "id": "ario",
            "error": {"code": 1, "message": "pause failed"}
        }))
        .await;
        let download = insert_active_download(&state);
        state.live_status.write().await.insert(
            download.id,
            LiveStats {
                completed_length: 20,
                download_speed: 50,
            },
        );

        assert!(
            pause_download(State(state.clone()), Path(download.id))
                .await
                .is_err()
        );

        let retained = state.db.get_download(download.id).unwrap().unwrap();
        assert_eq!(retained.status, DownloadStatus::Active);
        assert!(retained.paused_by_scheduler);
        assert!(retained.manually_started);
        assert_eq!(
            state.live_status.read().await[&download.id].download_speed,
            50
        );
        server.abort();
    }

    #[tokio::test]
    async fn resume_rejects_an_active_or_still_pausing_download_without_calling_aria2() {
        let (state, requests, server) = rpc_state(serde_json::json!({
            "jsonrpc": "2.0",
            "id": "ario",
            "result": "gid"
        }))
        .await;
        let download = insert_active_download(&state);

        let error = resume_download(State(state.clone()), Path(download.id))
            .await
            .unwrap_err();

        assert!(matches!(error, AppError::Conflict(_)));
        assert!(requests.lock().await.is_empty());
        assert!(
            state
                .db
                .get_download(download.id)
                .unwrap()
                .unwrap()
                .manually_started
        );
        server.abort();
    }
}

#[cfg(test)]
mod delete_files_tests {
    use super::*;
    use crate::{aria2::Aria2Client, config::ServerConfig, db::Database};
    use common::finetune::FineTune;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("ario-{name}-{}-{nonce}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn state() -> AppState {
        AppState::new(
            Database::open(":memory:").unwrap(),
            Aria2Client::new("http://127.0.0.1:1/jsonrpc", None),
            ServerConfig::default(),
            false,
        )
    }

    fn insert_download(
        state: &AppState,
        destination_path: &std::path::Path,
        filename: &str,
        status: DownloadStatus,
    ) -> Download {
        let mut download = Download {
            id: 0,
            aria2_gid: None,
            url: format!("https://example.test/{filename}"),
            filename: Some(filename.into()),
            destination_path: destination_path.to_string_lossy().into_owned(),
            source_type: SourceType::Http,
            category: FileCategory::Other,
            status,
            paused_by_scheduler: false,
            manually_started: false,
            size: None,
            completed_length: None,
            queue_id: 1,
            position_in_queue: 0,
            finetune: FineTune::default(),
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
        };
        download.id = state.db.insert_download(&download).unwrap();
        download
    }

    #[test]
    fn removes_multiple_payloads_and_control_files() {
        let dir = temp_dir("remove-artifacts");
        let first = dir.join("first.bin");
        let second = dir.join("second.bin");
        let control = dir.join("set.aria2");
        fs::write(&first, b"first").unwrap();
        fs::write(&second, b"second").unwrap();
        fs::write(&control, b"control").unwrap();
        let artifacts = vec![
            DownloadArtifact {
                path: first.to_string_lossy().into_owned(),
                kind: DownloadArtifactKind::Payload,
            },
            DownloadArtifact {
                path: second.to_string_lossy().into_owned(),
                kind: DownloadArtifactKind::Payload,
            },
            DownloadArtifact {
                path: control.to_string_lossy().into_owned(),
                kind: DownloadArtifactKind::Control,
            },
        ];

        let result = remove_download_artifacts(&artifacts, true).unwrap();
        assert_eq!(result.removed_payloads, 2);
        assert_eq!(result.missing_payloads, 0);
        assert!(result.metadata_complete);
        assert!(!first.exists());
        assert!(!second.exists());
        assert!(!control.exists());
        assert!(dir.is_dir());
        fs::remove_dir(&dir).unwrap();
    }

    #[test]
    fn missing_payloads_are_non_fatal_and_missing_controls_are_ignored() {
        let dir = temp_dir("missing-artifacts");
        let artifacts = vec![
            DownloadArtifact {
                path: dir.join("missing.bin").to_string_lossy().into_owned(),
                kind: DownloadArtifactKind::Payload,
            },
            DownloadArtifact {
                path: dir.join("missing.bin.aria2").to_string_lossy().into_owned(),
                kind: DownloadArtifactKind::Control,
            },
        ];

        let result = remove_download_artifacts(&artifacts, false).unwrap();
        assert_eq!(result.removed_payloads, 0);
        assert_eq!(result.missing_payloads, 1);
        assert!(!result.metadata_complete);
        fs::remove_dir(&dir).unwrap();
    }

    #[test]
    fn non_missing_filesystem_errors_abort_deletion() {
        let dir = temp_dir("artifact-error");
        let artifacts = vec![DownloadArtifact {
            path: dir.to_string_lossy().into_owned(),
            kind: DownloadArtifactKind::Payload,
        }];

        assert!(remove_download_artifacts(&artifacts, true).is_err());
        assert!(dir.is_dir());
        fs::remove_dir(&dir).unwrap();
    }

    #[tokio::test]
    async fn missing_legacy_file_removes_database_record_with_warning_result() {
        let dir = temp_dir("missing-legacy");
        let state = state();
        let mut download = insert_download(&state, &dir, "missing.bin", DownloadStatus::Completed);
        download.source_type = SourceType::Magnet;

        let result = delete_download_with_files(&state, download.clone())
            .await
            .unwrap();
        assert_eq!(result.missing_payloads, 1);
        assert!(!result.metadata_complete);
        assert!(state.db.get_download(download.id).unwrap().is_none());
        fs::remove_dir(&dir).unwrap();
    }

    #[tokio::test]
    async fn filesystem_error_retains_database_record_as_removed() {
        let dir = temp_dir("retained-record");
        let state = state();
        fs::create_dir(dir.join("not-a-file")).unwrap();
        let download = insert_download(&state, &dir, "not-a-file", DownloadStatus::Completed);

        assert!(
            delete_download_with_files(&state, download.clone())
                .await
                .is_err()
        );
        assert_eq!(
            state.db.get_download(download.id).unwrap().unwrap().status,
            DownloadStatus::Removed
        );
        fs::remove_dir(dir.join("not-a-file")).unwrap();
        fs::remove_dir(&dir).unwrap();
    }

    #[tokio::test]
    async fn never_started_download_does_not_delete_an_unallocated_name_collision() {
        let dir = temp_dir("unallocated");
        let coincidental_file = dir.join("same-name.bin");
        fs::write(&coincidental_file, b"unrelated").unwrap();
        let state = state();
        let download = insert_download(&state, &dir, "same-name.bin", DownloadStatus::Pending);

        let result = delete_download_with_files(&state, download.clone())
            .await
            .unwrap();
        assert_eq!(result.missing_payloads, 1);
        assert!(coincidental_file.exists());
        assert!(state.db.get_download(download.id).unwrap().is_none());
        fs::remove_file(&coincidental_file).unwrap();
        fs::remove_dir(&dir).unwrap();
    }
}

#[cfg(test)]
mod merge_live_tests {
    use super::*;
    use crate::{aria2::Aria2Client, config::ServerConfig, db::Database, live_status::LiveStats};
    use common::finetune::FineTune;

    fn state() -> AppState {
        AppState::new(
            Database::open(":memory:").unwrap(),
            Aria2Client::new("http://127.0.0.1:1/jsonrpc", None),
            ServerConfig::default(),
            false,
        )
    }

    fn insert_download(state: &AppState, completed_length: Option<u64>) -> Download {
        let mut download = Download {
            id: 0,
            aria2_gid: None,
            url: "https://example.test/file.bin".into(),
            filename: Some("file.bin".into()),
            destination_path: "/tmp".into(),
            source_type: SourceType::Http,
            category: FileCategory::Other,
            status: DownloadStatus::Paused,
            paused_by_scheduler: false,
            manually_started: false,
            size: Some(100),
            completed_length,
            queue_id: 1,
            position_in_queue: 0,
            finetune: FineTune::default(),
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
        };
        download.id = state.db.insert_download(&download).unwrap();
        download
    }

    #[tokio::test]
    async fn empty_live_map_uses_persisted_completed_length() {
        let state = state();
        let download = insert_download(&state, Some(42));
        let download = state.db.get_download(download.id).unwrap().unwrap();

        let live = merge_live(&state, download).await;
        assert_eq!(live.completed_length, 42);
        assert_eq!(live.download_speed, 0);
        assert_eq!(live.eta_seconds, None);
    }

    #[tokio::test]
    async fn live_map_overrides_persisted_completed_length() {
        let state = state();
        let mut download = insert_download(&state, Some(42));
        download.status = DownloadStatus::Active;
        state.live_status.write().await.insert(
            download.id,
            LiveStats {
                completed_length: 80,
                download_speed: 10,
            },
        );

        let live = merge_live(&state, download).await;
        assert_eq!(live.completed_length, 80);
        assert_eq!(live.download_speed, 10);
        assert_eq!(live.eta_seconds, Some(2));
    }

    #[tokio::test]
    async fn paused_download_suppresses_stale_transfer_metrics() {
        let state = state();
        let download = insert_download(&state, Some(42));
        state.live_status.write().await.insert(
            download.id,
            LiveStats {
                completed_length: 80,
                download_speed: 10,
            },
        );

        let live = merge_live(&state, download).await;
        assert_eq!(live.completed_length, 80);
        assert_eq!(live.download_speed, 0);
        assert_eq!(live.eta_seconds, None);
    }

    #[tokio::test]
    async fn missing_live_and_persisted_progress_defaults_to_zero() {
        let state = state();
        let download = insert_download(&state, None);

        let live = merge_live(&state, download).await;
        assert_eq!(live.completed_length, 0);
        assert_eq!(live.download_speed, 0);
    }

    #[tokio::test]
    async fn live_zero_does_not_clobber_persisted_completed_length() {
        let state = state();
        let download = insert_download(&state, Some(42));
        state.live_status.write().await.insert(
            download.id,
            LiveStats {
                completed_length: 0,
                download_speed: 0,
            },
        );

        let live = merge_live(&state, download).await;
        assert_eq!(live.completed_length, 42);
        assert_eq!(live.download_speed, 0);
    }
}

#[cfg(test)]
mod finetune_resolution_tests {
    use super::resolve_finetune;
    use common::{finetune::FineTune, queue::QueueSettings};

    #[test]
    fn unset_download_fields_inherit_individual_queue_defaults() {
        let settings = QueueSettings {
            max_concurrent_downloads: 2,
            max_retries: 3,
            retry_wait_seconds: 5,
            default_finetune: FineTune {
                connections_per_download: Some(4),
                max_connections_per_server: Some(2),
                ..FineTune::default()
            },
        };

        let resolved = resolve_finetune(
            &settings,
            Some(FineTune {
                max_retries: Some(0),
                ..FineTune::default()
            }),
        );

        assert_eq!(resolved.connections_per_download, Some(4));
        assert_eq!(resolved.max_connections_per_server, Some(2));
        assert_eq!(resolved.max_retries, Some(0));
        assert_eq!(resolved.retry_wait_seconds, Some(5));
    }
}

#[cfg(test)]
mod torrent_creation_tests {
    use super::*;
    use crate::{aria2::Aria2Client, config::ServerConfig, db::Database};
    use axum::{
        body::Body,
        http::{Request, StatusCode, header::CONTENT_TYPE},
    };
    use http_body_util::BodyExt;
    use std::sync::Arc;
    use tokio::sync::Mutex;
    use tower::ServiceExt;

    fn state() -> AppState {
        state_with_rpc("http://127.0.0.1:1/jsonrpc")
    }

    fn state_with_rpc(rpc_url: &str) -> AppState {
        AppState::new(
            Database::open(":memory:").unwrap(),
            Aria2Client::new(rpc_url, None),
            ServerConfig::default(),
            false,
        )
    }

    #[tokio::test]
    async fn saved_torrent_persists_metainfo_without_contacting_aria2() {
        let state = state();
        let created = create_download(
            &state,
            AddDownloadInput::TorrentFile {
                filename: "../sample.torrent".into(),
                data: b"metainfo".to_vec(),
            },
            1,
            0,
            FineTune::default(),
            false,
        )
        .await
        .unwrap();

        assert_eq!(created.download.filename.as_deref(), Some("sample.torrent"));
        assert_eq!(created.download.status, DownloadStatus::Pending);
        assert_eq!(
            state
                .db
                .get_download_torrent_data(created.download.id)
                .unwrap(),
            Some(b"metainfo".to_vec())
        );
    }

    #[tokio::test]
    async fn legacy_torrent_without_metainfo_has_an_actionable_error() {
        let state = state();
        let mut download = Download {
            id: 0,
            aria2_gid: None,
            url: String::new(),
            filename: Some("legacy.torrent".into()),
            destination_path: "/tmp".into(),
            source_type: SourceType::Torrent,
            category: FileCategory::Other,
            status: DownloadStatus::Pending,
            paused_by_scheduler: false,
            manually_started: false,
            size: None,
            completed_length: None,
            queue_id: 1,
            position_in_queue: 0,
            finetune: FineTune::default(),
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
        };
        download.id = state.db.insert_download(&download).unwrap();

        let error = start_in_aria2(&state, &download, Aria2AddMode::Fresh)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("add the .torrent file again"));
    }

    #[tokio::test]
    async fn persisted_torrent_is_readded_with_the_requested_mode() {
        async fn rpc(
            State(requests): State<Arc<Mutex<Vec<serde_json::Value>>>>,
            Json(request): Json<serde_json::Value>,
        ) -> Json<serde_json::Value> {
            requests.lock().await.push(request);
            Json(serde_json::json!({"jsonrpc": "2.0", "id": "ario", "result": "gid"}))
        }

        let requests = Arc::new(Mutex::new(Vec::new()));
        let rpc_app = Router::new()
            .route("/jsonrpc", post(rpc))
            .with_state(Arc::clone(&requests));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, rpc_app).await.unwrap();
        });
        let state = state_with_rpc(&format!("http://{address}/jsonrpc"));
        let created = create_download(
            &state,
            AddDownloadInput::TorrentFile {
                filename: "sample.torrent".into(),
                data: b"metainfo".to_vec(),
            },
            1,
            0,
            FineTune::default(),
            false,
        )
        .await
        .unwrap();

        let gid = start_in_aria2(&state, &created.download, Aria2AddMode::Retry)
            .await
            .unwrap();
        assert_eq!(gid, "gid");
        let requests = requests.lock().await;
        assert_eq!(requests[0]["method"], "aria2.addTorrent");
        assert_eq!(requests[0]["params"][0], "bWV0YWluZm8=");
        assert_eq!(requests[0]["params"][2]["continue"], "true");
        server.abort();
    }

    #[test]
    fn multipart_filenames_are_reduced_to_a_basename() {
        assert_eq!(
            safe_filename("../../sample.torrent").as_deref(),
            Some("sample.torrent")
        );
        assert_eq!(
            safe_filename(r"C:\Downloads\sample.torrent").as_deref(),
            Some("sample.torrent")
        );
        assert_eq!(safe_filename(""), None);
    }

    #[tokio::test]
    async fn multipart_endpoint_creates_a_saved_torrent() {
        let state = state();
        let app = router().with_state(state.clone());
        let metadata = serde_json::to_string(&TorrentUploadMetadata {
            queue_id: 1,
            finetune_override: None,
            start_immediately: false,
        })
        .unwrap();
        let boundary = "ario-test-boundary";
        let body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"metadata\"\r\nContent-Type: application/json\r\n\r\n{metadata}\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"sample.torrent\"\r\nContent-Type: application/x-bittorrent\r\n\r\nmetainfo\r\n--{boundary}--\r\n"
        );
        let response = app
            .oneshot(
                Request::post("/downloads/torrent")
                    .header(
                        CONTENT_TYPE,
                        format!("multipart/form-data; boundary={boundary}"),
                    )
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let created: DownloadLiveStatus = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(created.download.status, DownloadStatus::Pending);
        assert_eq!(
            state
                .db
                .get_download_torrent_data(created.download.id)
                .unwrap(),
            Some(b"metainfo".to_vec())
        );
    }

    #[tokio::test]
    async fn multipart_endpoint_rejects_a_missing_file() {
        let state = state();
        let app = router().with_state(state);
        let metadata = serde_json::to_string(&TorrentUploadMetadata {
            queue_id: 1,
            finetune_override: None,
            start_immediately: false,
        })
        .unwrap();
        let boundary = "ario-test-boundary";
        let body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"metadata\"\r\n\r\n{metadata}\r\n--{boundary}--\r\n"
        );
        let response = app
            .oneshot(
                Request::post("/downloads/torrent")
                    .header(
                        CONTENT_TYPE,
                        format!("multipart/form-data; boundary={boundary}"),
                    )
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
