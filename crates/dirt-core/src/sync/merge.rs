//! Pure pull-merge resolver.
//!
//! Given the current local row, the row the server just returned in a pull
//! response, and whether the local row has pending unsynced mutations,
//! `resolve` decides whether to apply the server's row or keep the local
//! copy. No I/O, no async, no locks: the output is a single `MergeAction`
//! the caller feeds to `LibSqlNoteRepository::upsert_from_server`.
//!
//! This module implements the ten-row conflict matrix specified in the
//! design doc (cato-feature-remove-supabase-auth-design-...md). Keeping it
//! platform-agnostic means every client driver — desktop, mobile, CLI —
//! shares the exact same semantics for free, and the matrix has a clean
//! unit-test surface without spinning up a database.

use crate::models::Note;

/// What the caller should do with the remote row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeAction {
    /// Upsert the contained note into the local database. Covers insert,
    /// overwrite, hard-tombstone, and un-tombstone in one shape because the
    /// repository's `upsert_from_server` handles all four uniformly
    /// (re-runs `sync_tags` for live rows, deletes `note_tags` for
    /// tombstones).
    Apply(Note),
    /// Leave the local row untouched. Happens when the local copy is dirty
    /// (next push reconciles) or when the server's row is older than or
    /// equal to the local server-authoritative timestamp.
    Skip,
}

/// Decide what to do with a server-returned note.
///
/// `local` is the current local row for the same `id`, if any.
/// `remote` is the row from the pull response. In Phase 1 the server always
/// returns something per `id` (pull filter: `server_updated_at > cursor`),
/// so `None` is unused but accepted for forward compatibility.
/// `is_dirty` is `true` iff the local row has a matching entry in
/// `pending_sync` — i.e. there are unpushed local mutations that must not
/// be overwritten by the server copy.
///
/// Comparison is done by `server_updated_at`, never by the client-clock
/// `updated_at`. A server-stamped timestamp is the only tie-break that's
/// safe across devices whose wall clocks drift.
#[must_use]
pub fn resolve(local: Option<&Note>, remote: Option<&Note>, is_dirty: bool) -> MergeAction {
    let Some(remote) = remote else {
        // No remote row. Nothing to merge in.
        return MergeAction::Skip;
    };

    match local {
        None => {
            // Rows 1 + 2: local absent.
            if remote.is_deleted() {
                // Row 2: absent + tombstoned → no-op (nothing to tombstone).
                MergeAction::Skip
            } else {
                // Row 1: absent + live → insert.
                MergeAction::Apply(remote.clone())
            }
        }
        // Rows 7 + 10: dirty local wins regardless of server state.
        // Next push reconciles.
        Some(_) if is_dirty => MergeAction::Skip,
        Some(local) => resolve_clean(local, remote),
    }
}

fn resolve_clean(local: &Note, remote: &Note) -> MergeAction {
    if remote.is_deleted() {
        // Remote tombstoned.
        if local.is_deleted() {
            // Row 9: both tombstoned → no-op.
            MergeAction::Skip
        } else {
            // Row 6: live local + tombstoned remote → hard-tombstone locally.
            // upsert_from_server will also clear this note's note_tags rows.
            MergeAction::Apply(remote.clone())
        }
    } else if remote_is_newer(local, remote) {
        // Rows 3 + 8: remote is newer by server_updated_at.
        //   Row 3: live + live newer   → overwrite content.
        //   Row 8: tombstoned + live newer → un-tombstone.
        MergeAction::Apply(remote.clone())
    } else {
        // Rows 4 + 5: same or older server_updated_at. The server only
        // returns rows with sua > cursor, so "older" should not occur in
        // practice — treat both as no-op.
        MergeAction::Skip
    }
}

