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
    first_key_version, limits, ChangeKey, DeviceKeys, ErrorCode, KeyChanged, OldKeyNotes,
    WrappedKey,
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

/// Locks the account's row for the rest of the transaction and returns the
/// version its notes are written under.
///
/// A change of the key holds this lock while it looks at which devices are
/// approved and stores a copy for each. Whatever approves a device, or
/// leaves a copy of the key for one, takes the same lock, so the two happen
/// one after the other. Without it a device approved while a change was
/// being made could be left without the new key.
pub async fn lock_account(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    account_id: Uuid,
) -> ApiResult<i32> {
    let (current,): (i32,) =
        sqlx::query_as("SELECT key_version FROM accounts WHERE id = $1 FOR UPDATE")
            .bind(account_id)
            .fetch_one(&mut **tx)
            .await?;
    Ok(current)
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
    let current = lock_account(tx, account_id).await?;
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
    // The account row is locked, as it is for a push and for approving a
    // device, so a key change and what happens around it go one at a time.
    let current = lock_account(&mut tx, device.account_id).await?;
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
    // A device opens the copies it missed one after another, so a copy is
    // of use only with every copy before it. Those too far back to be
    // reached are dropped, which bounds what an account's key changes can
    // leave behind. A device that far behind is let in again as a new one.
    sqlx::query(
        "DELETE FROM device_keys \
          WHERE key_version <= $2 \
            AND device_id IN (SELECT id FROM devices WHERE account_id = $1)",
    )
    .bind(device.account_id)
    .bind(next - limits::MAX_KEPT_KEY_VERSIONS)
    .execute(&mut *tx)
    .await?;
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
    // One statement, so that the account's version and the copies are read
    // as they stood at one moment.
    let rows: Vec<(i32, Option<i32>, Option<Vec<u8>>)> = sqlx::query_as(
        "SELECT a.key_version, k.key_version, k.wrapped \
           FROM accounts a \
           LEFT JOIN device_keys k ON k.device_id = $2 AND k.key_version > $3 \
          WHERE a.id = $1 ORDER BY k.key_version",
    )
    .bind(device.account_id)
    .bind(device.id)
    .bind(params.after)
    .fetch_all(&state.pool)
    .await?;
    let present = rows.first().map_or(first_key_version(), |row| row.0);
    let keys: Vec<WrappedKey> = rows
        .into_iter()
        .filter_map(|(_, key_version, wrapped)| {
            Some(WrappedKey {
                key_version: key_version?,
                wrapped: wrapped?,
            })
        })
        .collect();
    if !can_follow(params.after, present, keys.first()) && wait_again(&state, &device).await? {
        return Err(ApiError::new(
            ErrorCode::DevicePending,
            "this device missed too many changes of the account's key; \
             sign in again and let it in from another device",
        ));
    }
    Ok(device.respond(DeviceKeys { keys }))
}

/// Whether a device holding the key of version `held` can reach the
/// account's present key with the copies the server has for it.
///
/// Each copy opens with the key before it, so the device needs the copy
/// that follows its own key. The first version has no copy: a device
/// never asks for it.
fn can_follow(held: i32, present: i32, next_copy: Option<&WrappedKey>) -> bool {
    let held = held.max(first_key_version());
    present <= held || next_copy.is_some_and(|copy| copy.key_version == held + 1)
}

/// Puts an approved device that cannot follow the account's key back among
/// those waiting to be let in, and says whether it did.
///
/// The copy the device needs is gone: more changes of the key were made
/// while it was away than the server keeps copies for. It is still the
/// account's device, with notes of its own that may not have been sent, so
/// it is not removed. It waits, as a new device does, for another device
/// or the recovery phrase to give it the present key. Signing in again
/// shows it that it is waiting.
///
/// The account is locked and the copies read again, so that a change of
/// the key made at the same moment is not mistaken for a gap.
async fn wait_again(state: &AppState, device: &crate::auth::Device) -> ApiResult<bool> {
    let mut tx = state.pool.begin().await?;
    let present = lock_account(&mut tx, device.account_id).await?;
    let (newest,): (Option<i32>,) =
        sqlx::query_as("SELECT max(key_version) FROM device_keys WHERE device_id = $1")
            .bind(device.id)
            .fetch_one(&mut *tx)
            .await?;
    // Every change of the key leaves a copy for each approved device, so
    // a device that was away through the changes holds a copy of the
    // present key. One that holds none was let in after the last change
    // and has nothing to follow: whatever it asked for, it is left alone.
    if newest != Some(present) {
        return Ok(false);
    }
    let waiting = sqlx::query(
        "UPDATE devices SET status = 'pending', created_at = now() \
          WHERE id = $1 AND status = 'approved'",
    )
    .bind(device.id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    // Its copies are of no use to it now.
    sqlx::query("DELETE FROM device_keys WHERE device_id = $1")
        .bind(device.id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(waiting == 1)
}

#[derive(Debug, Deserialize)]
pub struct OldNotesParams {
    /// List notes whose last change is numbered after this.
    #[serde(default)]
    after: i64,
}

/// `GET /v1/account/key/notes?after=N`: notes still stored under an older
/// key, for a device to send again under the new one.
///
/// The list comes in pages. A device may be able to send none of a page:
/// one let in after an earlier change of the key holds none of the notes
/// still under the key before that. It goes on to the next page, where
/// there may be notes it can send.
pub async fn old_notes(
    State(state): State<AppState>,
    Approved(device): Approved,
    params: Result<Query<OldNotesParams>, QueryRejection>,
) -> ApiResult<Response> {
    let Query(params) = params.map_err(|error| ApiError::invalid(error.body_text()))?;
    if params.after < 0 {
        return Err(ApiError::invalid("the cursor cannot be negative"));
    }
    let (key_version, total): (i32, i64) =
        sqlx::query_as("SELECT key_version, old_key_notes FROM accounts WHERE id = $1")
            .bind(device.account_id)
            .fetch_one(&state.pool)
            .await?;
    let page = limits::MAX_BATCH_ITEMS;
    let rows: Vec<(Uuid, i64)> = if total > 0 {
        sqlx::query_as(
            "SELECT note_id, change_number FROM notes \
              WHERE account_id = $1 AND key_version < $2 AND ciphertext IS NOT NULL \
                AND change_number > $3 \
              ORDER BY change_number LIMIT $4",
        )
        .bind(device.account_id)
        .bind(key_version)
        .bind(params.after)
        .bind(i64::try_from(page).unwrap_or(i64::MAX))
        .fetch_all(&state.pool)
        .await?
    } else {
        Vec::new()
    };
    // A full page may be followed by another.
    let next_after = (rows.len() == page)
        .then(|| rows.last().map(|(_, change)| *change))
        .flatten();
    Ok(device.respond(OldKeyNotes {
        key_version,
        notes: rows
            .into_iter()
            .map(|(id, _)| NoteId::from_uuid(id))
            .collect(),
        total: to_count(total),
        next_after,
    }))
}
