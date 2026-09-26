//! Queue control runs after transfer polling. Manual runs respect persisted schedule deadlines.
//! Weekly recurrences use server-local time; persisted stop deadlines use UTC.
use crate::{aria2::Aria2AddMode, routes::downloads::start_in_aria2, state::AppState};
use chrono::{
    DateTime, Datelike, Local, LocalResult, NaiveDate, NaiveTime, TimeZone, Utc, Weekday,
};
use common::{
    enums::{DownloadStatus, QueueStatus, Recurrence},
    queue::Queue,
};

/// Compute the next stop using the supplied clock and timezone.
pub fn next_stop<T: TimeZone>(
    recurrence: &Recurrence,
    now: DateTime<Utc>,
    zone: &T,
) -> Option<DateTime<Utc>> {
    match recurrence {
        Recurrence::Once { end, .. } => (*end > now).then_some(*end),
        Recurrence::Weekly {
            days,
            start_time,
            end_time,
        } => {
            if days.is_empty() || start_time == end_time {
                return None;
            }
            let today = now.with_timezone(zone).date_naive();
            (-1..=14)
                .filter_map(|offset| {
                    let date = today.checked_add_signed(chrono::Duration::days(offset))?;
                    if !days.contains(&date.weekday()) {
                        return None;
                    }
                    let end_date = if start_time > end_time {
                        date.succ_opt()?
                    } else {
                        date
                    };
                    let local = end_date.and_time(*end_time);
                    let candidates = match zone.from_local_datetime(&local) {
                        LocalResult::Single(value) => vec![value.with_timezone(&Utc)],
                        LocalResult::Ambiguous(a, b) => {
                            vec![a.with_timezone(&Utc), b.with_timezone(&Utc)]
                        }
                        LocalResult::None => vec![],
                    };
                    candidates.into_iter().filter(|end| *end > now).min()
                })
                .min()
        }
    }
}

pub async fn prepare_start(
    state: &AppState,
    queue: &Queue,
    now: DateTime<Utc>,
) -> Result<(), crate::error::AppError> {
    let stop = if queue.scheduler.enabled {
        Some(
            next_stop(&queue.scheduler.recurrence, now, &Local).ok_or_else(|| {
                crate::error::AppError::BadRequest(
                    "schedule has no future stop; disable or edit the schedule before starting"
                        .into(),
                )
            })?,
        )
    } else {
        None
    };
    if queue.scheduler.enabled && queue.scheduled_stop_at.is_some_and(|end| now >= end) {
        state
            .db
            .update_queue_status(queue.id, QueueStatus::Paused)?;
        pause_downloads(state, queue, true)
            .await
            .map_err(|error| crate::error::AppError::Internal(error.to_string()))?;
    }
    // Repeated starts must not extend an existing run past its original stop.
    state.db.set_scheduled_stop(
        queue.id,
        queue
            .scheduled_stop_at
            .filter(|end| queue.scheduler.enabled && *end > now)
            .or(stop),
    )?;
    Ok(())
}

