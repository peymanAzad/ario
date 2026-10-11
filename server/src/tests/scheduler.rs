use super::support::{Barrier, Fault, Fixture, LIMIT, local, weekly};
use chrono::{Datelike, Duration, Local, NaiveTime};
use common::{
    download::{AddDownloadInput, AddDownloadsRequest},
    enums::{DownloadStatus, QueueStatus, Recurrence},
    finetune::FineTune,
    queue::CreateQueueRequest,
};
use serde_json::json;

#[tokio::test]
async fn scheduler_start_stop_boundaries_and_refill() {
    let f = Fixture::new().await;
    let q = f.once().await;
    let items = f.add(q.id, 3, false).await;
    let start = f.now() + Duration::minutes(1);
    let end = start + Duration::minutes(1);
    f.set_time(start - Duration::nanoseconds(1));
    f.tick().await;
    assert_eq!(f.calls("aria2.addUri"), 0);
    f.set_time(start);
    f.tick().await;
    f.tick().await;
    assert_eq!(f.calls("aria2.addUri"), 1);
    f.status(items[0].download.id, DownloadStatus::Active).await;
    f.transfer(items[0].download.id, "complete", 100).await;
    f.tick().await;
    f.status(items[1].download.id, DownloadStatus::Active).await;
    assert_eq!(f.calls("aria2.addUri"), 2);
    f.set_time(end);
    f.tick().await;
    f.tick().await;
    f.status(items[1].download.id, DownloadStatus::Paused).await;
    f.status(items[2].download.id, DownloadStatus::Pending)
        .await;
    assert_eq!(f.get(items[1].download.id).await.download_speed, 0);
    assert!(
        f.get(items[1].download.id)
            .await
            .download
            .paused_by_scheduler
    );
    assert_eq!(f.calls("aria2.addUri"), 2);
    assert_eq!(f.calls("aria2.pause"), 1);
    f.capacity(1);
    f.close().await;
}

#[tokio::test]
async fn scheduler_weekly_next_window_resumes_all_items_in_order() {
    let f = Fixture::new().await;
    f.set_time(local(7, 11, 59));
    assert_eq!(
        f.now().with_timezone(&Local).weekday(),
        chrono::Weekday::Mon
    );
    let mut q = f.queue(true, weekly(12, 13), false).await;
    q.settings.max_concurrent_downloads = 3;
    f.edit(&q).await;
    let items = f.add(q.id, 3, false).await;
    f.set_time(local(7, 12, 0));
    f.tick().await;
    f.set_time(local(7, 13, 0));
    f.tick().await;
    for item in &items {
        f.status(item.download.id, DownloadStatus::Paused).await;
        assert!(f.get(item.download.id).await.download.paused_by_scheduler);
    }
    q.settings.max_concurrent_downloads = 1;
    f.edit(&q).await;
    f.mock.lock().unwrap().max_active = 0;
    f.set_time(local(8, 12, 0));
    f.tick().await;
    assert_eq!(f.calls("aria2.unpause"), 0);
    f.set_time(local(14, 12, 0));
    f.tick().await;
    assert_eq!(f.calls("aria2.unpause"), 1);
    for item in &items {
        f.status(item.download.id, DownloadStatus::Active).await;
        f.transfer(item.download.id, "complete", 100).await;
        f.tick().await;
    }
    assert_eq!(f.calls("aria2.unpause"), 3);
    let urls: Vec<_> = f
        .mock
        .lock()
        .unwrap()
        .calls
        .iter()
        .filter(|c| c["method"] == "aria2.addUri")
        .map(|c| c["params"][1][0].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        urls,
        items
            .iter()
            .map(|d| d.download.url.clone())
            .collect::<Vec<_>>()
    );
    f.capacity(1);
    f.close().await;
}

#[tokio::test]
async fn scheduler_manual_pause_survives_restart_until_next_occurrence() {
    let mut f = Fixture::new().await;
    f.set_time(local(7, 12, 0));
    let q = f.queue(true, weekly(12, 13), false).await;
    let items = f.add(q.id, 3, false).await;
    f.tick().await;
    f.post(&format!("/queues/{}/pause", q.id)).await;
    let marker = f.state.db.get_queue_scheduler_suppression(q.id).unwrap();
    assert!(marker.is_some());
    f.transfer(items[0].download.id, "complete", 100).await;
    f.tick().await;
    f.restart().await;
    f.tick().await;
    assert_eq!(
        f.state.db.get_queue_scheduler_suppression(q.id).unwrap(),
        marker
    );
    assert_eq!(f.calls("aria2.addUri"), 1);
    f.status(items[1].download.id, DownloadStatus::Pending)
        .await;
    f.set_time(local(14, 12, 0));
    f.tick().await;
    f.status(items[1].download.id, DownloadStatus::Active).await;
    f.capacity(1);
    f.close().await;
}

