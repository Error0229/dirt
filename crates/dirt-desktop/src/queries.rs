//! Reactive queries using dioxus-query
//!
//! The query for the note list names the store it reads by the store's
//! file, and takes the store itself from [`set_notes_store`] when it runs.
//! It must not hold the store: the query library keeps a query for a while
//! after its last use, and a held store keeps its files open. After a
//! sign-out those files have to be removed, which an open file prevents.
//!
//! The list holds the most recently edited notes, and the note that is
//! open, whichever it is. A note found by searching may be far older than
//! the list reaches; while it is open it has to stay in what the app
//! holds, or a refresh of the list would close it under the person.

use std::path::PathBuf;
use std::sync::{Arc, PoisonError, RwLock};

use dioxus_query::prelude::*;

use dirt_core::models::{Note, NoteId};
use dirt_core::Store;

/// How many notes the list holds, most recently edited first.
pub const LISTED_NOTES: usize = 100;

/// The store the app reads notes from now.
static NOTES_STORE: RwLock<Option<Arc<Store>>> = RwLock::new(None);

/// The note that is open in the editor, if any.
static OPEN_NOTE: RwLock<Option<NoteId>> = RwLock::new(None);

/// Says which note is open, so that a refresh of the list keeps it.
pub fn set_open_note(note: Option<NoteId>) {
    *OPEN_NOTE.write().unwrap_or_else(PoisonError::into_inner) = note;
}

fn open_note() -> Option<NoteId> {
    *OPEN_NOTE.read().unwrap_or_else(PoisonError::into_inner)
}

/// Says which store the note list is read from. Called when the app opens
/// its store and whenever signing in or out changes it. The store set
/// before is let go of.
pub fn set_notes_store(store: Option<Arc<Store>>) {
    *NOTES_STORE.write().unwrap_or_else(PoisonError::into_inner) = store;
}

fn notes_store() -> Option<Arc<Store>> {
    NOTES_STORE
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

/// Query capability for fetching all notes. It is told apart by the file
/// of the store it reads, so a change of store is a new query.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct NotesQuery(pub Option<PathBuf>);

impl QueryCapability for NotesQuery {
    type Ok = Vec<Note>;
    type Err = String;
    type Keys = ();

    async fn run(&self, _keys: &Self::Keys) -> Result<Self::Ok, Self::Err> {
        let path = self.0.as_ref().ok_or("Database not initialized")?;
        // A query left over from the store before a sign-in or sign-out
        // has nothing to read.
        let db = notes_store()
            .filter(|store| store.path() == path.as_path())
            .ok_or("The note store has changed")?;
        tracing::debug!("NotesQuery: fetching notes from database");
        let mut notes = db
            .list_notes(LISTED_NOTES, 0)
            .await
            .map_err(|e| e.to_string())?;
        // The open note stays, however old it is.
        if let Some(open) = open_note() {
            if !notes.iter().any(|note| note.id == open) {
                if let Some(note) = db.get_note(&open).await.map_err(|e| e.to_string())? {
                    notes.push(note);
                }
            }
        }
        Ok(notes)
    }
}

/// Invalidate the notes query (call after creating/updating/deleting notes)
pub async fn invalidate_notes_query() {
    tracing::debug!("Invalidating notes query");
    QueriesStorage::<NotesQuery>::invalidate_matching(()).await;
}

/// Hook to use the notes query (always call unconditionally - uses enable flag)
pub fn use_notes_query(db: Option<Arc<Store>>) -> UseQuery<NotesQuery> {
    let enabled = db.is_some();
    let path = db.map(|store| store.path().to_path_buf());
    use_query(Query::new((), NotesQuery(path)).enable(enabled))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One test, because what it sets is shared by the whole program.
    #[tokio::test(flavor = "multi_thread")]
    async fn the_list_is_the_most_recent_notes_and_the_open_note_however_old() {
        let folder = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(folder.path().join("notes.db")).unwrap());
        let oldest = store.create_note("the oldest note").await.unwrap();
        for number in 0..LISTED_NOTES + 4 {
            store
                .create_note(&format!("filler {number}"))
                .await
                .unwrap();
        }
        let query = NotesQuery(Some(store.path().to_path_buf()));
        set_notes_store(Some(Arc::clone(&store)));

        set_open_note(None);
        let listed = query.run(&()).await.unwrap();
        assert_eq!(listed.len(), LISTED_NOTES);
        assert!(listed.iter().all(|note| note.id != oldest.id));

        // With the old note open, a refresh keeps it: without it the
        // editor would close under the person.
        set_open_note(Some(oldest.id));
        let listed = query.run(&()).await.unwrap();
        assert_eq!(listed.len(), LISTED_NOTES + 1);
        assert!(listed.iter().any(|note| note.id == oldest.id));

        // An open note that is among the most recent is not listed twice.
        set_open_note(Some(listed[0].id));
        assert_eq!(query.run(&()).await.unwrap().len(), LISTED_NOTES);

        // A note that was deleted while open is not brought back.
        store.delete_note(&oldest.id).await.unwrap();
        set_open_note(Some(oldest.id));
        assert_eq!(query.run(&()).await.unwrap().len(), LISTED_NOTES);

        // A query left over from another store reads nothing from this one.
        let other = NotesQuery(Some(folder.path().join("another.db")));
        assert!(other.run(&()).await.is_err());

        set_open_note(None);
        set_notes_store(None);
        assert!(query.run(&()).await.is_err());
    }
}
