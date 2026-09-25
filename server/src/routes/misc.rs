use axum::{
    Json, Router,
    extract::State,
    routing::{get, post},
};
use common::finetune::Aria2GlobalOptions;
use common::lifecycle::ShutdownIfIdleResponse;
use common::settings::Settings;
use common::{download::DownloadFilter, enums::DownloadStatus};
use serde::Serialize;

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/settings", get(get_settings))
        .route("/health", get(health))
        .route("/shutdown-if-idle", post(shutdown_if_idle))
        .route("/shutdown", post(shutdown))
}

async fn get_settings(State(state): State<AppState>) -> Json<Settings> {
    Json(state.config.settings.clone())
}

#[derive(Serialize)]
struct HealthResponse {
    server: &'static str,
    aria2_reachable: bool,
    tui_managed: bool,
    download_speed: u64,
    active_downloads: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    aria2_global_options: Option<Aria2GlobalOptions>,
}

async fn health(
    State(state): State<AppState>,
) -> Result<Json<HealthResponse>, crate::error::AppError> {
    let aria2_reachable = state.aria2.get_version().await.is_ok();
    let active_downloads = state.db.list_downloads(&DownloadFilter {
        status: Some(DownloadStatus::Active),
        ..DownloadFilter::default()
    })?;
    let download_speed = crate::live_status::total_download_speed(
        &*state.live_status.read().await,
        active_downloads.iter().map(|download| download.id),
    );
    Ok(Json(HealthResponse {
        server: "ok",
        aria2_reachable,
        tui_managed: state.tui_managed,
        download_speed,
        active_downloads: active_downloads.len() as u64,
        aria2_global_options: state.aria2_global_options.clone(),
    }))
}

async fn shutdown_if_idle(
    State(state): State<AppState>,
) -> Result<Json<ShutdownIfIdleResponse>, crate::error::AppError> {
    Ok(Json(state.request_idle_shutdown().await?))
}

async fn shutdown(
    State(state): State<AppState>,
) -> Result<Json<ShutdownIfIdleResponse>, crate::error::AppError> {
    Ok(Json(state.request_shutdown().await?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{aria2::Aria2Client, config::ServerConfig, db::Database, live_status::LiveStats};
    use chrono::Utc;
    use common::finetune::Aria2GlobalOptions;
    use common::{
        download::Download,
        enums::{FileCategory, SourceType},
        finetune::FineTune,
    };

    #[tokio::test]
    async fn health_returns_the_cached_global_options_snapshot() {
        let options = Aria2GlobalOptions {
            connections_per_download: Some(5),
            max_connections_per_server: Some(1),
            ..Aria2GlobalOptions::default()
        };
        let state = AppState::new(
            Database::open(":memory:").unwrap(),
            Aria2Client::new("http://127.0.0.1:1/jsonrpc", None),
            ServerConfig::default(),
            false,
        )
        .with_aria2_global_options(Some(options.clone()));

        let Json(response) = health(State(state)).await.unwrap();

        assert_eq!(response.aria2_global_options, Some(options));
        assert_eq!(response.active_downloads, 0);
    }

    #[tokio::test]
    async fn health_excludes_stale_live_speed_for_paused_downloads() {
        let state = AppState::new(
            Database::open(":memory:").unwrap(),
            Aria2Client::new("http://127.0.0.1:1/jsonrpc", None),
            ServerConfig::default(),
            false,
        );
        let mut download = Download {
            id: 0,
            aria2_gid: Some("gid".into()),
            url: "https://example.test/file.bin".into(),
            filename: Some("file.bin".into()),
            destination_path: "/tmp".into(),
            source_type: SourceType::Http,
            category: FileCategory::Other,
            status: DownloadStatus::Paused,
            paused_by_scheduler: false,
            manually_started: false,
            size: Some(100),
            completed_length: Some(20),
            queue_id: 1,
            position_in_queue: 0,
            finetune: FineTune::default(),
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
        };
        download.id = state.db.insert_download(&download).unwrap();
        state.live_status.write().await.insert(
            download.id,
            LiveStats {
                completed_length: 20,
                download_speed: 50,
            },
        );

        let Json(response) = health(State(state)).await.unwrap();

        assert_eq!(response.active_downloads, 0);
        assert_eq!(response.download_speed, 0);
    }
}
