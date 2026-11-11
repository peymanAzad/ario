use super::support::{Fixture, weekly};
use common::enums::{DownloadStatus, QueueStatus};

#[tokio::test]
async fn start_now_batches_respect_capacity_and_refill_paused_queues_after_restart() {
    for (scheduled, suppressed) in [(false, false), (true, false), (true, true)] {
        for capacity in [1, 2, 3] {
            let mut f = Fixture::new().await;
            let mut q = f.once().await;
            q.scheduler.enabled = scheduled;
            if suppressed {
                q.scheduler.recurrence = common::enums::Recurrence::Once {
                    start: f.now() - chrono::Duration::minutes(1),
                    end: f.now() + chrono::Duration::minutes(1),
                };
            }
            q.settings.max_concurrent_downloads = capacity;
            f.edit(&q).await;
            let saved = f.add(q.id, 2, false).await;
            if suppressed {
                f.post(&format!("/queues/{}/pause", q.id)).await;
            }
            // A second import must account for the first import's running item.
            let mut items = f.add(q.id, 1, true).await;
            items.extend(f.add(q.id, 4, true).await);
            assert_eq!(f.calls("aria2.addUri"), capacity as usize);
            for (index, item) in items.iter().enumerate() {
                assert_eq!(
                    item.download.status,
                    if index < capacity as usize {
                        DownloadStatus::Active
                    } else {
                        DownloadStatus::Pending
                    }
                );
                assert!(item.download.manually_started);
                assert_eq!(item.download.aria2_gid.is_some(), index < capacity as usize);
            }
            f.restart().await;
            f.tick().await;
            assert_eq!(f.calls("aria2.addUri"), capacity as usize);
            for (index, item) in items.iter().enumerate() {
                f.status(item.download.id, DownloadStatus::Active).await;
                f.transfer(item.download.id, "complete", 100).await;
                f.tick().await;
                assert_eq!(
                    f.calls("aria2.addUri"),
                    (capacity as usize + index + 1).min(items.len())
                );
                for item in &saved {
                    f.status(item.download.id, DownloadStatus::Pending).await;
                }
                assert_eq!(
                    f.state.db.get_queue(q.id).unwrap().unwrap().status,
                    QueueStatus::Paused
                );
            }
            f.capacity(capacity as usize);
            f.close().await;
        }
    }
}

#[tokio::test]
async fn start_now_batch_waits_for_existing_active_queue_items() {
    for capacity in [1, 2] {
        let f = Fixture::new().await;
        let mut q = f.queue(false, weekly(12, 13), false).await;
        q.settings.max_concurrent_downloads = capacity;
        f.edit(&q).await;
        let mut items = f.add(q.id, capacity as usize, false).await;
        f.post(&format!("/queues/{}/resume", q.id)).await;
        let imported = f.add(q.id, 4, true).await;
        assert_eq!(f.calls("aria2.addUri"), capacity as usize);
        for item in &imported {
            assert_eq!(item.download.status, DownloadStatus::Pending);
            assert!(item.download.aria2_gid.is_none());
        }
        items.extend(imported);
        for item in &items {
            f.status(item.download.id, DownloadStatus::Active).await;
            f.transfer(item.download.id, "complete", 100).await;
            f.tick().await;
        }
        f.capacity(capacity as usize);
        f.close().await;
    }
}

#[tokio::test]
async fn failed_start_now_item_does_not_block_the_remaining_batch() {
    use super::support::Fault;
    let f = Fixture::new().await;
    let q = f.queue(false, weekly(12, 13), false).await;
    f.fault("aria2.addUri", Fault::Rpc);
    let items = f.add(q.id, 3, true).await;
    assert!(matches!(items[0].download.status, DownloadStatus::Error(_)));
    assert!(!items[0].download.manually_started);
    assert_eq!(items[1].download.status, DownloadStatus::Active);
    assert_eq!(items[2].download.status, DownloadStatus::Pending);
    f.transfer(items[1].download.id, "complete", 100).await;
    f.tick().await;
    f.status(items[2].download.id, DownloadStatus::Active).await;
    f.capacity(1);
    f.close().await;
}

#[tokio::test]
async fn queue_pause_stops_start_now_items_whether_or_not_queue_was_started() {
    for scheduled in [false, true] {
        for started in [false, true] {
            let f = Fixture::new().await;
            let q = f.queue(scheduled, weekly(12, 13), false).await;
            let pending = f.add(q.id, 2, false).await;
            if started {
                f.post(&format!("/queues/{}/resume", q.id)).await;
            }
            let items = f.add(q.id, 3, true).await;
            let other = f.queue(false, weekly(12, 13), false).await;
            let unrelated = f.add(other.id, 1, true).await.remove(0);
            for (index, item) in items.iter().enumerate() {
                f.status(
                    item.download.id,
                    if !started && index == 0 {
                        DownloadStatus::Active
                    } else {
                        DownloadStatus::Pending
                    },
                )
                .await;
                assert!(f.get(item.download.id).await.download.manually_started);
            }
            assert_eq!(
                f.state.db.get_queue(q.id).unwrap().unwrap().status,
                if started {
                    QueueStatus::Active
                } else {
                    QueueStatus::Paused
                }
            );

            f.post(&format!("/queues/{}/pause", q.id)).await;
            f.tick().await;
            assert_eq!(f.get_queue(q.id).await.status, QueueStatus::Paused);
            for (index, item) in items.iter().enumerate() {
                f.status(
                    item.download.id,
                    if !started && index == 0 {
                        DownloadStatus::Paused
                    } else {
                        DownloadStatus::Pending
                    },
                )
                .await;
                assert!(!f.get(item.download.id).await.download.manually_started);
                assert!(
                    !f.state
                        .live_status
                        .read()
                        .await
                        .contains_key(&item.download.id)
                );
            }
            f.status(
                pending[0].download.id,
                if started {
                    DownloadStatus::Paused
                } else {
                    DownloadStatus::Pending
                },
            )
            .await;
            f.status(pending[1].download.id, DownloadStatus::Pending)
                .await;
            f.status(unrelated.download.id, DownloadStatus::Active)
                .await;
            let pauses = 1;
            assert_eq!(f.calls("aria2.pause"), pauses);

            // Repeated pauses are harmless; a later individual resume still overrides suppression.
            f.post(&format!("/queues/{}/pause", q.id)).await;
            assert_eq!(f.calls("aria2.pause"), pauses);
            f.post(&format!("/downloads/{}/resume", items[0].download.id))
                .await;
            f.tick().await;
            f.status(items[0].download.id, DownloadStatus::Active).await;
            f.status(items[1].download.id, DownloadStatus::Pending)
                .await;
            f.close().await;
        }
    }
}