/// Caller holds activity gate and control mutex.
pub async fn reconcile_queue(state: &AppState, id: i64, now: DateTime<Utc>) -> anyhow::Result<()> {
    let Some(mut queue) = state.db.get_queue(id)? else {
        return Ok(());
    };
    if !queue.scheduler.enabled {
        state.db.set_scheduled_stop(id, None)?;
        if queue.status == QueueStatus::Active {
            start_eligible_downloads(state, &queue, false).await?;
        }
        return Ok(());
    }
    let active = state.db.count_active_downloads_in_queue(id)?;
    if queue.scheduled_stop_at.is_none() && (queue.status == QueueStatus::Active || active > 0) {
        let stop = next_stop(&queue.scheduler.recurrence, now, &Local).unwrap_or(now);
        state.db.set_scheduled_stop(id, Some(stop))?;
        queue.scheduled_stop_at = Some(stop);
    }
    if queue.scheduled_stop_at.is_some_and(|stop| now >= stop) {
        // Keep the expired deadline until all RPC pauses succeed; never refill while stopping.
        state.db.update_queue_status(id, QueueStatus::Paused)?;
        pause_downloads(state, &queue, true).await?;
        if let Some(stop) = queue.scheduled_stop_at {
            let ended = schedule_occurrence(
                &queue.scheduler.recurrence,
                stop - chrono::Duration::nanoseconds(1),
            );
            state
                .db
                .set_queue_scheduler_suppression(id, ended.as_deref())?;
        }
        state.db.set_scheduled_stop(id, None)?;
        queue.status = QueueStatus::Paused;
        queue.scheduled_stop_at = None;
    }
    let occurrence = schedule_occurrence(&queue.scheduler.recurrence, now);
    match schedule_decision(&state.db, id, &queue.status, occurrence.as_deref())? {
        ScheduleDecision::Start => {
            if queue.scheduled_stop_at.is_none() {
                prepare_start(state, &queue, now).await?;
            }
            start_eligible_downloads(state, &queue, false).await?;
        }
        ScheduleDecision::StayPaused => pause_downloads(state, &queue, false).await?,
        ScheduleDecision::CloseWindow => {
            // Individual manual starts may run until the next stop without arming the queue.
            if queue.scheduled_stop_at.is_none() {
                pause_downloads(state, &queue, true).await?;
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScheduleDecision {
    Start,
    StayPaused,
    CloseWindow,
}

fn schedule_decision(
    db: &crate::db::Database,
    id: i64,
    status: &QueueStatus,
    occurrence: Option<&str>,
) -> anyhow::Result<ScheduleDecision> {
    if *status == QueueStatus::Active {
        return Ok(ScheduleDecision::Start);
    }
    let suppressed = db.get_queue_scheduler_suppression(id)?;
    match occurrence {
        Some(current) if suppressed.as_deref() == Some(current) => Ok(ScheduleDecision::StayPaused),
        Some(_) => {
            db.set_queue_scheduler_suppression(id, None)?;
            Ok(ScheduleDecision::Start)
        }
        None => Ok(ScheduleDecision::CloseWindow),
    }
}

/// Caller holds control mutex. Explicit Start also resumes user-paused items.
pub async fn start_eligible_downloads(
    state: &AppState,
    queue: &Queue,
    explicit: bool,
) -> anyhow::Result<()> {
    let mut capacity = (queue.settings.max_concurrent_downloads as i64
        - state.db.count_active_downloads_in_queue(queue.id)?)
    .max(0);
    let candidates = state.db.list_startable_downloads(queue.id, explicit)?;
    if explicit {
        // Queue Start authorizes every paused item, including those beyond current capacity.
        // Subsequent automatic refills must retain that intent.
        for download in &candidates {
            if download.status == DownloadStatus::Paused {
                state.db.set_paused_by_scheduler(download.id, true)?;
            }
        }
    }
    for download in candidates {
        if capacity == 0 {
            break;
        }
        if let Some(current) = state.db.get_queue(queue.id)? {
            if current.scheduler.enabled
                && current
                    .scheduled_stop_at
                    .is_some_and(|stop| state.now() >= stop)
            {
                break;
            }
        }
        let result: anyhow::Result<()> = async {
            if let Some(gid) = &download.aria2_gid {
                state.aria2.unpause(gid).await?;
            } else {
                let gid = start_in_aria2(state, &download, Aria2AddMode::Fresh).await?;
                state.db.update_download_gid(download.id, &gid)?;
            }
            state
                .db
                .update_download_status(download.id, &DownloadStatus::Active)?;
            state.db.set_paused_by_scheduler(download.id, false)?;
            Ok(())
        }
        .await;
        match result {
            Ok(()) => capacity -= 1,
            Err(error) => {
                eprintln!(
                    "queue {}: failed to start item {}: {error}",
                    queue.id, download.id
                );
                state.db.update_download_status(
                    download.id,
                    &DownloadStatus::Error(error.to_string()),
                )?;
            }
        }
    }
    Ok(())
}

pub async fn pause_downloads(
    state: &AppState,
    queue: &Queue,
    paused_by_scheduler: bool,
) -> anyhow::Result<()> {
    let active = if paused_by_scheduler {
        state.db.list_downloads(&common::download::DownloadFilter {
            queue_id: Some(queue.id),
            status: Some(DownloadStatus::Active),
            ..Default::default()
        })?
    } else {
        state.db.list_queue_controlled_active_downloads(queue.id)?
    };
    let mut failure = None;
    for download in active {
        if paused_by_scheduler {
            // Persist stop intent before RPC: if its response is lost but aria2
            // did pause, the next poll must retain automatic-resume eligibility.
            state.db.set_paused_by_scheduler(download.id, true)?;
            state.db.set_manually_started(download.id, false)?;
        }
        if let Some(gid) = &download.aria2_gid {
            if let Err(error) = state.aria2.pause(gid).await {
                failure = Some(error);
                continue;
            }
        }
        state
            .db
            .update_download_status(download.id, &DownloadStatus::Paused)?;
        state
            .db
            .set_paused_by_scheduler(download.id, paused_by_scheduler || queue.scheduler.enabled)?;
        if paused_by_scheduler {
            state.db.set_manually_started(download.id, false)?;
        }
        state.live_status.write().await.remove(&download.id);
    }
    if let Some(error) = failure {
        return Err(error.into());
    }
    Ok(())
}

/// Returns the queue state presented to clients. `QueueStatus::Active` in the
/// database arms a manual run until its deadline; a scheduled queue can also be
/// effectively running during an unsuppressed open occurrence without changing
/// that stored control state.
pub fn effective_queue_status(
    db: &crate::db::Database,
    queue: &Queue,
    now: DateTime<Utc>,
) -> anyhow::Result<QueueStatus> {
    let occurrence = schedule_occurrence(&queue.scheduler.recurrence, now);
    effective_queue_status_at(db, queue, occurrence.as_deref(), now)
}

#[cfg(test)]
fn effective_queue_status_for_occurrence(
    db: &crate::db::Database,
    queue: &Queue,
    occurrence: Option<&str>,
) -> anyhow::Result<QueueStatus> {
    effective_queue_status_at(db, queue, occurrence, Utc::now())
}

fn effective_queue_status_at(
    db: &crate::db::Database,
    queue: &Queue,
    occurrence: Option<&str>,
    now: DateTime<Utc>,
) -> anyhow::Result<QueueStatus> {
    if queue.scheduler.enabled && queue.scheduled_stop_at.is_some_and(|stop| now >= stop) {
        return Ok(QueueStatus::Paused);
    }
    if queue.status == QueueStatus::Active {
        return Ok(QueueStatus::Active);
    }
    if !queue.scheduler.enabled {
        return Ok(QueueStatus::Paused);
    }

    let Some(occurrence) = occurrence else {
        return Ok(QueueStatus::Paused);
    };
    let suppressed = db.get_queue_scheduler_suppression(queue.id)?;
    Ok(if suppressed.as_deref() == Some(occurrence) {
        QueueStatus::Paused
    } else {
        QueueStatus::Active
    })
}

/// Identifies an open schedule occurrence using exclusive stop boundaries.
pub fn current_schedule_occurrence(recurrence: &Recurrence, now: DateTime<Utc>) -> Option<String> {
    schedule_occurrence(recurrence, now)
}

fn schedule_occurrence(recurrence: &Recurrence, now: DateTime<Utc>) -> Option<String> {
    schedule_occurrence_in(recurrence, now, &Local)
}

fn schedule_occurrence_in<T: TimeZone>(
    recurrence: &Recurrence,
    now: DateTime<Utc>,
    zone: &T,
) -> Option<String> {
    match recurrence {
        Recurrence::Once { start, end } => {
            (now >= *start && now < *end).then(|| format!("once:{}", start.to_rfc3339()))
        }
        Recurrence::Weekly {
            days,
            start_time,
            end_time,
        } => {
            let local_now = now.with_timezone(zone);
            let date = weekly_occurrence_date(
                days,
                *start_time,
                *end_time,
                local_now.date_naive(),
                local_now.time(),
            )?;
            let end_date = if start_time > end_time {
                date.succ_opt()?
            } else {
                date
            };
            let end = match zone.from_local_datetime(&end_date.and_time(*end_time)) {
                LocalResult::Single(end) => end.with_timezone(&Utc),
                LocalResult::Ambiguous(a, b) => a.with_timezone(&Utc).min(b.with_timezone(&Utc)),
                LocalResult::None => return None,
            };
            (now < end).then(|| format!("weekly:{date}:{start_time}"))
        }
    }
}

fn weekly_occurrence_date(
    days: &[Weekday],
    start: NaiveTime,
    end: NaiveTime,
    today: NaiveDate,
    time_now: NaiveTime,
) -> Option<NaiveDate> {
    let weekday = today.weekday();

    if start <= end {
        // Same-day window, no midnight crossing.
        (days.contains(&weekday) && time_now >= start && time_now < end).then_some(today)
    } else {
        // Crosses midnight (e.g. 22:00-02:00): either today's late part, or
        // yesterday's window continuing into this morning.
        if days.contains(&weekday) && time_now >= start {
            Some(today)
        } else if days.contains(&weekday.pred()) && time_now < end {
            today.pred_opt()
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::{finetune::FineTune, queue::QueueSettings, scheduler::Scheduler};

    use crate::{aria2::Aria2Client, config::ServerConfig, db::Database};
    use common::{
        download::Download,
        enums::{FileCategory, SourceType},
    };
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    };

    struct Fixture {
        state: AppState,
        clock: Arc<Mutex<DateTime<Utc>>>,
        calls: Arc<Mutex<Vec<serde_json::Value>>>,
        fail_pause: Arc<AtomicBool>,
        server: tokio::task::JoinHandle<()>,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            self.server.abort();
        }
    }

    async fn fixture() -> Fixture {
        use axum::{Json, Router, extract::State, routing::post};
        type RpcState = (Arc<Mutex<Vec<serde_json::Value>>>, Arc<AtomicBool>);
        async fn rpc(
            State((calls, fail_pause)): State<RpcState>,
            Json(request): Json<serde_json::Value>,
        ) -> Json<serde_json::Value> {
            let mut calls = calls.lock().unwrap();
            calls.push(request.clone());
            let failed = (request["method"] == "aria2.pause" && fail_pause.load(Ordering::SeqCst))
                || (request["method"] == "aria2.addUri"
                    && request["params"][0][0] == "https://example.test/fail");
            Json(if failed {
                serde_json::json!({"jsonrpc":"2.0", "id": request["id"], "error": {"code": 1, "message": "test failure"}})
            } else {
                serde_json::json!({"jsonrpc":"2.0", "id": request["id"], "result": format!("gid-{}", calls.len())})
            })
        }
        let calls = Arc::new(Mutex::new(Vec::new()));
        let fail_pause = Arc::new(AtomicBool::new(false));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/jsonrpc", listener.local_addr().unwrap());
        let app = Router::new()
            .route("/jsonrpc", post(rpc))
            .with_state((calls.clone(), fail_pause.clone()));
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let clock = Arc::new(Mutex::new(
            Utc.with_ymd_and_hms(2030, 1, 7, 12, 0, 0).unwrap(),
        ));
        let clock_reader = clock.clone();
        let state = AppState::new(
            Database::open(":memory:").unwrap(),
            Aria2Client::new(url, None),
            ServerConfig::default(),
            false,
        )
        .with_clock(move || *clock_reader.lock().unwrap());
        Fixture {
            state,
            clock,
            calls,
            fail_pause,
            server,
        }
    }

    fn item(state: &AppState, position: i32, status: DownloadStatus) -> Download {
        let mut download = Download {
            id: 0,
            aria2_gid: None,
            url: format!("https://example.test/{position}"),
            filename: Some(format!("{position}.bin")),
            destination_path: "/tmp".into(),
            source_type: SourceType::Http,
            category: FileCategory::Other,
            status,
            paused_by_scheduler: false,
            manually_started: false,
            size: Some(100),
            completed_length: Some(20),
            queue_id: 1,
            position_in_queue: position,
            finetune: FineTune::default(),
            created_at: state.now(),
            started_at: None,
            completed_at: None,
        };
        download.id = state.db.insert_download(&download).unwrap();
        download
    }

    fn configure(state: &AppState, enabled: bool, status: QueueStatus, capacity: u32) -> Queue {
        let mut queue = state.db.get_queue(1).unwrap().unwrap();
        queue.settings.max_concurrent_downloads = capacity;
        queue.scheduler.enabled = enabled;
        queue.scheduler.recurrence = Recurrence::Once {
            start: state.now() - chrono::Duration::hours(1),
            end: state.now() + chrono::Duration::hours(1),
        };
        state.db.update_queue(&queue).unwrap();
        state.db.update_queue_status(1, status).unwrap();
        state.db.get_queue(1).unwrap().unwrap()
    }

    #[tokio::test]
    async fn manual_queue_advances_on_completion_without_scheduler() {
        let f = fixture().await;
        configure(&f.state, false, QueueStatus::Active, 1);
        let first = item(&f.state, 0, DownloadStatus::Pending);
        let second = item(&f.state, 1, DownloadStatus::Pending);
        reconcile_queue(&f.state, 1, f.state.now()).await.unwrap();
        assert_eq!(
            f.state.db.get_download(second.id).unwrap().unwrap().status,
            DownloadStatus::Pending
        );
        let first = f.state.db.get_download(first.id).unwrap().unwrap();
        crate::poller::reconcile_transfer_state(&f.state, &first, "complete", 100, 0, None).await;
        reconcile_queue(&f.state, 1, f.state.now()).await.unwrap();
        assert_eq!(
            f.state.db.get_download(first.id).unwrap().unwrap().status,
            DownloadStatus::Completed
        );
        assert_eq!(
            f.state.db.get_download(second.id).unwrap().unwrap().status,
            DownloadStatus::Active
        );
        assert_eq!(f.calls.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn failed_candidate_does_not_strand_capacity_or_resume_user_paused_items() {
        let f = fixture().await;
        let queue = configure(&f.state, false, QueueStatus::Active, 2);
        let template = item(&f.state, 0, DownloadStatus::Completed);
        let paused = item(&f.state, 1, DownloadStatus::Paused);
        f.state
            .db
            .set_paused_by_scheduler(paused.id, false)
            .unwrap();
        let a = item(&f.state, 2, DownloadStatus::Pending);
        let b = item(&f.state, 3, DownloadStatus::Pending);
        // The failed URI precedes both healthy candidates.
        let mut failed = template.clone();
        failed.status = DownloadStatus::Pending;
        failed.id = 0;
        failed.url = "https://example.test/fail".into();
        failed.position_in_queue = -1;
        let failed_id = f.state.db.insert_download(&failed).unwrap();
        start_eligible_downloads(&f.state, &queue, false)
            .await
            .unwrap();
        assert!(matches!(
            f.state.db.get_download(failed_id).unwrap().unwrap().status,
            DownloadStatus::Error(_)
        ));
        assert_eq!(
            f.state.db.get_download(paused.id).unwrap().unwrap().status,
            DownloadStatus::Paused
        );
        assert_eq!(
            f.state.db.get_download(a.id).unwrap().unwrap().status,
            DownloadStatus::Active
        );
        assert_eq!(
            f.state.db.get_download(b.id).unwrap().unwrap().status,
            DownloadStatus::Active
        );
        assert_eq!(f.state.db.count_active_downloads_in_queue(1).unwrap(), 2);
    }

    #[tokio::test]
    async fn scheduled_stop_pauses_individuals_and_retries_failures_without_refilling() {
        let f = fixture().await;
        let queue = configure(&f.state, true, QueueStatus::Active, 2);
        let active = item(&f.state, 0, DownloadStatus::Active);
        f.state
            .db
            .update_download_gid(active.id, "individual")
            .unwrap();
        f.state.db.set_manually_started(active.id, true).unwrap();
        let pending = item(&f.state, 1, DownloadStatus::Pending);
        let stop = match queue.scheduler.recurrence {
            Recurrence::Once { end, .. } => end,
            _ => unreachable!(),
        };
        f.state.db.set_scheduled_stop(1, Some(stop)).unwrap();
        *f.clock.lock().unwrap() = stop;
        f.fail_pause.store(true, Ordering::SeqCst);
        assert!(reconcile_queue(&f.state, 1, stop).await.is_err());
        assert_eq!(
            f.state.db.get_queue(1).unwrap().unwrap().scheduled_stop_at,
            Some(stop)
        );
        assert_eq!(
            f.state.db.get_download(pending.id).unwrap().unwrap().status,
            DownloadStatus::Pending
        );
        f.fail_pause.store(false, Ordering::SeqCst);
        reconcile_queue(&f.state, 1, stop).await.unwrap();
        let active = f.state.db.get_download(active.id).unwrap().unwrap();
        assert_eq!(active.status, DownloadStatus::Paused);
        assert!(active.paused_by_scheduler);
        assert!(!active.manually_started);
        assert_eq!(active.completed_length, Some(20));
        assert_eq!(
            f.state.db.get_queue(1).unwrap().unwrap().status,
            QueueStatus::Paused
        );
        assert!(
            f.state
                .db
                .get_queue(1)
                .unwrap()
                .unwrap()
                .scheduled_stop_at
                .is_none()
        );
        assert!(
            f.calls
                .lock()
                .unwrap()
                .iter()
                .all(|r| r["method"] == "aria2.pause")
        );
    }

    #[tokio::test]
    async fn lost_pause_response_keeps_resume_intent_when_poller_confirms_pause() {
        let f = fixture().await;
        configure(&f.state, true, QueueStatus::Active, 1);
        let active = item(&f.state, 0, DownloadStatus::Active);
        f.state
            .db
            .update_download_gid(active.id, "individual")
            .unwrap();
        f.state.db.set_manually_started(active.id, true).unwrap();
        f.state
            .db
            .set_scheduled_stop(1, Some(f.state.now()))
            .unwrap();
        f.fail_pause.store(true, Ordering::SeqCst);
        assert!(reconcile_queue(&f.state, 1, f.state.now()).await.is_err());
        let snapshot = f.state.db.get_download(active.id).unwrap().unwrap();
        crate::poller::reconcile_transfer_state(&f.state, &snapshot, "paused", 20, 0, None).await;
        reconcile_queue(&f.state, 1, f.state.now()).await.unwrap();
        let paused = f.state.db.get_download(active.id).unwrap().unwrap();
        assert_eq!(paused.status, DownloadStatus::Paused);
        assert!(paused.paused_by_scheduler);
        assert!(!paused.manually_started);
        assert!(
            f.state
                .db
                .get_queue(1)
                .unwrap()
                .unwrap()
                .scheduled_stop_at
                .is_none()
        );
    }

    #[tokio::test]
    async fn individual_start_does_not_arm_queue_and_expired_schedule_rejects_start() {
        let f = fixture().await;
        let mut queue = configure(&f.state, true, QueueStatus::Paused, 1);
        queue.scheduler.recurrence = Recurrence::Once {
            start: f.state.now() + chrono::Duration::hours(1),
            end: f.state.now() + chrono::Duration::hours(2),
        };
        f.state.db.update_queue(&queue).unwrap();
        let pending = item(&f.state, 0, DownloadStatus::Pending);
        let individual = item(&f.state, 1, DownloadStatus::Active);
        f.state
            .db
            .set_manually_started(individual.id, true)
            .unwrap();
        prepare_start(&f.state, &queue, f.state.now())
            .await
            .unwrap();
        reconcile_queue(&f.state, 1, f.state.now()).await.unwrap();
        assert_eq!(
            f.state.db.get_download(pending.id).unwrap().unwrap().status,
            DownloadStatus::Pending
        );
        assert_eq!(
            f.state.db.get_queue(1).unwrap().unwrap().status,
            QueueStatus::Paused
        );
        *f.clock.lock().unwrap() += chrono::Duration::hours(3);
        let queue = f.state.db.get_queue(1).unwrap().unwrap();
        assert!(
            prepare_start(&f.state, &queue, f.state.now())
                .await
                .unwrap_err()
                .to_string()
                .contains("disable or edit")
        );
    }

    #[tokio::test]
    async fn startup_catchup_skips_closed_windows_and_disabled_queue_stays_armed() {
        let f = fixture().await;
        let mut queue = configure(&f.state, true, QueueStatus::Paused, 1);
        queue.scheduler.run_missed_on_startup = true;
        queue.scheduler.recurrence = Recurrence::Once {
            start: f.state.now() - chrono::Duration::hours(2),
            end: f.state.now() - chrono::Duration::hours(1),
        };
        f.state.db.update_queue(&queue).unwrap();
        reconcile_queue(&f.state, 1, f.state.now()).await.unwrap();
        let pending = item(&f.state, 0, DownloadStatus::Pending);
        reconcile_queue(&f.state, 1, f.state.now()).await.unwrap();
        assert_eq!(
            f.state.db.get_download(pending.id).unwrap().unwrap().status,
            DownloadStatus::Pending
        );
        configure(&f.state, false, QueueStatus::Active, 1);
        f.state
            .db
            .set_scheduled_stop(1, Some(f.state.now() - chrono::Duration::seconds(1)))
            .unwrap();
        reconcile_queue(&f.state, 1, f.state.now()).await.unwrap();
        assert_eq!(
            f.state.db.get_download(pending.id).unwrap().unwrap().status,
            DownloadStatus::Active
        );
        assert!(
            f.state
                .db
                .get_queue(1)
                .unwrap()
                .unwrap()
                .scheduled_stop_at
                .is_none()
        );
    }

    #[tokio::test]
    async fn concurrent_refills_never_duplicate_starts_or_exceed_capacity() {
        let f = fixture().await;
        configure(&f.state, false, QueueStatus::Active, 1);
        item(&f.state, 0, DownloadStatus::Pending);
        item(&f.state, 1, DownloadStatus::Pending);
        let refill = || async {
            let _activity = f.state.activity_guard().await.unwrap();
            let _control = f.state.control.lock().await;
            reconcile_queue(&f.state, 1, f.state.now()).await.unwrap();
        };
        tokio::join!(refill(), refill());
        assert_eq!(f.state.db.count_active_downloads_in_queue(1).unwrap(), 1);
        assert_eq!(f.calls.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn concurrent_start_and_pause_requests_leave_consistent_control_state() {
        use axum::{body::Body, http::Request};
        use tower::ServiceExt;
        let f = fixture().await;
        configure(&f.state, false, QueueStatus::Paused, 1);
        item(&f.state, 0, DownloadStatus::Pending);
        item(&f.state, 1, DownloadStatus::Pending);
        let app = crate::routes::queues::router().with_state(f.state.clone());
        let start = app.clone().oneshot(
            Request::post("/queues/1/resume")
                .body(Body::empty())
                .unwrap(),
        );
        let pause = app.oneshot(
            Request::post("/queues/1/pause")
                .body(Body::empty())
                .unwrap(),
        );
        let (started, paused) = tokio::join!(start, pause);
        assert!(started.unwrap().status().is_success());
        assert!(paused.unwrap().status().is_success());
        reconcile_queue(&f.state, 1, f.state.now()).await.unwrap();
        let queue = f.state.db.get_queue(1).unwrap().unwrap();
        let active = f.state.db.count_active_downloads_in_queue(1).unwrap();
        assert_eq!(
            active,
            if queue.status == QueueStatus::Active {
                1
            } else {
                0
            }
        );
        assert_eq!(
            f.calls
                .lock()
                .unwrap()
                .iter()
                .filter(|call| call["method"] == "aria2.addUri")
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn explicit_start_authorizes_paused_items_beyond_initial_capacity() {
        let f = fixture().await;
        let queue = configure(&f.state, false, QueueStatus::Active, 1);
        let first = item(&f.state, 0, DownloadStatus::Paused);
        let second = item(&f.state, 1, DownloadStatus::Paused);
        start_eligible_downloads(&f.state, &queue, true)
            .await
            .unwrap();
        assert!(
            f.state
                .db
                .get_download(second.id)
                .unwrap()
                .unwrap()
                .paused_by_scheduler
        );
        f.state
            .db
            .update_download_status(first.id, &DownloadStatus::Completed)
            .unwrap();
        reconcile_queue(&f.state, 1, f.state.now()).await.unwrap();
        assert_eq!(
            f.state.db.get_download(second.id).unwrap().unwrap().status,
            DownloadStatus::Active
        );
    }

    #[tokio::test]
    async fn next_window_resumes_queue_paused_items_but_not_individually_paused_items() {
        let f = fixture().await;
        let mut queue = configure(&f.state, true, QueueStatus::Paused, 2);
        let active = item(&f.state, 0, DownloadStatus::Active);
        let paused = item(&f.state, 1, DownloadStatus::Paused);
        let occurrence = schedule_occurrence(&queue.scheduler.recurrence, f.state.now()).unwrap();
        f.state
            .db
            .set_queue_scheduler_suppression(1, Some(&occurrence))
            .unwrap();
        pause_downloads(&f.state, &queue, false).await.unwrap();
        reconcile_queue(&f.state, 1, f.state.now()).await.unwrap();
        assert_eq!(
            f.state.db.get_download(active.id).unwrap().unwrap().status,
            DownloadStatus::Paused
        );
        *f.clock.lock().unwrap() += chrono::Duration::days(1);
        queue.scheduler.recurrence = Recurrence::Once {
            start: f.state.now() - chrono::Duration::minutes(1),
            end: f.state.now() + chrono::Duration::minutes(1),
        };
        f.state.db.update_queue(&queue).unwrap();
        reconcile_queue(&f.state, 1, f.state.now()).await.unwrap();
        assert_eq!(
            f.state.db.get_download(active.id).unwrap().unwrap().status,
            DownloadStatus::Active
        );
        assert_eq!(
            f.state.db.get_download(paused.id).unwrap().unwrap().status,
            DownloadStatus::Paused
        );
    }

    #[tokio::test]
    async fn persisted_expired_deadline_wins_even_if_next_window_is_in_the_future() {
        let f = fixture().await;
        let mut queue = configure(&f.state, true, QueueStatus::Active, 1);
        queue.scheduler.recurrence = Recurrence::Once {
            start: f.state.now() + chrono::Duration::hours(1),
            end: f.state.now() + chrono::Duration::hours(2),
        };
        f.state.db.update_queue(&queue).unwrap();
        f.state
            .db
            .set_scheduled_stop(1, Some(f.state.now() - chrono::Duration::hours(1)))
            .unwrap();
        let active = item(&f.state, 0, DownloadStatus::Active);
        f.state
            .db
            .update_download_gid(active.id, "restored")
            .unwrap();
        let next = item(&f.state, 1, DownloadStatus::Pending);
        reconcile_queue(&f.state, 1, f.state.now()).await.unwrap();
        assert_eq!(
            f.state.db.get_download(active.id).unwrap().unwrap().status,
            DownloadStatus::Paused
        );
        assert_eq!(
            f.state.db.get_download(next.id).unwrap().unwrap().status,
            DownloadStatus::Pending
        );
        assert_eq!(f.calls.lock().unwrap()[0]["method"], "aria2.pause");
    }

    #[tokio::test]
    async fn active_empty_queue_starts_items_added_later() {
        let f = fixture().await;
        configure(&f.state, false, QueueStatus::Active, 1);
        reconcile_queue(&f.state, 1, f.state.now()).await.unwrap();
        assert_eq!(
            f.state.db.get_queue(1).unwrap().unwrap().status,
            QueueStatus::Active
        );
        let added = item(&f.state, 0, DownloadStatus::Pending);
        reconcile_queue(&f.state, 1, f.state.now()).await.unwrap();
        assert_eq!(
            f.state.db.get_download(added.id).unwrap().unwrap().status,
            DownloadStatus::Active
        );
    }

    #[test]
    fn weekly_deadlines_cover_before_inside_after_and_overnight_windows() {
        let monday = Utc.with_ymd_and_hms(2030, 1, 7, 8, 0, 0).unwrap();
        let recurrence = Recurrence::Weekly {
            days: vec![Weekday::Mon],
            start_time: time(9, 0),
            end_time: time(17, 0),
        };
        let stop = monday + chrono::Duration::hours(9);
        assert_eq!(next_stop(&recurrence, monday, &Utc), Some(stop));
        assert_eq!(
            next_stop(&recurrence, monday + chrono::Duration::hours(4), &Utc),
            Some(stop)
        );
        assert_eq!(
            next_stop(&recurrence, stop, &Utc),
            Some(stop + chrono::Duration::days(7))
        );
        assert!(schedule_occurrence_in(&recurrence, stop, &Utc).is_none());
        let overnight = Recurrence::Weekly {
            days: vec![Weekday::Mon],
            start_time: time(22, 0),
            end_time: time(2, 0),
        };
        let tuesday = monday + chrono::Duration::hours(17);
        assert_eq!(
            next_stop(&overnight, tuesday, &Utc),
            Some(monday + chrono::Duration::hours(18))
        );
        assert!(
            schedule_occurrence_in(&overnight, monday + chrono::Duration::hours(18), &Utc)
                .is_none()
        );
    }

    // Deterministic DST transitions, without changing process-wide TZ in parallel tests.
    #[derive(Clone)]
    struct DstZone;

    impl TimeZone for DstZone {
        type Offset = chrono::FixedOffset;
        fn from_offset(_: &Self::Offset) -> Self {
            Self
        }
        fn offset_from_local_date(&self, date: &NaiveDate) -> LocalResult<Self::Offset> {
            self.offset_from_local_datetime(&date.and_time(time(0, 0)))
        }
        fn offset_from_local_datetime(
            &self,
            datetime: &chrono::NaiveDateTime,
        ) -> LocalResult<Self::Offset> {
            let winter = chrono::FixedOffset::west_opt(5 * 3600).unwrap();
            let summer = chrono::FixedOffset::west_opt(4 * 3600).unwrap();
            let spring = date(2030, 3, 10);
            let fall = date(2030, 11, 3);
            if datetime.date() == spring
                && datetime.time() >= time(2, 0)
                && datetime.time() < time(3, 0)
            {
                LocalResult::None
            } else if datetime.date() == fall
                && datetime.time() >= time(1, 0)
                && datetime.time() < time(2, 0)
            {
                LocalResult::Ambiguous(summer, winter)
            } else if *datetime >= spring.and_time(time(3, 0))
                && *datetime < fall.and_time(time(1, 0))
            {
                LocalResult::Single(summer)
            } else {
                LocalResult::Single(winter)
            }
        }
        fn offset_from_utc_date(&self, date: &NaiveDate) -> Self::Offset {
            self.offset_from_utc_datetime(&date.and_time(time(0, 0)))
        }
        fn offset_from_utc_datetime(&self, datetime: &chrono::NaiveDateTime) -> Self::Offset {
            if *datetime >= date(2030, 3, 10).and_time(time(7, 0))
                && *datetime < date(2030, 11, 3).and_time(time(6, 0))
            {
                chrono::FixedOffset::west_opt(4 * 3600).unwrap()
            } else {
                chrono::FixedOffset::west_opt(5 * 3600).unwrap()
            }
        }
    }

    #[test]
    fn dst_skips_nonexistent_stops_and_resolves_ambiguous_stops_without_reopening_window() {
        let spring = Recurrence::Weekly {
            days: vec![Weekday::Sun],
            start_time: time(0, 0),
            end_time: time(2, 30),
        };
        let before_gap = Utc.with_ymd_and_hms(2030, 3, 10, 6, 0, 0).unwrap();
        assert_eq!(
            next_stop(&spring, before_gap, &DstZone),
            Some(Utc.with_ymd_and_hms(2030, 3, 17, 6, 30, 0).unwrap())
        );
        assert!(schedule_occurrence_in(&spring, before_gap, &DstZone).is_none());

        let fall = Recurrence::Weekly {
            days: vec![Weekday::Sun],
            start_time: time(0, 0),
            end_time: time(1, 30),
        };
        let first_stop = Utc.with_ymd_and_hms(2030, 11, 3, 5, 30, 0).unwrap();
        let second_stop = Utc.with_ymd_and_hms(2030, 11, 3, 6, 30, 0).unwrap();
        assert_eq!(
            next_stop(&fall, first_stop - chrono::Duration::hours(1), &DstZone),
            Some(first_stop)
        );
        assert_eq!(next_stop(&fall, first_stop, &DstZone), Some(second_stop));
        assert!(
            schedule_occurrence_in(&fall, second_stop - chrono::Duration::minutes(15), &DstZone)
                .is_none()
        );
    }

    fn date(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).unwrap()
    }

    fn time(hour: u32, minute: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(hour, minute, 0).unwrap()
    }

    fn queue(status: QueueStatus, scheduler_enabled: bool) -> Queue {
        Queue {
            scheduled_stop_at: None,
            id: 1,
            name: "test".into(),
            position: 0,
            settings: QueueSettings {
                max_concurrent_downloads: 1,
                max_retries: 3,
                retry_wait_seconds: 5,
                default_finetune: FineTune::default(),
            },
            scheduler: Scheduler {
                enabled: scheduler_enabled,
                recurrence: Recurrence::Once {
                    start: Utc::now(),
                    end: Utc::now(),
                },
                run_missed_on_startup: false,
            },
            created_at: Utc::now(),
            status,
        }
    }

    #[test]
    fn effective_status_covers_manual_scheduled_and_idle_queues() {
        let db = crate::db::Database::open(":memory:").unwrap();

        assert_eq!(
            effective_queue_status_for_occurrence(&db, &queue(QueueStatus::Active, false), None)
                .unwrap(),
            QueueStatus::Active
        );
        assert_eq!(
            effective_queue_status_for_occurrence(
                &db,
                &queue(QueueStatus::Paused, false),
                Some("open")
            )
            .unwrap(),
            QueueStatus::Paused
        );
        assert_eq!(
            effective_queue_status_for_occurrence(
                &db,
                &queue(QueueStatus::Paused, true),
                Some("open")
            )
            .unwrap(),
            QueueStatus::Active
        );

        db.set_queue_scheduler_suppression(1, Some("open")).unwrap();
        assert_eq!(
            effective_queue_status_for_occurrence(
                &db,
                &queue(QueueStatus::Paused, true),
                Some("open")
            )
            .unwrap(),
            QueueStatus::Paused
        );
    }

    #[test]
    fn weekly_occurrences_are_distinct_on_later_scheduled_days() {
        let days = [Weekday::Mon, Weekday::Tue];
        let monday = weekly_occurrence_date(
            &days,
            time(9, 0),
            time(17, 0),
            date(2026, 9, 7),
            time(12, 0),
        );
        let tuesday = weekly_occurrence_date(
            &days,
            time(9, 0),
            time(17, 0),
            date(2026, 9, 8),
            time(12, 0),
        );

        assert_eq!(monday, Some(date(2026, 9, 7)));
        assert_eq!(tuesday, Some(date(2026, 9, 8)));
        assert_ne!(monday, tuesday);
    }

    #[test]
    fn overnight_occurrence_keeps_the_scheduled_start_day() {
        let occurrence = weekly_occurrence_date(
            &[Weekday::Mon],
            time(22, 0),
            time(2, 0),
            date(2026, 9, 8),
            time(1, 0),
        );

        assert_eq!(occurrence, Some(date(2026, 9, 7)));
    }

    #[test]
    fn suppression_holds_for_one_occurrence_then_clears_for_the_next() {
        let db = crate::db::Database::open(":memory:").unwrap();
        let queue_id = 1;
        let monday = "weekly:2026-09-07:09:00:00";
        let tuesday = "weekly:2026-09-08:09:00:00";

        // This is the state written when the user pauses during Monday's
        // open window.
        db.set_queue_scheduler_suppression(queue_id, Some(monday))
            .unwrap();

        assert_eq!(
            schedule_decision(&db, queue_id, &QueueStatus::Paused, Some(monday)).unwrap(),
            ScheduleDecision::StayPaused
        );
        assert_eq!(
            schedule_decision(&db, queue_id, &QueueStatus::Paused, Some(monday)).unwrap(),
            ScheduleDecision::StayPaused
        );

        assert_eq!(
            schedule_decision(&db, queue_id, &QueueStatus::Paused, Some(tuesday)).unwrap(),
            ScheduleDecision::Start
        );
        assert_eq!(db.get_queue_scheduler_suppression(queue_id).unwrap(), None);
    }

    #[test]
    fn manually_started_queue_keeps_running_outside_its_schedule() {
        let db = crate::db::Database::open(":memory:").unwrap();

        assert_eq!(
            schedule_decision(&db, 1, &QueueStatus::Active, None).unwrap(),
            ScheduleDecision::Start
        );
        assert_eq!(
            schedule_decision(&db, 1, &QueueStatus::Active, None).unwrap(),
            ScheduleDecision::Start
        );
    }

    #[test]
    fn manually_started_queue_keeps_running_inside_its_schedule() {
        let db = crate::db::Database::open(":memory:").unwrap();
        let occurrence = "weekly:2026-09-07:09:00:00";
        db.set_queue_scheduler_suppression(1, Some(occurrence))
            .unwrap();

        assert_eq!(
            schedule_decision(&db, 1, &QueueStatus::Active, Some(occurrence)).unwrap(),
            ScheduleDecision::Start
        );
    }

    #[test]
    fn manually_paused_queue_returns_to_closed_window_control() {
        let db = crate::db::Database::open(":memory:").unwrap();

        assert_eq!(
            schedule_decision(&db, 1, &QueueStatus::Paused, None).unwrap(),
            ScheduleDecision::CloseWindow
        );
    }
}
