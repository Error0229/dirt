//! One sync cycle: pull, decrypt and merge, then encrypt and push.
//!
//! A cycle is safe to cut off at any point and to run again. Merging the
//! same document twice changes nothing, the pull cursor is saved after each
//! page, and a note is marked as sent only for the revision that was sent.
//!
//! Failures fall into four classes (see `dirt_proto::api::ErrorClass`). A
//! problem with one note flags that note and the cycle carries on. Any other
//! failure ends the cycle with an error whose class tells the caller what to
//! do.

use dirt_proto::api::{
    limits, ErrorClass, ErrorCode, NoteBlob, PushItem, PushOutcome, PushRequest,
};
use dirt_proto::NoteId;
use uuid::Uuid;

use crate::api::{ApiClient, ApiError};
use crate::crypto::{
    decrypt_note, encrypt_note, AccountKey, BlobContext, CryptoError, FORMAT_VERSION, KEY_VERSION,
};
use crate::store::{ProblemKind, UnsentNote};
use crate::{Error, Store};

/// How many times a cycle pulls and pushes before giving up on notes that
/// keep coming back stale. Each round merges what the server returned, so a
/// second round normally succeeds; more are needed only while other devices
/// keep writing the same notes.
const MAX_ROUNDS: usize = 4;

/// What an encrypted blob adds to a document: the layout byte, the nonce and
/// the authentication tag.
const BLOB_OVERHEAD: usize = 1 + 24 + 16;

/// What one cycle did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SyncReport {
    /// Documents pulled from the server and merged.
    pub pulled: usize,
    /// Notes the server accepted.
    pub pushed: usize,
    /// Pushes the server refused as stale. Each was merged and retried.
    pub stale: usize,
    /// Notes flagged during this cycle as not syncable.
    pub problems: usize,
    /// Notes still unsent when the cycle ended.
    pub unsent: usize,
}

