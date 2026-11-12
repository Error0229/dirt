//! Files in the sync cycle, and fetching one when it is wanted.
//!
//! A file's bytes never pass through the server. The device asks the
//! server for a link and sends to the storage, or fetches from it, itself.
//! What the server keeps is which notes refer to which files, and it
//! learns that from the devices, because it cannot read a note.
//!
//! After the notes have synced, the engine:
//!
//! 1. **Confirms** each reference the server has not taken: a file, and a
//!    note the server holds that names it. If the server answers that it
//!    does not have the file and this device does, the device sends it and
//!    confirms again. That one rule covers a file attached here, an upload
//!    that was cut short, and a server restored from a backup made before
//!    the file was sent. A device that does not hold the file leaves the
//!    reference for its next cycle: another device has the file to send.
//! 2. **Releases** each reference that a note has been without for as long
//!    as a deleted note waits to be purged.
//! 3. Removes from this device the files nothing needs any more.
//!
//! A failure here does not fail the cycle. The notes have synced by then,
//! and the files wait for the next one.

use std::collections::HashSet;
use std::path::PathBuf;

use dirt_proto::api::{limits, ErrorClass, ErrorCode, FileOutcome, FileReference};
use dirt_proto::NoteId;
use uuid::Uuid;

use super::{SyncEngine, SyncError, SyncReport};
use crate::api::ApiError;
use crate::store::FileLink;
use crate::{Error, Store};

