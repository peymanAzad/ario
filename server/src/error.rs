use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use common::api::ApiResponse;

use crate::aria2::Aria2Error;

#[derive(Debug)]
pub enum AppError {
    NotFound(String),
    Database(rusqlite::Error),
    BadRequest(String),
    Aria2(Aria2Error),
    Internal(String),
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppError::NotFound(msg) => write!(f, "not found: {msg}"),
            AppError::Database(e) => write!(f, "database error: {e}"),
            AppError::BadRequest(msg) => write!(f, "bad request: {msg}"),
            AppError::Aria2(e) => write!(f, "aria2 error: {e}"),
            AppError::Internal(msg) => write!(f, "internal error: {msg}"),
        }
    }
}

impl std::error::Error for AppError {}

impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        match e {
            rusqlite::Error::QueryReturnedNoRows => AppError::NotFound("row not found".into()),
            other => AppError::Database(other),
        }
    }
}

impl From<Aria2Error> for AppError {
    fn from(e: Aria2Error) -> Self {
        AppError::Aria2(e)
    }
}

impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        AppError::Internal(e.to_string())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AppError::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            AppError::Database(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
            AppError::Aria2(e) => (StatusCode::BAD_GATEWAY, e.to_string()),
            AppError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
        };

        let body: ApiResponse<()> = ApiResponse::Error { message };
        (status, Json(body)).into_response()
    }
}
