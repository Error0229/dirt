//! Sync conflict model

use serde::{Deserialize, Serialize};

/// Recorded sync conflict resolved by strategy (e.g., LWW)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncConflict {
    /// Conflict row identifier
    pub id: i64,
    /// Note involved in the conflict
    pub note_id: String,
    /// Existing row's timestamp when conflict occurred
    pub local_updated_at: i64,
    /// Incoming row's timestamp that was rejected
    pub incoming_updated_at: i64,
    /// Resolution timestamp (unix ms)
    pub resolved_at: i64,
    /// Resolution strategy name
    pub strategy: String,
}