#[tokio::test]
async fn download_manual_queue_advances_in_order_and_respects_capacity() {
    let f = Fixture::new().await;
    let q = f.queue(false, weekly(12, 13), false).await;
    let items = f.add(q.id, 3, false).await;
    f.post(&format!("/queues/{}/resume", q.id)).await;
    for item in &items {
        f.status(item.download.id, DownloadStatus::Active).await;
        f.transfer(item.download.id, "complete", 100).await;
        f.tick().await;
    }
    let urls: Vec<_> = f
        .mock
        .lock()
        .unwrap()
        .calls
        .iter()
        .filter(|c| c["method"] == "aria2.addUri")
        .map(|c| c["params"][1][0].as_str().unwrap().to_owned())
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
async fn queue_pause_prevents_refill_and_resume_advances_remaining_items() {
    let f = Fixture::new().await;
    let q = f.queue(false, weekly(12, 13), false).await;
    let items = f.add(q.id, 3, false).await;
    f.post(&format!("/queues/{}/resume", q.id)).await;
    f.post(&format!("/queues/{}/pause", q.id)).await;
    f.tick().await;
    f.status(items[0].download.id, DownloadStatus::Paused).await;
    f.status(items[1].download.id, DownloadStatus::Pending)
        .await;
    assert_eq!(f.calls("aria2.addUri"), 1);
    f.post(&format!("/queues/{}/resume", q.id)).await;
    for item in &items {
        f.status(item.download.id, DownloadStatus::Active).await;
        f.transfer(item.download.id, "complete", 100).await;
        f.tick().await;
    }
    f.capacity(1);
    f.close().await;
}

#[tokio::test]
async fn queue_reorder_move_and_restart_preserve_execution_order() {
    use serde_json::json;
    let mut f = Fixture::new().await;
    let q = f.queue(false, weekly(12, 13), false).await;
    let target = f.queue(false, weekly(12, 13), false).await;
    let items = f.add(q.id, 3, false).await;
    let ids: Vec<_> = items.iter().map(|i| i.download.id).collect();
    f.json(
        reqwest::Method::PUT,
        &format!("/queues/{}/reorder", q.id),
        Some(json!({"ordered_ids":[ids[2],ids[1],ids[0]]})),
    )
    .await;
    f.json(
        reqwest::Method::PUT,
        &format!("/downloads/{}/queue", ids[1]),
        Some(json!({"queue_id":target.id})),
    )
    .await;
    f.restart().await;
    assert_eq!(f.get(ids[1]).await.download.queue_id, target.id);
    f.post(&format!("/queues/{}/resume", q.id)).await;
    f.status(ids[2], DownloadStatus::Active).await;
    f.transfer(ids[2], "complete", 100).await;
    f.tick().await;
    f.status(ids[0], DownloadStatus::Active).await;
    f.status(ids[1], DownloadStatus::Pending).await;
    f.transfer(ids[0], "complete", 100).await;
    f.tick().await;
    f.post(&format!("/queues/{}/resume", target.id)).await;
    f.status(ids[1], DownloadStatus::Active).await;
    f.capacity(1);
    f.close().await;
}

#[tokio::test]
async fn queue_delete_confirmation_cleans_live_state_but_preserves_payloads() {
    let f = Fixture::new().await;
    let empty = f.queue(false, weekly(12, 13), false).await;
    f.json(
        reqwest::Method::DELETE,
        &format!("/queues/{}", empty.id),
        None,
    )
    .await;
    let q = f.queue(false, weekly(12, 13), false).await;
    let item = f.add(q.id, 1, true).await.remove(0);
    f.tick().await;
    assert!(
        f.state
            .live_status
            .read()
            .await
            .contains_key(&item.download.id)
    );
    assert_eq!(
        f.response(reqwest::Method::DELETE, &format!("/queues/{}", q.id), None)
            .await
            .status(),
        409
    );
    f.json(
        reqwest::Method::DELETE,
        &format!("/queues/{}?delete_downloads=true", q.id),
        None,
    )
    .await;
    assert!(f.state.db.get_download(item.download.id).unwrap().is_none());
    assert!(
        f.state
            .db
            .list_download_artifacts(item.download.id)
            .unwrap()
            .is_empty()
    );
    assert!(
        !f.state
            .live_status
            .read()
            .await
            .contains_key(&item.download.id)
    );
    assert!(f.root.join("downloads/file-0.bin").exists());
    assert_eq!(
        f.response(
            reqwest::Method::DELETE,
            "/queues/1?delete_downloads=true",
            None
        )
        .await
        .status(),
        400
    );
    assert!(f.state.db.get_queue(1).unwrap().is_some());
    f.close().await;
}
