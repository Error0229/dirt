//! Data access layer for the mobile app.

use std::path::Path;

use dirt_core::models::Note;
use dirt_core::{Error, Result, Store};

/// Thin wrapper around the shared core [`Store`] that adds the mobile
/// app's one default of its own: a note made of what came from outside the
/// app (a share, files chosen for a page with no words) is trimmed, and is
/// never empty. Which store it wraps is the session's decision.
///
/// The main screen writes what is typed through [`Self::core`], as it is
/// typed: a page keeps its spaces and line breaks, and is emptied again
/// when the person deletes its words.
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

    /// The store this wraps.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub const fn core(&self) -> &Store {
        &self.db
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

    /// Makes a note of what came from outside the app.
    pub async fn create_note(&self, content: &str) -> Result<Note> {
        let normalized = normalize_content(content)?;
        self.db.create_note(&normalized).await
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
    async fn what_comes_from_outside_is_trimmed() {
        let (_folder, store) = MobileNoteStore::open_temporary();
        let created = store.create_note("  Hello mobile  ").await.unwrap();
        assert_eq!(created.content, "Hello mobile");
        let listed = store.core().list_notes(10, 0).await.unwrap();
        assert_eq!(listed.len(), 1);
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
}
