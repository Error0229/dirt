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
| Stop until signed in | `unauthenticated`, `device_revoked`, `device_expired`, `device_pending`, `sign_in_rejected`, `too_many_pending_devices`, `recovery_rejected` | Stop syncing. Local notes are untouched |
| Update the app | `unsupported_protocol`, `invalid_request`, `payload_too_large` | Stop. Never overwrite what this version cannot read |
| Problem with one note | `not_found`, `note_too_large`, `quota_exceeded` | Flag that note and keep syncing the rest |

## Endpoints

### `GET /healthz`

Answers `{"ok": true, "protocol": 1}` without touching the database.

### `POST /v1/nonces`

Returns a single-use value for the Clerk sign-in:

```json
{ "nonce": "…", "expires_in_secs": 600,
  "issuer": "https://….clerk.accounts.dev", "client_id": "…" }
```

`issuer` and `client_id` say where to sign in. A client is configured with
the server's address only and learns the identity provider here, so the two
cannot disagree about it.

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

A device whose token expired signs in again with one more field,
`"previous_token": "…"`, holding the token it had. If the account has a
device with that token and that public key that has not been revoked, the
answer carries the same `device_id` and `status`, a new `token`, and
`first_device: false`; the old token stops working. Otherwise the request is
treated as a first sign-in. The public key alone identifies nothing, since
every approved device of the account can read it.

### `GET /v1/devices/me`

The calling device's own entry, with what it needs to know about its account:

```json
{ "id": "…", "name": "Phone", "public_key": "…", "status": "pending",
  "created_at_ms": 0, "last_seen_ms": 0, "current": true,
  "approvers": 1, "recovery_available": false }
```

- `approvers` is how many other devices of the account are approved: the
  devices that could approve this one, or that keep the account's key if this
  one signs out.
- `recovery_available` says whether the account has a recovery copy.
- `reset_effective_at_ms` is present while a reset has been asked for.

A pending device uses this to learn that it has been approved and what the
person's options are.

A request to join lapses seven days after the device registered. The device
is then answered `device_expired`, no longer counts against the limit of
three, and signing in again renews it.

### `GET /v1/devices`

The account's devices, for an approved device:

```json
{ "devices": [ { "id": "…", "name": "Laptop", "public_key": "…", "status": "approved",
                 "created_at_ms": 0, "last_seen_ms": 0, "current": true } ] }
```

### `DELETE /v1/devices/{id}`

Removes a device from the account. Its token stops working with its next
request. A device in another account is reported as not found.

An approved device may remove any device of its account. A pending device may
remove only itself, which is what signing out of it does.

Removing a device ends its approval exchange and any reset it asked for.

### The approval exchange

A pending device gets the account key from an approved one. The two devices
agree on a secret and on a short code that the person compares; the server
relays their messages and reads none of them. `docs/SECURITY.md` describes
the exchange itself.

Every call names the pending device. It may be made by that device or by an
approved device of the same account. To anyone else there is no such request:
another pending device is answered `device_pending`, another account
`not_found`.

**`POST /v1/enrolments/{device_id}/messages`** adds a message of up to 1 KiB:

```json
{ "body": "<base64>", "restart": false }
```

The answer is the message as relayed, `{ "seq": 3, "sender": "…", "body": "…" }`.
The server numbers messages and records who sent each. With `"restart": true`,
which only the pending device may send, the earlier messages and any key left
for it are dropped first. Numbers keep rising across a restart. An exchange
holds at most 32 messages.

**`GET /v1/enrolments/{device_id}/messages?after=N`** returns
`{ "messages": [ … ] }`, the messages numbered above `N`.

**`PUT /v1/enrolments/{device_id}/key`**, from an approved device, leaves the
account key encrypted for the pending device:

```json
{ "key_version": 1, "wrapped": "<base64, up to 256 bytes>" }
```

**`GET /v1/enrolments/{device_id}/key`**, from the pending device, returns it,
or `not_found` while none has been left.

**`POST /v1/enrolments/{device_id}/complete`**, from the pending device, says
it has opened and saved the key. Only now does the server mark it approved; it
deletes the key and the messages. A device is therefore never approved
without the key, and one that stopped half-way can begin again. Without a key
left by an approved device the answer is `not_found`.

