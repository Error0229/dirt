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

mod files;
mod location;
mod notes;
mod schema;
mod settings;
mod sync_state;
mod tags;
mod voice;

use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;

use rusqlite::Connection;
use tokio::sync::oneshot;

pub use files::{AttachedFile, FileLink, FileProblem, FileStanding};
pub use location::{
    close_data_folder, remove_store_folder, store_path, StoreName, SIGNED_OUT_MARKER,
};
pub use notes::{LetGo, NoteCounts};
pub use sync_state::{
    NoteProblem, ProblemKind, PurgeApplied, PurgeCandidate, SyncProgress, UnsentNote,
};
pub use tags::{TagCount, TagRule};
pub use voice::{VoiceWaiting, WordsPut, READING_STOPPED};

/// The name a voice recording has among a note's files.
pub const VOICE_FILE_NAME: &str = "Voice recording.opus";

/// What kind of file a voice recording is: Ogg Opus (6.18).
pub const VOICE_MEDIA_TYPE: &str = "audio/ogg";

use dirt_proto::api::limits;

use crate::files::shelf::Shelf;
use crate::files::{seal, thumbnail, Attachment, FileKey, FilePart, Thumbnail};
use crate::models::{Note, NoteId, PileNote, Settings};
use crate::notes::NoteDoc;
use crate::{Error, Result};

/// How many threads serve reads.
const READERS: usize = 2;

type Job = Box<dyn FnOnce(&mut Connection) + Send + 'static>;

/// Where a file being attached comes from.
enum Source {
    /// A file elsewhere on the device, copied onto the shelf.
    Path(PathBuf),
    /// Bytes in memory, written onto the shelf.
    Bytes(Vec<u8>),
}

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
    /// Which file the database was when it was opened: on Linux and macOS
    /// a file can be removed while it is open, and another made at its
    /// path.
    #[cfg(unix)]
    file: Option<(u64, u64)>,
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

/// The device and the number of the file at `path`, which together name
/// one file however it is reached.
#[cfg(unix)]
fn file_identity(path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt as _;
    std::fs::metadata(path)
        .ok()
        .map(|metadata| (metadata.dev(), metadata.ino()))
}

