//! The desktop's main screen (interface design, sections 3, 6.1 to 6.7,
//! 6.19 and 7.4): the list on the left, with Find at its top and the sync
//! status and Settings at its foot, and the page on the right.
//!
//! [`Screen`] holds what the screen's parts share: what Find holds, what
//! the list shows, what the page shows and the thought being written, and
//! the last archive or delete, for Undo. Its methods are what the person
//! can do here; the parts call them from their buttons and keys.

mod files;
mod focus;
mod keys;
mod list;
mod page;
pub mod window;
pub mod writing;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dioxus::prelude::*;
use dirt_core::models::{extract_tags, FocusStyle, Note, NoteId};
use dirt_core::session::SessionState;
use dirt_core::store::{AttachedFile, NoteCounts, TagCount, TagRule};
use dirt_core::sync::overview::SyncIndicator;
use dirt_core::sync::worker::Trigger;
use dirt_core::Store;
use dirt_ui::components::{use_toasts, ContextMenu, ListExit, MenuItem, Toast, Toasts};
use dirt_ui::dom::{self, Caret};
use dirt_ui::files::{use_note_files, use_pasted_files, FilePreview, FileSource, NoteFiles};
use dirt_ui::find;
use dirt_ui::pile::PileView;
use dirt_ui::time::Clock;
use dirt_ui::{Appearance, Icon};

use crate::services::Incoming;
use crate::state::AppState;
use dirt_ui::settings::Section;
use focus::Edge;
use keys::KeysPanel;
use list::Side;
use page::Page;
use tokio::sync::broadcast::error::RecvError;
use window::WindowControls;
use writing::{Done, Draft, Scribe, Work};

/// The page's text field.
const PAGE_ID: &str = "dk-page";
const PAGE: &str = "#dk-page";
const FIND: &str = "#dui-find";
const LIST: &str = "#dk-list";

/// How long typing pauses before what was typed is saved.
const SAVE_AFTER: Duration = Duration::from_millis(400);

/// How long typing pauses before focus while writing comes in (7.7).
const FOCUS_AFTER: Duration = Duration::from_millis(600);

/// Text that changes this soon after a key was typed; later, it came by
/// another way (pasted with the mouse), and focus comes at once.
const TYPED_WITHIN: Duration = Duration::from_millis(400);

/// How many notes the list holds at first, and how many more it takes
/// each time scrolling nears its end.
pub const LISTED_NOTES: usize = 100;

/// How many tags the list of tags offers besides those picked (6.3).
const TAGS_OFFERED: usize = 6;

/// Where the keyboard's focus is, as far as the screen's keys care.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Where {
    /// The page's text.
    Page,
    /// Find's text.
    Find,
    /// The list of notes.
    List,
    /// Anywhere else: a button, nothing.
    #[default]
    Elsewhere,
}

/// Which way a note's text comes in as it opens: from the list's side of
/// it (section 5.5, "A note opens in the editor").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arrival {
    /// The note is further up the list than the one before.
    Above,
    /// Further down.
    Below,
}

/// What the page shows. A new value with a new `serial` is a new page: its
/// text field is made afresh with `text` in it. While the person types,
/// the field keeps its own text and this does not change.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PageView {
    pub serial: u64,
    /// The note shown, once there is one.
    pub note: Option<NoteId>,
    pub text: String,
    pub archived: bool,
    pub from: Option<Arrival>,
    /// The field takes the keyboard's focus as it appears.
    pub take_focus: bool,
}

/// Focus while writing (7.7): how far the screen has gone towards showing
/// only the note.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Writing {
    /// Everything shows.
    #[default]
    No,
    /// A key was typed on the page: the controls have faded, and the
    /// layout waits for a pause.
    FirstKeys,
    /// Only the note, in the chosen style.
    Focused,
}

impl Writing {
    /// The name the style sheet and the script know it by.
    #[must_use]
    pub const fn name(self) -> Option<&'static str> {
        match self {
            Self::No => None,
            Self::FirstKeys => Some("hush"),
            Self::Focused => Some("on"),
        }
    }
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
    pub counts: NoteCounts,
    /// Find holds words or tags.
    pub finding: bool,
    /// The list shows every archived note ("3 archived" was pressed).
    pub archived_view: bool,
    /// There may be more than the list holds.
    pub more: bool,
    /// What Find's words ask for, to mark in the rows.
    pub words: Vec<String>,
    /// The pile under the last note (6.17): read while the list is not
    /// finding, and kept as it was while it is.
    pub pile: PileView,
}

/// What the page shows once the note on it is archived or deleted.
#[derive(Debug, Clone, Copy)]
enum After {
    /// A blank page, with the cursor in it.
    Blank,
    /// The note the list's choice moved to, if any, with the keyboard left
    /// in the list.
    Show(Option<NoteId>),
}

