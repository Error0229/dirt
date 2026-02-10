use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Invalid request: {0}")]
    BadRequest(String),
    #[error("Unauthorized: {0}")]
    Unauthorized(String),
    #[error("Configuration error: {0}")]
    Config(String),
    #[error("External dependency error: {0}")]
    External(String),
    #[error("Internal server error: {0}")]
    Internal(String),
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: String,
}

impl AppError {
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::BadRequest(message.into())
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::Unauthorized(message.into())
    }

    pub fn external(message: impl Into<String>) -> Self {
        Self::External(message.into())
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match self {
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            Self::External(_) => StatusCode::BAD_GATEWAY,
            Self::Config(_) | Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let body = ErrorBody {
            error: self.to_string(),
        };
        (status, Json(body)).into_response()
    }
}
