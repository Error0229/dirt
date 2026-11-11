//! The device registry: sign-in nonces, registration, the device list and
//! revocation.

use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::response::Response;
use axum::Json;
use dirt_proto::api::{
    limits, DeviceInfo, DeviceList, DeviceStatus, ErrorCode, NonceResponse, RegisterDeviceRequest,
    RegisterDeviceResponse,
};
use uuid::Uuid;

use crate::auth::{hash_secret, new_token, status_from_text, status_text, AnyDevice, Approved};
use crate::clerk::VerifyError;
use crate::error::{ApiError, ApiResult};
use crate::{ApiJson, AppState};

/// `POST /v1/nonces`
///
/// Clerk's ID token stays valid for a day. Binding each sign-in to a value
/// that works once means a copied token cannot register a second device.
pub async fn create_nonce(State(state): State<AppState>) -> ApiResult<Json<NonceResponse>> {
    let nonce = new_token();
    sqlx::query(
        "INSERT INTO nonces (nonce_hash, expires_at) \
         VALUES ($1, now() + make_interval(secs => $2))",
    )
    .bind(hash_secret(&nonce))
    .bind(f64::from(limits::NONCE_LIFETIME_SECS))
    .execute(&state.pool)
    .await?;
    // Nobody is signed in yet, so there is no account to tidy up for. Each
    // call clears a few expired rows instead.
    sqlx::query(
        "DELETE FROM nonces WHERE nonce_hash IN \
             (SELECT nonce_hash FROM nonces WHERE expires_at < now() LIMIT 100)",
    )
    .execute(&state.pool)
    .await?;
    Ok(Json(NonceResponse {
        nonce,
        expires_in_secs: limits::NONCE_LIFETIME_SECS,
    }))
}

fn sign_in_rejected(error: &VerifyError) -> ApiError {
    match error {
        VerifyError::KeysUnavailable(reason) => {
            tracing::error!("could not fetch the identity provider's keys: {reason}");
            ApiError::new(
                ErrorCode::ServerError,
                "the server could not reach the identity provider",
            )
        }
        other => ApiError::new(ErrorCode::SignInRejected, other.to_string()),
    }
}