/// What was last archived or deleted, for Undo and Ctrl Z.
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
    /// Find's text: words, and perhaps the start of a tag at its end.
    pub find_text: Signal<String>,
    /// The tags chosen, in lower case and without their `#`.
    pub tags: Signal<Vec<String>>,
    pub rule: Signal<TagRule>,
    /// The list of tags is open.
    pub tags_open: Signal<bool>,
    /// What the list of tags offers now.
    pub tag_offers: Signal<Vec<TagCount>>,
    /// The list shows the archived notes alone.
    pub archived_view: Signal<bool>,
    /// How many notes the list holds.
    pub limit: Signal<usize>,
    /// How a row that leaves the list is drawn leaving.
    pub exit: Signal<ListExit>,
    /// A note just brought back by Undo: its row drops in with a bounce.
    pub returned: Signal<Option<NoteId>>,
    /// Notes just arrived from another device: their rows flash once.
    pub flashing: Signal<HashSet<NoteId>>,
    /// A new thought just let go: its grain settles onto its row (5.5).
    pub settling: Signal<Option<NoteId>>,
    pub listing: Signal<Option<Listing>>,
    pub page: Signal<PageView>,
    draft: Signal<Draft>,
    /// Does the store's work for the page, in order.
    /// The store the page's note is in: the one in use when it was made
    /// or opened. Its work goes there even if another store has been taken
    /// into use since (signing in or out).
    page_store: CopyValue<Option<Arc<Store>>>,
    /// The files of the note on the page, and the preview of them.
    pub files: NoteFiles,
    /// The notes the store could not save, with the newest text of each,
    /// while the error says so.
    unsaved: Signal<Vec<(NoteId, String)>>,
    /// Focus while writing.
    pub writing: Signal<Writing>,
    /// When a key was last typed on the page.
    last_key: CopyValue<Option<Instant>>,
    last_undo: Signal<Option<Undo>>,
    serial: Signal<u64>,
    /// In a window narrower than 720 px, the page is over the list.
    pub page_over: Signal<bool>,
    /// The panel of keys (Ctrl /) is open.
    pub keys_open: Signal<bool>,
    pub focus: Signal<Where>,
    /// What time it is, for the times on rows and the sync status.
    pub clock: Signal<Clock>,
}

impl Screen {
    fn next_serial(mut self) -> u64 {
        let mut serial = self.serial.write();
        *serial += 1;
        *serial
    }

    /// The note open on the page.
    pub fn open_note(self) -> Option<NoteId> {
        *self.app.current_note_id.peek()
    }

    // ---- Writing ----------------------------------------------------------

    /// The store in use, if one is open.
    fn store(self) -> Option<Arc<Store>> {
        self.app.db_service.peek().clone()
    }

    /// Has the store do `work` after whatever was asked of it before: the
    /// store the page's note is in, or else the one in use.
    ///
    /// It may be asked as the screen goes, so nothing here assumes the
    /// screen's values are still there.
    fn ask(self, work: impl IntoIterator<Item = Work>) {
        let work: Vec<Work> = work.into_iter().collect();
        if work.is_empty() {
            return;
        }
        let page_store = self
            .page_store
            .try_read()
            .ok()
            .and_then(|store| store.clone());
        let in_use = self
            .app
            .db_service
            .try_peek()
            .ok()
            .and_then(|store| store.clone());
        let Some(store) = page_store.or(in_use) else {
            tracing::error!("No store is open to keep the page in");
            return;
        };
        let session = self
            .app
            .session
            .try_peek()
            .ok()
            .and_then(|session| session.clone());
        Scribe::global().ask(&store, session.as_ref(), work);
    }

    /// The page's note is in the store in use now.
    fn page_in_store_in_use(self) {
        let mut page_store = self.page_store;
        page_store.set(self.store());
    }

    /// Leaves the page: what it held is taken as it is and handed to the
    /// store's queue, to be kept or let go in the store its note is in,
    /// after whatever was asked before. The page is blank at once; nothing
    /// is waited for. The page then holds no store.
    ///
    /// The screen going (quick capture takes the window, the app quits,
    /// another store is taken into use) leaves it too, so nothing here
    /// assumes the screen's values are still there.
    pub fn leave(mut self) {
        let left = match self.draft.try_write() {
            Ok(mut draft) => draft.leave(),
            Err(error) => {
                tracing::error!("The page could not be left: {error}");
                return;
            }
        };
        self.ask(left.work);
        if let Ok(mut store) = self.page_store.try_write() {
            *store = None;
        }
    }

    /// A write failed. The person is told plainly and given their words: an
    /// error that stays until it is dismissed, with **Copy text**, which
    /// ordinary toasts neither replace nor clear. Several failures make one
    /// error, and **Copy text** gives the newest text of each of their
    /// notes. If the store could not make the note on the page, the page
    /// goes back to a thought not yet made, so that the next key makes it,
    /// with all that is on the page.
    fn unsaved(mut self, note: NoteId, text: &str, why: &str, making: bool) {
        if making && self.draft.write().unmade(note) {
            let mut current = self.app.current_note_id;
            current.set(None);
            self.page.write().note = None;
        }
        {
            let mut unsaved = self.unsaved.write();
            // An error dismissed since: its notes were seen to.
            if !self.toasts.error_showing() {
                unsaved.clear();
            }
            match unsaved.iter_mut().find(|(held, _)| *held == note) {
                Some(held) => text.clone_into(&mut held.1),
                None => unsaved.push((note, text.to_owned())),
            }
        }
        let count = self.unsaved.peek().len();
        let words = if count == 1 {
            "A note could not be saved".to_owned()
        } else {
            format!("{count} notes could not be saved")
        };
        let copy = Callback::new(move |()| self.copy_unsaved());
        self.toasts.show_error(
            Toast::new(words)
                .icon(Icon::Alert)
                .detail(why.to_owned())
                .action("Copy text", Icon::Copy, copy),
        );
    }

