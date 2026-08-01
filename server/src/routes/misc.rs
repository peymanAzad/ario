use axum::{Json, Router, extract::State, routing::get};
use common::settings::Settings;
use serde::Serialize;

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/settings", get(get_settings))
        .route("/health", get(health))
}

async fn get_settings(State(state): State<AppState>) -> Json<Settings> {
    Json(state.config.settings.clone())
}

#[derive(Serialize)]
struct HealthResponse {
    server: &'static str,
    aria2_reachable: bool,
}

async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    let aria2_reachable = state.aria2.get_version().await.is_ok();
    Json(HealthResponse {
        server: "ok",
        aria2_reachable,
    })
}
