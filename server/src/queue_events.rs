//! Queue events are claimed under queue control, then executed without holding it.
use crate::state::AppState;
use common::{
    download::DownloadFilter,
    enums::DownloadStatus,
    queue::{Queue, QueueEventResult, QueueEventStatus},
};
use std::{
    collections::HashSet,
    process::Stdio,
    sync::{Arc, Mutex},
};

pub type EventWorkers = Arc<Mutex<HashSet<i64>>>;

#[derive(Debug)]
pub struct EventAttempt {
    pub queue_id: i64,
    pub event: String,
    pub command: String,
    pub queue_name: String,
    pub stop_reason: String,
    pub result: QueueEventResult,
}

/// Called only after refills, or after every scheduled pause has succeeded.
pub fn reconcile(state: &AppState, queue: &Queue, scheduled_stop: bool) -> anyhow::Result<()> {
    let downloads = state.db.list_downloads(&DownloadFilter {
        queue_id: Some(queue.id),
        ..Default::default()
    })?;
    if downloads.is_empty() {
        state.db.cancel_event_run(queue.id)?;
        return Ok(());
    }
    let success = downloads
        .iter()
        .all(|item| item.status == DownloadStatus::Completed);
    let active = downloads
        .iter()
        .any(|item| item.status == DownloadStatus::Active);
    // Individual runs in a paused queue do not authorize pending queue work.
    let queue_running = crate::scheduler::effective_queue_status(&state.db, queue, state.now())?
        == common::enums::QueueStatus::Active;
    let eligible = queue_running
        && !state
            .db
            .list_startable_downloads(queue.id, false)?
            .is_empty();
    if scheduled_stop || (!active && !eligible) {
        let reason = if success {
            "completed"
        } else if scheduled_stop {
            "scheduled_stop"
        } else {
            "exhausted"
        };
        state
            .db
            .finish_event_run(queue, success, reason, state.now())?;
        dispatch(state, queue.id);
    }
    Ok(())
}

pub fn dispatch(state: &AppState, queue_id: i64) {
    if !state.event_workers.lock().unwrap().insert(queue_id) {
        return;
    }
    let state = state.clone();
    tokio::spawn(async move {
        loop {
            // Keep worker removal atomic with enqueue/dispatch, avoiding stranded commands.
            let attempt = {
                let mut workers = state.event_workers.lock().unwrap();
                match state.db.claim_next_event(queue_id) {
                    Ok(Some(attempt)) => Some(attempt),
                    result => {
                        workers.remove(&queue_id);
                        if let Err(error) = result {
                            eprintln!("queue {queue_id}: cannot claim event: {error}");
                        }
                        None
                    }
                }
            };
            let Some(attempt) = attempt else {
                return;
            };
            execute(&state, attempt).await;
        }
    });
}

async fn execute(state: &AppState, mut attempt: EventAttempt) {
    let mut cancel = state.event_cancel.subscribe();
    if *cancel.borrow() {
        return;
    }
    eprintln!(
        "queue {} run {}: starting {} event (attempt {}, reason {})",
        attempt.queue_id,
        attempt.result.run_id,
        attempt.event,
        attempt.result.id,
        attempt.stop_reason
    );
    let outcome = async {
        let directory = state
            .event_directory
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Could not determine Ario configuration directory"))?;
        let mut command = shell_command(&attempt.command);
        command
            .current_dir(directory)
            .env("ARIO_QUEUE_ID", attempt.queue_id.to_string())
            .env("ARIO_QUEUE_NAME", &attempt.queue_name)
            .env("ARIO_QUEUE_RUN_ID", attempt.result.run_id.to_string())
            .env("ARIO_EVENT", &attempt.event)
            .env("ARIO_STOP_REASON", &attempt.stop_reason)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .kill_on_drop(true);
        let mut child = command.spawn()?;
        let status = tokio::select! {
            status = child.wait() => status?,
            _ = cancel.changed() => {
                terminate_tree(&mut child).await?;
                anyhow::bail!("Daemon shutdown interrupted command");
            }
        };
        Ok::<_, anyhow::Error>(status)
    }
    .await;
    attempt.result.finished_at = Some(chrono::Utc::now());
    match outcome {
        Ok(status) => {
            attempt.result.exit_code = status.code();
            attempt.result.status = if status.success() {
                QueueEventStatus::Succeeded
            } else {
                QueueEventStatus::Failed
            };
            if !status.success() {
                attempt.result.message =
                    Some(format!("Command exited with {status}; see daemon logs"));
            }
        }
        Err(error) => {
            attempt.result.status = if *cancel.borrow() {
                QueueEventStatus::Interrupted
            } else {
                QueueEventStatus::Failed
            };
            attempt.result.message = Some(error.to_string());
        }
    }
    eprintln!(
        "queue {} run {}: {} event {:?} (attempt {}){}",
        attempt.queue_id,
        attempt.result.run_id,
        attempt.event,
        attempt.result.status,
        attempt.result.id,
        attempt
            .result
            .message
            .as_deref()
            .map(|message| format!(": {message}"))
            .unwrap_or_default()
    );
    if let Err(error) = state.db.complete_event(&attempt.result) {
        eprintln!("queue event: could not save result: {error}");
    }
}

async fn terminate_tree(child: &mut tokio::process::Child) -> std::io::Result<()> {
    if let Some(id) = child.id() {
        #[cfg(unix)]
        {
            // The shell created its own session/process group before exec.
            // SAFETY: kill takes integer arguments and accesses no Rust memory.
            let result = unsafe { libc::kill(-(id as i32), libc::SIGKILL) };
            if result == -1 {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() != Some(libc::ESRCH) {
                    return Err(error);
                }
            }
        }
        #[cfg(windows)]
        {
            let _ = tokio::process::Command::new("taskkill.exe")
                .args(["/PID", &id.to_string(), "/T", "/F"])
                .creation_flags(0x08000000)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await;
            let _ = child.start_kill();
        }
    }
    child.wait().await.map(|_| ())
}

fn shell_command(text: &str) -> tokio::process::Command {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let mut command = std::process::Command::new("/bin/sh");
        command.args(["-c", text]);
        // A new session removes access to the daemon's controlling terminal.
        // SAFETY: setsid is async-signal-safe and touches no Rust runtime state.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        command.into()
    }
    #[cfg(windows)]
    {
        let mut command = tokio::process::Command::new("cmd.exe");
        command
            .args(["/D", "/S", "/C"])
            .raw_arg(format!("\"{text}\""))
            .creation_flags(0x08000000); // CREATE_NO_WINDOW
        command
    }
}