    /// The text of every note that could not be saved, newest of each, on
    /// the clipboard: **Copy text** on the error.
    fn copy_unsaved(self) {
        let texts: Vec<String> = self
            .unsaved
            .peek()
            .iter()
            .map(|(_, text)| text.clone())
            .collect();
        let text = texts.join("\n\n");
        if let Err(error) = arboard::Clipboard::new().and_then(|mut board| board.set_text(text)) {
            tracing::error!("Could not copy the text: {error}");
        }
    }

    /// What the store's work came to.
    fn heard(mut self, done: &Done) {
        match *done {
            Done::PutAway {
                note,
                delete,
                kept: true,
            } => {
                let serial = self.next_serial();
                let undo = Callback::new(move |()| self.undo(Some(serial)));
                self.last_undo.set(Some(Undo {
                    serial,
                    note,
                    deleted: delete,
                }));
                let toast = if delete {
                    Toast::new("Note deleted").icon(Icon::Trash)
                } else {
                    Toast::new("Archived").icon(Icon::Archive)
                };
                self.toasts.show(toast.undo(undo));
            }
            Done::Attached { note, ref failed } => {
                if self.open_note() == Some(note) {
                    self.files.reread();
                }
                if !failed.is_empty() {
                    let words = if failed.len() == 1 {
                        "A file could not be attached".to_owned()
                    } else {
                        format!("{} files could not be attached", failed.len())
                    };
                    self.toasts
                        .show_error(Toast::new(words).icon(Icon::Alert).detail(failed.join(" ")));
                }
            }
            Done::Unsaved {
                note,
                ref text,
                ref why,
                making,
            } => self.unsaved(note, text, why, making),
            _ => {}
        }
        if done.changed() {
            self.app.reread();
        }
    }

    /// The page's text changed: it was typed, pasted or cut. The first
    /// character that is not a space makes the note (6.1); from then on it
    /// saves itself, a moment after typing pauses.
    ///
    /// `serial` is the page the text was typed on. A page already replaced
    /// may still say something in the moment it takes to go; that is not
    /// this page's text.
    pub fn typed(mut self, serial: u64, text: &str) {
        if serial != self.page.peek().serial {
            return;
        }
        let new_line = text.matches('\n').count() > self.draft.peek().text().matches('\n').count();
        let first = self.draft.write().typed(text.to_owned());
        let version = self.draft.peek().version();
        self.writing_typed(new_line, version);
        if let Some(work) = first {
            let note = self.draft.peek().note();
            let mut current = self.app.current_note_id;
            current.set(note);
            self.page.write().note = note;
            self.page_in_store_in_use();
            // With the list narrowed to tags, a new page gets them at its
            // end, where they can be seen and removed. The field says so
            // as if they had been typed, and that saves them.
            let written = extract_tags(text);
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
            self.page_over.set(true);
            self.ask([work]);
            return;
        }
        spawn(async move {
            tokio::time::sleep(SAVE_AFTER).await;
            if self.draft.peek().version() == version {
                let work = self.draft.write().flush();
                self.ask(work);
            }
        });
    }

    /// A blank page, made afresh: what had the page is replaced, and the
    /// cursor goes into the new one if `take_focus`.
    fn blank(mut self, take_focus: bool) {
        let serial = self.next_serial();
        let mut current = self.app.current_note_id;
        current.set(None);
        self.page.set(PageView {
            serial,
            take_focus,
            ..PageView::default()
        });
        self.page_over.set(false);
    }

    /// A blank page in the same field, which the script has emptied as the
    /// key that left the note was pressed (`data-dui-page`). Whatever was
    /// typed after that key is in the field already, and is the next
    /// thought's.
    fn blank_in_place(mut self) {
        let serial = self.page.peek().serial;
        let mut current = self.app.current_note_id;
        current.set(None);
        self.page.set(PageView {
            serial,
            ..PageView::default()
        });
        self.page_over.set(false);
    }

    /// Done with the thought on the page (Esc, Ctrl Enter, ✓, Ctrl N): it
    /// is kept, and the page is blank again with the cursor in it. With
    /// nothing written, done does nothing but put the cursor there.
    ///
    /// `emptied` says that the key was pressed in the page and the script
    /// has already emptied the field, so that the field stays and what is
    /// typed straight after is kept.
    pub fn done(self, emptied: bool) {
        self.calm();
        let settles = self.draft.peek().settles();
        if self.draft.peek().is_blank() {
            // In a narrow window the blank page comes over the list, and
            // goes again.
            let mut over = self.page_over;
            if *over.peek() {
                over.set(false);
                self.focus_page(Caret::Kept);
            } else if !emptied {
                self.new_page();
            }
            return;
        }
        self.leave();
        if emptied {
            self.blank_in_place();
        } else {
            self.blank(true);
        }
        if let Some(note) = settles {
            self.settle(note);
        }
    }