### `PUT /v1/recovery`

From an approved device. Stores the recovery copy, replacing any earlier one:

```json
{ "key_version": 1, "wrapped": "<base64>", "claim_hash": "<base64, 32 bytes>" }
```

`wrapped` is the account key encrypted under a key derived from the person's
recovery phrase. `claim_hash` is the SHA-256 hash of a claim code derived from
the same phrase. Neither lets the server open the key.

### `GET /v1/recovery`

From any device of the account, pending ones included. Returns
`{ "key_version": 1, "wrapped": "…" }`, or `not_found`.

### `DELETE /v1/recovery`

From an approved device. A pending device may not create, replace or delete
the copy: someone who took over the person's email gets a pending device, and
must not be able to destroy the way back in or to choose the claim code.

### `POST /v1/recovery/claim`

From a pending device that knows the phrase:

```json
{ "claim_code": "<base64>" }
```

If the code's hash is the stored one, the device is approved. Otherwise the
answer is `recovery_rejected` and nothing changes.

### `DELETE /v1/account`

From an approved device. Deletes everything the server holds for the account.
Its devices are answered `unauthenticated` from then on. The same person
signing in afterwards starts a new account.

### `POST /v1/account/reset`

From any device of the account. A reset deletes the account's notes, its
recovery copy and every other device; the calling device becomes approved and
is the account's first device again; the epoch rises.

```json
{ "done": true, "epoch": 2 }
```

- An approved device resets at once.
- A pending device resets at once if the account holds no notes.
- Otherwise a pending device can only ask. The answer is
  `{ "done": false, "effective_at_ms": …, "epoch": 1 }`, and the same call
  made by the same device after that time performs the reset. Asking again
  does not restart the wait, and another pending device neither replaces the
  request nor inherits it.

The wait is seven days. It exists because a pending device is all that someone
who took over the person's email can get.

### `DELETE /v1/account/reset`

From an approved device. Cancels a reset that was asked for.

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
    { "note_id": "…", "status": "stale", "current_change": 9,
      "current": { "note_id": "…", "change": 9, "key_version": 1, "format_version": 1, "ciphertext": "…" } },
    { "note_id": "…", "status": "rejected", "code": "note_too_large", "message": "…" } ] }
```

- **accepted:** stored, with the note's new change number.
- **stale:** somebody wrote the note first. `current_change` is the change
  number the server holds, or `null` if it has no such note. `current` is the
  server's copy, so the client can merge and push again without a separate
  pull. The copies returned in one response are held to the batch limit;
  past it `current` is `null` and the client gets the copy from its next
  pull.
- **rejected:** refused for a reason that concerns this note alone.

A refused item uses no change number. A push may name each note only once.

### `GET /v1/notes/pull?after=N`

Blobs changed after change number `N`, lowest first:

```json
{ "epoch": 1, "items": [ { "note_id": "…", "change": 12, "key_version": 1, "format_version": 1,
                           "ciphertext": "…" } ],
  "next_after": 12, "more": false, "pending_devices": 0 }
```

A page holds notes for as long as their blobs fit in the batch limit
together, and always at least one. `more` says whether to pull again from
`next_after`. A purged note has `"ciphertext": null`.

Two fields say what needs the person's attention, so that a device learns of
it at its next sync: `pending_devices` is how many devices are waiting to be
approved, and `reset_effective_at_ms` is present while a reset has been asked
for.

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
| A request to join | lapses after 7 days |
| Messages in one approval exchange | 32, of 1 KiB each |
| An encrypted copy of the account key | 256 bytes |
| A reset asked for by a pending device | takes effect after 7 days |
| Device token | stops working after 90 days without use |

**When an account is full,** a push that grows a note by more than 1 KiB is
rejected with `quota_exceeded`. Smaller growth is accepted up to 105% of the
quota. The server cannot tell a delete from an edit, and a delete adds a small
marker to the note's document, so this rule is what lets deletes through.

## Not built yet

Changing the account key, which is what finally shuts out a device that was
removed, is step 8 of the redesign. Attachments are step 9.
