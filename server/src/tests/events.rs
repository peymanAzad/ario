//! Real queue lifecycle and shell execution; only aria2 is mocked.
use super::support::{Fault, Fixture, LIMIT, weekly};
use chrono::Duration;
use common::{
    enums::{DownloadStatus, QueueStatus, Recurrence},
    queue::{Queue, QueueEventStatus as Status},
};
use reqwest::Method;
use serde_json::json;

async fn queue(f: &Fixture, scheduled: bool, success: &str, stop: &str) -> Queue {
    let mut q = if scheduled {
        f.once().await
    } else {
        f.queue(false, weekly(12, 13), false).await
    };
    q.settings.on_success_command = (!success.is_empty()).then(|| success.into());
    q.settings.on_stop_command = (!stop.is_empty()).then(|| stop.into());
    f.state.db.update_queue(&q).unwrap();
    q
}

fn append(f: &Fixture, file: &str, value: &str) -> String {
    format!(
        "printf '%s\\n' '{value}' >> '{}'",
        f.root.join(file).display()
    )
}

async fn settled(f: &Fixture) {
    tokio::time::timeout(LIMIT, async {
        while f.state.db.event_command_count().unwrap() > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("event commands did not settle");
}

async fn exists(path: &std::path::Path) {
    tokio::time::timeout(LIMIT, async {
        while !path.exists() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("command did not reach barrier");
}

fn lines(f: &Fixture, file: &str) -> String {
    std::fs::read_to_string(f.root.join(file)).unwrap_or_default()
}

#[tokio::test]
async fn concurrent_out_of_order_completion_runs_both_once_with_literal_context() {
    let f = Fixture::new().await;
    let context = format!(
        "printf '%s\\n' \"$ARIO_QUEUE_ID|$ARIO_QUEUE_NAME|$ARIO_QUEUE_RUN_ID|$ARIO_EVENT|$ARIO_STOP_REASON\" > '{}'; printf 'event fixture stdout\\n'; printf 'event fixture stderr\\n' >&2; {}",
        f.root.join("context").display(),
        append(&f, "order", "success")
    );
    let mut q = queue(&f, false, &context, &append(&f, "order", "stop")).await;
    q.name = "literal $(false); 'name'".into();
    f.state.db.update_queue(&q).unwrap();
    let items = f.add(q.id, 2, true).await;
    // Last position finishes first; it must not count as queue completion.
    f.transfer(items[1].download.id, "complete", 100).await;
    f.tick().await;
    assert_eq!(f.state.db.event_command_count().unwrap(), 0);
    assert!(f.get_queue(q.id).await.event_results.on_success.is_none());
    f.transfer(items[0].download.id, "complete", 100).await;
    f.tick().await;
    settled(&f).await;
    assert_eq!(lines(&f, "order"), "success\nstop\n");
    assert_eq!(
        lines(&f, "context"),
        format!("{}|{}|1|success|completed\n", q.id, q.name)
    );
    let results = f.get_queue(q.id).await.event_results;
    let success = results.on_success.unwrap();
    assert_eq!(success.status, Status::Succeeded);
    assert_eq!(success.exit_code, Some(0));
    assert!(success.started_at.is_some() && success.finished_at.is_some());
    assert!(success.id < results.on_stop.unwrap().id);
    f.tick().await;
    settled(&f).await;
    assert_eq!(lines(&f, "order"), "success\nstop\n");
    f.close().await;
}

#[tokio::test]
async fn failed_and_manually_paused_items_exhaust_then_individual_retry_rearms() {
    let f = Fixture::new().await;
    let q = queue(
        &f,
        false,
        &append(&f, "success", "success"),
        &append(&f, "stop", "stop"),
    )
    .await;
    let items = f.add(q.id, 2, true).await;
    f.transfer(items[0].download.id, "error", 10).await;
    f.post(&format!("/downloads/{}/pause", items[1].download.id))
        .await;
    f.tick().await;
    settled(&f).await;
    assert_eq!(lines(&f, "stop"), "stop\n");
    assert_eq!(lines(&f, "success"), "");
    let first = f.get_queue(q.id).await.event_results.on_stop.unwrap();
    f.tick().await;
    assert_eq!(
        f.get_queue(q.id).await.event_results.on_stop.unwrap().id,
        first.id
    );
    f.post(&format!("/downloads/{}/resume", items[0].download.id))
        .await;
    f.transfer(items[0].download.id, "complete", 100).await;
    f.tick().await;
    settled(&f).await;
    assert_eq!(lines(&f, "stop"), "stop\nstop\n");
    assert_eq!(
        f.get_queue(q.id)
            .await
            .event_results
            .on_stop
            .unwrap()
            .run_id,
        first.run_id + 1
    );
    // The remaining individually paused item prevents success, then completing it allows success.
    f.post(&format!("/downloads/{}/resume", items[1].download.id))
        .await;
    f.transfer(items[1].download.id, "complete", 100).await;
    f.tick().await;
    settled(&f).await;
    assert_eq!(lines(&f, "success"), "success\n");
    f.close().await;
}

#[tokio::test]
async fn pending_refills_and_aria2_retries_prevent_exhaustion_and_new_work_rearms() {
    let f = Fixture::new().await;
    let q = queue(&f, false, "", &append(&f, "stop", "stop")).await;
    let items = f.add(q.id, 2, false).await;
    f.post(&format!("/queues/{}/resume", q.id)).await;
    for _ in 0..2 {
        f.tick().await;
    }
    assert!(f.get_queue(q.id).await.event_results.on_stop.is_none());
    f.transfer(items[0].download.id, "complete", 100).await;
    f.tick().await;
    assert!(f.get_queue(q.id).await.event_results.on_stop.is_none());
    f.status(items[1].download.id, DownloadStatus::Active).await;
    f.transfer(items[1].download.id, "complete", 100).await;
    f.tick().await;
    settled(&f).await;
    let new = f.add(q.id, 1, false).await.remove(0);
    f.tick().await;
    f.status(new.download.id, DownloadStatus::Active).await;
    f.transfer(new.download.id, "complete", 100).await;
    f.tick().await;
    settled(&f).await;
    assert_eq!(lines(&f, "stop"), "stop\nstop\n");
    assert_eq!(f.get_queue(q.id).await.status, QueueStatus::Active);
    f.close().await;
}

#[tokio::test]
async fn manual_queue_stop_cancels_events_even_when_a_pause_rpc_fails() {
    for fail in [false, true] {
        let f = Fixture::new().await;
        let q = queue(&f, true, "exit 7", "exit 8").await;
        f.add(q.id, 2, true).await;
        if fail {
            f.fault("aria2.pause", Fault::Rpc);
        }
        let response = f
            .response(Method::POST, &format!("/queues/{}/pause", q.id), None)
            .await;
        assert_eq!(response.status().is_success(), !fail);
        f.tick().await;
        f.set_time(f.now() + Duration::minutes(3));
        f.tick().await;
        assert_eq!(f.get_queue(q.id).await.event_results, Default::default());
        f.close().await;
    }
}

#[tokio::test]
async fn scheduled_stop_waits_for_successful_pauses_and_fires_once() {
    let f = Fixture::new().await;
    let command = format!(
        "printf '%s' \"$ARIO_STOP_REASON\" > '{}'",
        f.root.join("reason").display()
    );
    let q = queue(&f, true, "exit 7", &command).await;
    f.add(q.id, 2, false).await;
    f.set_time(f.now() + Duration::minutes(1));
    f.tick().await;
    f.set_time(f.now() + Duration::minutes(1));
    f.fault("aria2.pause", Fault::Rpc);
    f.tick().await;
    assert!(f.get_queue(q.id).await.event_results.on_stop.is_none());
    assert!(f.get_queue(q.id).await.scheduled_stop_at.is_some());
    f.tick().await;
    settled(&f).await;
    let result = f.get_queue(q.id).await.event_results.on_stop.unwrap();
    assert_eq!(lines(&f, "reason"), "scheduled_stop");
    f.tick().await;
    assert_eq!(
        f.get_queue(q.id).await.event_results.on_stop.unwrap().id,
        result.id
    );
    assert!(f.get_queue(q.id).await.event_results.on_success.is_none());
    f.close().await;
}

#[tokio::test]
async fn completion_at_deadline_wins_and_exhaustion_before_deadline_is_not_repeated() {
    for at_deadline in [false, true] {
        let f = Fixture::new().await;
        let stop = format!(
            "printf '%s\\n' \"$ARIO_STOP_REASON\" >> '{}'",
            f.root.join("reasons").display()
        );
        let q = queue(&f, true, &append(&f, "success", "success"), &stop).await;
        let item = f.add(q.id, 1, true).await.remove(0);
        if at_deadline {
            f.set_time(f.now() + Duration::minutes(2));
        }
        f.transfer(item.download.id, "complete", 100).await;
        f.tick().await;
        settled(&f).await;
        f.set_time(f.now() + Duration::minutes(3));
        f.tick().await;
        settled(&f).await;
        assert_eq!(lines(&f, "reasons"), "completed\n");
        assert_eq!(lines(&f, "success"), "success\n");
        f.close().await;
    }
}

#[tokio::test]
async fn run_survives_restart_and_empty_or_historical_queues_do_not_fire() {
    let mut f = Fixture::new().await;
    let q = queue(&f, false, "exit 7", &append(&f, "stop", "stop")).await;
    f.state
        .db
        .update_queue_status(q.id, QueueStatus::Active)
        .unwrap();
    f.tick().await; // active and empty
    let historic = f.add(q.id, 1, false).await.remove(0);
    f.state
        .db
        .update_download_status(historic.download.id, &DownloadStatus::Completed)
        .unwrap();
    f.tick().await;
    assert_eq!(f.get_queue(q.id).await.event_results, Default::default());
    let item = f.add(q.id, 1, false).await.remove(0);
    f.tick().await;
    f.restart().await;
    f.transfer(item.download.id, "complete", 100).await;
    f.tick().await;
    settled(&f).await;
    assert_eq!(lines(&f, "stop"), "stop\n");
    assert_eq!(
        f.get_queue(q.id)
            .await
            .event_results
            .on_success
            .unwrap()
            .status,
        Status::Failed
    );
    f.close().await;
}

#[tokio::test]
async fn queued_and_running_attempts_are_interrupted_without_replay_on_restart() {
    let mut f = Fixture::new().await;
    let q = queue(
        &f,
        false,
        &append(&f, "unexpected", "success"),
        &append(&f, "unexpected", "stop"),
    )
    .await;
    f.state.db.begin_event_run(q.id).unwrap();
    f.state
        .db
        .finish_event_run(&q, true, "completed", f.now())
        .unwrap();
    let claim = f.state.db.claim_next_event(q.id).unwrap().unwrap();
    assert_eq!(claim.result.status, Status::Running);
    f.restart().await;
    let results = f.get_queue(q.id).await.event_results;
    assert_eq!(results.on_success.unwrap().status, Status::Interrupted);
    assert_eq!(results.on_stop.unwrap().status, Status::Interrupted);
    crate::queue_events::dispatch(&f.state, q.id);
    f.tick().await;
    settled(&f).await;
    assert_eq!(lines(&f, "unexpected"), "");
    f.close().await;
}

#[tokio::test]
async fn command_snapshots_survive_edits_and_a_failed_success_does_not_skip_stop() {
    let f = Fixture::new().await;
    let mut q = queue(&f, false, "exit 7", &append(&f, "order", "old stop")).await;
    f.state.db.begin_event_run(q.id).unwrap();
    f.state
        .db
        .finish_event_run(&q, true, "completed", f.now())
        .unwrap();
    q.settings.on_stop_command = Some(append(&f, "order", "new stop"));
    f.state.db.update_queue(&q).unwrap();
    crate::queue_events::dispatch(&f.state, q.id);
    settled(&f).await;
    assert_eq!(lines(&f, "order"), "old stop\n");
    let results = f.get_queue(q.id).await.event_results;
    assert_eq!(results.on_success.unwrap().exit_code, Some(7));
    assert_eq!(results.on_stop.unwrap().status, Status::Succeeded);
    f.close().await;
}

#[tokio::test]
async fn slow_commands_serialize_across_runs_but_do_not_block_downloads_other_queues_or_api() {
    let f = Fixture::with_managed(true).await;
    let gate = f.root.join("release");
    let marker = f.root.join("started");
    let slow = format!(
        "printf started > '{}'; while [ ! -f '{}' ]; do sleep 0.01; done; {}",
        marker.display(),
        gate.display(),
        append(&f, "order", "success")
    );
    let q = queue(&f, false, &slow, &append(&f, "order", "stop")).await;
    for _ in 0..2 {
        let item = f.add(q.id, 1, true).await.remove(0);
        f.transfer(item.download.id, "complete", 100).await;
        f.tick().await;
    }
    exists(&marker).await;
    assert_eq!(f.state.db.event_command_count().unwrap(), 4);
    let result = f.json(Method::POST, "/shutdown-if-idle", None).await;
    assert_eq!(result["outcome"], "kept_running");
    assert_eq!(result["event_commands"], 4);
    let other = queue(&f, false, "", &append(&f, "other", "done")).await;
    let item = f.add(other.id, 1, true).await.remove(0);
    f.transfer(item.download.id, "complete", 100).await;
    f.tick().await;
    exists(&f.root.join("other")).await;
    assert_eq!(lines(&f, "order"), "");
    std::fs::write(gate, "").unwrap();
    settled(&f).await;
    assert_eq!(lines(&f, "order"), "success\nstop\nsuccess\nstop\n");
    assert_eq!(
        f.json(Method::POST, "/shutdown-if-idle", None).await["outcome"],
        "shutting_down"
    );
    f.close().await;
}

#[tokio::test]
async fn forced_shutdown_interrupts_running_and_queued_commands() {
    let f = Fixture::with_managed(true).await;
    let marker = f.root.join("started");
    let slow = format!("printf started > '{}'; sleep 300", marker.display());
    let q = queue(&f, false, &slow, &append(&f, "unexpected", "stop")).await;
    let item = f.add(q.id, 1, true).await.remove(0);
    f.transfer(item.download.id, "complete", 100).await;
    f.tick().await;
    exists(&marker).await;
    assert_eq!(
        f.json(Method::POST, "/shutdown", None).await["outcome"],
        "shutting_down"
    );
    settled(&f).await;
    tokio::time::timeout(LIMIT, async {
        while !f.state.event_workers.lock().unwrap().is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let result = f.get_queue(q.id).await.event_results;
    assert_eq!(result.on_success.unwrap().status, Status::Interrupted);
    assert_eq!(result.on_stop.unwrap().status, Status::Interrupted);
    assert_eq!(lines(&f, "unexpected"), "");
    f.close().await;
}

#[tokio::test]
async fn start_failures_exhaust_and_retained_removed_items_prevent_success() {
    for start_failure in [false, true] {
        let f = Fixture::new().await;
        let q = queue(
            &f,
            false,
            &append(&f, "success", "success"),
            &append(&f, "stop", "stop"),
        )
        .await;
        if start_failure {
            f.add(q.id, 1, false).await;
            f.fault("aria2.addUri", Fault::Rpc);
            f.post(&format!("/queues/{}/resume", q.id)).await;
        } else {
            let items = f.add(q.id, 2, true).await;
            f.transfer(items[0].download.id, "removed", 10).await;
            f.transfer(items[1].download.id, "complete", 100).await;
        }
        f.tick().await;
        settled(&f).await;
        assert_eq!(lines(&f, "stop"), "stop\n");
        assert_eq!(lines(&f, "success"), "");
        f.close().await;
    }
}

#[tokio::test]
async fn queue_api_preserves_omitted_commands_and_clears_explicit_blanks() {
    let mut f = Fixture::new().await;
    let request = json!({"name":"hooks","position":1,"max_concurrent_downloads":1,"max_retries":0,"retry_wait_seconds":1,
        "default_finetune":{},"scheduler_enabled":false,"recurrence":Recurrence::Once{start:f.now(),end:f.now()+Duration::hours(1)},
        "run_missed_on_startup":false,"on_success_command":"exit 0","on_stop_command":"exit 1"});
    let mut q: Queue =
        serde_json::from_value(f.json(Method::POST, "/queues", Some(request.clone())).await)
            .unwrap();
    assert_eq!(q.settings.on_success_command.as_deref(), Some("exit 0"));
    let mut old = request.clone();
    old.as_object_mut().unwrap().remove("on_success_command");
    old.as_object_mut().unwrap().remove("on_stop_command");
    q = serde_json::from_value(
        f.json(Method::PUT, &format!("/queues/{}", q.id), Some(old))
            .await,
    )
    .unwrap();
    assert_eq!(q.settings.on_stop_command.as_deref(), Some("exit 1"));
    let mut clear = request;
    clear["on_success_command"] = json!("   ");
    clear["on_stop_command"] = json!("");
    f.json(Method::PUT, &format!("/queues/{}", q.id), Some(clear))
        .await;
    f.restart().await;
    let q = f.get_queue(q.id).await;
    assert!(q.settings.on_success_command.is_none() && q.settings.on_stop_command.is_none());
    f.close().await;
}

#[tokio::test]
async fn commands_have_closed_stdin_no_controlling_terminal_and_the_config_directory() {
    let f = Fixture::new().await;
    let script = format!(
        "if read answer; then exit 51; fi; if ( : </dev/tty ) 2>/dev/null; then exit 52; fi; pwd > '{}'",
        f.root.join("cwd").display()
    );
    let q = queue(&f, false, &script, "").await;
    let item = f.add(q.id, 1, true).await.remove(0);
    f.transfer(item.download.id, "complete", 100).await;
    f.tick().await;
    settled(&f).await;
    assert_eq!(
        f.get_queue(q.id)
            .await
            .event_results
            .on_success
            .unwrap()
            .status,
        Status::Succeeded
    );
    // The shell can resolve a symlink in the temp directory, such as /var on macOS.
    let reported_cwd = std::fs::canonicalize(lines(&f, "cwd").trim()).unwrap();
    assert_eq!(reported_cwd, f.root.canonicalize().unwrap());
    f.close().await;
}

#[tokio::test]
async fn spawn_failures_are_persisted_and_do_not_strand_following_commands() {
    let mut f = Fixture::new().await;
    f.state.event_directory = Some(f.root.join("missing-directory"));
    // Rebind the router to the same state with its intentionally missing cwd.
    let config = (*f.state.config).clone();
    f.reconfigure(config).await;
    let q = queue(&f, false, "exit 0", "exit 0").await;
    let item = f.add(q.id, 1, true).await.remove(0);
    f.transfer(item.download.id, "complete", 100).await;
    f.tick().await;
    settled(&f).await;
    let results = f.get_queue(q.id).await.event_results;
    for result in [results.on_success.unwrap(), results.on_stop.unwrap()] {
        assert_eq!(result.status, Status::Failed);
        assert!(result.message.is_some());
        assert!(result.exit_code.is_none());
        assert!(result.finished_at.is_some());
    }
    f.close().await;
}