    /// A new thought settles onto the pile: its row is marked for a moment,
    /// and the script tosses its grain onto it (5.5).
    fn settle(mut self, note: NoteId) {
        self.settling.set(Some(note));
        spawn(async move {
            tokio::time::sleep(Duration::from_millis(2500)).await;
            if *self.settling.peek() == Some(note) {
                self.settling.set(None);
            }
        });
    }

    /// The blank page with the cursor in it. In a window narrower than
    /// 720 px it comes over the list; it is focused once it is drawn.
    pub fn new_page(mut self) {
        self.page_over.set(true);
        spawn(async move {
            tokio::time::sleep(Duration::from_millis(40)).await;
            self.focus_page(Caret::Kept);
        });
    }

    /// Opens a note on the page. With `edit` the keyboard goes into its
    /// text; without, it stays where it is, for moving through the list.
    pub fn open(self, id: NoteId, edit: bool) {
        self.calm();
        self.show_note(id, edit);
    }

    /// Opens a note from the rail (7.7): focus stays.
    pub fn open_from_rail(self, id: NoteId) {
        self.show_note(id, true);
    }

    /// Puts a note on the page at once. The list's copy of it is shown
    /// straight away, so that the arrow keys can move on from it before
    /// the store has answered; the store's copy follows, and is taken if
    /// it says more and nothing has been typed yet.
    fn show_note(mut self, id: NoteId, edit: bool) {
        if self.open_note() == Some(id) {
            self.page_over.set(true);
            if edit {
                self.focus_page(Caret::End);
            }
            return;
        }
        let rows = self.row_order();
        let was = self
            .open_note()
            .and_then(|open| rows.iter().position(|row| *row == open));
        let now = rows.iter().position(|row| *row == id);
        let from = match (was, now) {
            (Some(was), Some(now)) if now > was => Some(Arrival::Below),
            (Some(was), Some(now)) if now < was => Some(Arrival::Above),
            _ => None,
        };
        self.leave();
        // Text of the note still waiting in the queue is newer than the
        // list's copy and the store's: the note shows that.
        let waiting = |mut note: Note| {
            if let Some(text) = Scribe::global().waiting_text(&note.id) {
                note.content = text;
            }
            note
        };
        let listed = self
            .listing
            .peek()
            .as_ref()
            .and_then(|listing| listing.notes.iter().find(|note| note.id == id).cloned())
            .map(waiting);
        if let Some(note) = listed {
            self.show(note, from, edit);
        } else {
            self.blank(false);
        }
        let shown = self.page.peek().serial;
        let Some(store) = self.store() else { return };
        spawn(async move {
            let note = match store.get_note(&id).await {
                Ok(Some(note)) => waiting(note),
                Ok(None) => return,
                Err(error) => {
                    tracing::error!("Could not open the note: {error}");
                    return;
                }
            };
            if self.page.peek().serial != shown {
                return;
            }
            if self.open_note() != Some(id) {
                self.show(note, from, edit);
            } else if self.draft.write().refreshed(&note) {
                self.page.write().archived = note.archived;
                dom::set_value(PAGE, &note.content);
            }
        });
    }

    /// Puts `note` on the page.
    fn show(mut self, note: Note, from: Option<Arrival>, take_focus: bool) {
        let serial = self.next_serial();
        let mut current = self.app.current_note_id;
        current.set(Some(note.id));
        self.page_in_store_in_use();
        self.draft.write().show(&note);
        self.page.set(PageView {
            serial,
            note: Some(note.id),
            text: note.content,
            archived: note.archived,
            from,
            take_focus,
        });
        self.page_over.set(true);
    }

    /// The page is blank: no note, and nothing typed.
    pub fn page_is_blank(self) -> bool {
        self.draft.peek().is_blank()
    }

    /// The notes of the list, in the order shown.
    pub fn row_order(self) -> Vec<NoteId> {
        self.listing
            .peek()
            .as_ref()
            .map(|listing| listing.notes.iter().map(|note| note.id).collect())
            .unwrap_or_default()
    }

    fn is_archived(self, id: NoteId) -> bool {
        if self.open_note() == Some(id) {
            return self.page.peek().archived;
        }
        self.listing.peek().as_ref().is_some_and(|listing| {
            listing
                .notes
                .iter()
                .any(|note| note.id == id && note.archived)
        })
    }

    // ---- Focus while writing (7.7) ------------------------------------------

    /// A key was pressed on the page.
    pub fn key_on_page(self) {
        let mut last = self.last_key;
        last.set(Some(Instant::now()));
    }

