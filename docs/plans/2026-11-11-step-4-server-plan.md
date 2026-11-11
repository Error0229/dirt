---
title: "Step 4: server on Postgres"
type: plan
date: 2026-11-11
status: done
parent: docs/plans/2026-11-10-refactor-system-redesign-implementation-plan.md
tracking: "#240"
---

# Step 4: server on Postgres

## Goal

Replace `dirt-api` with the server the design describes: it verifies a Clerk
sign-in once per device, keeps the device registry, stores encrypted note
blobs with a version check, and enforces limits. It reads no note, merges
nothing and sends no email.

Design sections 5.2, 6.3, 6.4 and 7 are the specification. Trial 4 proved
the database and hosting choices, and trial 3 the token check.

## State before this step

- `dirt-api` is 4,700 lines: email-code login, sessions with refresh, an
  in-memory rate limiter, and push and pull of plain-text notes on Turso.
  Every one of its modules is replaced.
- It shares only `NoteId` with the clients, through `dirt-proto`.
- No client in the repository calls its note endpoints since step 3. The
  account screens still call its login endpoints, on the deployed copy.
- The deployed copy is not touched by this step. Vercel builds it only when
  someone deploys by hand.

## Decisions

The parent plan's "Settle first" items:

| Question | Decision |
|---|---|
| The account epoch | A number on the account, sent with every authenticated response in the `Dirt-Account-Epoch` header and in the push and pull bodies. Nothing changes it in this step; account reset, which does, is built in step 6 |
| The first device | The account has a "key established" flag. Registration sets it with one conditional update. The device whose update wins is approved at once and creates the account key; every other device starts as pending. If every device is later lost, the flag stays set, so a new device is pending and needs the recovery phrase or a reset |
| Purged notes | The `ciphertext` column may be null and pull returns such a row as purged. Nothing purges yet: when to purge is still undecided in the design |
| When storage is full | A push that grows a note by more than 1 KiB is refused once the account is at its quota. Smaller growth is accepted up to 105% of the quota, so a delete, which adds a small marker, always goes through. The server cannot see what a push contains, so it cannot let deletes through by name |
| Quota numbers | 1 MiB per note, 20,000 notes and 50 MiB per account. A push batch and a pull page are capped at 3 MB of ciphertext |

Decisions this plan adds:

| Question | Decision |
|---|---|
| Enrolment and recovery endpoints | Built in step 6 with the exchange they carry. Their message shapes depend on the cryptography chosen there. This step builds devices, notes, the nonce and health |
| Account reset and deletion | Step 6, as amendment C2 says |
| Blobs on the wire | JSON with base64. A 3 MB batch becomes about 4 MB of JSON, inside Vercel's 4.5 MB limit |
| The sign-in nonce | `POST /v1/nonces` returns a random value valid for ten minutes. The server keeps only its hash, and registration deletes it, so it works once |
| The device token | 32 random bytes. The server keeps only the SHA-256 hash. It stops working after 90 days without use |
| Request counts | One row per account per ten-minute window, raised on each authenticated request. 600 requests per window |
| Protocol version | Clients send `Dirt-Protocol: 1`. A higher number than the server knows is refused with a code that tells the client to update |
| Clerk's keys | Fetched from the issuer's published address and kept in memory for an hour. An unknown key ID triggers one refetch. Tests supply a local key and never use the network |
| Tests that need Postgres | Integration tests under `crates/dirt-api/tests/`, each in its own throwaway database. CI runs them on Linux with a Postgres 18 service; the macOS and Windows jobs run the rest of the workspace |
| Migrations | SQL files applied by `dirt-api migrate`, run at deploy time against the direct endpoint. The server never migrates at start-up |

## Result (2026-11-11)

The server is rewritten, its tests pass, and it runs on the `dirt-trial`
Vercel project against the `dirt-trial` Neon database.

**The deployed server was checked end to end** with a real Clerk sign-in for
the test user: a nonce from the server, the sign-in in a browser, registration
(approved, first device), the same nonce refused a second time, a push
accepted with change number 1, the same push refused as stale with the
server's copy returned, the blob pulled back unchanged, a bad token and no
token both refused, and the device's token refused after it was revoked. The
account was deleted from the trial database afterwards.

**Deploying turned up one fault,** fixed in the same pull request:
`.vercelignore` replaces `.gitignore` for uploads and named only three env
files, so a local file holding connection strings would have been uploaded.
It now excludes every `.env*` file.

**Review of the pull request found two faults in the protocol,** both fixed
before merging:

- A pull page could pass the 3 MB limit by one note, because the size check
  did not count the note being added.
- A push with many stale items was answered with the server's copy of each,
  with no bound. A push now names each note once, and the copies returned
  are held to the batch limit; past it a stale answer carries only the change
  number, and the client gets the copy from its next pull.

Where the work differed from this plan:

- **`rusqlite` moved from 0.42 to 0.39.** `sqlx` 0.9 lists an optional SQLite
  driver that needs `libsqlite3-sys` below 0.38, and Cargo allows one version
  of that crate in a workspace even when the driver is off. `rusqlite` 0.39 is
  the newest release that fits. Trial 2 ran on 0.42; the store's tests,
  including the two-process tests, pass on 0.39.
