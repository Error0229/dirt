//! What the two apps' lists share.
//!
//! What Find asks for, what is read for it, how it is grouped, and the
//! tags offered (interface design, sections 6.1 to 6.3, 6.17 and 6.19);
//! and how sync has the notes read again.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::time::Duration;

use dioxus::prelude::*;
use dirt_core::models::{Layer, Note, NoteId, Words};
use dirt_core::session::{Session, SessionState};
use dirt_core::store::{NoteCounts, TagCount, TagRule};
use dirt_core::sync::worker::SyncStatus;
use dirt_core::Store;

use crate::components::NoteSummary;
use crate::find;
use crate::pile::PileView;
use crate::time::Clock;

/// How many notes the list holds at first, and how many more it takes
/// each time scrolling nears its end.
pub const LISTED_NOTES: usize = 100;

/// How many tags are offered besides those picked (6.3).
const TAGS_OFFERED: usize = 6;

/// What the list shows, read from the store at once.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Listing {
    /// The notes, in the order shown: when finding, the archived ones
    /// after the rest.
    pub notes: Vec<Note>,
    /// Notes that have not reached the server as they are now. Empty while
    /// signed out, when no note is meant to.
    pub waiting: HashSet<NoteId>,
    /// Notes with files.
    pub with_files: HashSet<NoteId>,
    /// The first file's name of each note that has files and no words.
    pub first_file: HashMap<NoteId, String>,
    /// Notes said on this device whose words it has stopped trying to
    /// make, with why (6.18).
    pub unread: HashMap<NoteId, String>,
    /// The thumbnail of a picture on each note that has one on the device,
    /// where the list shows them.
    pub thumbnails: HashMap<NoteId, String>,
    pub counts: NoteCounts,
    /// Find holds words or tags.
    pub finding: bool,
    /// The list shows every archived note ("3 archived" was pressed).
    pub archived_view: bool,
    /// There may be more than the list holds.
    pub more: bool,
    /// What Find's words ask for, to mark in the list.
    pub words: Vec<String>,
    /// The pile under the last note (6.17): read while the list is not
    /// finding, and kept as it was while it is.
    pub pile: PileView,
}

/// What the list shows of notes' files, as an app reads it.
#[derive(Debug, Default)]
pub struct ListedFiles {
    /// Notes with files: at least those shown that have some.
    pub with_files: HashSet<NoteId>,
    /// The first file's name of some of them: only one with no words is
    /// shown by it.
    pub first_file: HashMap<NoteId, String>,
    /// The thumbnails the list shows.
    pub thumbnails: HashMap<NoteId, String>,
}

/// One thing in the list: a note, or the line a group starts with.
pub enum ListItem {
    Line(String),
    Note(Box<NoteSummary>),
}

impl Listing {
    /// The list's notes and lines, in order: grouped in layers by when each
    /// note was last edited (6.17), or, while finding, the archived notes
    /// after the rest under a line of their own (6.19).
    #[must_use]
    pub fn items(&self, clock: Clock) -> Vec<ListItem> {
        let mut items = Vec::with_capacity(self.notes.len() + 8);
        let mut layer = None;
        let mut archived_line = false;
        for note in &self.notes {
            if self.finding {
                if note.archived && !archived_line {
                    archived_line = true;
                    items.push(ListItem::Line("Archived".to_owned()));
                }
            } else if !self.archived_view {
                let here = Layer::of(note.updated_at, clock.now_ms, clock.utc_offset_secs);
                if layer != Some(here) {
                    layer = Some(here);
                    if let Some(name) = here.name() {
                        items.push(ListItem::Line(name));
                    }
                }
            }
            let summary = NoteSummary::of(note, &self.words, clock)
                .files(
                    self.with_files.contains(&note.id),
                    self.first_file.get(&note.id).map(String::as_str),
                )
                .waiting(self.waiting.contains(&note.id))
                .unread(self.unread.get(&note.id).map(String::as_str))
                .thumbnail(self.thumbnails.get(&note.id).cloned());
            items.push(ListItem::Note(Box::new(summary)));
        }
        items
    }
}

