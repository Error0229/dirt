//! Android's main screen (interface design, sections 3, 6.1 to 6.7, 6.14,
//! 6.19 and 8): the title with 🔍, the sync status and Settings; Find,
//! which slides down under it; the list of cards; and the dock, the bar a
//! new page grows out of. A note opens as a page over all of it, grown out
//! of its card.
//!
//! [`Screen`] holds what the screen's parts share: what Find holds, what
//! the list shows, the page and what is written on it, and the last
//! archive or delete, for Undo. Its methods are what the person can do
//! here; the parts call them from their buttons and gestures.
//!
//! # Leaving a page
//!
//! The page closes the moment it is let go, and what it held is saved
//! afterwards, from what it held then: each page has a [`Draft`] of its
//! own, which its typing writes to and its leaving reads. A page opened
//! straight after is a new page with a draft of its own, so nothing typed
//! into it is lost to the one being saved, and a note whose first
//! character is still being made is made, and saved, all the same.

mod list;
mod page;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use dioxus::prelude::*;
use dirt_core::files::thumbnail::as_data_uri;
use dirt_core::models::{extract_tags, Note, NoteId};
use dirt_core::store::{LetGo, NoteCounts, TagCount, TagRule};
use dirt_core::sync::overview::SyncIndicator;
use dirt_core::sync::worker::Trigger;
use dirt_ui::components::{card_element_id, use_toasts, GrowFrom, ListExit, Toast, Toasts};
use dirt_ui::dom::{self, Caret};
use dirt_ui::find;
use dirt_ui::time::Clock;
use dirt_ui::Icon;
use uuid::Uuid;

use crate::data::MobileNoteStore;
use crate::incoming::{
    attach_incoming, discard_all, not_copied_in, note_text, worth_a_note, Capture,
};
use crate::services::activity::take_capture;
use crate::state::{AppState, View};
use crate::views::attachments::{attach, choose_files as pick_files};
use list::Main;
use page::Page;

/// The page's text field.
const PAGE: &str = "#dm-text";
const FIND: &str = "#dui-find";

/// The bar a new page grows out of.
pub const BAR_ID: &str = "dm-bar";

/// How long typing pauses before what was typed is saved.
const SAVE_AFTER: Duration = Duration::from_millis(400);

/// How many notes the list holds at first, and how many more it takes
/// each time scrolling nears its end.
pub const LISTED_NOTES: usize = 100;

/// How many tags the strip offers besides those picked (6.3).
const TAGS_OFFERED: usize = 6;

/// How long a page takes to shrink back into its card (5.5). A note put
/// away from its page is put away once it has.
const CLOSE: Duration = Duration::from_millis(340);

/// A thumbnail larger than this is not shown on a card.
const MOST_THUMBNAIL_BYTES: u64 = 512 * 1024;

/// What one page holds: what was typed last, and what the store has of it.
/// Its typing writes here, and its leaving reads it, so that a page left
/// and a page opened straight after never share one.
#[derive(Default)]
pub struct Draft {
    /// The store the page was opened on. Its writes go there, even if
    /// another store is taken into use while it is being left.
    store: Option<Arc<MobileNoteStore>>,
    /// The note, once there is one.
    note: Option<NoteId>,
    text: String,
    saved: String,
    version: u64,
    /// The first character has been typed and the store has not made the
    /// note yet.
    creating: bool,
    /// A save is being written; another waits for it.
    saving: bool,
    /// The page has been let go: nothing more is typed into it.
    left: bool,
}

/// One page's draft, shared by its typing and its leaving.
pub type DraftCell = Rc<RefCell<Draft>>;

/// What the page shows. The page is always in the screen; `open` says
/// whether it is up.
#[derive(Clone, Default)]
pub struct PageView {
    pub serial: u64,
    pub open: bool,
    /// The note shown, once there is one.
    pub note: Option<NoteId>,
    pub archived: bool,
    /// It grew out of the bar: a new page, done with ⌄.
    pub new_page: bool,
    /// What it grows out of, and goes back into.
    pub from: GrowFrom,
    draft: DraftCell,
}

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
    /// The thumbnail of a picture on each note that has one on the device.
    pub thumbnails: HashMap<NoteId, String>,
    pub counts: NoteCounts,
    /// Find holds words or tags.
    pub finding: bool,
    /// The list shows every archived note ("3 archived" was pressed).
    pub archived_view: bool,
    /// There may be more than the list holds.
    pub more: bool,
    /// What Find's words ask for, to mark in the cards.
    pub words: Vec<String>,
}

/// What was last archived or deleted, for Undo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Undo {
    serial: u64,
    note: NoteId,
    deleted: bool,
}

