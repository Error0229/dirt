//! Who is calling: device tokens and the checks made on every request.
//!
//! A device proves itself with the token it was given at registration. The
//! server keeps only the token's SHA-256 hash, so a copy of the database
//! yields no usable token.

use axum::extract::{FromRef, FromRequestParts};
use axum::http::request::Parts;
use axum::http::{header, HeaderName, HeaderValue};
use axum::response::{IntoResponse, Response};
use axum::Json;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use dirt_proto::api::{limits, DeviceStatus, ErrorCode, EPOCH_HEADER};
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::AppState;

/// A new random token: 256 bits, as URL-safe text.
#[must_use]
pub fn new_token() -> String {
    URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>())
}

/// What the server stores in place of a token or a nonce.
#[must_use]
pub fn hash_secret(secret: &str) -> Vec<u8> {
    Sha256::digest(secret.as_bytes()).to_vec()
}

pub fn status_from_text(text: &str) -> ApiResult<DeviceStatus> {
    match text {
        "pending" => Ok(DeviceStatus::Pending),
        "approved" => Ok(DeviceStatus::Approved),
        "revoked" => Ok(DeviceStatus::Revoked),
        other => {
            tracing::error!("a device row holds the unknown status {other:?}");
            Err(ApiError::new(ErrorCode::ServerError, "corrupt device row"))
        }
    }
}

#[must_use]
pub const fn status_text(status: DeviceStatus) -> &'static str {
    match status {
        DeviceStatus::Pending => "pending",
        DeviceStatus::Approved => "approved",
        DeviceStatus::Revoked => "revoked",
    }
}

/// The device that made a request, once its token has been checked.
#[derive(Debug, Clone, Copy)]
pub struct Device {
    pub id: Uuid,
    pub account_id: Uuid,
    pub status: DeviceStatus,
    pub epoch: i32,
}

impl Device {
    /// A JSON response carrying the account's epoch in its header, as every
    /// authenticated response must.
    pub fn respond<T: Serialize>(&self, body: T) -> Response {
        let mut response = Json(body).into_response();
        response.headers_mut().insert(
            HeaderName::from_static(EPOCH_HEADER),
            HeaderValue::from(self.epoch),
        );
        response
    }
}

fn bearer_token(parts: &Parts) -> Option<&str> {
    parts
        .headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(str::trim)
        .filter(|token| !token.is_empty())
}

/// Checks the token and counts the request. Accepts pending and approved
/// devices; refuses unknown tokens, revoked devices, tokens left unused for
/// too long, and devices whose request to join the account has lapsed.
async fn authenticate(pool: &PgPool, parts: &Parts) -> ApiResult<Device> {
    let token = bearer_token(parts)
        .ok_or_else(|| ApiError::new(ErrorCode::Unauthenticated, "a device token is required"))?;

    // A request to join lapses after a while, unless the device has asked
    // for the account to be reset: it then has to be able to come back when
    // the wait for that is over.
    let row: Option<(Uuid, Uuid, String, i32, bool, bool, bool)> = sqlx::query_as(
        "SELECT d.id, d.account_id, d.status, a.epoch, \
                d.last_seen < now() - make_interval(days => $2), \
                d.last_seen < now() - interval '1 hour', \
                d.status = 'pending' \
                    AND d.created_at < now() - make_interval(days => $3) \
                    AND a.reset_requested_by IS DISTINCT FROM d.id \
           FROM devices d JOIN accounts a ON a.id = d.account_id \
          WHERE d.token_hash = $1",
    )
    .bind(hash_secret(token))
    .bind(i32::try_from(limits::DEVICE_TOKEN_IDLE_DAYS).unwrap_or(i32::MAX))
    .bind(i32::try_from(limits::PENDING_DEVICE_DAYS).unwrap_or(i32::MAX))
    .fetch_optional(pool)
    .await?;
    let (id, account_id, status, epoch, expired, last_seen_is_old, request_lapsed) =
        row.ok_or_else(|| ApiError::new(ErrorCode::Unauthenticated, "unknown device token"))?;

    let status = status_from_text(&status)?;
    if status == DeviceStatus::Revoked {
        return Err(ApiError::new(
            ErrorCode::DeviceRevoked,
            "this device was removed from the account",
        ));
    }
    if expired {
        return Err(ApiError::new(
            ErrorCode::DeviceExpired,
            "this device's token went unused for too long; sign in again",
        ));
    }
    if request_lapsed {
        return Err(ApiError::new(
            ErrorCode::DeviceExpired,
            "this device's request to join the account was not answered in time; sign in again",
        ));
    }

    count_request(pool, account_id).await?;

    // "Last seen" only has to be accurate enough for the 90-day rule and for
    // the device list, so it is written at most once an hour.
    if last_seen_is_old {
        sqlx::query("UPDATE devices SET last_seen = now() WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;
    }
    Ok(Device {
        id,
        account_id,
        status,
        epoch,
    })
}

/// Adds one to the account's count for the current window and refuses the
/// request if that passes the limit.
///
/// The count lives in the database because serverless instances share no
/// memory.
async fn count_request(pool: &PgPool, account_id: Uuid) -> ApiResult<()> {
    let (requests, seconds_left): (i32, i32) = sqlx::query_as(
        "WITH w AS ( \
             SELECT to_timestamp((floor(extract(epoch FROM now()) / $2::bigint) * $2::bigint)::float8) AS start \
         ) \
         INSERT INTO request_counts (account_id, window_start, requests) \
         SELECT $1, w.start, 1 FROM w \
         ON CONFLICT (account_id, window_start) \
            DO UPDATE SET requests = request_counts.requests + 1 \
         RETURNING requests, \
                   ceil(extract(epoch FROM window_start) + $2::bigint - extract(epoch FROM now()))::int",
    )
    .bind(account_id)
    .bind(limits::REQUEST_WINDOW_SECS)
    .fetch_one(pool)
    .await?;

    if requests == 1 {
        // The first request of a window clears the account's old windows.
        sqlx::query(
            "DELETE FROM request_counts \
              WHERE account_id = $1 AND window_start < now() - interval '1 day'",
        )
        .bind(account_id)
        .execute(pool)
        .await?;
    }
    if requests > limits::REQUESTS_PER_WINDOW {
        return Err(
            ApiError::new(ErrorCode::RateLimited, "too many requests; slow down")
                .retry_after(u32::try_from(seconds_left.max(1)).unwrap_or(1)),
        );
    }
    Ok(())
}

/// Extractor: any device whose token is good, approved or still pending.
pub struct AnyDevice(pub Device);

/// Extractor: an approved device. A pending device is refused, so it can
/// read and write nothing.
pub struct Approved(pub Device);

impl<S> FromRequestParts<S> for AnyDevice
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let state = AppState::from_ref(state);
        authenticate(&state.pool, parts).await.map(Self)
    }
}

impl<S> FromRequestParts<S> for Approved
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let AnyDevice(device) = AnyDevice::from_request_parts(parts, state).await?;
        if device.status == DeviceStatus::Approved {
            Ok(Self(device))
        } else {
            Err(ApiError::new(
                ErrorCode::DevicePending,
                "this device is waiting to be approved by another device",
            ))
        }
    }
}
