//! Changing the account's key.
//!
//! Removing a device stops it reaching the server, but it keeps the key it
//! had. Changing the key is what ends that: notes written afterwards are
//! under a key the removed device never held.
//!
//! The server's part is small, as it is for the approval exchange. It
//! stores the new key wrapped for each remaining device, tells devices
//! which version to write under, refuses notes under an older one, and
//! counts the notes not yet sent again. It can open none of the wrapped
//! copies, and it cannot make one that a device would accept: wrapping
//! needs the key being replaced, which the server has never had (design
//! section 5.5).
//!
//! A change is all or nothing at its start. One request stores the new key
//! for every approved device, the caller included, and moves the account to
//! the new version. From then on any device can fetch the key and carry on
//! sending notes again, so a caller that stops half-way leaves nothing
//! broken.

use std::collections::HashSet;

use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use axum::response::Response;
use dirt_proto::api::{
    limits, ChangeKey, DeviceKeys, ErrorCode, KeyChanged, OldKeyNotes, WrappedKey,
};
use dirt_proto::NoteId;
use serde::Deserialize;
use uuid::Uuid;

use crate::auth::Approved;
use crate::error::{ApiError, ApiResult};
use crate::{ApiJson, AppState};

fn to_count(count: i64) -> u64 {
    u64::try_from(count).unwrap_or(0)
}

/// Locks the account and checks that `key_version` is the version its
/// notes are written under now.
///
/// A copy of the account key that is about to be stored, for a waiting
/// device or under the recovery phrase, has to hold the present key. A
/// device that has not caught up with a change would otherwise hand over
/// the key that was replaced. The lock keeps a change from falling between
/// this check and the write that follows it in the same transaction.
pub async fn lock_present_key(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    account_id: Uuid,
    key_version: i32,
) -> ApiResult<()> {
    let (current,): (i32,) =
        sqlx::query_as("SELECT key_version FROM accounts WHERE id = $1 FOR UPDATE")
            .bind(account_id)
            .fetch_one(&mut **tx)
            .await?;
    if key_version == current {
        Ok(())
    } else {
        Err(ApiError::new(
            ErrorCode::KeyChanged,
            "the account's key has changed; fetch the new one first",
        ))
    }
}