- **Push is several statements under one lock, not trial 4's single
  statement.** The first statement locks the account row, so the rest of the
  transaction can read the note, check the quotas and write, in plain steps.
  The concurrency tests from the trial pass against it, and fail when the lock
  is removed.
- **`sqlx` 0.9 accepts only string literals as SQL,** which rules out building
  a query with `format!`. Shared column lists are a macro.
- **The old server had 56 tests; the new one has 46,** of which 35 need
  Postgres. CI runs those in a Linux job with a Postgres 18 service.

## Protocol

Types live in `dirt-proto` and are shared with the clients.

| Endpoint | Who may call | Purpose |
|---|---|---|
| `GET /healthz` | anyone | Liveness; touches no database |
| `POST /v1/nonces` | anyone | A single-use value to put in the Clerk sign-in |
| `POST /v1/devices` | holder of a Clerk ID token | Register this device; returns its token and status |
| `GET /v1/devices` | approved device | The account's devices |
| `DELETE /v1/devices/{id}` | approved device | Revoke a device |
| `POST /v1/notes/push` | approved device | Store blobs, each with the change number it was based on |
| `GET /v1/notes/pull?after=N` | approved device | Blobs changed after N, in order |

A pending device may call nothing but `GET /v1/devices/me`, which tells it
whether it has been approved.

Errors are a code, a message and an optional retry delay. Each code belongs
to one of the design's four classes, and the class decides what a client
does.

## Schema

```sql
CREATE TABLE accounts (
  id               uuid PRIMARY KEY,
  clerk_issuer     text NOT NULL,
  clerk_user_id    text NOT NULL,
  email            text,
  change_counter   bigint NOT NULL DEFAULT 0,
  epoch            integer NOT NULL DEFAULT 1,
  key_version      integer NOT NULL DEFAULT 1,
  key_established  boolean NOT NULL DEFAULT false,
  note_count       bigint NOT NULL DEFAULT 0,
  bytes_used       bigint NOT NULL DEFAULT 0,
  created_at       timestamptz NOT NULL DEFAULT now(),
  UNIQUE (clerk_issuer, clerk_user_id)
);

CREATE TABLE devices (
  id          uuid PRIMARY KEY,
  account_id  uuid NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  name        text NOT NULL,
  public_key  bytea NOT NULL,
  token_hash  bytea NOT NULL UNIQUE,
  status      text NOT NULL CHECK (status IN ('pending', 'approved', 'revoked')),
  created_at  timestamptz NOT NULL DEFAULT now(),
  last_seen   timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE notes (
  account_id      uuid NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  note_id         uuid NOT NULL,
  change_number   bigint NOT NULL,
  key_version     integer NOT NULL,
  format_version  integer NOT NULL,
  ciphertext      bytea,                 -- null once purged
  PRIMARY KEY (account_id, note_id)
);
CREATE INDEX notes_by_change ON notes (account_id, change_number);

CREATE TABLE nonces (
  nonce_hash  bytea PRIMARY KEY,
  expires_at  timestamptz NOT NULL
);

CREATE TABLE request_counts (
  account_id    uuid NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  window_start  timestamptz NOT NULL,
  requests      integer NOT NULL,
  PRIMARY KEY (account_id, window_start)
);
```

## Work

- [x] `dirt-proto`: request, response and error types, the error classes, and
      the limits both sides must agree on.
- [x] `dirt-api`, rewritten:
  - configuration from the environment;
  - the pool, built without connecting, as in trial 4;
  - the token check from trial 3, with its tests on a local key;
  - devices: nonce, registration, list, revoke, and the status rules;
  - notes: push with the version check and quotas, and pull in pages;
  - request counts;
  - `dirt-api migrate`.
- [x] Delete `routes_auth.rs`, `email.rs`, `rate_limit.rs`, `turso.rs` and the
      old `auth.rs`, `routes.rs` and `error.rs`. Remove `libsql` from the
      workspace.
- [x] `compose.yaml` at the repository root with Postgres 18, and a Postgres
      service in CI.
- [x] `docs/BACKEND_API.md` and `docs/DEPLOY.md`, written fresh.
      `.env.server.example` for the new variables. `security-guard.sh` guards
      the new server-only names.
- [x] Deploy to the `dirt-trial` Vercel project against the `dirt-trial` Neon
      database, and run a registration, a push and a pull against it. The live
      `dirt-api` project is left alone.

## Tests

Each of these is an exit condition.

- [x] The version check under concurrency: of many pushes on one base, one is
      accepted; pushes to different notes take consecutive change numbers.
- [x] Account isolation: one account can never read, overwrite or count
      against another's notes, including with a guessed note ID.
- [x] Device rights: a pending device and a revoked device can read and write
      nothing; an expired token is refused; revoking takes effect at once.
- [x] Registration: the nonce works once and expires; a token for another
      client, a bad signature and an expired token are refused; two devices
      registering at the same moment produce exactly one approved device.
- [x] Quotas: note size, note count, account bytes with the 1 KiB rule, batch
      size, and the request limit.
- [x] A refused push returns the current blob.

## Exit

- [ ] Every test above passes locally and in CI.
- [x] `libsql` is gone from the workspace.
- [x] The server answers on a preview deployment.
