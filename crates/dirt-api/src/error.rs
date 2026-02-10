use axum::http::header::{HeaderName, HeaderValue};
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
    #[error("Too many requests: {0}")]
    TooManyRequests(String, u64),
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

    pub fn too_many_requests(message: impl Into<String>, retry_after_secs: u64) -> Self {
        Self::TooManyRequests(message.into(), retry_after_secs)
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
            Self::TooManyRequests(_, _) => StatusCode::TOO_MANY_REQUESTS,
            Self::External(_) => StatusCode::BAD_GATEWAY,
            Self::Config(_) | Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let body = ErrorBody {
            error: self.to_string(),
        };
        let mut response = (status, Json(body)).into_response();
        if let Self::TooManyRequests(_, retry_after_secs) = self {
            let header_name = HeaderName::from_static("retry-after");
            if let Ok(value) = HeaderValue::from_str(&retry_after_secs.to_string()) {
                response.headers_mut().insert(header_name, value);
            }
        }
        response
    }
}
