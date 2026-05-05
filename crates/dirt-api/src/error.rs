//! Unified error response shape.
//!
//! All client-facing error bodies follow the design-doc contract:
//! ```json
//! {
//!   "error": {
//!     "code": "TOKEN_MISSING",
//!     "message": "missing Authorization header",
//!     "cause": "No Authorization header present on the request.",
//!     "fix": "Include 'Authorization: Bearer <DIRT_CLIENT_TOKEN>'.",
//!     "retry_after_secs": null
//!   }
//! }
//! ```

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("{message}")]
    Unauthorized { code: &'static str, message: String },
    #[error("{message}")]
    BadRequest { code: &'static str, message: String },
    #[error("payload too large: {message}")]
    PayloadTooLarge { message: String },
    #[error("rate limited: {message}")]
    RateLimited {
        message: String,
        retry_after_secs: u64,
    },
    #[error("turso unreachable: {message}")]
    TursoUnreachable { message: String },
    #[error("config error: {0}")]
    Config(String),
    #[error("internal error: {0}")]
    Internal(String),
}

// Phase 2 magic-link auth: distinct error codes so clients can tell
// "wrong code" from "expired code" from "too many tries" without parsing
// human-readable messages.
//
// `SESSION_EXPIRED` is a 401 (the session middleware peels it off the
// Authorization header). The other three are 400s — they describe a
// client-supplied request body that the server rejected.

#[derive(Debug, Serialize)]
struct ErrorEnvelope {
    error: ErrorBody,
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    code: &'static str,
    message: String,
    cause: String,
    fix: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    retry_after_secs: Option<u64>,
}

impl AppError {
    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::Unauthorized {
            code: "UNAUTHORIZED",
            message: message.into(),
        }
    }

    pub fn session_expired(message: impl Into<String>) -> Self {
        Self::Unauthorized {
            code: "SESSION_EXPIRED",
            message: message.into(),
        }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::BadRequest {
            code: "BAD_REQUEST",
            message: message.into(),
        }
    }

    pub fn invalid_email(message: impl Into<String>) -> Self {
        Self::BadRequest {
            code: "INVALID_EMAIL",
            message: message.into(),
        }
    }

    pub fn invalid_code(message: impl Into<String>) -> Self {
        Self::BadRequest {
            code: "INVALID_CODE",
            message: message.into(),
        }
    }

    pub fn expired_code(message: impl Into<String>) -> Self {
        Self::BadRequest {
            code: "EXPIRED_CODE",
            message: message.into(),
        }
    }

    pub fn too_many_attempts(message: impl Into<String>) -> Self {
        Self::BadRequest {
            code: "TOO_MANY_ATTEMPTS",
            message: message.into(),
        }
    }

    pub fn batch_too_large(message: impl Into<String>) -> Self {
        Self::BadRequest {
            code: "BATCH_TOO_LARGE",
            message: message.into(),
        }
    }

    pub fn payload_too_large(message: impl Into<String>) -> Self {
        Self::PayloadTooLarge {
            message: message.into(),
        }
    }

    pub fn rate_limited(message: impl Into<String>, retry_after_secs: u64) -> Self {
        Self::RateLimited {
            message: message.into(),
            retry_after_secs,
        }
    }

    pub fn turso_unreachable(message: impl Into<String>) -> Self {
        Self::TursoUnreachable {
            message: message.into(),
        }
    }

    pub fn config(message: impl Into<String>) -> Self {
        Self::Config(message.into())
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }

    const fn code(&self) -> &'static str {
        match self {
            Self::Unauthorized { code, .. } | Self::BadRequest { code, .. } => code,
            Self::PayloadTooLarge { .. } => "PAYLOAD_TOO_LARGE",
            Self::RateLimited { .. } => "RATE_LIMITED",
            Self::TursoUnreachable { .. } => "TURSO_UNREACHABLE",
            Self::Config(_) | Self::Internal(_) => "INTERNAL",
        }
    }

    const fn status(&self) -> StatusCode {
        match self {
            Self::Unauthorized { .. } => StatusCode::UNAUTHORIZED,
            Self::BadRequest { .. } => StatusCode::BAD_REQUEST,
            Self::PayloadTooLarge { .. } => StatusCode::PAYLOAD_TOO_LARGE,
            Self::RateLimited { .. } => StatusCode::TOO_MANY_REQUESTS,
            Self::TursoUnreachable { .. } => StatusCode::SERVICE_UNAVAILABLE,
            Self::Config(_) | Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn cause(&self) -> String {
        match self {
            Self::Unauthorized { message, .. }
            | Self::BadRequest { message, .. }
            | Self::PayloadTooLarge { message }
            | Self::RateLimited { message, .. }
            | Self::TursoUnreachable { message } => message.clone(),
            Self::Config(msg) | Self::Internal(msg) => msg.clone(),
        }
    }

    const fn retry_after_secs(&self) -> Option<u64> {
        match self {
            Self::RateLimited {
                retry_after_secs, ..
            } => Some(*retry_after_secs),
            _ => None,
        }
    }

    fn fix(&self) -> &'static str {
        match self {
            Self::Unauthorized { code, .. } => match *code {
                "SESSION_EXPIRED" => {
                    "Sign in again to obtain a fresh session token."
                }
                _ => {
                    "Include 'Authorization: Bearer <session-token>' from the magic-code verify response."
                }
            },
            Self::BadRequest { code, .. } => match *code {
                "BATCH_TOO_LARGE" => "Split the batch into groups of at most 500 notes and retry.",
                "INVALID_EMAIL" => {
                    "Send a syntactically valid email address (RFC-5322-ish)."
                }
                "INVALID_CODE" => {
                    "Re-check the code from the email and retry; if it keeps failing, request a new one."
                }
                "EXPIRED_CODE" => {
                    "Magic codes are valid for 15 minutes. Request a new code via /v1/auth/request."
                }
                "TOO_MANY_ATTEMPTS" => {
                    "This request_id is locked. Request a fresh code via /v1/auth/request."
                }
                _ => "Fix the request body or parameters and retry.",
            },
            Self::PayloadTooLarge { .. } => {
                "Reduce the request body size (<= 8 MiB) or split the batch."
            }
            Self::RateLimited { .. } => {
                "Slow down requests; honour the retry_after_secs hint before retrying."
            }
            Self::TursoUnreachable { .. } => {
                "Retry shortly. If it persists, check TURSO_DATABASE_URL/TURSO_AUTH_TOKEN on the server."
            }
            Self::Config(_) | Self::Internal(_) => {
                "This is a server-side issue; check server logs."
            }
        }
    }
}

impl From<libsql::Error> for AppError {
    fn from(err: libsql::Error) -> Self {
        Self::turso_unreachable(err.to_string())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status();
        let retry_after = self.retry_after_secs();
        let body = ErrorEnvelope {
            error: ErrorBody {
                code: self.code(),
                message: self.to_string(),
                cause: self.cause(),
                fix: self.fix().to_string(),
                retry_after_secs: retry_after,
            },
        };
        let mut response = (status, Json(body)).into_response();
        if let Some(secs) = retry_after {
            // Standard `Retry-After` header so clients don't have to
            // parse the body to back off.
            if let Ok(value) = axum::http::HeaderValue::from_str(&secs.to_string()) {
                response.headers_mut().insert("retry-after", value);
            }
        }
        response
    }
}
