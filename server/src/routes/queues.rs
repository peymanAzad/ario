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
        current_schedule_occurrence, effective_queue_status, pause_downloads,
        start_eligible_downloads,
    },
};

const MAIN_QUEUE_ID: i64 = 1;

fn validate_schedule(
    enabled: bool,
    recurrence: &common::enums::Recurrence,
    now: chrono::DateTime<Utc>,
) -> Result<(), AppError> {
    if !enabled {
        return Ok(());
    }
    let valid = match recurrence {
        common::enums::Recurrence::Once { start, end } => start < end,
        common::enums::Recurrence::Weekly { .. } => {
            crate::scheduler::next_stop(recurrence, now, &chrono::Local).is_some()
        }
    };
    if !valid {
        return Err(AppError::BadRequest(
            "schedule needs valid days and a stop after its start".into(),
        ));
    }
    Ok(())
}

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
    let mut queues = state.db.list_queues()?;
    for queue in &mut queues {
        queue.status = effective_queue_status(&state.db, queue, state.now())?;
    }
    Ok(Json(queues))
}

async fn get_queue(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Queue>, AppError> {
    let mut queue = state
        .db
        .get_queue(id)?
        .ok_or_else(|| AppError::NotFound(format!("queue {id}")))?;
    queue.status = effective_queue_status(&state.db, &queue, state.now())?;
    Ok(Json(queue))
}

async fn create_queue(
    State(state): State<AppState>,
    Json(req): Json<CreateQueueRequest>,
) -> Result<Json<Queue>, AppError> {
    let _activity = state.activity_guard().await?;
    let _control = state.control.lock().await;
    validate_schedule(req.scheduler_enabled, &req.recurrence, state.now())?;
    let queue = Queue {
        scheduled_stop_at: None,
        id: 0, // placeholder — overwritten by the id sqlite assigns on insert
        name: req.name,
        position: req.position,
        settings: QueueSettings {
            max_concurrent_downloads: req.max_concurrent_downloads,
            max_retries: req.max_retries,
            retry_wait_seconds: req.retry_wait_seconds,
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
    let mut created = state
        .db
        .get_queue(id)?
        .ok_or_else(|| AppError::NotFound(format!("queue {id}")))?;
    created.status = effective_queue_status(&state.db, &created, state.now())?;
    Ok(Json(created))
}

async fn update_queue(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<UpdateQueueRequest>,
) -> Result<Json<Queue>, AppError> {
    let _activity = state.activity_guard().await?;
    let _control = state.control.lock().await;
    let existing = state
        .db
        .get_queue(id)?
        .ok_or_else(|| AppError::NotFound(format!("queue {id}")))?;

    validate_schedule(req.scheduler_enabled, &req.recurrence, state.now())?;
    let queue = Queue {
        scheduled_stop_at: existing.scheduled_stop_at,
        id,
        name: req.name,
        position: req.position,
        settings: QueueSettings {
            max_concurrent_downloads: req.max_concurrent_downloads,
            max_retries: req.max_retries,
            retry_wait_seconds: req.retry_wait_seconds,
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
        state.db.set_scheduled_stop(id, None)?;
        if queue.scheduler.enabled
            && (queue.status == common::enums::QueueStatus::Active
                || state.db.count_active_downloads_in_queue(id)? > 0)
        {
            let stop = crate::scheduler::next_stop(
                &queue.scheduler.recurrence,
                state.now(),
                &chrono::Local,
            )
            .unwrap_or_else(|| state.now());
            state.db.set_scheduled_stop(id, Some(stop))?;
        }
        crate::scheduler::reconcile_queue(&state, id, state.now())
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;
    }
    let mut updated = state
        .db
        .get_queue(id)?
        .ok_or_else(|| AppError::NotFound(format!("queue {id}")))?;
    updated.status = effective_queue_status(&state.db, &updated, state.now())?;
    Ok(Json(updated))
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
    let _control = state.control.lock().await;

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
    let _control = state.control.lock().await;
    let queue = state
        .db
        .get_queue(id)?
        .ok_or_else(|| AppError::NotFound(format!("queue {id}")))?;
    crate::scheduler::prepare_start(&state, &queue, state.now()).await?;
    state.db.set_queue_scheduler_suppression(id, None)?;
    state
        .db
        .update_queue_status(id, common::enums::QueueStatus::Active)?;
    start_eligible_downloads(&state, &queue, true).await?;
    Ok(axum::http::StatusCode::OK)
}

async fn pause_queue(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<axum::http::StatusCode, AppError> {
    let _activity = state.activity_guard().await?;
    let _control = state.control.lock().await;
    let queue = state
        .db
        .get_queue(id)?
        .ok_or_else(|| AppError::NotFound(format!("queue {id}")))?;
    let suppressed_occurrence = queue
        .scheduler
        .enabled
        .then(|| current_schedule_occurrence(&queue.scheduler.recurrence, state.now()))
        .flatten();
    state
        .db
        .set_queue_scheduler_suppression(id, suppressed_occurrence.as_deref())?;
    state
        .db
        .update_queue_status(id, common::enums::QueueStatus::Paused)?;
    pause_downloads(&state, &queue, false).await?;
    if state.db.count_active_downloads_in_queue(id)? == 0 {
        state.db.set_scheduled_stop(id, None)?;
    }
    Ok(axum::http::StatusCode::OK)
}

#[cfg(test)]
mod control_tests {
    use super::*;
    use crate::{aria2::Aria2Client, config::ServerConfig, db::Database};
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use chrono::{Duration, TimeZone};
    use common::{
        download::{AddDownloadInput, AddDownloadsRequest, Download},
        enums::{DownloadStatus, FileCategory, QueueStatus, Recurrence, SourceType},
        finetune::FineTune,
    };
    use tower::ServiceExt;

    fn state() -> AppState {
        let now = Utc.with_ymd_and_hms(2030, 1, 7, 12, 0, 0).unwrap();
        AppState::new(
            Database::open(":memory:").unwrap(),
            Aria2Client::new("http://127.0.0.1:1/jsonrpc", None),
            ServerConfig::default(),
            false,
        )
        .with_clock(move || now)
    }

    fn request(queue: &Queue) -> UpdateQueueRequest {
        UpdateQueueRequest {
            name: queue.name.clone(),
            position: queue.position,
            max_concurrent_downloads: queue.settings.max_concurrent_downloads,
            max_retries: queue.settings.max_retries,
            retry_wait_seconds: queue.settings.retry_wait_seconds,
            default_finetune: queue.settings.default_finetune.clone(),
            scheduler_enabled: queue.scheduler.enabled,
            recurrence: queue.scheduler.recurrence.clone(),
            run_missed_on_startup: queue.scheduler.run_missed_on_startup,
        }
    }

    fn expired_queue(state: &AppState) -> Queue {
        let mut queue = state.db.get_queue(1).unwrap().unwrap();
        queue.scheduler.enabled = true;
        queue.scheduler.recurrence = Recurrence::Once {
            start: state.now() - Duration::hours(2),
            end: state.now() - Duration::hours(1),
        };
        state.db.update_queue(&queue).unwrap();
        queue
    }

    fn download(state: &AppState) -> i64 {
        state
            .db
            .insert_download(&Download {
                id: 0,
                aria2_gid: None,
                url: "https://example.test/a".into(),
                filename: None,
                destination_path: "/tmp".into(),
                source_type: SourceType::Http,
                category: FileCategory::Other,
                status: DownloadStatus::Pending,
                paused_by_scheduler: false,
                manually_started: false,
                size: None,
                completed_length: None,
                queue_id: 1,
                position_in_queue: 0,
                finetune: FineTune::default(),
                created_at: state.now(),
                started_at: None,
                completed_at: None,
            })
            .unwrap()
    }

    #[tokio::test]
    async fn expired_schedule_rejects_queue_individual_and_immediate_add_starts() {
        let state = state();
        expired_queue(&state);
        let id = download(&state);
        let app = router()
            .merge(crate::routes::downloads::router())
            .with_state(state.clone());
        for uri in [
            "/queues/1/resume".to_owned(),
            format!("/downloads/{id}/resume"),
        ] {
            let response = app
                .clone()
                .oneshot(Request::post(uri).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        }
        let body = serde_json::to_vec(&AddDownloadsRequest {
            inputs: vec![AddDownloadInput::Url("https://example.test/new".into())],
            queue_id: 1,
            finetune_override: None,
            start_immediately: true,
        })
        .unwrap();
        let response = app
            .oneshot(
                Request::post("/downloads")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            state
                .db
                .list_downloads(&DownloadFilter::default())
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            state.db.get_download(id).unwrap().unwrap().status,
            DownloadStatus::Pending
        );
    }

    #[tokio::test]
    async fn schedule_edits_recompute_clear_or_expire_running_deadline() {
        let state = state();
        let mut queue = state.db.get_queue(1).unwrap().unwrap();
        state
            .db
            .update_queue_status(1, QueueStatus::Active)
            .unwrap();
        queue.scheduler.enabled = true;
        queue.scheduler.recurrence = Recurrence::Once {
            start: state.now() - Duration::hours(1),
            end: state.now() + Duration::hours(1),
        };
        let Json(updated) = update_queue(State(state.clone()), Path(1), Json(request(&queue)))
            .await
            .unwrap();
        assert_eq!(
            updated.scheduled_stop_at,
            Some(state.now() + Duration::hours(1))
        );
        assert_eq!(updated.status, QueueStatus::Active);

        queue.scheduler.recurrence = Recurrence::Once {
            start: state.now() - Duration::hours(1),
            end: state.now() + Duration::hours(2),
        };
        let Json(updated) = update_queue(State(state.clone()), Path(1), Json(request(&queue)))
            .await
            .unwrap();
        assert_eq!(
            updated.scheduled_stop_at,
            Some(state.now() + Duration::hours(2))
        );

        queue.scheduler.enabled = false;
        let Json(updated) = update_queue(State(state.clone()), Path(1), Json(request(&queue)))
            .await
            .unwrap();
        assert!(updated.scheduled_stop_at.is_none());
        assert_eq!(updated.status, QueueStatus::Active);

        queue.scheduler.enabled = true;
        queue.scheduler.recurrence = Recurrence::Once {
            start: state.now() - Duration::hours(2),
            end: state.now() - Duration::hours(1),
        };
        let Json(updated) = update_queue(State(state.clone()), Path(1), Json(request(&queue)))
            .await
            .unwrap();
        assert_eq!(updated.status, QueueStatus::Paused);
        assert!(updated.scheduled_stop_at.is_none());
        assert_eq!(
            state.db.get_queue(1).unwrap().unwrap().status,
            QueueStatus::Paused
        );
    }

    #[tokio::test]
    async fn overdue_deadline_is_reported_paused_even_before_rpc_pause_succeeds() {
        let state = state();
        expired_queue(&state);
        state
            .db
            .update_queue_status(1, QueueStatus::Active)
            .unwrap();
        state.db.set_scheduled_stop(1, Some(state.now())).unwrap();
        let queue = state.db.get_queue(1).unwrap().unwrap();
        assert_eq!(
            effective_queue_status(&state.db, &queue, state.now()).unwrap(),
            QueueStatus::Paused
        );
    }
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
                scheduled_stop_at: None,
                id: 0,
                name: name.into(),
                position: 1,
                settings: QueueSettings {
                    max_concurrent_downloads: 1,
                    max_retries: 3,
                    retry_wait_seconds: 5,
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
