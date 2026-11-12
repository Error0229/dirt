//! The local database of notes, tags, search and settings.
//!
//! A [`Store`] owns three threads. One holds the only connection that writes;
//! two more each hold a connection that reads. A call sends a job to one of
//! them and waits for the answer, so a slow write never blocks a read and the
//! interface never waits on a lock (issues #210 and #218). The functions are
//! `async` but need no particular runtime.
//!
//! The file may be open in two processes at once: the desktop app and the
//! CLI. `schema` holds the rules that make that safe.

mod location;
mod notes;
mod schema;
mod settings;
mod sync_state;

use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;

use rusqlite::Connection;
use tokio::sync::oneshot;

pub use location::{store_path, StoreName};
pub use sync_state::{NoteProblem, ProblemKind, SyncProgress, UnsentNote};

use crate::models::{Note, NoteId, Settings};
use crate::{Error, Result};

/// How many threads serve reads.
const READERS: usize = 2;

type Job = Box<dyn FnOnce(&mut Connection) + Send + 'static>;

/// A handle to one local database. Cloning it is cheap and every clone
/// shares the same threads; the threads stop when the last clone is dropped.
#[derive(Clone)]
pub struct Store {
    inner: Arc<Inner>,
}

struct Inner {
    path: PathBuf,
    /// The database's own identifier, read from the file. Two stores hold
    /// the same database exactly when these are equal, however each was
    /// opened.
    store_id: String,
    // `Option` so that `drop` can close the channels before joining.
    writer: Option<mpsc::Sender<Job>>,
    readers: Option<mpsc::Sender<Job>>,
    /// The writer's thread first, then the readers'.
    threads: Vec<JoinHandle<()>>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        // Closing a channel ends its threads' loops. Joining them means the
        // file is closed by the time the last handle is gone, which matters
        // to anything that then moves, copies or deletes the file.
        //
        // The readers go first and the writer last. SQLite folds the
        // write-ahead log into the database file when the last connection
        // closes, but only a connection that may write can do it. Closing
        // the writer last leaves one complete file and no log beside it.
        self.readers = None;
        while self.threads.len() > 1 {
            if let Some(reader) = self.threads.pop() {
                let _ = reader.join();
            }
        }
        self.writer = None;
        if let Some(writer) = self.threads.pop() {
            let _ = writer.join();
        }
    }
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store")
            .field("path", &self.inner.path)
            .finish_non_exhaustive()
    }
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