/// What the screen's parts share.
#[derive(Clone, Copy)]
pub struct Screen {
    app: AppState,
    toasts: Toasts,
    /// Find is down under the title.
    pub find_open: Signal<bool>,
    /// Find's text: words, and perhaps the start of a tag at its end.
    pub find_text: Signal<String>,
    /// The tags chosen, in lower case and without their `#`.
    pub tags: Signal<Vec<String>>,
    pub rule: Signal<TagRule>,
    /// The `#` is pressed: the strip offers the tags that can be picked.
    pub tags_open: Signal<bool>,
    /// What the strip offers now.
    pub tag_offers: Signal<Vec<TagCount>>,
    /// The list shows the archived notes alone.
    pub archived_view: Signal<bool>,
    /// How many notes the list holds.
    pub limit: Signal<usize>,
    /// How a card that leaves the list is drawn leaving.
    pub exit: Signal<ListExit>,
    /// A note just brought back by Undo: its card drops in with a bounce.
    pub returned: Signal<Option<NoteId>>,
    /// Notes just arrived from another device: their cards flash once.
    pub flashing: Signal<HashSet<NoteId>>,
    pub listing: Signal<Option<Listing>>,
    pub page: Signal<PageView>,
    /// The drafts of pages being left, until the store has what they held.
    leaving: Signal<Vec<DraftCell>>,
    /// Typing on the page: its top bar fades until the page is touched.
    pub typing: Signal<bool>,
    /// Raised when the open note's files change here, to read them again.
    pub files_changed: Signal<u64>,
    last_undo: Signal<Option<Undo>>,
    serial: Signal<u64>,
    /// Thumbnails read from the store, by the thumbnail's file ID, so that
    /// a card's picture is read once.
    thumbs: Signal<HashMap<Uuid, String>>,
    /// The activity is being asked what came from outside the app.
    receiving: Signal<bool>,
    /// What time it is, for the times on cards and the sync status.
    pub clock: Signal<Clock>,
}

impl Screen {
    fn next_serial(mut self) -> u64 {
        let mut serial = self.serial.write();
        *serial += 1;
        *serial
    }

    fn store(self) -> Option<Arc<MobileNoteStore>> {
        self.app.store.peek().clone()
    }

