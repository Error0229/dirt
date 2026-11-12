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
//!
//! # When the account's key changes
//!
//! A device changes the key after another was removed (`key_change`). The
//! engine follows a change without being told to:
//!
//! - A pull says which key version notes are written under. If that is
//!   newer than the engine's, it fetches this device's wrapped copies and
//!   opens them in order, each with the key before it.
//! - A note the server refuses as written under an old key stays unsent,
//!   the engine catches up, and the next round sends it under the new key.
//! - While notes under an old key remain on the server, the engine sends
//!   those it holds again under the new one. Any device does this, so a
//!   change does not depend on the device that began it.
//! - A wrapped copy that does not open was not made by a device of the
//!   account. The engine stops and adopts nothing.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use dirt_proto::api::{
    limits, ErrorClass, ErrorCode, NoteBlob, PushItem, PushOutcome, PushRequest,
};
use dirt_proto::NoteId;
use uuid::Uuid;

use crate::api::{ApiClient, ApiError};
use crate::crypto::{
    decrypt_note, encrypt_note, AccountKey, BlobContext, CryptoError, DeviceKeyPair, FORMAT_VERSION,
};
use crate::key_change::{unwrap_account_key, KeyRing, WrapContext};
use crate::store::{ProblemKind, UnsentNote};
use crate::{Error, Store};

/// How many times a cycle pulls and pushes before giving up on notes that
/// keep coming back stale. Each round merges what the server returned, so a
/// second round normally succeeds; more are needed only while other devices
/// keep writing the same notes.
const MAX_ROUNDS: usize = 4;

/// How many times a cycle goes round to send notes again under a new key.
/// Each round sends up to a batch of them, and an account holds at most
/// forty batches of notes. The limit is for a server that never counts
/// them down.
const MAX_RESEND_ROUNDS: usize = 64;

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
    /// Devices waiting to be approved, as the server last said.
    pub pending_devices: u32,
    /// When a reset that a waiting device asked for takes effect, if one is
    /// pending.
    pub reset_effective_at_ms: Option<i64>,
    /// The version of the account's key that notes are written under.
    pub key_version: i32,
    /// Notes still stored under an older key when the cycle ended. While
    /// there are any, a change of the account's key is not complete.
    pub old_key_notes: u64,
}

