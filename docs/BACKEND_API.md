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
| Try again later | `server_error`, `rate_limited`, `key_changed`, `devices_changed`, `files_unavailable` | Back off and retry; show as offline. `key_changed` and `devices_changed` a device puts right itself and then asks again: see "Changing the account's key". `files_unavailable` concerns files only: notes go on syncing |
| Stop until signed in | `unauthenticated`, `device_revoked`, `device_expired`, `device_pending`, `sign_in_rejected`, `too_many_pending_devices`, `too_many_devices`, `recovery_rejected` | Stop syncing. Local notes are untouched |
| Update the app | `unsupported_protocol`, `invalid_request`, `payload_too_large` | Stop. Never overwrite what this version cannot read |
| Problem with one note | `not_found`, `note_too_large`, `quota_exceeded`, `file_too_large` | Flag that note and keep syncing the rest |

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
- An account may have three devices pending at a time. A fourth is refused
  with `too_many_pending_devices`.
- An account may have 20 approved devices. Signing in is never refused for
  that: the limit is applied when a device is approved (see "The approval
  exchange" and `POST /v1/recovery/claim`). A person who has lost every
  device can therefore always sign a new one in.

Registering is also when the server forgets what the account no longer needs:
a device removed more than 30 days ago, a request to join that was made more
than 30 days ago and never answered, and the oldest removed devices past 20.
A device forgotten this way is answered `unauthenticated` where it was
answered `device_revoked` or `device_expired`. Either way it signs in again
and is let in as a new device.

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
The device stays in the list, marked `revoked`, until the server forgets it
(see `POST /v1/devices`).

### The approval exchange

A pending device gets the account key from an approved one. The two devices
agree on a secret and on a short code that the person compares; the server
relays their messages and reads none of them. `docs/SECURITY.md` describes
the exchange itself.

Every call names the pending device. It may be made by that device or by an
approved device of the same account. To anyone else there is no such request:
another pending device is answered `device_pending`, another account
`not_found`. A pending device that has been removed is answered
`device_revoked` when it asks about its own request, also when the removal
falls while that request is being handled.

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

An account may have 20 approved devices. With that many, leaving a key is
answered `too_many_devices`, so the person at the approving device learns of
it and can remove one. `complete` is answered the same if the last place was
taken after the key was left; the key stays, and the device is let in once a
place is free.

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

An account that already has 20 approved devices takes up to 3 more this way,
and then answers `too_many_devices`. The person may have none of those
devices left; the device the phrase lets in is what removes them.

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

### Changing the account's key

An approved device replaces the account's key, so that a device removed
earlier holds a key that reads nothing written afterwards. The devices wrap
and open the key; the server stores the wrapped copies and opens none.
`docs/SECURITY.md` describes how a copy is wrapped.

**`POST /v1/account/key`** changes the key:

```json
{ "from_version": 1, "keys": [ { "device_id": "…", "wrapped": "<base64>" } ] }
```

`keys` holds the new key wrapped for every approved device of the account,
the caller included, and for no other. In one transaction the server stores
the copies, moves the account to version `from_version + 1`, counts every
note it holds as still under an older key, and removes the recovery copy and
anything an approval exchange has left for a waiting device. The answer is
`{ "key_version": 2, "old_key_notes": 14 }`.

- `key_changed` if the account is no longer at `from_version`: another
  device changed the key first.
- `devices_changed` if `keys` does not name exactly the approved devices.
  The caller fetches the device list and wraps the key again.

**`GET /v1/account/keys?after=V`** returns the calling device's copies of
the keys later than version `V`, oldest first:
`{ "keys": [ { "key_version": 2, "wrapped": "…" } ] }`. Each copy opens
with the key before it.

**`GET /v1/account/key/notes?after=N`** lists notes still stored under an
older key, up to 500 at a time in the order of their last change, with how
many there are in all:
`{ "key_version": 2, "notes": [ "…" ], "total": 14, "next_after": 812 }`.
`next_after` is present when the page was full: pass it as `after` for the
next page. A device sends the notes it holds again under the new key, and
looks at every page, because it may hold none of the notes on one. A push
that replaces such a note counts it off; at zero the change is complete.

Approving a device, by the approval exchange or the recovery phrase, and
changing the key lock the same account row, so one happens wholly before
the other. A change made first has removed the key left for the waiting
device, or the recovery copy, and the approval finds none. A change made
after must name the newly approved device.

While a key version is the account's present one:

- a pushed note under any other version is rejected with `key_changed`,
  which uses no change number;
- a key left for a waiting device, and a recovery copy, must carry that
  version, or the request is refused with `key_changed`.

Removing a device removes its wrapped copies. A reset starts the versions
again at 1.

The server keeps the copies of the 100 latest versions for each device. A
device opens the copies it missed one after another, so one that was away
through more than 100 changes of the key finds the copy that follows its own
key gone, and can no longer follow.

`GET /v1/account/keys?after=V` sees this: the account is past version `V`,
the device has a copy of the present key, and it has none of version
`V + 1`. The server then makes the device `pending` again, with a new seven
days to be answered, removes its copies, and answers `device_pending`. The
device is still the account's, and keeps its ID. It signs in again, learns
that it is waiting, and is let in by another device or the recovery phrase
with the present key, as a new device is. Notes it wrote while away are
still on it and are sent then.

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
- **rejected:** refused for a reason that concerns this note alone. With
  `key_changed` nothing is wrong with the note: the device fetches the
  account's new key and sends the note again under it.

A refused item uses no change number. A push may name each note only once.

A push onto a purged note is always `stale`, whatever `base_change` it
names, and `current` is the purged note's marker: the note's row with
`"ciphertext": null`. Nothing is ever stored in a purged note's place.

### `POST /v1/notes/purge`

Removes the blobs of notes that have been deleted for long enough. The
server cannot tell a deleted note from a live one, so a device says which,
and names the version of each that it holds:

```json
{ "items": [ { "note_id": "…", "base_change": 12 } ] }
```

Each item gets its own result, in order:

```json
{ "epoch": 1, "results": [
    { "note_id": "…", "status": "purged", "change": 13 },
    { "note_id": "…", "status": "stale", "current_change": 14 },
    { "note_id": "…", "status": "rejected", "code": "quota_exceeded", "message": "…" } ] }
```

- **purged:** the server held exactly the version named, and its blob is
  gone. The note's row stays, with no blob and the account's next change
  number, so that every device learns of the purge by pulling. The note no
  longer counts against the account's notes or its bytes. A note that was
  already purged is answered the same, with the marker's change number.
- **stale:** the note changed after the version named, or the server has no
  such note (`current_change` is then `null`). Nothing is purged. A device
  that edited the note without knowing of the delete may have brought it
  back.
- **rejected:** with `quota_exceeded`, the account has purged as many notes
  as it may.

A purge may name up to 500 notes, each once, and each with a `base_change`
above zero. A device asks only for notes it has itself seen deleted for
thirty days, with nothing unsent. That rule is the client's: the server
would purge whatever an approved device named, as it would store whatever
one pushed.

**A device that is told of a purge** removes its copy of the note. If the
copy holds a change the device has not sent and the note is not deleted on
that device, the person wrote something there that no other device has
seen. The device keeps the text as a new note, with a new ID, which syncs
like any other.

### Files

A file attached to a note is encrypted on the device and kept in object
storage, not in the database. The server never handles its bytes: it signs
a link, and the device sends to the storage or fetches from it directly.
The server knows that a file exists, how large it is and which notes refer
to it. It does not know its name, its type, its key or its contents: those
are in the note's document, which it cannot read.

A server with no storage answers every call here `files_unavailable`. So
does one whose storage cannot be reached when it has to be asked. Notes are
not affected.

**`POST /v1/files`** asks for a link to upload one file:

```json
{ "file_id": "…", "size": 48213 }
```

`file_id` is made by the device. `size` is the size of the encrypted bytes.
The answer is `{ "url": "…", "expires_in_secs": 900 }`: one `PUT` of
exactly `size` bytes to `url`. The length is part of what the link's
signature covers, so the storage refuses a body of any other length.

- The size is counted against the account from this moment, whether or not
  anything is uploaded.
- Asking again for the same file at the same size gives a new link and
  counts nothing more. That is how an upload is tried again. Another size
  is `invalid_request`: a file never changes.
- `file_too_large` over 10 MiB; `quota_exceeded` when the account's files
  would pass 200 MiB, or when it has 20,000 files already, whatever their
  size. A file that is refused is not counted.
- If no note ever refers to the file, it is discarded after seven days.

**`GET /v1/files/{id}`** answers `{ "url": "…", "size": 48213,
"expires_in_secs": 300 }`: one `GET` of the file's bytes. `not_found` if the
account has no such file.

**`POST /v1/files/confirm`** says that notes refer to files, and
**`POST /v1/files/release`** that they no longer do. The server cannot read
a note, so devices tell it:

```json
{ "items": [ { "file_id": "…", "note_id": "…" } ] }
```

Each item gets its own result, in order:
`{ "results": [ { "file_id": "…", "note_id": "…", "status": "done" } ] }`.

| Status | Meaning |
|---|---|
| `done` | Confirmed, or released. Either may be repeated |
| `not_uploaded` | The server knows the file but the storage does not hold it at the size asked for. The device uploads it again |
| `no_such_file` | The server does not know the file: never asked for, discarded, or lost to a restore. A device that holds the file asks for a link and uploads it again |
| `no_such_note` | On confirming: the server does not hold the note, or it was purged. A device tries again once the note has synced |
| `rejected` | With `quota_exceeded`: the note already refers to 20 files, or the account's notes to 40,000 between them. A reference the server already has is never refused, so a device can always confirm again what it holds |

Before a file is confirmed for the first time the server asks the storage
whether it is there, at the size asked for. A file is counted as present
only after that.

A file may be confirmed for more than one note. Step 8e keeps an unsent
edit to a purged note as a new note, and that note keeps the files: the
device confirms them for it. Two devices may each do so. The file is
counted against the account once.

**A file is kept for as long as any note refers to it,** and for thirty
days after the last one stops, by being purged or by a release. Confirming
it for a note in that time keeps it. A device releases a file only when it
has itself seen the note without it for thirty days, with nothing unsent,
as it purges a note.

**Deleting.** Nothing is deleted from the storage in the request that
decides it. The decision is a date on the file. What is due is dealt with
when the account next asks for an upload link, and for every account once a
day, in two steps:

1. The file's row is removed and its object queued, together. From then on
   the server says the file is gone: `not_found` for a download link,
   `no_such_file` for a confirm.
2. The queued object is deleted from the storage, an hour or more after it
   was queued. If the storage cannot be reached, it stays queued and is
   tried again.

The hour is for upload links. One may have been issued just before the row
went, and a link works for fifteen minutes. The storage knows nothing of
the server's rows, so an upload that arrived after the object had been
deleted would put it back with nothing to say it was there. After an hour
no link for the object can work and no upload begun under one can still be
running.

So the server never says a file is there when its object is gone, and a
storage that cannot be reached delays a deletion and loses none. Deleting
or resetting an account removes its files' rows at once and queues the
objects.

A file's ID is its place in the storage. A device may ask to upload a file
whose old object is still queued: after a reset, the device that remains
sends its files again under the IDs its notes name. Asking takes the object
out of the queue, so that its deletion cannot overtake the new upload.

**What a device does with these.** After its notes have synced, a device
confirms every reference the server has not taken, for every note it holds,
whether or not it holds the file. If the answer is `not_uploaded` or
`no_such_file` and the device holds the file, it asks for a link, uploads
and confirms again; if it does not hold the file it asks again at its next
cycle. No device keeps a record of what it has uploaded: the server is
asked. A new epoch makes a device confirm everything again. A request to
the storage carries the signed link and nothing else of the device's.

A device asks for a download link when a file is wanted and it does not
hold it: the person opens or saves the file, a picture's thumbnail is to be
shown, or an export is made, which wants every file of every note. A
fetched file is kept, so each is asked for once. An export stops asking at
the first request that does not get through, to the server or to the
storage, and says which files it lacks.

**`GET /v1/maintenance/sweep`** is the daily request. It must carry
`Authorization: Bearer <CRON_SECRET>`; to anyone else, and on a server
given no secret, there is no such address. It answers `{ "deleted": 3 }`.

### `GET /v1/notes/pull?after=N`

Blobs changed after change number `N`, lowest first:

```json
{ "epoch": 1, "items": [ { "note_id": "…", "change": 12, "key_version": 1, "format_version": 1,
                           "ciphertext": "…" } ],
  "next_after": 12, "more": false, "pending_devices": 0 }
```

A page holds notes for as long as their blobs fit in the batch limit
together, and always at least one. `more` says whether to pull again from
`next_after`. A purged note has `"ciphertext": null`; see
`POST /v1/notes/purge` for what a device does with one.

Two fields say what needs the person's attention, so that a device learns of
it at its next sync: `pending_devices` is how many devices are waiting to be
approved, and `reset_effective_at_ms` is present while a reset has been asked
for.

Two more follow a change of the account's key: `key_version` is the version
notes are written under now, and `old_key_notes` is how many notes are still
stored under an older one. `GET /v1/devices/me` reports `key_version` too.

## Limits

`docs/LIMITS.md` gives the reason for each number and what was measured.

| Limit | Value |
|---|---|
| One note's blob | 1 MiB |
| Blobs in one push or one pull page | 3 MB |
| Notes in one push | 500 |
| Notes per account | 20,000. A deleted note counts until it is purged |
| Purged notes per account | 100,000 |
| A deleted note is purged | 30 days after a device saw it deleted |
| One file | 10 MiB |
| Files per account | 200 MiB, and 20,000 files however small |
| Files a note refers to | 20 |
| References from an account's notes to files | 40,000 |
| An upload no note confirmed | discarded after 7 days |
| A file no note refers to | deleted after 30 days |
| An upload link, a download link | good for 15 minutes, 5 minutes |
| Blobs per account | 50 MiB |
| Requests per account | 600 in ten minutes |
| Devices waiting for approval | 3 per account |
| Approved devices | 20 per account, and 3 more by the recovery phrase |
| A removed device, or an unanswered request to join | forgotten after 30 days |
| Removed devices remembered | 20 per account |
| Copies of later account keys kept for a device | the 100 latest versions. A device further behind waits to be let in again |
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

Nothing: every call the redesign's nine steps planned is above. What the
owner has still to decide before the server is public is in
`docs/OWNER_DECISIONS.md`.
