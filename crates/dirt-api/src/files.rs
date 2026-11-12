//! Files attached to notes.
//!
//! A file is encrypted on a device and stored as one object. The server
//! holds neither its bytes nor its name nor its key. What it keeps is a
//! row saying that the file exists and how large it is, and a list of the
//! notes that refer to it.
//!
//! The server cannot read a note, so it cannot tell which files a note
//! refers to. Devices tell it. A file is kept for as long as any note
//! refers to it, and for thirty days after the last one stops: a device
//! that was away may yet come back with a note that needs it (design
//! section 10, and the step 9 plan's review).
//!
//! An object is never deleted in the request that decides it should go.
//! The decision is a date on the row, or a row in `orphan_objects`, and
//! [`sweep`] deletes what is due. So a storage that cannot be reached
//! delays a deletion and never loses one.

use std::collections::HashSet;

use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap};
use axum::response::Response;
use axum::Json;
use dirt_proto::api::{
    limits, DownloadLink, ErrorCode, FileOutcome, FileReference, FileReferences, FileResult,
    FileResults, UploadLink, UploadRequest,
};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::Approved;
use crate::error::{ApiError, ApiResult};
use crate::storage::{FileStore, StorageError};
use crate::{ApiJson, AppState};

/// How many expired files are deleted on the way to issuing an upload link.
const SWEEP_WITH_UPLOAD: i64 = 20;
/// How many the daily request deletes.
const SWEEP_DAILY: i64 = 500;

fn days(count: i64) -> i32 {
    i32::try_from(count).unwrap_or(i32::MAX)
}

fn unavailable(error: &StorageError) -> ApiError {
    tracing::error!("file storage: {error}");
    ApiError::new(
        ErrorCode::FilesUnavailable,
        "the file storage could not be reached",
    )
}

/// The storage, or the answer for a server that has none.
fn storage(state: &AppState) -> ApiResult<&FileStore> {
    state
        .files
        .as_deref()
        .ok_or_else(|| ApiError::new(ErrorCode::FilesUnavailable, "this server stores no files"))
}

/// `POST /v1/files`: a link to upload one file.
pub async fn upload(
    State(state): State<AppState>,
    Approved(device): Approved,
    ApiJson(request): ApiJson<UploadRequest>,
) -> ApiResult<Response> {
    let store = storage(&state)?;
    if request.size <= 0 {
        return Err(ApiError::invalid("a file has at least one byte"));
    }
    if request.size > limits::MAX_FILE_BYTES {
        return Err(ApiError::new(
            ErrorCode::FileTooLarge,
            format!("a file may be at most {} bytes", limits::MAX_FILE_BYTES),
        ));
    }
    // What the account no longer needs goes first, so that the space is
    // there for what it is about to add.
    sweep(
        &state.pool,
        store,
        Some(device.account_id),
        SWEEP_WITH_UPLOAD,
    )
    .await?;

    let mut tx = state.pool.begin().await?;
    // The account row is locked, so two uploads cannot both take the last
    // of the account's space.
    let (used,): (i64,) =
        sqlx::query_as("SELECT file_bytes FROM accounts WHERE id = $1 FOR UPDATE")
            .bind(device.account_id)
            .fetch_one(&mut *tx)
            .await?;
    let known: Option<(i64,)> =
        sqlx::query_as("SELECT size FROM files WHERE account_id = $1 AND file_id = $2")
            .bind(device.account_id)
            .bind(request.file_id)
            .fetch_optional(&mut *tx)
            .await?;
    match known {
        // Asked for again: the upload failed, or the storage lost the
        // object. The file is already counted.
        Some((size,)) if size == request.size => {}
        Some(_) => {
            return Err(ApiError::invalid(
                "this file was asked for at another size; a file never changes",
            ));
        }
        None => {
            if used + request.size > limits::MAX_ACCOUNT_FILE_BYTES {
                return Err(ApiError::new(
                    ErrorCode::QuotaExceeded,
                    "the account's file storage is full",
                ));
            }
            // If nothing is ever uploaded, or no note ever refers to it,
            // the row goes after a week and its space with it.
            sqlx::query(
                "INSERT INTO files (account_id, file_id, size, delete_after) \
                 VALUES ($1, $2, $3, now() + make_interval(days => $4))",
            )
            .bind(device.account_id)
            .bind(request.file_id)
            .bind(request.size)
            .bind(days(limits::UNCONFIRMED_FILE_DAYS))
            .execute(&mut *tx)
            .await?;
            sqlx::query("UPDATE accounts SET file_bytes = file_bytes + $2 WHERE id = $1")
                .bind(device.account_id)
                .bind(request.size)
                .execute(&mut *tx)
                .await?;
        }
    }
    tx.commit().await?;

    let key = FileStore::object_key(device.account_id, request.file_id);
    Ok(device.respond(UploadLink {
        url: store.upload_link(&key, request.size),
        expires_in_secs: limits::UPLOAD_LINK_SECS,
    }))
}

