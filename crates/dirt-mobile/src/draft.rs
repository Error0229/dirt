//! What one page of the main screen holds (interface design, sections 6.1
//! and 8.7): what was typed last, what the store has of it, and whether
//! its note has been made.
//!
//! The page's typing writes here and its leaving reads it, so that a page
//! left and a page opened straight after never share one. Everything that
//! can make the page's note (the first character typed, files chosen on a
//! page with no note, the page left before its note was made) asks
//! [`Draft::claim_create`] or goes through [`Draft::typed`], so that one
//! page makes one note.
//!
//! A write the store could not make is not tried again by itself: it is a
//! failure of the disk, and the person is told (the screen's problem
//! toast). A create that failed leaves the page as if its note had never
//! been made, so that the next key, or leaving the page, makes it with
//! everything typed by then.

use std::sync::Arc;

use dirt_core::models::NoteId;

use crate::data::MobileNoteStore;

/// What a change of the page's text asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Typed {
    /// Nothing: the page has been let go.
    Ignored,
    /// Make the note, of this text: it is the first that is not spaces
    /// alone, and no note is made or being made.
    Create(String),
    /// Save the text once typing pauses; this is the change's number, and
    /// a save is written only if no change came after it.
    Save(u64),
}

/// One page's draft.
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
    /// The note is being made, and the store has not answered yet.
    creating: bool,
    /// A save is being written; another waits for it.
    saving: bool,
    /// The page has been let go: nothing more is typed into it.
    left: bool,
}

