//! What the page holds, and the store's work it asks for, kept apart from
//! the window so that it can be tried without one.
//!
//! The page never waits for the store. Done, opening another note and
//! putting a note away change the page at once; what the page held is
//! taken as it was at that moment, and the store is told about it
//! afterwards, in order. So the next thought can be typed straight after
//! <kbd>Esc</kbd>, and nothing typed in that moment goes into a page about
//! to be replaced (capture in under two seconds is the product).
//!
//! - [`Draft`] is the page: which note it shows, what was typed, and what
//!   of that the store has been asked to keep. The first character that
//!   is not a space makes the note, under an ID made here, so that the
//!   page names its note from that moment: files attached in the same
//!   breath go to it, and a second note is never made.
//! - [`Work`] is one thing for the store to do. [`Scribe`] does the work
//!   it is given one piece after another, in the order given, on a task
//!   of its own that outlives the screen, and says how each went
//!   ([`Done`]).
//!   A note opened again while its text waits in the queue shows that
//!   text. A write that fails is said once, with the words it carried, and
//!   is not tried again.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use dirt_core::models::{Note, NoteId};
use dirt_core::session::Session;
use dirt_core::store::LetGo;
use dirt_core::sync::worker::Trigger;
use dirt_core::Store;
use tokio::sync::broadcast;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

use crate::services::{attach_all, Incoming};

/// The page: the note it shows, and the thought being written.
#[derive(Debug, Clone, Default)]
pub struct Draft {
    /// The note on the page, once there is one.
    note: Option<NoteId>,
    /// What was typed last.
    text: String,
    /// What the store has been asked to keep.
    saved: String,
    /// Raised by every change, so that a save waited for can tell that it
    /// is out of date.
    version: u64,
}

/// What the page held when it was left: the work that keeps what was
/// typed.
#[derive(Debug, Default)]
pub struct Left {
    /// The note was emptied, and is let go as it is left (6.1), unless it
    /// has files.
    pub emptied: bool,
    /// What the store is to do, in this order.
    pub work: Vec<Work>,
}

impl Draft {
    /// The note on the page.
    #[must_use]
    pub const fn note(&self) -> Option<NoteId> {
        self.note
    }

    /// What was typed last.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The version a save waited for has to find unchanged.
    #[must_use]
    pub const fn version(&self) -> u64 {
        self.version
    }

    /// Nothing is on the page: no note, and nothing typed.
    #[must_use]
    pub const fn is_blank(&self) -> bool {
        self.note.is_none() && self.text.is_empty()
    }

    /// The page's text changed. The first character that is not a space
    /// makes the note, and that is returned to be done at once; anything
    /// later waits for [`Self::flush`].
    pub fn typed(&mut self, text: String) -> Option<Work> {
        self.version += 1;
        if self.note.is_none() && !text.trim().is_empty() {
            let note = NoteId::new();
            self.note = Some(note);
            self.saved.clone_from(&text);
            self.text = text.clone();
            return Some(Work::Create { note, text });
        }
        self.text = text;
        None
    }

    /// What was typed and not yet kept, to be saved now.
    pub fn flush(&mut self) -> Option<Work> {
        let note = self.note?;
        if self.text == self.saved {
            return None;
        }
        self.saved.clone_from(&self.text);
        Some(Work::Save {
            note,
            text: self.text.clone(),
        })
    }

    /// A note for files dropped or chosen on the page: the one on it, or one
    /// made now with `text` (6.1: a file with no words under it is a note).
    /// The work that makes it, if it is made.
    pub fn note_for_files(&mut self, text: impl FnOnce(&str) -> String) -> (NoteId, Option<Work>) {
        if let Some(note) = self.note {
            return (note, None);
        }
        let text = text(&self.text);
        let note = NoteId::new();
        self.version += 1;
        self.note = Some(note);
        self.text.clone_from(&text);
        self.saved.clone_from(&text);
        (note, Some(Work::Create { note, text }))
    }

    /// Leaves the page: takes what it holds, and leaves it blank. Nothing
    /// is waited for.
    pub fn leave(&mut self) -> Left {
        let mut left = Left::default();
        if let Some(note) = self.note {
            left.work.extend(self.flush());
            if self.text.trim().is_empty() {
                left.emptied = true;
                left.work.push(Work::LetGoIfEmpty { note });
            }
        }
        self.show_nothing();
        left
    }