    /// The page's text changed. The first key only fades the controls; the
    /// column does not move under a word being typed. Focus comes at the
    /// first pause or the first new line (5.5). Text that came without a
    /// key, pasted with the mouse, brings it at once.
    fn writing_typed(mut self, new_line: bool, version: u64) {
        let now = *self.writing.peek();
        if now == Writing::Focused {
            return;
        }
        let keyed = self
            .last_key
            .peek()
            .is_some_and(|at| at.elapsed() < TYPED_WITHIN);
        if new_line || !keyed {
            self.writing.set(Writing::Focused);
            return;
        }
        if now != Writing::FirstKeys {
            self.writing.set(Writing::FirstKeys);
        }
        spawn(async move {
            tokio::time::sleep(FOCUS_AFTER).await;
            if self.draft.peek().version() == version && *self.writing.peek() == Writing::FirstKeys
            {
                self.writing.set(Writing::Focused);
            }
        });
    }

    /// Focus while writing ends: the list and the controls come back.
    pub fn calm(mut self) {
        if *self.writing.peek() != Writing::No {
            self.writing.set(Writing::No);
        }
    }

    /// The keyboard went somewhere. Into Find or the list, focus ends: the
    /// person has gone to the list.
    pub fn focused(mut self, at: Where) {
        self.focus.set(at);
        if matches!(at, Where::Find | Where::List) {
            self.calm();
        }
    }

    /// The chosen style of focus while writing.
    pub fn focus_style(self) -> FocusStyle {
        self.app.settings.read().focus_style
    }

    // ---- Archive, delete, undo (6.19, 6.7) --------------------------------

    /// Archives a note, or deletes it if it is archived already: what the
    /// box, Del and Ctrl Del do.
    pub fn archive_or_delete(self, id: NoteId) {
        if self.is_archived(id) {
            self.delete(id);
        } else {
            self.archive(id);
        }
    }

    /// The same from the list's keys: the choice moves to `next`, and the
    /// keyboard stays in the list.
    pub fn archive_or_delete_from_list(self, id: NoteId, next: Option<NoteId>) {
        let delete = self.is_archived(id);
        self.put_away(id, delete, After::Show(next));
    }

    /// Deletes for good from the list's keys: Shift Del.
    pub fn delete_from_list(self, id: NoteId, next: Option<NoteId>) {
        self.put_away(id, true, After::Show(next));
    }

    /// Archives a note: out of the list, with **Undo**.
    pub fn archive(self, id: NoteId) {
        self.put_away(id, false, After::Blank);
    }

    /// Deletes a note for good, with **Undo**. No question is asked first.
    pub fn delete(self, id: NoteId) {
        self.put_away(id, true, After::Blank);
    }

    /// Archives or deletes a note, and says so with **Undo** once it is
    /// done. If it was the note on the page, the page shows what `after`
    /// says at once, and what was typed in the note is kept first. A note
    /// emptied on the page is let go as it is left: there is nothing to
    /// archive or delete then, and nothing to undo.
    fn put_away(mut self, id: NoteId, delete: bool, after: After) {
        let was_open = self.open_note() == Some(id);
        if was_open {
            self.calm();
            self.leave();
        }
        let work = [Work::PutAway { note: id, delete }];
        self.exit.set(if delete {
            ListExit::Tear
        } else {
            ListExit::Fold
        });
        self.ask(work);
        if was_open {
            match after {
                After::Blank => self.blank(true),
                After::Show(None) => self.blank(false),
                After::Show(Some(next)) => {
                    self.blank(false);
                    self.open(next, false);
                }
            }
        }
    }

    /// Brings an archived note back to the list. Not an edit.
    pub fn unarchive(mut self, id: NoteId) {
        spawn(async move {
            let Some(store) = self.app.db_service.peek().clone() else {
                return;
            };
            if let Err(error) = store.unarchive_note(&id).await {
                tracing::error!("Could not bring the note back: {error}");
                return;
            }
            if self.open_note() == Some(id) {
                self.page.write().archived = false;
            }
            self.app.note_changed();
        });
    }

    /// Undoes the last archive or delete. `serial` names the one a toast
    /// offered to undo: if something newer has happened since, that toast
    /// is out of date and does nothing. Without it, as for Ctrl Z, the last
    /// one is undone, however long ago the toast went.
    pub fn undo(mut self, serial: Option<u64>) {
        let taken = {
            let mut last = self.last_undo.write();
            match (*last, serial) {
                (Some(undo), Some(serial)) if undo.serial != serial => None,
                _ => last.take(),
            }
        };
        let Some(undo) = taken else { return };
        self.toasts.clear();
        spawn(async move {
            let Some(store) = self.app.db_service.peek().clone() else {
                return;
            };
            let done = if undo.deleted {
                store.restore_note(&undo.note).await.map(|_| ())
            } else {
                store.unarchive_note(&undo.note).await.map(|_| ())
            };
            if let Err(error) = done {
                tracing::error!("Could not undo: {error}");
                return;
            }
            self.exit.set(ListExit::None);
            self.returned.set(Some(undo.note));
            if self.open_note() == Some(undo.note) && !undo.deleted {
                self.page.write().archived = false;
            }
            self.app.note_changed();
            tokio::time::sleep(Duration::from_millis(1200)).await;
            if *self.returned.peek() == Some(undo.note) {
                self.returned.set(None);
            }
        });
    }

