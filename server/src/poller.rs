use crate::db::{DownloadArtifact, DownloadArtifactKind};
use crate::routing::{candidates_from_aria_files, route_for_candidates};
use crate::state::AppState;
use common::{
    download::{Download, DownloadFilter},
    enums::{DownloadStatus, SourceType},
};
use std::time::Duration;

const POLL_INTERVAL: Duration = Duration::from_secs(2);

pub async fn run(state: AppState) {
    loop {
        let Ok(_activity) = state.activity_guard().await else {
            return;
        };

        let downloads = match state.db.list_downloads(&DownloadFilter::default()) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("poller: failed to list downloads: {e}");
                drop(_activity);
                tokio::time::sleep(POLL_INTERVAL).await;
                continue;
            }
        };

        for snapshot in downloads {
            let _control = state.control.lock().await;
            let Ok(Some(download)) = state.db.get_download(snapshot.id) else {
                continue;
            };
            let Some(gid) = download.aria2_gid.clone() else {
                continue; // never started in aria2 — nothing to poll
            };

            if matches!(
                download.status,
                DownloadStatus::Completed | DownloadStatus::Error(_) | DownloadStatus::Removed
            ) {
                state.live_status.write().await.remove(&download.id);
                continue;
            }

            match state.aria2.tell_status(&gid).await {
                Ok(status) => {
                    if download.source_type == SourceType::Magnet && !status.followed_by.is_empty()
                    {
                        match adopt_magnet_child(&state, &download, &gid, &status.followed_by).await
                        {
                            Ok(()) => {}
                            Err(error) => eprintln!(
                                "poller: failed to prepare magnet payload for download {}: {error}",
                                download.id
                            ),
                        }
                        continue;
                    }

                    let (payloads, controls) = status.artifact_paths();
                    if !payloads.is_empty() {
                        let artifacts: Vec<DownloadArtifact> = payloads
                            .into_iter()
                            .map(|path| DownloadArtifact {
                                path,
                                kind: DownloadArtifactKind::Payload,
                            })
                            .chain(controls.into_iter().map(|path| DownloadArtifact {
                                path,
                                kind: DownloadArtifactKind::Control,
                            }))
                            .collect();
                        if let Err(error) =
                            state.db.replace_download_artifacts(download.id, &artifacts)
                        {
                            eprintln!(
                                "poller: failed to store artifacts for download {}: {error}",
                                download.id
                            );
                        }
                    }
                    let completed_length = crate::live_status::coalesce_completed_length(
                        status.completed_length.parse().unwrap_or(0),
                        download.completed_length,
                    );
                    let total_length: u64 = status.total_length.parse().unwrap_or(0);
                    let download_speed: u64 = status.download_speed.parse().unwrap_or(0);

                    if download.completed_length != Some(completed_length)
                        && let Err(error) = state
                            .db
                            .update_download_completed_length(download.id, completed_length)
                    {
                        eprintln!(
                            "poller: failed to persist completed_length for download {}: {error}",
                            download.id
                        );
                    }

                    let candidates = candidates_from_aria_files(&status.files);
                    if !candidates.is_empty()
                        && let Ok(route) = route_for_candidates(&state.config.settings, &candidates)
                    {
                        let size = (total_length > 0).then_some(total_length);
                        let _ = state.db.update_download_resolved_info(
                            download.id,
                            &route.filename,
                            size,
                            &route.category,
                        );
                    }

                    reconcile_transfer_state(
                        &state,
                        &download,
                        &status.status,
                        completed_length,
                        download_speed,
                        status.error_message.as_deref(),
                    )
                    .await;
                }
                Err(e) => {
                    eprintln!(
                        "poller: tellStatus failed for download {} (gid {gid}): {e}",
                        download.id
                    );
                }
            }
        }
        {
            let _control = state.control.lock().await;
            match state.db.list_queues() {
                Ok(queues) => {
                    for queue in queues {
                        if let Err(error) =
                            crate::scheduler::reconcile_queue(&state, queue.id, state.now()).await
                        {
                            eprintln!("queue {}: reconciliation failed: {error}", queue.id);
                        }
                    }
                }
                Err(error) => eprintln!("poller: failed to list queues: {error}"),
            }
        }
        drop(_activity);
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

