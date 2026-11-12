//! The approval exchange: how a waiting device gets the account key from an
//! approved one.
//!
//! The server is a mailbox here. It relays small messages between the two
//! devices and holds the encrypted key until the waiting device says it has
//! it. It reads none of it: the devices agree on a key and on a short code
//! the person compares, which is what stops the server from putting itself
//! in the middle (design section 5.4).
//!
//! A device is approved only when it reports that it has the key. A device
//! that stopped half-way is therefore still waiting and can begin again.

use axum::extract::rejection::{PathRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::response::Response;
use dirt_proto::api::{
    limits, DeviceStatus, EnrolmentMessage, EnrolmentMessages, ErrorCode, PostEnrolmentMessage,
    WrappedKey,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::auth::{removed_from_account, status_from_text, AnyDevice, Approved, Device};
use crate::error::{ApiError, ApiResult};
use crate::{ApiJson, AppState};

fn no_such_request() -> ApiError {
    ApiError::new(
        ErrorCode::NotFound,
        "no device is waiting to join under this ID",
    )
}

/// Checks that `target` is a device of the caller's account that is waiting
/// and whose request has not lapsed, and that the caller may take part in
/// its exchange: it is that device, or it is approved.
async fn waiting_device(
    state: &AppState,
    caller: &Device,
    target: Result<Path<Uuid>, PathRejection>,
) -> ApiResult<Uuid> {
    let Path(target) = target.map_err(|_| no_such_request())?;
    if caller.status != DeviceStatus::Approved && caller.id != target {
        return Err(ApiError::new(
            ErrorCode::DevicePending,
            "this device is waiting to be approved by another device",
        ));
    }
    // The account is part of the condition, so no account can reach into
    // another's exchange, whatever ID it sends.
    let found: Option<(String, bool)> = sqlx::query_as(
        "SELECT status, created_at < now() - make_interval(days => $3) \
           FROM devices \
          WHERE id = $1 AND account_id = $2",
    )
    .bind(target)
    .bind(caller.account_id)
    .bind(i32::try_from(limits::PENDING_DEVICE_DAYS).unwrap_or(i32::MAX))
    .fetch_optional(&state.pool)
    .await?;
    let Some((status, lapsed)) = found else {
        return Err(no_such_request());
    };
    match status_from_text(&status)? {
        DeviceStatus::Pending if !lapsed => Ok(target),
        // The device was removed after its token was checked and before
        // this look. It gets the answer its next request would get. "No
        // such request" would leave it waiting for an exchange that is
        // over.
        DeviceStatus::Revoked if caller.id == target => Err(removed_from_account()),
        // A lapsed request is as good as none.
        _ => Err(no_such_request()),
    }
}

/// `POST /v1/enrolments/{device_id}/messages`
pub async fn post_message(
    State(state): State<AppState>,
    AnyDevice(device): AnyDevice,
    target: Result<Path<Uuid>, PathRejection>,
    ApiJson(request): ApiJson<PostEnrolmentMessage>,
) -> ApiResult<Response> {
    let target = waiting_device(&state, &device, target).await?;
    if request.body.is_empty() || request.body.len() > limits::MAX_ENROLMENT_MESSAGE_BYTES {
        return Err(ApiError::invalid(format!(
            "an enrolment message must be 1 to {} bytes",
            limits::MAX_ENROLMENT_MESSAGE_BYTES
        )));
    }
    if request.restart && device.id != target {
        return Err(ApiError::invalid(
            "only the waiting device may begin its exchange again",
        ));
    }

    let mut tx = state.pool.begin().await?;
    // The row lock puts the messages of one exchange in one order.
    let (last,): (i64,) =
        sqlx::query_as("SELECT enrolment_seq FROM devices WHERE id = $1 FOR UPDATE")
            .bind(target)
            .fetch_one(&mut *tx)
            .await?;
    if request.restart {
        sqlx::query("DELETE FROM enrolment_messages WHERE device_id = $1")
            .bind(target)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM enrolment_keys WHERE device_id = $1")
            .bind(target)
            .execute(&mut *tx)
            .await?;
    }
    let (held,): (i64,) =
        sqlx::query_as("SELECT count(*) FROM enrolment_messages WHERE device_id = $1")
            .bind(target)
            .fetch_one(&mut *tx)
            .await?;
    if held >= limits::MAX_ENROLMENT_MESSAGES {
        return Err(ApiError::invalid(
            "this exchange holds as many messages as it may; the waiting device must begin again",
        ));
    }
    let seq = last + 1;
    sqlx::query("UPDATE devices SET enrolment_seq = $2 WHERE id = $1")
        .bind(target)
        .bind(seq)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO enrolment_messages (device_id, seq, sender_id, body) VALUES ($1, $2, $3, $4)",
    )
    .bind(target)
    .bind(seq)
    .bind(device.id)
    .bind(&request.body)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(device.respond(EnrolmentMessage {
        seq,
        sender: device.id,
        body: request.body,
    }))
}

#[derive(Debug, Deserialize)]
pub struct MessagesParams {
    #[serde(default)]
    after: i64,
}

/// `GET /v1/enrolments/{device_id}/messages?after=N`
pub async fn messages(
    State(state): State<AppState>,
    AnyDevice(device): AnyDevice,
    target: Result<Path<Uuid>, PathRejection>,
    params: Result<Query<MessagesParams>, QueryRejection>,
) -> ApiResult<Response> {
    let target = waiting_device(&state, &device, target).await?;
    let Query(params) = params.map_err(|error| ApiError::invalid(error.body_text()))?;
    let rows: Vec<(i64, Uuid, Vec<u8>)> = sqlx::query_as(
        "SELECT seq, sender_id, body FROM enrolment_messages \
          WHERE device_id = $1 AND seq > $2 ORDER BY seq",
    )
    .bind(target)
    .bind(params.after)
    .fetch_all(&state.pool)
    .await?;
    let messages = rows
        .into_iter()
        .map(|(seq, sender, body)| EnrolmentMessage { seq, sender, body })
        .collect();
    Ok(device.respond(EnrolmentMessages { messages }))
}

/// `PUT /v1/enrolments/{device_id}/key`: an approved device leaves the
/// account key, encrypted for the waiting device.
pub async fn put_key(
    State(state): State<AppState>,
    Approved(device): Approved,
    target: Result<Path<Uuid>, PathRejection>,
    ApiJson(key): ApiJson<WrappedKey>,
) -> ApiResult<Response> {
    let target = waiting_device(&state, &device, target).await?;
    if key.wrapped.is_empty() || key.wrapped.len() > limits::MAX_WRAPPED_KEY_BYTES {
        return Err(ApiError::invalid(format!(
            "an encrypted key must be 1 to {} bytes",
            limits::MAX_WRAPPED_KEY_BYTES
        )));
    }
    let mut tx = state.pool.begin().await?;
    // The waiting device must be given the key the account has now. An
    // approving device that has not caught up with a change of the key
    // fetches the new one and leaves that.
    crate::keys::lock_present_key(&mut tx, device.account_id, key.key_version).await?;
    // The approving device is told here that the account is full, while
    // the person is looking at it and can remove a device.
    crate::devices::room_for_a_device(&mut tx, device.account_id, 0).await?;
    sqlx::query(
        "INSERT INTO enrolment_keys (device_id, approver_id, key_version, wrapped) \
         VALUES ($1, $2, $3, $4) \
         ON CONFLICT (device_id) DO UPDATE \
            SET approver_id = EXCLUDED.approver_id, key_version = EXCLUDED.key_version, \
                wrapped = EXCLUDED.wrapped, created_at = now()",
    )
    .bind(target)
    .bind(device.id)
    .bind(key.key_version)
    .bind(&key.wrapped)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(device.respond(serde_json::json!({ "stored": true })))
}

fn only_the_waiting_device(device: &Device, target: Uuid) -> ApiResult<()> {
    if device.id == target {
        Ok(())
    } else {
        Err(ApiError::invalid(
            "only the waiting device may do this for its own request",
        ))
    }
}

/// `GET /v1/enrolments/{device_id}/key`: the waiting device fetches the
/// encrypted key left for it.
pub async fn get_key(
    State(state): State<AppState>,
    AnyDevice(device): AnyDevice,
    target: Result<Path<Uuid>, PathRejection>,
) -> ApiResult<Response> {
    let target = waiting_device(&state, &device, target).await?;
    only_the_waiting_device(&device, target)?;
    let row: Option<(i32, Vec<u8>)> =
        sqlx::query_as("SELECT key_version, wrapped FROM enrolment_keys WHERE device_id = $1")
            .bind(target)
            .fetch_optional(&state.pool)
            .await?;
    let (key_version, wrapped) =
        row.ok_or_else(|| ApiError::new(ErrorCode::NotFound, "no key has been left yet"))?;
    Ok(device.respond(WrappedKey {
        key_version,
        wrapped,
    }))
}

/// `POST /v1/enrolments/{device_id}/complete`: the waiting device says it
/// has opened and saved the key. Only now does it become approved, so a
/// device is never approved without the key.
pub async fn complete(
    State(state): State<AppState>,
    AnyDevice(device): AnyDevice,
    target: Result<Path<Uuid>, PathRejection>,
) -> ApiResult<Response> {
    let target = waiting_device(&state, &device, target).await?;
    only_the_waiting_device(&device, target)?;

    let mut tx = state.pool.begin().await?;
    // Approving a device and changing the account's key take the same
    // lock. A change made first has removed the key left for this device,
    // which then begins its exchange again. A change made after sees this
    // device as approved and must hold a copy of the new key for it.
    let present = crate::keys::lock_account(&mut tx, device.account_id).await?;
    // The last place may have been taken since the key was left. The key
    // stays, so the device is let in once a place is free.
    crate::devices::room_for_a_device(&mut tx, device.account_id, 0).await?;
    // An approved device must have left a key: that is the approval. The
    // waiting device cannot approve itself by calling this.
    let offered: Option<(i32,)> =
        sqlx::query_as("DELETE FROM enrolment_keys WHERE device_id = $1 RETURNING key_version")
            .bind(target)
            .fetch_optional(&mut *tx)
            .await?;
    let Some((offered,)) = offered else {
        return Err(ApiError::new(
            ErrorCode::NotFound,
            "no key has been left yet",
        ));
    };
    if offered != present {
        // Dropping the transaction keeps the key that was left. It cannot
        // happen while a key change removes those keys, and is refused in
        // case it ever can.
        return Err(ApiError::new(
            ErrorCode::KeyChanged,
            "the key left for this device is not the account's present key",
        ));
    }
    sqlx::query("UPDATE devices SET status = 'approved' WHERE id = $1 AND status = 'pending'")
        .bind(target)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM enrolment_messages WHERE device_id = $1")
        .bind(target)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(device.respond(serde_json::json!({ "approved": true })))
}