    /// Waits until the pages being left have been saved, so that what is
    /// read from the store next has what they held.
    async fn settled(self) {
        for _ in 0..300 {
            if self.leaving.peek().is_empty() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    // ---- The page -----------------------------------------------------------

    /// Tapping the bar: a new page grows out of it, with the keyboard up.
    pub fn new_page(mut self) {
        if self.page.peek().open {
            return;
        }
        let serial = self.next_serial();
        let draft = Rc::new(RefCell::new(Draft {
            store: self.store(),
            ..Draft::default()
        }));
        dom::set_value(PAGE, "");
        self.typing.set(false);
        self.page.set(PageView {
            serial,
            open: true,
            note: None,
            archived: false,
            new_page: true,
            from: GrowFrom::Bar(BAR_ID.to_owned()),
            draft,
        });
        self.app.current_note_id.set(None);
        focus_soon(PAGE, Caret::End);
    }

    /// Tapping a card: the note grows out of it, with the cursor at the end
    /// of its text. What it shows at once is what the list or a page being
    /// left has of it; the store is asked straight after.
    pub fn open(mut self, id: NoteId) {
        if self.page.peek().open {
            return;
        }
        let being_left = self
            .leaving
            .peek()
            .iter()
            .find(|draft| draft.borrow().note == Some(id))
            .map(|draft| draft.borrow().text.clone());
        let listed = self.listing.peek().as_ref().and_then(|listing| {
            listing
                .notes
                .iter()
                .find(|note| note.id == id)
                .map(|note| (note.content.clone(), note.archived))
        });
        let archived = listed.as_ref().is_some_and(|(_, archived)| *archived);
        let text = being_left
            .or_else(|| listed.map(|(text, _)| text))
            .unwrap_or_default();
        let serial = self.next_serial();
        let draft = Rc::new(RefCell::new(Draft {
            store: self.store(),
            note: Some(id),
            text: text.clone(),
            saved: text.clone(),
            ..Draft::default()
        }));
        dom::set_value(PAGE, &text);
        self.typing.set(false);
        self.page.set(PageView {
            serial,
            open: true,
            note: Some(id),
            archived,
            new_page: false,
            from: GrowFrom::Card(card_element_id(&id)),
            draft: Rc::clone(&draft),
        });
        self.app.current_note_id.set(Some(id));
        focus_soon(PAGE, Caret::End);
        spawn(async move {
            self.settled().await;
            let Some(store) = draft.borrow().store.clone() else {
                return;
            };
            let found = store.core().get_note(&id).await;
            if self.page.peek().serial != serial {
                return;
            }
            match found {
                Ok(Some(note)) => {
                    let untouched = draft.borrow().version == 0;
                    if untouched {
                        let mut held = draft.borrow_mut();
                        if held.text != note.content {
                            dom::set_value(PAGE, &note.content);
                            held.text.clone_from(&note.content);
                        }
                        held.saved.clone_from(&note.content);
                    }
                    self.page.write().archived = note.archived;
                }
                // Gone meanwhile: deleted on another device and purged.
                Ok(None) => self.done(),
                Err(error) => tracing::error!("Could not read the note: {error}"),
            }
        });
    }

    /// The page's text changed: typed, pasted or cut. The first character
    /// that is not a space makes the note (6.1); from then on it saves
    /// itself, a moment after typing pauses.
    pub fn typed(mut self, text: String) {
        if !*self.typing.peek() {
            self.typing.set(true);
        }
        let view = self.page.peek().clone();
        if !view.open {
            return;
        }
        let draft = view.draft;
        let (version, first) = {
            let mut held = draft.borrow_mut();
            if held.left {
                return;
            }
            held.text.clone_from(&text);
            held.version += 1;
            let first = held.note.is_none() && !held.creating && !text.trim().is_empty();
            if first {
                held.creating = true;
            }
            (held.version, first)
        };
        if first {
            // With the list narrowed to tags, a new page gets them at its
            // end, where they can be seen and removed. The field says so as
            // if they had been typed, and that saves them.
            let written = extract_tags(&text);
            let missing: Vec<String> = self
                .tags
                .peek()
                .iter()
                .filter(|tag| !written.contains(tag))
                .map(|tag| format!("#{tag}"))
                .collect();
            if !missing.is_empty() {
                dom::append_text(PAGE, &format!(" {}", missing.join(" ")));
            }
            spawn(async move { self.create(draft, view.serial, text).await });
            return;
        }
        spawn(async move {
            tokio::time::sleep(SAVE_AFTER).await;
            let current = draft.borrow().version == version;
            if current {
                self.save(&draft).await;
            }
        });
    }

    async fn create(mut self, draft: DraftCell, serial: u64, text: String) {
        let Some(store) = draft.borrow().store.clone() else {
            draft.borrow_mut().creating = false;
            return;
        };
        match store.core().create_note(&text).await {
            Ok(note) => {
                {
                    let mut held = draft.borrow_mut();
                    held.note = Some(note.id);
                    held.saved = text;
                    held.creating = false;
                }
                let (same_page, open) = {
                    let page = self.page.peek();
                    (page.serial == serial, page.open)
                };
                if same_page {
                    let mut page = self.page.write();
                    page.note = Some(note.id);
                    if open {
                        page.from = GrowFrom::Card(card_element_id(&note.id));
                    }
                }
                if same_page && open {
                    self.app.current_note_id.set(Some(note.id));
                }
                self.app.note_changed();
                self.save(&draft).await;
            }
            Err(error) => {
                tracing::error!("Could not make the note: {error}");
                draft.borrow_mut().creating = false;
            }
        }
    }

    /// Saves what was typed, if the store does not have it yet. One save is
    /// written at a time; one asked for meanwhile is written after it, with
    /// what was typed by then.
    async fn save(self, draft: &DraftCell) {
        {
            let mut held = draft.borrow_mut();
            if held.saving {
                return;
            }
            held.saving = true;
        }
        let store = draft.borrow().store.clone();
        let mut changed = false;
        while let Some(store) = store.as_ref() {
            let next = {
                let held = draft.borrow();
                match held.note {
                    Some(id) if held.text != held.saved => Some((id, held.text.clone())),
                    _ => None,
                }
            };
            let Some((id, text)) = next else { break };
            match store.core().update_note(&id, &text).await {
                Ok(_) => {
                    draft.borrow_mut().saved = text;
                    changed = true;
                }
                Err(error) => {
                    tracing::error!("Could not save the note: {error}");
                    break;
                }
            }
        }
        draft.borrow_mut().saving = false;
        if changed {
            self.app.note_changed();
        }
    }

    /// Saves a page that has been let go, from what it held, and lets its
    /// note go if it was emptied again (6.1). Returns the note, unless
    /// there was none or it was let go.
    async fn leave(mut self, draft: DraftCell) -> Option<NoteId> {
        draft.borrow_mut().left = true;
        self.leaving.write().push(Rc::clone(&draft));
        let kept = self.settle(&draft).await;
        self.leaving
            .write()
            .retain(|other| !Rc::ptr_eq(other, &draft));
        kept
    }

    async fn settle(self, draft: &DraftCell) -> Option<NoteId> {
        // A note still being made is waited for, so that it is not lost.
        for _ in 0..300 {
            if !draft.borrow().creating {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        self.save(draft).await;
        for _ in 0..300 {
            if !draft.borrow().saving {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let (id, empty, store) = {
            let held = draft.borrow();
            (held.note?, held.text.trim().is_empty(), held.store.clone())
        };
        if empty {
            if let Some(store) = store {
                match store.core().let_go_if_empty(&id).await {
                    Ok(LetGo::Kept) => {}
                    Ok(LetGo::Dropped | LetGo::Deleted | LetGo::Gone) => {
                        self.app.note_changed();
                        return None;
                    }
                    Err(error) => tracing::error!("Could not let the empty note go: {error}"),
                }
            }
        }
        Some(id)
    }

    /// Closes the page into `into`, and puts the keyboard away.
    fn close_page(mut self, into: GrowFrom) {
        if let GrowFrom::Card(card) = &into {
            // Into its card wherever it is now, which is in sight.
            dom::reveal(card);
        }
        dom::blur();
        {
            let mut page = self.page.write();
            page.open = false;
            page.from = into;
        }
        self.app.current_note_id.set(None);
        self.typing.set(false);
    }

    /// Done with the page: ⌄, ‹ or Back. It closes at once, into its card,
    /// or into the bar when nothing was written on a new page, and is
    /// saved from what it held.
    pub fn done(self) {
        let view = self.page.peek().clone();
        if !view.open {
            return;
        }
        let (note, empty) = {
            let held = view.draft.borrow();
            (held.note, held.text.trim().is_empty())
        };
        let into = match note {
            _ if view.new_page && empty => GrowFrom::Bar(BAR_ID.to_owned()),
            Some(id) => GrowFrom::Card(card_element_id(&id)),
            None => GrowFrom::Nowhere,
        };
        self.close_page(into);
        spawn(async move {
            self.leave(view.draft).await;
        });
    }

    /// The box, or the bin, in the page's bar: the page goes back into its
    /// card, and then the note is archived or deleted. A note emptied on
    /// the page is let go as it is left: there is nothing to put away, and
    /// nothing to undo.
    pub fn put_away_open(self, delete: bool) {
        let view = self.page.peek().clone();
        if !view.open {
            return;
        }
        let note = view.draft.borrow().note;
        self.close_page(note.map_or(GrowFrom::Nowhere, |id| GrowFrom::Card(card_element_id(&id))));
        spawn(async move {
            let began = tokio::time::Instant::now();
            let Some(id) = self.leave(view.draft).await else {
                return;
            };
            tokio::time::sleep_until(began + CLOSE).await;
            let exit = if delete {
                ListExit::Tear
            } else {
                ListExit::Fold
            };
            self.put_away(id, delete, exit);
        });
    }

    /// "Back to the list", on an archived note's page. Not an edit.
    pub fn unarchive_open(mut self) {
        let Some(id) = self.page.peek().note else {
            return;
        };
        spawn(async move {
            let Some(store) = self.store() else { return };
            if let Err(error) = store.core().unarchive_note(&id).await {
                tracing::error!("Could not bring the note back: {error}");
                return;
            }
            if self.page.peek().note == Some(id) {
                self.page.write().archived = false;
            }
            self.app.note_changed();
        });
    }

    // ---- Archive, delete, undo (6.19, 6.7) ----------------------------------

    /// A card swiped away: archived, or deleted if it is archived already.
    /// It has flown off; the list closes up after it.
    pub fn swiped(self, id: NoteId) {
        let archived = self.listing.peek().as_ref().is_some_and(|listing| {
            listing
                .notes
                .iter()
                .any(|note| note.id == id && note.archived)
        });
        self.put_away(id, archived, ListExit::Fold);
    }

    /// Archives or deletes a note, and says so with **Undo**.
    fn put_away(mut self, id: NoteId, delete: bool, exit: ListExit) {
        let serial = self.next_serial();
        let undo = Callback::new(move |()| self.undo(serial));
        spawn(async move {
            // A page of this note being left is saved first.
            self.settled().await;
            let Some(store) = self.store() else { return };
            self.exit.set(exit);
            let done = if delete {
                store.core().delete_note(&id).await
            } else {
                store.core().archive_note(&id).await.map(|_| ())
            };
            if let Err(error) = done {
                tracing::error!("Could not put the note away: {error}");
                // A card swiped away comes back.
                self.app.reread();
                return;
            }
            self.last_undo.set(Some(Undo {
                serial,
                note: id,
                deleted: delete,
            }));
            self.app.note_changed();
            let toast = if delete {
                Toast::new("Note deleted").icon(Icon::Trash)
            } else {
                Toast::new("Archived").icon(Icon::Archive)
            };
            self.toasts.show(toast.undo(undo));
        });
    }

    /// Undoes the archive or delete numbered `serial`, which the toast's
    /// **Undo** offered. If something newer has been put away since, that
    /// toast is out of date and does nothing.
    pub fn undo(mut self, serial: u64) {
        let taken = {
            let mut last = self.last_undo.write();
            match *last {
                Some(undo) if undo.serial == serial => last.take(),
                _ => None,
            }
        };
        let Some(undo) = taken else { return };
        spawn(async move {
            let Some(store) = self.store() else { return };
            let done = if undo.deleted {
                store.core().restore_note(&undo.note).await.map(|_| ())
            } else {
                store.core().unarchive_note(&undo.note).await.map(|_| ())
            };
            if let Err(error) = done {
                tracing::error!("Could not undo: {error}");
                return;
            }
            self.exit.set(ListExit::None);
            self.returned.set(Some(undo.note));
            self.app.note_changed();
            tokio::time::sleep(Duration::from_millis(1200)).await;
            if *self.returned.peek() == Some(undo.note) {
                self.returned.set(None);
            }
        });
    }

    // ---- Find and tags (6.1, 6.3) -------------------------------------------

    /// 🔍: Find slides down under the title, with the keyboard up.
    pub fn open_find(mut self) {
        self.archived_view.set(false);
        self.find_open.set(true);
        focus_soon(FIND, Caret::End);
    }

    /// Find's cross: Find goes back up, and the list is whole again.
    pub fn close_find(mut self) {
        dom::blur();
        self.exit.set(ListExit::None);
        self.limit.set(LISTED_NOTES);
        self.find_open.set(false);
        self.tags_open.set(false);
        self.set_find("");
        self.tags.set(Vec::new());
        self.rule.set(TagRule::All);
    }

    /// Back while Find is down: the offers in the strip go first, then
    /// Find.
    pub fn find_back(mut self) {
        if *self.tags_open.peek() {
            self.tags_open.set(false);
        } else {
            self.close_find();
        }
    }

    /// Find's text changed.
    pub fn find_typed(mut self, text: String) {
        if find::tag_word(&text).is_some() {
            self.tags_open.set(true);
        }
        self.exit.set(ListExit::None);
        self.limit.set(LISTED_NOTES);
        self.find_text.set(text);
    }

    /// Sets Find's text from here, not from the field.
    fn set_find(mut self, text: &str) {
        dom::set_value(FIND, text);
        self.find_text.set(text.to_owned());
    }

    /// Picks a tag, or takes it back if it is picked. What was typed after
    /// a `#` goes, since it was the tag's name.
    pub fn toggle_tag(mut self, tag: &str) {
        self.exit.set(ListExit::None);
        self.limit.set(LISTED_NOTES);
        let picked = self.tags.peek().iter().any(|chosen| chosen == tag);
        if picked {
            self.tags.write().retain(|chosen| chosen != tag);
            if self.tags.peek().len() < 2 {
                self.rule.set(TagRule::All);
            }
            return;
        }
        self.tags.write().push(tag.to_owned());
        let text = self.find_text.peek().clone();
        if find::tag_word(&text).is_some() {
            self.set_find(find::words(&text));
        }
    }

    /// A tag tapped on a card: the list narrows to it, and Find comes down
    /// to show it, as a pill that can be taken back.
    pub fn narrow_to(mut self, tag: &str) {
        self.archived_view.set(false);
        self.find_open.set(true);
        if !self.tags.peek().iter().any(|chosen| chosen == tag) {
            self.toggle_tag(tag);
        }
    }

    /// The first tag the strip offers, picked: Enter on a lone `#tag`.
    pub fn pick_first_tag(self) -> bool {
        let picked = self.tags.peek().clone();
        let first = self
            .tag_offers
            .peek()
            .iter()
            .find(|offer| !picked.contains(&offer.tag))
            .map(|offer| offer.tag.clone());
        first.is_some_and(|tag| {
            self.toggle_tag(&tag);
            true
        })
    }

    /// Takes back the last tag picked: Backspace in an empty Find.
    pub fn untag_last(mut self) {
        self.exit.set(ListExit::None);
        self.tags.write().pop();
    }

    /// Shows every archived note, or goes back to the list.
    pub fn show_archived(mut self, shown: bool) {
        self.exit.set(ListExit::None);
        self.limit.set(LISTED_NOTES);
        self.archived_view.set(shown);
    }

    // ---- Files ----------------------------------------------------------------

    /// 📎 on the page: Android's screen for choosing files, and what is
    /// chosen goes to the page's note. A page with no note yet becomes a
    /// note of the files, with the tags chosen in Find as its words (6.1: a
    /// file with no words under it is a note). The files go to the note
    /// they were chosen for, even if the page has been left by then.
    pub fn choose_files(mut self) {
        let view = self.page.peek().clone();
        if !view.open {
            return;
        }
        let draft = view.draft;
        let tags: Vec<String> = self
            .tags
            .peek()
            .iter()
            .map(|tag| format!("#{tag}"))
            .collect();
        spawn(async move {
            let Some(files) = pick_files().await else {
                return;
            };
            for _ in 0..300 {
                if !draft.borrow().creating {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            let held_note = draft.borrow().note;
            let id = if let Some(id) = held_note {
                id
            } else {
                let (store, typed) = {
                    let held = draft.borrow();
                    (held.store.clone(), held.text.clone())
                };
                let Some(store) = store else {
                    discard_all(&files);
                    return;
                };
                let text = if tags.is_empty() {
                    typed.clone()
                } else {
                    format!("{} {}", typed.trim_end(), tags.join(" "))
                        .trim_start()
                        .to_owned()
                };
                match store.core().create_note(&text).await {
                    Ok(note) => {
                        {
                            let mut held = draft.borrow_mut();
                            held.note = Some(note.id);
                            held.saved = typed;
                        }
                        let same = self.page.peek().serial == view.serial;
                        if same {
                            let open = self.page.peek().open;
                            let mut page = self.page.write();
                            page.note = Some(note.id);
                            if open {
                                page.from = GrowFrom::Card(card_element_id(&note.id));
                            }
                        }
                        note.id
                    }
                    Err(error) => {
                        discard_all(&files);
                        tracing::error!("Could not make a note for the files: {error}");
                        return;
                    }
                }
            };
            let failed = attach(&self.app, id, files).await;
            *self.files_changed.write() += 1;
            self.app.note_changed();
            if !failed.is_empty() {
                self.app.startup_issue.set(Some(failed.join(" ")));
            }
        });
    }

    // ---- Sync (6.6) -------------------------------------------------------------

    /// How sync stands now.
    pub fn sync_indicator(self) -> SyncIndicator {
        let now = self.clock.read().now_ms;
        // Read for their changes: the session's own answer comes from them.
        let _ = (self.app.sync_status.read(), self.app.session_state.read());
        self.app
            .session
            .read()
            .as_ref()
            .map_or(SyncIndicator::SignedOut, |session| {
                session.sync_indicator(now)
            })
    }

    /// What tapping the sync status does (6.6): syncs now, or opens
    /// Settings, where a problem is explained and where to sign in is.
    pub fn sync_now(self) {
        match self.sync_indicator() {
            SyncIndicator::Problem { .. } | SyncIndicator::SignedOut => self.open_settings(),
            _ => self.pulled(),
        }
    }

    /// The list pulled down past the mark: sync now.
    pub fn pulled(self) {
        if let Some(session) = self.app.session.peek().as_ref() {
            session.notify(Trigger::Manual);
        }
    }

    pub fn open_settings(self) {
        let mut view = self.app.view;
        view.set(View::Settings);
    }

    // ---- What comes from outside the app (8.1) --------------------------------

    /// Deals with what was asked of the app from outside it, if anything
    /// was. What was shared is saved as a note straight away: capture does
    /// not wait for the person, who finds the note at the top of the list.
    /// Shared pictures are attached to it, and a note made of pictures
    /// alone is called after them. The widget asks for a new page. Neither
    /// takes the person out of a page they are writing on.
    async fn receive_captures(mut self) {
        if *self.receiving.peek() {
            return;
        }
        self.receiving.set(true);
        // More than one may be waiting: two shares can arrive before either
        // is asked for.
        while let Some(capture) = take_capture() {
            match capture {
                Capture::Blank => {
                    let mut view = self.app.view;
                    view.set(View::Main);
                    self.new_page();
                }
                Capture::Shared { text, files } => self.keep_shared(&text, files).await,
            }
        }
        self.receiving.set(false);
    }

    async fn keep_shared(mut self, text: &str, files: Vec<crate::incoming::Incoming>) {
        let mut issue = self.app.startup_issue;
        if !worth_a_note(text, &files) {
            // Nothing came in to make a note of. Say what was lost.
            issue.set(Some(not_copied_in(&files).join(" ")));
            return;
        }
        let Some(store) = self.store() else {
            discard_all(&files);
            issue.set(Some(
                "What was shared could not be saved: the notes are not open.".to_owned(),
            ));
            return;
        };
        match store.create_note(&note_text(text, &files)).await {
            Ok(note) => {
                // As a task of its own: it finishes whatever the screen
                // does meanwhile.
                let core = store.core().clone();
                let failed = tokio::spawn(attach_incoming(core, note.id, files))
                    .await
                    .unwrap_or_default();
                if !failed.is_empty() {
                    issue.set(Some(failed.join(" ")));
                }
                self.exit.set(ListExit::None);
                self.app.note_changed();
            }
            Err(error) => {
                discard_all(&files);
                tracing::error!("What was shared could not be saved: {error}");
                issue.set(Some(format!("What was shared could not be saved: {error}")));
            }
        }
    }
}

/// Gives the keyboard's focus to a field once the page has drawn it.
fn focus_soon(selector: &'static str, caret: Caret) {
    spawn(async move {
        tokio::time::sleep(Duration::from_millis(30)).await;
        dom::focus(&[selector], caret);
    });
}

/// Keeps the clock: every half minute, for the times on cards, and soon
/// after sync changes, for "Just synced" giving way.
fn use_clock(app: &AppState) -> Signal<Clock> {
    let app = *app;
    let mut clock = use_signal(Clock::now);
    use_future(move || async move {
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;
            clock.set(Clock::now());
        }
    });
    use_effect(move || {
        let _ = app.sync_status.read();
        clock.set(Clock::now());
        spawn(async move {
            tokio::time::sleep(Duration::from_millis(1500)).await;
            clock.set(Clock::now());
        });
    });
    clock
}

/// The thumbnail of a picture on `note`, if the device has one: from what
/// was read before, or from the store.
async fn thumbnail_of(
    store: &MobileNoteStore,
    note: &NoteId,
    mut thumbs: Signal<HashMap<Uuid, String>>,
) -> Option<String> {
    let files = store.core().attached_files(note).await.ok()?;
    let picture = files
        .iter()
        .find(|file| file.attachment.thumbnail.is_some() && file.thumbnail_on_device)?;
    let thumbnail = picture.attachment.thumbnail.as_ref()?.file_id;
    if let Some(known) = thumbs.peek().get(&thumbnail) {
        return Some(known.clone());
    }
    let bytes = store
        .core()
        .small_file(thumbnail, MOST_THUMBNAIL_BYTES)
        .await
        .ok()??;
    let shown = as_data_uri(&bytes);
    thumbs.write().insert(thumbnail, shown.clone());
    Some(shown)
}

/// Reads what the list shows from the store, again whenever what it
/// depends on changes, and marks the notes a sync has just brought.
fn use_listing(screen: &Screen, app: &AppState) {
    let (screen, app) = (*screen, *app);
    let mut seen = use_signal(HashMap::<NoteId, i64>::new);
    let mut pulls_seen = use_signal(|| 0_u64);
    let _reader = use_resource(move || {
        let store = app.store.read().clone();
        let _ = app.notes_read.read();
        let pulled = *app.pulled.read();
        let signed_in = app.session_state.read().is_ready();
        let text = screen.find_text.read().clone();
        let tags = screen.tags.read().clone();
        let rule = *screen.rule.read();
        let limit = *screen.limit.read();
        let archived_view = *screen.archived_view.read();
        let mut listing_signal = screen.listing;
        let mut flashing = screen.flashing;
        async move {
            let store = store?;
            let core = store.core();
            let words = find::words(&text).trim().to_owned();
            let finding = !archived_view && (!words.is_empty() || !tags.is_empty());
            let notes = if archived_view {
                core.list_archived_notes(limit, 0).await
            } else if finding {
                core.find_notes_by_tags(&words, &tags, rule, limit).await
            } else {
                core.list_notes(limit, 0).await
            };
            let notes = match notes {
                Ok(notes) => notes,
                Err(error) => {
                    tracing::error!("Could not read the list: {error}");
                    return None;
                }
            };
            let waiting = if signed_in {
                let shown = notes.iter().map(|note| note.id).collect();
                core.unsynced_note_ids(shown).await.unwrap_or_default()
            } else {
                HashSet::new()
            };
            let with_files = core.notes_with_files().await.unwrap_or_default();
            let mut first_file = HashMap::new();
            let mut thumbnails = HashMap::new();
            for note in notes.iter().filter(|note| with_files.contains(&note.id)) {
                if note.outline().title.is_empty() {
                    if let Some(file) = core
                        .note_attachments(&note.id)
                        .await
                        .ok()
                        .and_then(|files| files.into_iter().next())
                    {
                        first_file.insert(note.id, file.name);
                    }
                }
                if let Some(picture) = thumbnail_of(&store, &note.id, screen.thumbs).await {
                    thumbnails.insert(note.id, picture);
                }
            }
            let counts = core.note_counts().await.unwrap_or_default();
            let more = notes.len() >= limit;

            // What a sync has just brought: new to the list, or edited
            // since it was last read, and not the note being written.
            if pulled != *pulls_seen.peek() && !finding && !archived_view {
                let open = *app.current_note_id.peek();
                let before = seen.peek();
                let arrived: HashSet<NoteId> = notes
                    .iter()
                    .filter(|note| Some(note.id) != open)
                    .filter(|note| before.get(&note.id).is_none_or(|&at| note.updated_at > at))
                    .map(|note| note.id)
                    .collect();
                drop(before);
                if !arrived.is_empty() && !seen.peek().is_empty() {
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

            listing_signal.set(Some(Listing {
                notes,
                waiting,
                with_files,
                first_file,
                thumbnails,
                counts,
                finding,
                archived_view,
                more,
                words: dirt_ui::text::find_words(&words),
            }));
            Some(())
        }
    });
}

/// Reads what the strip of tags offers, while the `#` is pressed.
fn use_tag_offers(screen: &Screen, app: &AppState) {
    let (screen, app) = (*screen, *app);
    let _reader = use_resource(move || {
        let store = app.store.read().clone();
        let _ = app.notes_read.read();
        let open = *screen.tags_open.read();
        let text = screen.find_text.read().clone();
        let tags = screen.tags.read().clone();
        let rule = *screen.rule.read();
        let mut offers = screen.tag_offers;
        async move {
            if !open {
                return;
            }
            let Some(store) = store else { return };
            let words = find::words(&text).trim().to_owned();
            let starting = find::tag_word(&text);
            match store
                .core()
                .tags_to_narrow(&words, &tags, rule, starting, TAGS_OFFERED)
                .await
            {
                Ok(found) => offers.set(found),
                Err(error) => tracing::error!("Could not read the tags: {error}"),
            }
        }
    });
}

/// The main screen.
#[component]
pub fn MainScreen() -> Element {
    let app = use_context::<AppState>();
    let toasts = use_toasts();
    let clock = use_clock(&app);
    let screen = use_context_provider(|| Screen {
        app,
        toasts,
        find_open: Signal::new(false),
        find_text: Signal::new(String::new()),
        tags: Signal::new(Vec::new()),
        rule: Signal::new(TagRule::All),
        tags_open: Signal::new(false),
        tag_offers: Signal::new(Vec::new()),
        archived_view: Signal::new(false),
        limit: Signal::new(LISTED_NOTES),
        exit: Signal::new(ListExit::None),
        returned: Signal::new(None),
        flashing: Signal::new(HashSet::new()),
        listing: Signal::new(None),
        page: Signal::new(PageView::default()),
        leaving: Signal::new(Vec::new()),
        typing: Signal::new(false),
        files_changed: Signal::new(0),
        last_undo: Signal::new(None),
        serial: Signal::new(0),
        thumbs: Signal::new(HashMap::new()),
        receiving: Signal::new(false),
        clock,
    });
    use_listing(&screen, &app);
    use_tag_offers(&screen, &app);

    // Another store taken into use (signing in or out) closes the page; it
    // is saved to the store it was opened on.
    let mut store_seen = use_signal(|| None::<std::path::PathBuf>);
    use_effect(move || {
        let now = app
            .store
            .read()
            .as_ref()
            .map(|store| store.path().to_path_buf());
        let before = store_seen.peek().clone();
        if before.is_some() && before != now {
            screen.done();
        }
        store_seen.set(now);
    });

    // A note asked for from elsewhere: Settings → Sync.
    use_effect(move || {
        let mut request = app.open_request;
        let asked = *request.read();
        if let Some(id) = asked {
            request.set(None);
            screen.done();
            screen.open(id);
        }
    });

    // The keyboard going away brings the page's bar back, and the dock,
    // which Find hides while it has the keyboard: the window grows again
    // when it does.
    use_future(move || async move {
        let mut grew = document::eval(concat!(
            "let tall = innerHeight;",
            "addEventListener('resize', () => {",
            "  if (innerHeight > tall + 120) {",
            "    const at = document.activeElement;",
            "    if (at && at.id === 'dui-find') at.blur();",
            "    dioxus.send(true);",
            "  }",
            "  tall = innerHeight;",
            "});",
            "await new Promise(() => {});",
        ));
        let mut typing = screen.typing;
        while grew.recv::<bool>().await.is_ok() {
            if *typing.peek() {
                typing.set(false);
            }
        }
    });

    // Something may have come from outside the app.
    use_effect(move || {
        let _ = app.capture_ready.read();
        spawn(async move { screen.receive_captures().await });
    });

    rsx! {
        style { {include_str!("screen.css")} }
        Main {}
        Page {}
    }
}
