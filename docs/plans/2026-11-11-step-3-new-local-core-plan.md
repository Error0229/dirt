---
title: "Step 3: new local core"
type: plan
date: 2026-11-11
status: in progress
parent: docs/plans/2026-11-10-refactor-system-redesign-implementation-plan.md
tracking: "#239"
---

# Step 3: new local core

## Goal

Replace the local half of `dirt-core` with the design's note model and store:
each note is a CRDT document in a `rusqlite` database, and the desktop app,
the CLI and the Android app run on it. Nothing in this step talks to a server.

Design sections 6.1, 6.2, 6.5, 8.2 and 8.4 are the specification. This plan
covers the order of work and the decisions the parent plan left open.

## State before this step

Facts from a survey of the code on 2026-11-11:

- `dirt-core` holds one `libsql` connection behind an `Arc<Mutex<Database>>`,
  with no busy timeout. Every client calls it through `DatabaseService`.
- The database path does not depend on who is signed in, and every row
  carries the same fixed user ID (`SOLO_USER_ID`).
- `dirt-api` uses `dirt-core` only for `NoteId`, the `Note` struct and
  `SOLO_USER_ID`. Its `libsql` use is its own dependency.
- `dirt-cli` depends on `libsql` only for one error conversion.
- The old sync engine reads and writes local sync state through
  `DatabaseService`: a pending queue, a timestamp cursor and a merge matrix.
  None of that exists in the new store.
- Clients call eleven store operations between them: list, create, create
  with a given ID, get, update, delete, search, list by tag, find IDs by
  prefix, load settings, save settings.

## Decisions

The parent plan's "Settle first" items, taken at their defaults unless noted.

| Question | Decision |
|---|---|
| Where notes live before sign-in | A store named `local`. An account's store sits in a folder named after the account. This step ships the folder layout and a function that moves the local notes into an account store; step 5 calls it at first sign-in, after a confirmation naming the account |
| `dirt-mobile` between steps 3 and 7 | Kept compiling and working locally on the new store. Trial 2 showed `rusqlite` builds for Android, so there is no reason to drop it from CI |
| Durability | `synchronous=FULL`. Trial 2 passed with it |
| Existing local data | Not imported. The design states there is none to preserve, and the developer's own database is empty. The new store uses a new path, so an old `dirt.db` is left untouched on disk |

Decisions this plan adds:

| Question | Decision |
|---|---|
| The old sync engine | Deleted in this step, not in step 5. It cannot compile against the new store, and adapting it would mean writing code for a server that step 4 replaces. This departs from amendment C4 for `sync/engine.rs`, `sync/merge.rs` and the two `sync_worker.rs` files |
| The old sign-in | Kept as it is until step 5. It talks to the server only and compiles unchanged. Signing in has no effect on notes during steps 3 and 4 |
| Sync between this step and step 5 | Off. Clients show sync as unavailable, and `dirt sync` says so. Notes are local only |
| Issue #234 | Stays open until step 5. This step builds the per-account folders, but only step 5's sign-in opens them |
| Where `NoteId` lives | In the new `dirt-proto` crate, so `dirt-api` can drop `dirt-core` |
| Async interface | The store's functions are `async`. Each sends a job to the store's own threads and awaits the answer. No tokio runtime is required to call them |
| Tests that need a database | Use a temporary file, not an in-memory database. Write-ahead-log mode and separate read connections do not work on `:memory:` |

## Work, in three pull requests

`rusqlite` and `libsql` cannot be linked into one program, so the store cannot
arrive beside the old database code. It arrives in the same pull request that
removes `libsql` from the clients.

### 3a: `dirt-proto`, the server cut loose, and the notes module

No behaviour changes.

- [ ] New crate `dirt-proto` with `NoteId`. `dirt-core` re-exports it.
- [ ] `dirt-api`: remove the `dirt-core` dependency (amendment C1). It gets its
      own row type in place of `dirt_core::Note`, and its own constant for the
      fixed user ID.
- [ ] `dirt-cli`: remove the direct `libsql` dependency.
- [ ] `dirt-core::notes`: the CRDT document, ported from
      `trials/yrs-size/` with its tests.
  - `diff`: old text and new text to the smallest inserts and deletes.
  - `NoteDoc`: text, edit counts and edit time per device, deletion markers
    per device, merge, and the yrs v2 encoding.
  - The lost-state check: an incoming document that holds more of this
    device's edits than the device does.
  - Derived values: text, tags, edit time, deleted or not.

