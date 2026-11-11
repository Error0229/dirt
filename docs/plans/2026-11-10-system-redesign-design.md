# Dirt system redesign: design

- **Date:** 2026-11-10
- **Status:** design approved section by section in discussion; amended the
  same day with the 21 amendments (A1 to C6) recorded in the implementation
  plan; not yet implemented
- **Supersedes:** the original design document and the backend API, deploy,
  security and feature-parity documents. All are now in `docs/archive/`. Each
  replacement is written by the build step that defines its subject.

## 1. Purpose

Dirt grew through three rebuilds of its sync and login stack without a written
baseline of requirements. Each piece was hardened in isolation, and nobody
could say when one was finished. This document fixes the requirements first,
then records each design decision with the alternatives that were rejected and
the reason.

The project is treated as a system design case study. The existing code is the
"before", and this design is the "after". Scope is not cut to reduce
complexity: multi-user accounts, three clients and sync all stay. The aim is
that every part is justified by a stated requirement.

## 2. Where the code is today

About 23,600 lines of Rust across five crates, tests included.

| Area | Lines | Share |
|---|---|---|
| Login: magic codes, sessions, refresh, token stores, email, rate limits | ~8,400 | ~36% |
| Sync: engine, merge, HTTP client, background workers, server routes | ~3,200 | ~14% |
| The notes app: models, database, search, interface for three clients | ~12,000 | ~50% |

History of the sync and login stack:

1. **February 2026:** Supabase auth, Turso embedded replicas, R2 media and
   voice memos, built in about ten days.
2. **April 2026:** torn down and replaced by `dirt-api` with one shared token.
3. **May 2026:** the shared token replaced by email magic codes, sessions,
   refresh rotation and three rate limiters, across eight pull requests.

Known problems this design must resolve:

- **Silent data loss:** the server accepts whichever push arrives last and
  overwrites the other without checking (`dirt-api/src/turso.rs`, `upsert`).
