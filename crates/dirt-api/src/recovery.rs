//! The recovery copy: the account key encrypted under the person's recovery
//! phrase, kept so that an account can be opened again when no approved
//! device is left.
//!
//! The server stores two things it cannot use against the account. One is
//! the encrypted key, which only the phrase opens. The other is the hash of
//! a claim code, which is also derived from the phrase: a waiting device
//! that presents the code has shown it knows the phrase, and is approved.
//!
//! A waiting device may read the copy and claim with it. It may not create,
//! replace or delete it: someone who took over the person's email gets a
//! waiting device, and must not be able to destroy the way back in (design
//! amendment B6).

use axum::extract::State;
use axum::response::Response;
use dirt_proto::api::{limits, DeviceStatus, ErrorCode, PutRecovery, RecoveryClaim, WrappedKey};
use sha2::{Digest, Sha256};

use crate::auth::{AnyDevice, Approved};
use crate::error::{ApiError, ApiResult};
use crate::{ApiJson, AppState};

/// `PUT /v1/recovery`
pub async fn put(
    State(state): State<AppState>,
    Approved(device): Approved,
    ApiJson(request): ApiJson<PutRecovery>,
) -> ApiResult<Response> {
    if request.wrapped.is_empty() || request.wrapped.len() > limits::MAX_WRAPPED_KEY_BYTES {
        return Err(ApiError::invalid(format!(
            "an encrypted key must be 1 to {} bytes",
            limits::MAX_WRAPPED_KEY_BYTES
        )));
    }
    if request.claim_hash.len() != limits::CLAIM_HASH_BYTES {
        return Err(ApiError::invalid(format!(
            "a claim hash must be {} bytes",
            limits::CLAIM_HASH_BYTES
        )));
    }
    let mut tx = state.pool.begin().await?;
    // A phrase must open the key the account has now.
    crate::keys::lock_present_key(&mut tx, device.account_id, request.key_version).await?;
    sqlx::query(
        "INSERT INTO recovery_keys (account_id, key_version, wrapped, claim_hash) \
         VALUES ($1, $2, $3, $4) \
         ON CONFLICT (account_id) DO UPDATE \
            SET key_version = EXCLUDED.key_version, wrapped = EXCLUDED.wrapped, \
                claim_hash = EXCLUDED.claim_hash, created_at = now()",
    )
    .bind(device.account_id)
    .bind(request.key_version)
    .bind(&request.wrapped)
    .bind(&request.claim_hash)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(device.respond(serde_json::json!({ "stored": true })))
}

fn no_recovery_copy() -> ApiError {
    ApiError::new(ErrorCode::NotFound, "this account has no recovery copy")
}

/// `GET /v1/recovery`
pub async fn get(
    State(state): State<AppState>,
    AnyDevice(device): AnyDevice,
) -> ApiResult<Response> {
    let row: Option<(i32, Vec<u8>)> =
        sqlx::query_as("SELECT key_version, wrapped FROM recovery_keys WHERE account_id = $1")
            .bind(device.account_id)
            .fetch_optional(&state.pool)
            .await?;
    let (key_version, wrapped) = row.ok_or_else(no_recovery_copy)?;
    Ok(device.respond(WrappedKey {
        key_version,
        wrapped,
    }))
}

/// `DELETE /v1/recovery`
pub async fn delete(
    State(state): State<AppState>,
    Approved(device): Approved,
) -> ApiResult<Response> {
    let removed = sqlx::query("DELETE FROM recovery_keys WHERE account_id = $1")
        .bind(device.account_id)
        .execute(&state.pool)
        .await?
        .rows_affected();
    if removed == 0 {
        return Err(no_recovery_copy());
    }
    Ok(device.respond(serde_json::json!({ "deleted": true })))
}

/// Compares two hashes without stopping at the first byte that differs.
fn same_hash(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |sum, (x, y)| sum | (x ^ y)) == 0
}

/// `POST /v1/recovery/claim`: a waiting device shows that it knows the
/// recovery phrase, and is approved.
pub async fn claim(
    State(state): State<AppState>,
    AnyDevice(device): AnyDevice,
    ApiJson(request): ApiJson<RecoveryClaim>,
) -> ApiResult<Response> {
    if request.claim_code.is_empty() || request.claim_code.len() > limits::MAX_CLAIM_CODE_BYTES {
        return Err(ApiError::invalid("the claim code has the wrong length"));
    }
    let mut tx = state.pool.begin().await?;
    // Approving a device and changing the account's key take the same
    // lock. A change made first has removed the recovery copy, and the
    // claim finds none. A change made after sees this device as approved
    // and must hold a copy of the new key for it.
    crate::keys::lock_account(&mut tx, device.account_id).await?;
    let stored: Option<(Vec<u8>,)> =
        sqlx::query_as("SELECT claim_hash FROM recovery_keys WHERE account_id = $1")
            .bind(device.account_id)
            .fetch_optional(&mut *tx)
            .await?;
    let (claim_hash,) = stored.ok_or_else(no_recovery_copy)?;
    let presented = Sha256::digest(&request.claim_code);
    if !same_hash(&presented, &claim_hash) {
        return Err(ApiError::new(
            ErrorCode::RecoveryRejected,
            "the claim code is not this account's",
        ));
    }
    if device.status == DeviceStatus::Pending {
        sqlx::query("UPDATE devices SET status = 'approved' WHERE id = $1 AND status = 'pending'")
            .bind(device.id)
            .execute(&mut *tx)
            .await?;
        // Whatever exchange this device had begun is over.
        sqlx::query("DELETE FROM enrolment_messages WHERE device_id = $1")
            .bind(device.id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM enrolment_keys WHERE device_id = $1")
            .bind(device.id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(device.respond(serde_json::json!({ "approved": true })))
}
