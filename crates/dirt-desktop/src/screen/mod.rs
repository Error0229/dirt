//! The desktop's main screen (interface design, sections 3, 6.1 to 6.7,
//! 6.19 and 7.4): the list on the left, with Find at its top and the sync
//! status and Settings at its foot, and the page on the right.
//!
//! [`Screen`] holds what the screen's parts share: what Find holds, what
//! the list shows, what the page shows and the thought being written, and
//! the last archive or delete, for Undo. Its methods are what the person
//! can do here; the parts call them from their buttons and keys.

mod keys;
mod list;
mod page;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

use dioxus::prelude::*;
use dirt_core::models::{extract_tags, Note, NoteId};
use dirt_core::store::{NoteCounts, TagCount, TagRule};
use dirt_core::sync::overview::SyncIndicator;
use dirt_core::sync::worker::Trigger;
use dirt_ui::components::{use_toasts, ContextMenu, ListExit, MenuItem, Toast, Toasts};
use dirt_ui::dom::{self, Caret};
use dirt_ui::find;
use dirt_ui::time::Clock;
use dirt_ui::{Appearance, Icon};

use crate::state::{AppState, SettingsTab};
use keys::KeysPanel;
use list::Side;
use page::Page;

/// The page's text field.
const PAGE_ID: &str = "dk-page";
const PAGE: &str = "#dk-page";
const FIND: &str = "#dui-find";
const LIST: &str = "#dk-list";

/// How long typing pauses before what was typed is saved.
const SAVE_AFTER: Duration = Duration::from_millis(400);

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

/// The thought being written: what was typed last, and what the store
/// has of it.
#[derive(Debug, Clone, Default)]
struct Draft {
    text: String,
    saved: String,
    version: u64,
    /// The note is being made: the first character has been typed and the
    /// store has not answered yet.
    creating: bool,
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
    pub listing: Signal<Option<Listing>>,
    pub page: Signal<PageView>,
    draft: Signal<Draft>,
    last_undo: Signal<Option<Undo>>,
    serial: Signal<u64>,
    /// In a window narrower than 720 px, the page is over the list.
    pub page_over: Signal<bool>,
    /// The panel of keys (Ctrl /) is open.
    pub keys_open: Signal<bool>,
    pub focus: Signal<Where>,
    /// Files to attach to the open note, taken by its list of files.
    pub incoming: Signal<Vec<PathBuf>>,
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

    /// The page's text changed: it was typed, pasted or cut. The first
    /// character that is not a space makes the note (6.1); from then on it
    /// saves itself, a moment after typing pauses.
    pub fn typed(mut self, text: String) {
        let no_note = self.open_note().is_none();
        let (version, first) = {
            let mut draft = self.draft.write();
            draft.text.clone_from(&text);
            draft.version += 1;
            let first = no_note && !draft.creating && !text.trim().is_empty();
            if first {
                draft.creating = true;
            }
            (draft.version, first)
        };
        if first {
            // With the list narrowed to tags, a new page gets them at its
            // end, where they can be seen and removed. The field says so
            // as if they had been typed, and that saves them.
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
            self.page_over.set(true);
            spawn(async move { self.create(text).await });
            return;
        }
        spawn(async move {
            tokio::time::sleep(SAVE_AFTER).await;
            if self.draft.peek().version == version {
                self.save().await;
            }
        });
    }

    async fn create(mut self, text: String) {
        let Some(store) = self.app.db_service.peek().clone() else {
            self.draft.write().creating = false;
            return;
        };
        match store.create_note(&text).await {
            Ok(note) => {
                let mut current = self.app.current_note_id;
                current.set(Some(note.id));
                self.page.write().note = Some(note.id);
                {
                    let mut draft = self.draft.write();
                    draft.saved = text;
                    draft.creating = false;
                }
                self.app.note_changed();
                self.save().await;
            }
            Err(error) => {
                tracing::error!("Could not make the note: {error}");
                self.draft.write().creating = false;
            }
        }
    }

    /// Saves what was typed, if the store does not have it yet.
    async fn save(mut self) {
        let Some(id) = self.open_note() else { return };
        let text = {
            let draft = self.draft.peek();
            if draft.text == draft.saved {
                return;
            }
            draft.text.clone()
        };
        let Some(store) = self.app.db_service.peek().clone() else {
            return;
        };
        match store.update_note(&id, &text).await {
            Ok(_) => {
                if self.open_note() == Some(id) {
                    self.draft.write().saved = text;
                }
                self.app.note_changed();
            }
            Err(error) => tracing::error!("Could not save the note: {error}"),
        }
    }

