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
//! - [`Ledger`] is what the store still owes: the text asked for and not
//!   yet kept, note by note. A note opened again shows it; a write that
//!   failed is tried again from it, and the person is told.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use dirt_core::models::{Note, NoteId};
use dirt_core::session::Session;
use dirt_core::store::LetGo;
use dirt_core::sync::worker::Trigger;
use dirt_core::Store;
use tokio::sync::broadcast;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

use crate::services::attach_all;

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
    /// Attach files to the note.
    Attach { note: NoteId, paths: Vec<PathBuf> },
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
    /// What was typed could not be kept, even when tried again. It stays
    /// owed, and is tried again now and then and with the next work.
    Unsaved { note: NoteId, why: String },
    /// A note that could not be kept before has been now.
    Kept(NoteId),
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

/// What the store still owes the page, note by note: the text last asked
/// to be kept that the store does not have yet, and whether the store has
/// made the note at all.
///
/// A note opened again before its text has reached the store shows that
/// text, not the store's older copy. A write that failed is tried again
/// from here, and a note whose making failed is made by the next write
/// asked for it, with the text then owed. Kept apart from the store, so
/// that it can be tried without one.
#[derive(Debug, Default)]
pub struct Ledger {
    owed: HashMap<NoteId, Owed>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Owed {
    /// The text asked for last, while the store does not have it.
    text: Option<String>,
    /// The store has made the note.
    made: bool,
    /// The last write failed, and the note waits to be tried again.
    failed: bool,
}

/// What a turn of the queue does with a piece of work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// This work, as it is.
    Do(Work),
    /// The note was never made, and holds nothing: nothing to do but to
    /// forget it.
    Forget(NoteId),
}

impl Ledger {
    /// Work was asked for. Its text is owed from now on.
    pub fn asked(&mut self, work: &Work) {
        match work {
            Work::Create { note, text } => {
                self.owed.insert(
                    *note,
                    Owed {
                        text: Some(text.clone()),
                        made: false,
                        failed: false,
                    },
                );
            }
            Work::Save { note, text } => {
                let owed = self.owed.entry(*note).or_insert(Owed {
                    text: None,
                    made: true,
                    failed: false,
                });
                owed.text = Some(text.clone());
            }
            _ => {}
        }
    }

    /// The text owed for `note`, if the store does not have it yet.
    #[must_use]
    pub fn text_of(&self, note: &NoteId) -> Option<&str> {
        self.owed.get(note)?.text.as_deref()
    }

    fn unmade(&self, note: &NoteId) -> bool {
        self.owed.get(note).is_some_and(|owed| !owed.made)
    }

    /// What to do, now that the turn of `work` has come. Work comes in the
    /// order asked, so a note still unmade here is one whose making failed:
    /// a save makes it, with the text owed; work on its files or its place
    /// makes it first; and letting it go, if it holds nothing, forgets it.
    #[must_use]
    pub fn plan(&self, work: Work) -> Vec<Step> {
        match work {
            Work::Save { note, text } if self.unmade(&note) => {
                vec![Step::Do(Work::Create { note, text })]
            }
            Work::LetGoIfEmpty { note } if self.unmade(&note) => {
                let text = self.text_of(&note).unwrap_or_default();
                if text.trim().is_empty() {
                    vec![Step::Forget(note)]
                } else {
                    vec![Step::Do(Work::Create {
                        note,
                        text: text.to_owned(),
                    })]
                }
            }
            Work::PutAway { note, .. } | Work::Attach { note, .. } if self.unmade(&note) => {
                let text = self.text_of(&note).unwrap_or_default().to_owned();
                vec![Step::Do(Work::Create { note, text }), Step::Do(work)]
            }
            work => vec![Step::Do(work)],
        }
    }

    /// The store did `work`.
    pub fn kept(&mut self, work: &Work) {
        let (Work::Create { note, text } | Work::Save { note, text }) = work else {
            return;
        };
        let Some(owed) = self.owed.get_mut(note) else {
            return;
        };
        owed.made = true;
        owed.failed = false;
        if owed.text.as_ref() == Some(text) {
            owed.text = None;
        }
        if owed.text.is_none() {
            self.owed.remove(note);
        }
    }

