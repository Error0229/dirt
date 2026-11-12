//! Deleting an account and resetting one.
//!
//! **Deleting** removes everything the server holds for the account. Any
//! approved device may do it.
//!
//! **Resetting** is for the person who can no longer read their account: no
//! approved device left and no recovery phrase. The notes on the server
//! cannot be decrypted by anyone, so they go, along with the recovery copy
//! and every other device. The device that asked becomes the account's
//! first device and makes a new key. The epoch rises, which tells that
//! device to send its own notes again.
//!
//! An approved device resets at once. A waiting device does too if the
//! server holds no notes for the account, since nothing is lost. Otherwise a
//! waiting device can only ask, and the reset happens after a delay unless
//! an approved device cancels it. Someone who took over the person's email
//! gets a waiting device; without the delay that would be enough to wipe an
//! account whose devices are working.

use axum::extract::State;
use axum::response::Response;
use dirt_proto::api::{limits, DeviceStatus, ErrorCode, ResetResponse};

use crate::auth::{AnyDevice, Approved, Device};
use crate::error::{ApiError, ApiResult};
use crate::AppState;

/// `DELETE /v1/account`
pub async fn delete(
    State(state): State<AppState>,
    Approved(device): Approved,
) -> ApiResult<Response> {
    // Devices, notes, the recovery copy and everything else go with the
    // account row.
    sqlx::query("DELETE FROM accounts WHERE id = $1")
        .bind(device.account_id)
        .execute(&state.pool)
        .await?;
    Ok(device.respond(serde_json::json!({ "deleted": true })))
}

fn delay_days() -> i32 {
    i32::try_from(limits::RESET_DELAY_DAYS).unwrap_or(i32::MAX)
}

/// `POST /v1/account/reset`
pub async fn reset(
    State(state): State<AppState>,
    AnyDevice(device): AnyDevice,
) -> ApiResult<Response> {
    let mut tx = state.pool.begin().await?;
    // The row lock makes two resets, or a reset and a push, take turns.
    let (notes, asked_by_this_device, wait_is_over): (i64, bool, bool) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM notes WHERE account_id = a.id), \
                coalesce(a.reset_requested_by = $2, false), \
                coalesce(a.reset_requested_at <= now() - make_interval(days => $3), false) \
           FROM accounts a WHERE a.id = $1 FOR UPDATE",
    )
    .bind(device.account_id)
    .bind(device.id)
    .bind(delay_days())
    .fetch_one(&mut *tx)
    .await?;

    let may_reset_now = device.status == DeviceStatus::Approved
        // Nothing would be lost.
        || notes == 0
        || (asked_by_this_device && wait_is_over);

    if !may_reset_now {
        // Asking again does not restart the wait, and another device's
        // request is not replaced: the earliest one stands.
        let (effective_at_ms,): (i64,) = sqlx::query_as(
            "UPDATE accounts \
                SET reset_requested_at = coalesce(reset_requested_at, now()), \
                    reset_requested_by = coalesce(reset_requested_by, $2) \
              WHERE id = $1 \
          RETURNING (extract(epoch FROM reset_requested_at + make_interval(days => $3)) * 1000)::bigint",
        )
        .bind(device.account_id)
        .bind(device.id)
        .bind(delay_days())
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        return Ok(device.respond(ResetResponse {
            done: false,
            effective_at_ms: Some(effective_at_ms),
            epoch: device.epoch,
        }));
    }

    sqlx::query("DELETE FROM notes WHERE account_id = $1")
        .bind(device.account_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM recovery_keys WHERE account_id = $1")
        .bind(device.account_id)
        .execute(&mut *tx)
        .await?;
    // Every other device leaves the account: none of them will hold the new
    // key. Their exchange rows go with them.
    sqlx::query("DELETE FROM devices WHERE account_id = $1 AND id <> $2")
        .bind(device.account_id)
        .bind(device.id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM enrolment_messages WHERE device_id = $1")
        .bind(device.id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM enrolment_keys WHERE device_id = $1")
        .bind(device.id)
        .execute(&mut *tx)
        .await?;
    // The device that remains makes a first key again. Wrapped copies of
    // the keys before it are of no use to anyone.
    sqlx::query("DELETE FROM device_keys WHERE device_id = $1")
        .bind(device.id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE devices SET status = 'approved' WHERE id = $1")
        .bind(device.id)
        .execute(&mut *tx)
        .await?;
    let (epoch,): (i32,) = sqlx::query_as(
        "UPDATE accounts \
            SET epoch = epoch + 1, change_counter = 0, note_count = 0, bytes_used = 0, \
                key_version = 1, old_key_notes = 0, \
                key_established = true, reset_requested_at = NULL, reset_requested_by = NULL \
          WHERE id = $1 \
      RETURNING epoch",
    )
    .bind(device.account_id)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;

    let reset = Device { epoch, ..device };
    Ok(reset.respond(ResetResponse {
        done: true,
        effective_at_ms: None,
        epoch,
    }))
}

/// `DELETE /v1/account/reset`: an approved device refuses a reset that a
/// waiting device asked for.
pub async fn cancel_reset(
    State(state): State<AppState>,
    Approved(device): Approved,
) -> ApiResult<Response> {
    let cancelled = sqlx::query(
        "UPDATE accounts SET reset_requested_at = NULL, reset_requested_by = NULL \
          WHERE id = $1 AND reset_requested_at IS NOT NULL",
    )
    .bind(device.account_id)
    .execute(&state.pool)
    .await?
    .rows_affected();
    if cancelled == 0 {
        return Err(ApiError::new(
            ErrorCode::NotFound,
            "no reset has been asked for",
        ));
    }
    Ok(device.respond(serde_json::json!({ "cancelled": true })))
}