/// `POST /v1/devices`
pub async fn register(
    State(state): State<AppState>,
    ApiJson(request): ApiJson<RegisterDeviceRequest>,
) -> ApiResult<Json<RegisterDeviceResponse>> {
    let name = request.name.trim();
    if name.is_empty() || name.len() > limits::MAX_DEVICE_NAME_BYTES {
        return Err(ApiError::invalid(format!(
            "a device name must be 1 to {} bytes",
            limits::MAX_DEVICE_NAME_BYTES
        )));
    }
    if request.public_key.len() != limits::PUBLIC_KEY_BYTES {
        return Err(ApiError::invalid(format!(
            "a device public key must be {} bytes",
            limits::PUBLIC_KEY_BYTES
        )));
    }

    // The token is checked before the nonce is spent, so that a bad token
    // cannot use up somebody's nonce.
    let sign_in = state
        .keys
        .verify(
            &request.id_token,
            &state.config.clerk_issuer,
            &state.config.clerk_client_id,
            &request.nonce,
        )
        .await
        .map_err(|error| sign_in_rejected(&error))?;

    let mut tx = state.pool.begin().await?;

    let spent = sqlx::query("DELETE FROM nonces WHERE nonce_hash = $1 AND expires_at > now()")
        .bind(hash_secret(&request.nonce))
        .execute(&mut *tx)
        .await?
        .rows_affected();
    if spent == 0 {
        return Err(ApiError::new(
            ErrorCode::SignInRejected,
            "the sign-in nonce is unknown, expired or already used",
        ));
    }

    let (account_id, epoch): (Uuid, i32) = sqlx::query_as(
        "INSERT INTO accounts (id, clerk_issuer, clerk_user_id, email) \
         VALUES ($1, $2, $3, $4) \
         ON CONFLICT (clerk_issuer, clerk_user_id) DO UPDATE SET email = EXCLUDED.email \
         RETURNING id, epoch",
    )
    .bind(Uuid::now_v7())
    .bind(&state.config.clerk_issuer)
    .bind(&sign_in.user_id)
    .bind(&sign_in.email)
    .fetch_one(&mut *tx)
    .await?;

    // Only one registration can flip the flag, however many arrive at once:
    // the update locks the account row, and the others then see it set. The
    // winner is the first device, which creates the account key.
    let first_device = sqlx::query(
        "UPDATE accounts SET key_established = true \
          WHERE id = $1 AND key_established = false",
    )
    .bind(account_id)
    .execute(&mut *tx)
    .await?
    .rows_affected()
        == 1;

    let status = if first_device {
        DeviceStatus::Approved
    } else {
        let (pending,): (i64,) = sqlx::query_as(
            "SELECT count(*) FROM devices WHERE account_id = $1 AND status = 'pending'",
        )
        .bind(account_id)
        .fetch_one(&mut *tx)
        .await?;
        if pending >= limits::MAX_PENDING_DEVICES {
            return Err(ApiError::new(
                ErrorCode::TooManyPendingDevices,
                "this account already has devices waiting for approval; \
                 approve or remove one first",
            ));
        }
        DeviceStatus::Pending
    };

    let device_id = Uuid::now_v7();
    let token = new_token();
    sqlx::query(
        "INSERT INTO devices (id, account_id, name, public_key, token_hash, status) \
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(device_id)
    .bind(account_id)
    .bind(name)
    .bind(&request.public_key)
    .bind(hash_secret(&token))
    .bind(status_text(status))
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(Json(RegisterDeviceResponse {
        account_id,
        device_id,
        token,
        status,
        first_device,
        epoch,
    }))
}

type DeviceRow = (Uuid, String, Vec<u8>, String, i64, i64);

/// The columns of a [`DeviceRow`]. A macro, not a constant, because sqlx
/// accepts only string literals as SQL.
macro_rules! device_columns {
    () => {
        "id, name, public_key, status, \
         (extract(epoch FROM created_at) * 1000)::bigint, \
         (extract(epoch FROM last_seen) * 1000)::bigint"
    };
}

fn device_info(row: DeviceRow, current: Uuid) -> ApiResult<DeviceInfo> {
    let (id, name, public_key, status, created_at_ms, last_seen_ms) = row;
    Ok(DeviceInfo {
        id,
        name,
        public_key,
        status: status_from_text(&status)?,
        created_at_ms,
        last_seen_ms,
        current: id == current,
    })
}

/// `GET /v1/devices/me`: the one call a pending device may make, to learn
/// whether it has been approved.
pub async fn me(
    State(state): State<AppState>,
    AnyDevice(device): AnyDevice,
) -> ApiResult<Response> {
    let row: DeviceRow = sqlx::query_as(concat!(
        "SELECT ",
        device_columns!(),
        " FROM devices WHERE id = $1"
    ))
    .bind(device.id)
    .fetch_one(&state.pool)
    .await?;
    Ok(device.respond(device_info(row, device.id)?))
}

/// `GET /v1/devices`
pub async fn list(
    State(state): State<AppState>,
    Approved(device): Approved,
) -> ApiResult<Response> {
    let rows: Vec<DeviceRow> = sqlx::query_as(concat!(
        "SELECT ",
        device_columns!(),
        " FROM devices WHERE account_id = $1 ORDER BY created_at, id"
    ))
    .bind(device.account_id)
    .fetch_all(&state.pool)
    .await?;
    let devices = rows
        .into_iter()
        .map(|row| device_info(row, device.id))
        .collect::<ApiResult<Vec<_>>>()?;
    Ok(device.respond(DeviceList { devices }))
}

/// `DELETE /v1/devices/{id}`: removes a device from the account. Its token
/// stops working with the next request it makes.
pub async fn revoke(
    State(state): State<AppState>,
    Approved(device): Approved,
    target: Result<Path<Uuid>, PathRejection>,
) -> ApiResult<Response> {
    let Path(target) = target.map_err(|_| ApiError::new(ErrorCode::NotFound, "no such device"))?;
    // The account is part of the condition, so one account can never revoke
    // another's device, whatever ID it sends.
    let revoked =
        sqlx::query("UPDATE devices SET status = 'revoked' WHERE id = $1 AND account_id = $2")
            .bind(target)
            .bind(device.account_id)
            .execute(&state.pool)
            .await?
            .rows_affected();
    if revoked == 0 {
        return Err(ApiError::new(ErrorCode::NotFound, "no such device"));
    }
    Ok(device.respond(serde_json::json!({ "revoked": target })))
}
