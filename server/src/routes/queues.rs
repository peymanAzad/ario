use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use chrono::Utc;
use common::{
    queue::{CreateQueueRequest, Queue, QueueSettings, UpdateQueueRequest},
    scheduler::Scheduler,
};

use crate::state::AppState;
use crate::{
    error::AppError,
    scheduler::{current_schedule_occurrence, pause_downloads, start_eligible_downloads},
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/queues", get(list_queues).post(create_queue))
        .route(
            "/queues/{id}",
            get(get_queue).put(update_queue).delete(delete_queue),
        )
        .route("/queues/{id}/resume", post(resume_queue))
        .route("/queues/{id}/pause", post(pause_queue))
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
        status: common::enums::QueueStatus::Paused,
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
        status: existing.status,
        created_at: existing.created_at,
    };

    let scheduler_changed = queue.scheduler != existing.scheduler;
    state.db.update_queue(&queue)?;
    if scheduler_changed {
        // Occurrence keys belong to the old schedule definition and must not
        // suppress or claim ownership of a newly edited schedule.
        state.db.set_queue_scheduler_suppression(id, None)?;
    }
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

async fn resume_queue(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<axum::http::StatusCode, AppError> {
    let queue = state
        .db
        .get_queue(id)?
        .ok_or_else(|| AppError::NotFound(format!("queue {id}")))?;
    state.db.set_queue_scheduler_suppression(id, None)?;
    state
        .db
        .update_queue_status(id, common::enums::QueueStatus::Active)?;
    start_eligible_downloads(&state, &queue).await?;
    Ok(axum::http::StatusCode::OK)
}

async fn pause_queue(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<axum::http::StatusCode, AppError> {
    let queue = state
        .db
        .get_queue(id)?
        .ok_or_else(|| AppError::NotFound(format!("queue {id}")))?;
    let suppressed_occurrence = queue
        .scheduler
        .enabled
        .then(|| current_schedule_occurrence(&queue.scheduler.recurrence))
        .flatten();
    state
        .db
        .set_queue_scheduler_suppression(id, suppressed_occurrence.as_deref())?;
    state
        .db
        .update_queue_status(id, common::enums::QueueStatus::Paused)?;
    pause_downloads(&state, &queue, false).await?;
    Ok(axum::http::StatusCode::OK)
}
