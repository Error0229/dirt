# Dirt server API

The protocol between a Dirt client and `dirt-api`. The types are defined once,
in `crates/dirt-proto/src/api.rs`, and shared by both sides; this document
describes them for readers.

The server stores each note as an encrypted blob it cannot read. It checks who
is calling, keeps blobs in order, and enforces limits. It never merges, and
it never sees note text, tags or titles.

## Conventions

- **Bodies** are JSON. Binary values (public keys and blobs) are base64 text.
- **Protocol version:** a client sends `Dirt-Protocol: 1`. A server that sees
  a higher number than it knows answers `426` with the code
  `unsupported_protocol`. A request with no version is read as version 1.
- **Authentication:** `Authorization: Bearer <device token>` on every endpoint
  except health, nonces and registration.
- **Account epoch:** every authenticated response carries
  `Dirt-Account-Epoch`. When the number changes, the account was reset or
  restored from a backup, and the client must forget its cursor and the
  change numbers it has seen, then merge everything again.

## Errors

Every error is this body, with a matching HTTP status:

```json
{ "code": "quota_exceeded", "message": "the account's storage is full", "retry_after_secs": 30 }
```

`retry_after_secs` is present only when the server knows how long to wait.
A client acts on the code's class and nothing else.

| Class | Codes | What the client does |
|---|---|---|
| Try again later | `server_error`, `rate_limited` | Back off and retry; show as offline |
| Stop until signed in | `unauthenticated`, `device_revoked`, `device_expired`, `device_pending`, `sign_in_rejected`, `too_many_pending_devices` | Stop syncing. Local notes are untouched |
| Update the app | `unsupported_protocol`, `invalid_request`, `payload_too_large` | Stop. Never overwrite what this version cannot read |
| Problem with one note | `not_found`, `note_too_large`, `quota_exceeded` | Flag that note and keep syncing the rest |

## Endpoints

### `GET /healthz`

Answers `{"ok": true, "protocol": 1}` without touching the database.

### `POST /v1/nonces`

Returns a single-use value for the Clerk sign-in:

```json
{ "nonce": "…", "expires_in_secs": 600 }
```

The client passes it to Clerk as the OpenID Connect `nonce`. Clerk's ID token
is valid for a day; binding it to a value that works once means a copied
token cannot register another device.

### `POST /v1/devices`

Registers the device that just signed in.

```json
{ "id_token": "…", "nonce": "…", "name": "Laptop", "public_key": "<base64, 32 bytes>" }
```

The server checks the ID token against Clerk's published keys: RS256, the
configured issuer, the configured client ID, not expired, and carrying the
nonce. It then spends the nonce.

```json
{ "account_id": "…", "device_id": "…", "token": "…", "status": "approved", "first_device": true, "epoch": 1 }
```

- `token` is shown this once. The server keeps only its hash.
- The first device an account ever has is `approved` and has
  `first_device: true`: it must create the account key. Every later device is
  `pending` until an approved device hands it the key.
- An account may have three devices pending at a time.

### `GET /v1/devices/me`

The calling device's own entry. This is the only call a pending device may
make; it uses it to learn that it has been approved.

### `GET /v1/devices`

The account's devices, for an approved device:

```json
{ "devices": [ { "id": "…", "name": "Laptop", "public_key": "…", "status": "approved",
                 "created_at_ms": 0, "last_seen_ms": 0, "current": true } ] }
```

### `DELETE /v1/devices/{id}`

Removes a device from the account. Its token stops working with its next
request. A device in another account is reported as not found.

### `POST /v1/notes/push`

```json
{ "items": [ { "note_id": "…", "base_change": 0, "key_version": 1, "format_version": 1,
               "ciphertext": "<base64>" } ] }
```

`base_change` is the change number of the version this blob was made from, or
zero for a note the server has never had. Each item gets its own result, in
order:

```json
{ "epoch": 1, "results": [
    { "note_id": "…", "status": "accepted", "change": 12 },
    { "note_id": "…", "status": "stale", "current": { "note_id": "…", "change": 9, "key_version": 1,
                                                        "format_version": 1, "ciphertext": "…" } },
    { "note_id": "…", "status": "rejected", "code": "note_too_large", "message": "…" } ] }
```

- **accepted:** stored, with the note's new change number.
- **stale:** somebody wrote the note first. `current` is what the server
  holds, so the client can merge and push again without a separate pull. It
  is `null` when the base names a note the server does not have.
- **rejected:** refused for a reason that concerns this note alone.

A refused item uses no change number.

### `GET /v1/notes/pull?after=N`

Blobs changed after change number `N`, lowest first:

```json
{ "epoch": 1, "items": [ { "note_id": "…", "change": 12, "key_version": 1, "format_version": 1,
                           "ciphertext": "…" } ],
  "next_after": 12, "more": false }
```

A page holds notes until their blobs add up to the batch limit; the first
note of a page is always included. `more` says whether to pull again from
`next_after`. A purged note has `"ciphertext": null`.

## Limits

| Limit | Value |
|---|---|
| One note's blob | 1 MiB |
| Blobs in one push or one pull page | 3 MB |
| Notes in one push | 500 |
| Notes per account | 20,000 |
| Blobs per account | 50 MiB |
| Requests per account | 600 in ten minutes |
| Devices waiting for approval | 3 per account |
| Device token | stops working after 90 days without use |

**When an account is full,** a push that grows a note by more than 1 KiB is
rejected with `quota_exceeded`. Smaller growth is accepted up to 105% of the
quota. The server cannot tell a delete from an edit, and a delete adds a small
marker to the note's document, so this rule is what lets deletes through.

## Not built yet

Step 6 of the redesign adds the endpoints for approving a device, for the
recovery phrase, and for resetting or deleting an account.
