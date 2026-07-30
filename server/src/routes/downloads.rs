use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, post, put},
};
use chrono::Utc;
use common::{
    download::{AddDownloadInput, AddDownloadsRequest, Download, DownloadFilter},
    enums::{DownloadStatus, FileCategory, SourceType},
    finetune::FineTune,
};

use crate::{error::AppError, state::AppState};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/downloads", get(get_downloads).post(add_download))
        .route("/downloads/{id}", get(get_download).delete(delete_download))
        .route("/downloads/{id}/finetune", put(update_finetune))
        .route("/downloads/{id}/pause", post(pause_download))
        .route("/downloads/{id}/resume", post(resume_download))
        .route("/queues/{queue_id}/reorder", put(reorder_queue))
}

async fn get_downloads(
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
        .ok_or_else(|| AppError::NotFound(format!("downalod {id}")))?;
    Ok(Json(download))
}

async fn add_download(
    State(state): State<AppState>,
    Json(payload): Json<AddDownloadsRequest>,
) -> Result<Json<Vec<Download>>, AppError> {
    if payload.inputs.is_empty() {
        return Err(AppError::BadRequest("inputs must not be empty".into()));
    }

    let queue = state.db.get_queue(payload.queue_id)?.ok_or_else(|| {
        AppError::BadRequest(format!("queue {} does not exist", payload.queue_id))
    })?;

    let finetune = payload
        .finetune_override
        .unwrap_or(queue.settings.default_finetune);
    let mut next_position = state.db.next_position_in_queue(queue.id)?;
    let mut rows = Vec::<Download>::with_capacity(payload.inputs.len());
    for input in payload.inputs {
        let (url, filename, source_type) = match input {
            AddDownloadInput::Url(url) => {
                let filename = url.rsplit('/').next().map(str::to_string);
                let source_type = if (url.starts_with("magnet:")) {
                    SourceType::Magnet
                } else {
                    SourceType::Http
                };
                (url, filename, source_type)
            }
            AddDownloadInput::TorrentFile { filename, data } => {
                (String::new(), Some(filename), SourceType::Torrent)
            }
        };

        let download_path = "~/Downloads".to_string();
        let file_category = FileCategory::Other;

        let download = Download {
            id: 0,
            filename,
            url,
            source_type,
            status: DownloadStatus::Pending,
            category: file_category,
            aria2_gid: None,
            size: None,
            queue_id: queue.id,
            position_in_queue: next_position,
            finetune: finetune.clone(),
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
            destination_path: download_path,
        };
        next_position += 1;
        let id = state.db.insert_download(&download)?;
        rows.push(
            state
                .db
                .get_download(id)?
                .ok_or_else(|| AppError::NotFound(format!("download {id}")))?,
        );
    }
    Ok(Json(rows))
}
async fn delete_download(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<axum::http::StatusCode, AppError> {
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