#[tokio::test]
async fn scheduler_restart_open_window_keeps_active_gid_and_refills() {
    let mut f = Fixture::new().await;
    let q = f.once().await;
    let items = f.add(q.id, 2, false).await;
    f.set_time(f.now() + Duration::minutes(1));
    f.tick().await;
    f.transfer(items[0].download.id, "active", 40).await;
    f.tick().await;
    let before = f.get(items[0].download.id).await;
    f.restart().await;
    assert_eq!(f.get(items[0].download.id).await.completed_length, 40);
    f.tick().await;
    assert_eq!(
        f.get(items[0].download.id).await.download.aria2_gid,
        before.download.aria2_gid
    );
    assert_eq!(f.calls("aria2.addUri"), 1);
    f.transfer(items[0].download.id, "complete", 100).await;
    f.tick().await;
    f.status(items[1].download.id, DownloadStatus::Active).await;
    f.capacity(1);
    f.close().await;
}

#[tokio::test]
async fn scheduler_restart_catchup_only_open_windows_for_both_flags() {
    for flag in [false, true] {
        for open in [false, true] {
            let mut f = Fixture::new().await;
            let t = f.now();
            let q = f
                .queue(
                    true,
                    Recurrence::Once {
                        start: t - Duration::hours(2),
                        end: if open {
                            t + Duration::hours(1)
                        } else {
                            t - Duration::hours(1)
                        },
                    },
                    flag,
                )
                .await;
            let item = f.add(q.id, 1, false).await.remove(0);
            f.restart().await;
            f.tick().await;
            f.status(
                item.download.id,
                if open {
                    DownloadStatus::Active
                } else {
                    DownloadStatus::Pending
                },
            )
            .await;
            assert_eq!(f.calls("aria2.addUri"), usize::from(open));
            f.close().await;
        }
    }
}

#[tokio::test]
async fn scheduler_overdue_deadline_stops_individual_before_refill() {
    let mut f = Fixture::new().await;
    let q = f.once().await;
    let items = f.add(q.id, 2, true).await;
    assert_eq!(f.calls("aria2.addUri"), 2); // Individual starts deliberately bypass capacity.
    let pending = f.add(q.id, 1, false).await.remove(0);
    f.state
        .db
        .set_scheduled_stop(q.id, Some(f.now() - Duration::seconds(1)))
        .unwrap();
    f.restart().await;
    f.tick().await;
    for item in items {
        let d = f.get(item.download.id).await;
        assert_eq!(d.download.status, DownloadStatus::Paused);
        assert!(d.download.paused_by_scheduler);
        assert!(!d.download.manually_started);
    }
    f.status(pending.download.id, DownloadStatus::Pending).await;
    assert_eq!(f.calls("aria2.addUri"), 2);
    assert_eq!(f.get_queue(q.id).await.status, QueueStatus::Paused);
    f.close().await;
}

#[tokio::test]
async fn scheduler_schedule_edits_recalculate_and_clear_control_markers() {
    let f = Fixture::new().await;
    let mut q = f.once().await;
    let item = f.add(q.id, 1, true).await.remove(0);
    let now = f.now();
    for seconds in [180, 90] {
        q.scheduler.recurrence = Recurrence::Once {
            start: now - Duration::seconds(60),
            end: now + Duration::seconds(seconds),
        };
        let updated = f.edit(&q).await;
        assert_eq!(
            updated.scheduled_stop_at,
            Some(now + Duration::seconds(seconds))
        );
    }
    f.post(&format!("/queues/{}/pause", q.id)).await;
    assert!(
        f.state
            .db
            .get_queue_scheduler_suppression(q.id)
            .unwrap()
            .is_some()
    );
    q.scheduler.recurrence = Recurrence::Once {
        start: now - Duration::seconds(30),
        end: now + Duration::seconds(120),
    };
    f.edit(&q).await;
    assert!(
        f.state
            .db
            .get_queue_scheduler_suppression(q.id)
            .unwrap()
            .is_none()
    );
    f.status(item.download.id, DownloadStatus::Active).await;
    q.scheduler.enabled = false;
    let updated = f.edit(&q).await;
    assert!(updated.scheduled_stop_at.is_none());
    assert!(
        f.state
            .db
            .get_queue_started_occurrence(q.id)
            .unwrap()
            .is_none()
    );
    q.scheduler.enabled = true;
    q.scheduler.recurrence = Recurrence::Once {
        start: now - Duration::minutes(2),
        end: now - Duration::minutes(1),
    };
    f.edit(&q).await;
    f.status(item.download.id, DownloadStatus::Paused).await;
    assert_eq!(f.get(item.download.id).await.download_speed, 0);
    f.close().await;
}