/// `GET /v1/files/{id}`: a link to download one file.
pub async fn download(
    State(state): State<AppState>,
    Approved(device): Approved,
    file: Result<Path<Uuid>, PathRejection>,
) -> ApiResult<Response> {
    let store = storage(&state)?;
    let no_such_file = || ApiError::new(ErrorCode::NotFound, "no such file");
    let Path(file) = file.map_err(|_| no_such_file())?;
    // The account is part of the question, so one account can never be
    // given a link to another's file, whatever ID it sends.
    let known: Option<(i64,)> =
        sqlx::query_as("SELECT size FROM files WHERE account_id = $1 AND file_id = $2")
            .bind(device.account_id)
            .bind(file)
            .fetch_optional(&state.pool)
            .await?;
    let (size,) = known.ok_or_else(no_such_file)?;
    let key = FileStore::object_key(device.account_id, file);
    Ok(device.respond(DownloadLink {
        url: store.download_link(&key),
        size,
        expires_in_secs: limits::DOWNLOAD_LINK_SECS,
    }))
}

/// A batch names each file and note together once, and is no larger than a
/// batch of notes.
fn check_batch(request: &FileReferences) -> ApiResult<()> {
    if request.items.len() > limits::MAX_BATCH_ITEMS {
        return Err(ApiError::new(
            ErrorCode::PayloadTooLarge,
            format!(
                "a request may name at most {} files",
                limits::MAX_BATCH_ITEMS
            ),
        ));
    }
    let mut seen = HashSet::with_capacity(request.items.len());
    if !request
        .items
        .iter()
        .all(|item| seen.insert((item.file_id, item.note_id.as_uuid())))
    {
        return Err(ApiError::invalid(
            "a request may name a file and a note together only once",
        ));
    }
    Ok(())
}

/// `POST /v1/files/confirm`: these notes refer to these files.
pub async fn confirm(
    State(state): State<AppState>,
    Approved(device): Approved,
    ApiJson(request): ApiJson<FileReferences>,
) -> ApiResult<Response> {
    let store = storage(&state)?;
    check_batch(&request)?;
    let mut results = Vec::with_capacity(request.items.len());
    for item in &request.items {
        results.push(FileResult {
            file_id: item.file_id,
            note_id: item.note_id,
            outcome: confirm_one(&state.pool, store, device.account_id, item).await?,
        });
    }
    Ok(device.respond(FileResults { results }))
}

