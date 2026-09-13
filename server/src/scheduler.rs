//! Background scheduler loop: evaluates each queue's `Scheduler` on a fixed
//! tick, starting or pausing that queue's downloads depending on whether
//! it's currently inside its configured window. Plain polling — matching
//! the poll architecture used everywhere else in Ario, per earlier design
//! discussion — not push/event-driven.
//!
//! IMPORTANT — timezone handling: `Recurrence::Weekly`'s `start_time`/
//! `end_time` are evaluated against the SERVER MACHINE's local system time
//! (`chrono::Local`), not UTC. This is deliberate for a personal, single-
//! user, single-machine tool: `server` and the user share a machine, so
//! "2am-6am" naturally means the machine's own local 2am-6am. `Once`, by
//! contrast, is a specific absolute instant (stored/compared in UTC), so it
//! behaves correctly regardless of local timezone — no local-time
//! interpretation needed there.
//!
//! Scheduler-pause vs user-pause: see `common::Download::paused_by_scheduler`
//! doc comment — the scheduler only ever auto-resumes downloads it itself
//! paused; a user-initiated pause is never touched by this loop.
//!
//! KNOWN SIMPLIFICATION (`run_missed_on_startup`): the one-time startup
//! catch-up pass starts eligible downloads immediately regardless of the
//! current window. If the window is genuinely closed, the very next regular
//! tick will pause them again (since this loop can't yet distinguish
//! "started by catch-up" from "started because the window was open"). This
//! still satisfies the core intent (downloads move immediately on startup
//! rather than waiting for the next scheduled window) but a caught-up
//! download won't necessarily run to completion uninterrupted if the window
//! is closed. Worth revisiting with a dedicated "catch-up in progress" flag
//! if this proves annoying in practice.

use crate::aria2::Aria2AddMode;
use crate::state::AppState;
use chrono::{Datelike, Local, NaiveDate, NaiveTime, Utc, Weekday};
use common::{
    enums::{DownloadStatus, QueueStatus, Recurrence},
    queue::Queue,
};
use std::time::Duration;

const TICK_INTERVAL: Duration = Duration::from_secs(60);

/// Runs forever — spawn as its own tokio task from `main`.
pub async fn run(state: AppState) {
    // One-time startup catch-up pass — see module doc comment's "KNOWN
    // SIMPLIFICATION" note.
    if let Ok(queues) = state.db.list_queues() {
        for queue in &queues {
            let Ok(_activity) = state.activity_guard().await else {
                return;
            };
            let occurrence = current_schedule_occurrence(&queue.scheduler.recurrence);
            let suppressed = state
                .db
                .get_queue_scheduler_suppression(queue.id)
                .ok()
                .flatten();
            let current_occurrence_is_suppressed = occurrence.is_some() && occurrence == suppressed;
            if queue.status == QueueStatus::Active
                && queue.scheduler.enabled
                && queue.scheduler.run_missed_on_startup
                && !current_occurrence_is_suppressed
            {
                if let Err(e) = start_eligible_downloads(&state, queue).await {
                    eprintln!(
                        "scheduler: startup catch-up failed for queue {}: {e}",
                        queue.id
                    );
                }
            }
        }
    }

    loop {
        tokio::time::sleep(TICK_INTERVAL).await;

        let queues = match state.db.list_queues() {
            Ok(q) => q,
            Err(e) => {
                eprintln!("scheduler: failed to list queues: {e}");
                continue;
            }
        };

        for queue in &queues {
            if let Err(e) = process_queue(&state, queue).await {
                eprintln!("scheduler: error processing queue {}: {e}", queue.id);
            }
        }
    }
}

