//! Reactive queries using dioxus-query

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use dioxus_query::prelude::*;

use dirt_core::models::Note;

use crate::services::DatabaseService;

/// Query capability for fetching all notes
#[derive(Clone)]
pub struct NotesQuery(pub Option<Arc<DatabaseService>>);

impl PartialEq for NotesQuery {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        }
    }
}

impl Eq for NotesQuery {}

impl Hash for NotesQuery {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0
            .as_ref()
            .map(|arc| Arc::as_ptr(arc) as usize)
            .hash(state);
    }
}

impl QueryCapability for NotesQuery {
    type Ok = Vec<Note>;
    type Err = String;
    type Keys = ();

    async fn run(&self, _keys: &Self::Keys) -> Result<Self::Ok, Self::Err> {
        let db = self.0.as_ref().ok_or("Database not initialized")?;
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
pub fn use_notes_query(db: Option<Arc<DatabaseService>>) -> UseQuery<NotesQuery> {
    let enabled = db.is_some();
    use_query(Query::new((), NotesQuery(db)).enable(enabled))
}
