use super::support::{Fixture, weekly};
use common::enums::DownloadStatus;

#[tokio::test]
async fn download_successful_lifecycle_reports_metadata_progress_and_artifacts() {
    let f = Fixture::new().await;
    let item = f.add(1, 1, true).await.remove(0);
    let id = item.download.id;
    assert_eq!(item.download.status, DownloadStatus::Active);
    assert!(item.download.started_at.is_some());
    assert_eq!(item.download.filename.as_deref(), Some("file-0.bin"));
    assert_eq!(
        item.download.destination_path,
        f.root.join("downloads").to_str().unwrap()
    );
    f.transfer(id, "active", 40).await;
    f.tick().await;
    let progress = f.get(id).await;
    assert_eq!(progress.completed_length, 40);
    assert_eq!(progress.download.size, Some(100));
    assert_eq!(progress.download_speed, 10);
    assert_eq!(progress.eta_seconds, Some(6));
    assert_eq!(f.state.db.list_download_artifacts(id).unwrap().len(), 2);
    f.transfer(id, "complete", 100).await;
    f.tick().await;
    let done = f.get(id).await;
    assert_eq!(done.download.status, DownloadStatus::Completed);
    assert_eq!(done.completed_length, 100);
    assert_eq!(done.download_speed, 0);
    assert!(done.download.completed_at.is_some());
    assert_eq!(done.download.aria2_gid, item.download.aria2_gid);
    let path = f.root.join("downloads/file-0.bin");
    assert_eq!(std::fs::read(path).unwrap(), b"synthetic payload");
    f.close().await;
}

#[tokio::test]
async fn download_deferred_start_pause_resume_keeps_gid_and_progress() {
    let f = Fixture::new().await;
    let item = f.add(1, 1, false).await.remove(0);
    let id = item.download.id;
    assert_eq!(f.calls("aria2.addUri"), 0);
    assert!(!f.root.join("downloads/file-0.bin").exists());
    f.post(&format!("/downloads/{id}/resume")).await;
    let gid = f.get(id).await.download.aria2_gid;
    f.transfer(id, "active", 40).await;
    f.tick().await;
    f.post(&format!("/downloads/{id}/pause")).await;
    f.tick().await;
    let paused = f.get(id).await;
    assert_eq!(paused.download.status, DownloadStatus::Paused);
    assert_eq!(paused.completed_length, 40);
    assert_eq!(paused.download_speed, 0);
    f.tick().await;
    assert_eq!(f.get(id).await.completed_length, 40);
    f.post(&format!("/downloads/{id}/resume")).await;
    assert_eq!(f.get(id).await.download.aria2_gid, gid);
    assert_eq!(f.calls("aria2.addUri"), 1);
    assert_eq!(f.calls("aria2.unpause"), 1);
    f.transfer(id, "complete", 100).await;
    f.tick().await;
    f.status(id, DownloadStatus::Completed).await;
    f.close().await;
}

#[tokio::test]
async fn download_restart_preserves_pending_paused_active_completed_and_artifacts() {
    let mut f = Fixture::new().await;
    let pending = f.add(1, 1, false).await.remove(0);
    let paused = f.add(1, 1, true).await.remove(0);
    let active = f.add(1, 1, true).await.remove(0);
    let completed = f.add(1, 1, true).await.remove(0);
    f.transfer(paused.download.id, "active", 30).await;
    f.transfer(active.download.id, "active", 40).await;
    f.transfer(completed.download.id, "complete", 100).await;
    f.tick().await;
    f.post(&format!("/downloads/{}/pause", paused.download.id))
        .await;
    f.tick().await;
    let ids = [
        pending.download.id,
        paused.download.id,
        active.download.id,
        completed.download.id,
    ];
    let mut before = Vec::new();
    for id in ids {
        before.push(f.get(id).await);
    }
    f.restart().await;
    for old in &before {
        let new = f.get(old.download.id).await;
        assert_eq!(new.download.aria2_gid, old.download.aria2_gid);
        assert_eq!(new.download.status, old.download.status);
        assert_eq!(new.download.queue_id, old.download.queue_id);
        assert_eq!(new.completed_length, old.completed_length);
        assert_eq!(new.download.completed_at, old.download.completed_at);
        if new.download.aria2_gid.is_some() {
            assert_eq!(
                f.state
                    .db
                    .list_download_artifacts(new.download.id)
                    .unwrap()
                    .len(),
                2
            );
        }
    }
    f.tick().await;
    assert_eq!(f.calls("aria2.addUri"), 3);
    f.status(pending.download.id, DownloadStatus::Pending).await;
    f.status(paused.download.id, DownloadStatus::Paused).await;
    f.status(active.download.id, DownloadStatus::Active).await;
    f.status(completed.download.id, DownloadStatus::Completed)
        .await;
    f.close().await;
}

