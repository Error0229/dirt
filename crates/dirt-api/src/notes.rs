//! Storing and serving encrypted note blobs.
//!
//! The server cannot read a blob. It keeps one per note, replaces it only
//! when the writer saw the version being replaced, and numbers every
//! accepted write from one counter per account. That number is the note's
//! version and the pull cursor in one, so the protocol contains no clocks.

use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use axum::response::Response;
use dirt_proto::api::{
    limits, ErrorCode, NoteBlob, PullResponse, PushItem, PushOutcome, PushRequest, PushResponse,
    PushResult,
};
use dirt_proto::NoteId;
use serde::Deserialize;
use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::Approved;
use crate::error::{ApiError, ApiResult};
use crate::{ApiJson, AppState};

fn to_len(length: usize) -> i64 {
    i64::try_from(length).unwrap_or(i64::MAX)
}

fn rejected(code: ErrorCode, message: &str) -> PushOutcome {
    PushOutcome::Rejected {
        code,
        message: message.to_owned(),
    }
}

/// Stores one note if the writer saw the version it replaces.
///
/// The first statement locks the account row. Every push to one account
/// therefore runs one at a time, in the order the locks are granted, and the
/// change numbers come out in that same order with no gaps: a push that is
/// refused rolls back and uses no number.
async fn push_one(pool: &PgPool, account_id: Uuid, item: &PushItem) -> ApiResult<PushOutcome> {
    let note = item.note_id.as_uuid();
    let new_size = to_len(item.ciphertext.len());
    if item.ciphertext.len() > limits::MAX_NOTE_BYTES {
        return Ok(rejected(ErrorCode::NoteTooLarge, "the note is too large"));
    }
    if item.base_change < 0 {
        return Err(ApiError::invalid("a base change number cannot be negative"));
    }

    let mut tx = pool.begin().await?;
    let (counter, note_count, bytes_used): (i64, i64, i64) = sqlx::query_as(
        "SELECT change_counter, note_count, bytes_used FROM accounts WHERE id = $1 FOR UPDATE",
    )
    .bind(account_id)
    .fetch_one(&mut *tx)
    .await?;

    let existing: Option<(i64, i32, i32, Option<Vec<u8>>)> = sqlx::query_as(
        "SELECT change_number, key_version, format_version, ciphertext \
           FROM notes WHERE account_id = $1 AND note_id = $2",
    )
    .bind(account_id)
    .bind(note)
    .fetch_optional(&mut *tx)
    .await?;

    let old_size = match existing {
        // The client believes the note is new and it is.
        None if item.base_change == 0 => None,
        // The client names a version of a note the server has never had.
        None => return Ok(PushOutcome::Stale { current: None }),
        Some((change, _, _, ref blob)) if change == item.base_change => {
            Some(to_len(blob.as_ref().map_or(0, Vec::len)))
        }
        // Somebody else wrote first. Hand back what they wrote, so the
        // client can merge and push again without a separate pull.
        Some((change, key_version, format_version, ciphertext)) => {
            return Ok(PushOutcome::Stale {
                current: Some(NoteBlob {
                    note_id: item.note_id,
                    change,
                    key_version,
                    format_version,
                    ciphertext,
                }),
            });
        }
    };

    if old_size.is_none() && note_count >= limits::MAX_NOTES_PER_ACCOUNT {
        return Ok(rejected(
            ErrorCode::QuotaExceeded,
            "the account holds as many notes as it may",
        ));
    }
    let growth = new_size - old_size.unwrap_or(0);
    let new_total = bytes_used + growth;
    if new_total > limits::MAX_ACCOUNT_BYTES {
        // A full account still takes small growth, up to a hard ceiling.
        // The server cannot tell a delete from an edit, and a delete adds a
        // small marker to the document, so this is what lets deletes through.
        let ceiling = limits::MAX_ACCOUNT_BYTES / 100 * limits::ACCOUNT_CEILING_PERCENT;
        if growth > limits::FULL_ACCOUNT_GROWTH_BYTES || new_total > ceiling {
            return Ok(rejected(
                ErrorCode::QuotaExceeded,
                "the account's storage is full",
            ));
        }
    }

    let change = counter + 1;
    sqlx::query(
        "INSERT INTO notes (account_id, note_id, change_number, key_version, format_version, ciphertext) \
         VALUES ($1, $2, $3, $4, $5, $6) \
         ON CONFLICT (account_id, note_id) DO UPDATE \
            SET change_number = EXCLUDED.change_number, \
                key_version = EXCLUDED.key_version, \
                format_version = EXCLUDED.format_version, \
                ciphertext = EXCLUDED.ciphertext",
    )
    .bind(account_id)
    .bind(note)
    .bind(change)
    .bind(item.key_version)
    .bind(item.format_version)
    .bind(&item.ciphertext)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE accounts SET change_counter = $2, note_count = $3, bytes_used = $4 WHERE id = $1",
    )
    .bind(account_id)
    .bind(change)
    .bind(note_count + i64::from(old_size.is_none()))
    .bind(new_total)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(PushOutcome::Accepted { change })
}