    /// Writing `work` failed. Its text stays owed, to be tried again.
    pub fn failed(&mut self, work: &Work) {
        if let Work::Create { note, .. } | Work::Save { note, .. } = work {
            if let Some(owed) = self.owed.get_mut(note) {
                owed.failed = true;
            }
        }
    }

    /// Nothing is owed for `note` any more.
    pub fn forget(&mut self, note: &NoteId) {
        self.owed.remove(note);
    }

    /// A note waits to be tried again.
    #[must_use]
    pub fn has_failed(&self) -> bool {
        self.owed.values().any(|owed| owed.failed)
    }

    /// The writes to try again: each note whose last write failed, made
    /// or saved with the text owed.
    #[must_use]
    pub fn to_try_again(&self) -> Vec<Work> {
        self.owed
            .iter()
            .filter(|(_, owed)| owed.failed)
            .map(|(note, owed)| {
                let text = owed.text.clone().unwrap_or_default();
                if owed.made {
                    Work::Save { note: *note, text }
                } else {
                    Work::Create { note: *note, text }
                }
            })
            .collect()
    }
}

/// How many times a write is tried before it is said to have failed, and
/// how long to wait before each try after the first. The store already
/// waits half a minute for a busy file within each try; what is left is
/// rarer (a full disk, a file that cannot be written).
const TRIES: [Duration; 3] = [
    Duration::ZERO,
    Duration::from_millis(400),
    Duration::from_millis(1600),
];

/// How often notes whose writes failed are tried again, while any are.
const TRY_AGAIN_EVERY: Duration = Duration::from_secs(10);