- **Half-finished multi-user:** the server is per-user, but clients sync under
  the hardcoded `SOLO_USER_ID`, and all accounts share one local database
  (issue #234).
- **Duplication:** the desktop and mobile `sync_worker.rs` files are about 500
  lines each and near-copies. Wire types are defined once on the server and
  again on the client.
- **Rate limiters that do not limit:** they live in process memory, and
  serverless instances do not share memory.
- **Dead code:** `transcription.rs` and the media settings remain, but nothing
  records audio.
- **Stale documentation and issues:** the docs describe the April architecture,
  and 13 of 14 open issues describe the February one.

## 3. Requirements

### 3.1 Operating assumptions

- One developer, a hobby budget, and up to a few thousand users.
- Every design claim should be testable by deploying and measuring.
- Each decision carries a short note on what breaks first at 100 times the
  load, and the way out. Nothing is built for that load now.

### 3.2 Functional scope

- Accounts for many users.
- Three clients: desktop (Dioxus), Android (Dioxus) and CLI. All Rust.
- Plain-text notes with `#tag` extraction and full-text search, as today.
- Offline-first: capture never waits on the network. The app is usable before
  sign-in.
- Sync between a user's devices, with automatic merging of concurrent edits.
- End-to-end encryption: the server stores only ciphertext.
- Attachments are specified in this document and built last.

Principle adopted during the discussion: anything we intend to build later is
included in the specification now, with its place in the build order.

### 3.3 Non-goals

- Real-time collaboration or live typing between devices.
- Rich text.
- Any server-side feature that needs to read notes, such as server search.
- Web and iOS clients. They are not covered by this document.

### 3.4 Threat model

| Threat | Covered? |
|---|---|
| Stolen or leaked server database | Yes |
| Operator reading notes | Yes |
| Compromised login provider, or a stolen email inbox | Yes: it yields only a pending device, which needs approval |
| Server that actively swaps keys during device enrolment | Only if the user compares the short code |
| Stolen device token | Yields ciphertext only; revocable |
| Unlocked, compromised device | No. The local database is unencrypted so search works, and relies on the operating system's disk encryption |

The server still sees metadata: how many notes an account has, their sizes,
and when they change.

## 4. Decision summary

| # | Decision | Chosen | Main alternatives rejected |
|---|---|---|---|
| 1 | Scale target | Small real product, with 100x notes | Design for a million users; three explicit tiers |
| 2 | Feature scope | Text sync now; attachments specified, built last | Attachments first-class now; everything that ever existed |
| 3 | Concurrent edits | Automatic merge, one text CRDT per note | Last push wins; conflict copies; three-way merge |
| 4 | Freshness | Sync on events: app start, return to the app, after local changes, and a sync button. No timer | A polling timer; server push over a socket; live typing |
| 5 | Privacy | End-to-end encrypted | Server-readable; server-readable with opaque payloads |
| 6 | New devices | Approval from an existing device; optional recovery phrase | Forced recovery phrase; user passphrase; QR pairing; passkeys |
| 7 | Existing code | Rebuild core and server; keep client interfaces | Step-by-step migration; fresh repository |
| 8 | Sync shape | One encrypted blob per note, replaced with a version check | Per-note edit log; one log per account |
| 9 | Login | Clerk, for identity only | Own email-code system |
| 10 | Server database | Postgres on Neon | Turso; SQLite file on our own server |
| 11 | Server hosting | Vercel serverless (axum) | Cloudflare Workers; Cloudflare Containers; always-on server |
| 12 | Local database library | `rusqlite` | `libsql` |
| 13 | CRDT library | yrs, pending a size measurement | Automerge; Loro |

### Notes on rejected alternatives

- **Convex (decision 3):** considered as a way to buy merging and syncing. It
  would replace the server, which is the simple part. Its standard clients hold
  state in memory over a live connection, built-in offline support was still in
  development when checked, it does not merge text edited offline, and server
  logic must be TypeScript. No record was found of an earlier evaluation.
- **PowerSync (decision 3):** syncs rows, but merging text inside a row is
  still left to the application.
- **Three-way merge (decision 3):** merges edits to different parts of a note
  but still needs a fallback when edits overlap, so it is not fully automatic.
- **A polling timer (decision 4):** the design first had a 30-second timer.
  Its only benefit is that an app left open updates by itself. One person
  rarely uses two devices at the same moment, so syncing when they return to
  the app gives nearly the same result. Trial 4 showed the cost: a polling
  client keeps the database awake for as long as it is open. The timer was
  removed on 2026-11-11.
- **Server push over a socket (decision 4):** it needs an always-on service
  holding a connection for every client, which the serverless function is
  not. The server cannot read notes, so the message could only say "pull
  now". On Android a background socket costs battery, and the proper tool,
  platform push, is a third component. A "pull now" signal can be added
  later without changing the protocol (section 11).
- **Passkeys (decision 6):** the lowest friction where passkeys already sync
  between devices, but support in native desktop apps and a CLI is uneven. The
  key design leaves room to add them: every device receives its own wrapped
  copy of the same account key, and passkeys would be one more way to deliver
  it.
- **Turso (decision 10):** inherited from the embedded-replica plan, which was
  removed. The "same engine on both ends" reason no longer applies, because
  client and server schemas now have nothing in common. Turso also announced on
  2026-10-02 that it is being acquired by Supabase, with no commitment on
  current plans or prices. Its advantages were a larger free allowance and
  in-process tests.
- **Cloudflare Workers (decision 11):** the server would have to compile to
  WebAssembly, which rules out the usual tokio and sqlx stack and constrains
  every dependency. Cloudflare Containers run a normal binary but need a paid
  plan and are a newer product. Because the server is a stateless axum app with
  Postgres behind it, moving hosts later is a deployment change.

## 5. Identity, devices and keys

### 5.1 Login

- Clerk proves who the person is. It does nothing else.
- The app opens the system browser on Clerk's hosted sign-in page using OAuth
  with PKCE, requesting the `openid` scope so that Clerk returns an ID token.
  Desktop and CLI receive the result on `127.0.0.1` with a port chosen at run
  time. Android receives it through a custom URL scheme, or an app link if
  Clerk refuses the scheme.
- Before sign-in, the client asks our server for a single-use nonce and passes
  it to Clerk. The ID token stays valid for a day, so without the nonce it
  could be replayed.
- The client sends Clerk's ID token to our server once, when registering the
  device. The server verifies the signature against Clerk's public keys and
  checks the issuer, the audience, the expiry and the nonce. It maps the Clerk
  issuer and user ID to an account, and issues a device token.
- After that, Clerk is out of the path. Sync uses only the device token.
- Clerk also returns an access token and a refresh token. The client
  discards both without storing them.
- A sign-in that never comes back must time out with a message. For a wrong
  redirect target Clerk shows an error page in the browser and does not
  return to the app.
- No Clerk SDK is needed: a generic OAuth client and standard token
  verification are enough.
- Clerk's free plan offers email code and up to three social logins. Passkeys
  and multi-factor login need its paid plan in production.
- A Clerk production instance needs a domain we own. Development instances are
  capped at 100 users.

### 5.2 Devices and sessions

- Logging in registers a **device**: a name, a public key and a status of
  pending, approved or revoked.
- The desktop app and the CLI on one computer are one device. They share
  credentials through the operating system's secure storage, and share the
  local database.
- Each device has one long-lived token, stored hashed on the server and
  revocable. It expires after 90 days without use.
- The current refresh-and-rotate machinery is removed. A stolen token now
  yields only ciphertext, so rotation no longer earns its complexity.

### 5.3 Keys

- **Account key:** a random 256-bit key created silently on the first device.
  It encrypts every note and never reaches the server unwrapped.
- **Device key pair:** generated on each device. The private half stays in the
  device's secure storage.
- **Key version:** every note blob records which account key version encrypted
  it, so the key can be rotated later.

### 5.4 Adding a device

1. The new device logs in through Clerk and uploads its public key. It is
   marked pending and can read nothing.
2. Existing devices see the pending device at their next sync and show
   "Approve new device?". While the approval screen is open on a device,
   that device polls every few seconds, because each side is waiting for the
   other. This is the only polling in the system. It stops when the screen
   closes, and after a few minutes in any case.
3. Both devices display the same short code. It comes from a commit-then-reveal
   exchange, so the server cannot choose keys that produce a matching code.
4. On approval, the existing device encrypts the account key to the new
   device's public key and uploads it. The new device downloads and unwraps it,
   and the server deletes its copy.

The first device of an account needs none of this. A CLI on a computer that
already runs the desktop app needs none of it either.

### 5.5 Recovery and revocation

- **Recovery phrase (optional, never forced):** settings can generate one. The
  server then stores the account key wrapped under a key derived from the
  phrase. A new device with no other device available signs in and enters the
  phrase. Deleting the wrapped copy cancels the phrase.
- **Lost everything, no phrase:** the notes cannot be recovered. The user can
  reset the account, which deletes the ciphertext and starts fresh.
- **Removing a device:** revokes its token at once.
- **Key rotation:** issue a new account key, re-encrypt all notes, and wrap the
  new key for the remaining devices. Specified here, built in step 8.

### 5.6 Rules for the build

- Use audited cryptography libraries only. No hand-written primitives.
- Model the enrolment exchange on an established short-code verification
  protocol, not a new invention.
- Proposed primitives, to confirm at implementation: XChaCha20-Poly1305 for
  note blobs with a random nonce, and X25519 for device keys.

## 6. Note model and sync protocol

### 6.1 What a note is

- One CRDT document holding the note's text, a last-edited time for display,
  and a deletion marker.
- Plain text, tags and the search index are derived from the document on every
  change.
- Creation time is read from the note's UUID v7, so it is not stored
  separately.
- **Edits are applied as diffs.** The editors hand over the whole text. The
  client compares it with the previous text and applies only the inserts and
  deletes. Replacing the whole text would make two concurrent edits appear
  twice after merging.
- **Client ID.** Each install has one CRDT client ID, stored in the local
  database so that it is lost together with the data it describes. Every edit
  is read, changed and written back inside one write transaction, which keeps
  the desktop app and CLI safe on a shared ID. If an incoming document holds
  more of this device's own edits than the device does, the device takes a new
  ID.

### 6.2 Deletion

A delete covers only what the deleting device had seen. If another device
edited the note without knowing about the delete, the merge brings the note
back with that edit.

Mechanism, as proven in trial 1:

- The document keeps a count of edits per device. Every edit raises the
  editing device's count, in the same write that records the edit time.
- A device that deletes a note writes its own marker: a copy of the counts it
  had seen.
- The note counts as deleted while at least one marker covers every count.
  Restoring a note is an ordinary edit.

Two details matter:

- The counts are kept in the document, not taken from yrs's own state vector.
  The state vector does not move when an edit only removes characters, so such
  an edit would count as already seen.
- There is one marker per device. With a single shared marker, two devices
  deleting the same note could bring it back.

"Delete always wins" would be simpler but can drop an edit, which breaks the
promise of never losing a thought.

### 6.3 What the server stores per note

| Field | Purpose |
|---|---|
| Account ID, note ID | Ownership and identity |
| Change number | The account's next number from a single counter, taken at each accepted write. It is both the note's version and the pull cursor, so the protocol contains no clocks |
| Key version | Which account key encrypted the blob |
| Format version | Which document format is inside, so the CRDT library can be changed later |
| Ciphertext | The whole CRDT document in the yrs v2 encoding, encrypted. Trial 1 measured v1 at 3.6 times the size |

The account ID, note ID, key version and format version are bound into the
encryption as associated data, so the server cannot move one note's blob to
another note. Each client also remembers the last change number it saw per
note and refuses a lower one, which stops the server replaying an old version.

### 6.4 Protocol

- **Push:** a batch of items, each with a note ID, the change number the client
  last saw for that note (zero for a new note), and the new ciphertext. Each
  item is accepted with its new change number, or refused as stale. A refusal
  includes the current blob, which saves a round trip.
- **Pull:** everything after the client's cursor, in pages ordered by change
  number.
- **Size caps:** a push batch and a pull page are each capped at about 3 MB,
  counted in bytes and not in notes. The hosting platform rejects bodies over
  4.5 MB.
- **Account epoch:** every response carries the account's epoch number. It
  changes when the account is reset or the database is restored from a backup.
  A client that sees a new epoch resets its cursor and its last-seen change
  numbers, then merges everything again. Without it, clients would refuse the
  lower change numbers that follow a restore and stay stuck.
- **One sync cycle:** pull, decrypt and merge every incoming document into the
  local one, then push whatever is still changed locally. A refused push is
  merged and retried on the next cycle.
- **Retries are safe:** merging the same document twice changes nothing, so a
  lost response or repeated push cannot corrupt a note.

### 6.5 Local database

- One database per account, in a folder named after the account. This closes
  issue #234.
- Per note: the CRDT document, the derived text, the last-seen change number,
  and a local revision counter. A successful push marks the note as synced
  only if the counter has not moved since the push began, so an edit made
  during a push is never skipped.
- Tags, the tag index, the full-text index, settings and the sync cursor, as
  today.
- Notes created before sign-in live in a local store and are uploaded to the
  account on first sign-in. Every note has a unique ID, so nothing can collide.

### 6.6 What this replaces

- The ten-case merge matrix in `dirt-core/src/sync/merge.rs`: every incoming
  note is simply merged.
- The `pending_sync` table and the timestamp cursor.
- Silent overwrites and `SOLO_USER_ID`.

## 7. Server and hosting

### 7.1 Responsibilities

- Verify the Clerk token once per device and issue a device token.
- Keep the device registry.
- Relay enrolment messages and the wrapped account key, and hold the
  recovery-wrapped key if one exists.
- Store note blobs, apply the version check on push, serve pages on pull.
- Enforce limits.

It sends no email, merges nothing, and never inspects note content.

### 7.2 Endpoints (proposed paths)

| Area | Endpoints |
|---|---|
| Health | `GET /healthz` |
| Devices | `POST /v1/devices`, `GET /v1/devices`, `DELETE /v1/devices/{id}` |
| Enrolment | `POST` and `GET /v1/enrolments/{device_id}/messages`, `PUT` and `GET /v1/enrolments/{device_id}/key` |
| Recovery | `PUT`, `GET` and `DELETE /v1/recovery` |
| Notes | `POST /v1/notes/push`, `GET /v1/notes/pull` |
| Account | `DELETE /v1/account` |

A pending device may call only the enrolment endpoints and
`GET /v1/recovery`. It may not create or delete the recovery copy, so a stolen
email inbox cannot destroy it.

### 7.3 Tables

- **accounts:** ID, Clerk issuer and user ID, email, change counter, epoch,
  current key version, usage totals for quotas.
- **devices:** ID, account, name, public key, token hash, status, last seen.
- **notes:** account, note ID, change number, key version, format version,
  ciphertext. Primary key on account and note ID; index on account and change
  number.
- **enrolment_messages:** short-lived rows for the approval exchange.
- **recovery_keys:** one optional row per account.

### 7.4 Database

- Postgres on Neon. The server always connects through the pooled endpoint.
  Vercel runs about one instance per request in a burst (156 instances for
  500 simultaneous requests in trial 4), so the number of instances, not the
  pool size, sets the number of connections. The direct endpoint allows 112
  at the smallest compute size.
- `sqlx` as the driver. All SQL lives in one module and uses standard SQL only.
- Migrations are SQL files applied at deploy time, not at server start. They
  run against Neon's direct endpoint, because the migration lock is not
  supported through the pooled one.
- **The counter:** taking the account's next change number locks that account's
  row until the write commits. Writes for one account are therefore ordered
  even with many server instances. The version check is one conditional update
  in the same transaction.
- **Capacity:** an estimated 1 MB per active user, so about 3 GB at 3,000
  users. Neon's free plan holds 1 GB, which covers somewhat under a thousand
  users; beyond that, storage is paid by use.
- **Idle wake-up:** on the free plan the database sleeps after five idle
  minutes. The first request after that took 0.99 s in trial 4, of which
  0.7 s was the database. Acceptable for background sync.
- **Compute allowance:** the free plan includes 100 compute-unit hours per
  project per month, which is 400 awake hours at the smallest size, about 13
  a day. The database stays awake for five minutes after each request.
  Trial 4 showed that a client polling every 30 seconds keeps it awake
  without a break: 154 polls over 76 minutes and it never slept. That would
  use 182.5 hours a month for one open client. This is why sync has no timer
  (decision 4): the database is awake for about five minutes after each
  sync and asleep otherwise.
- **When the free plan runs out:** the allowance is shared by all users, so
  it lasts until people between them sync through more than about 13 hours
  of the day. Expect a paid plan at that point. A database of this size that
  never sleeps costs about $19 a month.
- **Region:** a Neon project's region cannot be changed, and Neon has no Tokyo
  region. The trial project uses AWS US West 2 (Oregon), which is co-located
  with Vercel's `pdx1`. The function region must be pinned to match.
- **Backups:** Neon's point-in-time restore, plus a scheduled dump stored
  outside Neon.

### 7.5 Hosting

- The axum app stays on Vercel's serverless Rust runtime.
- Vercel's free plan is for non-commercial use only. Charging for Dirt, or
  showing ads, requires the paid plan. Donations are exempt.
- Vercel caps request bodies at 4.5 MB, which is why section 6.4 caps batches
  by bytes. Trial 4 measured it: 4,493,163 bytes accepted, 4,494,140
  rejected. A 16 MB response was returned in full, but pull pages keep the
  same cap so that the protocol does not depend on this host.
- The deployed project must name its Rust toolchain in `rust-toolchain.toml`.
  Vercel's builder ships Rust 1.92.0 and `sqlx` 0.9 needs 1.94.
- Measured in trial 4: a warm pull takes 59 ms and a warm push 61 ms at the
  95th percentile. The database share is 3 to 4 ms at the median.

### 7.6 Limits

- Per-account request counts kept in the database, not in process memory.
- Storage quotas: notes per account, bytes per note, total bytes per account.
  The server cannot inspect what it stores, so size is the only thing it can
  police.
- Unauthenticated traffic is left to the hosting platform's firewall.

## 8. Client architecture

### 8.1 Crates

| Crate | Role |
|---|---|
| `dirt-proto` (new) | Request, response and error types shared by server and clients |
| `dirt-core` | Everything the three clients share |
| `dirt-desktop`, `dirt-mobile`, `dirt-cli` | Interface and platform glue only |
| `dirt-api` | The server. It no longer depends on `dirt-core` |

Rule: no HTTP, SQL or cryptography lives in a client crate.

### 8.2 Modules in `dirt-core`

- **notes:** the CRDT document, and the text and tags derived from it.
- **store:** the local SQLite database, with search, settings and sync state.
- **crypto:** account and device keys, note encryption, the enrolment exchange
  and the recovery phrase.
- **identity:** the Clerk browser sign-in and device registration.
- **secrets:** one interface for the device's secure storage, holding the
  device token, device key and account key together. Versions: operating-system
  keychain for desktop and CLI, the Android implementation, and in-memory for
  tests.
- **api:** the HTTP client for the server.
- **sync:** the pull, merge and push cycle, and the background worker that
  schedules it. The CLI runs one cycle.
- **session:** one object a client holds, with four states: signed out, signing
  in, waiting for approval, and ready. All three clients render the same
  states.
- **export:** as today.

### 8.3 Behaviour

- **Capture never waits:** a note is written to the local store and returned at
  once, then the sync worker is nudged.
- **Sync triggers:** events only. There is no timer. The worker syncs:
  - on app start;
  - when the person returns to the app, which is window focus on desktop and
    resume on Android, unless it synced within the last 30 seconds;
  - after each local change, debounced;
  - when the person presses the sync button;
  - again, with backoff, only while local changes are still unsent, for
    example after being offline.
- **Sync button:** every client has a manual sync action and shows when it
  last synced. An app that is open but untouched does not update until the
  person returns to it or presses the button.
- **Sign-out:** the app syncs, then deletes that account's local database and
  keys. If unsynced notes cannot be uploaded, it warns and offers an export
  first.
- **Sign-out guard:** signing out of the only approved device is blocked with
  a warning unless a recovery phrase exists. It would otherwise destroy the
  last copy of the account key.

### 8.4 Local database library

- `rusqlite` replaces `libsql`, which was adopted only for embedded replicas.
- One write connection and separate read connections, in write-ahead-log mode.
  This addresses issues #210 and #218, where one locked connection blocks the
  interface during sync.
- The desktop app and CLI open the same file from two processes. Trial 2
  showed that two rules make this safe and that each is needed:
  - Every write transaction starts with `BEGIN IMMEDIATE`. With plain
    `BEGIN`, 850 of 2,000 writes failed with a busy error that no timeout
    prevents.
  - Opening the file sets the busy timeout first, then retries the switch to
    write-ahead-log mode until that timeout has passed. The busy timeout
    does not cover the switch: without the retry, one of two processes
    opening a new file together failed in 70 of 100 attempts.
- Schema set-up checks the schema version inside its write transaction, so
  two processes starting together cannot both run it.
- On Android the bundled SQLite keeps temporary data in memory. `rusqlite`
  sets this at build time for Android targets, where an app has no shared
  temporary folder. Full-text search is on in the bundled build everywhere.
- `rusqlite` and `libsql` each compile their own SQLite and cannot be linked
  into one program. A program's dependencies must contain one or the other,
  which fixes how step 3 begins.

## 9. Errors, testing and visibility

### 9.1 Errors

- The server returns a code, a message and an optional retry delay. Codes are
  defined once in `dirt-proto`.
- Every error maps to one of four classes on the client, and the class alone
  decides what happens. This replaces string matching on messages (issue #216).

| Class | Examples | Client behaviour |
|---|---|---|
| Try again later | Network down, server error, rate limited | Back off; show "offline" |
| Sign in again | Device token revoked or expired | Stop syncing; local notes untouched |
| Update the app | Newer protocol or document format | Stop; never overwrite what it cannot read |
| Problem with one note | Will not decrypt, corrupt, over quota | Flag that note; keep syncing the rest |

- **Crash safety:** a local change is one database transaction, and the cursor
  is saved after each page. Any interrupted step can run again.

### 9.2 Testing

Tests are concentrated where bugs lose data.

- **Merge:** randomised tests apply random edits on two or three copies, merge
  them in every order, and require identical text. The deletion rule is
  included.
- **Encryption:** round trips, plus tampering. A flipped bit must be rejected,
  and so must a blob moved to another note.
- **Multi-device simulation:** the real server runs in-process with two or
  three real client cores against it. Both devices edit offline, both sync, and
  both must end up equal. This is the most valuable test in the system.
- **Enrolment:** the full approval flow, including a server that swaps a key,
  which must produce mismatched codes.
- **Server:** the version check, quotas, account isolation, and that a pending
  device can read nothing.
- **Clerk:** replaced in tests by a local key that issues tokens.
- **Interfaces:** kept thin and covered by the session state tests, plus a
  short manual checklist for the hotkey, tray and Android glue.

Local tests need a running Postgres. The repository gets a Docker Compose file
for it, and each test runs in its own throwaway database. Continuous
integration uses a Postgres service container.

### 9.3 Continuous integration

- Kept: formatting, lints, tests and the Android build check.
- Changed: the Android job runs `cargo build` in place of `cargo check`, so
  that link failures show. Plain cargo links `rusqlite` for Android, so the
  job does not need `dx` (trial 2).
- Removed: the workflow named after the deleted "managed mode".
- Updated: the secret-scanning script, which still guards token names that no
  longer exist.

### 9.4 Visibility

- **Server:** one structured log line per request with account, endpoint,
  outcome and duration. Bodies and tokens are never logged.
- **Clients:** a sync status view showing last success, notes waiting, and the
  last error class. The CLI gets a verbose flag that prints the same.

## 10. Attachments (specified now, built last)

- **What one is:** an immutable encrypted file. Changing a file means adding a
  new attachment, so there is nothing to merge.
- **Encryption:** each file gets its own random key and is encrypted on the
  device before upload.
- **Where the key lives:** inside the note's CRDT document, next to the file's
  name, type and size. The server sees neither the key nor the name, and two
  devices adding attachments to one note merge like any other edit.
- **Storage:** ciphertext goes straight from the device to Cloudflare R2, using
  a short-lived upload link issued by our server. The server never handles the
  bytes. R2 charges nothing for downloads.
- **Offline:** the file is kept locally and queued for upload. The note syncs
  on its own, and other devices show a placeholder until the file arrives.
- **Thumbnails:** a small encrypted thumbnail is uploaded alongside, because
  the server cannot generate one.
- **Clean-up:** the server cannot tell which files are referenced, so clients
  tell it. A file is confirmed once its note has synced, and deleted 30 days
  after its removal has settled. Uploads never confirmed are discarded after a
  week.
- **Limits:** a maximum size per file and a total per account.
- **Key rotation stays cheap:** rotating the account key re-encrypts notes
  only, not files.
- **Voice memos:** audio attachments. Transcription could only run on the
  device, since the server cannot read the audio.

## 11. What breaks at 100x

| Area | What breaks first | Way out |
|---|---|---|
| Sync shape | Long, heavily edited notes re-upload in full on every edit | A per-note log of encrypted edits, for those notes only |
| Freshness | An app left open does not update until the person returns to it, which matters more once people expect live updates | A "pull now" signal from the server: platform push on Android, a long-lived connection on desktop. The protocol does not change |
| Hosting | Cold starts, and a database round trip on every request | An always-on server, which push also requires |
| Database | Connection limits | The same always-on server with a real pool |
| Login | Nothing of ours; abuse of sign-in is Clerk's problem | Clerk's paid plan above its free allowance |
| New device | First sync downloads every note | Already paged and resumable; add a progress display |
| Visibility | Logs stop being enough | Metrics, alerting and an error-tracking service |
| Attachments | Storage cost dominates | Paid tiers and stricter quotas |

## 12. Build order

1. **Trials for the unverified assumptions** in section 13. Any of them could
   change a decision, and finding out before building is cheap.
2. **Clean-up:** delete dead code that nothing live depends on, archive the
   stale docs, and close the stale issues. Code that a later step replaces is
   deleted by that step, and each archived document is written fresh by the
   step that defines its subject.
3. **New local core:** first remove the server's dependency on `dirt-core`,
   so that the server keeps building on `libsql`. Then build `dirt-proto`,
   CRDT notes and the per-account store. Desktop and CLI work locally on it,
   without sync.
4. **Server:** the Postgres schema, device registry, push and pull with the
   version check, quotas and Clerk verification.
5. **Encryption and sync for one device:** sign-in, the account key, the sync
   cycle and the worker, with the multi-device simulation tests. The tests
   give a second device the account key directly, because real enrolment
   arrives in step 6.
6. **More devices:** approval, the recovery phrase, the device list with
   removal, and account reset and deletion.
7. **Clients reconnected:** desktop first, then CLI, then Android.
8. **Hardening:** backups, key rotation, and the hosting and database plan
   decisions.
9. **Attachments.**

The app has no sync between steps 3 and 5. That is acceptable only because
there is no existing data to preserve. If real notes or a live deployment turn
out to exist, the plan gains a one-time step: export from the old app, then
import into the new one.

## 13. Unverified assumptions and open details

### To verify in step 1

Trials 1, 2 and 4 have passed. Trial 3 is part done. The write-ups are in
`docs/trials/` on the `trials/step-1` branch.

- **Trial 1, passed 2026-11-10:** 15.3 KB after 1,000 edits, about 15 bytes
  per edit.
- **Trial 2, passed 2026-11-11:** `rusqlite` builds, links and runs on
  Android, and two processes share one file safely given the two rules in
  section 8.4. No physical arm64 device was tested.
- **Trial 3, part done 2026-11-11:** sign-in from a desktop client works with
  an email code, and the server-side check accepts Clerk's ID token. Google
  sign-in and Android have not been run.
- **Trial 4, passed 2026-11-11:** every criterion met. It also showed that a
  polling timer keeps the database awake (section 7.4). Decision 4 was
  changed the same day as a result: sync is driven by events, with no timer.

| Assumption | Trial | If it fails |
|---|---|---|
| yrs documents stay small (verified) | Measure document size after about a thousand edits to a short note | Compare Loro and Automerge; revisit the sync shape for long notes |
| `rusqlite` builds for Android (verified) | Build and run a query in the mobile shell | Keep `libsql` for the local file |
| Clerk works from a native client (desktop verified; Android open) | Sign in from desktop (loopback) and Android (custom URL scheme) with a public PKCE client | Fall back to the existing email-code login |
| Postgres works from the Vercel Rust runtime (verified) | Deploy a function that runs one transaction against Neon's pooled endpoint | Reconsider the host before the database |

The trials need Rust 1.95 or newer. The development machine had 1.85 when this
was written.

### Confirmed since the design was approved

- Clerk's free plan covers 50,000 monthly retained users per app, per its
  pricing page.
- Vercel's free plan forbids commercial use (section 7.5).

### Details not yet decided

- The exact quota numbers.
- Purging deleted notes: when a deleted note's blob is finally removed, and
  what a device does if it still holds unseen edits for a purged note.
- Whether to pad blobs so their sizes reveal less.
- The flows a gap check found unspecified, such as what a pending device may
  do and how key rotation treats offline devices. The implementation plan
  lists them under the build step that must settle each one.

## 14. What is deleted, kept and rewritten

| Fate | Code |
|---|---|
| Deleted | `dirt-core/src/sync/*`, `dirt-core/src/auth/client.rs`, `dirt-api/src/email.rs`, `dirt-api/src/routes_auth.rs`, `dirt-api/src/rate_limit.rs`, `dirt-desktop/src/services/transcription.rs`, the media settings, `dirt-cli/src/config_profiles.rs`, both `sync_worker.rs` files |
| Rewritten against this design | The rest of `dirt-core` and `dirt-api`; the local and server schemas |
| Kept and reconnected | Desktop views and components, tray and hotkey; mobile views and the Android secure-storage code; CLI commands for add, list, search, edit, delete and export |

Deletion happens in the step that replaces each piece, not all at once. Only
the transcription and media code, which nothing depends on, goes in step 2.

## 15. Sources checked during the discussion

- Convex: [A Map of Sync](https://stack.convex.dev/a-map-of-sync),
  [An Object Sync Engine for Local-first Apps](https://stack.convex.dev/object-sync-engine),
  [convex crate](https://docs.rs/crate/convex/latest)
- Clerk: [Adding Clerk auth to your CLI](https://clerk.com/blog/adding-clerk-auth-to-your-cli),
  [How Clerk implements OAuth](https://clerk.com/docs/guides/configure/auth-strategies/oauth/how-clerk-implements-oauth),
  [pricing](https://clerk.com/pricing)
- Turso: [joining Supabase announcement](https://x.com/tursodatabase/status/2106054567125532976),
  [coverage of plan continuity](https://www.beri.net/article/supabase-turso-acquisition-database-per-agent-sqlite-postgres-pricing-continuity)
- Neon: [free plan limits](https://neon.com/faqs/free-plan-limits-and-quotas),
  [connection latency](https://neon.com/docs/connect/connection-latency)
- Cloudflare: [Workers limits](https://developers.cloudflare.com/workers/platform/limits),
  [Containers pricing](https://developers.cloudflare.com/containers/pricing/),
  [Hyperdrive with Postgres](https://developers.cloudflare.com/hyperdrive/configuration/connect-to-postgres)
