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
    /// Pre-formatted auth-subsystem error string assembled by
    /// `auth_cmd::auth_error_to_cli` / `token_store_error_to_cli`.
    /// We collapse `AuthError` and `TokenStoreError` into a single
    /// `String` (rather than carrying the source via `#[from]`) so
    /// the CLI can present a `(cause); (fix)` line tuned per
    /// variant. The tradeoff is that `Error::source()` returns
    /// `None` here — a future `--verbose` mode that wants the full
    /// chain would need to plumb the structured source through.
    #[error("auth error: {0}")]
    Auth(String),
    #[error(
        "Sync via the new dirt-api backend is not yet wired into the CLI. \
         Run `dirt config init --api-base-url <URL>` and watch for the \
         follow-up commit that adds the ApiClient-driven sync worker."
    )]
    SyncNotConfigured,
}
