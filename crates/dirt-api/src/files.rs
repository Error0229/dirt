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
//! The decision is a date on the file's row. When the date comes, [`sweep`]
//! removes the row and puts the object's key in `orphan_objects`, in one
//! transaction; deleting the object from the storage is a second step,
//! tried until it works. The database is never left saying that a file is
//! there when its object is gone, and a storage that cannot be reached
//! delays a deletion and never loses one.
//!
//! Everything here that touches an account's files takes the account's row
//! lock first. So a file's row is never looked at by two requests at once,
//! and no two of them can wait on each other.

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
/// How long an object waits in the queue before it is deleted from the
/// storage.
///
/// A link to upload it may have been issued just before its row went, and
/// a device may still be sending through it. The storage knows nothing of
/// the queue: an upload that finished after the object was deleted would
/// put it back, with no row and no place in the queue to say it is there.
/// So the object is deleted only when no link for it can still work and no
/// upload begun under one can still be running: four times the life of a
/// link.
const OBJECT_WAIT_SECS: u32 = 4 * limits::UPLOAD_LINK_SECS;

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

/// Locks the account's row for the rest of the transaction.
///
/// Every request that reads or changes an account's files does this before
/// anything else, as a push and a purge do for its notes. A link is issued,
/// a file confirmed and an object deleted one at a time for one account,
/// always in the order account first and file second.
async fn lock_account(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    account_id: Uuid,
) -> ApiResult<bool> {
    let row: Option<(bool,)> = sqlx::query_as("SELECT true FROM accounts WHERE id = $1 FOR UPDATE")
        .bind(account_id)
        .fetch_optional(&mut **tx)
        .await?;
    Ok(row.is_some())
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
    // The file may have been here before: discarded, or lost when the
    // account was reset, with its object still waiting to be deleted. A
    // file's ID is its place in the storage, so that deletion would take
    // the new upload. It is called off: the object is this row's now.
    let key = FileStore::object_key(device.account_id, request.file_id);
    sqlx::query("DELETE FROM orphan_objects WHERE object_key = $1")
        .bind(&key)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

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
    let mut tx = pool.begin().await?;
    // The account row is locked, as it is for a purge and for a sweep. The
    // note cannot be purged, nor the file's object deleted, between the
    // checks below and the reference to it.
    lock_account(&mut tx, account_id).await?;
    let known: Option<(i64, bool)> =
        sqlx::query_as("SELECT size, confirmed FROM files WHERE account_id = $1 AND file_id = $2")
            .bind(account_id)
            .bind(item.file_id)
            .fetch_optional(&mut *tx)
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
    sqlx::query(
        "UPDATE files SET confirmed = true, delete_after = NULL \
          WHERE account_id = $1 AND file_id = $2",
    )
    .bind(account_id)
    .bind(item.file_id)
    .execute(&mut *tx)
    .await?;
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
    lock_account(tx, account_id).await?;
    let known: Option<(bool,)> =
        sqlx::query_as("SELECT true FROM files WHERE account_id = $1 AND file_id = $2")
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
    lock_account(tx, account_id).await?;
    sqlx::query(
        "INSERT INTO orphan_objects (object_key) \
         SELECT account_id::text || '/' || file_id::text FROM files WHERE account_id = $1 \
         ON CONFLICT (object_key) DO UPDATE SET created_at = now()",
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
/// all, and as many objects that no row refers to any more. Returns how
/// many objects were deleted from the storage.
///
/// It is done in two steps, because the database and the storage cannot be
/// changed together.
///
/// 1. A file that is due has its row removed and its object's key queued,
///    in one transaction. From here on the server says the file is gone,
///    which is true whether or not step 2 has happened.
/// 2. A queued object is deleted from the storage, and then its place in
///    the queue, once it has waited an hour (see [`OBJECT_WAIT_SECS`]). If
///    the storage cannot be reached the key stays queued and is tried
///    again. If the transaction fails after the object has gone, the key
///    is still queued, and deleting an object that is not there succeeds.
///
/// Both steps hold the account's lock. A device that asks to upload the
/// same file again takes the key out of the queue (see [`upload`]), under
/// the same lock, so step 2 never deletes an object that a row has taken
/// over.
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
    for (account_id, file_id) in due {
        let mut tx = pool.begin().await?;
        lock_account(&mut tx, account_id).await?;
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
        // A key that is somehow queued already starts its wait again.
        sqlx::query(
            "INSERT INTO orphan_objects (object_key) VALUES ($1) \
             ON CONFLICT (object_key) DO UPDATE SET created_at = now()",
        )
        .bind(FileStore::object_key(account_id, file_id))
        .execute(&mut *tx)
        .await?;
        sqlx::query("UPDATE accounts SET file_bytes = GREATEST(file_bytes - $2, 0) WHERE id = $1")
            .bind(account_id)
            .bind(size)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
    }

    // An object's key begins with its account, so one account's objects
    // are those under its prefix.
    let prefix = account.map(|account| format!("{account}/"));
    let orphans: Vec<(String,)> = sqlx::query_as(
        "SELECT object_key FROM orphan_objects \
          WHERE ($1::text IS NULL OR starts_with(object_key, $1)) \
            AND created_at < now() - make_interval(secs => $3) \
          ORDER BY created_at LIMIT $2",
    )
    .bind(&prefix)
    .bind(limit)
    .bind(f64::from(OBJECT_WAIT_SECS))
    .fetch_all(pool)
    .await?;
    let mut deleted = 0;
    for (key,) in orphans {
        if delete_waiting_object(pool, store, &key).await? {
            deleted += 1;
        }
    }
    Ok(deleted)
}

/// Deletes one queued object from the storage, if it has waited long
/// enough, and says whether it did.
///
/// The wait is asked about here, under the account's lock, and not only
/// when the queue was listed. Between the two, the file may have been asked
/// for again, which takes the key out of the queue and issues a new link,
/// and the account reset, which puts the key back. The entry that is there
/// now is a new one, and its hour has only begun.
pub async fn delete_waiting_object(pool: &PgPool, store: &FileStore, key: &str) -> ApiResult<bool> {
    let mut tx = pool.begin().await?;
    // The account may be gone, which is why its objects are here. If it is
    // not, its lock keeps an upload of this very file waiting until the
    // object has been dealt with.
    if let Some(account_id) = key.split('/').next().and_then(|id| id.parse().ok()) {
        lock_account(&mut tx, account_id).await?;
    }
    let taken = sqlx::query(
        "DELETE FROM orphan_objects \
          WHERE object_key = $1 AND created_at < now() - make_interval(secs => $2)",
    )
    .bind(key)
    .bind(f64::from(OBJECT_WAIT_SECS))
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if taken == 0 {
        // A new upload of the file called the deletion off, or the key was
        // queued afresh and has not waited yet.
        return Ok(false);
    }
    if let Err(error) = store.delete(key).await {
        // Dropping the transaction leaves the key queued.
        tracing::warn!("an object was not deleted and will be tried again: {error}");
        return Ok(false);
    }
    tx.commit().await?;
    Ok(true)
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
