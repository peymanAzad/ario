use axum::{
    Json, Router,
    extract::State,
    routing::{get, post},
};
use common::finetune::Aria2GlobalOptions;
use common::lifecycle::ShutdownIfIdleResponse;
use common::settings::Settings;
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
    #[serde(skip_serializing_if = "Option::is_none")]
    aria2_global_options: Option<Aria2GlobalOptions>,
}

async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    let aria2_reachable = state.aria2.get_version().await.is_ok();
    let download_speed = crate::live_status::total_download_speed(&*state.live_status.read().await);
    Json(HealthResponse {
        server: "ok",
        aria2_reachable,
        tui_managed: state.tui_managed,
        download_speed,
        aria2_global_options: state.aria2_global_options.clone(),
    })
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
    use crate::{aria2::Aria2Client, config::ServerConfig, db::Database};
    use common::finetune::Aria2GlobalOptions;

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

        let Json(response) = health(State(state)).await;

        assert_eq!(response.aria2_global_options, Some(options));
    }
}