/// What the list is read for: Find's words and tags, the archived notes,
/// and how many.
#[derive(Clone, Copy)]
pub struct Find {
    /// Find's text: words, and perhaps the start of a tag at its end.
    pub text: Signal<String>,
    /// The tags chosen, in lower case and without their `#`.
    pub tags: Signal<Vec<String>>,
    pub rule: Signal<TagRule>,
    /// The tags are offered.
    pub tags_open: Signal<bool>,
    /// The list shows the archived notes alone.
    pub archived_view: Signal<bool>,
    /// How many notes the list holds.
    pub limit: Signal<usize>,
}

impl Find {
    /// Find, empty.
    pub fn use_new() -> Self {
        use_hook(|| Self {
            text: Signal::new(String::new()),
            tags: Signal::new(Vec::new()),
            rule: Signal::new(TagRule::All),
            tags_open: Signal::new(false),
            archived_view: Signal::new(false),
            limit: Signal::new(LISTED_NOTES),
        })
    }

    /// The tags offered to narrow what Find finds, from the store `store`
    /// reads: while they are shown, and while Find holds a lone `#tag`,
    /// which Enter picks the first of.
    pub fn use_tag_offers(
        self,
        store: impl Fn() -> Option<Store> + 'static,
        notes_read: Signal<u64>,
    ) -> Resource<Vec<TagCount>> {
        use_resource(move || {
            let text = self.text.read().clone();
            let wanted = *self.tags_open.read() || find::is_lone_tag(&text);
            let store = store().filter(|_| wanted);
            let _ = notes_read.read();
            let tags = self.tags.read().clone();
            let rule = *self.rule.read();
            async move {
                let Some(store) = store else {
                    return Vec::new();
                };
                let words = find::words(&text).trim().to_owned();
                let starting = find::tag_word(&text);
                store
                    .tags_to_narrow(&words, &tags, rule, starting, TAGS_OFFERED)
                    .await
                    .unwrap_or_else(|error| {
                        tracing::error!("Could not read the tags: {error}");
                        Vec::new()
                    })
            }
        })
    }

