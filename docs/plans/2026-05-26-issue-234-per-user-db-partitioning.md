---
title: "Issue #234: Per-user DB partitioning + SOLO_USER_ID cutover"
type: security
date: 2026-05-26
issue: 234
---

# Issue #234: Per-user DB partitioning + SOLO_USER_ID cutover

## Problem

All three clients (CLI / desktop / mobile) open `dirt.db` at a single
shared path under `dirs::data_dir()/dirt/`. The local DB pre-dates
magic-link auth and has no concept of "which user owns this data."
Combined with the hardcoded `SOLO_USER_ID` used everywhere the sync
engine is constructed, a sign-out / sign-in across two accounts on the
same machine leaks notes from A into B's sync push and surfaces A's
local rows in B's UI. See issue #234 for the full sequence.

## Goal

1. Partition the local SQLite database by authenticated `user_id`, so
   two accounts on the same machine see strictly separate stores.
2. Replace the hardcoded `SOLO_USER_ID` in every `SyncEngine::new` call
   site with the active user's `user_id`, so cursors and
   `pending_sync` rows are correctly scoped after partitioning.
3. Preserve Phase-1 capture history by migrating the existing
   `dirt.db` into the first authenticated user's directory on first
   sign-in post-upgrade, rewriting every row's `user_id` from
   `SOLO_USER_ID` to the new user.
4. Keep using the same DB after sign-out: sign-out clears the bearer
   but the local DB stays bound to the "active user." Sync just gets
   parked until the next sign-in.

Out of scope: server-side schema changes, multi-account-at-once UX in
any one process, removing `SOLO_USER_ID` from migration defaults (the
column DEFAULT and the truly-never-signed-in fresh-machine case still
use it).

## Active-user pointer model

The keyring session token clears on sign-out, but the local DB
allegiance does NOT. We add a small persistent state file that records
which user owns the local DB regardless of whether a bearer is
currently present:

```
<data_dir>/dirt/state.json    # {"active_user_id": "<uuid>"}
```

**Resolution order** (every binary, every operation):

1. If `state.json` is present and parses → use
   `<data_dir>/dirt/<active_user_id>/dirt.db`. New notes stamped
   with `active_user_id`.
2. Else → legacy `<data_dir>/dirt/dirt.db`, `SOLO_USER_ID`. (Only
   reachable on a truly fresh machine that has never signed in.)

**State transitions:**

| Event | `state.json` action | DB path |
| --- | --- | --- |
| Truly fresh launch (never signed in) | absent | `<data_dir>/dirt/dirt.db`, SOLO |
| First sign-in as A | create with `active_user_id = A` | migrate legacy `dirt.db` → `<A>/dirt.db` with row rewrite |
| Sign-out (A still pointer) | unchanged | still `<A>/dirt.db`, stamped A; sync parked |
| Sign back in as A | unchanged | `<A>/dirt.db`, sync resumes; pending rows flush |
| Sign in as B (was A) | rewrite to `active_user_id = B` | open/create `<B>/dirt.db`; `<A>/dirt.db` untouched on disk for A's next sign-in |
| Sign-out then offline `dirt add` | unchanged | writes to last-active user's DB, stamped that user_id, queued in `pending_sync` |

This means there's no "orphan solo offline" problem after the very
first sign-in — every subsequent local write is bound to a real user.

## Cross-client mismatch guard

The keyring slot `(dev.dirt.session, default)` is shared across
CLI / desktop / mobile by design. If the CLI logs in as B while the
desktop is running with `state.json.active_user_id = A`, the desktop's
next sync cycle would otherwise push A's pending rows under B's
bearer — the original leak.

**Guard:** at the top of every `SyncEngine::run_once` cycle, compare
`stored_token.user_id` against `db.user_id()`. Mismatch is a hard
error:

```
SyncEngineError::ScopeMismatch { db_user: String, session_user: String }
```

