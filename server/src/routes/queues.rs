use axum::{
    Json, Router,
    extract::{Path, State},
    routing::get,
};
use chrono::Utc;
use common::{
    queue::{CreateQueueRequest, Queue, QueueSettings, UpdateQueueRequest},
    scheduler::Scheduler,
};

use crate::error::AppError;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/queues", get(list_queues).post(create_queue))
        .route(
            "/queues/{id}",
            get(get_queue).put(update_queue).delete(delete_queue),
        )
}

async fn list_queues(State(state): State<AppState>) -> Result<Json<Vec<Queue>>, AppError> {
    let queues = state.db.list_queues()?;
    Ok(Json(queues))
}

async fn get_queue(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Queue>, AppError> {
    let queue = state
        .db
        .get_queue(id)?
        .ok_or_else(|| AppError::NotFound(format!("queue {id}")))?;
    Ok(Json(queue))
}

async fn create_queue(
    State(state): State<AppState>,
    Json(req): Json<CreateQueueRequest>,
) -> Result<Json<Queue>, AppError> {
    let queue = Queue {
        id: 0, // placeholder — overwritten by the id sqlite assigns on insert
        name: req.name,
        position: req.position,
        settings: QueueSettings {
            max_concurrent_downloads: req.max_concurrent_downloads,
            max_retries: req.max_retries,
            default_finetune: req.default_finetune,
        },
        scheduler: Scheduler {
            enabled: req.scheduler_enabled,
            recurrence: req.recurrence,
            run_missed_on_startup: req.run_missed_on_startup,
        },
        created_at: Utc::now(),
    };

    let id = state.db.insert_queue(&queue)?;
    let created = state
        .db
        .get_queue(id)?
        .ok_or_else(|| AppError::NotFound(format!("queue {id}")))?;
    Ok(Json(created))
}

async fn update_queue(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<UpdateQueueRequest>,
) -> Result<Json<Queue>, AppError> {
    let existing = state
        .db
        .get_queue(id)?
        .ok_or_else(|| AppError::NotFound(format!("queue {id}")))?;

    let queue = Queue {
        id,
        name: req.name,
        position: req.position,
        settings: QueueSettings {
            max_concurrent_downloads: req.max_concurrent_downloads,
            max_retries: req.max_retries,
            default_finetune: req.default_finetune,
        },
        scheduler: Scheduler {
            enabled: req.scheduler_enabled,
            recurrence: req.recurrence,
            run_missed_on_startup: req.run_missed_on_startup,
        },
        created_at: existing.created_at,
    };

    state.db.update_queue(&queue)?;
    let updated = state
        .db
        .get_queue(id)?
        .ok_or_else(|| AppError::NotFound(format!("queue {id}")))?;
    Ok(Json(updated))
}

async fn delete_queue(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<axum::http::StatusCode, AppError> {
    state.db.delete_queue(id)?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}