    // ---- Find and tags (6.1, 6.3) -----------------------------------------

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
            return;
        }
        self.tags.write().push(tag.to_owned());
        let text = self.find_text.peek().clone();
        if find::tag_word(&text).is_some() {
            self.set_find(find::words(&text));
        }
    }

    /// Narrows the list to a tag pressed on a note.
    pub fn narrow_to(mut self, tag: &str) {
        self.archived_view.set(false);
        if !self.tags.peek().iter().any(|chosen| chosen == tag) {
            self.toggle_tag(tag);
        }
    }

    /// The first tag the list of tags offers, picked: Tab, and Enter on a
    /// lone `#tag`.
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

    /// Takes back the last tag picked.
    pub fn untag_last(mut self) {
        self.exit.set(ListExit::None);
        self.tags.write().pop();
    }

    /// Esc in Find: closes the list of tags; then empties the text; then
    /// takes back the tags; then goes back to the page.
    pub fn find_escape(mut self) {
        if *self.tags_open.peek() {
            self.tags_open.set(false);
        } else if !self.find_text.peek().is_empty() {
            self.set_find("");
        } else if !self.tags.peek().is_empty() {
            self.exit.set(ListExit::None);
            self.tags.set(Vec::new());
        } else {
            self.focus_page(Caret::Kept);
        }
    }

    /// Any letter typed in the list goes to Find, and is typed there.
    pub fn type_into_find(self, letter: &str) {
        let text = format!("{}{letter}", self.find_text.peek());
        self.set_find(&text);
        self.find_typed(text);
        dom::focus(&[FIND], Caret::End);
    }

    /// Shows every archived note, or goes back to the list.
    pub fn show_archived(mut self, shown: bool) {
        self.exit.set(ListExit::None);
        self.limit.set(LISTED_NOTES);
        self.archived_view.set(shown);
    }

    // ---- Files -------------------------------------------------------------

    /// Attaches files: to the open note, or to a new one made for them, as
    /// a file with no words under it is a note (6.1).
    ///
    /// Nothing is attached while the preview, Settings or the panel of keys
    /// is over the note: the person cannot see where the files would go.
    /// One drop or paste takes as many files as a note may have; the rest
    /// are named in an error.
    pub fn attach(mut self, mut files: Vec<Incoming>) {
        if files.is_empty() || self.something_over_the_note() {
            return;
        }
        let most = usize::try_from(dirt_core::files::MAX_FILES_PER_NOTE).unwrap_or(usize::MAX);
        if files.len() > most {
            let left: Vec<String> = files.split_off(most).iter().map(Incoming::name).collect();
            self.toasts.show_error(
                Toast::new(format!("{} files were not attached", left.len()))
                    .icon(Icon::Alert)
                    .detail(format!(
                        "A note may have at most {most} files. Not attached: {}.",
                        left.join(", ")
                    )),
            );
        }
        let tags: Vec<String> = self
            .tags
            .peek()
            .iter()
            .map(|tag| format!("#{tag}"))
            .collect();
        let (note, made) = self.draft.write().note_for_files(|typed| {
            if tags.is_empty() {
                typed.to_owned()
            } else {
                format!("{} {}", typed.trim_end(), tags.join(" "))
                    .trim_start()
                    .to_owned()
            }
        });
        if made.is_some() {
            // The blank page becomes the note, with the tags written in.
            let serial = self.next_serial();
            let mut current = self.app.current_note_id;
            current.set(Some(note));
            self.page_in_store_in_use();
            let text = self.draft.peek().text().to_owned();
            self.page.set(PageView {
                serial,
                note: Some(note),
                text,
                take_focus: true,
                ..PageView::default()
            });
            self.page_over.set(true);
        }
        self.ask(made.into_iter().chain([Work::Attach { note, files }]));
    }

    /// The preview, Settings or the panel of keys is open over the note.
    fn something_over_the_note(self) -> bool {
        self.files.previewing().is_some() || self.app.nav.is_open() || *self.keys_open.peek()
    }

    /// Opens one of the note's files in another app.
    pub fn open_file(self, file: AttachedFile) {
        files::open_in_another_app(&self.app, &self.files, self.toasts, file);
    }

    /// Saves a copy of one of the note's files where the person says.
    pub fn save_file(self, file: AttachedFile) {
        files::save_a_copy(&self.files, self.toasts, file);
    }

    /// Opens the file dialog, and attaches what is chosen: 📎 and Ctrl
    /// Shift A.
    pub fn choose_files(self) {
        spawn(async move {
            let chosen = rfd::AsyncFileDialog::new()
                .set_title("Attach files to the note")
                .pick_files()
                .await;
            if let Some(chosen) = chosen {
                self.attach(
                    chosen
                        .iter()
                        .map(|file| Incoming::Path(file.path().to_path_buf()))
                        .collect(),
                );
            }
        });
    }

    // ---- The rest of the keys (7.4) -----------------------------------------

    /// Copies the note's text: Ctrl Shift C, and the menu.
    pub fn copy_note(self) {
        let text = self.draft.peek().text().to_owned();
        if text.is_empty() {
            return;
        }
        if let Err(error) = arboard::Clipboard::new().and_then(|mut board| board.set_text(text)) {
            tracing::error!("Could not copy the note: {error}");
        }
    }

    /// Paste, chosen in the app's own menu.
    pub fn paste() {
        match arboard::Clipboard::new().and_then(|mut board| board.get_text()) {
            Ok(text) => dom::paste(&text),
            Err(error) => tracing::warn!("Nothing to paste: {error}"),
        }
    }

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

    /// What pressing the sync status does (6.6): syncs now, or opens
    /// where a problem is explained, or where to sign in.
    pub fn sync_now(self) {
        match self.sync_indicator() {
            SyncIndicator::Problem { .. } => self.app.nav.open_at(Section::Sync),
            // A device that waits to be let in waits in Devices.
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
            _ => {
                if let Some(session) = self.app.session.peek().as_ref() {
                    session.notify(Trigger::Manual);
                }
            }
        }
    }

    /// Ctrl + and Ctrl −: the text size, kept in this device's settings.
    pub fn text_size(self, larger: bool) {
        let mut settings = self.app.settings;
        let mut chosen = settings.peek().clone();
        let mut appearance = Appearance::from_settings(&chosen);
        appearance.text_size = if larger {
            appearance.text_size.larger()
        } else {
            appearance.text_size.smaller()
        };
        appearance.write_to(&mut chosen);
        settings.set(chosen);
        self.app.save_settings();
    }

    /// The gear, and <kbd>Ctrl ,</kbd>: Settings, where it was.
    pub fn open_settings(self) {
        self.app.nav.toggle();
    }

    /// To the page's text: back from the list, and after done.
    #[allow(clippy::unused_self)] // One way to reach every action: the screen's.
    pub fn focus_page(self, caret: Caret) {
        dom::focus(&[PAGE, FIND], caret);
    }

    /// To Find, its text selected: Ctrl F, Ctrl K, `/`.
    pub fn focus_find(mut self) {
        self.archived_view.set(false);
        dom::focus(&[FIND], Caret::All);
    }

    /// Into the list: the open note's row, or the first.
    pub fn focus_list(self) {
        if self.open_note().is_none() {
            if let Some(first) = self.row_order().first() {
                self.open(*first, false);
            }
        }
        dom::focus(&[LIST], Caret::Kept);
    }
}