impl Store {
    /// Opens the database at `path`, creating the file, its folder and its
    /// tables if they are not there yet.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder)?;
        }
        // Each step is tried again while the file is busy: see `schema`.
        // Setting up the tables is safe to repeat, because it looks at what
        // is there inside its own transaction.
        let (mut write_connection, store_id) = schema::retry_while_busy(|| {
            let mut connection = schema::open_connection(&path)?;
            let store_id = schema::set_up(&mut connection)?;
            Ok((connection, store_id))
        })?;
        // Open every connection before starting any thread, so that a
        // failure here leaves nothing running.
        let read_connections = (0..READERS)
            .map(|_| schema::retry_while_busy(|| schema::open_reader(&path)))
            .collect::<Result<Vec<_>>>()?;

        let mut threads = Vec::with_capacity(READERS + 1);
        let (writer, write_jobs) = mpsc::channel::<Job>();
        threads.push(spawn("dirt-store-write", move || {
            for job in write_jobs {
                job(&mut write_connection);
            }
        })?);

        let (readers, read_jobs) = mpsc::channel::<Job>();
        let read_jobs = Arc::new(Mutex::new(read_jobs));
        for mut connection in read_connections {
            let jobs = Arc::clone(&read_jobs);
            threads.push(spawn("dirt-store-read", move || loop {
                // The lock is held only while waiting for the next job, not
                // while running it.
                let job = match jobs.lock() {
                    Ok(queue) => queue.recv(),
                    Err(_) => break,
                };
                match job {
                    Ok(job) => job(&mut connection),
                    Err(_) => break,
                }
            })?);
        }

        Ok(Self {
            inner: Arc::new(Inner {
                path,
                store_id,
                writer: Some(writer),
                readers: Some(readers),
                threads,
            }),
        })
    }

    /// The database file this store reads and writes.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.inner.path
    }

    async fn run<T, F>(sender: Option<&mpsc::Sender<Job>>, work: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T> + Send + 'static,
    {
        let (answer, wait) = oneshot::channel();
        let job: Job = Box::new(move |connection| {
            // The caller may have stopped waiting; that is not an error here.
            let _ = answer.send(work(connection));
        });
        sender
            .ok_or(Error::StoreClosed)?
            .send(job)
            .map_err(|_| Error::StoreClosed)?;
        wait.await.map_err(|_| Error::StoreClosed)?
    }

    async fn write<T, F>(&self, work: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T> + Send + 'static,
    {
        Self::run(self.inner.writer.as_ref(), work).await
    }

    async fn read<T, F>(&self, work: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T> + Send + 'static,
    {
        Self::run(self.inner.readers.as_ref(), work).await
    }

    // ---- Notes: writing ----

    /// Creates a note with this text and a new ID.
    pub async fn create_note(&self, content: &str) -> Result<Note> {
        let content = content.to_owned();
        self.write(move |c| notes::create(c, NoteId::new(), &content, now_ms()))
            .await
    }

    /// Replaces a note's text. Only the parts that changed are recorded, so
    /// an edit made on another device at the same time merges with this one.
    pub async fn update_note(&self, id: &NoteId, content: &str) -> Result<Note> {
        let (id, content) = (*id, content.to_owned());
        self.write(move |c| notes::update(c, &id, &content, now_ms()))
            .await
    }

    /// Deletes a note. The note's document is kept, marked as deleted, so
    /// that the deletion can reach other devices and can be undone.
    pub async fn delete_note(&self, id: &NoteId) -> Result<()> {
        let id = *id;
        self.write(move |c| notes::delete(c, &id)).await
    }

    /// Brings a deleted note back with the text it had.
    pub async fn restore_note(&self, id: &NoteId) -> Result<Note> {
        let id = *id;
        self.write(move |c| notes::restore(c, &id, now_ms())).await
    }

    // ---- Notes: reading ----

    /// The note with this ID, unless it is deleted or does not exist.
    pub async fn get_note(&self, id: &NoteId) -> Result<Option<Note>> {
        let id = *id;
        self.read(move |c| notes::get(c, &id)).await
    }

    /// Notes, most recently edited first.
    pub async fn list_notes(&self, limit: usize, offset: usize) -> Result<Vec<Note>> {
        self.read(move |c| notes::list(c, limit, offset)).await
    }

    /// Notes containing every word of `query`, best match first. Each word
    /// also matches as the start of a longer word.
    pub async fn search_notes(&self, query: &str, limit: usize) -> Result<Vec<Note>> {
        let query = query.to_owned();
        self.read(move |c| notes::search(c, &query, limit)).await
    }

    /// Notes carrying this tag, with or without its leading `#`.
    pub async fn list_notes_by_tag(
        &self,
        tag: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<Note>> {
        let tag = tag.to_owned();
        self.read(move |c| notes::list_by_tag(c, &tag, limit, offset))
            .await
    }

    /// Notes that contain every word of `query` and carry `tag`. Either
    /// may be left out. This is what a list with a search box and tag
    /// filter shows: it looks through every note, not only those a client
    /// has loaded.
    pub async fn find_notes(
        &self,
        query: &str,
        tag: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Note>> {
        let query = query.to_owned();
        let tag = tag.map(str::to_owned);
        self.read(move |c| notes::find(c, &query, tag.as_deref(), limit))
            .await
    }

    /// Every tag in use, with how many notes carry it.
    pub async fn list_tags(&self) -> Result<Vec<(String, usize)>> {
        self.read(|c| notes::list_tags(c)).await
    }

    /// The IDs of notes whose ID starts with `prefix`.
    pub async fn list_note_ids_by_prefix(&self, prefix: &str, limit: usize) -> Result<Vec<String>> {
        let prefix = prefix.to_owned();
        self.read(move |c| notes::ids_by_prefix(c, &prefix, limit))
            .await
    }

    /// How many notes the store holds, not counting deleted ones.
    pub async fn note_count(&self) -> Result<usize> {
        self.read(|c| notes::count(c)).await
    }

    // ---- Documents ----

    /// Every note's CRDT document, deleted notes included.
    pub async fn note_documents(&self) -> Result<Vec<(NoteId, Vec<u8>)>> {
        let snapshots = self.read(|c| notes::snapshots(c)).await?;
        Ok(snapshots
            .into_iter()
            .map(|snapshot| (snapshot.id, snapshot.document))
            .collect())
    }

    /// Merges another copy of a note into this store, creating the note if
    /// it is not here yet. Returns the note, or `None` if it is deleted.
    ///
    /// Merging is safe to repeat: the same document merged twice changes
    /// nothing.
    pub async fn merge_note_document(
        &self,
        id: &NoteId,
        document: Vec<u8>,
    ) -> Result<Option<Note>> {
        let id = *id;
        self.write(move |c| notes::merge_document(c, &id, &document))
            .await
    }

    /// A note's local revision counter, which rises with every change made
    /// to it here. `None` if the store has never held the note.
    pub async fn note_revision(&self, id: &NoteId) -> Result<Option<i64>> {
        let id = *id;
        self.read(move |c| notes::revision(c, &id)).await
    }

    /// Moves the notes of this store into `target`, merging each with any
    /// copy already there, and returns how many were moved.
    ///
    /// This is how notes written before sign-in reach the account's store.
    ///
    /// Nothing is lost if the move is interrupted or if this store is
    /// written to while it runs. Each note is merged into the target first,
    /// and is then removed from here only if it has not changed since it was
    /// read. A note that was created, edited or merged with a version from
    /// the server in the meantime stays here,
    /// so call this again until it returns zero if other writers may be
    /// active. Merging the same note twice is harmless.
    pub async fn move_notes_into(&self, target: &Self) -> Result<usize> {
        // Compared by the identifier kept inside each file, not by path: one
        // file can be reached through several paths, and moving a database
        // into itself would remove every note in it.
        if self.inner.store_id == target.inner.store_id {
            return Err(Error::InvalidInput(
                "a store cannot be moved into itself".to_owned(),
            ));
        }
        let copied = self.copy_notes_into(target).await?;
        self.write(move |c| notes::remove_unchanged(c, &copied))
            .await
    }

    /// Merges every note of this store into `target` and returns each note's
    /// ID with the document that was copied.
    async fn copy_notes_into(&self, target: &Self) -> Result<Vec<(NoteId, Vec<u8>)>> {
        let snapshots = self.read(|c| notes::snapshots(c)).await?;
        let mut copied = Vec::with_capacity(snapshots.len());
        for snapshot in snapshots {
            target
                .merge_note_document(&snapshot.id, snapshot.document.clone())
                .await?;
            copied.push((snapshot.id, snapshot.document));
        }
        Ok(copied)
    }

    // ---- Sync state ----

    /// Notes changed here and not yet accepted by the server, positioned
    /// after `after` (zero for the start), up to `max_notes` and for as
    /// long as their documents fit in `max_bytes` together. Notes with a
    /// sync problem are left out.
    pub async fn unsent_notes(
        &self,
        after: i64,
        max_notes: usize,
        max_bytes: usize,
    ) -> Result<Vec<UnsentNote>> {
        self.read(move |c| sync_state::unsent(c, after, max_notes, max_bytes))
            .await
    }

    /// Forgets the server's version of a note, so that the next push
    /// offers it as new.
    pub async fn forget_note_server_version(&self, id: &NoteId) -> Result<()> {
        let id = *id;
        self.write(move |c| sync_state::forget_server_version(c, &id))
            .await
    }

    /// Marks a note as unsent though it has not changed, so that the next
    /// push sends it again: under a new account key. Returns whether the
    /// store holds the note and has no sync problem with it.
    pub async fn resend_note(&self, id: &NoteId) -> Result<bool> {
        let id = *id;
        self.write(move |c| sync_state::resend(c, &id)).await
    }

    /// How many notes are waiting to be pushed.
    pub async fn unsent_count(&self) -> Result<usize> {
        self.read(|c| sync_state::unsent_count(c)).await
    }

    /// How many notes hold changes the server has not accepted, counting
    /// the notes held back by a sync problem too: what deleting this store
    /// now would lose.
    pub async fn unsynced_count(&self) -> Result<usize> {
        self.read(|c| sync_state::unsynced_count(c)).await
    }

    /// When a sync cycle last completed, as Unix milliseconds, or `None` if
    /// none ever has.
    pub async fn last_synced_ms(&self) -> Result<Option<i64>> {
        self.read(|c| sync_state::last_synced(c)).await
    }

    /// Records that a sync cycle completed at `at_ms`.
    pub async fn set_last_synced_ms(&self, at_ms: i64) -> Result<()> {
        self.write(move |c| sync_state::set_last_synced(c, at_ms))
            .await
    }

    /// Records that the server accepted `revision` of a note as change
    /// number `change`. A note edited since then stays unsent.
    pub async fn mark_note_pushed(&self, id: &NoteId, revision: i64, change: i64) -> Result<()> {
        let id = *id;
        self.write(move |c| sync_state::mark_pushed(c, &id, revision, change))
            .await
    }

    /// Merges a document that came from the server as change number
    /// `change`. The note does not become unsent by this. Returns the
    /// note, or `None` if it is deleted.
    ///
    /// Refuses a change number lower than one already seen for the note
    /// with [`Error::Replay`].
    pub async fn apply_remote_note(
        &self,
        id: &NoteId,
        document: Vec<u8>,
        change: i64,
    ) -> Result<Option<Note>> {
        let id = *id;
        self.write(move |c| sync_state::apply_remote(c, &id, &document, change))
            .await
    }

    /// The last change number seen for a note, or `None` if the store has
    /// never held it.
    pub async fn note_server_change(&self, id: &NoteId) -> Result<Option<i64>> {
        let id = *id;
        self.read(move |c| sync_state::server_change(c, &id)).await
    }

    /// The pull cursor and the account epoch it belongs to.
    pub async fn sync_progress(&self) -> Result<SyncProgress> {
        self.read(|c| sync_state::progress(c)).await
    }

    pub async fn set_sync_cursor(&self, cursor: i64) -> Result<()> {
        self.write(move |c| sync_state::set_cursor(c, cursor)).await
    }

    /// Starts over for a new account epoch: forgets the cursor and every
    /// change number seen, and marks every note unsent.
    pub async fn begin_epoch(&self, epoch: i32) -> Result<()> {
        self.write(move |c| sync_state::begin_epoch(c, epoch)).await
    }

    /// Flags a note as not syncable, with the reason and what would end
    /// the problem. A flagged note is not pushed.
    pub async fn flag_note_problem(
        &self,
        id: &NoteId,
        reason: String,
        kind: ProblemKind,
    ) -> Result<()> {
        let id = *id;
        self.write(move |c| sync_state::set_problem(c, &id, Some((&reason, kind))))
            .await
    }

    /// Clears a note's sync problem, if it has one.
    pub async fn clear_note_problem(&self, id: &NoteId) -> Result<()> {
        let id = *id;
        self.write(move |c| sync_state::set_problem(c, &id, None))
            .await
    }

    /// The notes that could not be synced, and why.
    pub async fn note_problems(&self) -> Result<Vec<NoteProblem>> {
        self.read(|c| sync_state::problems(c)).await
    }

    // ---- Settings ----

    /// The saved settings, with defaults for any that were never saved.
    pub async fn load_settings(&self) -> Result<Settings> {
        self.read(|c| settings::load(c)).await
    }

    pub async fn save_settings(&self, settings: &Settings) -> Result<()> {
        let settings = settings.clone();
        self.write(move |c| settings::save(c, &settings)).await
    }
}

fn spawn(name: &str, body: impl FnOnce() + Send + 'static) -> Result<JoinHandle<()>> {
    Ok(std::thread::Builder::new()
        .name(name.to_owned())
        .spawn(body)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair() -> (tempfile::TempDir, Store, Store) {
        let folder = tempfile::tempdir().unwrap();
        let source = Store::open(folder.path().join("source.db")).unwrap();
        let target = Store::open(folder.path().join("target.db")).unwrap();
        (folder, source, target)
    }

    /// A merge from the server changes a note without raising its revision.
    /// The move must still see that the note is no longer what it copied.
    #[tokio::test]
    async fn a_note_merged_from_the_server_during_a_move_stays_in_the_source() {
        let (_folder, source, target) = pair();
        let note = source.create_note("buy milk").await.unwrap();
        let document = source.note_documents().await.unwrap().remove(0).1;
        let elsewhere =
            crate::notes::NoteDoc::from_bytes(7_100_000_000_000_001, &document).unwrap();
        elsewhere.set_text("buy milk and bread", 1_800_000_000_000);

        let copied = source.copy_notes_into(&target).await.unwrap();
        let revision = source.note_revision(&note.id).await.unwrap();
        source
            .apply_remote_note(&note.id, elsewhere.encode(), 3)
            .await
            .unwrap();
        assert_eq!(source.note_revision(&note.id).await.unwrap(), revision);
        let removed = source
            .write(move |c| notes::remove_unchanged(c, &copied))
            .await
            .unwrap();

        assert_eq!(removed, 0, "the target holds only the older document");
        assert_eq!(source.move_notes_into(&target).await.unwrap(), 1);
        assert_eq!(
            target.get_note(&note.id).await.unwrap().unwrap().content,
            "buy milk and bread"
        );
    }

    /// The window the move must survive: another writer changes the source
    /// after its notes were copied and before they are removed.
    #[tokio::test]
    async fn notes_written_during_a_move_stay_in_the_source() {
        let (_folder, source, target) = pair();
        let untouched = source.create_note("untouched").await.unwrap();
        let edited = source.create_note("before the move").await.unwrap();

        let copied = source.copy_notes_into(&target).await.unwrap();
        // Between the copy and the removal: an edit and a new note.
        source
            .update_note(&edited.id, "edited during the move")
            .await
            .unwrap();
        let late = source.create_note("created during the move").await.unwrap();
        let removed = source
            .write(move |c| notes::remove_unchanged(c, &copied))
            .await
            .unwrap();

        assert_eq!(removed, 1, "only the untouched note was safe to remove");
        assert_eq!(source.get_note(&untouched.id).await.unwrap(), None);
        assert_eq!(
            source.get_note(&edited.id).await.unwrap().unwrap().content,
            "edited during the move"
        );
        assert!(source.get_note(&late.id).await.unwrap().is_some());
        assert_eq!(source.search_notes("untouched", 10).await.unwrap().len(), 0);
        assert_eq!(source.search_notes("during", 10).await.unwrap().len(), 2);

        // A second pass moves what the first had to leave.
        assert_eq!(source.move_notes_into(&target).await.unwrap(), 2);
        assert!(source.note_documents().await.unwrap().is_empty());
        assert_eq!(
            target.get_note(&edited.id).await.unwrap().unwrap().content,
            "edited during the move"
        );
        assert!(target.get_note(&late.id).await.unwrap().is_some());
        assert_eq!(target.list_notes(10, 0).await.unwrap().len(), 3);
    }

    #[tokio::test]
    async fn one_database_reached_by_two_paths_is_not_moved_into_itself() {
        let folder = tempfile::tempdir().unwrap();
        std::fs::create_dir(folder.path().join("sub")).unwrap();
        let direct = folder.path().join("notes.db");
        let roundabout = folder.path().join("sub").join("..").join("notes.db");
        let first = Store::open(&direct).unwrap();
        let second = Store::open(&roundabout).unwrap();
        assert_ne!(first.path(), second.path(), "the test needs two spellings");
        first.create_note("only copy").await.unwrap();

        let error = first.move_notes_into(&second).await.unwrap_err();

        assert!(matches!(error, Error::InvalidInput(_)), "{error}");
        assert_eq!(first.list_notes(10, 0).await.unwrap().len(), 1);
        assert_eq!(second.list_notes(10, 0).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn two_databases_have_different_identifiers_and_reopening_keeps_one() {
        let (folder, source, target) = pair();
        assert_ne!(source.inner.store_id, target.inner.store_id);

        let before = source.inner.store_id.clone();
        drop(source);
        let reopened = Store::open(folder.path().join("source.db")).unwrap();
        assert_eq!(reopened.inner.store_id, before);
    }
}