    /// A blank page.
    fn show_nothing(&mut self) {
        *self = Self {
            version: self.version + 1,
            ..Self::default()
        };
    }

    /// Puts a note on the page, as the store has it.
    pub fn show(&mut self, note: &Note) {
        *self = Self {
            note: Some(note.id),
            text: note.content.clone(),
            saved: note.content.clone(),
            version: self.version + 1,
        };
    }

    /// The store could not make the page's note: the page goes back to a
    /// thought not yet made, so that the next key makes the note, with all
    /// that is on the page then (as before the note was named here). Says
    /// whether the note was the page's.
    pub fn unmade(&mut self, note: NoteId) -> bool {
        if self.note != Some(note) {
            return false;
        }
        self.note = None;
        self.saved.clear();
        self.version += 1;
        true
    }

    /// The store's text of the note on the page came after the page showed
    /// it. Taken, and true, if nothing has been typed since; otherwise the
    /// page keeps what was typed.
    pub fn refreshed(&mut self, note: &Note) -> bool {
        if self.note != Some(note.id) || self.text != self.saved || self.text == note.content {
            return false;
        }
        self.show(note);
        true
    }
}

/// One thing for the store to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Work {
    /// Make the note, under the ID the page gave it.
    Create { note: NoteId, text: String },
    /// Keep what was typed.
    Save { note: NoteId, text: String },
    /// Let the note go if it holds nothing (6.1).
    LetGoIfEmpty { note: NoteId },
    /// Archive the note, or delete it.
    PutAway { note: NoteId, delete: bool },
    /// Attach files to the note: dropped, chosen or pasted.
    Attach { note: NoteId, files: Vec<Incoming> },
}

/// How a piece of work went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Done {
    /// The note was made.
    Created(NoteId),
    /// What was typed was kept.
    Saved(NoteId),
    /// The note was let go, or was kept because it held a file.
    LetGo { note: NoteId, gone: bool },
    /// The note was archived or deleted. `kept` is false when there was
    /// nothing to put away: the note had been let go.
    PutAway {
        note: NoteId,
        delete: bool,
        kept: bool,
    },
    /// Files were attached, but for those named in `failed`, with why.
    Attached { note: NoteId, failed: Vec<String> },
    /// What was typed could not be kept: `text` is what the write carried,
    /// to be given back to the person, and `why` the store's reason.
    /// `making` says the store could not make the note at all; later work
    /// for it is not done, and the note is never made from this.
    Unsaved {
        note: NoteId,
        text: String,
        why: String,
        making: bool,
    },
    /// The work failed; it is logged.
    Failed,
}

impl Done {
    /// The notes in the store changed, and whatever shows them reads them
    /// again.
    #[must_use]
    pub const fn changed(&self) -> bool {
        !matches!(
            self,
            Self::Failed | Self::Unsaved { .. } | Self::LetGo { gone: false, .. }
        )
    }
}

/// Does one piece of work.
pub async fn run(store: &Arc<Store>, work: Work) -> Done {
    match work {
        Work::Create { note, text } => match store.create_note_with_id(note, &text).await {
            Ok(_) => Done::Created(note),
            Err(error) => {
                tracing::error!("Could not make the note: {error}");
                Done::Unsaved {
                    note,
                    text,
                    why: dirt_core::util::as_sentence(&error),
                    making: true,
                }
            }
        },
        Work::Save { note, text } => match store.update_note(&note, &text).await {
            Ok(_) => Done::Saved(note),
            Err(error) => {
                tracing::error!("Could not save the note: {error}");
                Done::Unsaved {
                    note,
                    text,
                    why: dirt_core::util::as_sentence(&error),
                    making: false,
                }
            }
        },
        Work::LetGoIfEmpty { note } => match store.let_go_if_empty(&note).await {
            Ok(LetGo::Kept) => Done::LetGo { note, gone: false },
            Ok(LetGo::Dropped | LetGo::Deleted | LetGo::Gone) => Done::LetGo { note, gone: true },
            Err(error) => {
                tracing::error!("Could not let the empty note go: {error}");
                Done::Failed
            }
        },
        Work::PutAway { note, delete } => {
            // A note let go as it was left is not there: nothing to put
            // away, and nothing to undo.
            if !matches!(store.get_note(&note).await, Ok(Some(_))) {
                return Done::PutAway {
                    note,
                    delete,
                    kept: false,
                };
            }
            let done = if delete {
                store.delete_note(&note).await
            } else {
                store.archive_note(&note).await.map(|_| ())
            };
            match done {
                Ok(()) => Done::PutAway {
                    note,
                    delete,
                    kept: true,
                },
                Err(error) => {
                    tracing::error!("Could not put the note away: {error}");
                    Done::Failed
                }
            }
        }
        Work::Attach { note, files } => {
            let failed = attach_all(Arc::clone(store), note, files).await;
            Done::Attached { note, failed }
        }
    }
}

