use crate::state::AppState;
use crate::{error::AppError, live_status::LiveStats};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::get,
};
use chrono::Utc;
use common::{
    download::{
        AddDownloadInput, AddDownloadsRequest, Download, DownloadFilter, DownloadLiveStatus,
    },
    enums::{DownloadStatus, FileCategory, SourceType},
    finetune::FineTune,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/downloads", get(list_downloads).post(add_downloads))
        .route("/downloads/{id}", get(get_download).delete(delete_download))
        .route(
            "/downloads/{id}/finetune",
            axum::routing::put(update_finetune),
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
    let live = state
        .live_status
        .read()
        .await
        .get(&download.id)
        .copied()
        .unwrap_or(LiveStats {
            completed_length: 0,
            download_speed: 0,
        });

    let eta_seconds = match (download.size, live.download_speed) {
        (Some(total), speed) if speed > 0 && total > live.completed_length => {
            Some((total - live.completed_length) / speed)
        }
        _ => None,
    };

    DownloadLiveStatus {
        completed_length: live.completed_length,
        download_speed: live.download_speed,
        eta_seconds,
        download,
    }
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
    if req.inputs.is_empty() {
        return Err(AppError::BadRequest("inputs must not be empty".into()));
    }

    let queue = state
        .db
        .get_queue(req.queue_id)?
        .ok_or_else(|| AppError::BadRequest(format!("queue {} does not exist", req.queue_id)))?;

    let finetune: FineTune = req
        .finetune_override
        .unwrap_or(queue.settings.default_finetune);
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
            size: None,
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

        if req.start_immediately {
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
                        .add_uri(&download.url, &download.finetune, &destination_path)
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

async fn delete_download(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<axum::http::StatusCode, AppError> {
    let download = state
        .db
        .get_download(id)?
        .ok_or_else(|| AppError::NotFound(format!("download {id}")))?;

    if let Some(gid) = &download.aria2_gid {
        let _ = state.aria2.remove(gid).await;
    }

    state.db.delete_download(id)?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

async fn update_finetune(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(finetune): Json<FineTune>,
) -> Result<Json<DownloadLiveStatus>, AppError> {
    state.db.update_download_finetune(id, &finetune)?;
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

    let updated = state
        .db
        .get_download(id)?
        .ok_or_else(|| AppError::NotFound(format!("download {id}")))?;
    Ok(Json(merge_live(&state, updated).await))
}

async fn start_in_aria2(state: &AppState, download: &Download) -> Result<String, AppError> {
    match download.source_type {
        // NOTE: a "Save For Later" torrent can't be started this way for now
        SourceType::Torrent => Err(AppError::BadRequest(
            "torrents currently can only be started immediately (\"Start Now\"), not \
             resumed after being saved."
                .into(),
        )),
        SourceType::Http | SourceType::Magnet => Ok(state
            .aria2
            .add_uri(
                &download.url,
                &download.finetune,
                &download.destination_path,
            )
            .await?),
    }
}

async fn resume_download(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<DownloadLiveStatus>, AppError> {
    let download = state
        .db
        .get_download(id)?
        .ok_or_else(|| AppError::NotFound(format!("download {id}")))?;

    match download.aria2_gid {
        Some(gid) => {
            state.aria2.unpause(&gid).await?;
            state
                .db
                .update_download_status(id, &DownloadStatus::Active)?;
        }
        None => {
            let gid = start_in_aria2(&state, &download).await?;
            state.db.update_download_gid(download.id, &gid)?;
            state
                .db
                .update_download_status(download.id, &DownloadStatus::Active)?;
        }
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
    state.db.reorder_queue(queue_id, &req.ordered_ids)?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}
