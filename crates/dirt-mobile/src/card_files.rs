//! What the cards of the list show of their notes' files, read once for
//! each change of a note and kept (interface design, section 6.2).
//!
//! The list asks the store which of the notes it shows have files, with
//! each note's revision (`Store::file_revisions`), and reads a note's
//! files only when its revision is not the one already read. A picture's
//! thumbnail, a small file of its own, is kept as the card shows it until
//! no card shows it. Nothing is kept for a note the list no longer shows,
//! and everything goes when another store is taken into use.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use dirt_core::models::NoteId;
use uuid::Uuid;

/// What a card shows of its note's files.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CardFiles {
    /// The first file's name, which names a note with no words.
    pub first_name: Option<String>,
    /// The thumbnail of the first file that has one.
    pub thumbnail: Option<Uuid>,
}

/// The files of the notes the list shows, as last read.
#[derive(Debug, Default)]
pub struct CardFilesCache {
    /// The store they were read from.
    store: Option<PathBuf>,
    by_note: HashMap<NoteId, (i64, CardFiles)>,
    /// Thumbnails as a card shows them, by the thumbnail's file ID.
    thumbs: HashMap<Uuid, String>,
}

impl CardFilesCache {
    /// Before a reading of the list from `store`: all is forgotten if it is
    /// another store than the last, an account's after the device's own.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub fn reading_from(&mut self, store: &Path) {
        if self.store.as_deref() != Some(store) {
            *self = Self {
                store: Some(store.to_path_buf()),
                ..Self::default()
            };
        }
    }

    /// Of the notes with files the list shows, with their revisions now,
    /// those whose files have to be read: new to the list, or changed.
    pub fn to_read(&self, revisions: &HashMap<NoteId, i64>) -> Vec<NoteId> {
        revisions
            .iter()
            .filter(|(note, revision)| {
                self.by_note
                    .get(note)
                    .is_none_or(|(known, _)| known != *revision)
            })
            .map(|(note, _)| *note)
            .collect()
    }

    /// A note's files, as read at `revision`.
    pub fn read(&mut self, note: NoteId, revision: i64, files: CardFiles) {
        self.by_note.insert(note, (revision, files));
    }

    /// What a card shows of `note`'s files, if it has any.
    pub fn files(&self, note: &NoteId) -> Option<&CardFiles> {
        self.by_note.get(note).map(|(_, files)| files)
    }

    /// A thumbnail already read.
    pub fn thumb(&self, thumbnail: &Uuid) -> Option<&String> {
        self.thumbs.get(thumbnail)
    }

    /// A thumbnail, as a card shows it.
    pub fn keep_thumb(&mut self, thumbnail: Uuid, shown: String) {
        self.thumbs.insert(thumbnail, shown);
    }

    /// After a reading: only what the notes with files that the list shows
    /// need is kept, so that what is held is bounded by the list.
    pub fn keep_only(&mut self, revisions: &HashMap<NoteId, i64>) {
        self.by_note.retain(|note, _| revisions.contains_key(note));
        let wanted: Vec<Uuid> = self
            .by_note
            .values()
            .filter_map(|(_, files)| files.thumbnail)
            .collect();
        self.thumbs
            .retain(|thumbnail, _| wanted.contains(thumbnail));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_notes_files_are_read_again_only_when_it_changed() {
        let mut cache = CardFilesCache::default();
        cache.reading_from(Path::new("account"));
        let (one, two) = (NoteId::new(), NoteId::new());
        let mut revisions = HashMap::from([(one, 3), (two, 7)]);
        let mut wanted = cache.to_read(&revisions);
        wanted.sort();
        let mut both = vec![one, two];
        both.sort();
        assert_eq!(wanted, both);
        cache.read(one, 3, CardFiles::default());
        cache.read(two, 7, CardFiles::default());
        assert!(cache.to_read(&revisions).is_empty());
        revisions.insert(two, 8);
        assert_eq!(cache.to_read(&revisions), vec![two]);
    }

    #[test]
    fn only_what_the_list_shows_is_kept_and_another_store_forgets_it_all() {
        let mut cache = CardFilesCache::default();
        cache.reading_from(Path::new("device"));
        let (shown, gone) = (NoteId::new(), NoteId::new());
        let (kept_thumb, dropped_thumb) = (Uuid::now_v7(), Uuid::now_v7());
        cache.read(
            shown,
            1,
            CardFiles {
                first_name: None,
                thumbnail: Some(kept_thumb),
            },
        );
        cache.read(
            gone,
            1,
            CardFiles {
                first_name: None,
                thumbnail: Some(dropped_thumb),
            },
        );
        cache.keep_thumb(kept_thumb, "data:a".to_owned());
        cache.keep_thumb(dropped_thumb, "data:b".to_owned());

        cache.keep_only(&HashMap::from([(shown, 1)]));
        assert!(cache.files(&shown).is_some());
        assert!(cache.files(&gone).is_none());
        assert!(cache.thumb(&kept_thumb).is_some());
        assert!(cache.thumb(&dropped_thumb).is_none());

        // The same store keeps what it has; another forgets it.
        cache.reading_from(Path::new("device"));
        assert!(cache.files(&shown).is_some());
        cache.reading_from(Path::new("account"));
        assert!(cache.files(&shown).is_none());
        assert!(cache.thumb(&kept_thumb).is_none());
    }
}
