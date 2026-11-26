use std::io;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CliError {
    #[error(transparent)]
    Core(#[from] dirt_core::Error),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Serialization(#[from] serde_json::Error),
    #[error("No note content provided")]
    EmptyContent,
    #[error("Edited note content cannot be empty")]
    EmptyEditedContent,
    #[error("Say which note: its number in the last list, its ID, or words from it")]
    EmptyNoteId,
    #[error("Search query cannot be empty")]
    EmptySearchQuery,
    /// No note is the one named. The text says why, and what to try.
    #[error("{0}")]
    NoteNotFound(String),
    /// More than one note is. The text lists them, numbered.
    #[error("{0}")]
    SeveralNotes(String),
    #[error("No note was chosen")]
    NothingChosen,
    #[error("Editor command failed: {0}")]
    EditorFailed(String),
    #[error("Configuration error: {0}")]
    Config(String),
    /// Something about a note's files. The text already says what the
    /// person can do about it.
    #[error("{0}")]
    Files(String),
    /// Signing in, signing out or syncing failed. The text already says
    /// what the person can do about it.
    #[error("{0}")]
    Account(String),
}