/// Keeps the clock: every half minute, for the times on rows, and soon
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

/// Reads what the list shows from the store, again whenever what it
/// depends on changes, and marks the notes a sync has just brought.
fn use_listing(screen: &Screen, app: &AppState) {
    let (screen, app) = (*screen, *app);
    let mut seen = use_signal(HashMap::<NoteId, i64>::new);
    let mut pulls_seen = use_signal(|| 0_u64);
    let _reader = use_resource(move || {
        let store = app.db_service.read().clone();
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
            let words = find::words(&text).trim().to_owned();
            let finding = !archived_view && (!words.is_empty() || !tags.is_empty());
            let notes = if archived_view {
                store.list_archived_notes(limit, 0).await
            } else if finding {
                store.find_notes_by_tags(&words, &tags, rule, limit).await
            } else {
                store.list_notes(limit, 0).await
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
                store.unsynced_note_ids(shown).await.unwrap_or_default()
            } else {
                HashSet::new()
            };
            let with_files = store.notes_with_files().await.unwrap_or_default();
            let mut first_file = HashMap::new();
            for note in notes
                .iter()
                .filter(|note| with_files.contains(&note.id) && note.outline().title.is_empty())
            {
                if let Some(file) = store
                    .note_attachments(&note.id)
                    .await
                    .ok()
                    .and_then(|files| files.into_iter().next())
                {
                    first_file.insert(note.id, file.name);
                }
            }
            let counts = store.note_counts().await.unwrap_or_default();
            let more = notes.len() >= limit;
            let before = listing_signal
                .peek()
                .as_ref()
                .map(|listing| listing.pile.clone());
            let pile = if finding || archived_view {
                before.unwrap_or_default()
            } else {
                match store.pile().await {
                    Ok(pile) => {
                        let offset = Clock::now().utc_offset_secs;
                        PileView::of(pile, before.as_ref(), offset).await
                    }
                    Err(error) => {
                        tracing::error!("Could not read the pile: {error}");
                        before.unwrap_or_default()
                    }
                }
            };

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

            let listing = Listing {
                notes,
                waiting,
                with_files,
                first_file,
                counts,
                finding,
                archived_view,
                more,
                words: dirt_ui::text::find_words(&words),
                pile,
            };
            listing_signal.set(Some(listing));
            Some(())
        }
    });
}

