//! The device registry: sign-in nonces, registration, the device list and
//! revocation.

use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::response::Response;
use axum::Json;
use dirt_proto::api::{
    limits, DeviceInfo, DeviceList, DeviceStatus, ErrorCode, MyDevice, NonceResponse,
    RegisterDeviceRequest, RegisterDeviceResponse,
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
        issuer: state.config.clerk_issuer.clone(),
        client_id: state.config.clerk_client_id.clone(),
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

    // A device whose token expired signs in again and stays the device it
    // was. It proves that with the old token, which only it ever held, and
    // its public key. A revoked device never matches, so it starts over as
    // a new device that has to be approved. A device that was still waiting
    // has its request renewed: the time it has to be approved starts again.
    if let Some(previous) = request.previous_token.as_deref() {
        let token = new_token();
        let known: Option<(Uuid, String)> = sqlx::query_as(
            "UPDATE devices SET token_hash = $1, name = $2, last_seen = now(), \
                    created_at = CASE WHEN status = 'pending' THEN now() ELSE created_at END \
              WHERE account_id = $3 AND token_hash = $4 AND public_key = $5 \
                AND status <> 'revoked' \
             RETURNING id, status",
        )
        .bind(hash_secret(&token))
        .bind(name)
        .bind(account_id)
        .bind(hash_secret(previous))
        .bind(&request.public_key)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some((device_id, status)) = known {
            let status = status_from_text(&status)?;
            tx.commit().await?;
            return Ok(Json(RegisterDeviceResponse {
                account_id,
                device_id,
                token,
                status,
                first_device: false,
                epoch,
            }));
        }
    }

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
        // Requests that have lapsed no longer hold a place.
        let (pending,): (i64,) = sqlx::query_as(
            "SELECT count(*) FROM devices \
              WHERE account_id = $1 AND status = 'pending' \
                AND created_at >= now() - make_interval(days => $2)",
        )
        .bind(account_id)
        .bind(i32::try_from(limits::PENDING_DEVICE_DAYS).unwrap_or(i32::MAX))
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

/// `GET /v1/devices/me`: the calling device and the state of its account.
///
/// A waiting device learns here whether it has been approved, how many
/// devices could approve it, and whether a recovery phrase would let it in.
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
    let (approvers, recovery_available, reset_effective_at_ms, key_version): (
        i64,
        bool,
        Option<i64>,
        i32,
    ) = sqlx::query_as(
            "SELECT (SELECT count(*) FROM devices \
                      WHERE account_id = a.id AND status = 'approved' AND id <> $2), \
                    EXISTS (SELECT 1 FROM recovery_keys WHERE account_id = a.id), \
                    (extract(epoch FROM a.reset_requested_at + make_interval(days => $3)) * 1000)::bigint, \
                    a.key_version \
               FROM accounts a WHERE a.id = $1",
        )
        .bind(device.account_id)
        .bind(device.id)
        .bind(i32::try_from(limits::RESET_DELAY_DAYS).unwrap_or(i32::MAX))
        .fetch_one(&state.pool)
        .await?;
    Ok(device.respond(MyDevice {
        device: device_info(row, device.id)?,
        approvers: u32::try_from(approvers).unwrap_or(u32::MAX),
        recovery_available,
        reset_effective_at_ms,
        key_version,
    }))
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
///
/// An approved device may remove any device of its account. A device that
/// is still waiting may remove only itself, which is what signing out of
/// it does: otherwise it would wait for ever and count against the limit
/// on waiting devices.
pub async fn revoke(
    State(state): State<AppState>,
    AnyDevice(device): AnyDevice,
    target: Result<Path<Uuid>, PathRejection>,
) -> ApiResult<Response> {
    let Path(target) = target.map_err(|_| ApiError::new(ErrorCode::NotFound, "no such device"))?;
    if device.status != DeviceStatus::Approved && target != device.id {
        return Err(ApiError::new(
            ErrorCode::DevicePending,
            "this device is waiting to be approved by another device",
        ));
    }
    // The account is part of the condition, so one account can never revoke
    // another's device, whatever ID it sends.
    let mut tx = state.pool.begin().await?;
    let revoked =
        sqlx::query("UPDATE devices SET status = 'revoked' WHERE id = $1 AND account_id = $2")
            .bind(target)
            .bind(device.account_id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
    if revoked == 0 {
        return Err(ApiError::new(ErrorCode::NotFound, "no such device"));
    }
    // Whatever exchange the device had begun ends with it, and so does a
    // reset it asked for.
    sqlx::query("DELETE FROM enrolment_messages WHERE device_id = $1")
        .bind(target)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM enrolment_keys WHERE device_id = $1")
        .bind(target)
        .execute(&mut *tx)
        .await?;
    // Its wrapped copies of the account's keys are of no more use to it.
    sqlx::query("DELETE FROM device_keys WHERE device_id = $1")
        .bind(target)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "UPDATE accounts SET reset_requested_at = NULL, reset_requested_by = NULL \
          WHERE id = $1 AND reset_requested_by = $2",
    )
    .bind(device.account_id)
    .bind(target)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(device.respond(serde_json::json!({ "revoked": target })))
}
