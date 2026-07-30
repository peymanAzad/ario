use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use common::api::ApiResponse;

#[derive(Debug)]
pub enum AppError {
    NotFound(String),
    Database(rusqlite::Error),
    BadRequest(String),
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppError::NotFound(msg) => write!(f, "not found: {msg}"),
            AppError::Database(e) => write!(f, "database error: {e}"),
            AppError::BadRequest(msg) => write!(f, "bad request: {msg}"),
        }
    }
}

impl std::error::Error for AppError {}

impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        // rusqlite's own "row not found" case reads more naturally as a 404
        // than a generic 500 — everything else is a real DB-layer failure.
        match e {
            rusqlite::Error::QueryReturnedNoRows => AppError::NotFound("row not found".into()),
            other => AppError::Database(other),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AppError::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            AppError::Database(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        };

        let body: ApiResponse<()> = ApiResponse::Error { message };
        (status, Json(body)).into_response()
    }
}