impl Store {
    /// Opens a store of the application's own, under `data_folder`, making
    /// what is not there yet.
    ///
    /// The data folder is closed to the other users of the computer first
    /// ([`close_data_folder`]). This is how a program opens the store it
    /// keeps for the person. [`Self::open`] is for a database file the
    /// person gave by hand, whose folder is theirs to arrange.
    pub fn open_in(data_folder: &Path, name: &StoreName) -> Result<Self> {
        close_data_folder(data_folder)?;
        Self::open(store_path(data_folder, name))
    }

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
                #[cfg(unix)]
                file: file_identity(&path),
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
        if self.signed_out() {
            return Err(Error::SignedOut);
        }
        Self::run(self.inner.writer.as_ref(), work).await
    }

    /// Whether this is an account's store that the device was signed out
    /// of while the store was open: its folder is marked for removal, or
    /// its file already removed (on Linux and macOS a file can be removed
    /// under a program that has it open, and a new store made at its path).
    /// The CLI and the desktop app share the store, and either may sign out
    /// while the other runs. What the other wrote from then on would be
    /// removed with the store without ever having synced, so nothing more
    /// is written to it (issue #314).
    fn signed_out(&self) -> bool {
        let path = &self.inner.path;
        let marked = path
            .parent()
            .is_some_and(|folder| folder.join(SIGNED_OUT_MARKER).exists());
        #[cfg(unix)]
        let gone = self.inner.file.is_some() && file_identity(path) != self.inner.file;
        #[cfg(not(unix))]
        let gone = !path.exists();
        marked || gone
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

    /// Creates a note with this text under an ID the caller made.
    ///
    /// For an app that has to name the note before the store has made it:
    /// the desktop's page makes the note at the first character typed, and
    /// files attached in the same moment go to that note. A note with this
    /// ID already is [`Error::InvalidInput`], and so is an ID that is not a
    /// UUID v7: a note's ID begins with the time it was made, which is its
    /// creation time.
    pub async fn create_note_with_id(&self, id: NoteId, content: &str) -> Result<Note> {
        if id.as_uuid().get_version_num() != 7 {
            return Err(Error::InvalidInput(format!(
                "note {id} has no time in its ID: a note's ID is a UUID v7"
            )));
        }
        let content = content.to_owned();
        self.write(move |c| notes::create(c, id, &content, now_ms()))
            .await
    }

    /// Makes a note of a thought that waited outside the store, under the
    /// ID it was given when it was written: Android's capture sheet keeps a
    /// thought in a file named by its ID, and the app and a background job
    /// both take such files in (interface design 8.5).
    ///
    /// The note is dated at the time its ID carries, never later than now,
    /// so a thought that waited, for a network or for the app, is where it
    /// belongs in the list. It is made only if no note has this ID, deleted
    /// or not: taking the same thought in twice, at once or after a crash,
    /// makes one note, and `None` says one was there already. An ID that is
    /// not a UUID v7 is [`Error::InvalidInput`].
    pub async fn take_in_note(&self, id: NoteId, content: &str) -> Result<Option<Note>> {
        let Some(written_at) = id.created_at_ms() else {
            return Err(Error::InvalidInput(format!(
                "note {id} has no time in its ID: a note's ID is a UUID v7"
            )));
        };
        let at = written_at.min(now_ms());
        let content = content.to_owned();
        self.write(move |c| notes::create_if_new(c, id, &content, at))
            .await
    }

    /// Where this store keeps the files attached to its notes.
    #[must_use]
    pub fn shelf(&self) -> Shelf {
        Shelf::beside(self.path())
    }

    /// Attaches the file at `source` to a note, under `name`.
    ///
    /// The file is copied onto the store's shelf, given a key of its own,
    /// and named in the note's document. The copy is what the note has
    /// from now on: the file at `source` is not touched again, and changing
    /// it changes nothing here.
    ///
    /// A picture gets a thumbnail, which is a second, small file with a
    /// key of its own. It counts as one of the note's files, as the server
    /// counts it. A picture that would fit in the note only without its
    /// thumbnail is attached without.
    pub async fn attach_file(
        &self,
        note: &NoteId,
        source: &Path,
        name: &str,
        media_type: &str,
    ) -> Result<Attachment> {
        self.attach(note, Source::Path(source.to_path_buf()), name, media_type)
            .await
    }

    /// Attaches a file that is in memory, not on disk, to a note, under
    /// `name`: a picture pasted into the page. It is written onto the
    /// shelf and named in the note exactly as [`Self::attach_file`] does
    /// with a file it copies, thumbnail and all.
    pub async fn attach_bytes(
        &self,
        note: &NoteId,
        bytes: Vec<u8>,
        name: &str,
        media_type: &str,
    ) -> Result<Attachment> {
        self.attach(note, Source::Bytes(bytes), name, media_type)
            .await
    }

    async fn attach(
        &self,
        note: &NoteId,
        source: Source,
        name: &str,
        media_type: &str,
    ) -> Result<Attachment> {
        let note = *note;
        let (name, media_type) = (name.to_owned(), media_type.to_owned());
        let shelf = self.shelf();
        // The files first, away from the thread that writes: reading a
        // large photograph to make it small takes a moment, and nothing
        // else that is written should wait for it.
        let made = shelf.clone();
        let prepared = self
            .read(move |_| {
                let size = match &source {
                    Source::Path(path) => std::fs::metadata(path)?.len(),
                    Source::Bytes(bytes) => bytes.len() as u64,
                };
                let most = u64::try_from(limits::MAX_FILE_BYTES).unwrap_or(u64::MAX);
                if seal::sealed_size(size) > most {
                    return Err(Error::InvalidInput(format!(
                        "the file is too large to attach: {size} bytes"
                    )));
                }
                let mut attachment = Attachment {
                    file_id: uuid::Uuid::now_v7(),
                    name,
                    media_type,
                    size,
                    key: FileKey::generate(),
                    added_at_ms: now_ms(),
                    thumbnail: None,
                };
                match &source {
                    Source::Path(path) => {
                        let copied = made.add(attachment.file_id, path)?;
                        if copied != size {
                            let _ = made.remove(attachment.file_id);
                            return Err(Error::InvalidInput(
                                "the file changed while it was being attached".to_owned(),
                            ));
                        }
                    }
                    Source::Bytes(bytes) => made.put(attachment.file_id, bytes)?,
                }
                // Made from the copy, which no longer changes. A thumbnail
                // that cannot be kept is done without.
                if let Some(picture) = thumbnail::make(&made.path(attachment.file_id)) {
                    let file_id = uuid::Uuid::now_v7();
                    if made.put(file_id, &picture).is_ok() {
                        attachment.thumbnail = Some(Thumbnail {
                            file_id,
                            key: FileKey::generate(),
                            size: picture.len() as u64,
                        });
                    }
                }
                Ok(attachment)
            })
            .await?;

        let on_shelf = prepared.parts();
        let named = self
            .write(move |c| notes::attach(c, &note, prepared, now_ms()))
            .await;
        // Whatever was put on the shelf and is not named after all is of
        // no use: the whole attachment, or a thumbnail the note had no
        // room for.
        let kept = named.as_ref().map(Attachment::parts).unwrap_or_default();
        for part in on_shelf {
            if !kept.iter().any(|kept| kept.file_id == part.file_id) {
                let _ = shelf.remove(part.file_id);
            }
        }
        named
    }

    /// Removes a file from a note. Returns whether the note had it.
    ///
    /// The file stays on the device and on the server for a while: another
    /// device may not have fetched it yet, and the removal has to sync.
    pub async fn detach_file(&self, note: &NoteId, file_id: uuid::Uuid) -> Result<bool> {
        let note = *note;
        self.write(move |c| notes::detach(c, &note, file_id, now_ms()))
            .await
    }

    /// Every file that some note in this store names, a deleted note
    /// included: a deleted note can come back, and its files with it.
    pub async fn files_in_use(&self) -> Result<Vec<uuid::Uuid>> {
        self.read(|c| notes::files_in_use(c)).await
    }

    /// The files attached to a note, in the order they were attached.
    pub async fn note_attachments(&self, note: &NoteId) -> Result<Vec<Attachment>> {
        let note = *note;
        self.read(move |c| notes::attachments(c, &note)).await
    }

    /// The files attached to a note, each with whether this device holds
    /// it and what the server has said of it: what a list of a note's
    /// files shows.
    pub async fn attached_files(&self, note: &NoteId) -> Result<Vec<AttachedFile>> {
        let (note, shelf) = (*note, self.shelf());
        self.read(move |c| files::attached(c, &note, &shelf)).await
    }

    /// The notes that name a file, deleted notes among them.
    pub async fn notes_with_files(&self) -> Result<std::collections::HashSet<NoteId>> {
        self.read(|c| files::notes_with_files(c)).await
    }

    /// Of the notes `among`, those that have files, each with its revision,
    /// which rises with every write of the note on this device, a file
    /// attached or removed among them, and a copy merged from another
    /// device. What a list asks before it reads the files of the notes it
    /// shows, so that it reads them again only for a note that changed.
    /// Each note is looked up by its ID; nothing else is read.
    pub async fn file_revisions(
        &self,
        among: Vec<NoteId>,
    ) -> Result<std::collections::HashMap<NoteId, i64>> {
        self.read(move |c| files::revisions_with_files(c, &among))
            .await
    }

    /// The bytes of a small file this device holds, such as a thumbnail,
    /// read on the store's own thread. `None` if the device does not hold
    /// it, or it is larger than `most` bytes: this is for showing a small
    /// picture, not for handling a file.
    pub async fn small_file(&self, file_id: uuid::Uuid, most: u64) -> Result<Option<Vec<u8>>> {
        let shelf = self.shelf();
        self.read(move |_| {
            let path = shelf.path(file_id);
            match std::fs::metadata(&path) {
                Ok(found) if found.is_file() && found.len() <= most => {
                    Ok(Some(std::fs::read(&path)?))
                }
                Ok(_) => Ok(None),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(error) => Err(error.into()),
            }
        })
        .await
    }

    /// The bytes of a file this device holds, whole and in memory, read
    /// on the store's own thread. `None` if the device does not hold it.
    ///
    /// For showing a file inside the app without making a copy of it for
    /// another program. A file is 10 MiB at most, and one on the shelf
    /// that is larger is not a file a note could name: it is refused,
    /// not read.
    pub async fn file_bytes(&self, file_id: uuid::Uuid) -> Result<Option<Vec<u8>>> {
        let shelf = self.shelf();
        self.read(move |_| {
            let path = shelf.path(file_id);
            let most = u64::try_from(limits::MAX_FILE_BYTES).unwrap_or(u64::MAX);
            match std::fs::metadata(&path) {
                Ok(found) if !found.is_file() => Ok(None),
                Ok(found) if found.len() > most => Err(Error::InvalidInput(format!(
                    "the file on this device is {} bytes, more than a file may be",
                    found.len()
                ))),
                Ok(_) => Ok(Some(std::fs::read(&path)?)),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(error) => Err(error.into()),
            }
        })
        .await
    }

    // ---- Files and the server ----

    /// One file that a note names: an attachment or a thumbnail, with the
    /// key that opens it. `None` if the note does not name it.
    pub async fn file_part(&self, note: &NoteId, file_id: uuid::Uuid) -> Result<Option<FilePart>> {
        let note = *note;
        self.read(move |c| files::part(c, &note, file_id)).await
    }

    /// References the server has not taken yet, positioned after `after`
    /// (zero for the start), up to `max`: a file, and a note on the server
    /// that names it.
    pub async fn files_to_confirm(&self, after: i64, max: usize) -> Result<Vec<FileLink>> {
        self.read(move |c| files::to_confirm(c, after, max)).await
    }

    /// Records that the server has taken a reference.
    pub async fn mark_file_confirmed(&self, link: &FileLink) -> Result<()> {
        let link = *link;
        self.write(move |c| files::mark_confirmed(c, &link)).await
    }

    /// Records why a file could not be sent for a note.
    pub async fn flag_file_problem(&self, link: &FileLink, reason: String) -> Result<()> {
        let link = *link;
        self.write(move |c| files::set_problem(c, &link, &reason))
            .await
    }

    /// The files that could not be sent, and why.
    pub async fn file_problems(&self) -> Result<Vec<FileProblem>> {
        self.read(|c| files::problems(c)).await
    }

    /// How many files could not be sent: the length of
    /// [`Self::file_problems`], for a status that shows only the number.
    pub async fn file_problem_count(&self) -> Result<usize> {
        self.read(|c| files::problem_count(c)).await
    }

    /// References to tell the server to drop: a note has been without the
    /// file on this device since before `unnamed_before_ms`, and the server
    /// holds the note as this device does. Up to `max`.
    pub async fn files_to_release(
        &self,
        unnamed_before_ms: i64,
        max: usize,
    ) -> Result<Vec<FileLink>> {
        self.read(move |c| files::to_release(c, unnamed_before_ms, max))
            .await
    }

    /// Forgets a reference that the server has dropped.
    pub async fn forget_file_link(&self, link: &FileLink) -> Result<()> {
        let link = *link;
        self.write(move |c| files::forget(c, &link)).await
    }

    /// How many files this device holds that the server has not taken,
    /// leaving out those it refused.
    pub async fn unsent_file_count(&self) -> Result<usize> {
        let shelf = self.shelf();
        self.read(move |c| {
            let waiting = files::unconfirmed(c)?;
            Ok(waiting
                .into_iter()
                .filter(|file| shelf.holds(*file))
                .count())
        })
        .await
    }

    /// How many files this device holds that the server has not been
    /// seen to take, counting those it refused: what deleting this store
    /// now would lose of files, as [`Self::unsynced_count`] is of notes.
    pub async fn unsynced_file_count(&self) -> Result<usize> {
        let shelf = self.shelf();
        self.read(move |c| {
            let waiting = files::not_taken(c)?;
            Ok(waiting
                .into_iter()
                .filter(|file| shelf.holds(*file))
                .count())
        })
        .await
    }

    /// A file from the shelf as it is sent: sealed, bound to the account.
    ///
    /// Fails if the shelf does not hold the file at the size the note
    /// says, which is the size the server is told.
    pub async fn sealed_file(&self, account_id: uuid::Uuid, part: FilePart) -> Result<Vec<u8>> {
        let shelf = self.shelf();
        self.read(move |_| {
            let path = shelf.path(part.file_id);
            let size = std::fs::metadata(&path)?.len();
            if size != part.size {
                return Err(Error::InvalidInput(format!(
                    "the file is {size} bytes on this device and {} in the note",
                    part.size
                )));
            }
            let context = seal::FileContext {
                account_id,
                file_id: part.file_id,
            };
            let room = usize::try_from(part.stored_size()).unwrap_or_default();
            let mut sealed = Vec::with_capacity(room);
            seal::seal(
                &part.key,
                &context,
                std::fs::File::open(&path)?,
                &mut sealed,
            )
            .map_err(|error| Error::Storage(error.to_string()))?;
            Ok(sealed)
        })
        .await
    }

    /// Opens a sealed file that came from the storage and puts it on the
    /// shelf. Returns where it is, or `None` if it did not open, or opened
    /// to another size than the note says: it is not the file the note
    /// names, and nothing is kept.
    pub async fn keep_fetched_file(
        &self,
        account_id: uuid::Uuid,
        part: FilePart,
        sealed: Vec<u8>,
    ) -> Result<Option<PathBuf>> {
        let shelf = self.shelf();
        self.read(move |_| {
            let context = seal::FileContext {
                account_id,
                file_id: part.file_id,
            };
            let unfinished = shelf.unfinished_of_my_own(part.file_id)?;
            let opened = std::fs::File::create(&unfinished)
                .map_err(seal::SealError::from)
                .and_then(|sink| seal::open(&part.key, &context, sealed.as_slice(), sink));
            match opened {
                Ok(size) if size == part.size => {
                    shelf.keep_from(part.file_id, &unfinished)?;
                    Ok(Some(shelf.path(part.file_id)))
                }
                Ok(_) | Err(seal::SealError::UnknownLayout | seal::SealError::Rejected) => {
                    let _ = std::fs::remove_file(&unfinished);
                    Ok(None)
                }
                Err(seal::SealError::Io(error)) => {
                    let _ = std::fs::remove_file(&unfinished);
                    Err(error.into())
                }
            }
        })
        .await
    }

    /// Removes from the shelf the files that nothing on this device has
    /// needed for `wait`: no note names them and nothing about them is
    /// left to tell the server. Returns how many went.
    ///
    /// A file is put on the shelf a moment before the note that names it
    /// is written, and this store may be open in two programs. So a file
    /// is only noted the first time it is seen unneeded, and removed when
    /// it has stayed so.
    pub async fn clear_spare_files(&self, wait: std::time::Duration) -> Result<usize> {
        let shelf = self.shelf();
        self.write(move |c| {
            let now = now_ms();
            let wait_ms = i64::try_from(wait.as_millis()).unwrap_or(i64::MAX);
            let removed = files::clear_spare(c, &shelf, now, now.saturating_sub(wait_ms))?;
            // What a program left that stopped half-way through a file.
            // A day is longer than any file takes.
            let stale = std::time::SystemTime::now()
                .checked_sub(std::time::Duration::from_hours(24).max(wait));
            if let Some(stale) = stale {
                shelf.clear_unfinished(stale)?;
            }
            Ok(removed)
        })
        .await
    }

    // ---- Voice (interface design 6.18) ----

    /// Keeps a recording as a note, at once: under the ID the caller made,
    /// with no words yet, the recording attached as its file and marked as
    /// its voice recording, and waiting on this device for its words
    /// ([`Self::voice_waiting`]). The recording is an Ogg Opus file of
    /// `duration_ms`.
    ///
    /// A recording larger than a file may be is [`Error::InvalidInput`],
    /// as is an ID that is not a UUID v7 or that a note has already.
    pub async fn keep_voice_note(
        &self,
        id: NoteId,
        recording: Vec<u8>,
        duration_ms: u64,
    ) -> Result<Note> {
        if id.as_uuid().get_version_num() != 7 {
            return Err(Error::InvalidInput(format!(
                "note {id} has no time in its ID: a note's ID is a UUID v7"
            )));
        }
        self.keep_voice(id, recording, duration_ms, now_ms())
            .await?
            .ok_or_else(|| Error::InvalidInput(format!("note {id} already exists")))
    }

    /// Makes a note of a recording that waited outside the store, under the
    /// ID it was given when it was said: Android's capture sheet keeps a
    /// recording in a file named by its ID (interface design 8.9), as it
    /// does a typed thought ([`Self::take_in_note`]).
    ///
    /// The note is dated at the time its ID carries, never later than now,
    /// and is made only if no note has this ID: `None` says one was there
    /// already. Otherwise as [`Self::keep_voice_note`].
    pub async fn take_in_voice_note(
        &self,
        id: NoteId,
        recording: Vec<u8>,
        duration_ms: u64,
    ) -> Result<Option<Note>> {
        let Some(said_at) = id.created_at_ms() else {
            return Err(Error::InvalidInput(format!(
                "note {id} has no time in its ID: a note's ID is a UUID v7"
            )));
        };
        self.keep_voice(id, recording, duration_ms, said_at.min(now_ms()))
            .await
    }

    async fn keep_voice(
        &self,
        id: NoteId,
        recording: Vec<u8>,
        duration_ms: u64,
        now: i64,
    ) -> Result<Option<Note>> {
        let attachment = self.shelve_recording(recording, now).await?;
        let file_id = attachment.file_id;
        let kept = self
            .write(move |c| voice::keep(c, id, &attachment, duration_ms, now))
            .await;
        if !matches!(kept, Ok(Some(_))) {
            let _ = self.shelf().remove(file_id);
        }
        kept
    }

    /// Adds a recording to a note that is there: what was said into a note
    /// being written (🎙 in Android's page, interface design 8.9). The
    /// recording becomes one of the note's files, at once, and waits on
    /// this device for its words, which go at the end of the note
    /// ([`crate::voice::make_words`]). Unlike [`Self::keep_voice_note`]'s,
    /// the note is not marked as said, nor named by the recording, whose
    /// length its file's player shows. The recording is an Ogg Opus file.
    ///
    /// A note that is not here, or is deleted, is [`Error::NotFound`]. A
    /// recording larger than a file may be, or a note with no room for
    /// another file, is [`Error::InvalidInput`].
    pub async fn add_voice_to_note(&self, note: &NoteId, recording: Vec<u8>) -> Result<Note> {
        let now = now_ms();
        let attachment = self.shelve_recording(recording, now).await?;
        let file_id = attachment.file_id;
        let note = *note;
        let added = self
            .write(move |c| voice::add(c, &note, &attachment, now))
            .await;
        if added.is_err() {
            let _ = self.shelf().remove(file_id);
        }
        added
    }

    /// Puts a recording on the shelf, away from the thread that writes, as
    /// any file attached is, and gives the attachment that names it.
    async fn shelve_recording(&self, recording: Vec<u8>, now: i64) -> Result<Attachment> {
        let size = recording.len() as u64;
        let most = u64::try_from(limits::MAX_FILE_BYTES).unwrap_or(u64::MAX);
        if seal::sealed_size(size) > most {
            return Err(Error::InvalidInput(format!(
                "the recording is too large to keep: {size} bytes"
            )));
        }
        let attachment = Attachment {
            file_id: uuid::Uuid::now_v7(),
            name: VOICE_FILE_NAME.to_owned(),
            media_type: VOICE_MEDIA_TYPE.to_owned(),
            size,
            key: FileKey::generate(),
            added_at_ms: now,
            thumbnail: None,
        };
        let put = self.shelf();
        let file_id = attachment.file_id;
        self.read(move |_| Ok(put.put(file_id, &recording)?))
            .await?;
        Ok(attachment)
    }

    /// The recordings made on this device whose words are still to be
    /// made, the oldest first. Only the device that recorded a thought
    /// makes its words.
    pub async fn voice_waiting(&self) -> Result<Vec<VoiceWaiting>> {
        self.read(|c| voice::waiting(c)).await
    }

    /// Puts a recording's words at the start of its note, and marks them
    /// made; see [`WordsPut`]. `words` empty means none were found. With
    /// `text_only` the recording is taken off the note once its words are
    /// in (Settings → Voice → Text only).
    pub async fn put_voice_words(
        &self,
        note: &NoteId,
        words: &str,
        text_only: bool,
    ) -> Result<WordsPut> {
        let (note, words) = (*note, words.to_owned());
        self.write(move |c| voice::put_words(c, &note, &words, text_only, now_ms()))
            .await
    }

    /// Of these notes, those whose words this device has stopped trying to
    /// make, each with why the last try failed: what their rows say.
    pub async fn voice_problems(
        &self,
        notes: Vec<NoteId>,
    ) -> Result<std::collections::HashMap<NoteId, String>> {
        self.read(move |c| voice::problems(c, &notes, crate::voice::MOST_TRIES))
            .await
    }

    /// Puts the words of a recording that waits for them where they go:
    /// at the start of a note said ([`Self::put_voice_words`]), or at the
    /// end of the note a recording was added to
    /// ([`Self::add_voice_to_note`]), after a blank line. There, with
    /// `text_only` the recording is then taken off the note; with no words
    /// the note is left as it is; and a note that no longer has the
    /// recording, the person having taken it off, is not written.
    pub async fn put_words_of(
        &self,
        waiting: &VoiceWaiting,
        words: &str,
        text_only: bool,
    ) -> Result<WordsPut> {
        if !waiting.added {
            return self.put_voice_words(&waiting.note, words, text_only).await;
        }
        let (note, file_id, words) = (waiting.note, waiting.file_id, words.to_owned());
        self.write(move |c| voice::put_added_words(c, &note, file_id, &words, text_only, now_ms()))
            .await
    }

    /// A try at a recording's words begins. It is counted now, so that one
    /// the process does not survive counts too (`dirt_core::voice::MOST_TRIES`).
    pub async fn voice_reading(&self, waiting: &VoiceWaiting) -> Result<()> {
        let waiting = waiting.clone();
        self.write(move |c| voice::reading(c, &waiting)).await
    }

    /// Making a recording's words failed: it waits for the next try, and
    /// its note's row says why once the tries have run out.
    pub async fn voice_not_read(&self, waiting: &VoiceWaiting, why: &str) -> Result<()> {
        let (waiting, why) = (waiting.clone(), why.to_owned());
        self.write(move |c| voice::not_read(c, &waiting, &why))
            .await
    }

    /// Replaces a note's text. Only the parts that changed are recorded, so
    /// an edit made on another device at the same time merges with this one.
    pub async fn update_note(&self, id: &NoteId, content: &str) -> Result<Note> {
        let (id, content) = (*id, content.to_owned());
        self.write(move |c| notes::update(c, &id, None, &content, now_ms()))
            .await
    }

    /// Saves a note's text as a page that showed `base` changed it to
    /// `content`: the changes from `base` are made to the text the note has
    /// now, so that what came meanwhile is kept (the words of what was
    /// said, an edit from another device). With the note still at `base`
    /// this is [`Self::update_note`].
    pub async fn update_note_from(&self, id: &NoteId, base: &str, content: &str) -> Result<Note> {
        let (id, base, content) = (*id, base.to_owned(), content.to_owned());
        self.write(move |c| notes::update(c, &id, Some(&base), &content, now_ms()))
            .await
    }

    /// Deletes a note. The note's document is kept, marked as deleted, so
    /// that the deletion can reach other devices and can be undone.
    pub async fn delete_note(&self, id: &NoteId) -> Result<()> {
        let id = *id;
        self.write(move |c| notes::delete(c, &id)).await
    }

    /// Brings a deleted note back with the text it had: what undoes a
    /// delete. It works for as long as this device holds the note, which is
    /// until the note is purged, thirty days after the delete. A note that
    /// was archived comes back archived.
    ///
    /// The note keeps the edit time it had, so it comes back into its place
    /// in the list and not at the top. Bringing it back is still a change
    /// to the note, sent to the other devices like an edit, and it brings
    /// the note back on them too.
    ///
    /// A note that is not deleted is returned as it is. One this device
    /// does not hold is [`Error::NotFound`].
    pub async fn restore_note(&self, id: &NoteId) -> Result<Note> {
        let id = *id;
        self.write(move |c| notes::restore(c, &id, now_ms())).await
    }

    /// Archives a note: takes it out of the list. It is still a note, found
    /// by a search, synced and exported.
    ///
    /// Archiving is not an edit. The note keeps its edit time, and a note
    /// that another device has deleted meanwhile stays deleted. A note that
    /// is archived already is returned as it is.
    pub async fn archive_note(&self, id: &NoteId) -> Result<Note> {
        let id = *id;
        self.write(move |c| notes::set_archived(c, &id, true)).await
    }

    /// Brings an archived note back to the list. Not an edit either.
    pub async fn unarchive_note(&self, id: &NoteId) -> Result<Note> {
        let id = *id;
        self.write(move |c| notes::set_archived(c, &id, false))
            .await
    }

    /// Lets go of a note that holds nothing, when the person is done with
    /// it: no text but spaces and line breaks, and no file.
    ///
    /// A note is made at the first character typed. One emptied again was
    /// never a thought, and goes without a word: off the device altogether
    /// if it was never offered to the server, deleted like any other note
    /// if it was. A note that holds something is left as it is.
    pub async fn let_go_if_empty(&self, id: &NoteId) -> Result<LetGo> {
        let id = *id;
        self.write(move |c| notes::let_go_if_empty(c, &id)).await
    }

    // ---- Notes: reading ----

    /// The note with this ID, archived or not, unless it is deleted or does
    /// not exist.
    pub async fn get_note(&self, id: &NoteId) -> Result<Option<Note>> {
        let id = *id;
        self.read(move |c| notes::get(c, &id)).await
    }

    /// The list: notes that are not archived, most recently edited first.
    pub async fn list_notes(&self, limit: usize, offset: usize) -> Result<Vec<Note>> {
        self.read(move |c| notes::list(c, notes::Which::Listed, limit, offset))
            .await
    }

    /// The archived notes, most recently edited first.
    pub async fn list_archived_notes(&self, limit: usize, offset: usize) -> Result<Vec<Note>> {
        self.read(move |c| notes::list(c, notes::Which::Archived, limit, offset))
            .await
    }

    /// Every note that is not deleted: the notes of the list, then the
    /// archived ones, each most recently edited first. What an export
    /// holds.
    pub async fn list_every_note(&self, limit: usize, offset: usize) -> Result<Vec<Note>> {
        self.read(move |c| notes::list(c, notes::Which::Every, limit, offset))
            .await
    }

    /// Notes containing every word of `query`, best match first. Each word
    /// also matches as the start of a longer word.
    ///
    /// Archived notes are found too. They come after every note that is
    /// not archived, and [`Note::archived`] says which they are.
    pub async fn search_notes(&self, query: &str, limit: usize) -> Result<Vec<Note>> {
        let query = query.to_owned();
        self.read(move |c| notes::search(c, &query, limit)).await
    }

    /// Notes of the list carrying this tag, with or without its leading
    /// `#`. Archived notes are left out, as the list leaves them out.
    pub async fn list_notes_by_tag(
        &self,
        tag: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<Note>> {
        self.list_by_one_tag(notes::Which::Listed, tag, limit, offset)
            .await
    }

    /// Archived notes carrying this tag.
    pub async fn list_archived_notes_by_tag(
        &self,
        tag: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<Note>> {
        self.list_by_one_tag(notes::Which::Archived, tag, limit, offset)
            .await
    }

    /// Notes carrying one tag. A tag that is empty is no tag at all, and
    /// no note carries it.
    async fn list_by_one_tag(
        &self,
        which: notes::Which,
        tag: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<Note>> {
        let chosen = tags::Chosen::new(&[tag], TagRule::All);
        if chosen.is_empty() {
            return Ok(Vec::new());
        }
        self.read(move |c| notes::list_by_tags(c, which, &chosen, limit, offset))
            .await
    }

    /// Notes of the list that carry every one of `tags`, or under
    /// [`TagRule::Any`] at least one of them, most recently edited first.
    /// Archived notes are left out. A tag may have its `#` or not, and
    /// with no tags this is [`Self::list_notes`].
    pub async fn list_notes_by_tags(
        &self,
        tags: &[String],
        rule: TagRule,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<Note>> {
        let chosen = tags::Chosen::new(tags, rule);
        self.read(move |c| notes::list_by_tags(c, notes::Which::Listed, &chosen, limit, offset))
            .await
    }

    /// The archived notes narrowed in the same way.
    pub async fn list_archived_notes_by_tags(
        &self,
        tags: &[String],
        rule: TagRule,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<Note>> {
        let chosen = tags::Chosen::new(tags, rule);
        self.read(move |c| notes::list_by_tags(c, notes::Which::Archived, &chosen, limit, offset))
            .await
    }

    /// Notes that contain every word of `query` and carry `tag`. Either
    /// may be left out. This is what a list with a search box and tag
    /// filter shows: it looks through every note, not only those a client
    /// has loaded.
    ///
    /// With neither, it is the list, and archived notes are left out. With
    /// either, the archived notes that match come after the rest, and
    /// [`Note::archived`] says which they are.
    ///
    /// [`Self::find_notes_by_tags`] takes several tags.
    pub async fn find_notes(
        &self,
        query: &str,
        tag: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Note>> {
        let query = query.to_owned();
        let chosen = tags::Chosen::new(tag.as_slice(), TagRule::All);
        self.read(move |c| notes::find(c, &query, &chosen, limit))
            .await
    }

    /// What Find shows for the words typed and the tags chosen: notes that
    /// contain every word of `query` and carry every one of `tags`, or
    /// under [`TagRule::Any`] at least one of them. Words and tags may
    /// each be left out.
    ///
    /// With neither, it is the list, and archived notes are left out. With
    /// either, the archived notes that match come after the rest. With
    /// words the best match comes first, and without the most recently
    /// edited.
    pub async fn find_notes_by_tags(
        &self,
        query: &str,
        tags: &[String],
        rule: TagRule,
        limit: usize,
    ) -> Result<Vec<Note>> {
        let query = query.to_owned();
        let chosen = tags::Chosen::new(tags, rule);
        self.read(move |c| notes::find(c, &query, &chosen, limit))
            .await
    }

    /// The tags that could be chosen next in Find, each with how many
    /// notes there would be with that tag chosen too. The most notes
    /// first, and tags with as many in the order of their names; at most
    /// `limit` of them.
    ///
    /// `query` holds the words typed and `chosen` the tags already picked,
    /// combined by `rule`. The chosen tags themselves are not among the
    /// answers. `starting` is what was typed after a `#`: only tags whose
    /// names start so are given.
    ///
    /// **The count is the number of notes that
    /// [`Self::find_notes_by_tags`] gives for the same words and the
    /// chosen tags with this one added.** Finding keeps archived notes,
    /// after the rest, so they are counted, and a tag that only archived
    /// notes carry is offered. Deleted notes are not.
    ///
    /// Under [`TagRule::All`] a tag is given when a note that matches the
    /// words and carries every chosen tag carries it too. A tag that would
    /// leave no note is not given, so no choice can empty the list.
    ///
    /// Under [`TagRule::Any`] another tag takes nothing away: the count is
    /// the notes there are already and those the tag would add. A tag is
    /// given when a note that matches the words carries it. One whose
    /// notes are all there already is given with the number there is now.
    ///
    /// A query with text and no letter or digit finds nothing, so nothing
    /// is given for it.
    pub async fn tags_to_narrow(
        &self,
        query: &str,
        chosen: &[String],
        rule: TagRule,
        starting: Option<&str>,
        limit: usize,
    ) -> Result<Vec<TagCount>> {
        let query = query.to_owned();
        let chosen = tags::Chosen::new(chosen, rule);
        let starting = starting.map(str::to_owned);
        self.read(move |c| tags::narrowing(c, &query, &chosen, starting.as_deref(), limit))
            .await
    }

    /// Every tag in use, with how many notes carry it. Archived notes
    /// count: the number beside a tag is how many notes
    /// [`Self::find_notes`] gives for it.
    pub async fn list_tags(&self) -> Result<Vec<(String, usize)>> {
        self.read(|c| notes::list_tags(c)).await
    }

    /// The IDs of notes whose ID starts with `prefix`, archived notes
    /// among them and deleted ones not.
    pub async fn list_note_ids_by_prefix(&self, prefix: &str, limit: usize) -> Result<Vec<String>> {
        let prefix = prefix.to_owned();
        self.read(move |c| notes::ids_by_prefix(c, &prefix, false, limit))
            .await
    }

    /// The IDs of deleted notes whose ID starts with `prefix`: the notes
    /// that [`Self::restore_note`] can bring back.
    pub async fn list_deleted_note_ids_by_prefix(
        &self,
        prefix: &str,
        limit: usize,
    ) -> Result<Vec<String>> {
        let prefix = prefix.to_owned();
        self.read(move |c| notes::ids_by_prefix(c, &prefix, true, limit))
            .await
    }

    /// How many notes the store holds, archived ones among them, not
    /// counting deleted ones.
    pub async fn note_count(&self) -> Result<usize> {
        self.read(|c| notes::count(c)).await
    }

    /// How many notes the list holds, and how many are archived.
    pub async fn note_counts(&self) -> Result<NoteCounts> {
        self.read(|c| notes::counts(c)).await
    }

    /// Every note that is not deleted, archived ones among them, as the
    /// pile needs it: its ID, when it was last edited and whether it is
    /// archived. Oldest first, by when each was made.
    ///
    /// An app holds only the notes its list shows, and the pile has a
    /// grain for every note. This reads no text and no document, however
    /// long the notes are, and [`PileNote`] is 32 bytes: 20,000 notes are
    /// 640 KB and take about 20 ms (`docs/LIMITS.md`).
    ///
    /// The shade of each grain ([`crate::models::Grain`]), the layer it
    /// lies in ([`crate::models::Layer`]) and the line under the pile
    /// ([`crate::models::PileSummary`]) are worked out from the answer.
    pub async fn pile(&self) -> Result<Vec<PileNote>> {
        self.read(|c| notes::pile(c)).await
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
    /// A note's files go with it: each one this store holds is copied onto
    /// the target's shelf before the note is. The store the notes leave may
    /// be removed afterwards, folder and all, and a note that named a file
    /// would otherwise arrive without it.
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
            // The files first. If the move stops here the note is still in
            // this store with its files, and a file on the target's shelf
            // that nothing names yet does no harm.
            self.copy_files_into(target, &snapshot.document).await?;
            target
                .merge_note_document(&snapshot.id, snapshot.document.clone())
                .await?;
            // A recording made here whose words are still to be made is
            // the target's to make them for: it is this device's store
            // from now on.
            let id = snapshot.id;
            if let Some(pending) = self.read(move |c| voice::waiting_row(c, &id)).await? {
                target.write(move |c| voice::wait(c, &id, &pending)).await?;
            }
            let added = self.read(move |c| voice::added_waiting(c, &id)).await?;
            if !added.is_empty() {
                target
                    .write(move |c| voice::wait_added(c, &id, &added))
                    .await?;
            }
            copied.push((snapshot.id, snapshot.document));
        }
        Ok(copied)
    }

    /// Copies the files that `document` names from this store's shelf onto
    /// `target`'s. A file this store does not hold is passed over: the
    /// note names it all the same, and it is fetched when it is wanted.
    async fn copy_files_into(&self, target: &Self, document: &[u8]) -> Result<()> {
        // Reading needs no client ID of its own: nothing is written.
        let files = NoteDoc::from_bytes(0, document)?.attachments();
        if files.is_empty() {
            return Ok(());
        }
        let (from, to) = (self.shelf(), target.shelf());
        // On the store's own thread, as attaching a file is: copying a
        // file is not for the thread that draws the screen.
        self.read(move |_| {
            for file in files {
                if from.holds(file.file_id) && !to.holds(file.file_id) {
                    to.add(file.file_id, &from.path(file.file_id))?;
                }
            }
            Ok(())
        })
        .await
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

    /// Records that these notes are about to be sent to the server, and
    /// returns those the store still holds. A push does this before it
    /// sends a note for the first time ([`UnsentNote::offered`]), and
    /// leaves out a note that is not in the answer: it was let go as empty
    /// in the meantime.
    pub async fn offer_notes(&self, ids: Vec<NoteId>) -> Result<Vec<NoteId>> {
        self.write(move |c| sync_state::offer(c, &ids)).await
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

    /// Of the notes `among`, those that have not reached the server as they
    /// are now: never sent, or changed since. Deleted notes are left out. A
    /// list marks these among its rows while the device is signed in
    /// (interface design 6.2); on a device that is signed out it is every
    /// note.
    pub async fn unsynced_note_ids(
        &self,
        among: Vec<NoteId>,
    ) -> Result<std::collections::HashSet<NoteId>> {
        self.read(move |c| sync_state::unsynced_ids(c, &among))
            .await
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

    /// Deleted notes this device may ask the server to purge: seen deleted
    /// here since before `deleted_before_ms`, with nothing unsent. Up to
    /// `max`.
    pub async fn purge_candidates(
        &self,
        deleted_before_ms: i64,
        max: usize,
    ) -> Result<Vec<PurgeCandidate>> {
        self.read(move |c| sync_state::purge_candidates(c, deleted_before_ms, max))
            .await
    }

    /// Removes a note the server purged at this device's request, unless
    /// the note changed after it was listed at `revision`. Returns whether
    /// it was removed.
    pub async fn drop_purged_note(&self, id: &NoteId, revision: i64) -> Result<bool> {
        let id = *id;
        self.write(move |c| sync_state::drop_purged(c, &id, revision))
            .await
    }

    /// Takes the news that the server has purged a note: this device's
    /// copy goes, and an unsent edit to a note that is not deleted here is
    /// kept as a new note.
    pub async fn apply_note_purge(&self, id: &NoteId) -> Result<PurgeApplied> {
        let id = *id;
        self.write(move |c| sync_state::apply_purge(c, &id)).await
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

    // ---- A store signed out of while it is open (issue #314) ----

    #[tokio::test]
    async fn a_store_marked_signed_out_takes_no_more_writes_and_can_still_be_read() {
        let folder = tempfile::tempdir().unwrap();
        let store = Store::open(folder.path().join("notes.db")).unwrap();
        let kept = store.create_note("before the sign-out").await.unwrap();
        assert!(!store.signed_out());

        // The other program signs out and marks the folder.
        std::fs::write(folder.path().join(SIGNED_OUT_MARKER), b"").unwrap();

        assert!(store.signed_out());
        let refused = store.create_note("after the sign-out").await.unwrap_err();
        assert!(matches!(refused, Error::SignedOut), "{refused}");
        let refused = store
            .update_note(&kept.id, "changed after")
            .await
            .unwrap_err();
        assert!(matches!(refused, Error::SignedOut), "{refused}");
        let notes = store.list_notes(10, 0).await.unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].content, "before the sign-out");
    }

    #[tokio::test]
    async fn a_store_folder_is_removed_whole_or_not_at_all() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("account-a");
        let store = Store::open(folder.join("notes.db")).unwrap();
        let note = store.create_note("with a file").await.unwrap();
        let file = root.path().join("picture.txt");
        std::fs::write(&file, b"not really a picture").unwrap();
        store
            .attach_file(&note.id, &file, "picture.txt", "text/plain")
            .await
            .unwrap();
        std::fs::write(folder.join(SIGNED_OUT_MARKER), b"").unwrap();
        let everything = |folder: &Path| {
            walk(folder)
                .into_iter()
                .map(|path| path.strip_prefix(folder).unwrap().to_owned())
                .collect::<Vec<_>>()
        };
        let before = everything(&folder);

        let removed = remove_store_folder(&folder);

        if cfg!(windows) {
            // The store is open, so its database cannot be removed, and
            // nothing else is: the files and the mark are all still there.
            assert!(removed.is_err());
            assert_eq!(everything(&folder), before);
        } else {
            // An open file can be removed here: the folder goes, and the
            // store that had it open writes nothing more.
            removed.unwrap();
            assert!(!folder.exists());
            assert!(store.signed_out());
        }
        // Once nothing holds it, it goes.
        drop(store);
        remove_store_folder(&folder).unwrap();
        assert!(!folder.exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_store_whose_file_was_replaced_takes_no_more_writes() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("notes.db");
        let old = Store::open(&path).unwrap();
        // The folder is removed under it, and a new store made in its place
        // by a sign-in in the other program.
        remove_store_folder(folder.path()).unwrap();
        let new = Store::open(&path).unwrap();

        assert!(old.signed_out());
        assert!(matches!(
            old.create_note("lost").await.unwrap_err(),
            Error::SignedOut
        ));
        new.create_note("kept").await.unwrap();
    }

    fn walk(folder: &Path) -> Vec<PathBuf> {
        let mut found = Vec::new();
        for entry in std::fs::read_dir(folder).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                found.extend(walk(&path));
            }
            found.push(path);
        }
        found.sort();
        found
    }
}
