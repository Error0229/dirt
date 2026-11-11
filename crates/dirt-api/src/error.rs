//! The one error type every handler returns.
//!
//! A client acts on the code alone (see `dirt_proto::api::ErrorCode`), so a
//! message here is for logs and developers. Database errors are logged in
//! full and reported to the client only as a server error.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use dirt_proto::api::{ErrorBody, ErrorCode};

#[derive(Debug)]
pub struct ApiError {
    pub code: ErrorCode,
    pub message: String,
    pub retry_after_secs: Option<u32>,
}

pub type ApiResult<T> = Result<T, ApiError>;

impl ApiError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            retry_after_secs: None,
        }
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidRequest, message)
    }

    #[must_use]
    pub const fn retry_after(mut self, seconds: u32) -> Self {
        self.retry_after_secs = Some(seconds);
        self
    }

    /// The HTTP status that goes with each code.
    #[must_use]
    pub const fn status(code: ErrorCode) -> StatusCode {
        match code {
            ErrorCode::ServerError => StatusCode::INTERNAL_SERVER_ERROR,
            ErrorCode::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            ErrorCode::Unauthenticated
            | ErrorCode::DeviceRevoked
            | ErrorCode::DeviceExpired
            | ErrorCode::SignInRejected => StatusCode::UNAUTHORIZED,
            ErrorCode::DevicePending => StatusCode::FORBIDDEN,
            ErrorCode::TooManyPendingDevices => StatusCode::CONFLICT,
            ErrorCode::UnsupportedProtocol => StatusCode::UPGRADE_REQUIRED,
            ErrorCode::InvalidRequest => StatusCode::BAD_REQUEST,
            ErrorCode::NotFound => StatusCode::NOT_FOUND,
            ErrorCode::PayloadTooLarge | ErrorCode::NoteTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            ErrorCode::QuotaExceeded => StatusCode::INSUFFICIENT_STORAGE,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = ErrorBody {
            code: self.code,
            message: self.message,
            retry_after_secs: self.retry_after_secs,
        };
        (Self::status(self.code), Json(body)).into_response()
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(error: sqlx::Error) -> Self {
        tracing::error!("database error: {error}");
        Self::new(
            ErrorCode::ServerError,
            "the server could not reach its database",
        )
    }
}
