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

use crate::state::AppState;
use chrono::{Datelike, Local, NaiveTime, Utc, Weekday};
use common::{enums::DownloadStatus, enums::Recurrence, queue::Queue};
use std::time::Duration;

const TICK_INTERVAL: Duration = Duration::from_secs(60);

/// Runs forever — spawn as its own tokio task from `main`.
pub async fn run(state: AppState) {
    // One-time startup catch-up pass — see module doc comment's "KNOWN
    // SIMPLIFICATION" note.
    if let Ok(queues) = state.db.list_queues() {
        for queue in &queues {
            if queue.scheduler.enabled && queue.scheduler.run_missed_on_startup {
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
            if !queue.scheduler.enabled {
                continue;
            }

            let result = if is_within_window(&queue.scheduler.recurrence) {
                start_eligible_downloads(&state, queue).await
            } else {
                pause_scheduled_downloads(&state, queue).await
            };

            if let Err(e) = result {
                eprintln!("scheduler: error processing queue {}: {e}", queue.id);
            }
        }
    }
}

/// Starts (or resumes) enough `Pending`/scheduler-paused downloads in
/// `queue` to fill up to `max_concurrent_downloads`, in `position_in_queue`
/// order — this is the app-level concurrency cap we enforce ourselves
/// (independent of aria2's own global settings), per the queue design.
async fn start_eligible_downloads(state: &AppState, queue: &Queue) -> anyhow::Result<()> {
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

/// Pauses every currently-`Active` download in `queue`, marking each as
/// scheduler-paused so the next open window knows it's safe to auto-resume.
async fn pause_scheduled_downloads(state: &AppState, queue: &Queue) -> anyhow::Result<()> {
    let active = state.db.list_active_downloads_in_queue(queue.id)?;
    for download in active {
        if let Some(gid) = &download.aria2_gid {
            state.aria2.pause(gid).await?;
        }
        state
            .db
            .update_download_status(download.id, &DownloadStatus::Paused)?;
        state.db.set_paused_by_scheduler(download.id, true)?;
    }
    Ok(())
}

/// See module doc comment: `Weekly` uses local time, `Once` uses UTC instant.
pub fn is_within_window(recurrence: &Recurrence) -> bool {
    match recurrence {
        Recurrence::Once { start, end } => {
            let now = Utc::now();
            now >= *start && now <= *end
        }
        Recurrence::Weekly {
            days,
            start_time,
            end_time,
        } => is_within_weekly_window(days, *start_time, *end_time),
    }
}

fn is_within_weekly_window(days: &[Weekday], start: NaiveTime, end: NaiveTime) -> bool {
    let now = Local::now();
    let today = now.weekday();
    let time_now = now.time();

    if start <= end {
        // Same-day window, no midnight crossing.
        days.contains(&today) && time_now >= start && time_now <= end
    } else {
        // Crosses midnight (e.g. 22:00-02:00): either today's late part, or
        // yesterday's window continuing into this morning.
        let late_part = days.contains(&today) && time_now >= start;
        let early_part = days.contains(&today.pred()) && time_now <= end;
        late_part || early_part
    }
}
