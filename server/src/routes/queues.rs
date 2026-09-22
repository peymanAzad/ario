use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};
use chrono::Utc;
use common::{
    download::DownloadFilter,
    queue::{CreateQueueRequest, Queue, QueueSettings, UpdateQueueRequest},
    scheduler::Scheduler,
};

use crate::state::AppState;
use crate::{
    error::AppError,
    routes::downloads::delete_download_record,
    scheduler::{
        current_schedule_occurrence, pause_downloads, queue_is_running, start_eligible_downloads,
    },
};

const MAIN_QUEUE_ID: i64 = 1;

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
    let queues = state
        .db
        .list_queues()?
        .into_iter()
        .map(|queue| with_running_state(&state, queue))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(queues))
}

fn with_running_state(state: &AppState, mut queue: Queue) -> Result<Queue, AppError> {
    let occurrence = queue
        .scheduler
        .enabled
        .then(|| current_schedule_occurrence(&queue.scheduler.recurrence))
        .flatten();
    let suppressed = state.db.get_queue_scheduler_suppression(queue.id)?;
    queue.running = queue_is_running(&queue.status, suppressed.as_deref(), occurrence.as_deref())
        && state.db.queue_has_work(queue.id)?;
    Ok(queue)
}

async fn get_queue(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Queue>, AppError> {
    let queue = state
        .db
        .get_queue(id)?
        .ok_or_else(|| AppError::NotFound(format!("queue {id}")))?;
    Ok(Json(with_running_state(&state, queue)?))
}

async fn create_queue(
    State(state): State<AppState>,
    Json(req): Json<CreateQueueRequest>,
) -> Result<Json<Queue>, AppError> {
    let _activity = state.activity_guard().await?;
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
        running: false,
    };

    let id = state.db.insert_queue(&queue)?;
    let created = state
        .db
        .get_queue(id)?
        .ok_or_else(|| AppError::NotFound(format!("queue {id}")))?;
    Ok(Json(with_running_state(&state, created)?))
}

async fn update_queue(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<UpdateQueueRequest>,
) -> Result<Json<Queue>, AppError> {
    let _activity = state.activity_guard().await?;
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
        running: false,
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
    Ok(Json(with_running_state(&state, updated)?))
}

#[derive(Default, serde::Deserialize)]
struct DeleteQueueQuery {
    #[serde(default)]
    delete_downloads: bool,
}

