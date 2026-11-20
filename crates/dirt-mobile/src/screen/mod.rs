//! Android's main screen (interface design, sections 3, 6.1 to 6.7, 6.14,
//! 6.19 and 8): the title with 🔍, the sync status and Settings; Find,
//! which slides down under it; the list of cards; and the dock, the bar a
//! new page grows out of. A note opens as a page over all of it, grown out
//! of its card. The dock's mic listens in a sheet over the lower half of
//! the screen (6.18, 8.9); the page's mic, under the page's bar, and what
//! is said there is added to the page's note.
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
use dirt_core::session::SessionState;
use dirt_core::store::{LetGo, NoteCounts, TagCount, TagRule};
use dirt_core::sync::overview::SyncIndicator;
use dirt_core::sync::worker::Trigger;
use dirt_ui::components::{card_element_id, use_toasts, GrowFrom, ListExit, Talk, Toast, Toasts};
use dirt_ui::dom::{self, Caret};
use dirt_ui::files::{use_note_files, FilePreview, FileSource, NoteFiles};
use dirt_ui::find;
use dirt_ui::pile::PileView;
use dirt_ui::settings::Section;
use dirt_ui::time::Clock;
use dirt_ui::Icon;

use crate::card_files::{CardFiles, CardFilesCache};
use crate::data::MobileNoteStore;
use crate::draft::{Draft, Typed};
use crate::files::{attach, choose_files as pick_files};
use crate::incoming::{attach_incoming, discard_all, not_copied_in, note_text, worth_a_note};
use crate::services::activity::{copy_text, take_capture};
use crate::state::AppState;
use crate::voice::{Said, Talking, Unheard};
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
    /// Why the words of each listed note said here could not be made,
    /// once the phone has stopped trying (6.18).
    pub unread: HashMap<NoteId, String>,
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
    /// The pile under the last card (6.17): read while the list is not
    /// finding, and kept as it was while it is.
    pub pile: PileView,
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
    /// A new thought just let go: its grain settles onto its card (5.5).
    pub settling: Signal<Option<NoteId>>,
    pub listing: Signal<Option<Listing>>,
    pub page: Signal<PageView>,
    /// The drafts of pages being left, until the store has what they held.
    leaving: Signal<Vec<DraftCell>>,
    /// Typing on the page: its top bar fades until the page is touched.
    pub typing: Signal<bool>,
    /// Raised when the open note's files change here, to read them again.
    /// The files of the note on the page, and the preview of them.
    pub files: NoteFiles,
    last_undo: Signal<Option<Undo>>,
    serial: Signal<u64>,
    /// The newest text of each page (by its number) whose write failed,
    /// while the error says so: what **Copy text** gives.
    unsaved: Signal<Vec<(u64, String)>>,
    /// What the cards show of their notes' files, read once for each change
    /// of a note.
    card_files: Signal<CardFilesCache>,
    /// The activity is being asked what came from outside the app.
    receiving: Signal<bool>,
    /// What time it is, for the times on cards and the sync status.
    pub clock: Signal<Clock>,
    /// The listening of the dock, or of the page's mic (6.18).
    pub talking: Talking,
    /// The page's mic is listening, not the dock's: the page shows it.
    pub talking_on_page: Signal<bool>,
    /// Notes that what was said into a page was added to in this run,
    /// whose words a screen reader is told of when they come (section 12).
    said_into: Signal<HashSet<NoteId>>,
    /// What a screen reader was last told of words that arrived, and how
    /// many times: told again when it reads the same.
    pub words_heard: Signal<(u64, String)>,
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

    /// A write of a note's text failed: the page numbered `page`, or a
    /// share. It is not tried again: the store waits out a busy file inside
    /// every write, so a write that still fails is the disk's. The person
    /// is told plainly and given their words, as the desktop does (design
    /// 6.7): an error that stays until it is dismissed, the store's reason
    /// in its tooltip, and **Copy text**, which gives the newest text of
    /// each page that could not be saved. Ordinary toasts neither replace
    /// nor clear it.
    fn not_saved(mut self, page: u64, error: &dirt_core::Error, text: &str) {
        tracing::error!("Could not save a note: {error}");
        {
            let mut unsaved = self.unsaved.write();
            // An error dismissed since: its notes were seen to.
            if !self.toasts.error_showing() {
                unsaved.clear();
            }
            match unsaved.iter_mut().find(|(held, _)| *held == page) {
                Some(held) => text.clone_into(&mut held.1),
                None => unsaved.push((page, text.to_owned())),
            }
        }
        let count = self.unsaved.peek().len();
        let words = if count == 1 {
            "A note could not be saved".to_owned()
        } else {
            format!("{count} notes could not be saved")
        };
        let copy = Callback::new(move |()| {
            let texts: Vec<String> = self
                .unsaved
                .peek()
                .iter()
                .map(|(_, text)| text.clone())
                .collect();
            if !copy_text(&texts.join("\n\n")) {
                tracing::error!("Could not copy the text");
            }
        });
        self.toasts.show_error(
            Toast::new(words)
                .icon(Icon::Alert)
                .detail(dirt_core::util::as_sentence(error))
                .action("Copy text", Icon::Copy, copy),
        );
    }

    /// Something other than a note's text could not be written: an
    /// archive, a delete, or bringing a note back. Said as the error is,
    /// with nothing to copy, since no words were lost.
    fn not_done(self, what: &str, error: &dirt_core::Error) {
        tracing::error!("{what}: {error}");
        self.toasts.show_error(
            Toast::new(what)
                .icon(Icon::Alert)
                .detail(dirt_core::util::as_sentence(error)),
        );
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
        self.stop_listening();
        let serial = self.next_serial();
        let draft = Rc::new(RefCell::new(Draft::blank(self.store())));
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
        self.stop_listening();
        let being_left = self
            .leaving
            .peek()
            .iter()
            .find(|draft| draft.borrow().note() == Some(id))
            .map(|draft| draft.borrow().text().to_owned());
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
        let draft = Rc::new(RefCell::new(Draft::of_note(self.store(), id, &text)));
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
            let Some(store) = draft.borrow().store() else {
                return;
            };
            let found = store.core().get_note(&id).await;
            if self.page.peek().serial != serial {
                return;
            }
            match found {
                Ok(Some(note)) => {
                    if draft.borrow_mut().read_back(&note.content) {
                        dom::set_value(PAGE, &note.content);
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
    pub fn typed(mut self, text: &str) {
        if !*self.typing.peek() {
            self.typing.set(true);
        }
        let view = self.page.peek().clone();
        if !view.open {
            return;
        }
        let draft = view.draft;
        let typed = draft.borrow_mut().typed(text);
        // A page that could not be saved: Copy text gives its newest words.
        if let Some(held) = self
            .unsaved
            .write()
            .iter_mut()
            .find(|(page, _)| *page == view.serial)
        {
            text.clone_into(&mut held.1);
        }
        let version = match typed {
            Typed::Ignored => return,
            Typed::Save(version) => Some(version),
            Typed::Create(_) => None,
        };
        if let Typed::Create(text) = typed {
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
            spawn(async move {
                self.create(draft, view.serial, text).await;
            });
            return;
        }
        spawn(async move {
            tokio::time::sleep(SAVE_AFTER).await;
            let current = Some(draft.borrow().version()) == version;
            if current {
                self.save(&draft, view.serial).await;
            }
        });
    }

    /// Makes the page's note of `text`, which the draft has said is this
    /// caller's to make (its first character, or files chosen on it).
    /// Returns the note.
    ///
    /// If it cannot be made, the person is told, with the text to copy, and
    /// the page is as it was before its first character, so that the next
    /// key makes the note with all of it.
    async fn create(mut self, draft: DraftCell, serial: u64, text: String) -> Option<NoteId> {
        let Some(store) = draft.borrow().store() else {
            draft.borrow_mut().create_failed();
            return None;
        };
        match store.core().create_note(&text).await {
            Ok(note) => {
                draft.borrow_mut().created(note.id, &text);
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
                self.save(&draft, serial).await;
                Some(note.id)
            }
            Err(error) => {
                draft.borrow_mut().create_failed();
                self.not_saved(serial, &error, &text);
                None
            }
        }
    }

    /// Saves what was typed, if the store does not have it yet. One save is
    /// written at a time; one asked for meanwhile is written after it, with
    /// what was typed by then.
    async fn save(self, draft: &DraftCell, page: u64) {
        if !draft.borrow_mut().begin_save() {
            return;
        }
        let store = draft.borrow().store();
        let mut changed = false;
        let mut came = None;
        while let Some(store) = store.as_ref() {
            let next = draft.borrow().to_save();
            let Some((id, base, text)) = next else { break };
            match store.core().update_note_from(&id, &base, &text).await {
                Ok(note) => {
                    draft.borrow_mut().saved(&text);
                    changed = true;
                    // The store kept what came meanwhile: the page takes it.
                    came = (note.content != text).then_some(note);
                }
                Err(error) => {
                    // Not tried again; the next change, or leaving the
                    // page, writes what the page holds then.
                    self.not_saved(page, &error, &text);
                    break;
                }
            }
        }
        draft.borrow_mut().end_save();
        if changed {
            self.app.note_changed();
        }
        if let Some(note) = came {
            self.bring_in(draft, page, &note);
        }
    }

    /// What reached the page's note that the page did not write (the words
    /// of what was said) is brought into the page, keeping the caret and
    /// what is typed meanwhile, and saved as merged. Tried again if a key
    /// was typed where the change goes, in that moment.
    fn bring_in(self, draft: &DraftCell, page: u64, note: &Note) {
        let showing = {
            let view = self.page.peek();
            view.open && view.serial == page
        };
        if !showing {
            return;
        }
        let before = draft.borrow().clone();
        let expected = before.text().to_owned();
        let Some(merged) = draft.borrow_mut().took(note.id, &note.content) else {
            return;
        };
        let draft = Rc::clone(draft);
        let id = note.id;
        spawn(async move {
            let held = dom::merge_into(PAGE, &expected, &merged).await;
            match held {
                Some((now, true)) => draft.borrow_mut().holds(now),
                Some((now, false)) => {
                    // Back to what the page had, with what it holds now:
                    // the next save keeps what came, and it is tried again.
                    let mut held = draft.borrow_mut();
                    *held = before;
                    held.holds(now);
                    drop(held);
                    self.read_again(&draft, page, id);
                }
                None => {
                    *draft.borrow_mut() = before;
                    return;
                }
            }
            self.save(&draft, page).await;
        });
    }

    /// Reads the page's note again, once its saves are written, and brings
    /// in what came.
    fn read_again(self, draft: &DraftCell, page: u64, id: NoteId) {
        let draft = Rc::clone(draft);
        spawn(async move {
            wait_while(|| draft.borrow().is_saving()).await;
            let Some(store) = draft.borrow().store() else {
                return;
            };
            if let Ok(Some(note)) = store.core().get_note(&id).await {
                self.bring_in(&draft, page, &note);
            }
        });
    }

    /// Saves a page that has been let go, from what it held, and lets its
    /// note go if it was emptied again (6.1). Returns the note, unless
    /// there was none or it was let go.
    async fn leave(mut self, draft: DraftCell, serial: u64) -> Option<NoteId> {
        draft.borrow_mut().leave();
        self.leaving.write().push(Rc::clone(&draft));
        let kept = self.settle(&draft, serial).await;
        self.leaving
            .write()
            .retain(|other| !Rc::ptr_eq(other, &draft));
        kept
    }

    async fn settle(self, draft: &DraftCell, serial: u64) -> Option<NoteId> {
        // What was being said into the page is kept first, in its note, so
        // that a note with no words and a recording coming is not let go.
        if draft.borrow().has_listened() {
            let _ = dirt_ui::voice::keeping_settled(KEEPING_AT_MOST).await;
            wait_long(|| draft.borrow().is_hearing()).await;
        }
        // A note still being made is waited for, so that it is not lost.
        wait_while(|| draft.borrow().is_creating()).await;
        // A page whose note could not be made is not made now: a note is
        // never made later from a failure (design 6.7), and the error's
        // Copy text holds what the page held.
        self.save(draft, serial).await;
        wait_while(|| draft.borrow().is_saving()).await;
        let (id, empty, store) = {
            let held = draft.borrow();
            (held.note()?, held.is_empty(), held.store())
        };
        if empty {
            if let Some(store) = store {
                match store.core().let_go_if_empty(&id).await {
                    Ok(LetGo::Kept) => {}
                    Ok(LetGo::Dropped | LetGo::Deleted | LetGo::Gone) => {
                        self.app.note_changed();
                        return None;
                    }
                    // An empty note kept: nothing typed is lost.
                    Err(error) => tracing::error!("Could not let the empty note go: {error}"),
                }
            }
        }
        Some(id)
    }

    /// Closes the page into `into`, and puts the keyboard away.
    fn close_page(mut self, into: GrowFrom) {
        if let GrowFrom::Card(card) | GrowFrom::Grain(card) = &into {
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
        // Listening on the page ends, keeping what was said: it goes to
        // the note all the same (6.18).
        self.stop_listening();
        let (note, empty) = {
            let held = view.draft.borrow();
            (held.note(), held.is_empty())
        };
        let into = match note {
            _ if view.new_page && empty => GrowFrom::Bar(BAR_ID.to_owned()),
            // A new thought closes to its grain, which settles onto its
            // card (5.5).
            Some(id) if view.new_page => GrowFrom::Grain(card_element_id(&id)),
            Some(id) => GrowFrom::Card(card_element_id(&id)),
            None => GrowFrom::Nowhere,
        };
        let settles = matches!(into, GrowFrom::Grain(_)).then_some(note).flatten();
        self.close_page(into);
        if let Some(id) = settles {
            self.settle_grain(id);
        }
        spawn(async move {
            self.leave(view.draft, view.serial).await;
        });
    }

    /// A new thought settles onto the pile: its card is marked for a
    /// moment, and the script tosses its grain onto it (5.5).
    fn settle_grain(mut self, note: NoteId) {
        self.settling.set(Some(note));
        spawn(async move {
            tokio::time::sleep(Duration::from_millis(2500)).await;
            if *self.settling.peek() == Some(note) {
                self.settling.set(None);
            }
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
        self.stop_listening();
        let note = view.draft.borrow().note();
        self.close_page(note.map_or(GrowFrom::Nowhere, |id| GrowFrom::Card(card_element_id(&id))));
        spawn(async move {
            let began = tokio::time::Instant::now();
            let Some(id) = self.leave(view.draft, view.serial).await else {
                return;
            };
            tokio::time::sleep_until(began + CLOSE).await;
            let exit = if delete {
                ListExit::Tear
            } else {
                ListExit::Sink
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
                self.not_done("The note could not be brought back", &error);
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
                self.not_done(
                    if delete {
                        "The note could not be deleted"
                    } else {
                        "The note could not be archived"
                    },
                    &error,
                );
                // A card swiped away comes back.
                self.exit.set(ListExit::None);
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
                self.not_done("The note could not be brought back", &error);
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
    pub fn choose_files(self) {
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
            // One note for the page: a note being made from its first
            // character is waited for, and one made here is claimed first,
            // so that a key typed meanwhile does not make another.
            wait_while(|| draft.borrow().is_creating()).await;
            let held_note = draft.borrow().note();
            let id = if let Some(id) = held_note {
                id
            } else if draft.borrow_mut().claim_create() {
                let typed = draft.borrow().text().to_owned();
                let text = if tags.is_empty() {
                    typed
                } else {
                    format!("{} {}", typed.trim_end(), tags.join(" "))
                        .trim_start()
                        .to_owned()
                };
                let Some(id) = self.create(Rc::clone(&draft), view.serial, text).await else {
                    discard_all(&files);
                    return;
                };
                id
            } else {
                // Made meanwhile by the page's own typing.
                wait_while(|| draft.borrow().is_creating()).await;
                let Some(id) = draft.borrow().note() else {
                    discard_all(&files);
                    return;
                };
                id
            };
            let failed = attach(&self.app, id, files).await;
            self.files.reread();
            self.app.note_changed();
            if !failed.is_empty() {
                let words = if failed.len() == 1 {
                    "A file could not be attached".to_owned()
                } else {
                    format!("{} files could not be attached", failed.len())
                };
                self.toasts
                    .show_error(Toast::new(words).icon(Icon::Alert).detail(failed.join(" ")));
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
    /// Settings where a problem is explained, where to sign in is, or, on
    /// a device waiting to be let in, where it waits.
    pub fn sync_now(self) {
        match self.sync_indicator() {
            SyncIndicator::Problem { .. } => self.app.nav.open_at(Section::Sync),
            SyncIndicator::SignedOut => self.app.nav.open_at(
                if matches!(
                    *self.app.session_state.peek(),
                    SessionState::WaitingForApproval { .. }
                ) {
                    Section::Devices
                } else {
                    Section::Account
                },
            ),
            _ => self.pulled(),
        }
    }

    /// The list pulled down past the mark: sync now.
    pub fn pulled(self) {
        if let Some(session) = self.app.session.peek().as_ref() {
            session.notify(Trigger::Manual);
        }
    }

    /// The gear: Settings, where it was.
    pub fn open_settings(self) {
        self.stop_listening();
        self.app.nav.open();
    }

    // ---- Voice (6.18, 8.9) ------------------------------------------------------

    /// What the dock's mic asked for.
    pub fn talk(self, talk: Talk) {
        match talk {
            Talk::Listen => self.listen(false),
            // A tap from a screen reader is the first the dock hears of it.
            Talk::Lock if !self.talking.is_shown() => self.listen(true),
            Talk::Lock => self.talking.lock(),
            Talk::Keep => self.talking.end(true),
            Talk::Throw => self.talking.end(false),
        }
    }

    /// The dock listens, in a sheet over the lower half of the screen. What
    /// is said goes to the store in use as it starts, whatever is in use
    /// when it ends: signing in or out meanwhile does not send it to the
    /// other account.
    fn listen(self, hands_free: bool) {
        if self.page.peek().open || !crate::voice::offered() {
            return;
        }
        // The press that asks for the microphone, in Android's own dialog,
        // records nothing: no sheet rises behind the dialog.
        if !self.may_listen() {
            return;
        }
        let Some(store) = self.store() else { return };
        // A toast would cover the listening sheet's ✕ and ✓.
        self.toasts.clear();
        let mut on_page = self.talking_on_page;
        on_page.set(false);
        let keep = Callback::new(move |said: Said| self.keep_said(&store, said));
        self.talking.listen(hands_free, keep);
    }

    /// Whether the app may use the microphone now. If not, Android is asked
    /// in its own dialog, and the press that asks records nothing.
    fn may_listen(self) -> bool {
        if crate::services::activity::microphone_allowed() {
            return true;
        }
        let mut unheard = self.talking.unheard;
        spawn(async move {
            let answer = tokio::task::spawn_blocking(crate::voice::ask)
                .await
                .unwrap_or_else(|error| Unheard::Failed(error.to_string()));
            unheard.set((answer != Unheard::Asked).then_some(answer));
        });
        false
    }

    /// What the mic in the page's bar asked for (6.18, 8.9).
    pub fn page_talk(self, talk: Talk) {
        match talk {
            Talk::Listen => self.listen_on_page(false),
            // A tap from a screen reader is the first the page hears of it.
            Talk::Lock if !self.talking.is_shown() => self.listen_on_page(true),
            Talk::Lock => self.talking.lock(),
            Talk::Keep => self.talking.end(true),
            Talk::Throw => self.talking.end(false),
        }
    }

    /// The page listens, under its bar, with the keyboard down. What is
    /// said is added at the end of the page's note: the recording among its
    /// files at once, its words once they are made. It goes to the page's
    /// note in the store the page was opened on, even if the page is left,
    /// or another store taken into use, before it is kept.
    fn listen_on_page(mut self, hands_free: bool) {
        let view = self.page.peek().clone();
        if !view.open || !crate::voice::offered() || !self.may_listen() {
            return;
        }
        if view.draft.borrow().store().is_none() {
            return;
        }
        self.toasts.clear();
        dom::blur();
        self.typing.set(false);
        self.talking_on_page.set(true);
        let draft = view.draft;
        draft.borrow_mut().listens();
        let keep = Callback::new(move |said: Said| {
            // Counted at once: leaving the page waits for it.
            draft.borrow_mut().hearing();
            self.add_said(Rc::clone(&draft), view.serial, said);
        });
        self.talking.listen(hands_free, keep);
    }

    /// What was said into a page goes to its note: the note it has, or, on
    /// a page with no note yet, a note of what was said, as the dock makes
    /// (one note for a page). A note that cannot take it (deleted
    /// meanwhile, or with no room for another file) does not lose it: it
    /// becomes a note of its own, and the person is told.
    fn add_said(self, draft: DraftCell, serial: u64, said: Said) {
        spawn(async move {
            wait_while(|| draft.borrow().is_creating()).await;
            let store = draft.borrow().store();
            let Some(store) = store else {
                draft.borrow_mut().heard();
                return;
            };
            let held = draft.borrow().note();
            let claimed = held.is_none() && draft.borrow_mut().claim_create();
            let added = if claimed {
                let note = NoteId::new();
                crate::voice::said_here(note);
                let kept = store
                    .core()
                    .keep_voice_note(note, said.file.clone(), said.duration_ms)
                    .await;
                if kept.is_ok() {
                    draft.borrow_mut().created(note, "");
                    self.became_the_pages(serial, note);
                    self.save(&draft, serial).await;
                } else {
                    draft.borrow_mut().create_failed();
                }
                kept.map(|_| ())
            } else {
                // Made meanwhile by the page's typing, if not before: it
                // goes there.
                wait_while(|| draft.borrow().is_creating()).await;
                let made = draft.borrow().note();
                match made {
                    Some(note) => {
                        let added = store
                            .core()
                            .add_voice_to_note(&note, said.file.clone())
                            .await;
                        if added.is_ok() {
                            self.said_into.write_unchecked().insert(note);
                        }
                        added.map(|_| ())
                    }
                    None => Err(dirt_core::Error::NotFound("the page's note".to_owned())),
                }
            };
            match added {
                Ok(()) => {
                    self.files.reread();
                    self.app.note_changed();
                    crate::voice::make_words_soon();
                }
                Err(error) => self.keep_apart(&store, &error, said).await,
            }
            draft.borrow_mut().heard();
        });
    }

    /// A note made for the page by what was said into it is the page's,
    /// if the page is still up.
    fn became_the_pages(mut self, serial: u64, note: NoteId) {
        let (same, open) = {
            let page = self.page.peek();
            (page.serial == serial, page.open)
        };
        if !same {
            return;
        }
        {
            let mut page = self.page.write();
            page.note = Some(note);
            if open {
                page.from = GrowFrom::Card(card_element_id(&note));
            }
        }
        if open {
            self.app.current_note_id.set(Some(note));
        }
    }

    /// What was said into a page whose note could not take it is kept as a
    /// note of its own, so that nothing said is lost, and the person is
    /// told why.
    async fn keep_apart(self, store: &Arc<MobileNoteStore>, why: &dirt_core::Error, said: Said) {
        tracing::warn!("What was said could not go to its page's note: {why}");
        let note = NoteId::new();
        crate::voice::said_here(note);
        match store
            .core()
            .keep_voice_note(note, said.file, said.duration_ms)
            .await
        {
            Ok(_) => {
                self.app.note_changed();
                crate::voice::make_words_soon();
                self.toasts.show(
                    Toast::new("What was said is a note of its own")
                        .icon(Icon::Mic)
                        .detail(dirt_core::util::as_sentence(why)),
                );
            }
            Err(error) => self.not_done("What was said could not be kept", &error),
        }
    }

    /// Listening ends, keeping what was said: the person leaves the app,
    /// opens a note, a new page or Settings (as on the desktop, 7.11).
    pub fn stop_listening(self) {
        if self.talking.is_listening_still() {
            self.talking.end(true);
        }
    }

    /// What was said is kept as a note at once, and settles onto the pile
    /// as a typed thought does. Its words follow.
    fn keep_said(mut self, store: &Arc<MobileNoteStore>, said: Said) {
        let note = NoteId::new();
        crate::voice::said_here(note);
        self.settle_grain(note);
        let store = Arc::clone(store);
        spawn(async move {
            match store
                .core()
                .keep_voice_note(note, said.file, said.duration_ms)
                .await
            {
                Ok(_) => {
                    self.exit.set(ListExit::None);
                    self.app.note_changed();
                    crate::voice::make_words_soon();
                }
                Err(error) => self.not_done("What was said could not be kept", &error),
            }
        });
    }

    /// The words of what was said were put in `note`. A screen reader is
    /// told, for a thought said here in this run; on its page, they are
    /// brought in.
    pub fn words_came(mut self, note: &Note) {
        let said_into = self.said_into.write().remove(&note.id);
        let first = if crate::voice::was_said_here(note.id) {
            Some(note.outline().title)
        } else if said_into {
            // At the end of the note: its last line is theirs.
            note.content
                .lines()
                .rev()
                .find(|line| !line.trim().is_empty())
                .map(|line| line.trim().to_owned())
        } else {
            None
        };
        if let Some(first) = first {
            let mut heard = self.words_heard.write();
            heard.0 += 1;
            let mark = if heard.0.is_multiple_of(2) {
                ""
            } else {
                "\u{200b}"
            };
            heard.1 = format!("Words arrived: {first}{mark}");
        }
        let view = self.page.peek().clone();
        if view.open && view.note == Some(note.id) {
            self.read_again(&view.draft, view.serial, note.id);
        }
    }

    /// The listening sheet has drawn how it ended.
    pub fn talked(self, serial: u64) {
        self.talking.gone(serial);
    }

    /// The microphone could not be used: the press says why, with the way
    /// to Android's page for the app once it was refused for good.
    fn unheard(self, why: &Unheard) {
        let toast = Toast::new(why.tip()).icon(Icon::MicOff);
        let toast = if *why == Unheard::RefusedForGood {
            toast.action(
                "Settings",
                Icon::Settings,
                Callback::new(|()| {
                    if !crate::services::activity::open_app_settings() {
                        tracing::warn!("Android's page for the app could not be opened");
                    }
                }),
            )
        } else {
            toast
        };
        self.toasts.show(toast);
    }

    // ---- What comes from outside the app (8.1) --------------------------------

    /// Deals with what another app shared, if anything waits. It is saved
    /// as a note straight away: capture does not wait for the person, who
    /// finds the note at the top of the list, and a toast with **Undo**
    /// says so (8.1). Shared pictures are attached to it, and a note made
    /// of pictures alone is called after them. It does not take the person
    /// out of a page they are writing on. (The widget, the shortcut and the
    /// tile open the capture sheet, which is not the app: `capture`.)
    async fn receive_captures(mut self) {
        if *self.receiving.peek() {
            return;
        }
        self.receiving.set(true);
        // More than one may be waiting: two shares can arrive before either
        // is asked for.
        while let Some(shared) = take_capture() {
            self.keep_shared(&shared.text, shared.files).await;
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
                // **Undo** takes the note away again, as if nothing had
                // been shared: from the store it was made in, whatever is
                // open by the time **Undo** is pressed.
                let made_in = Arc::clone(&store);
                let undo = Callback::new(move |()| self.unshare(&made_in, note.id));
                self.toasts
                    .show(Toast::new("Saved to Dirt").icon(Icon::Check).undo(undo));
            }
            Err(error) => {
                discard_all(&files);
                let share = self.next_serial();
                self.not_saved(share, &error, text);
            }
        }
    }
}

impl Screen {
    /// **Undo** on the toast after a share: the note it made is deleted,
    /// and no toast says so.
    fn unshare(self, store: &Arc<MobileNoteStore>, note: NoteId) {
        let store = Arc::clone(store);
        spawn(async move {
            match store.core().delete_note(&note).await {
                Ok(()) => self.app.note_changed(),
                Err(error) => self.not_done("The note could not be deleted", &error),
            }
        });
    }
}

/// Waits, for at most three seconds, while `busy` holds: a note being
/// made, a save being written.
/// How long leaving a page waits for what was being said into it to be
/// kept.
const KEEPING_AT_MOST: Duration = Duration::from_secs(30);

/// Waits while `busy`, for as long as a recording takes to be kept: at
/// most half a minute.
async fn wait_long(busy: impl Fn() -> bool) {
    for _ in 0..3000 {
        if !busy() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn wait_while(busy: impl Fn() -> bool) {
    for _ in 0..300 {
        if !busy() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
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

/// What the cards show of the files of the notes `shown`: which have
/// files, the first file's name, and a picture's thumbnail if the device
/// has it. A note's files are read only when it has changed since they
/// were last read; a thumbnail is read until the device has it, and then
/// kept while a card shows it.
async fn read_card_files(
    store: &MobileNoteStore,
    shown: Vec<NoteId>,
    mut cache: Signal<CardFilesCache>,
) -> (
    HashSet<NoteId>,
    HashMap<NoteId, String>,
    HashMap<NoteId, String>,
) {
    let core = store.core();
    cache.write().reading_from(store.path());
    let revisions = core.file_revisions(shown).await.unwrap_or_default();
    let wanted = cache.peek().to_read(&revisions);
    for note in wanted {
        let Ok(files) = core.note_attachments(&note).await else {
            continue;
        };
        let read = CardFiles {
            first_name: files.first().map(|file| file.name.clone()),
            thumbnail: files
                .iter()
                .find_map(|file| file.thumbnail.as_ref().map(|thumb| thumb.file_id)),
        };
        cache.write().read(note, revisions[&note], read);
    }
    cache.write().keep_only(&revisions);

    let with_files: HashSet<NoteId> = revisions.keys().copied().collect();
    let mut first_file = HashMap::new();
    let mut thumbnails = HashMap::new();
    for note in &with_files {
        let files = cache.peek().files(note).cloned().unwrap_or_default();
        if let Some(name) = files.first_name {
            first_file.insert(*note, name);
        }
        let Some(thumbnail) = files.thumbnail else {
            continue;
        };
        let known = cache.peek().thumb(&thumbnail).cloned();
        let shown = match known {
            Some(shown) => Some(shown),
            None => match core.small_file(thumbnail, MOST_THUMBNAIL_BYTES).await {
                Ok(Some(bytes)) => {
                    let shown = as_data_uri(&bytes);
                    cache.write().keep_thumb(thumbnail, shown.clone());
                    Some(shown)
                }
                // Not on the device yet: opening the note fetches it.
                _ => None,
            },
        };
        if let Some(shown) = shown {
            thumbnails.insert(*note, shown);
        }
    }
    (with_files, first_file, thumbnails)
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
            let shown = notes.iter().map(|note| note.id).collect();
            let (with_files, mut first_file, thumbnails) =
                read_card_files(&store, shown, screen.card_files).await;
            let waiting_words: Vec<NoteId> = notes
                .iter()
                .filter(|note| {
                    note.voice
                        .is_some_and(|voice| voice.words == dirt_core::models::Words::Waiting)
                })
                .map(|note| note.id)
                .collect();
            let unread = if waiting_words.is_empty() {
                HashMap::new()
            } else {
                core.voice_problems(waiting_words).await.unwrap_or_default()
            };
            // Only a note with no words goes by its first file's name.
            first_file.retain(|id, _| {
                notes
                    .iter()
                    .any(|note| note.id == *id && note.outline().title.is_empty())
            });
            let counts = core.note_counts().await.unwrap_or_default();
            let more = notes.len() >= limit;
            // The pile's notes are read only when the counts say they may
            // have changed: not at every pause in typing (6.17).
            let before = listing_signal
                .peek()
                .as_ref()
                .map(|listing| listing.pile.clone());
            let offset = Clock::now().utc_offset_secs;
            let pile = PileView::read(before, counts, offset, || core.pile()).await;
            // The pile widget on the home screen is drawn from the same
            // heap, when it is a new one (8.5).
            crate::pile_widget::show(&crate::config::default_mobile_data_directory(), &pile);

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
                unread,
                thumbnails,
                counts,
                finding,
                archived_view,
                more,
                words: dirt_ui::text::find_words(&words),
                pile,
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
    let note_files = use_note_files(
        app.current_note_id.into(),
        app.sync_status.into(),
        move || FileSource {
            store: app.store.peek().as_ref().map(|store| store.core().clone()),
            session: app.session.peek().clone(),
        },
        move || app.note_changed(),
    );
    crate::files::use_served_files();
    let talking = Talking::use_new(crate::voice::open_ear);
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
        settling: Signal::new(None),
        listing: Signal::new(None),
        page: Signal::new(PageView::default()),
        leaving: Signal::new(Vec::new()),
        typing: Signal::new(false),
        files: note_files,
        last_undo: Signal::new(None),
        serial: Signal::new(0),
        unsaved: Signal::new(Vec::new()),
        card_files: Signal::new(CardFilesCache::default()),
        receiving: Signal::new(false),
        clock,
        talking,
        talking_on_page: Signal::new(false),
        said_into: Signal::new(HashSet::new()),
        words_heard: Signal::new((0, String::new())),
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

    // Leaving the app ends listening, keeping what was said: the person
    // said it, and nothing said is lost (6.18). The web view says when its
    // page is hidden, and the activity when it is paused, which it is under
    // the capture sheet, while the page is still seen.
    use_future(move || async move {
        let mut hidden = document::eval(concat!(
            "document.addEventListener('visibilitychange', () => {",
            "  if (document.visibilityState === 'hidden') dioxus.send(true);",
            "});",
            "window.addEventListener('dirt-pause', () => dioxus.send(true));",
            "await new Promise(() => {});",
        ));
        while hidden.recv::<bool>().await.is_ok() {
            screen.stop_listening();
        }
    });
    // The words of what was said, for a screen reader and the open page.
    use_future(move || async move {
        let mut words = crate::voice::words_made().subscribe();
        loop {
            match words.recv().await {
                Ok(note) => screen.words_came(&note),
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
    // The microphone could not be used: the press says why.
    use_effect(move || {
        if let Some(why) = screen.talking.unheard.read().as_ref() {
            screen.unheard(why);
        }
    });

    // Something may have come from outside the app.
    use_effect(move || {
        let _ = app.capture_ready.read();
        spawn(async move { screen.receive_captures().await });
    });

    // While Settings covers the main screen, the main screen is out of
    // reach: no tap, key or screen reader reaches it. It steps back behind
    // the sheet (5.5).
    let covered = app.nav.is_open();

    rsx! {
        style { {include_str!("screen.css")} }
        div {
            class: "dm-under dui-steps-back",
            "inert": covered.then_some(""),
            "aria-hidden": covered.then_some("true"),
            Main {}
            Page {}
            // A screen reader is told when a voice note's words arrive
            // (section 12); nothing on the screen says so but the words.
            span { class: "dui-voice-said", role: "status", "aria-live": "polite",
                "{screen.words_heard.read().1}"
            }
        }
        // Beside the screen, not inside it: the screen steps back behind a
        // sheet, and what is drawn over it must not step back with it.
        FilePreview {
            files: screen.files,
            on_open: move |file| crate::files::open_in_another_app(&screen.files, screen.toasts, file),
            on_save: move |file| crate::files::save_a_copy(&screen.files, screen.toasts, file),
        }
    }
}