const fn remote_is_newer(local: &Note, remote: &Note) -> bool {
    match (local.server_updated_at, remote.server_updated_at) {
        (Some(local_sua), Some(remote_sua)) => remote_sua > local_sua,
        // Clean local without a server_updated_at would mean it was never
        // synced, which is impossible in practice (clean ⇒ successfully
        // pushed). Defensive fallback: treat any remote as newer so the
        // local row gets reconciled instead of silently diverging.
        (None, Some(_)) => true,
        // Remote missing a server_updated_at violates the API contract
        // (server always stamps on accept). Skip rather than overwrite.
        (_, None) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::NoteId;
    use crate::SOLO_USER_ID;

    fn note(id: &str, content: &str, sua: Option<i64>, deleted_at: Option<i64>) -> Note {
        Note {
            id: id.parse::<NoteId>().unwrap(),
            user_id: SOLO_USER_ID.to_string(),
            content: content.to_string(),
            created_at: 1_700_000_000_000,
            updated_at: 1_700_000_000_500,
            server_updated_at: sua,
            deleted_at,
        }
    }

    const ID: &str = "01932aaa-0000-7000-8000-000000000001";

    // --- Row 1: Absent local + Live remote → Apply(remote) ---
    #[test]
    fn row_1_absent_local_live_remote_applies() {
        let remote = note(ID, "hello", Some(100), None);
        let action = resolve(None, Some(&remote), false);
        assert_eq!(action, MergeAction::Apply(remote));
    }

    // --- Row 2: Absent local + Tombstoned remote → Skip ---
    #[test]
    fn row_2_absent_local_tombstoned_remote_skips() {
        let remote = note(ID, "dead", Some(100), Some(200));
        let action = resolve(None, Some(&remote), false);
        assert_eq!(action, MergeAction::Skip);
    }

    // --- Row 3: Live clean local + Live newer remote → Apply(remote) ---
    #[test]
    fn row_3_live_clean_local_live_newer_remote_applies() {
        let local = note(ID, "old", Some(100), None);
        let remote = note(ID, "new", Some(200), None);
        let action = resolve(Some(&local), Some(&remote), false);
        assert_eq!(action, MergeAction::Apply(remote));
    }

    // --- Row 4: Live clean local + Live same-sua remote → Skip ---
    #[test]
    fn row_4_live_clean_local_live_same_remote_skips() {
        let local = note(ID, "same", Some(100), None);
        let remote = note(ID, "same", Some(100), None);
        let action = resolve(Some(&local), Some(&remote), false);
        assert_eq!(action, MergeAction::Skip);
    }

    // --- Row 5: Live clean local + Live older-sua remote → Skip ---
    // "Shouldn't happen in pull" per design, but covered defensively.
    #[test]
    fn row_5_live_clean_local_live_older_remote_skips() {
        let local = note(ID, "newer", Some(200), None);
        let remote = note(ID, "older", Some(100), None);
        let action = resolve(Some(&local), Some(&remote), false);
        assert_eq!(action, MergeAction::Skip);
    }

    // --- Row 6: Live clean local + Tombstoned remote → Apply(tombstone) ---
    #[test]
    fn row_6_live_clean_local_tombstoned_remote_applies_tombstone() {
        let local = note(ID, "alive", Some(100), None);
        let remote = note(ID, "alive", Some(200), Some(210));
        let action = resolve(Some(&local), Some(&remote), false);
        assert_eq!(action, MergeAction::Apply(remote));
    }

    // --- Row 7: Live dirty local + any remote → Skip ---
    #[test]
    fn row_7_live_dirty_local_any_remote_skips() {
        let local = note(ID, "local edits", Some(100), None);
        let remote = note(ID, "server state", Some(300), None);
        let action = resolve(Some(&local), Some(&remote), true);
        assert_eq!(action, MergeAction::Skip);
    }

    // --- Row 8: Tombstoned clean local + Live newer remote → Apply (un-tombstone) ---
    #[test]
    fn row_8_tombstoned_clean_local_live_newer_remote_untombstones() {
        let local = note(ID, "was deleted", Some(100), Some(150));
        let remote = note(ID, "restored", Some(200), None);
        let action = resolve(Some(&local), Some(&remote), false);
        assert_eq!(action, MergeAction::Apply(remote));
    }

    // --- Row 9: Tombstoned clean local + Tombstoned remote → Skip ---
    #[test]
    fn row_9_tombstoned_clean_local_tombstoned_remote_skips() {
        let local = note(ID, "dead", Some(100), Some(150));
        let remote = note(ID, "dead", Some(200), Some(250));
        let action = resolve(Some(&local), Some(&remote), false);
        assert_eq!(action, MergeAction::Skip);
    }

    // --- Row 10: Tombstoned dirty local + any remote → Skip ---
    #[test]
    fn row_10_tombstoned_dirty_local_any_remote_skips() {
        let local = note(ID, "deleted locally", Some(100), Some(150));
        let live_remote = note(ID, "someone else kept", Some(500), None);
        let action = resolve(Some(&local), Some(&live_remote), true);
        assert_eq!(action, MergeAction::Skip);
    }

    // --- Edge: remote = None → Skip (forward-compat; Phase 1 pulls always
    // send a row per id) ---
    #[test]
    fn remote_none_is_skip() {
        let local = note(ID, "anything", Some(100), None);
        let action = resolve(Some(&local), None, false);
        assert_eq!(action, MergeAction::Skip);
    }

    // --- Edge: clean local with no server_updated_at (theoretically
    // impossible; defensively treat remote as authoritative) ---
    #[test]
    fn clean_local_without_sua_yields_to_remote() {
        let local = note(ID, "never synced", None, None);
        let remote = note(ID, "server stamped", Some(1), None);
        let action = resolve(Some(&local), Some(&remote), false);
        assert_eq!(action, MergeAction::Apply(remote));
    }

    // --- Edge: remote without server_updated_at (API contract violation;
    // prefer Skip over blindly overwriting) ---
    #[test]
    fn remote_without_sua_is_skip() {
        let local = note(ID, "live", Some(100), None);
        let remote = note(ID, "bogus", None, None);
        let action = resolve(Some(&local), Some(&remote), false);
        assert_eq!(action, MergeAction::Skip);
    }
}