    /// Reads what the list shows from the store `store` reads, again
    /// whenever what it depends on changes, and marks the notes a sync has
    /// just brought (`pulled` counts the syncs that brought some), other
    /// than the one `open`. `files` reads what the list shows of the
    /// files of the notes shown.
    ///
    /// Returns the listing, `None` until the store has answered, and the
    /// notes to flash.
    pub fn use_listing<Fut>(
        self,
        store: impl Fn() -> Option<Store> + 'static,
        notes_read: Signal<u64>,
        pulled: Signal<u64>,
        session: Signal<SessionState>,
        open: Signal<Option<NoteId>>,
        files: impl Fn(Store, &[Note]) -> Fut + Copy + 'static,
    ) -> (Signal<Option<Listing>>, Signal<HashSet<NoteId>>)
    where
        Fut: Future<Output = ListedFiles> + 'static,
    {
        let mut listing = use_signal(|| None::<Listing>);
        let mut flashing = use_signal(HashSet::new);
        let mut seen = use_signal(HashMap::<NoteId, i64>::new);
        let mut pulls_seen = use_signal(|| 0_u64);
        use_resource(move || {
            let store = store();
            let _ = notes_read.read();
            let pulled = *pulled.read();
            let signed_in = session.read().is_ready();
            let text = self.text.read().clone();
            let tags = self.tags.read().clone();
            let rule = *self.rule.read();
            let limit = *self.limit.read();
            let archived_view = *self.archived_view.read();
            async move {
                let store = store?;
                let words = find::words(&text).trim().to_owned();
                let finding = !archived_view && (!words.is_empty() || !tags.is_empty());
                let notes = if archived_view {
                    store.list_archived_notes(limit, 0).await
                } else if finding {
                    store.find_notes_by_tags(&words, &tags, rule, limit).await
                } else {
                    store.list_notes(limit, 0).await
                };
                let notes = notes
                    .inspect_err(|error| tracing::error!("Could not read the list: {error}"))
                    .ok()?;
                let shown: Vec<NoteId> = notes.iter().map(|note| note.id).collect();
                let waiting = if signed_in {
                    store.unsynced_note_ids(shown).await.unwrap_or_default()
                } else {
                    HashSet::new()
                };
                let ListedFiles {
                    with_files,
                    mut first_file,
                    thumbnails,
                } = files(store.clone(), &notes).await;
                // Only a note with no words goes by its first file's name.
                first_file.retain(|id, _| {
                    notes
                        .iter()
                        .any(|note| note.id == *id && note.outline().title.is_empty())
                });
                let waiting_words: Vec<NoteId> = notes
                    .iter()
                    // A note said waiting for its words, or one with files:
                    // a recording added to a note (8.10) is one of its
                    // files, and leaves the note unmarked.
                    .filter(|note| {
                        with_files.contains(&note.id)
                            || note
                                .voice
                                .is_some_and(|voice| voice.words == Words::Waiting)
                    })
                    .map(|note| note.id)
                    .collect();
                let unread = if waiting_words.is_empty() {
                    HashMap::new()
                } else {
                    store
                        .voice_problems(waiting_words)
                        .await
                        .unwrap_or_default()
                };
                let counts = store.note_counts().await.unwrap_or_default();
                let more = notes.len() >= limit;
                // The pile's notes are read only when the counts say they
                // may have changed: not at every pause in typing (6.17).
                let before = listing.peek().as_ref().map(|listing| listing.pile.clone());
                let offset = Clock::now().utc_offset_secs;
                let pile = PileView::read(before, counts, offset, || store.pile()).await;

                // What a sync has just brought: new to the list, or edited
                // since it was last read, and not the note being written.
                if pulled != *pulls_seen.peek() && !finding && !archived_view {
                    let open = *open.peek();
                    let before = seen.peek();
                    let arrived: HashSet<NoteId> = notes
                        .iter()
                        .filter(|note| Some(note.id) != open)
                        .filter(|note| before.get(&note.id).is_none_or(|&at| note.updated_at > at))
                        .map(|note| note.id)
                        .collect();
                    let flash = !arrived.is_empty() && !before.is_empty();
                    drop(before);
                    if flash {
                        flashing.set(arrived.clone());
                        spawn(async move {
                            tokio::time::sleep(Duration::from_millis(1700)).await;
                            if *flashing.peek() == arrived {
                                flashing.set(HashSet::new());
                            }
                        });
                    }
                }
                pulls_seen.set(pulled);
                if !finding && !archived_view {
                    seen.set(
                        notes
                            .iter()
                            .map(|note| (note.id, note.updated_at))
                            .collect(),
                    );
                }

                listing.set(Some(Listing {
                    notes,
                    waiting,
                    with_files,
                    first_file,
                    unread,
                    thumbnails,
                    counts,
                    finding,
                    archived_view,
                    more,
                    words: crate::text::find_words(&words),
                    pile,
                }));
                Some(())
            }
        });
        (listing, flashing)
    }
}

/// Follows sync for as long as there is a session.
///
/// Its status goes into `status`. The notes are read again (`notes_read`)
/// when a cycle has brought notes from another device, counted in
/// `pulled`, or sent some.
pub fn use_sync_watch(
    session: Signal<Option<Session>>,
    mut status: Signal<SyncStatus>,
    mut pulled: Signal<u64>,
    mut notes_read: Signal<u64>,
) {
    use_resource(move || {
        let current = session();
        async move {
            let Some(current) = current else { return };
            let mut statuses = current.watch_sync();
            let mut pulled_before = statuses.borrow().pulled;
            let mut synced_before = statuses.borrow().last_synced_ms;
            while statuses.changed().await.is_ok() {
                let now = statuses.borrow_and_update().clone();
                if now.pulled != pulled_before {
                    pulled_before = now.pulled;
                    pulled += 1;
                    notes_read += 1;
                } else if now.last_synced_ms != synced_before {
                    // A cycle that only sent: the notes it sent are on the
                    // server now, and the list stops marking them.
                    notes_read += 1;
                }
                synced_before = now.last_synced_ms;
                status.set(now);
            }
        }
    });
}

/// Keeps the clock: every half minute, for the times in the list, and
/// soon after sync changes, for "Just synced" giving way.
pub fn use_clock(sync: Signal<SyncStatus>) -> Signal<Clock> {
    let mut clock = use_signal(Clock::now);
    use_future(move || async move {
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;
            clock.set(Clock::now());
        }
    });
    use_effect(move || {
        let _ = sync.read();
        clock.set(Clock::now());
        spawn(async move {
            tokio::time::sleep(Duration::from_millis(1500)).await;
            clock.set(Clock::now());
        });
    });
    clock
}
