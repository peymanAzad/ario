use axum::{
    Json, Router,
    extract::State,
    routing::{get, post},
};
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
}

async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    let aria2_reachable = state.aria2.get_version().await.is_ok();
    let download_speed = crate::live_status::total_download_speed(&*state.live_status.read().await);
    Json(HealthResponse {
        server: "ok",
        aria2_reachable,
        tui_managed: state.tui_managed,
        download_speed,
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
