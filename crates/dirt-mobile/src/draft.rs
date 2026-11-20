//! What one page of the main screen holds (interface design, sections 6.1
//! and 8.7): what was typed last, what the store has of it, and whether
//! its note has been made.
//!
//! The page's typing writes here and its leaving reads it, so that a page
//! left and a page opened straight after never share one. Everything that
//! can make the page's note (the first character typed, files chosen on a
//! page with no note) asks
//! [`Draft::claim_create`] or goes through [`Draft::typed`], so that one
//! page makes one note.
//!
//! A write the store could not make is not tried again by itself: it is a
//! failure of the disk, and the person is told (design 6.7's error). A
//! create that failed leaves the page as if its note had never been made,
//! so that the next key makes it with everything typed by then. Leaving
//! the page does not: a note is never made later from a failure.

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
#[derive(Default, Clone)]
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

    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub const fn version(&self) -> u64 {
        self.version
    }

    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
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
    /// files chosen on a page with no note. True if it is the caller's to make: no note is made or
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

    /// What to write next: the note, the text the page saved before, which
    /// what it holds now was typed over, and that text, if the store does
    /// not have it yet. The store makes the changes from the one to the
    /// other to what the note holds now (`Store::update_note_from`), so that
    /// what came meanwhile, the words of what was said or another device's
    /// edit, is not saved over (design 8.9).
    pub fn to_save(&self) -> Option<(NoteId, String, String)> {
        match self.note {
            Some(note) if self.text != self.saved => {
                Some((note, self.saved.clone(), self.text.clone()))
            }
            _ => None,
        }
    }

    /// The store's text of the page's note is `held`, which has what the
    /// page saved and something the page did not write: the words of what
    /// was said. The page's changes since it last saved are made to the
    /// store's text, which is the page's base from now on. Returns the
    /// page's new text when it changed.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub fn took(&mut self, note: NoteId, held: &str) -> Option<String> {
        if self.note != Some(note) || held == self.saved {
            return None;
        }
        let merged = dirt_core::notes::merge3(&self.saved, &self.text, held);
        held.clone_into(&mut self.saved);
        self.version += 1;
        let changed = merged != self.text;
        self.text = merged;
        changed.then(|| self.text.clone())
    }

    /// The page's field holds `text`, which the page had not been told of:
    /// what [`Self::took`] gave it, with whatever was typed meanwhile.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub fn holds(&mut self, text: String) {
        self.version += 1;
        self.text = text;
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
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
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
        assert_eq!(
            draft.to_save(),
            Some((note, "  b".to_owned(), "  bu".to_owned()))
        );
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
        assert_eq!(
            draft.to_save(),
            Some((note, String::new(), "milk".to_owned()))
        );
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
    fn a_page_left_takes_no_more_typing() {
        let mut draft = Draft::blank(None);
        assert_eq!(draft.typed("b"), Typed::Create("b".to_owned()));
        draft.create_failed();
        draft.leave();
        assert_eq!(draft.typed("bread"), Typed::Ignored);
    }

    #[test]
    fn a_save_is_written_one_at_a_time_and_only_what_is_new() {
        let note = NoteId::new();
        let mut draft = Draft::of_note(None, note, "old");
        assert_eq!(draft.to_save(), None);
        draft.typed("new");
        assert!(draft.begin_save());
        assert!(!draft.begin_save());
        assert_eq!(
            draft.to_save(),
            Some((note, "old".to_owned(), "new".to_owned()))
        );
        draft.saved("new");
        assert_eq!(draft.to_save(), None);
        draft.end_save();
        assert!(draft.begin_save());
    }

    /// The words of a thought said arrive at the start of the note while
    /// its page is open and being typed on: the page keeps what was typed
    /// and gains the words, and the next save goes from the store's text.
    #[test]
    fn words_that_came_meanwhile_are_taken_into_the_page() {
        let note = NoteId::new();
        let mut draft = Draft::of_note(None, note, "");
        draft.typed("milk");
        draft.saved("milk");
        assert_eq!(
            draft.took(note, "milk"),
            None,
            "the store has what the page saved"
        );
        draft.typed("milk and eggs");
        assert_eq!(
            draft.took(note, "Buy bread\n\nmilk"),
            Some("Buy bread\n\nmilk and eggs".to_owned())
        );
        assert_eq!(
            draft.to_save(),
            Some((
                note,
                "Buy bread\n\nmilk".to_owned(),
                "Buy bread\n\nmilk and eggs".to_owned()
            ))
        );
        assert_eq!(draft.took(NoteId::new(), "other"), None);
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