async fn delete_queue(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(query): Query<DeleteQueueQuery>,
) -> Result<axum::http::StatusCode, AppError> {
    let _activity = state.activity_guard().await?;

    state
        .db
        .get_queue(id)?
        .ok_or_else(|| AppError::NotFound(format!("queue {id}")))?;
    if id == MAIN_QUEUE_ID {
        return Err(AppError::BadRequest(
            "the Main Queue cannot be removed".into(),
        ));
    }

    if !query.delete_downloads {
        if state.db.delete_queue_if_empty(id)? {
            return Ok(axum::http::StatusCode::NO_CONTENT);
        }
        return Err(AppError::Conflict(
            "queue contains download items; confirmation is required".into(),
        ));
    }

    let downloads = state.db.list_downloads(&DownloadFilter {
        queue_id: Some(id),
        ..DownloadFilter::default()
    })?;
    for download in downloads {
        delete_download_record(&state, download).await?;
    }
    state.db.delete_queue(id)?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

async fn resume_queue(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<axum::http::StatusCode, AppError> {
    let _activity = state.activity_guard().await?;
    let queue = state
        .db
        .get_queue(id)?
        .ok_or_else(|| AppError::NotFound(format!("queue {id}")))?;
    state.db.reset_error_retry_counts(id)?;
    if !state.db.queue_has_work(id)? {
        return Ok(axum::http::StatusCode::NO_CONTENT);
    }
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
    let _activity = state.activity_guard().await?;
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

#[cfg(test)]
mod delete_tests {
    use super::*;
    use crate::{
        aria2::Aria2Client,
        config::ServerConfig,
        db::{Database, DownloadArtifact, DownloadArtifactKind},
        live_status::LiveStats,
    };
    use chrono::Utc;
    use common::{
        download::Download,
        enums::{DownloadStatus, FileCategory, QueueStatus, Recurrence, SourceType},
        finetune::FineTune,
        scheduler::Scheduler,
    };
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn state() -> AppState {
        AppState::new(
            Database::open(":memory:").unwrap(),
            Aria2Client::new("http://127.0.0.1:1/jsonrpc", None),
            ServerConfig::default(),
            false,
        )
    }

    fn insert_queue(state: &AppState, name: &str) -> i64 {
        state
            .db
            .insert_queue(&Queue {
                id: 0,
                name: name.into(),
                position: 1,
                settings: QueueSettings {
                    max_concurrent_downloads: 1,
                    max_retries: 3,
                    default_finetune: FineTune::default(),
                },
                scheduler: Scheduler {
                    enabled: false,
                    recurrence: Recurrence::Once {
                        start: Utc::now(),
                        end: Utc::now(),
                    },
                    run_missed_on_startup: false,
                },
                status: QueueStatus::Paused,
                created_at: Utc::now(),
                running: false,
            })
            .unwrap()
    }

    fn insert_download(state: &AppState, queue_id: i64, path: &str) -> i64 {
        state
            .db
            .insert_download(&Download {
                id: 0,
                aria2_gid: None,
                url: "https://example.test/file.bin".into(),
                filename: Some("file.bin".into()),
                destination_path: path.into(),
                source_type: SourceType::Http,
                category: FileCategory::Other,
                status: DownloadStatus::Paused,
                paused_by_scheduler: false,
                manually_started: false,
                retry_count: 0,
                size: None,
                completed_length: None,
                queue_id,
                position_in_queue: 0,
                finetune: FineTune::default(),
                created_at: Utc::now(),
                started_at: None,
                completed_at: None,
            })
            .unwrap()
    }

    #[tokio::test]
    async fn empty_queue_deletes_without_confirmation() {
        let state = state();
        let queue_id = insert_queue(&state, "Empty");

        let status = delete_queue(
            State(state.clone()),
            Path(queue_id),
            Query(DeleteQueueQuery::default()),
        )
        .await
        .unwrap();

        assert_eq!(status, axum::http::StatusCode::NO_CONTENT);
        assert!(state.db.get_queue(queue_id).unwrap().is_none());
    }

    #[tokio::test]
    async fn populated_queue_requires_confirmation() {
        let state = state();
        let queue_id = insert_queue(&state, "Populated");
        let download_id = insert_download(&state, queue_id, "/tmp");

        let result = delete_queue(
            State(state.clone()),
            Path(queue_id),
            Query(DeleteQueueQuery::default()),
        )
        .await;

        assert!(matches!(result, Err(AppError::Conflict(_))));
        assert!(state.db.get_queue(queue_id).unwrap().is_some());
        assert!(state.db.get_download(download_id).unwrap().is_some());
    }

    #[tokio::test]
    async fn confirmed_delete_cleans_metadata_and_live_status_but_keeps_files() {
        let state = state();
        let queue_id = insert_queue(&state, "Populated");
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let file = std::env::temp_dir().join(format!("ario-queue-delete-{unique}.bin"));
        fs::write(&file, b"keep me").unwrap();
        let download_id =
            insert_download(&state, queue_id, file.parent().unwrap().to_str().unwrap());
        state
            .db
            .replace_download_artifacts(
                download_id,
                &[DownloadArtifact {
                    path: file.to_string_lossy().into_owned(),
                    kind: DownloadArtifactKind::Payload,
                }],
            )
            .unwrap();
        state.live_status.write().await.insert(
            download_id,
            LiveStats {
                completed_length: 1,
                download_speed: 2,
            },
        );

        delete_queue(
            State(state.clone()),
            Path(queue_id),
            Query(DeleteQueueQuery {
                delete_downloads: true,
            }),
        )
        .await
        .unwrap();

        assert!(state.db.get_queue(queue_id).unwrap().is_none());
        assert!(state.db.get_download(download_id).unwrap().is_none());
        assert!(
            state
                .db
                .list_download_artifacts(download_id)
                .unwrap()
                .is_empty()
        );
        assert!(!state.live_status.read().await.contains_key(&download_id));
        assert!(file.exists());
        fs::remove_file(file).unwrap();
    }

    #[tokio::test]
    async fn main_queue_is_protected() {
        let state = state();
        let result = delete_queue(
            State(state.clone()),
            Path(MAIN_QUEUE_ID),
            Query(DeleteQueueQuery {
                delete_downloads: true,
            }),
        )
        .await;

        assert!(matches!(result, Err(AppError::BadRequest(_))));
        assert!(state.db.get_queue(MAIN_QUEUE_ID).unwrap().is_some());
    }
}

#[cfg(test)]
mod resume_tests {
    use super::*;
    use crate::{aria2::Aria2Client, config::ServerConfig, db::Database};
    use chrono::Utc;
    use common::{
        download::Download,
        enums::{DownloadStatus, FileCategory, QueueStatus, Recurrence, SourceType},
        finetune::FineTune,
        queue::{Queue, QueueSettings},
        scheduler::Scheduler,
    };

    fn state() -> AppState {
        AppState::new(
            Database::open(":memory:").unwrap(),
            Aria2Client::new("http://127.0.0.1:1/jsonrpc", None),
            ServerConfig::default(),
            false,
        )
    }

    fn insert_queue(state: &AppState) -> i64 {
        state
            .db
            .insert_queue(&Queue {
                id: 0,
                name: "Retry".into(),
                position: 1,
                settings: QueueSettings {
                    max_concurrent_downloads: 1,
                    max_retries: 3,
                    default_finetune: FineTune::default(),
                },
                scheduler: Scheduler {
                    enabled: false,
                    recurrence: Recurrence::Once {
                        start: Utc::now(),
                        end: Utc::now(),
                    },
                    run_missed_on_startup: false,
                },
                status: QueueStatus::Paused,
                created_at: Utc::now(),
                running: false,
            })
            .unwrap()
    }

    fn insert_download(
        state: &AppState,
        queue_id: i64,
        status: DownloadStatus,
        retry_count: u32,
    ) -> i64 {
        state
            .db
            .insert_download(&Download {
                id: 0,
                aria2_gid: None,
                url: "https://example.test/file.bin".into(),
                filename: Some("file.bin".into()),
                destination_path: "/tmp".into(),
                source_type: SourceType::Torrent,
                category: FileCategory::Other,
                status,
                paused_by_scheduler: false,
                manually_started: false,
                retry_count,
                size: None,
                completed_length: None,
                queue_id,
                position_in_queue: 0,
                finetune: FineTune::default(),
                created_at: Utc::now(),
                started_at: None,
                completed_at: None,
            })
            .unwrap()
    }

    #[tokio::test]
    async fn empty_or_completed_queue_resume_does_nothing() {
        let state = state();
        let empty_id = insert_queue(&state);
        let status = resume_queue(State(state.clone()), Path(empty_id))
            .await
            .unwrap();
        assert_eq!(status, axum::http::StatusCode::NO_CONTENT);
        assert_eq!(
            state.db.get_queue(empty_id).unwrap().unwrap().status,
            QueueStatus::Paused
        );

        let completed_id = insert_queue(&state);
        insert_download(&state, completed_id, DownloadStatus::Completed, 0);
        let status = resume_queue(State(state.clone()), Path(completed_id))
            .await
            .unwrap();
        assert_eq!(status, axum::http::StatusCode::NO_CONTENT);
        assert_eq!(
            state.db.get_queue(completed_id).unwrap().unwrap().status,
            QueueStatus::Paused
        );
    }

    #[tokio::test]
    async fn resume_retries_an_eligible_error_with_a_fresh_budget() {
        let state = state();
        let queue_id = insert_queue(&state);
        let download_id = insert_download(
            &state,
            queue_id,
            DownloadStatus::Error("disk full".into()),
            4,
        );

        let status = resume_queue(State(state.clone()), Path(queue_id))
            .await
            .unwrap();

        assert_eq!(status, axum::http::StatusCode::OK);
        assert_eq!(
            state.db.get_queue(queue_id).unwrap().unwrap().status,
            QueueStatus::Active
        );
        let download = state.db.get_download(download_id).unwrap().unwrap();
        assert_eq!(download.retry_count, 1);
        assert_eq!(download.status, DownloadStatus::Error("disk full".into()));
    }

    #[tokio::test]
    async fn running_requires_retryable_work() {
        let state = state();
        let queue_id = insert_queue(&state);
        state
            .db
            .update_queue_status(queue_id, QueueStatus::Active)
            .unwrap();
        insert_download(
            &state,
            queue_id,
            DownloadStatus::Error("disk full".into()),
            3,
        );

        let exhausted =
            with_running_state(&state, state.db.get_queue(queue_id).unwrap().unwrap()).unwrap();
        assert!(!exhausted.running);

        state.db.reset_error_retry_counts(queue_id).unwrap();
        let eligible =
            with_running_state(&state, state.db.get_queue(queue_id).unwrap().unwrap()).unwrap();
        assert!(eligible.running);
    }
}