/// Why a cycle ended early.
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error(transparent)]
    Api(#[from] ApiError),
    #[error(transparent)]
    Store(#[from] Error),
}

impl SyncError {
    /// What the caller should do. A local database failure is treated as
    /// "try again later": nothing was lost, and nothing can be done about it
    /// from here.
    #[must_use]
    pub const fn class(&self) -> ErrorClass {
        match self {
            Self::Api(error) => error.class(),
            Self::Store(_) => ErrorClass::TryAgainLater,
        }
    }
}

/// Syncs one account's store with the server.
pub struct SyncEngine {
    store: Store,
    api: ApiClient,
    account_id: Uuid,
    key: AccountKey,
}

/// What became of one pulled or returned blob.
enum Applied {
    Merged,
    /// The store already holds this version.
    AlreadyHere,
    /// The blob could not be used. The note is flagged with this reason.
    Problem(String),
}

impl SyncEngine {
    /// `api` must carry the device's token, and `key` must be the account's.
    pub const fn new(store: Store, api: ApiClient, account_id: Uuid, key: AccountKey) -> Self {
        Self {
            store,
            api,
            account_id,
            key,
        }
    }

    /// Runs one cycle and reports what it did.
    pub async fn run_once(&self) -> Result<SyncReport, SyncError> {
        let mut report = SyncReport::default();
        for _ in 0..MAX_ROUNDS {
            self.pull(&mut report).await?;
            let stale = self.push(&mut report).await?;
            if stale == 0 {
                break;
            }
        }
        report.unsent = self.store.unsent_count().await?;
        self.store
            .set_last_synced_ms(chrono::Utc::now().timestamp_millis())
            .await?;
        Ok(report)
    }

    /// Follows the account's epoch. Returns true if it changed, in which
    /// case the store has started over and the caller must too.
    async fn follow_epoch(&self, epoch: i32) -> Result<bool, SyncError> {
        if self.store.sync_progress().await?.epoch == epoch {
            return Ok(false);
        }
        self.store.begin_epoch(epoch).await?;
        Ok(true)
    }

    async fn pull(&self, report: &mut SyncReport) -> Result<(), SyncError> {
        loop {
            let cursor = self.store.sync_progress().await?.cursor;
            let page = self.api.pull(cursor).await?;
            if self.follow_epoch(page.epoch).await? {
                // The page was fetched with a cursor from the old epoch.
                continue;
            }
            for blob in &page.items {
                self.apply(blob, report).await?;
            }
            self.store.set_sync_cursor(page.next_after).await?;
            if !page.more {
                return Ok(());
            }
        }
    }

    /// Decrypts one blob from the server and merges it into the store. A
    /// blob that cannot be used flags its note and is otherwise skipped.
    async fn apply(&self, blob: &NoteBlob, report: &mut SyncReport) -> Result<(), SyncError> {
        match self.try_apply(blob).await? {
            Applied::Merged => report.pulled += 1,
            Applied::AlreadyHere => {}
            Applied::Problem(reason) => {
                // Held: this device must not push over a copy it could not
                // read.
                self.flag(&blob.note_id, &reason, ProblemKind::Held, report)
                    .await?;
            }
        }
        Ok(())
    }

    async fn try_apply(&self, blob: &NoteBlob) -> Result<Applied, SyncError> {
        // A change number belongs to one version of one note. If this device
        // recorded it, the device either pushed that version or has merged
        // it already, and need not read the blob again. This is also what
        // lets the device that wrote a version repair the server's copy if
        // that copy is later damaged: it is not held by a blob it cannot
        // read when it already has what the blob held.
        if self.store.note_server_change(&blob.note_id).await? == Some(blob.change) {
            return Ok(Applied::AlreadyHere);
        }
        let Some(ciphertext) = &blob.ciphertext else {
            // The server purged the note. Nothing purges yet, so there is
            // nothing to merge and nothing to decide.
            return Ok(Applied::Merged);
        };
        if blob.format_version > FORMAT_VERSION {
            return Ok(Applied::Problem(
                "written by a newer version of Dirt; update the app to sync this note".to_owned(),
            ));
        }
        if blob.key_version != KEY_VERSION {
            return Ok(Applied::Problem(
                "encrypted with a key this device does not have".to_owned(),
            ));
        }
        let context = BlobContext {
            account_id: self.account_id,
            note_id: blob.note_id,
            key_version: blob.key_version,
            format_version: blob.format_version,
        };
        let document = match decrypt_note(&self.key, &context, ciphertext) {
            Ok(document) => document,
            Err(CryptoError::UnknownLayout) => {
                return Ok(Applied::Problem(
                    "stored in a layout this version of Dirt cannot read".to_owned(),
                ));
            }
            Err(_) => {
                return Ok(Applied::Problem(
                    "the server's copy could not be decrypted".to_owned(),
                ));
            }
        };
        match self
            .store
            .apply_remote_note(&blob.note_id, document, blob.change)
            .await
        {
            Ok(_) => Ok(Applied::Merged),
            Err(error @ (Error::Replay { .. } | Error::Document(_))) => {
                Ok(Applied::Problem(error.to_string()))
            }
            Err(other) => Err(other.into()),
        }
    }

    /// Pushes every unsent note once and returns how many came back stale.
    async fn push(&self, report: &mut SyncReport) -> Result<usize, SyncError> {
        let mut stale = 0;
        let mut after = 0;
        loop {
            // Leave room for what encryption adds to each document.
            let room = limits::MAX_BATCH_BYTES - limits::MAX_BATCH_ITEMS * BLOB_OVERHEAD;
            let batch = self
                .store
                .unsent_notes(after, limits::MAX_BATCH_ITEMS, room)
                .await?;
            let Some(last) = batch.last() else {
                return Ok(stale);
            };
            after = last.position;

            let mut sent: Vec<&UnsentNote> = Vec::with_capacity(batch.len());
            let mut items = Vec::with_capacity(batch.len());
            for note in &batch {
                if note.document.len() + BLOB_OVERHEAD > limits::MAX_NOTE_BYTES {
                    self.flag(
                        &note.id,
                        "too large to sync",
                        ProblemKind::RetryAfterEdit,
                        report,
                    )
                    .await?;
                    continue;
                }
                let context = BlobContext::current(self.account_id, note.id);
                items.push(PushItem {
                    note_id: note.id,
                    base_change: note.base_change,
                    key_version: context.key_version,
                    format_version: context.format_version,
                    ciphertext: encrypt_note(&self.key, &context, &note.document),
                });
                sent.push(note);
            }
            if items.is_empty() {
                continue;
            }

            let response = self.api.push(&PushRequest { items }).await?;
            if self.follow_epoch(response.epoch).await? {
                // The store has started over. What was just accepted belongs
                // to the new epoch, but its base numbers did not; the next
                // round pulls and pushes everything afresh.
                return Ok(stale + 1);
            }
            for result in &response.results {
                // Results are matched to what was sent by note, not by
                // position, so an answer that is short, reordered or about
                // some other note cannot mark the wrong note as sent.
                let Some(note) = sent.iter().find(|note| note.id == result.note_id) else {
                    continue;
                };
                match &result.outcome {
                    PushOutcome::Accepted { change } => {
                        self.store
                            .mark_note_pushed(&note.id, note.revision, *change)
                            .await?;
                        report.pushed += 1;
                    }
                    PushOutcome::Stale {
                        current_change,
                        current,
                    } => {
                        stale += 1;
                        report.stale += 1;
                        match (current, current_change) {
                            // Merge what the server holds; the next round
                            // pushes the result on top of it.
                            (Some(blob), _) => self.apply(blob, report).await?,
                            // The server holds a newer version it had no
                            // room to return. The next round's pull brings it.
                            (None, Some(_)) => {}
                            // The server has no such note, though this device
                            // once saw it there. Offer it as new.
                            (None, None) => {
                                self.store.forget_note_server_version(&note.id).await?;
                            }
                        }
                    }
                    PushOutcome::Rejected { code, message } => {
                        let reason = match code {
                            ErrorCode::NoteTooLarge => "too large to sync".to_owned(),
                            ErrorCode::QuotaExceeded => "the account's storage is full".to_owned(),
                            _ => format!("refused by the server: {message}"),
                        };
                        self.flag(&note.id, &reason, ProblemKind::RetryAfterEdit, report)
                            .await?;
                    }
                }
            }
        }
    }

    async fn flag(
        &self,
        note: &NoteId,
        reason: &str,
        kind: ProblemKind,
        report: &mut SyncReport,
    ) -> Result<(), SyncError> {
        tracing::warn!("note {note} cannot be synced: {reason}");
        self.store
            .flag_note_problem(note, reason.to_owned(), kind)
            .await?;
        report.problems += 1;
        Ok(())
    }
}
