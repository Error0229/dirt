//! Shared cross-platform state types.

/// Unified sync state used by desktop and mobile clients.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncState {
    Offline,
    Syncing,
    Synced,
    Error,
}
