use crate::aria2::Aria2AddMode;
use crate::db::{DownloadArtifact, DownloadArtifactKind};
use crate::error::AppError;
use crate::state::AppState;
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    response::{IntoResponse, Response},
    routing::{delete, get},
};
use chrono::Utc;
use common::{
    download::{
        AddDownloadInput, AddDownloadsRequest, DeleteDownloadFilesResult, Download, DownloadFilter,
        DownloadLiveStatus,
    },
    enums::{DownloadStatus, FileCategory, SourceType},
    finetune::FineTune,
    queue::QueueSettings,
};
use std::{collections::HashSet, fs, io::ErrorKind, path::PathBuf};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/downloads", get(list_downloads).post(add_downloads))
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
    let mut next_position = state.db.next_position_in_queue(queue.id)?;

    let mut created = Vec::with_capacity(req.inputs.len());

    for input in req.inputs {
        let (url, filename, source_type, torrent_b64) = match input {
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
                use base64::Engine;
                let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
                (
                    String::new(),
                    Some(filename),
                    SourceType::Torrent,
                    Some(b64),
                )
            }
        };

        let category = filename
            .as_deref()
            .map(|f| {
                FileCategory::infer_from_filename(f, &state.config.settings.category_extensions)
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
            id: 0, // placeholder — overwritten by the id sqlite assigns
            aria2_gid: None,
            url,
            filename,
            destination_path: destination_path.clone(),
            source_type,
            category,
            status: DownloadStatus::Pending,
            paused_by_scheduler: false,
            manually_started: false,
            size: None,
            completed_length: None,
            queue_id: queue.id,
            position_in_queue: next_position,
            finetune: finetune.clone(),
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
        };
        next_position += 1;

        let id = state.db.insert_download(&download)?;
        download.id = id;

        // Match /resume: start even if the queue is Paused, and mark as
        // manually started so a concurrent scheduler tick cannot reclaim it.
        if req.start_immediately {
            state.db.set_manually_started(id, true)?;
            state.db.set_paused_by_scheduler(id, false)?;

            let aria2_result = match (&torrent_b64, download.source_type) {
                (Some(b64), _) => {
                    state
                        .aria2
                        .add_torrent(b64, &download.finetune, &destination_path)
                        .await
                }
                (None, _) => {
                    state
                        .aria2
                        .add_uri(
                            &download.url,
                            &download.finetune,
                            &destination_path,
                            Aria2AddMode::Fresh,
                        )
                        .await
                }
            };

            match aria2_result {
                Ok(gid) => {
                    state.db.update_download_gid(id, &gid)?;
                    state
                        .db
                        .update_download_status(id, &DownloadStatus::Active)?;
                }
                Err(e) => {
                    state.db.set_manually_started(id, false)?;
                    state
                        .db
                        .update_download_status(id, &DownloadStatus::Error(e.to_string()))?;
                }
            }
        }

        let row = state
            .db
            .get_download(id)?
            .ok_or_else(|| AppError::NotFound(format!("download {id}")))?;
        created.push(merge_live(&state, row).await);
    }

    Ok(Json(created))
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
    state
        .db
        .update_download_status(id, &DownloadStatus::Paused)?;
    state.db.set_paused_by_scheduler(id, false)?;
    state.db.set_manually_started(id, false)?;

    let updated = state
        .db
        .get_download(id)?
        .ok_or_else(|| AppError::NotFound(format!("download {id}")))?;
    Ok(Json(merge_live(&state, updated).await))
}

async fn start_in_aria2(
    state: &AppState,
    download: &Download,
    mode: Aria2AddMode,
) -> Result<String, AppError> {
    match download.source_type {
        SourceType::Torrent => Err(AppError::BadRequest(
            "torrent files cannot be resumed, retried, or restarted because the .torrent \
             data is not stored; they can only be started immediately (\"Start Now\")."
                .into(),
        )),
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
