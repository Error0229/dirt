//! Error types for dirt-core

use thiserror::Error;

/// Result type alias using dirt-core's Error
pub type Result<T> = std::result::Result<T, Error>;

/// Errors that can occur in dirt-core operations
#[derive(Error, Debug)]
pub enum Error {
    /// Database error
    #[error("Database error: {0}")]
    Database(String),

    /// An error from SQLite
    #[error("Database error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    /// A note document that cannot be read or merged
    #[error(transparent)]
    Document(#[from] crate::notes::DocError),

    /// The server sent a version of a note older than one already seen
    #[error("the server sent change {got} of note {note} after change {seen}")]
    Replay {
        /// The note concerned
        note: String,
        /// The highest change number already seen for it
        seen: i64,
        /// The lower change number that arrived
        got: i64,
    },

    /// The store's threads have stopped, so nothing more can be asked of it
    #[error("The note store is closed")]
    StoreClosed,

    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Note not found
    #[error("Note not found: {0}")]
    NotFound(String),

    /// Invalid input
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Media/object storage error
    #[error("Storage error: {0}")]
    Storage(String),
}