/// What is done, in place of `work`, for a note the store could not make:
/// nothing is made later from a failure. What was typed is handed back to
/// the person, files are not attached, and there is nothing to let go or
/// to put away.
fn never_made(work: Work) -> Done {
    const WHY: &str = "The note could not be made.";
    match work {
        Work::Create { note, text } | Work::Save { note, text } => Done::Unsaved {
            note,
            text,
            why: WHY.to_owned(),
            making: true,
        },
        Work::Attach { note, files } => Done::Attached {
            note,
            failed: files
                .iter()
                .map(|file| format!("{}: {WHY}", file.name()))
                .collect(),
        },
        Work::LetGoIfEmpty { note } => Done::LetGo { note, gone: true },
        Work::PutAway { note, delete } => Done::PutAway {
            note,
            delete,
            kept: false,
        },
    }
}

impl Work {
    /// The note the work is for.
    #[must_use]
    pub const fn note(&self) -> NoteId {
        match self {
            Self::Create { note, .. }
            | Self::Save { note, .. }
            | Self::LetGoIfEmpty { note }
            | Self::PutAway { note, .. }
            | Self::Attach { note, .. } => *note,
        }
    }
}

/// One piece of work, with the store and session it is for: those of the
/// note when it was asked for, whatever is in use when it is done.
struct Job {
    serial: u64,
    store: Arc<Store>,
    session: Option<Session>,
    work: Work,
}

/// What the queue's task shares with those who ask of it.
struct Shared {
    /// For each note with a create or a save waiting, the newest text
    /// waiting, and the job that carries it.
    texts: Mutex<HashMap<NoteId, (u64, String)>>,
    /// Pieces of work asked for and not yet done.
    waiting: AtomicUsize,
    serial: AtomicU64,
}

impl Shared {
    fn texts(&self) -> std::sync::MutexGuard<'_, HashMap<NoteId, (u64, String)>> {
        // A panic while the map was held leaves it as it was.
        self.texts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Does the store's work for the page, in the order it was asked for.
///
/// There is one for the app, on a task of its own. The screen may go
/// (quick capture takes the window) with work still waiting: the work is
/// done all the same, and only its news goes unheard. Quitting and taking
/// another store into use wait for it ([`Self::settle`]).
///
/// A write that fails is not tried again. The store already waits out a
/// busy file inside every write; what still fails is the disk or the file,
/// which a moment's wait does not mend. It is said once ([`Done::Unsaved`],
/// with the text the write carried), and the app gives the person their
/// words.
#[derive(Clone)]
pub struct Scribe {
    jobs: UnboundedSender<Job>,
    news: broadcast::Sender<Done>,
    shared: Arc<Shared>,
}

impl Scribe {
    /// The app's one queue, started the first time it is asked for. Call
    /// from within the app's runtime.
    #[must_use]
    pub fn global() -> &'static Self {
        static SCRIBE: OnceLock<Scribe> = OnceLock::new();
        SCRIBE.get_or_init(Self::start)
    }

    /// Starts a queue of its own: the app's, or a test's.
    #[must_use]
    pub fn start() -> Self {
        let (jobs, waiting) = unbounded_channel::<Job>();
        let (news, _) = broadcast::channel(256);
        let shared = Arc::new(Shared {
            texts: Mutex::new(HashMap::new()),
            waiting: AtomicUsize::new(0),
            serial: AtomicU64::new(0),
        });
        tokio::spawn(work_through(waiting, news.clone(), Arc::clone(&shared)));
        Self { jobs, news, shared }
    }

    /// Hears how each piece of work went, from now on.
    #[must_use]
    pub fn listen(&self) -> broadcast::Receiver<Done> {
        self.news.subscribe()
    }

