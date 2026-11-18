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

use std::path::PathBuf;
use std::sync::Arc;

use dirt_core::models::{Note, NoteId};
use dirt_core::session::Session;
use dirt_core::store::LetGo;
use dirt_core::Store;
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
    /// The work failed; it is logged.
    Failed,
}

impl Done {
    /// The notes in the store changed, and whatever shows them reads them
    /// again.
    #[must_use]
    pub const fn changed(&self) -> bool {
        !matches!(self, Self::Failed | Self::LetGo { gone: false, .. })
    }
}

/// Does one piece of work.
pub async fn run(store: &Arc<Store>, work: Work) -> Done {
    match work {
        Work::Create { note, text } => match store.create_note_with_id(note, &text).await {
            Ok(_) => Done::Created(note),
            Err(error) => {
                tracing::error!("Could not make the note: {error}");
                Done::Failed
            }
        },
        Work::Save { note, text } => match store.update_note(&note, &text).await {
            Ok(_) => Done::Saved(note),
            Err(error) => {
                tracing::error!("Could not save the note: {error}");
                Done::Failed
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

/// One piece of work, with the store and session it is for: those in use
/// when it was asked for, whatever is in use when it is done.
struct Job {
    store: Arc<Store>,
    session: Option<Session>,
    work: Work,
}

/// Does the store's work for the page, in the order it was asked for.
///
/// It runs on a task of its own. The screen may go (quick capture takes
/// the window) with work still waiting; the work is done all the same,
/// and only the news of it is lost.
#[derive(Clone)]
pub struct Scribe {
    jobs: UnboundedSender<Job>,
}

impl Scribe {
    /// Starts the task. Its news comes on the receiver.
    #[must_use]
    pub fn start() -> (Self, UnboundedReceiver<Done>) {
        let (jobs, mut waiting) = unbounded_channel::<Job>();
        let (news, heard) = unbounded_channel();
        tokio::spawn(async move {
            while let Some(job) = waiting.recv().await {
                let done = run(&job.store, job.work).await;
                if done.changed() {
                    if let Some(session) = &job.session {
                        session.notify(dirt_core::sync::worker::Trigger::Changed);
                    }
                }
                // Nobody may be listening any more; the work is done.
                let _ = news.send(done);
            }
        });
        (Self { jobs }, heard)
    }

    /// Asks for work to be done after everything asked for before it.
    pub fn ask(
        &self,
        store: &Arc<Store>,
        session: Option<&Session>,
        work: impl IntoIterator<Item = Work>,
    ) {
        for work in work {
            let job = Job {
                store: Arc::clone(store),
                session: session.cloned(),
                work,
            };
            if self.jobs.send(job).is_err() {
                tracing::error!("The page's work could not be handed on");
            }
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
}
