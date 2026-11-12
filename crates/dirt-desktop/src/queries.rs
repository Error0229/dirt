//! Reactive queries using dioxus-query
//!
//! The query for the note list names the store it reads by the store's
//! file, and takes the store itself from [`set_notes_store`] when it runs.
//! It must not hold the store: the query library keeps a query for a while
//! after its last use, and a held store keeps its files open. After a
//! sign-out those files have to be removed, which an open file prevents.

use std::path::PathBuf;
use std::sync::{Arc, PoisonError, RwLock};

use dioxus_query::prelude::*;

use dirt_core::models::Note;
use dirt_core::Store;

/// The store the app reads notes from now.
static NOTES_STORE: RwLock<Option<Arc<Store>>> = RwLock::new(None);

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
        db.list_notes(100, 0).await.map_err(|e| e.to_string())
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