    /// Asks for work to be done after everything asked for before it.
    pub fn ask(
        &self,
        store: &Arc<Store>,
        session: Option<&Session>,
        work: impl IntoIterator<Item = Work>,
    ) {
        for work in work {
            let serial = self.shared.serial.fetch_add(1, Ordering::SeqCst) + 1;
            if let Work::Create { note, text } | Work::Save { note, text } = &work {
                self.shared.texts().insert(*note, (serial, text.clone()));
            }
            self.shared.waiting.fetch_add(1, Ordering::SeqCst);
            let job = Job {
                serial,
                store: Arc::clone(store),
                session: session.cloned(),
                work,
            };
            if self.jobs.send(job).is_err() {
                self.shared.waiting.fetch_sub(1, Ordering::SeqCst);
                tracing::error!("The page's work could not be handed on");
            }
        }
    }

    /// The newest text waiting in the queue for `note`: newer than the
    /// store's while it waits.
    #[must_use]
    pub fn waiting_text(&self, note: &NoteId) -> Option<String> {
        self.shared.texts().get(note).map(|(_, text)| text.clone())
    }

    /// Waits until everything asked for has been done, or `at_most` has
    /// passed. Says whether it was all done.
    pub async fn settle(&self, at_most: Duration) -> bool {
        let start = std::time::Instant::now();
        while self.shared.waiting.load(Ordering::SeqCst) > 0 {
            if start.elapsed() >= at_most {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        true
    }
}

/// The queue's task: the work, in order.
async fn work_through(
    mut waiting: UnboundedReceiver<Job>,
    news: broadcast::Sender<Done>,
    shared: Arc<Shared>,
) {
    // Notes the store could not make. Later work for one is not done.
    let mut unmade: HashSet<NoteId> = HashSet::new();
    while let Some(job) = waiting.recv().await {
        let Job {
            serial,
            store,
            session,
            work,
        } = job;
        let note = work.note();
        let done = if unmade.contains(&note) {
            never_made(work)
        } else {
            run(&store, work).await
        };
        if matches!(done, Done::Unsaved { making: true, .. }) {
            unmade.insert(note);
        }
        {
            let mut texts = shared.texts();
            if texts
                .get(&note)
                .is_some_and(|(newest, _)| *newest == serial)
            {
                texts.remove(&note);
            }
        }
        // The store is let go as soon as its work is done: one being taken
        // out of use must not be held open here.
        drop(store);
        if done.changed() {
            if let Some(session) = &session {
                session.notify(Trigger::Changed);
            }
        }
        // Nobody may be listening: the screen has gone. The work is done
        // all the same.
        let _ = news.send(done);
        shared.waiting.fetch_sub(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> (tempfile::TempDir, Arc<Store>) {
        let folder = tempfile::tempdir().unwrap();
        let store = Store::open(folder.path().join("notes.db")).unwrap();
        (folder, Arc::new(store))
    }

    async fn run_all(store: &Arc<Store>, work: Vec<Work>) -> Vec<Done> {
        let mut done = Vec::new();
        for work in work {
            done.push(run(store, work).await);
        }
        done
    }

    async fn texts(store: &Store) -> Vec<String> {
        let mut texts: Vec<String> = store
            .list_notes(100, 0)
            .await
            .unwrap()
            .into_iter()
            .map(|note| note.content)
            .collect();
        texts.sort();
        texts
    }

    /// Esc, and the next thought typed at once: the first note is kept as
    /// it was at Esc, and what comes after is a note of its own. Nothing
    /// is waited for in between.
    #[tokio::test]
    async fn what_is_typed_after_done_is_the_next_thought() {
        let (_folder, store) = temp_store();
        let mut page = Draft::default();
        let mut work = Vec::new();

        work.extend(page.typed("b".into()));
        assert!(page.typed("bread".into()).is_none());
        let first = page.note().unwrap();
        let left = page.leave();
        assert!(!left.emptied);
        work.extend(left.work);
        assert!(page.is_blank());

        // Typed before the store has done anything.
        work.extend(page.typed("m".into()));
        page.typed("milk".into());
        let second = page.note().unwrap();
        assert_ne!(first, second);
        work.extend(page.leave().work);

        assert_eq!(
            work,
            [
                Work::Create {
                    note: first,
                    text: "b".into()
                },
                Work::Save {
                    note: first,
                    text: "bread".into()
                },
                Work::Create {
                    note: second,
                    text: "m".into()
                },
                Work::Save {
                    note: second,
                    text: "milk".into()
                },
            ]
        );
        run_all(&store, work).await;
        assert_eq!(texts(&store).await, ["bread", "milk"]);
    }

    /// Many quick thoughts in a row, each left at once.
    #[tokio::test]
    async fn twenty_thoughts_left_at_once_are_twenty_notes() {
        let (_folder, store) = temp_store();
        let mut page = Draft::default();
        let mut work = Vec::new();
        for n in 0..20 {
            let thought = format!("thought {n}");
            for end in 1..=thought.len() {
                work.extend(page.typed(thought[..end].to_owned()));
            }
            work.extend(page.leave().work);
        }
        run_all(&store, work).await;
        let mut expected: Vec<String> = (0..20).map(|n| format!("thought {n}")).collect();
        expected.sort();
        assert_eq!(texts(&store).await, expected);
    }

    /// A note emptied again is let go as it is left.
    #[tokio::test]
    async fn a_note_emptied_and_left_is_let_go() {
        let (_folder, store) = temp_store();
        let mut page = Draft::default();
        let mut work = Vec::new();
        work.extend(page.typed("x".into()));
        page.typed("  ".into());
        let left = page.leave();
        assert!(left.emptied);
        work.extend(left.work);
        let done = run_all(&store, work).await;
        assert!(matches!(done.last(), Some(Done::LetGo { gone: true, .. })));
        assert_eq!(store.note_count().await.unwrap(), 0);
    }

    /// Spaces alone are not a note, and leaving them asks nothing of the
    /// store.
    #[test]
    fn spaces_alone_are_written_but_are_no_note() {
        let mut page = Draft::default();
        assert!(page.typed("   ".into()).is_none());
        assert!(!page.is_blank());
        assert_eq!(page.note(), None);
        let left = page.leave();
        assert!(left.work.is_empty());
        assert!(page.is_blank());
    }

    /// A file attached while the first character's note is still being
    /// made goes to that note: there is one note, with the file.
    #[tokio::test]
    async fn a_file_attached_with_the_first_character_goes_to_its_note() {
        let (folder, store) = temp_store();
        let file = folder.path().join("list.txt");
        std::fs::write(&file, "eggs").unwrap();
        let mut page = Draft::default();
        let mut work = Vec::new();
        work.extend(page.typed("s".into()));
        let (note, made) = page.note_for_files(|_| unreachable!("the page has its note"));
        assert!(made.is_none());
        assert_eq!(Some(note), page.note());
        work.push(Work::Attach {
            note,
            files: vec![Incoming::Path(file)],
        });
        work.extend(page.typed("shopping".into()));
        work.extend(page.leave().work);

        let done = run_all(&store, work).await;
        assert!(done.contains(&Done::Attached {
            note,
            failed: Vec::new()
        }));
        assert_eq!(texts(&store).await, ["shopping"]);
        assert_eq!(store.note_attachments(&note).await.unwrap().len(), 1);
    }

    /// Files on a blank page make a note of their own, named by the text
    /// given, and a note that holds only a file is kept when it is left.
    #[tokio::test]
    async fn files_on_a_blank_page_make_a_note_that_is_kept() {
        let (folder, store) = temp_store();
        let file = folder.path().join("photo.png");
        std::fs::write(&file, "not really a picture").unwrap();
        let mut page = Draft::default();
        let (note, made) = page.note_for_files(|typed| format!("{typed}#work"));
        let mut work: Vec<Work> = made.into_iter().collect();
        assert_eq!(
            work,
            [Work::Create {
                note,
                text: "#work".into()
            }]
        );
        work.push(Work::Attach {
            note,
            files: vec![Incoming::Path(file)],
        });
        // Emptied, and left: it holds a file, so it is kept.
        page.typed(String::new());
        work.extend(page.leave().work);
        run_all(&store, work).await;
        assert_eq!(store.note_count().await.unwrap(), 1);
        assert_eq!(store.note_attachments(&note).await.unwrap().len(), 1);
    }

    /// Archiving the note on the page keeps what was typed first, and a
    /// note let go as it was left has nothing to archive.
    #[tokio::test]
    async fn putting_a_note_away_comes_after_what_was_typed() {
        let (_folder, store) = temp_store();
        let mut page = Draft::default();
        let mut work = Vec::new();
        work.extend(page.typed("p".into()));
        page.typed("pay the rent".into());
        let note = page.note().unwrap();
        let mut left = page.leave();
        left.work.push(Work::PutAway {
            note,
            delete: false,
        });
        work.extend(left.work);
        let done = run_all(&store, work).await;
        assert_eq!(
            done.last(),
            Some(&Done::PutAway {
                note,
                delete: false,
                kept: true
            })
        );
        let archived = store.list_archived_notes(10, 0).await.unwrap();
        assert_eq!(archived[0].content, "pay the rent");

        let mut work = Vec::new();
        work.extend(page.typed("o".into()));
        let emptied = page.note().unwrap();
        page.typed(String::new());
        let mut left = page.leave();
        left.work.push(Work::PutAway {
            note: emptied,
            delete: true,
        });
        work.extend(left.work);
        let done = run_all(&store, work).await;
        assert_eq!(
            done.last(),
            Some(&Done::PutAway {
                note: emptied,
                delete: true,
                kept: false
            })
        );
    }

    /// A save waited for is out of date once the page has changed.
    #[test]
    fn leaving_the_page_dates_what_was_waiting() {
        let mut page = Draft::default();
        page.typed("a".into());
        page.typed("ab".into());
        let waiting = page.version();
        let _ = page.leave();
        assert_ne!(page.version(), waiting);
        assert!(page.flush().is_none());
    }

    /// The store's text reaches a note shown from the list only while
    /// nothing has been typed in it.
    #[test]
    fn a_fresher_text_is_taken_only_before_typing() {
        let older = Note::new("one");
        let mut newer = older.clone();
        newer.content = "one, and two".into();
        let mut page = Draft::default();
        page.show(&older);
        assert!(page.refreshed(&newer));
        assert_eq!(page.text(), "one, and two");

        page.show(&older);
        page.typed("one!".into());
        assert!(!page.refreshed(&newer));
        assert_eq!(page.text(), "one!");
    }

    // ---- Through the queue -----------------------------------------------

    /// Everything said about the queue's work since `news` began to listen,
    /// once it has settled, and then a moment more: anything still coming
    /// would be work done again.
    async fn heard_once_settled(
        scribe: &Scribe,
        news: &mut broadcast::Receiver<Done>,
    ) -> Vec<Done> {
        assert!(scribe.settle(Duration::from_secs(20)).await);
        tokio::time::sleep(Duration::from_millis(300)).await;
        let mut heard = Vec::new();
        while let Ok(done) = news.try_recv() {
            heard.push(done);
        }
        heard
    }

    fn unsaved(heard: &[Done]) -> Vec<(NoteId, String, bool)> {
        heard
            .iter()
            .filter_map(|done| match done {
                Done::Unsaved {
                    note, text, making, ..
                } => Some((*note, text.clone(), *making)),
                _ => None,
            })
            .collect()
    }

    /// The queue against a real store: what is asked is done in order.
    #[tokio::test]
    async fn the_queue_keeps_everything_asked_of_it() {
        let (_folder, store) = temp_store();
        let scribe = Scribe::start();
        let mut page = Draft::default();
        let mut work = Vec::new();
        for n in 0..5 {
            work.extend(page.typed(format!("note {n}")));
            page.typed(format!("note {n}, longer"));
            work.extend(page.leave().work);
        }
        scribe.ask(&store, None, work);
        assert!(scribe.settle(Duration::from_secs(10)).await);
        let mut expected: Vec<String> = (0..5).map(|n| format!("note {n}, longer")).collect();
        expected.sort();
        assert_eq!(texts(&store).await, expected);
    }

    /// The store cannot make the note on the page (here, because the ID is
    /// taken). The person is told, with the words; the page goes back to a
    /// thought not yet made, and the next key makes it once, with all of
    /// it. Nothing makes the first note later.
    #[tokio::test]
    async fn a_failed_create_of_the_open_note_is_made_by_the_next_key() {
        let (_folder, store) = temp_store();
        let scribe = Scribe::start();
        let mut news = scribe.listen();
        let mut page = Draft::default();
        let first = page.typed("h".into()).unwrap();
        let taken = page.note().unwrap();
        store
            .create_note_with_id(taken, "already here")
            .await
            .unwrap();
        scribe.ask(&store, None, [first]);
        page.typed("he".into());
        scribe.ask(&store, None, page.flush());

        let heard = heard_once_settled(&scribe, &mut news).await;
        assert_eq!(
            unsaved(&heard),
            [
                (taken, "h".to_owned(), true),
                (taken, "he".to_owned(), true)
            ]
        );
        assert!(page.unmade(taken));
        assert_eq!(page.note(), None);

        let again = page
            .typed("hello".into())
            .expect("the next key makes the note");
        let made = page.note().unwrap();
        assert_ne!(made, taken);
        scribe.ask(&store, None, [again]);
        let heard = heard_once_settled(&scribe, &mut news).await;
        assert_eq!(heard, [Done::Created(made)]);
        assert_eq!(texts(&store).await, ["already here", "hello"]);
    }

    /// A save of a note already left fails: the person is told once, with
    /// the words it carried, and nothing tries it again.
    #[tokio::test]
    async fn a_failed_save_of_a_note_left_is_told_once() {
        let (_folder, store) = temp_store();
        let scribe = Scribe::start();
        let mut news = scribe.listen();
        // A note the store has let go of altogether: the save finds none.
        let gone = store.create_note("x").await.unwrap();
        store.update_note(&gone.id, "").await.unwrap();
        store.let_go_if_empty(&gone.id).await.unwrap();

        scribe.ask(
            &store,
            None,
            [Work::Save {
                note: gone.id,
                text: "written after all".into(),
            }],
        );
        let heard = heard_once_settled(&scribe, &mut news).await;
        assert_eq!(
            unsaved(&heard),
            [(gone.id, "written after all".to_owned(), false)]
        );
        assert_eq!(heard.len(), 1);
        assert_eq!(scribe.waiting_text(&gone.id), None);
    }

    /// A note deleted on another device and merged in while a save waits:
    /// the save is done or told once, and nothing loops.
    #[tokio::test]
    async fn a_note_deleted_while_its_save_waits_is_no_loop() {
        let (_folder, store) = temp_store();
        let scribe = Scribe::start();
        let mut news = scribe.listen();
        let note = store.create_note("first").await.unwrap();
        store.delete_note(&note.id).await.unwrap();
        scribe.ask(
            &store,
            None,
            [Work::Save {
                note: note.id,
                text: "first, and more".into(),
            }],
        );
        let heard = heard_once_settled(&scribe, &mut news).await;
        assert_eq!(heard.len(), 1, "{heard:?}");
        assert!(unsaved(&heard).len() <= 1);
    }

    /// A note opened again while its save waits shows the waiting text.
    #[tokio::test]
    async fn the_text_waiting_in_the_queue_is_known_until_it_is_done() {
        let (folder, store) = temp_store();
        let scribe = Scribe::start();
        let note = store.create_note("hello").await.unwrap();
        // Something slow ahead of it: a file to copy and encrypt.
        let big = folder.path().join("big.bin");
        std::fs::write(&big, vec![7_u8; 6 * 1024 * 1024]).unwrap();
        let other = store.create_note("other").await.unwrap();
        scribe.ask(
            &store,
            None,
            [
                Work::Attach {
                    note: other.id,
                    files: vec![Incoming::Path(big)],
                },
                Work::Save {
                    note: note.id,
                    text: "hello world".into(),
                },
            ],
        );
        assert_eq!(
            scribe.waiting_text(&note.id).as_deref(),
            Some("hello world")
        );
        assert!(scribe.settle(Duration::from_secs(30)).await);
        assert_eq!(scribe.waiting_text(&note.id), None);
        assert_eq!(
            store.get_note(&note.id).await.unwrap().unwrap().content,
            "hello world"
        );
    }

    /// Signing out: the page's work is done in the store it was asked of,
    /// the queue is settled, and then the queue holds that store no more,
    /// so that it can be removed.
    #[tokio::test]
    async fn settling_the_queue_leaves_the_old_store_free() {
        let (_old_folder, old) = temp_store();
        let (_new_folder, new) = temp_store();
        let scribe = Scribe::start();
        let mut page = Draft::default();
        let mut work: Vec<Work> = page.typed("signed in".into()).into_iter().collect();
        page.typed("signed in, then out".into());
        work.extend(page.leave().work);
        scribe.ask(&old, None, work);
        assert!(scribe.settle(Duration::from_secs(10)).await);
        assert_eq!(texts(&old).await, ["signed in, then out"]);
        assert!(texts(&new).await.is_empty());
        assert_eq!(Arc::strong_count(&old), 1, "the queue holds the old store");
    }
}
