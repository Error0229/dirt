use std::io;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CliError {
    #[error(transparent)]
    Core(#[from] dirt_core::Error),
    #[error(transparent)]
    LibSql(#[from] libsql::Error),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Serialization(#[from] serde_json::Error),
    #[error("No note content provided")]
    EmptyContent,
    #[error("Edited note content cannot be empty")]
    EmptyEditedContent,
    #[error("Note ID cannot be empty")]
    EmptyNoteId,
    #[error("Search query cannot be empty")]
    EmptySearchQuery,
    #[error("Note not found for id/prefix: {0}")]
    NoteNotFound(String),
    #[error("{0}")]
    AmbiguousNoteId(String),
    #[error("Editor command failed: {0}")]
    EditorFailed(String),
    #[error("Configuration error: {0}")]
    Config(String),
    #[error("Authentication error: {0}")]
    Auth(String),
    #[error("Managed sync error: {0}")]
    ManagedSync(String),
    #[error(
        "Sync is not configured. Run `dirt config init` + `dirt auth login`, or set TURSO_DATABASE_URL and TURSO_AUTH_TOKEN for advanced env mode."
    )]
    SyncNotConfigured,
}
