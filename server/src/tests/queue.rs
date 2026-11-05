use super::support::{Fixture, weekly};
use common::enums::{DownloadStatus, QueueStatus};

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
            for item in &items {
                f.status(item.download.id, DownloadStatus::Active).await;
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
            for item in &items {
                f.status(item.download.id, DownloadStatus::Paused).await;
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
            let pauses = 3 + usize::from(started);
            assert_eq!(f.calls("aria2.pause"), pauses);

            // Repeated pauses are harmless; a later individual resume still overrides suppression.
            f.post(&format!("/queues/{}/pause", q.id)).await;
            assert_eq!(f.calls("aria2.pause"), pauses);
            f.post(&format!("/downloads/{}/resume", items[0].download.id))
                .await;
            f.tick().await;
            f.status(items[0].download.id, DownloadStatus::Active).await;
            f.status(items[1].download.id, DownloadStatus::Paused).await;
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