#[tokio::test]
async fn download_deletion_keeps_or_removes_only_owned_files() {
    let f = Fixture::new().await;
    let items = f.add(1, 2, true).await;
    for item in &items {
        f.transfer(item.download.id, "complete", 100).await;
    }
    f.tick().await;
    let unrelated = f.root.join("downloads/unrelated.bin");
    std::fs::write(&unrelated, b"keep").unwrap();
    f.json(
        reqwest::Method::DELETE,
        &format!("/downloads/{}", items[0].download.id),
        None,
    )
    .await;
    assert!(f.root.join("downloads/file-0.bin").exists());
    let result = f
        .json(
            reqwest::Method::DELETE,
            &format!("/downloads/{}?delete_files=true", items[1].download.id),
            None,
        )
        .await;
    assert_eq!(result["removed_payloads"], 1);
    assert_eq!(result["metadata_complete"], true);
    assert!(!f.root.join("downloads/file-1.bin").exists());
    assert!(!f.root.join("downloads/file-1.bin.aria2").exists());
    assert!(unrelated.exists());
    assert!(
        f.state
            .db
            .list_downloads(&Default::default())
            .unwrap()
            .is_empty()
    );
    let saved = f.add(1, 1, false).await.remove(0);
    let collision = f.root.join("downloads/file-0.bin");
    assert!(collision.exists());
    f.json(
        reqwest::Method::DELETE,
        &format!("/downloads/{}?delete_files=true", saved.download.id),
        None,
    )
    .await;
    assert!(collision.exists());
    f.close().await;
}

#[tokio::test]
async fn download_transfer_error_advances_queue_and_retry_replaces_gid() {
    let f = Fixture::new().await;
    let q = f.queue(false, weekly(12, 13), false).await;
    let items = f.add(q.id, 2, false).await;
    f.post(&format!("/queues/{}/resume", q.id)).await;
    let old = f.get(items[0].download.id).await.download.aria2_gid;
    f.transfer(items[0].download.id, "error", 20).await;
    f.tick().await;
    let failed = f.get(items[0].download.id).await;
    assert_eq!(
        failed.download.status,
        DownloadStatus::Error("fixture transfer failure".into())
    );
    assert_eq!(failed.download_speed, 0);
    f.status(items[1].download.id, DownloadStatus::Active).await;
    f.transfer(items[1].download.id, "complete", 100).await;
    f.tick().await;
    f.post(&format!("/downloads/{}/resume", items[0].download.id))
        .await;
    assert_ne!(f.get(items[0].download.id).await.download.aria2_gid, old);
    assert_eq!(f.calls("aria2.removeDownloadResult"), 1);
    f.transfer(items[0].download.id, "complete", 100).await;
    f.tick().await;
    f.status(items[0].download.id, DownloadStatus::Completed)
        .await;
    f.capacity(1);
    f.close().await;
}

#[tokio::test]
async fn concurrent_resume_delete_and_polling_cannot_resurrect_record() {
    use super::support::{Barrier, Fault, LIMIT};
    let f = Fixture::new().await;
    let item = f.add(1, 1, false).await.remove(0);
    let id = item.download.id;
    let barrier = Barrier::new();
    f.fault("aria2.addUri", Fault::Delay(barrier.clone()));
    let http = f.http.clone();
    let base = f.base.clone();
    let resume = tokio::spawn(async move {
        http.post(format!("{base}/downloads/{id}/resume"))
            .send()
            .await
            .unwrap()
    });
    barrier.entered().await;
    let http = f.http.clone();
    let base = f.base.clone();
    let delete = tokio::spawn(async move {
        http.delete(format!("{base}/downloads/{id}"))
            .send()
            .await
            .unwrap()
    });
    let state = f.state.clone();
    let poll = tokio::spawn(async move {
        let _guard = state.activity_guard().await.unwrap();
        crate::poller::poll_once(&state).await;
    });
    barrier.release.notify_one();
    assert!(
        tokio::time::timeout(LIMIT, resume)
            .await
            .unwrap()
            .unwrap()
            .status()
            .is_success()
    );
    assert!(
        tokio::time::timeout(LIMIT, delete)
            .await
            .unwrap()
            .unwrap()
            .status()
            .is_success()
    );
    tokio::time::timeout(LIMIT, poll).await.unwrap().unwrap();
    f.tick().await;
    assert!(f.state.db.get_download(id).unwrap().is_none());
    assert!(!f.state.live_status.read().await.contains_key(&id));
    assert_eq!(f.calls("aria2.addUri"), 1);
    f.close().await;
}

#[tokio::test]
async fn list_filters_and_scoped_completed_cleanup_preserve_other_work_and_files() {
    use serde_json::json;
    let f = Fixture::new().await;
    let q = f.queue(false, weekly(12, 13), false).await;
    let done = f.add(q.id, 1, true).await.remove(0);
    let other = f.add(1, 1, true).await.remove(0);
    let pending = f.add(q.id, 1, false).await.remove(0);
    for id in [done.download.id, other.download.id] {
        f.transfer(id, "complete", 100).await;
    }
    f.tick().await;
    let result = f
        .json(
            reqwest::Method::GET,
            &format!("/downloads?queue_id={}&status=Completed", q.id),
            None,
        )
        .await;
    assert_eq!(result.as_array().unwrap().len(), 1);
    assert_eq!(result[0]["download"]["id"], json!(done.download.id));
    f.json(
        reqwest::Method::DELETE,
        &format!("/downloads/completed?queue_id={}", q.id),
        None,
    )
    .await;
    assert!(f.state.db.get_download(done.download.id).unwrap().is_none());
    f.status(other.download.id, DownloadStatus::Completed).await;
    f.status(pending.download.id, DownloadStatus::Pending).await;
    assert!(f.root.join("downloads/file-0.bin").exists());
    f.close().await;
}