async fn confirm_one(
    pool: &PgPool,
    store: &FileStore,
    account_id: Uuid,
    item: &FileReference,
) -> ApiResult<FileOutcome> {
    let note = item.note_id.as_uuid();
    let known: Option<(i64, bool)> =
        sqlx::query_as("SELECT size, confirmed FROM files WHERE account_id = $1 AND file_id = $2")
            .bind(account_id)
            .bind(item.file_id)
            .fetch_optional(pool)
            .await?;
    let Some((size, confirmed)) = known else {
        return Ok(FileOutcome::NoSuchFile);
    };
    if !confirmed {
        // The storage is asked before the file is ever counted as there.
        // A device may have asked for a link and uploaded nothing, or the
        // upload may have been cut short.
        let key = FileStore::object_key(account_id, item.file_id);
        let stored = store
            .size_of(&key)
            .await
            .map_err(|error| unavailable(&error))?;
        if stored != Some(size) {
            return Ok(FileOutcome::NotUploaded);
        }
    }

    let mut tx = pool.begin().await?;
    // The account row is locked, as it is for a purge. A note cannot be
    // purged between the check that it is there and the reference to it.
    sqlx::query("SELECT 1 FROM accounts WHERE id = $1 FOR UPDATE")
        .bind(account_id)
        .execute(&mut *tx)
        .await?;
    let (note_is_there,): (bool,) = sqlx::query_as(
        "SELECT EXISTS (SELECT 1 FROM notes \
                         WHERE account_id = $1 AND note_id = $2 AND ciphertext IS NOT NULL)",
    )
    .bind(account_id)
    .bind(note)
    .fetch_one(&mut *tx)
    .await?;
    if !note_is_there {
        return Ok(FileOutcome::NoSuchNote);
    }
    let (others,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM file_notes \
          WHERE account_id = $1 AND note_id = $2 AND file_id <> $3",
    )
    .bind(account_id)
    .bind(note)
    .bind(item.file_id)
    .fetch_one(&mut *tx)
    .await?;
    if others >= limits::MAX_FILES_PER_NOTE {
        return Ok(FileOutcome::Rejected {
            code: ErrorCode::QuotaExceeded,
            message: "the note has as many files as it may".to_owned(),
        });
    }
    // The row may have gone since it was read: discarded by a sweep.
    let kept = sqlx::query(
        "UPDATE files SET confirmed = true, delete_after = NULL \
          WHERE account_id = $1 AND file_id = $2",
    )
    .bind(account_id)
    .bind(item.file_id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if kept == 0 {
        return Ok(FileOutcome::NoSuchFile);
    }
    sqlx::query(
        "INSERT INTO file_notes (account_id, file_id, note_id) VALUES ($1, $2, $3) \
         ON CONFLICT DO NOTHING",
    )
    .bind(account_id)
    .bind(item.file_id)
    .bind(note)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(FileOutcome::Done)
}

/// `POST /v1/files/release`: these notes no longer refer to these files.
pub async fn release(
    State(state): State<AppState>,
    Approved(device): Approved,
    ApiJson(request): ApiJson<FileReferences>,
) -> ApiResult<Response> {
    storage(&state)?;
    check_batch(&request)?;
    let mut results = Vec::with_capacity(request.items.len());
    for item in &request.items {
        let mut tx = state.pool.begin().await?;
        let outcome = release_one(&mut tx, device.account_id, item).await?;
        tx.commit().await?;
        results.push(FileResult {
            file_id: item.file_id,
            note_id: item.note_id,
            outcome,
        });
    }
    Ok(device.respond(FileResults { results }))
}

async fn release_one(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    account_id: Uuid,
    item: &FileReference,
) -> ApiResult<FileOutcome> {
    let known: Option<(bool,)> =
        sqlx::query_as("SELECT true FROM files WHERE account_id = $1 AND file_id = $2 FOR UPDATE")
            .bind(account_id)
            .bind(item.file_id)
            .fetch_optional(&mut **tx)
            .await?;
    if known.is_none() {
        return Ok(FileOutcome::NoSuchFile);
    }
    sqlx::query("DELETE FROM file_notes WHERE account_id = $1 AND file_id = $2 AND note_id = $3")
        .bind(account_id)
        .bind(item.file_id)
        .bind(item.note_id.as_uuid())
        .execute(&mut **tx)
        .await?;
    mark_unreferenced(tx, account_id, &[item.file_id]).await?;
    Ok(FileOutcome::Done)
}

/// Starts the last thirty days of each of these files that no note refers
/// to any more. A file that already has a date keeps it.
async fn mark_unreferenced(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    account_id: Uuid,
    files: &[Uuid],
) -> ApiResult<()> {
    sqlx::query(
        "UPDATE files f \
            SET delete_after = now() + make_interval(days => $3) \
          WHERE f.account_id = $1 AND f.file_id = ANY($2) AND f.delete_after IS NULL \
            AND NOT EXISTS (SELECT 1 FROM file_notes r \
                             WHERE r.account_id = f.account_id AND r.file_id = f.file_id)",
    )
    .bind(account_id)
    .bind(files)
    .bind(days(limits::UNREFERENCED_FILE_DAYS))
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// A note has been purged: it refers to nothing any more.
///
/// Called in the transaction that purges the note. The files are not
/// deleted here. A device that was away with an unsent edit keeps the
/// note's text as a new note, and confirms the files for that.
pub async fn forget_note(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    account_id: Uuid,
    note: Uuid,
) -> ApiResult<()> {
    let files: Vec<(Uuid,)> = sqlx::query_as(
        "DELETE FROM file_notes WHERE account_id = $1 AND note_id = $2 RETURNING file_id",
    )
    .bind(account_id)
    .bind(note)
    .fetch_all(&mut **tx)
    .await?;
    if files.is_empty() {
        return Ok(());
    }
    let files: Vec<Uuid> = files.into_iter().map(|(file,)| file).collect();
    mark_unreferenced(tx, account_id, &files).await
}

/// An account is being deleted or reset: its files' rows go with it, and
/// the objects are queued to be deleted.
pub async fn orphan_account(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    account_id: Uuid,
) -> ApiResult<()> {
    sqlx::query(
        "INSERT INTO orphan_objects (object_key) \
         SELECT account_id::text || '/' || file_id::text FROM files WHERE account_id = $1 \
         ON CONFLICT DO NOTHING",
    )
    .bind(account_id)
    .execute(&mut **tx)
    .await?;
    sqlx::query("DELETE FROM files WHERE account_id = $1")
        .bind(account_id)
        .execute(&mut **tx)
        .await?;
    sqlx::query("UPDATE accounts SET file_bytes = 0 WHERE id = $1")
        .bind(account_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Deletes up to `limit` files whose time has come, of one account or of
/// all, and as many objects whose accounts are gone. Returns how many
/// objects were deleted.
///
/// The row is taken first, inside a transaction, and the object deleted
/// before the transaction ends. If the storage cannot be reached the
/// transaction is given up, the row stays, and the file is tried again next
/// time. A device that confirms the file in that moment waits for the row,
/// finds it gone, and uploads the file again.
pub async fn sweep(
    pool: &PgPool,
    store: &FileStore,
    account: Option<Uuid>,
    limit: i64,
) -> ApiResult<usize> {
    let due: Vec<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT account_id, file_id FROM files \
          WHERE delete_after < now() AND ($1::uuid IS NULL OR account_id = $1) \
          ORDER BY delete_after LIMIT $2",
    )
    .bind(account)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    let mut deleted = 0;
    for (account_id, file_id) in due {
        let mut tx = pool.begin().await?;
        // Still due: a device may have confirmed the file for a note since
        // the list was read. Then it is no longer ours to delete.
        let gone: Option<(i64,)> = sqlx::query_as(
            "DELETE FROM files \
              WHERE account_id = $1 AND file_id = $2 AND delete_after < now() \
          RETURNING size",
        )
        .bind(account_id)
        .bind(file_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some((size,)) = gone else {
            continue;
        };
        let key = FileStore::object_key(account_id, file_id);
        if let Err(error) = store.delete(&key).await {
            // Dropping the transaction puts the row back.
            tracing::warn!("an expired file was not deleted and will be tried again: {error}");
            continue;
        }
        sqlx::query("UPDATE accounts SET file_bytes = GREATEST(file_bytes - $2, 0) WHERE id = $1")
            .bind(account_id)
            .bind(size)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        deleted += 1;
    }

    if account.is_none() {
        let orphans: Vec<(String,)> =
            sqlx::query_as("SELECT object_key FROM orphan_objects ORDER BY created_at LIMIT $1")
                .bind(limit)
                .fetch_all(pool)
                .await?;
        for (key,) in orphans {
            if let Err(error) = store.delete(&key).await {
                tracing::warn!("an orphaned file was not deleted and will be tried again: {error}");
                continue;
            }
            sqlx::query("DELETE FROM orphan_objects WHERE object_key = $1")
                .bind(&key)
                .execute(pool)
                .await?;
            deleted += 1;
        }
    }
    Ok(deleted)
}

/// Compares two secrets without stopping at the first byte that differs.
fn same_secret(presented: &str, expected: &str) -> bool {
    let (a, b) = (Sha256::digest(presented), Sha256::digest(expected));
    a.iter()
        .zip(b.iter())
        .fold(0u8, |sum, (x, y)| sum | (x ^ y))
        == 0
}

/// `GET /v1/maintenance/sweep`: deletes what is due, for every account.
///
/// The hosting platform asks once a day, and presents the secret it was
/// configured with. To anyone else there is no such address.
pub async fn sweep_all(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    let not_found = || ApiError::new(ErrorCode::NotFound, "no such endpoint");
    let expected = state.config.cron_secret.as_deref().ok_or_else(not_found)?;
    let presented = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or_else(not_found)?;
    if !same_secret(presented, expected) {
        return Err(not_found());
    }
    let store = storage(&state)?;
    let deleted = sweep(&state.pool, store, None, SWEEP_DAILY).await?;
    Ok(Json(serde_json::json!({ "deleted": deleted })))
}