### 3b: the store, and the clients moved onto it

- [ ] `dirt-core::store`, on `rusqlite` with the `bundled` feature.
  - Opening: busy timeout first, then the switch to write-ahead-log mode with
    a retry, then `synchronous=FULL` and foreign keys.
  - Set-up: schema version checked inside a `BEGIN IMMEDIATE` transaction.
  - One writer thread owning the write connection. Every write is one
    `BEGIN IMMEDIATE` transaction that reads the document, changes it and
    writes it back with its derived values.
  - Two reader threads, each with a read-only connection.
  - The install's CRDT client ID, kept in the database.
  - Per note: document, derived text, edit time, deleted flag, a local
    revision counter, the revision last pushed, and the last change number
    seen from the server. The last three are written now and used in step 5.
  - Tags as rows of note ID and tag. The full-text index updated by the same
    write, not by triggers.
  - Settings as key and value rows.
- [ ] Store locations: `<data folder>/dirt/stores/local/` and
      `<data folder>/dirt/stores/<account id>/`, and the function that moves
      local notes into an account store.
- [ ] Delete `db/`, `services/database.rs`, `sync/engine.rs`, `sync/merge.rs`
      and the conversions between `Note` and the old wire types. Remove
      `libsql` from `dirt-core`.
- [ ] `Note` loses `user_id`, `server_updated_at` and `deleted_at`. It keeps
      `id`, `content`, `created_at` and `updated_at`, so the interfaces change
      little. `created_at` now comes from the note's ID.
- [ ] Desktop, CLI and Android: call the store. Remove the two sync workers
      and the large-stack workaround that `libsql` needed on Windows. Undo of
      a delete becomes a restore of the same note.
- [ ] `dirt sync` and the Sync settings tab report that sync is unavailable
      until the redesign's sync lands.

### 3c: tests across processes, and CI

- [ ] A two-process test in `dirt-core`, modelled on
      `trials/sqlite-two-process/`, with its two controls. It runs on Windows,
      macOS and Linux.
- [ ] The convergence and deletion tests from trial 1 run in `dirt-core`.
- [ ] The Android CI job builds and links, in place of `cargo check`, if the
      mobile binary links with plain cargo. If it does not, record why.
- [ ] Close #210 and #218. Update the parent plan.

## Schema of the new store

```sql
-- Schema version is SQLite's user_version.
CREATE TABLE meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
) WITHOUT ROWID;                      -- 'client_id'

CREATE TABLE notes (
  rowid            INTEGER PRIMARY KEY,   -- joins the full-text index
  id               TEXT NOT NULL UNIQUE,  -- UUID v7
  doc              BLOB NOT NULL,         -- yrs v2 encoding
  text             TEXT NOT NULL,         -- derived
  edited_at        INTEGER NOT NULL,      -- derived, Unix ms
  deleted          INTEGER NOT NULL,      -- derived, 0 or 1
  revision         INTEGER NOT NULL,      -- raised by every local change
  pushed_revision  INTEGER,               -- revision last accepted by the server
  server_change    INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX notes_by_edit ON notes (deleted, edited_at DESC);

CREATE TABLE note_tags (
  note_id TEXT NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
  tag     TEXT NOT NULL,
  PRIMARY KEY (note_id, tag)
) WITHOUT ROWID;
CREATE INDEX note_tags_by_tag ON note_tags (tag);

CREATE VIRTUAL TABLE notes_fts USING fts5(text);  -- rowid = notes.rowid, live notes only

CREATE TABLE settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
) WITHOUT ROWID;
```

## Risks

| Risk | Handling |
|---|---|
| 3b is large: the store arrives and three clients move in one pull request | It cannot be split, because the two SQLite libraries cannot coexist. The store is written and tested first within the branch, then each client is moved in its own commit |
| Search behaves differently | Queries are passed to the full-text index as phrases, so punctuation in a query can no longer cause an error. Tests cover the cases the old tests covered |
| Sync is off for two steps | Stated in the README and in the app. The server and its data are untouched |

## Exit

- [ ] The convergence, deletion and two-process tests pass in `dirt-core`.
- [ ] Create, edit, delete, restore, search, tags and export work in the
      desktop app and the CLI.
- [ ] `libsql` appears in the dependencies of `dirt-api` only.
- [ ] #210 and #218 are closed. #234 is linked to step 5.
- [ ] The README says that sync is off until step 5.