impl Draft {
    /// A new page, with no note yet.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub fn blank(store: Option<Arc<MobileNoteStore>>) -> Self {
        Self {
            store,
            ..Self::default()
        }
    }

    /// A page opened on a note that holds `text`.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub fn of_note(store: Option<Arc<MobileNoteStore>>, note: NoteId, text: &str) -> Self {
        Self {
            store,
            note: Some(note),
            text: text.to_owned(),
            saved: text.to_owned(),
            ..Self::default()
        }
    }

    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub fn store(&self) -> Option<Arc<MobileNoteStore>> {
        self.store.clone()
    }

    pub const fn note(&self) -> Option<NoteId> {
        self.note
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub const fn version(&self) -> u64 {
        self.version
    }

    pub const fn is_creating(&self) -> bool {
        self.creating
    }

    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub const fn is_saving(&self) -> bool {
        self.saving
    }

    /// The page's text changed.
    pub fn typed(&mut self, text: &str) -> Typed {
        if self.left {
            return Typed::Ignored;
        }
        text.clone_into(&mut self.text);
        self.version += 1;
        if self.note.is_none() && !self.creating && !text.trim().is_empty() {
            self.creating = true;
            return Typed::Create(text.to_owned());
        }
        Typed::Save(self.version)
    }

    /// Asks to make the page's note from somewhere other than its typing:
    /// files chosen on a page with no note, or a page left before its note
    /// was made. True if it is the caller's to make: no note is made or
    /// being made, and the draft now says one is being made, so that a key
    /// typed meanwhile does not make another.
    pub const fn claim_create(&mut self) -> bool {
        if self.note.is_some() || self.creating {
            return false;
        }
        self.creating = true;
        true
    }

    /// The note was made, holding `written`.
    pub fn created(&mut self, note: NoteId, written: &str) {
        self.note = Some(note);
        written.clone_into(&mut self.saved);
        self.creating = false;
    }

    /// The note could not be made. The page is as it was before its first
    /// character: the next key makes the note, with everything typed by
    /// then.
    pub const fn create_failed(&mut self) {
        self.creating = false;
    }

    /// The text of a page whose note was never made, if there is any that
    /// is not spaces alone: what leaving the page must make a note of.
    pub fn unmade(&self) -> Option<String> {
        (self.note.is_none() && !self.creating && !self.text.trim().is_empty())
            .then(|| self.text.clone())
    }

    /// The page's note was read from the store as it opened, and holds
    /// `text`. If nothing has been typed yet, that is what the page holds
    /// too; returns whether the page's text changed.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub fn read_back(&mut self, text: &str) -> bool {
        if self.version != 0 {
            return false;
        }
        text.clone_into(&mut self.saved);
        if self.text == text {
            return false;
        }
        text.clone_into(&mut self.text);
        true
    }

    /// Starts writing what the store does not have yet. False if a save is
    /// being written already: that one writes what is typed by then.
    pub const fn begin_save(&mut self) -> bool {
        if self.saving {
            return false;
        }
        self.saving = true;
        true
    }

    /// What to write next: the note and its text, if the store does not
    /// have the text yet.
    pub fn to_save(&self) -> Option<(NoteId, String)> {
        match self.note {
            Some(note) if self.text != self.saved => Some((note, self.text.clone())),
            _ => None,
        }
    }

    /// The store has `text`.
    pub fn saved(&mut self, text: &str) {
        text.clone_into(&mut self.saved);
    }

    pub const fn end_save(&mut self) {
        self.saving = false;
    }

    /// The page has been let go.
    pub const fn leave(&mut self) {
        self.left = true;
    }

    /// The page was left with no words: its note, if it has one, is to be
    /// let go.
    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_character_makes_the_note_once() {
        let mut draft = Draft::blank(None);
        assert_eq!(draft.typed("  "), Typed::Save(1));
        assert_eq!(draft.typed("  b"), Typed::Create("  b".to_owned()));
        // Typed while the note is being made: saved once it is, never made
        // a second time.
        assert_eq!(draft.typed("  bu"), Typed::Save(3));
        let note = NoteId::new();
        draft.created(note, "  b");
        assert_eq!(draft.to_save(), Some((note, "  bu".to_owned())));
    }

    #[test]
    fn files_chosen_and_a_key_typed_at_once_make_one_note() {
        let mut draft = Draft::blank(None);
        // Files chosen on a page with nothing written: theirs to make.
        assert!(draft.claim_create());
        // A key typed while that note is being made does not make another.
        assert_eq!(draft.typed("milk"), Typed::Save(1));
        assert!(!draft.claim_create());
        let note = NoteId::new();
        draft.created(note, "");
        assert_eq!(draft.to_save(), Some((note, "milk".to_owned())));
        // Once there is a note, nothing makes another.
        assert!(!draft.claim_create());
        assert_eq!(draft.typed("milk and eggs"), Typed::Save(2));
    }

    #[test]
    fn the_first_character_claims_the_note_before_files_can() {
        let mut draft = Draft::blank(None);
        assert_eq!(draft.typed("b"), Typed::Create("b".to_owned()));
        assert!(!draft.claim_create(), "files wait for the note being made");
    }

    #[test]
    fn a_note_that_could_not_be_made_is_made_by_the_next_key_with_all_of_it() {
        let mut draft = Draft::blank(None);
        assert_eq!(draft.typed("b"), Typed::Create("b".to_owned()));
        assert_eq!(draft.typed("bread"), Typed::Save(2));
        draft.create_failed();
        assert_eq!(draft.note(), None);
        assert_eq!(draft.typed("bread!"), Typed::Create("bread!".to_owned()));
    }

    #[test]
    fn a_page_left_before_its_note_was_made_says_what_to_make() {
        let mut draft = Draft::blank(None);
        assert_eq!(draft.unmade(), None);
        assert_eq!(draft.typed("b"), Typed::Create("b".to_owned()));
        assert_eq!(draft.unmade(), None, "it is being made");
        draft.typed("bread");
        draft.create_failed();
        draft.leave();
        assert_eq!(draft.unmade(), Some("bread".to_owned()));
        assert_eq!(draft.typed("bread and"), Typed::Ignored);
        assert!(draft.claim_create());
    }

    #[test]
    fn a_save_is_written_one_at_a_time_and_only_what_is_new() {
        let note = NoteId::new();
        let mut draft = Draft::of_note(None, note, "old");
        assert_eq!(draft.to_save(), None);
        draft.typed("new");
        assert!(draft.begin_save());
        assert!(!draft.begin_save());
        assert_eq!(draft.to_save(), Some((note, "new".to_owned())));
        draft.saved("new");
        assert_eq!(draft.to_save(), None);
        draft.end_save();
        assert!(draft.begin_save());
    }

    #[test]
    fn what_the_store_holds_replaces_the_page_only_while_nothing_is_typed() {
        let note = NoteId::new();
        let mut draft = Draft::of_note(None, note, "from the list");
        assert!(draft.read_back("from the store"));
        assert_eq!(draft.text(), "from the store");
        draft.typed("from the store, and more");
        assert!(!draft.read_back("from elsewhere"));
        assert_eq!(draft.text(), "from the store, and more");
    }
}