#[tokio::test]
async fn scheduler_invalid_and_expired_schedules_reject_all_start_paths() {
    let f = Fixture::new().await;
    let t = f.now();
    for recurrence in [
        Recurrence::Once { start: t, end: t },
        Recurrence::Weekly {
            days: vec![],
            start_time: NaiveTime::from_hms_opt(12, 0, 0).unwrap(),
            end_time: NaiveTime::from_hms_opt(13, 0, 0).unwrap(),
        },
    ] {
        let req = CreateQueueRequest {
            name: "invalid".into(),
            position: 1,
            max_concurrent_downloads: 1,
            max_retries: 0,
            retry_wait_seconds: 1,
            default_finetune: FineTune::default(),
            scheduler_enabled: true,
            recurrence,
            run_missed_on_startup: false,
        };
        assert_eq!(
            f.response(reqwest::Method::POST, "/queues", Some(json!(req)))
                .await
                .status(),
            400
        );
    }
    assert_eq!(f.state.db.list_queues().unwrap().len(), 1);
    let q = f
        .queue(
            true,
            Recurrence::Once {
                start: t - Duration::hours(2),
                end: t - Duration::hours(1),
            },
            false,
        )
        .await;
    let item = f.add(q.id, 1, false).await.remove(0);
    for path in [
        format!("/queues/{}/resume", q.id),
        format!("/downloads/{}/resume", item.download.id),
    ] {
        let r = f.response(reqwest::Method::POST, &path, None).await;
        assert_eq!(r.status(), 400);
        assert!(r.text().await.unwrap().contains("schedule has ended"));
    }
    let req = AddDownloadsRequest {
        inputs: vec![AddDownloadInput::Url(
            "http://fixture.invalid/rejected.bin".into(),
        )],
        queue_id: q.id,
        finetune_override: None,
        start_immediately: true,
    };
    assert_eq!(
        f.response(reqwest::Method::POST, "/downloads", Some(json!(req)))
            .await
            .status(),
        400
    );
    assert_eq!(
        f.state
            .db
            .list_downloads(&Default::default())
            .unwrap()
            .len(),
        1
    );
    assert_eq!(f.calls("aria2.addUri"), 0);
    f.status(item.download.id, DownloadStatus::Pending).await;
    f.close().await;
}

#[tokio::test]
async fn scheduler_start_failure_does_not_strand_capacity() {
    for fault in [Fault::Rpc, Fault::Transport] {
        let f = Fixture::new().await;
        let q = f.once().await;
        let items = f.add(q.id, 3, false).await;
        f.fault("aria2.addUri", fault);
        f.set_time(f.now() + Duration::minutes(1));
        f.tick().await;
        assert!(matches!(
            f.get(items[0].download.id).await.download.status,
            DownloadStatus::Error(_)
        ));
        f.status(items[1].download.id, DownloadStatus::Active).await;
        f.status(items[2].download.id, DownloadStatus::Pending)
            .await;
        f.capacity(1);
        f.close().await;
    }
}

#[tokio::test]
async fn scheduler_pause_failure_retries_and_lost_response_preserves_resume_intent() {
    for fault in [Fault::Rpc, Fault::LostPause] {
        let f = Fixture::new().await;
        f.set_time(local(7, 12, 0));
        let q = f.queue(true, weekly(12, 13), false).await;
        let items = f.add(q.id, 2, false).await;
        f.tick().await;
        f.fault("aria2.pause", fault);
        f.set_time(local(7, 13, 0));
        f.tick().await;
        assert_eq!(f.get_queue(q.id).await.status, QueueStatus::Paused);
        assert!(
            f.state
                .db
                .get_queue(q.id)
                .unwrap()
                .unwrap()
                .scheduled_stop_at
                .is_some()
        );
        assert!(
            f.get(items[0].download.id)
                .await
                .download
                .paused_by_scheduler
        );
        f.tick().await;
        f.status(items[0].download.id, DownloadStatus::Paused).await;
        assert_eq!(f.calls("aria2.addUri"), 1);
        f.status(items[1].download.id, DownloadStatus::Pending)
            .await;
        f.set_time(local(14, 12, 0));
        f.tick().await;
        f.status(items[0].download.id, DownloadStatus::Active).await;
        assert_eq!(f.calls("aria2.unpause"), 1);
        f.capacity(1);
        f.close().await;
    }
}

