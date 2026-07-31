use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::get,
};
use chrono::Utc;
use common::{
    download::{AddDownloadInput, AddDownloadsRequest, Download, DownloadFilter},
    enums::{DownloadStatus, FileCategory, SourceType},
    finetune::FineTune,
};

use crate::error::AppError;
use crate::state::AppState;

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

/// `GET /downloads?queue_id=&status=&category=&sort_by=&sort_desc=`
async fn list_downloads(
    State(state): State<AppState>,
    Query(filter): Query<DownloadFilter>,
) -> Result<Json<Vec<Download>>, AppError> {
    let downloads = state.db.list_downloads(&filter)?;
    Ok(Json(downloads))
}

async fn get_download(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Download>, AppError> {
    let download = state
        .db
        .get_download(id)?
        .ok_or_else(|| AppError::NotFound(format!("download {id}")))?;
    Ok(Json(download))
}

async fn add_downloads(
    State(state): State<AppState>,
    Json(req): Json<AddDownloadsRequest>,
) -> Result<Json<Vec<Download>>, AppError> {
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

        let row = state
            .db
            .get_download(id)?
            .ok_or_else(|| AppError::NotFound(format!("download {id}")))?;
        created.push(row);
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
) -> Result<Json<Download>, AppError> {
    state.db.update_download_finetune(id, &finetune)?;
    let updated = state
        .db
        .get_download(id)?
        .ok_or_else(|| AppError::NotFound(format!("download {id}")))?;
    Ok(Json(updated))
}

async fn pause_download(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Download>, AppError> {
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

    let updated = state
        .db
        .get_download(id)?
        .ok_or_else(|| AppError::NotFound(format!("download {id}")))?;
    Ok(Json(updated))
}

async fn resume_download(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Download>, AppError> {
    let download = state
        .db
        .get_download(id)?
        .ok_or_else(|| AppError::NotFound(format!("download {id}")))?;
    let gid = download
        .aria2_gid
        .ok_or_else(|| AppError::BadRequest("download has not been started in aria2 yet".into()))?;

    state.aria2.unpause(&gid).await?;
    state
        .db
        .update_download_status(id, &DownloadStatus::Active)?;

    let updated = state
        .db
        .get_download(id)?
        .ok_or_else(|| AppError::NotFound(format!("download {id}")))?;
    Ok(Json(updated))
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