async fn adopt_magnet_child(
    state: &AppState,
    download: &Download,
    parent_gid: &str,
    followed_by: &[String],
) -> Result<(), String> {
    let child_gid = followed_by
        .first()
        .ok_or_else(|| "metadata result did not contain a child GID".to_string())?;
    for unexpected_gid in followed_by.iter().skip(1) {
        let _ = state.aria2.remove(unexpected_gid).await;
        let _ = state.aria2.remove_download_result(unexpected_gid).await;
    }

    let child = state
        .aria2
        .tell_status(child_gid)
        .await
        .map_err(|error| error.to_string())?;
    let candidates = candidates_from_aria_files(&child.files);
    if candidates.is_empty() {
        return Err("resolved magnet has no payload files yet".into());
    }
    let route = route_for_candidates(&state.config.settings, &candidates)
        .map_err(|error| error.to_string())?;
    let completed_length = child.completed_length.parse::<u64>().unwrap_or(0);
    let total_length = child.total_length.parse::<u64>().unwrap_or(0);
    let can_route = child.status == "paused" && completed_length == 0;
    let destination_path = if can_route {
        if let Err(error) = state
            .aria2
            .change_dir(child_gid, &route.destination_path)
            .await
        {
            let _ = state.db.update_download_gid(download.id, child_gid);
            let _ = state.db.update_download_status(
                download.id,
                &DownloadStatus::Error(format!("failed to route magnet payload: {error}")),
            );
            let _ = state.aria2.remove_download_result(parent_gid).await;
            return Ok(());
        }
        route.destination_path.clone()
    } else if child.dir.is_empty() {
        download.destination_path.clone()
    } else {
        child.dir.clone()
    };

    state
        .db
        .adopt_resolved_download(
            download.id,
            child_gid,
            &route.filename,
            (total_length > 0).then_some(total_length),
            &route.category,
            &destination_path,
        )
        .map_err(|error| error.to_string())?;
    let _ = state.aria2.remove_download_result(parent_gid).await;

    if download.status == DownloadStatus::Active
        && child.status == "paused"
        && let Err(error) = state.aria2.unpause(child_gid).await
    {
        let _ = state.db.update_download_status(
            download.id,
            &DownloadStatus::Error(format!("failed to start magnet payload: {error}")),
        );
    }
    Ok(())
}

