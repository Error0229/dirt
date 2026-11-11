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

use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;

use rusqlite::Connection;
use tokio::sync::oneshot;

pub use location::{store_path, StoreName};

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
    // `Option` so that `drop` can close the channels before joining.
    writer: Option<mpsc::Sender<Job>>,
    readers: Option<mpsc::Sender<Job>>,
    threads: Vec<JoinHandle<()>>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        // Closing the channels ends each thread's loop. Joining them means
        // the file is closed by the time the last handle is gone, which
        // matters to anything that then moves or deletes the file.
        self.writer = None;
        self.readers = None;
        for thread in self.threads.drain(..) {
            let _ = thread.join();
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
        let mut write_connection = schema::open_connection(&path)?;
        schema::set_up(&mut write_connection)?;
        // Open every connection before starting any thread, so that a
        // failure here leaves nothing running.
        let read_connections = (0..READERS)
            .map(|_| schema::open_reader(&path))
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

    // ---- Documents ----

    /// Every note's CRDT document, deleted notes included.
    pub async fn note_documents(&self) -> Result<Vec<(NoteId, Vec<u8>)>> {
        self.read(|c| notes::documents(c)).await
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

    /// Moves every note of this store into `target`, merging with any copy
    /// already there, and returns how many were moved.
    ///
    /// This is how notes written before sign-in reach the account's store.
    /// The notes are merged into the target first and removed from here
    /// afterwards, so a failure in between loses nothing and the move can
    /// simply be run again.
    pub async fn move_notes_into(&self, target: &Self) -> Result<usize> {
        if self.inner.path == target.inner.path {
            return Err(Error::InvalidInput(
                "a store cannot be moved into itself".to_owned(),
            ));
        }
        let documents = self.note_documents().await?;
        let moved = documents.len();
        for (id, document) in documents {
            target.merge_note_document(&id, document).await?;
        }
        self.write(notes::remove_all).await?;
        Ok(moved)
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