#[tokio::test]
async fn scheduler_overnight_window_obeys_weekday_and_exact_end() {
    let f = Fixture::new().await;
    f.set_time(local(7, 21, 59));
    let q = f.queue(true, weekly(22, 2), false).await;
    let item = f.add(q.id, 1, false).await.remove(0);
    f.tick().await;
    assert_eq!(f.calls("aria2.addUri"), 0);
    f.set_time(local(7, 22, 0));
    f.tick().await;
    f.status(item.download.id, DownloadStatus::Active).await;
    f.set_time(local(8, 1, 59));
    f.tick().await;
    f.status(item.download.id, DownloadStatus::Active).await;
    f.set_time(local(8, 2, 0));
    f.tick().await;
    f.status(item.download.id, DownloadStatus::Paused).await;
    f.set_time(local(8, 22, 0));
    f.tick().await;
    assert_eq!(f.calls("aria2.unpause"), 0);
    f.set_time(local(14, 22, 0));
    f.tick().await;
    assert_eq!(f.calls("aria2.unpause"), 1);
    f.close().await;
}

#[tokio::test]
async fn scheduler_concurrent_tick_resume_and_pause_preserve_control_state() {
    let f = Fixture::new().await;
    let q = f.once().await;
    let items = f.add(q.id, 3, false).await;
    f.set_time(f.now() + Duration::minutes(1));
    let barrier = Barrier::new();
    f.fault("aria2.addUri", Fault::Delay(barrier.clone()));
    let state = f.state.clone();
    let tick = tokio::spawn(async move {
        let _guard = state.activity_guard().await.unwrap();
        crate::poller::poll_once(&state).await;
    });
    barrier.entered().await;
    let http = f.http.clone();
    let url = format!("{}/queues/{}/resume", f.base, q.id);
    let resume = tokio::spawn(async move { http.post(url).send().await.unwrap() });
    barrier.release.notify_one();
    tokio::time::timeout(LIMIT, tick).await.unwrap().unwrap();
    assert!(
        tokio::time::timeout(LIMIT, resume)
            .await
            .unwrap()
            .unwrap()
            .status()
            .is_success()
    );
    assert_eq!(f.calls("aria2.addUri"), 1);
    f.capacity(1);
    // Complete the active item and hold its status response while pause competes with refill.
    f.transfer(items[0].download.id, "complete", 100).await;
    let barrier = Barrier::new();
    f.fault("aria2.tellStatus", Fault::Delay(barrier.clone()));
    let state = f.state.clone();
    let tick = tokio::spawn(async move {
        let _guard = state.activity_guard().await.unwrap();
        crate::poller::poll_once(&state).await;
    });
    barrier.entered().await;
    let http = f.http.clone();
    let url = format!("{}/queues/{}/pause", f.base, q.id);
    let pause = tokio::spawn(async move { http.post(url).send().await.unwrap() });
    barrier.release.notify_one();
    tokio::time::timeout(LIMIT, tick).await.unwrap().unwrap();
    assert!(
        tokio::time::timeout(LIMIT, pause)
            .await
            .unwrap()
            .unwrap()
            .status()
            .is_success()
    );
    f.tick().await;
    assert_eq!(f.get_queue(q.id).await.status, QueueStatus::Paused);
    assert!(
        f.state
            .db
            .get_queue_scheduler_suppression(q.id)
            .unwrap()
            .is_some()
    );
    for item in &items[1..] {
        assert_ne!(
            f.get(item.download.id).await.download.status,
            DownloadStatus::Active
        );
    }
    let gids: Vec<_> = f
        .state
        .db
        .list_downloads(&Default::default())
        .unwrap()
        .iter()
        .filter_map(|d| d.aria2_gid.clone())
        .collect();
    let unique: std::collections::HashSet<_> = gids.iter().collect();
    assert_eq!(unique.len(), gids.len());
    f.capacity(1);
    f.close().await;
}

#[tokio::test]
async fn scheduler_deadline_crossed_during_refill_prevents_second_start() {
    let f = Fixture::new().await;
    let q = f.once().await;
    let mut q = f.get_queue(q.id).await;
    q.settings.max_concurrent_downloads = 2;
    f.edit(&q).await;
    let items = f.add(q.id, 3, false).await;
    f.set_time(f.now() + Duration::minutes(1));
    let barrier = Barrier::new();
    f.fault("aria2.addUri", Fault::Delay(barrier.clone()));
    let state = f.state.clone();
    let tick = tokio::spawn(async move {
        let _guard = state.activity_guard().await.unwrap();
        crate::poller::poll_once(&state).await;
    });
    barrier.entered().await;
    f.set_time(f.now() + Duration::minutes(1));
    barrier.release.notify_one();
    tokio::time::timeout(LIMIT, tick).await.unwrap().unwrap();
    f.tick().await;
    assert_eq!(f.calls("aria2.addUri"), 1);
    f.status(items[0].download.id, DownloadStatus::Paused).await;
    f.status(items[1].download.id, DownloadStatus::Pending)
        .await;
    f.close().await;
}