pub(crate) async fn reconcile_transfer_state(
    state: &AppState,
    download: &Download,
    aria2_status: &str,
    completed_length: u64,
    download_speed: u64,
    error_message: Option<&str>,
) {
    // Use current intent; the RPC snapshot may predate a serialized control operation.
    let Ok(Some(current)) = state.db.get_download(download.id) else {
        return;
    };
    if matches!(
        current.status,
        DownloadStatus::Completed | DownloadStatus::Error(_) | DownloadStatus::Removed
    ) {
        return;
    }
    match aria2_status {
        "active" => {
            // Re-read the persisted intent because this tellStatus request may have
            // started before a concurrent pause request completed.
            let persisted_status = state
                .db
                .get_download(download.id)
                .ok()
                .flatten()
                .map(|current| current.status)
                .unwrap_or_else(|| download.status.clone());
            if persisted_status == DownloadStatus::Paused {
                state.live_status.write().await.remove(&download.id);
                return;
            }

            state.live_status.write().await.insert(
                download.id,
                crate::live_status::LiveStats {
                    completed_length,
                    download_speed,
                },
            );
            if persisted_status != DownloadStatus::Active {
                let _ = state
                    .db
                    .update_download_status(download.id, &DownloadStatus::Active);
            }
        }
        "paused" => {
            let persisted_status = state
                .db
                .get_download(download.id)
                .ok()
                .flatten()
                .map(|current| current.status)
                .unwrap_or_else(|| download.status.clone());
            if persisted_status == DownloadStatus::Active
                && download.status == DownloadStatus::Paused
            {
                return;
            }
            let _ = state
                .db
                .update_download_status(download.id, &DownloadStatus::Paused);
            state.live_status.write().await.remove(&download.id);
        }
        "complete" => {
            let _ = state
                .db
                .update_download_status(download.id, &DownloadStatus::Completed);
            let _ = state.db.set_completed_at_now(download.id);
            let _ = state.db.set_manually_started(download.id, false);
            state.live_status.write().await.remove(&download.id);
        }
        "error" => {
            let message = error_message
                .unwrap_or("aria2 reported an error")
                .to_string();
            let _ = state
                .db
                .update_download_status(download.id, &DownloadStatus::Error(message));
            let _ = state.db.set_manually_started(download.id, false);
            state.live_status.write().await.remove(&download.id);
        }
        "removed" => {
            let _ = state
                .db
                .update_download_status(download.id, &DownloadStatus::Removed);
            let _ = state.db.set_manually_started(download.id, false);
            state.live_status.write().await.remove(&download.id);
        }
        // waiting downloads are not currently transferring, so do not retain a
        // stale speed sample for them.
        _ => {
            state.live_status.write().await.remove(&download.id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{aria2::Aria2Client, config::ServerConfig, db::Database};
    use chrono::Utc;
    use common::{
        enums::{FileCategory, SourceType},
        finetune::FineTune,
    };

    fn state() -> AppState {
        AppState::new(
            Database::open(":memory:").unwrap(),
            Aria2Client::new("http://127.0.0.1:1/jsonrpc", None),
            ServerConfig::default(),
            false,
        )
    }

    fn insert_download(state: &AppState, status: DownloadStatus) -> Download {
        let mut download = Download {
            id: 0,
            aria2_gid: Some("gid".into()),
            url: "https://example.test/file.bin".into(),
            filename: Some("file.bin".into()),
            destination_path: "/tmp".into(),
            source_type: SourceType::Http,
            category: FileCategory::Other,
            status,
            paused_by_scheduler: false,
            manually_started: false,
            size: Some(100),
            completed_length: Some(10),
            queue_id: 1,
            position_in_queue: 0,
            finetune: FineTune::default(),
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
        };
        download.id = state.db.insert_download(&download).unwrap();
        download
    }

    #[tokio::test]
    async fn late_active_result_does_not_resurrect_a_paused_download() {
        let state = state();
        let mut stale_snapshot = insert_download(&state, DownloadStatus::Active);
        state
            .db
            .update_download_status(stale_snapshot.id, &DownloadStatus::Paused)
            .unwrap();
        stale_snapshot.status = DownloadStatus::Active;
        state.live_status.write().await.insert(
            stale_snapshot.id,
            crate::live_status::LiveStats {
                completed_length: 10,
                download_speed: 50,
            },
        );

        reconcile_transfer_state(&state, &stale_snapshot, "active", 20, 75, None).await;

        assert_eq!(
            state
                .db
                .get_download(stale_snapshot.id)
                .unwrap()
                .unwrap()
                .status,
            DownloadStatus::Paused
        );
        assert!(
            !state
                .live_status
                .read()
                .await
                .contains_key(&stale_snapshot.id)
        );
    }

    #[tokio::test]
    async fn paused_result_reconciles_status_and_clears_live_speed() {
        let state = state();
        let download = insert_download(&state, DownloadStatus::Active);
        state.live_status.write().await.insert(
            download.id,
            crate::live_status::LiveStats {
                completed_length: 10,
                download_speed: 50,
            },
        );

        reconcile_transfer_state(&state, &download, "paused", 20, 0, None).await;

        assert_eq!(
            state.db.get_download(download.id).unwrap().unwrap().status,
            DownloadStatus::Paused
        );
        assert!(!state.live_status.read().await.contains_key(&download.id));
    }

    #[tokio::test]
    async fn late_paused_result_does_not_undo_a_resume() {
        let state = state();
        let mut stale_snapshot = insert_download(&state, DownloadStatus::Paused);
        state
            .db
            .update_download_status(stale_snapshot.id, &DownloadStatus::Active)
            .unwrap();
        stale_snapshot.status = DownloadStatus::Paused;

        reconcile_transfer_state(&state, &stale_snapshot, "paused", 20, 0, None).await;

        assert_eq!(
            state
                .db
                .get_download(stale_snapshot.id)
                .unwrap()
                .unwrap()
                .status,
            DownloadStatus::Active
        );
    }

    #[tokio::test]
    async fn active_result_updates_status_and_live_metrics() {
        let state = state();
        let download = insert_download(&state, DownloadStatus::Pending);

        reconcile_transfer_state(&state, &download, "active", 30, 80, None).await;

        assert_eq!(
            state.db.get_download(download.id).unwrap().unwrap().status,
            DownloadStatus::Active
        );
        let live = state.live_status.read().await[&download.id];
        assert_eq!(live.completed_length, 30);
        assert_eq!(live.download_speed, 80);
    }

    #[tokio::test]
    async fn terminal_results_clear_live_metrics() {
        for aria2_status in ["complete", "error", "removed"] {
            let state = state();
            let download = insert_download(&state, DownloadStatus::Active);
            state.live_status.write().await.insert(
                download.id,
                crate::live_status::LiveStats {
                    completed_length: 10,
                    download_speed: 50,
                },
            );

            reconcile_transfer_state(&state, &download, aria2_status, 20, 0, Some("failure")).await;

            assert!(!state.live_status.read().await.contains_key(&download.id));
        }
    }
}
