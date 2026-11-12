//! Data access layer for the mobile app.

use std::path::Path;

use dirt_core::models::{Note, NoteId};
use dirt_core::{Error, Result, Store};

/// How many notes the list shows at first, and how many a search may
/// find. Each request for more adds as many again.
pub const NOTES_PAGE: usize = 100;
const EXPORT_NOTES_PAGE_SIZE: usize = 500;

/// Thin wrapper around the shared core [`Store`] that adds the mobile
/// app's defaults: how many notes the list shows, and trimming of note
/// content. Which store it wraps is the session's decision.
#[derive(Clone)]
pub struct MobileNoteStore {
    db: Store,
}

impl MobileNoteStore {
    /// Wraps the store the session says is in use.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub const fn new(db: Store) -> Self {
        Self { db }
    }

    /// The file of the store this wraps, which tells two stores apart.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub fn path(&self) -> &Path {
        self.db.path()
    }

    /// Open a store on a file inside a new temporary folder, for tests.
    /// The folder is removed when the returned guard is dropped, so the
    /// caller keeps the guard for as long as it uses the store.
    #[cfg(test)]
    pub fn open_temporary() -> (tempfile::TempDir, Self) {
        let folder = tempfile::tempdir().unwrap();
        let db = Store::open(folder.path().join("notes.db")).unwrap();
        (folder, Self { db })
    }

    /// The `limit` most recently edited notes, the latest first.
    pub async fn list_notes(&self, limit: usize) -> Result<Vec<Note>> {
        self.db.list_notes(limit, 0).await
    }

    /// Up to `limit` notes that contain every word of `query` and carry
    /// `tag`, either of which may be left out. The store looks through
    /// every note, not only the ones the list has loaded.
    pub async fn find_notes(
        &self,
        query: &str,
        tag: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Note>> {
        self.db.find_notes(query, tag, limit).await
    }

    /// Every tag in use, with how many notes carry it, most used first.
    pub async fn list_tags(&self) -> Result<Vec<(String, usize)>> {
        let mut tags = self.db.list_tags().await?;
        tags.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        Ok(tags)
    }

    /// List all notes for full export operations.
    pub async fn list_all_notes(&self) -> Result<Vec<Note>> {
        let mut notes = Vec::new();
        let mut offset = 0usize;

        loop {
            let batch = self.db.list_notes(EXPORT_NOTES_PAGE_SIZE, offset).await?;
            let count = batch.len();
            notes.extend(batch);

            if count < EXPORT_NOTES_PAGE_SIZE {
                break;
            }
            offset += count;
        }

        Ok(notes)
    }

    /// Create a note.
    pub async fn create_note(&self, content: &str) -> Result<Note> {
        let normalized = normalize_content(content)?;
        self.db.create_note(&normalized).await
    }

    /// Update an existing note.
    pub async fn update_note(&self, id: &NoteId, content: &str) -> Result<Note> {
        let normalized = normalize_content(content)?;
        self.db.update_note(id, &normalized).await
    }

    /// Delete a note.
    pub async fn delete_note(&self, id: &NoteId) -> Result<()> {
        self.db.delete_note(id).await
    }
}

fn normalize_content(content: &str) -> Result<String> {
    let normalized = content.trim();
    if normalized.is_empty() {
        return Err(Error::InvalidInput(
            "Note content cannot be empty".to_string(),
        ));
    }
    Ok(normalized.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn create_update_delete_roundtrip() {
        let (_folder, store) = MobileNoteStore::open_temporary();

        let created = store.create_note("  Hello mobile  ").await.unwrap();
        assert_eq!(created.content, "Hello mobile");

        let updated = store
            .update_note(&created.id, "Updated #mobile")
            .await
            .unwrap();
        assert_eq!(updated.content, "Updated #mobile");
        assert_eq!(updated.id, created.id);

        store.delete_note(&updated.id).await.unwrap();
        let notes = store.list_notes(NOTES_PAGE).await.unwrap();
        assert!(notes.is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn create_rejects_empty_content() {
        let (_folder, store) = MobileNoteStore::open_temporary();
        let err = store.create_note("   ").await.unwrap_err();

        match err {
            Error::InvalidInput(msg) => assert!(msg.contains("cannot be empty")),
            other => panic!("expected invalid input error, got {other:?}"),
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn finding_reaches_notes_the_list_has_not_loaded() {
        let (_folder, store) = MobileNoteStore::open_temporary();
        let oldest = store.create_note("call the plumber #home").await.unwrap();
        for number in 0..NOTES_PAGE {
            store
                .create_note(&format!("filler {number} #work"))
                .await
                .unwrap();
        }
        let listed = store.list_notes(NOTES_PAGE).await.unwrap();
        assert_eq!(listed.len(), NOTES_PAGE);
        assert!(listed.iter().all(|note| note.id != oldest.id));
        // Asked for more, the list reaches it.
        let longer = store.list_notes(2 * NOTES_PAGE).await.unwrap();
        assert_eq!(longer.len(), NOTES_PAGE + 1);
        assert_eq!(longer.last().unwrap().id, oldest.id);

        let by_word = store.find_notes("plumb", None, NOTES_PAGE).await.unwrap();
        assert_eq!(by_word.len(), 1);
        assert_eq!(by_word[0].id, oldest.id);
        let by_tag = store
            .find_notes("", Some("home"), NOTES_PAGE)
            .await
            .unwrap();
        assert_eq!(by_tag.len(), 1);
        assert!(store
            .find_notes("plumb", Some("work"), NOTES_PAGE)
            .await
            .unwrap()
            .is_empty());
        // A search finds no more than it is allowed, and more when allowed.
        let some = store.find_notes("filler", None, 10).await.unwrap();
        assert_eq!(some.len(), 10);
        let all = store
            .find_notes("filler", None, 2 * NOTES_PAGE)
            .await
            .unwrap();
        assert_eq!(all.len(), NOTES_PAGE);

        // Most used first, then by name.
        assert_eq!(
            store.list_tags().await.unwrap(),
            [("work".to_owned(), NOTES_PAGE), ("home".to_owned(), 1)]
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn list_all_notes_returns_full_collection() {
        let (_folder, store) = MobileNoteStore::open_temporary();
        store.create_note("One").await.unwrap();
        store.create_note("Two").await.unwrap();
        store.create_note("Three").await.unwrap();

        let notes = store.list_all_notes().await.unwrap();
        assert_eq!(notes.len(), 3);
    }
}