/// Does one piece of work.
pub async fn run(store: &Arc<Store>, work: Work) -> Done {
    match work {
        Work::Create { note, text } => {
            let made = match store.create_note_with_id(note, &text).await {
                Ok(_) => Ok(()),
                // Made by an earlier try that said it failed.
                Err(error) if matches!(store.get_note(&note).await, Ok(Some(_))) => {
                    tracing::warn!("The note was there already ({error}); saving it instead");
                    store.update_note(&note, &text).await.map(|_| ())
                }
                Err(error) => Err(error),
            };
            match made {
                Ok(()) => Done::Created(note),
                Err(error) => {
                    tracing::error!("Could not make the note: {error}");
                    Done::Unsaved {
                        note,
                        why: dirt_core::util::as_sentence(&error),
                    }
                }
            }
        }
        Work::Save { note, text } => match store.update_note(&note, &text).await {
            Ok(_) => Done::Saved(note),
            Err(error) => {
                tracing::error!("Could not save the note: {error}");
                Done::Unsaved {
                    note,
                    why: dirt_core::util::as_sentence(&error),
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
        Work::Attach { note, paths } => {
            let failed = attach_all(Arc::clone(store), note, paths).await;
            Done::Attached { note, failed }
        }
    }
}

/// Does a write, trying it again after a pause if it fails.
async fn run_with_tries(store: &Arc<Store>, work: &Work) -> Done {
    let writes = matches!(work, Work::Create { .. } | Work::Save { .. });
    let mut done = Done::Failed;
    for pause in TRIES {
        if !pause.is_zero() {
            tokio::time::sleep(pause).await;
        }
        done = run(store, work.clone()).await;
        if !writes || !matches!(done, Done::Unsaved { .. }) {
            break;
        }
    }
    done
}

/// One piece of work, with the store and session it is for: those of the
/// note when it was asked for, whatever is in use when it is done.
struct Job {
    store: Arc<Store>,
    session: Option<Session>,
    work: Work,
}

/// What the queue's task shares with those who ask of it.
struct Shared {
    ledger: Mutex<Ledger>,
    /// Pieces of work asked for and not yet done.
    waiting: AtomicUsize,
}

impl Shared {
    fn ledger(&self) -> std::sync::MutexGuard<'_, Ledger> {
        // A panic while the ledger was held leaves it as it was: still the
        // best account of what is owed.
        self.ledger
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Does the store's work for the page, in the order it was asked for.
///
/// There is one for the app, on a task of its own. The screen may go
/// (quick capture takes the window) with work still waiting: the work is
/// done all the same, only the news of it goes unheard, and the screen
/// that comes back finds what is still owed. Quitting waits for it
/// ([`Self::settle`]).
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
            ledger: Mutex::new(Ledger::default()),
            waiting: AtomicUsize::new(0),
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
            self.shared.ledger().asked(&work);
            self.shared.waiting.fetch_add(1, Ordering::SeqCst);
            let job = Job {
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

    /// The text owed for `note`, newer than the store's, if there is one.
    #[must_use]
    pub fn owed_text(&self, note: &NoteId) -> Option<String> {
        self.shared.ledger().text_of(note).map(str::to_owned)
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

/// The queue's task: the work in order, and now and then the writes that
/// failed, again.
async fn work_through(
    mut waiting: UnboundedReceiver<Job>,
    news: broadcast::Sender<Done>,
    shared: Arc<Shared>,
) {
    // Where to try a failed note again: the store and session it was for.
    let mut homes: HashMap<NoteId, (Arc<Store>, Option<Session>)> = HashMap::new();
    loop {
        let job = if shared.ledger().has_failed() {
            if let Ok(job) = tokio::time::timeout(TRY_AGAIN_EVERY, waiting.recv()).await {
                job
            } else {
                try_again(&shared, &homes, &news).await;
                continue;
            }
        } else {
            waiting.recv().await
        };
        let Some(job) = job else { break };
        let steps = shared.ledger().plan(job.work);
        for step in steps {
            let done = match step {
                Step::Forget(note) => {
                    shared.ledger().forget(&note);
                    Done::LetGo { note, gone: true }
                }
                Step::Do(work) => {
                    let done = run_with_tries(&job.store, &work).await;
                    record(
                        &shared,
                        &mut homes,
                        (&job.store, &job.session),
                        &work,
                        &done,
                    );
                    done
                }
            };
            if done.changed() {
                if let Some(session) = &job.session {
                    session.notify(Trigger::Changed);
                }
            }
            // Nobody may be listening: the screen has gone. The work is
            // done all the same.
            let _ = news.send(done);
        }
        shared.waiting.fetch_sub(1, Ordering::SeqCst);
        // A note that failed earlier is tried again with the next work, as
        // well as now and then.
        if shared.ledger().has_failed() {
            try_again(&shared, &homes, &news).await;
        }
    }
}

/// Writes down in the ledger how a piece of work went.
fn record(
    shared: &Shared,
    homes: &mut HashMap<NoteId, (Arc<Store>, Option<Session>)>,
    (store, session): (&Arc<Store>, &Option<Session>),
    work: &Work,
    done: &Done,
) {
    let mut ledger = shared.ledger();
    match done {
        Done::Created(_) | Done::Saved(_) => ledger.kept(work),
        Done::Unsaved { note, .. } => {
            ledger.failed(work);
            homes.insert(*note, (Arc::clone(store), session.clone()));
        }
        Done::LetGo { note, gone: true } => ledger.forget(note),
        _ => {}
    }
}

/// Tries again each note whose write failed. One that is kept now is said
/// to have been ([`Done::Kept`]).
async fn try_again(
    shared: &Shared,
    homes: &HashMap<NoteId, (Arc<Store>, Option<Session>)>,
    news: &broadcast::Sender<Done>,
) {
    let again = shared.ledger().to_try_again();
    for work in again {
        let (Work::Create { note, .. } | Work::Save { note, .. }) = &work else {
            continue;
        };
        let Some((store, session)) = homes.get(note) else {
            continue;
        };
        let done = run(store, work.clone()).await;
        if matches!(done, Done::Created(_) | Done::Saved(_)) {
            shared.ledger().kept(&work);
            if let Some(session) = session {
                session.notify(Trigger::Changed);
            }
            let _ = news.send(Done::Kept(*note));
        }
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
            paths: vec![file],
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
            paths: vec![file],
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

    // ---- What the store still owes -------------------------------------

    /// A note whose making failed is made by the next write asked for it,
    /// with the text owed then: what was typed into it is not lost.
    #[test]
    fn a_save_for_a_note_never_made_makes_it() {
        let note = NoteId::new();
        let mut ledger = Ledger::default();
        let create = Work::Create {
            note,
            text: "h".into(),
        };
        ledger.asked(&create);
        ledger.failed(&create);
        let save = Work::Save {
            note,
            text: "hello".into(),
        };
        ledger.asked(&save);
        assert_eq!(
            ledger.plan(save),
            [Step::Do(Work::Create {
                note,
                text: "hello".into()
            })]
        );
        assert_eq!(
            ledger.to_try_again(),
            [Work::Create {
                note,
                text: "hello".into()
            }]
        );
        ledger.kept(&Work::Create {
            note,
            text: "hello".into(),
        });
        assert!(!ledger.has_failed());
        assert_eq!(ledger.text_of(&note), None);
    }

    /// A failed save stays owed and is tried again; a later save that was
    /// kept settles it.
    #[test]
    fn a_failed_save_stays_owed() {
        let note = NoteId::new();
        let mut ledger = Ledger::default();
        let save = Work::Save {
            note,
            text: "one".into(),
        };
        ledger.asked(&save);
        ledger.failed(&save);
        assert_eq!(ledger.to_try_again(), std::slice::from_ref(&save));
        assert_eq!(ledger.text_of(&note), Some("one"));
        ledger.kept(&save);
        assert!(ledger.to_try_again().is_empty());
    }

    /// The text owed is the newest asked for. The store keeping an older
    /// one leaves the newer owed, which a note opened again shows.
    #[test]
    fn the_newest_text_is_owed_until_it_is_kept() {
        let note = NoteId::new();
        let mut ledger = Ledger::default();
        let older = Work::Save {
            note,
            text: "hello".into(),
        };
        let newer = Work::Save {
            note,
            text: "hello world".into(),
        };
        ledger.asked(&older);
        ledger.asked(&newer);
        ledger.kept(&older);
        assert_eq!(ledger.text_of(&note), Some("hello world"));
        ledger.kept(&newer);
        assert_eq!(ledger.text_of(&note), None);
    }

    /// A note never made: let go when it holds nothing, made when it holds
    /// something, and made before its files or its place are seen to.
    #[test]
    fn work_on_a_note_never_made_makes_it_first() {
        let note = NoteId::new();
        let mut ledger = Ledger::default();
        let create = Work::Create {
            note,
            text: "x".into(),
        };
        ledger.asked(&create);
        ledger.failed(&create);
        let put_away = Work::PutAway {
            note,
            delete: false,
        };
        assert_eq!(
            ledger.plan(put_away.clone()),
            [
                Step::Do(Work::Create {
                    note,
                    text: "x".into()
                }),
                Step::Do(put_away)
            ]
        );
        assert_eq!(
            ledger.plan(Work::LetGoIfEmpty { note }),
            [Step::Do(Work::Create {
                note,
                text: "x".into()
            })]
        );
        ledger.asked(&Work::Save {
            note,
            text: " ".into(),
        });
        assert_eq!(
            ledger.plan(Work::LetGoIfEmpty { note }),
            [Step::Forget(note)]
        );
        // A note the store has is let go by the store.
        let held = NoteId::new();
        assert_eq!(
            ledger.plan(Work::LetGoIfEmpty { note: held }),
            [Step::Do(Work::LetGoIfEmpty { note: held })]
        );
    }

    /// The queue against a real store: what is asked is done in order, and
    /// nothing is owed once it has settled.
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

    /// A note that cannot be made is said to be unsaved, and its text
    /// stays owed: shown if the note is opened again, and tried again.
    #[tokio::test]
    async fn a_note_that_cannot_be_made_is_said_and_kept_owed() {
        let (_folder, store) = temp_store();
        let scribe = Scribe::start();
        let mut news = scribe.listen();
        // The store makes notes only under IDs that begin with their time.
        let note = NoteId::from_uuid(uuid::Uuid::from_u128(
            0x0123_4567_89ab_4cde_8f01_2345_6789_abcd,
        ));
        scribe.ask(
            &store,
            None,
            [
                Work::Create {
                    note,
                    text: "a".into(),
                },
                Work::Save {
                    note,
                    text: "a thought".into(),
                },
            ],
        );
        assert!(scribe.settle(Duration::from_secs(20)).await);
        assert!(matches!(news.recv().await, Ok(Done::Unsaved { .. })));
        assert_eq!(scribe.owed_text(&note).as_deref(), Some("a thought"));
        assert_eq!(store.note_count().await.unwrap(), 0);
    }
}