    /// Leaves the note on the page: saves it, and lets it go if it was
    /// emptied again (6.1). Returns which note it was.
    async fn leave(self) -> Option<NoteId> {
        // A note still being made is waited for, so that it is not lost.
        for _ in 0..200 {
            if !self.draft.peek().creating {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        self.save().await;
        let id = self.open_note()?;
        if self.draft.peek().text.trim().is_empty() {
            if let Some(store) = self.app.db_service.peek().clone() {
                match store.let_go_if_empty(&id).await {
                    Ok(_) => self.app.note_changed(),
                    Err(error) => tracing::error!("Could not let the empty note go: {error}"),
                }
            }
        }
        Some(id)
    }

    /// A blank page, ready for the next thought.
    fn blank(mut self, take_focus: bool) {
        let serial = self.next_serial();
        let mut current = self.app.current_note_id;
        current.set(None);
        {
            let mut draft = self.draft.write();
            let version = draft.version + 1;
            *draft = Draft {
                version,
                ..Draft::default()
            };
        }
        self.page.set(PageView {
            serial,
            take_focus,
            ..PageView::default()
        });
        self.page_over.set(false);
    }

    /// Done with the thought on the page (Esc, Ctrl Enter, ✓, Ctrl N): it
    /// is saved, and the page is blank again with the cursor in it. With
    /// nothing written, done does nothing but put the cursor there.
    pub fn done(self) {
        let written = self.open_note().is_some() || !self.draft.peek().text.is_empty();
        if !written {
            // In a narrow window the blank page comes over the list, and
            // goes again.
            let mut over = self.page_over;
            if *over.peek() {
                over.set(false);
                self.focus_page(Caret::Kept);
            } else {
                self.new_page();
            }
            return;
        }
        spawn(async move {
            self.leave().await;
            self.blank(true);
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
    pub fn open(mut self, id: NoteId, edit: bool) {
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
        spawn(async move {
            self.leave().await;
            let Some(store) = self.app.db_service.peek().clone() else {
                return;
            };
            let note = match store.get_note(&id).await {
                Ok(Some(note)) => note,
                Ok(None) => return,
                Err(error) => {
                    tracing::error!("Could not open the note: {error}");
                    return;
                }
            };
            self.show(note, from, edit);
        });
    }

    /// Puts `note` on the page.
    fn show(mut self, note: Note, from: Option<Arrival>, take_focus: bool) {
        let serial = self.next_serial();
        let mut current = self.app.current_note_id;
        current.set(Some(note.id));
        {
            let mut draft = self.draft.write();
            let version = draft.version + 1;
            *draft = Draft {
                text: note.content.clone(),
                saved: note.content.clone(),
                version,
                creating: false,
            };
        }
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
        self.open_note().is_none() && self.draft.peek().text.is_empty()
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

    /// Archives or deletes a note, and says so with **Undo**. If it was the
    /// note on the page, the page then shows what `after` says.
    fn put_away(mut self, id: NoteId, delete: bool, after: After) {
        let serial = self.next_serial();
        let undo = Callback::new(move |()| self.undo(Some(serial)));
        let was_open = self.open_note() == Some(id);
        spawn(async move {
            if was_open {
                self.leave().await;
            }
            let Some(store) = self.app.db_service.peek().clone() else {
                return;
            };
            self.exit.set(if delete {
                ListExit::Tear
            } else {
                ListExit::Fold
            });
            let done = if delete {
                store.delete_note(&id).await
            } else {
                store.archive_note(&id).await.map(|_| ())
            };
            if let Err(error) = done {
                tracing::error!("Could not put the note away: {error}");
                return;
            }
            self.last_undo.set(Some(Undo {
                serial,
                note: id,
                deleted: delete,
            }));
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
            self.app.note_changed();
            let toast = if delete {
                Toast::new("Note deleted").icon(Icon::Trash)
            } else {
                Toast::new("Archived").icon(Icon::Archive)
            };
            self.toasts.show(toast.undo(undo));
        });
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
    pub fn attach(mut self, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            return;
        }
        if self.open_note().is_some() {
            self.incoming.set(paths);
            return;
        }
        spawn(async move {
            let Some(store) = self.app.db_service.peek().clone() else {
                return;
            };
            let tags: Vec<String> = self
                .tags
                .peek()
                .iter()
                .map(|tag| format!("#{tag}"))
                .collect();
            let typed = self.draft.peek().text.clone();
            let text = if tags.is_empty() {
                typed
            } else {
                format!("{} {}", typed.trim_end(), tags.join(" "))
                    .trim_start()
                    .to_owned()
            };
            match store.create_note(&text).await {
                Ok(note) => {
                    self.app.note_changed();
                    self.show(note, None, true);
                    self.incoming.set(paths);
                }
                Err(error) => tracing::error!("Could not make a note for the files: {error}"),
            }
        });
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
                        .map(|file| file.path().to_path_buf())
                        .collect(),
                );
            }
        });
    }

    // ---- The rest of the keys (7.4) -----------------------------------------

    /// Copies the note's text: Ctrl Shift C, and the menu.
    pub fn copy_note(self) {
        let text = self.draft.peek().text.clone();
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
            SyncIndicator::Problem { .. } => self.app.open_settings(SettingsTab::Sync),
            SyncIndicator::SignedOut => self.app.open_settings(SettingsTab::Account),
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

    pub fn open_settings(self) {
        self.app.open_settings(SettingsTab::Appearance);
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
                store.unsynced_note_ids().await.unwrap_or_default()
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
        listing: Signal::new(None),
        page: Signal::new(PageView {
            take_focus: true,
            ..PageView::default()
        }),
        draft: Signal::new(Draft::default()),
        last_undo: Signal::new(None),
        serial: Signal::new(0),
        page_over: Signal::new(false),
        keys_open: Signal::new(false),
        focus: Signal::new(Where::Elsewhere),
        incoming: Signal::new(Vec::new()),
        clock,
    });
    use_listing(&screen, &app);
    use_tag_offers(&screen, &app);

    // A note closed under the page (another store taken into use on
    // signing in or out) leaves a blank page.
    use_effect(move || {
        let open = *app.current_note_id.read();
        if open.is_none() && screen.page.peek().note.is_some() {
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

    rsx! {
        style { {include_str!("screen.css")} }
        div {
            class: "dk",
            "data-over": over.then_some(""),
            // Where the keyboard goes when what had it is taken away, so
            // that the screen's keys are still heard.
            tabindex: "-1",
            "data-dui-home": "",
            onkeydown: move |event| keys::on_key(&screen, &app, &event),
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
                Page {}
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