/// Why a file could not be fetched.
#[derive(Debug, thiserror::Error)]
pub enum FileError {
    /// The note does not name the file.
    #[error("the note has no such file")]
    NotAttached,
    /// This device does not hold the file and the server does not have
    /// it. A device that holds it sends it when it next syncs; if none
    /// does, the file is lost.
    #[error("the file is not on this device and the server does not have it")]
    Missing,
    /// What the storage returned is not the file the note names: it does
    /// not open with the file's key. Nothing was kept.
    #[error("the server's copy of the file could not be decrypted")]
    Damaged,
    #[error(transparent)]
    Api(#[from] ApiError),
    #[error(transparent)]
    Store(#[from] Error),
}

/// Where a file that `note` names is on this device, for a device that
/// cannot fetch: one that is signed out, or works on a database file given
/// by hand.
pub async fn file_on_device(
    store: &Store,
    note: &NoteId,
    file_id: Uuid,
) -> Result<PathBuf, FileError> {
    if store.file_part(note, file_id).await?.is_none() {
        return Err(FileError::NotAttached);
    }
    let shelf = store.shelf();
    if shelf.holds(file_id) {
        Ok(shelf.path(file_id))
    } else {
        Err(FileError::Missing)
    }
}

/// What became of one file the engine tried to send.
enum Sent {
    Yes,
    /// It could not be sent, for a reason that trying again will not cure
    /// by itself. `full` says the account has no room, so that the cycle
    /// asks for no more.
    No {
        reason: String,
        full: bool,
    },
    /// The note stopped naming the file while the cycle ran.
    NotNamed,
}

impl SyncEngine {
    /// The step of a cycle that deals with files. See the module.
    pub(super) async fn sync_files(&self, report: &mut SyncReport) -> Result<(), SyncError> {
        let told = async {
            self.confirm_files(report).await?;
            self.release_files().await
        }
        .await;
        match told {
            Ok(()) => {}
            // A server that was given no storage, or cannot reach it. The
            // next cycle asks again; nothing is gained by hurrying.
            Err(SyncError::Api(ApiError::Server {
                code: ErrorCode::FilesUnavailable,
                ..
            })) => tracing::info!("the server has no file storage just now; files wait"),
            Err(SyncError::Api(error)) if error.class() == ErrorClass::TryAgainLater => {
                tracing::warn!("files were not all sent: {error}");
                report.files_failed = true;
            }
            Err(other) => return Err(other),
        }
        self.store.clear_spare_files(self.spare_file_wait).await?;
        report.files_unsent = self.store.unsent_file_count().await?;
        Ok(())
    }

    /// Asks the server about `links` and returns its answer for each. An
    /// answer is matched to what was asked by file and note, not by
    /// position, so one that is short, reordered or about something else
    /// cannot mark the wrong reference.
    async fn ask(
        &self,
        links: &[FileLink],
        release: bool,
    ) -> Result<Vec<(FileLink, FileOutcome)>, SyncError> {
        let items = links
            .iter()
            .map(|link| FileReference {
                file_id: link.file_id,
                note_id: link.note_id,
            })
            .collect();
        let answer = if release {
            self.api.release_files(items).await?
        } else {
            self.api.confirm_files(items).await?
        };
        Ok(answer
            .results
            .into_iter()
            .filter_map(|result| {
                let link = links.iter().find(|link| {
                    link.file_id == result.file_id && link.note_id == result.note_id
                })?;
                Some((*link, result.outcome))
            })
            .collect())
    }

    async fn confirm_files(&self, report: &mut SyncReport) -> Result<(), SyncError> {
        let shelf = self.store.shelf();
        let mut after = 0;
        let mut full = false;
        loop {
            let links = self
                .store
                .files_to_confirm(after, limits::MAX_BATCH_ITEMS)
                .await?;
            let Some(last) = links.last() else {
                return Ok(());
            };
            after = last.position;

            let mut to_send = Vec::new();
            for (link, outcome) in self.ask(&links, false).await? {
                // The server does not have the file. If this device does,
                // it sends it. If not, another device has to.
                let missing = matches!(outcome, FileOutcome::NotUploaded | FileOutcome::NoSuchFile);
                if missing && shelf.holds(link.file_id) {
                    to_send.push(link);
                } else {
                    self.settle(&link, outcome).await?;
                }
            }

            // A file is sent once, however many notes name it.
            let mut sent = HashSet::new();
            let mut tried = HashSet::new();
            for link in &to_send {
                if full || !tried.insert(link.file_id) {
                    continue;
                }
                match self.send_file(link).await? {
                    Sent::Yes => {
                        sent.insert(link.file_id);
                    }
                    Sent::No {
                        reason,
                        full: no_room,
                    } => {
                        full |= no_room;
                        self.flag_file(link, reason).await?;
                    }
                    Sent::NotNamed => {}
                }
            }
            report.files_sent += sent.len();
            to_send.retain(|link| sent.contains(&link.file_id));
            if to_send.is_empty() {
                continue;
            }
            // The server looks at the note's other files only once it has
            // found this one in the storage. So a refusal may come now,
            // and not with the first answer.
            for (link, outcome) in self.ask(&to_send, false).await? {
                self.settle(&link, outcome).await?;
            }
        }
    }

    /// Records the server's answer to a confirm that this cycle does
    /// nothing more about.
    async fn settle(&self, link: &FileLink, outcome: FileOutcome) -> Result<(), SyncError> {
        match outcome {
            FileOutcome::Done => self.store.mark_file_confirmed(link).await?,
            // Still not there: this device does not hold the file, or it
            // has just sent it and the storage does not show it yet. And a
            // note that was purged, or went with a restore; the next pull
            // says which. All wait for the next cycle.
            FileOutcome::NotUploaded | FileOutcome::NoSuchFile | FileOutcome::NoSuchNote => {}
            FileOutcome::Rejected { code, message } => {
                let reason = match code {
                    ErrorCode::QuotaExceeded => "the note has too many files".to_owned(),
                    _ => format!("refused by the server: {message}"),
                };
                self.flag_file(link, reason).await?;
            }
        }
        Ok(())
    }

    async fn flag_file(&self, link: &FileLink, reason: String) -> Result<(), SyncError> {
        tracing::warn!(
            "file {} of note {} cannot be sent: {reason}",
            link.file_id,
            link.note_id
        );
        self.store.flag_file_problem(link, reason).await?;
        Ok(())
    }

    /// Seals one file from the shelf and sends it to the storage.
    async fn send_file(&self, link: &FileLink) -> Result<Sent, SyncError> {
        let Some(part) = self.store.file_part(&link.note_id, link.file_id).await? else {
            return Ok(Sent::NotNamed);
        };
        let no = |reason: String| Sent::No {
            reason,
            full: false,
        };
        let size = i64::try_from(part.stored_size()).unwrap_or(i64::MAX);
        // The file is read before the server is asked for anything: the
        // server counts a file against the account from the moment it
        // gives a link, whether or not anything is then sent.
        let sealed = match self.store.sealed_file(self.account_id, part).await {
            Ok(sealed) => sealed,
            Err(Error::StoreClosed) => return Err(Error::StoreClosed.into()),
            Err(error) => {
                return Ok(no(format!(
                    "the copy on this device could not be read: {error}"
                )))
            }
        };
        let upload = match self.api.upload_link(link.file_id, size).await {
            Ok(upload) => upload,
            Err(ApiError::Server { code, message, .. }) => {
                return match code {
                    ErrorCode::FileTooLarge => Ok(no("too large to sync".to_owned())),
                    ErrorCode::QuotaExceeded => Ok(Sent::No {
                        reason: "the account's storage for files is full".to_owned(),
                        full: true,
                    }),
                    ErrorCode::InvalidRequest => {
                        Ok(no(format!("refused by the server: {message}")))
                    }
                    _ => Err(ApiError::Server {
                        code,
                        message,
                        retry_after_secs: None,
                    }
                    .into()),
                };
            }
            Err(other) => return Err(other.into()),
        };
        self.api.put_file(upload.url, sealed).await?;
        Ok(Sent::Yes)
    }

    /// Tells the server which notes no longer refer to which files. One
    /// batch is told in a cycle. The rest wait for the next.
    async fn release_files(&self) -> Result<(), SyncError> {
        let wait = i64::try_from(self.purge_after.as_millis()).unwrap_or(i64::MAX);
        let before = crate::util::now_ms().saturating_sub(wait);
        let links = self
            .store
            .files_to_release(before, limits::MAX_BATCH_ITEMS)
            .await?;
        if links.is_empty() {
            return Ok(());
        }
        for (link, outcome) in self.ask(&links, true).await? {
            // A file the server does not know has no reference to drop.
            if matches!(outcome, FileOutcome::Done | FileOutcome::NoSuchFile) {
                self.store.forget_file_link(&link).await?;
            }
        }
        Ok(())
    }

    /// Where a file that `note` names is on this device, fetching it from
    /// the storage first if the device does not hold it. A fetched file is
    /// kept, so it is fetched once.
    pub async fn fetch_file(&self, note: &NoteId, file_id: Uuid) -> Result<PathBuf, FileError> {
        let part = self
            .store
            .file_part(note, file_id)
            .await?
            .ok_or(FileError::NotAttached)?;
        let shelf = self.store.shelf();
        if shelf.holds(file_id) {
            return Ok(shelf.path(file_id));
        }
        let link = match self.api.download_link(file_id).await {
            Ok(link) => link,
            Err(ApiError::Server {
                code: ErrorCode::NotFound,
                ..
            }) => return Err(FileError::Missing),
            Err(other) => return Err(other.into()),
        };
        let sealed = self
            .api
            .get_file(link.url)
            .await?
            .ok_or(FileError::Missing)?;
        self.store
            .keep_fetched_file(self.account_id, part, sealed)
            .await?
            .ok_or(FileError::Damaged)
    }
}