The worker surfaces this as `SyncStatus::Error` with a clear message:
"Session user differs from local user — restart the app (or run
`dirt auth login` again) to switch accounts." It does NOT auto-rotate
the local DB mid-process; that's a bigger UX change than this fix.

`dirt-cli` shows the same error on `dirt sync`. The next CLI
invocation (which reopens everything from scratch) will read the new
keyring, update `state.json`, and open the new user's DB.

## Storage layout (after this change)

```
<data_dir>/dirt/
├── state.json                    # {"active_user_id": "<uuid>"} or absent
├── dirt.db                       # legacy SOLO DB, ONLY pre-first-signin
├── <user_id_A>/
│   └── dirt.db                   # User A's data
└── <user_id_B>/
    └── dirt.db                   # User B's data
```

`<dirt.db>` at the top level is only ever present until the first
successful sign-in (when it gets migrated into the user's dir). After
that, it never reappears.

## Design

### Core: `dirt_core::services::db_paths`

New module. Surface:

```rust
/// Compute the legacy solo-mode local DB path (pre-first-signin only).
pub fn solo_db_path(data_dir: &Path) -> PathBuf;

/// Compute the per-user local DB path for `user_id`.
pub fn user_db_path(data_dir: &Path, user_id: &str) -> Result<PathBuf>;

/// Validate that `user_id` is safe as a filesystem segment.
/// Rejects empty, slashes, NUL, `..`, and anything not matching
/// UUID-v7 shape.
pub fn validate_user_id(user_id: &str) -> Result<&str>;

/// Read the active-user pointer. `Ok(None)` if the file is absent.
pub fn read_active_user(data_dir: &Path) -> Result<Option<String>>;

/// Atomically rewrite the active-user pointer (write-temp +
/// fsync + rename so a power-cut never leaves a half-written file).
pub fn write_active_user(data_dir: &Path, user_id: &str) -> Result<()>;

/// Resolve the DB path + scoping user_id from disk state.
///
/// - Returns `(user_db_path(active), active)` if `state.json` is
///   present and valid.
/// - Returns `(solo_db_path, SOLO_USER_ID)` if `state.json` is absent.
pub fn resolve_active_db(data_dir: &Path) -> Result<(PathBuf, String)>;

/// First-time-only migration on first authenticated sign-in.
///
/// If `<data_dir>/dirt/dirt.db` exists AND
/// `<data_dir>/dirt/<user_id>/dirt.db` does not, move the file into
/// the user's directory and rewrite `notes.user_id`,
/// `pending_sync.user_id`, and `sync_state.user_id` from
/// `SOLO_USER_ID` to `user_id`. Aborts loudly if the destination
/// already contains a file. Idempotent: subsequent calls return
/// `AlreadyMigrated` or `NoSoloDb`.
pub async fn migrate_solo_db_to_user(
    data_dir: &Path,
    user_id: &str,
) -> Result<SoloMigrationOutcome>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoloMigrationOutcome {
    NoSoloDb,
    Migrated,
    AlreadyMigrated,
}
```

Migration mechanics:

1. Check destination doesn't exist; if it does and is non-empty,
   return `Err` (refuse to overwrite real data — CLAUDE.md: no silent
   fallback).
2. `std::fs::rename` legacy `dirt.db` into `<user_id>/dirt.db`
   (atomic on same FS). If cross-FS, fall back to copy + fsync + delete.
3. Open the moved DB; run `UPDATE notes SET user_id = ?` (and same
   for `pending_sync`, `sync_state`); commit; close.
4. Return `Migrated`.

If step 3 fails after step 2, the destination is partially-migrated
but in a recoverable state. Re-running the function will see the
destination exists, the source is gone → return `AlreadyMigrated` and
the next sync push will fail with a `ScopeMismatch` (because the
in-DB user_id is still SOLO). We surface that mismatch to the user
along with a clear "rerun the migration: ..." hint. Sharp edge,
acceptable for a 1-in-N-thousand IO failure mid-rename.

### Core: `DatabaseService` carries a `user_id`

```rust
impl DatabaseService {
    /// Open the DB and tag it with the user_id under which new notes
    /// will be created. `user_id` must be pre-validated.
    pub async fn open_for_user(
        db_path: impl Into<PathBuf>,
        user_id: impl Into<String>,
    ) -> Result<Self>;

    pub fn user_id(&self) -> &str;
}
```

Legacy `open_local_path` becomes `open_for_user(path, SOLO_USER_ID)`
internally — exists as a backwards-compat shim for the never-signed-in
fresh-machine case and existing tests. `open_in_memory` keeps
`SOLO_USER_ID` so the giant existing test suite doesn't churn.

`create_note` / `create_note_with_id` use `self.user_id` to stamp new
rows. The sync-helper methods (`is_pending`, `list_pending_notes`,
etc.) keep their explicit `user_id: &str` parameter — callers pass
`self.user_id()` into them.

`Note::new(content)` keeps its current SOLO behavior for backwards
compat. New helper `Note::new_for_user(content: impl Into<String>,
user_id: impl Into<String>) -> Result<Self>` — empty user_id returns
`Err(Error::InvalidInput)` rather than silently substituting SOLO.

### Core: sync engine

`SyncEngine::new` already takes `user_id: &'a str`. Every call site
stops passing `SOLO_USER_ID` and starts passing `db.user_id()`.

New error variant on `SyncEngineError`:

```rust
#[error("local DB belongs to {db_user} but session is for {session_user}; restart to switch accounts")]
ScopeMismatch { db_user: String, session_user: String },
```

`run_once` callers (worker + CLI sync) pass the session's `user_id`
along so the engine can compare at the start of the cycle. To avoid
threading a third argument, instead expose a small precheck on the
SessionApiClient consumers: the worker/CLI does the comparison
itself and short-circuits before constructing the engine. This keeps
the engine signature unchanged.

### Desktop (`dirt-desktop`)

`services::database::DatabaseService` (the desktop wrapper) loses its
hardcoded `default_db_path` and gains `new_for_user(user_id)`. Path
resolution moves to `db_paths::resolve_active_db`.

`app.rs::App` startup:

1. Resolve `(db_path, db_user_id)` from `db_paths::resolve_active_db`.
2. Open `DatabaseService::open_for_user(db_path, db_user_id)`.
3. Read keyring token.
4. Branch:
   - **Token present, token.user_id == db.user_id():** hydrate
     SessionApiClient, spawn worker.
   - **Token present, token.user_id != db.user_id():** This means a
     different client signed in while this one was closed. Treat as a
     user switch: shut down nothing (worker not spawned yet), update
     `state.json` to the new user_id, open the new user's DB
     (migrating from solo if applicable, else create fresh), then
     hydrate session + spawn worker. `signed_in` reflects the new
     user.
   - **Token absent:** leave `signed_in` = None, no worker. App still
     operates on the active-user DB (or the solo DB if never signed
     in). Local writes target that DB.

`account_settings.rs::apply_login_outcome` — after successful verify:

1. Shut down existing worker.
2. If `stored.user_id == current db.user_id()` → no DB swap; just
   hydrate session + spawn worker.
3. Else (first sign-in OR user switch):
   - `migrate_solo_db_to_user(data_dir, &stored.user_id)` (no-op if
     not the first sign-in).
   - `write_active_user(data_dir, &stored.user_id)`.
   - Drop current `db_service` Arc; open
     `DatabaseService::open_for_user(user_db_path, stored.user_id)`.
   - Reload settings + notes for the new DB.
   - Hydrate SessionApiClient, spawn worker.

`account_settings.rs::apply_logout_outcome`:

1. Shut down worker.
2. Clear `signed_in` + `session_client`.
3. **Do NOT touch the DB** — `db_service` keeps the same Arc, all
   subsequent local writes still target the active user's DB.
4. `state.json` stays pointed at the same user.

`services::sync_worker::sync_once` (and the CLI equivalent) reads
both `db.user_id()` and the current `stored_token.user_id` from the
SessionApiClient's store before constructing the engine. If they
mismatch, emit `SyncEvent::Status(Error)` + `Issue(...)` with the
mismatch copy and return without contacting the server.

### Mobile (`dirt-mobile`)

Same shape as desktop:

- `MobileNoteStore::open_for_user(user_id)`; `user_id()` accessor.
- `app_shell.rs` startup: `resolve_active_db` → open → branch on
  keyring presence/match same as desktop.
- `views::settings.rs::handle_verify` and `handle_logout`: same swap
  semantics as desktop (no DB touch on logout; swap on user change).
- `services::sync_worker::sync_once`: same mismatch guard.

Android paths: `default_mobile_data_directory().join(user_id).join("dirt-mobile.db")`
— no JNI change needed; `dirs::data_local_dir()` already resolves
right on Android.

### CLI (`dirt-cli`)

`commands::common.rs::resolve_db_path`:

Old shape returned `PathBuf` only. New shape returns
`(PathBuf, String)` — path and user_id. Resolution:

1. If `--db-path` or `DIRT_DB_PATH` is set → honor it; user_id is
   read from `state.json` if present, else SOLO. (Explicit override
   is a developer / test concern.)
2. Else → `db_paths::resolve_active_db(data_dir)`.

`commands::common.rs::open_database` accepts both and routes to
`DatabaseService::open_for_user`.

`commands::auth_cmd.rs::login_flow` — after successful verify:

1. Save token to keyring (already done).
2. If `state.json` is absent: migrate solo → `<user_id>` (preserves
   pre-upgrade CLI captures), write `state.json` with new user_id.
3. Else if `state.json.active_user_id != stored.user_id`: write
   `state.json` to new user_id. (Existing user's DB is left on disk.)
4. Else (same user signing back in): no-op.

`commands::auth_cmd.rs::logout_flow` — unchanged behavior on
`state.json` (leave alone) and DB (leave alone).

`commands::sync.rs::run_session_sync`:
- Read `stored.user_id` from the session client.
- Compare to `db.user_id()`.
- Mismatch → return `CliError::Auth("Session user differs from
  local user — sign in again on this client or run `dirt auth login`
  to refresh.")`.
- Else → pass `db.user_id()` to `SyncEngine::new`.

### Files touched

| File | Change |
| --- | --- |
| `crates/dirt-core/src/services/mod.rs` | export `db_paths` |
| `crates/dirt-core/src/services/db_paths.rs` | NEW: path helpers + state.json IO + migration |
| `crates/dirt-core/src/services/database.rs` | `user_id` field, `open_for_user`, route through |
| `crates/dirt-core/src/db/repository.rs` | `create` accepts user_id; `create_for_user` helper |
| `crates/dirt-core/src/models/note.rs` | `new_for_user(content, user_id)` (`Result` return) |
| `crates/dirt-core/src/sync/engine.rs` | `ScopeMismatch` error variant (or worker-side guard) |
| `crates/dirt-desktop/src/services/database.rs` | `new_for_user`, path helpers route through core |
| `crates/dirt-desktop/src/app.rs` | startup branches on `state.json` + keyring |
| `crates/dirt-desktop/src/components/settings/account_settings.rs` | login swaps DB on user change; logout leaves DB |
| `crates/dirt-desktop/src/services/sync_worker.rs` | mismatch guard; `db.user_id()` not `SOLO_USER_ID` |
| `crates/dirt-mobile/src/data.rs` | `open_for_user`, `user_id()`; legacy `open_default` removed |
| `crates/dirt-mobile/src/app_shell.rs` | startup branches on `state.json` + keyring |
| `crates/dirt-mobile/src/views/settings.rs` | login swaps store; logout leaves store |
| `crates/dirt-mobile/src/services/sync_worker.rs` | mismatch guard; `store.user_id()` not `SOLO_USER_ID` |
| `crates/dirt-cli/src/commands/common.rs` | `resolve_db_path` returns `(PathBuf, user_id)`; `open_database` takes user_id |
| `crates/dirt-cli/src/commands/auth_cmd.rs` | `login_flow` writes/updates `state.json` + runs migration |
| `crates/dirt-cli/src/commands/sync.rs` | mismatch guard; `db.user_id()` not `SOLO_USER_ID` |
| `crates/dirt-cli/tests/cli_db_per_user.rs` | NEW: two-user smoke + state.json behavior |

Estimated total: **~800–1000 LOC** (slightly larger than the original
estimate due to the active-user-pointer plumbing and the mismatch
guard).

### Tests

New tests:

- `dirt_core::services::db_paths` unit:
  - `solo_db_path`, `user_db_path`, `validate_user_id` happy + edge.
  - `read_active_user` / `write_active_user` round-trip with atomic
    rename; recovers from a half-written temp file.
  - `resolve_active_db` returns solo when absent, user-path when
    present.
  - `migrate_solo_db_to_user`:
    - `NoSoloDb` when nothing exists.
    - `Migrated` on first call: rows rewritten, file moved.
    - `AlreadyMigrated` on second call.
    - `Err` when destination already has data.

- `dirt_core::services::database`:
  - `open_for_user` stamps new notes with the given user_id (not SOLO).

- `dirt_core::sync` (or worker-level):
  - Sync precheck refuses when session user_id ≠ db.user_id().

- `dirt-cli/tests/cli_db_per_user.rs` end-to-end:
  - Two-user smoke: seed keyring + state.json as A, `dirt add "A1"`,
    swap to B (simulating a re-login), `dirt add "B1"`, list should
    show only "B1". Then swap back to A, list should show only "A1".
  - Cross-client mismatch: state.json = A, keyring = B → `dirt sync`
    errors with the mismatch copy and does not push.

Updated tests: existing `SyncEngine::new(_, _, SOLO_USER_ID)` test
fixtures stay verbatim (they're using SOLO as the test user_id; the
engine remains parameterized on user_id).

## Risks

- **Half-migrated DB after IO failure.** Mitigation in the migration
  ordering: rename first, then row rewrite. If rewrite fails, the
  next sync push surfaces `ScopeMismatch` (rows still SOLO,
  state.json says user A), and we instruct the user to delete the
  half-migrated file or rerun the migration explicitly. Sharp but
  recoverable.

- **`state.json` race.** Atomic write (temp + fsync + rename) handles
  single-process races. Multi-process races (CLI and desktop both
  running, both writing on respective logins) — last writer wins;
  this is correct since whichever sign-in finished last is the
  current active user.

- **Existing solo data on disk on a machine where the user has not
  yet signed in.** Behavior is preserved: legacy `dirt.db` + SOLO
  until first sign-in.

## Implementation order

1. `dirt-core::services::db_paths` (path helpers, state.json IO,
   migration, tests). No callers yet.
2. `DatabaseService::open_for_user` + `Note::new_for_user` + repo
   plumbing. Tests.
3. `SyncEngine` mismatch error variant (or worker-level precheck).
   Tests.
4. CLI cutover: `resolve_db_path` consults `state.json`; `auth_cmd`
   writes state on login; `sync` passes `db.user_id()` and guards
   against mismatch. Integration tests.
5. Desktop cutover: startup → resolve_active_db; login → swap on
   user change, no-op on same user; logout leaves DB. Mismatch guard
   in worker.
6. Mobile cutover: same as desktop.
7. CHANGELOG; close issue #234 in PR description.

If PR ends up >800 LOC, split after step 4 into PR1 (core + CLI;
fully ships the security fix for CLI users) + PR2 (desktop +
mobile).