async fn process_queue(state: &AppState, queue: &Queue) -> anyhow::Result<()> {
    let _activity = state.activity_guard().await?;
    if !queue.scheduler.enabled {
        return Ok(());
    }

    let occurrence = current_schedule_occurrence(&queue.scheduler.recurrence);
    match schedule_decision(&state.db, queue.id, &queue.status, occurrence.as_deref())? {
        ScheduleDecision::Start => start_eligible_downloads(state, queue).await,
        ScheduleDecision::StayPaused => pause_downloads(state, queue, false).await,
        ScheduleDecision::CloseWindow => pause_scheduled_downloads(state, queue).await,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScheduleDecision {
    Start,
    StayPaused,
    CloseWindow,
}

/// Resolves the queue's manual-run override, then advances the persisted
/// per-occurrence suppression state. A manually resumed queue stays active
/// until the user pauses it, regardless of its configured schedule. Otherwise,
/// a stale suppression key is cleared as soon as a different occurrence opens
/// (or the current window closes), so it cannot leak into a later scheduled day.
fn schedule_decision(
    db: &crate::db::Database,
    queue_id: i64,
    queue_status: &QueueStatus,
    occurrence: Option<&str>,
) -> anyhow::Result<ScheduleDecision> {
    if *queue_status == QueueStatus::Active {
        return Ok(ScheduleDecision::Start);
    }

    let suppressed = db.get_queue_scheduler_suppression(queue_id)?;
    match occurrence {
        Some(current) if suppressed.as_deref() == Some(current) => Ok(ScheduleDecision::StayPaused),
        Some(_) => {
            if suppressed.is_some() {
                db.set_queue_scheduler_suppression(queue_id, None)?;
            }
            Ok(ScheduleDecision::Start)
        }
        None => {
            if suppressed.is_some() {
                db.set_queue_scheduler_suppression(queue_id, None)?;
            }
            Ok(ScheduleDecision::CloseWindow)
        }
    }
}

/// Starts (or resumes) enough `Pending`/scheduler-paused downloads in
/// `queue` to fill up to `max_concurrent_downloads`, in `position_in_queue`
/// order — this is the app-level concurrency cap we enforce ourselves
/// (independent of aria2's own global settings), per the queue design.
pub async fn start_eligible_downloads(state: &AppState, queue: &Queue) -> anyhow::Result<()> {
    let active_count = state.db.count_active_downloads_in_queue(queue.id)?;
    let capacity = (queue.settings.max_concurrent_downloads as i64 - active_count).max(0);
    if capacity == 0 {
        return Ok(());
    }

    let candidates = state.db.list_startable_downloads(queue.id)?;

    for download in candidates.into_iter().take(capacity as usize) {
        match &download.aria2_gid {
            // Already known to aria2 (was scheduler-paused earlier) — unpause it.
            Some(gid) => {
                state.aria2.unpause(gid).await?;
                state
                    .db
                    .update_download_status(download.id, &DownloadStatus::Active)?;
                state.db.set_paused_by_scheduler(download.id, false)?;
            }
            // Never started — hand it to aria2 for the first time now.
            None => match state
                .aria2
                .add_uri(
                    &download.url,
                    &download.finetune,
                    &download.destination_path,
                    Aria2AddMode::Fresh,
                )
                .await
            {
                Ok(gid) => {
                    state.db.update_download_gid(download.id, &gid)?;
                    state
                        .db
                        .update_download_status(download.id, &DownloadStatus::Active)?;
                }
                Err(e) => {
                    state.db.update_download_status(
                        download.id,
                        &DownloadStatus::Error(e.to_string()),
                    )?;
                }
            },
        }
    }

    Ok(())
}

pub async fn pause_downloads(
    state: &AppState,
    queue: &Queue,
    paused_by_scheduler: bool,
) -> anyhow::Result<()> {
    let active = state.db.list_queue_controlled_active_downloads(queue.id)?;
    for download in active {
        if let Some(gid) = &download.aria2_gid {
            state.aria2.pause(gid).await?;
        }
        state
            .db
            .update_download_status(download.id, &DownloadStatus::Paused)?;
        state
            .db
            .set_paused_by_scheduler(download.id, paused_by_scheduler)?;
    }
    Ok(())
}

/// Pauses every currently-`Active` download in `queue`, marking each as
/// scheduler-paused so the next open window knows it's safe to auto-resume.
async fn pause_scheduled_downloads(state: &AppState, queue: &Queue) -> anyhow::Result<()> {
    pause_downloads(state, queue, true).await
}

/// Identifies the currently-open schedule occurrence. A manual queue pause
/// stores this key so only this occurrence is skipped; the next scheduled day
/// naturally has a different key and can start normally.
pub fn current_schedule_occurrence(recurrence: &Recurrence) -> Option<String> {
    match recurrence {
        Recurrence::Once { start, end } => {
            let now = Utc::now();
            (now >= *start && now <= *end).then(|| format!("once:{}", start.to_rfc3339()))
        }
        Recurrence::Weekly {
            days,
            start_time,
            end_time,
        } => {
            let now = Local::now();
            weekly_occurrence_date(days, *start_time, *end_time, now.date_naive(), now.time())
                .map(|date| format!("weekly:{date}:{start_time}"))
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
        (days.contains(&weekday) && time_now >= start && time_now <= end).then_some(today)
    } else {
        // Crosses midnight (e.g. 22:00-02:00): either today's late part, or
        // yesterday's window continuing into this morning.
        if days.contains(&weekday) && time_now >= start {
            Some(today)
        } else if days.contains(&weekday.pred()) && time_now <= end {
            today.pred_opt()
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).unwrap()
    }

    fn time(hour: u32, minute: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(hour, minute, 0).unwrap()
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