/// Reads what the list of tags offers, while it is open.
fn use_tag_offers(screen: &Screen, app: &AppState) {
    let (screen, app) = (*screen, *app);
    let _reader = use_resource(move || {
        let store = app.db_service.read().clone();
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
            store: app.db_service.peek().as_deref().cloned(),
            session: app.session.peek().clone(),
        },
        move || app.note_changed(),
    );
    files::use_served_files();
    let screen = use_context_provider(|| Screen {
        app,
        toasts,
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
        page: Signal::new(PageView {
            take_focus: true,
            ..PageView::default()
        }),
        draft: Signal::new(Draft::default()),
        page_store: CopyValue::new(None),
        files: note_files,
        unsaved: Signal::new(Vec::new()),
        writing: Signal::new(Writing::No),
        last_key: CopyValue::new(None),
        last_undo: Signal::new(None),
        serial: Signal::new(0),
        page_over: Signal::new(false),
        keys_open: Signal::new(false),
        focus: Signal::new(Where::Elsewhere),
        clock,
    });
    // What the store's work came to: Undo for a note put away, files
    // attached, the list to read again.
    use_future(move || async move {
        let mut news = Scribe::global().listen();
        loop {
            match news.recv().await {
                Ok(done) => screen.heard(&done),
                // Some news was missed while the screen was busy: the list
                // is read again, which is what most news asks for.
                Err(RecvError::Lagged(_)) => screen.app.reread(),
                Err(RecvError::Closed) => break,
            }
        }
    });
    // The screen going (quick capture takes the window) hands over what
    // the page holds, and so does the app quitting from the tray, which
    // then waits for the queue. Before another store is taken into use the
    // app asks for the page through `page_leaver`, and waits for the queue.
    use_hook(move || {
        let mut leaver = app.page_leaver;
        leaver.set(Some(Callback::new(move |()| screen.leave())));
    });
    use_drop(move || {
        let mut leaver = app.page_leaver;
        if let Ok(mut leaver) = leaver.try_write() {
            *leaver = None;
        }
        screen.leave();
    });
    use_effect(move || {
        if *app.quitting.read() {
            screen.leave();
        }
    });
    use_listing(&screen, &app);
    use_tag_offers(&screen, &app);
    // A picture pasted into the page is attached to its note (7.3).
    use_pasted_files(PAGE, move |pasted| {
        screen.attach(pasted.into_iter().map(Incoming::Pasted).collect());
    });

    // A note closed under the page (another store taken into use on
    // signing in or out) leaves a blank page. What it held belongs to the
    // store that was let go; the work asked of that store before is done.
    use_effect(move || {
        let open = *app.current_note_id.read();
        if open.is_none() && screen.page.peek().note.is_some() {
            // Left already, as the app took the other store into use.
            screen.leave();
            screen.calm();
            screen.blank(false);
        }
    });

    // A note asked for from elsewhere: Settings → Sync.
    use_effect(move || {
        let mut request = app.open_request;
        let asked = *request.read();
        if let Some(id) = asked {
            request.set(None);
            screen.open(id, true);
        }
    });

    let over = *screen.page_over.read() || app.current_note_id.read().is_some();
    let has_note = app.current_note_id.read().is_some();
    let writing = screen.writing.read().name();
    let style = screen.focus_style().name();
    let drag = window::use_window_drag();

    rsx! {
        style { {include_str!("screen.css")} }
        div {
            class: "dk",
            "data-over": over.then_some(""),
            // Focus while writing (7.7): the style sheet lays it out, and
            // the script plays what depends on where the rows are.
            "data-dui-focus": style,
            "data-dui-writing": writing,
            // The window's top row moves it, where there is no title bar.
            "data-dui-drag": drag.is_some().then_some(window::DRAG_ROW),
            "data-dui-id": drag,
            // Where the keyboard goes when what had it is taken away, so
            // that the screen's keys are still heard.
            tabindex: "-1",
            "data-dui-home": "",
            onkeydown: move |event| keys::on_key(&screen, &app, &event),
            // Files dropped anywhere on the window are attached to the open
            // note, or become a note of their own (7.3). Saying that a drop
            // is welcome is what lets one happen.
            "data-dui-drop": "",
            ondragover: move |event: Event<DragData>| event.prevent_default(),
            ondrop: move |event: Event<DragData>| {
                // Left to itself the window would leave the app and show
                // the file.
                event.prevent_default();
                screen.attach(files::dropped(&event).into_iter().map(Incoming::Path).collect());
            },
            // A press anywhere but Find and the list of tags closes it.
            onclick: move |_| {
                let mut open = screen.tags_open;
                if *open.peek() {
                    open.set(false);
                }
            },
            if let Some(message) = app.startup_error.read().clone() {
                div { class: "dk-alarm", role: "alert", "{message}" }
            }
            div { class: "dk-body",
                Side {}
                Edge {}
                Page {}
            }
            WindowControls {}
            div { class: "dui-drop-hint", "aria-hidden": "true",
                span {
                    if has_note {
                        "Drop to attach to this note"
                    } else {
                        "Drop to make a note of it"
                    }
                }
            }
            FilePreview {
                files: screen.files,
                on_open: move |file| screen.open_file(file),
                on_save: move |file| screen.save_file(file),
            }
            ContextMenu { on_paste: move |()| Screen::paste(),
                if has_note {
                    MenuItem {
                        icon: Icon::Copy,
                        label: "Copy the note",
                        shortcut: "Ctrl Shift C",
                        onclick: move |_| screen.copy_note(),
                    }
                }
            }
            KeysPanel {}
        }
    }
}
