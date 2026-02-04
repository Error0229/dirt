//! Error types for dirt-core

use thiserror::Error;

/// Result type alias using dirt-core's Error
pub type Result<T> = std::result::Result<T, Error>;

/// Errors that can occur in dirt-core operations
#[derive(Error, Debug)]
pub enum Error {
    /// Database error
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    /// Note not found
    #[error("Note not found: {0}")]
    NotFound(String),

    /// Invalid input
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}