/// `POST /v1/notes/push`
pub async fn push(
    State(state): State<AppState>,
    Approved(device): Approved,
    ApiJson(request): ApiJson<PushRequest>,
) -> ApiResult<Response> {
    if request.items.len() > limits::MAX_BATCH_ITEMS {
        return Err(ApiError::new(
            ErrorCode::PayloadTooLarge,
            format!("a push may hold at most {} notes", limits::MAX_BATCH_ITEMS),
        ));
    }
    let batch_bytes: usize = request.items.iter().map(|item| item.ciphertext.len()).sum();
    if batch_bytes > limits::MAX_BATCH_BYTES {
        return Err(ApiError::new(
            ErrorCode::PayloadTooLarge,
            format!(
                "a push may hold at most {} bytes of notes",
                limits::MAX_BATCH_BYTES
            ),
        ));
    }

    let mut results = Vec::with_capacity(request.items.len());
    for item in &request.items {
        results.push(PushResult {
            note_id: item.note_id,
            outcome: push_one(&state.pool, device.account_id, item).await?,
        });
    }
    Ok(device.respond(PushResponse {
        epoch: device.epoch,
        results,
    }))
}

#[derive(Deserialize)]
pub struct PullParams {
    #[serde(default)]
    after: i64,
}

/// `GET /v1/notes/pull?after=N`
///
/// A page holds notes in change order until their blobs add up to the batch
/// limit. The first note of a page is always included, whatever its size, so
/// a pull always makes progress.
pub async fn pull(
    State(state): State<AppState>,
    Approved(device): Approved,
    params: Result<Query<PullParams>, QueryRejection>,
) -> ApiResult<Response> {
    let Query(params) = params.map_err(|error| ApiError::invalid(error.body_text()))?;
    if params.after < 0 {
        return Err(ApiError::invalid("the cursor cannot be negative"));
    }
    let rows: Vec<(Uuid, i64, i32, i32, Option<Vec<u8>>)> = sqlx::query_as(
        "SELECT note_id, change_number, key_version, format_version, ciphertext FROM ( \
             SELECT *, \
                    coalesce(sum(coalesce(octet_length(ciphertext), 0)) OVER ( \
                        ORDER BY change_number \
                        ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING), 0) AS bytes_before \
               FROM notes \
              WHERE account_id = $1 AND change_number > $2 \
         ) page \
          WHERE bytes_before < $3 \
          ORDER BY change_number \
          LIMIT $4",
    )
    .bind(device.account_id)
    .bind(params.after)
    .bind(to_len(limits::MAX_BATCH_BYTES))
    .bind(to_len(limits::MAX_BATCH_ITEMS))
    .fetch_all(&state.pool)
    .await?;

    let next_after = rows.last().map_or(params.after, |row| row.1);
    let (more,): (bool,) = sqlx::query_as(
        "SELECT EXISTS (SELECT 1 FROM notes WHERE account_id = $1 AND change_number > $2)",
    )
    .bind(device.account_id)
    .bind(next_after)
    .fetch_one(&state.pool)
    .await?;

    let items = rows
        .into_iter()
        .map(
            |(id, change, key_version, format_version, ciphertext)| NoteBlob {
                note_id: NoteId::from_uuid(id),
                change,
                key_version,
                format_version,
                ciphertext,
            },
        )
        .collect();
    Ok(device.respond(PullResponse {
        epoch: device.epoch,
        items,
        next_after,
        more,
    }))
}
