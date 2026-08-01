use crate::state::AppState;
use common::{download::DownloadFilter, enums::DownloadStatus, enums::FileCategory};
use std::time::Duration;

const POLL_INTERVAL: Duration = Duration::from_secs(2);

pub async fn run(state: AppState) {
    loop {
        tokio::time::sleep(POLL_INTERVAL).await;

        let downloads = match state.db.list_downloads(&DownloadFilter::default()) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("poller: failed to list downloads: {e}");
                continue;
            }
        };

        for download in downloads {
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
                    let completed_length: u64 = status.completed_length.parse().unwrap_or(0);
                    let total_length: u64 = status.total_length.parse().unwrap_or(0);
                    let download_speed: u64 = status.download_speed.parse().unwrap_or(0);

                    state.live_status.write().await.insert(
                        download.id,
                        crate::live_status::LiveStats {
                            completed_length,
                            download_speed,
                        },
                    );

                    if let Some(file) = status.files.first() {
                        if !file.path.is_empty() {
                            let filename = file
                                .path
                                .rsplit('/')
                                .next()
                                .unwrap_or(&file.path)
                                .to_string();
                            let category = FileCategory::infer_from_filename(
                                &filename,
                                &state.config.settings.category_extensions,
                            );
                            let size = if total_length > 0 {
                                Some(total_length)
                            } else {
                                None
                            };
                            let _ = state.db.update_download_resolved_info(
                                download.id,
                                &filename,
                                size,
                                &category,
                            );
                        }
                    }

                    match status.status.as_str() {
                        "complete" => {
                            let _ = state
                                .db
                                .update_download_status(download.id, &DownloadStatus::Completed);
                            let _ = state.db.set_completed_at_now(download.id);
                            state.live_status.write().await.remove(&download.id);
                        }
                        "error" => {
                            let msg = status
                                .error_message
                                .unwrap_or_else(|| "aria2 reported an error".to_string());
                            let _ = state
                                .db
                                .update_download_status(download.id, &DownloadStatus::Error(msg));
                            state.live_status.write().await.remove(&download.id);
                        }
                        "removed" => {
                            let _ = state
                                .db
                                .update_download_status(download.id, &DownloadStatus::Removed);
                            state.live_status.write().await.remove(&download.id);
                        }
                        "active" if download.status != DownloadStatus::Active => {
                            let _ = state
                                .db
                                .update_download_status(download.id, &DownloadStatus::Active);
                        }
                        _ => {}
                    }
                }
                Err(e) => {
                    eprintln!(
                        "poller: tellStatus failed for download {} (gid {gid}): {e}",
                        download.id
                    );
                }
            }
        }
    }
}