/// `POST /v1/account/key`: an approved device changes the account's key.
pub async fn change(
    State(state): State<AppState>,
    Approved(device): Approved,
    ApiJson(request): ApiJson<ChangeKey>,
) -> ApiResult<Response> {
    for key in &request.keys {
        if key.wrapped.is_empty() || key.wrapped.len() > limits::MAX_WRAPPED_KEY_BYTES {
            return Err(ApiError::invalid(format!(
                "an encrypted key must be 1 to {} bytes",
                limits::MAX_WRAPPED_KEY_BYTES
            )));
        }
    }
    let mut named = HashSet::with_capacity(request.keys.len());
    if !request.keys.iter().all(|key| named.insert(key.device_id)) {
        return Err(ApiError::invalid(
            "a key change may name each device only once",
        ));
    }

    let mut tx = state.pool.begin().await?;
    // The account row is locked, as it is for a push, so a key change and
    // the pushes around it happen one at a time.
    let (current,): (i32,) =
        sqlx::query_as("SELECT key_version FROM accounts WHERE id = $1 FOR UPDATE")
            .bind(device.account_id)
            .fetch_one(&mut *tx)
            .await?;
    if request.from_version != current {
        return Err(ApiError::new(
            ErrorCode::KeyChanged,
            "the account's key has already been changed; fetch the new one",
        ));
    }

    // The new key has to be there for every approved device, and for no
    // other. A device approved or removed since the caller looked would be
    // locked out, or let back in.
    let approved: Vec<(Uuid,)> =
        sqlx::query_as("SELECT id FROM devices WHERE account_id = $1 AND status = 'approved'")
            .bind(device.account_id)
            .fetch_all(&mut *tx)
            .await?;
    let approved: HashSet<Uuid> = approved.into_iter().map(|(id,)| id).collect();
    if approved != named {
        return Err(ApiError::new(
            ErrorCode::DevicesChanged,
            "the key must be wrapped for each approved device and no other",
        ));
    }

    let next = current + 1;
    for key in &request.keys {
        sqlx::query(
            "INSERT INTO device_keys (device_id, key_version, wrapped) VALUES ($1, $2, $3)",
        )
        .bind(key.device_id)
        .bind(next)
        .bind(&key.wrapped)
        .execute(&mut *tx)
        .await?;
    }
    // Every note the server holds is now under an older key.
    let (old_key_notes,): (i64,) = sqlx::query_as(
        "UPDATE accounts \
            SET key_version = $2, \
                old_key_notes = (SELECT count(*) FROM notes \
                                  WHERE account_id = $1 AND ciphertext IS NOT NULL) \
          WHERE id = $1 \
      RETURNING old_key_notes",
    )
    .bind(device.account_id)
    .bind(next)
    .fetch_one(&mut *tx)
    .await?;
    // The recovery copy holds the old key, and so does anything an approval
    // exchange has left for a waiting device. Both go: the phrase has to be
    // made again, and a waiting device begins its exchange again.
    sqlx::query("DELETE FROM recovery_keys WHERE account_id = $1")
        .bind(device.account_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "DELETE FROM enrolment_keys WHERE device_id IN \
            (SELECT id FROM devices WHERE account_id = $1)",
    )
    .bind(device.account_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "DELETE FROM enrolment_messages WHERE device_id IN \
            (SELECT id FROM devices WHERE account_id = $1)",
    )
    .bind(device.account_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(device.respond(KeyChanged {
        key_version: next,
        old_key_notes: to_count(old_key_notes),
    }))
}

#[derive(Debug, Deserialize)]
pub struct KeysParams {
    #[serde(default)]
    after: i32,
}

/// `GET /v1/account/keys?after=V`: the calling device's copies of the keys
/// later than version `V`, oldest first.
pub async fn mine(
    State(state): State<AppState>,
    Approved(device): Approved,
    params: Result<Query<KeysParams>, QueryRejection>,
) -> ApiResult<Response> {
    let Query(params) = params.map_err(|error| ApiError::invalid(error.body_text()))?;
    let rows: Vec<(i32, Vec<u8>)> = sqlx::query_as(
        "SELECT key_version, wrapped FROM device_keys \
          WHERE device_id = $1 AND key_version > $2 ORDER BY key_version",
    )
    .bind(device.id)
    .bind(params.after)
    .fetch_all(&state.pool)
    .await?;
    let keys = rows
        .into_iter()
        .map(|(key_version, wrapped)| WrappedKey {
            key_version,
            wrapped,
        })
        .collect();
    Ok(device.respond(DeviceKeys { keys }))
}

/// `GET /v1/account/key/notes`: notes still stored under an older key, for
/// a device to send again under the new one.
pub async fn old_notes(
    State(state): State<AppState>,
    Approved(device): Approved,
) -> ApiResult<Response> {
    let (key_version, total): (i32, i64) =
        sqlx::query_as("SELECT key_version, old_key_notes FROM accounts WHERE id = $1")
            .bind(device.account_id)
            .fetch_one(&state.pool)
            .await?;
    let rows: Vec<(Uuid,)> = if total > 0 {
        sqlx::query_as(
            "SELECT note_id FROM notes \
              WHERE account_id = $1 AND key_version < $2 AND ciphertext IS NOT NULL \
              ORDER BY change_number LIMIT $3",
        )
        .bind(device.account_id)
        .bind(key_version)
        .bind(i64::try_from(limits::MAX_BATCH_ITEMS).unwrap_or(i64::MAX))
        .fetch_all(&state.pool)
        .await?
    } else {
        Vec::new()
    };
    Ok(device.respond(OldKeyNotes {
        key_version,
        notes: rows
            .into_iter()
            .map(|(id,)| NoteId::from_uuid(id))
            .collect(),
        total: to_count(total),
    }))
}