/// Why a cycle ended early.
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error(transparent)]
    Api(#[from] ApiError),
    #[error(transparent)]
    Store(#[from] Error),
    /// Another program on this device signed out, or replaced the account's
    /// key, since this engine was made. Syncing with what this engine holds
    /// would upload notes that the account's devices cannot read.
    #[error("this device's keys were changed by another program; they have to be read again")]
    Superseded,
    /// The account's key has changed and the server holds no copy of the
    /// new one that this device can open.
    #[error("the account's key was changed and this device could not get the new one")]
    KeyUnavailable,
    /// The copy of a new account key that the server gave this device was
    /// not made by a device of the account: it does not open with the key
    /// it claims to replace. Nothing was adopted.
    #[error("the server offered an account key that no device of this account made")]
    KeyRejected,
    /// A newer account key was opened but could not be saved on this
    /// device. It is not used until it has been.
    #[error("the account's new key could not be saved on this device: {0}")]
    KeysNotStored(String),
}

impl SyncError {
    /// What the caller should do. A local database failure is treated as
    /// "try again later": nothing was lost, and nothing can be done about it
    /// from here.
    #[must_use]
    pub const fn class(&self) -> ErrorClass {
        match self {
            Self::Api(error) => error.class(),
            // The session reads the keys again when the person comes back
            // to the app, and syncs with them.
            Self::Store(_) | Self::Superseded | Self::KeysNotStored(_) => ErrorClass::TryAgainLater,
            // Trying again will not help. Signing in again makes this a
            // new device, which another device lets in with the key the
            // account has now.
            Self::KeyUnavailable | Self::KeyRejected => ErrorClass::StopUntilSignedIn,
        }
    }
}

/// What secure storage says about the keys an engine holds.
pub enum StoredKeys {
    /// The device's keys as they are stored now.
    Keys(KeyRing),
    /// The device signed out, or signed in as someone else.
    Gone,
    /// Storage could not be read just now, which says nothing either way.
    Unknown,
}

/// Where an engine's keys are kept, and what it needs to open a new one.
///
/// The desktop app and the CLI on one computer are one device with one
/// place for its keys, so either may change what is stored while the other
/// runs. The engine asks before each cycle.
pub trait KeyKeeper: Send + Sync {
    /// The device's keys as secure storage holds them now.
    fn stored(&self) -> StoredKeys;

    /// Stores `to` as the device's keys in place of `from`. Called when
    /// the engine has opened a newer key, and when it no longer needs the
    /// older ones.
    ///
    /// `from` is what the engine believes is stored. If something else is,
    /// nothing may be written: the other program on this device reset the
    /// account or signed out while the engine was at work, and what it
    /// stored is the only copy of a new key, or the absence of one. The
    /// answer is then [`SyncError::Superseded`].
    fn keep(&self, from: &KeyRing, to: &KeyRing) -> Result<(), SyncError>;

    /// The device's ID and its own key pair, which open the copies of a
    /// new account key made for it.
    fn device(&self) -> (Uuid, DeviceKeyPair);
}

/// Syncs one account's store with the server.
pub struct SyncEngine {
    store: Store,
    api: ApiClient,
    account_id: Uuid,
    /// The account keys this engine reads and writes with.
    keys: Mutex<KeyRing>,
    /// How many documents this engine has merged into the store, over all
    /// its cycles, including cycles that went on to fail.
    merged: AtomicU64,
    /// Where the keys are kept. Without one the engine holds the key it
    /// was made with and cannot follow a change.
    keeper: Option<Arc<dyn KeyKeeper>>,
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
    /// `api` must carry the device's token, and `key` must be the account's
    /// first key. An engine made this way cannot follow a change of the
    /// key; see [`Self::kept_by`].
    pub fn new(store: Store, api: ApiClient, account_id: Uuid, key: AccountKey) -> Self {
        Self {
            store,
            api,
            account_id,
            keys: Mutex::new(KeyRing::first(key)),
            merged: AtomicU64::new(0),
            keeper: None,
        }
    }

    /// Gives the engine the device's keys and the place they are kept.
    ///
    /// The engine then asks `keeper` before every cycle whether its keys
    /// are still the device's, and refuses to sync if the device signed
    /// out or its key was replaced: the desktop app and the CLI share one
    /// device's keys. It also follows a change of the account's key, and
    /// stores each key it opens with `keeper`.
    #[must_use]
    pub fn kept_by(mut self, keys: KeyRing, keeper: Arc<dyn KeyKeeper>) -> Self {
        self.keys = Mutex::new(keys);
        self.keeper = Some(keeper);
        self
    }

    fn ring(&self) -> KeyRing {
        self.keys
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn set_ring(&self, ring: KeyRing) {
        *self.keys.lock().unwrap_or_else(PoisonError::into_inner) = ring;
    }

    /// Checks this engine's keys against what secure storage holds.
    fn check_keys(&self) -> Result<(), SyncError> {
        let Some(keeper) = &self.keeper else {
            return Ok(());
        };
        let stored = match keeper.stored() {
            StoredKeys::Keys(stored) => stored,
            StoredKeys::Gone => return Err(SyncError::Superseded),
            StoredKeys::Unknown => return Ok(()),
        };
        let mine = self.ring();
        if stored.same_as(&mine) {
            return Ok(());
        }
        // The other program on this device may have followed a change of
        // the key first, or let go of older keys first. What it stored is
        // then a later version, or this engine's own key with fewer older
        // ones beside it, and is simply taken: it is this device's own
        // secure storage, and the keys in it were opened by this device.
        //
        // Another key under the same version, or an earlier version, is a
        // reset: the versions began again. The engine stops, and the
        // session opens the account afresh.
        let later = stored.version() > mine.version();
        let same_key = stored.version() == mine.version()
            && stored.current().as_bytes() == mine.current().as_bytes();
        if later || same_key {
            self.set_ring(stored);
            Ok(())
        } else {
            Err(SyncError::Superseded)
        }
    }

    /// Fetches this device's copies of the account keys after the one it
    /// holds, opens them in order and stores them. Returns whether the
    /// engine now holds a newer key than before.
    async fn catch_up_keys(&self) -> Result<bool, SyncError> {
        let Some(keeper) = &self.keeper else {
            return Err(SyncError::KeyUnavailable);
        };
        let (device_id, device_key) = keeper.device();
        let held = self.ring();
        let mut ring = held.clone();
        let copies = self.api.account_keys(ring.version()).await?;
        let mut moved = false;
        for copy in copies {
            // Each copy opens with the key before it, so only the one that
            // follows the key held now can be opened.
            if copy.key_version <= ring.version() {
                continue;
            }
            if copy.key_version != ring.version() + 1 {
                break;
            }
            let context = WrapContext {
                account_id: self.account_id,
                device_id,
                key_version: copy.key_version,
            };
            let key = unwrap_account_key(&copy.wrapped, ring.current(), &device_key, &context)
                .map_err(|_| SyncError::KeyRejected)?;
            ring.adopt(key);
            moved = true;
        }
        if moved {
            // Stored before it is used: a note written under a key that
            // was then lost could be read by no one.
            keeper.keep(&held, &ring)?;
            self.set_ring(ring);
        }
        Ok(moved)
    }

    /// The key of `version`, fetching it first if it is newer than any
    /// this engine holds. `None` if the device does not have it and cannot
    /// get it: a version from before it was let in.
    async fn key_for(&self, version: i32) -> Result<Option<AccountKey>, SyncError> {
        let ring = self.ring();
        if let Some(key) = ring.get(version) {
            return Ok(Some(key.clone()));
        }
        if version > ring.version() && self.keeper.is_some() {
            self.catch_up_keys().await?;
            return Ok(self.ring().get(version).cloned());
        }
        Ok(None)
    }

    /// Marks the notes that the server still holds under an older key as
    /// unsent, so that this cycle sends them again under the new one.
    /// Returns how many this device could mark: it sends only notes it
    /// holds and has no problem with.
    async fn resend_old_key_notes(&self) -> Result<usize, SyncError> {
        let mut marked = 0;
        let mut after = 0;
        // Every page is looked at, not only the first. A device may hold
        // none of the notes on a page: one let in after an earlier change
        // of the key never had the notes still under the key before that.
        // Stopping there would leave the notes it does hold, further on,
        // under the old key.
        loop {
            let listed = self.api.old_key_notes(after).await?;
            for note in &listed.notes {
                if self.store.resend_note(note).await? {
                    marked += 1;
                }
            }
            match listed.next_after {
                Some(next) if next > after => after = next,
                _ => return Ok(marked),
            }
        }
    }

    /// How many documents from the server this engine has merged into the
    /// store so far. A cycle that fails part-way has still changed the
    /// store by what it merged first, and its report is lost with the
    /// error; this count is not. A change in it means the notes on screen
    /// may be out of date.
    #[must_use]
    pub fn merged_total(&self) -> u64 {
        self.merged.load(Ordering::Relaxed)
    }

    /// Runs one cycle and reports what it did.
    pub async fn run_once(&self) -> Result<SyncReport, SyncError> {
        self.check_keys()?;
        let mut report = SyncReport::default();
        let (mut rounds, mut stale_rounds, mut resent_any) = (0, 0, false);
        loop {
            self.pull(&mut report).await?;
            let resent = if report.old_key_notes > 0 {
                self.resend_old_key_notes().await?
            } else {
                0
            };
            resent_any |= resent > 0;
            let stale = self.push(&mut report).await?;
            rounds += 1;
            if stale > 0 {
                stale_rounds += 1;
            }
            // Another round is worth it while notes come back stale, a few
            // times, and while there were notes to send again under a new
            // key: each round sends a batch of those, and the server's
            // list shrinks by what was sent.
            let again = (stale > 0 && stale_rounds < MAX_ROUNDS)
                || (resent > 0 && rounds < MAX_RESEND_ROUNDS);
            if !again {
                break;
            }
        }
        if resent_any {
            // The count in the report is from before the last push.
            report.old_key_notes = self.api.old_key_notes(0).await?.total;
        }
        if report.old_key_notes == 0 {
            self.forget_older_keys()?;
        }
        report.unsent = self.store.unsent_count().await?;
        self.store
            .set_last_synced_ms(chrono::Utc::now().timestamp_millis())
            .await?;
        Ok(report)
    }

    /// Lets go of the keys before the current one, once no note under
    /// them remains on the server.
    fn forget_older_keys(&self) -> Result<(), SyncError> {
        let held = self.ring();
        if held.older().is_empty() {
            return Ok(());
        }
        let mut ring = held.clone();
        ring.forget_older();
        if let Some(keeper) = &self.keeper {
            keeper.keep(&held, &ring)?;
        }
        self.set_ring(ring);
        Ok(())
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
            report.pending_devices = page.pending_devices;
            report.reset_effective_at_ms = page.reset_effective_at_ms;
            report.key_version = page.key_version;
            report.old_key_notes = page.old_key_notes;
            if page.key_version > self.ring().version() {
                // The key was changed. Nothing can be written until this
                // device has the new one.
                self.catch_up_keys().await?;
                if page.key_version > self.ring().version() {
                    return Err(SyncError::KeyUnavailable);
                }
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
            Applied::Merged => {
                report.pulled += 1;
                self.merged.fetch_add(1, Ordering::Relaxed);
            }
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
        // A device let in after the key was changed never had the keys
        // before. A note still under one of those is held until another
        // device has sent it again under the new key.
        let Some(key) = self.key_for(blob.key_version).await? else {
            return Ok(Applied::Problem(
                "encrypted with a key this device does not have".to_owned(),
            ));
        };
        let context = BlobContext {
            account_id: self.account_id,
            note_id: blob.note_id,
            key_version: blob.key_version,
            format_version: blob.format_version,
        };
        let document = match decrypt_note(&key, &context, ciphertext) {
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
            // Read for each batch: the key may change between two.
            let ring = self.ring();
            let mut key_changed = false;

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
                let context = BlobContext::under(self.account_id, note.id, ring.version());
                items.push(PushItem {
                    note_id: note.id,
                    base_change: note.base_change,
                    key_version: context.key_version,
                    format_version: context.format_version,
                    ciphertext: encrypt_note(ring.current(), &context, &note.document),
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
                    // Nothing is wrong with the note. It stays unsent and
                    // goes again once this device has the new key.
                    PushOutcome::Rejected {
                        code: ErrorCode::KeyChanged,
                        ..
                    } => key_changed = true,
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
            if key_changed {
                if !self.catch_up_keys().await? {
                    return Err(SyncError::KeyUnavailable);
                }
                // Counted as stale so that the cycle goes round again and
                // sends what was refused.
                stale += 1;
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
