---
title: "Step 5: encryption and sync for one device"
type: plan
date: 2026-11-11
status: in progress
parent: docs/plans/2026-11-10-refactor-system-redesign-implementation-plan.md
tracking: "#241"
---

# Step 5: encryption and sync for one device

## Goal

Connect the clients to the new server. A person signs in through Clerk, the
device registers, and notes sync: encrypted on the device, stored as blobs
the server cannot read, merged on every device that holds the account key.

This step makes one device sync end to end and proves, in tests, that several
devices converge. Handing the key to a second device is step 6; until then a
second device is approved and given the key by the tests themselves
(amendment C3).

Design sections 5, 6.3, 6.4, 8 and 9 are the specification.

## State before this step

- The clients run on the local store from step 3. Each note has a local
  revision counter, a "revision last pushed" and a "last change number seen",
  all written and none yet used.
- The server from step 4 answers on the `dirt-trial` Vercel project.
- `dirt-core` still holds the old email-code sign-in (`auth/`) and the old
  HTTP clients (`sync/`). The account screens use them. Nothing syncs.

## Decisions

The parent plan's "Settle first" items, at their defaults:

| Question | Decision |
|---|---|
| A blob the client cannot read | That note is held: it is flagged with the reason, shown as a problem, and never pushed over. Every other note keeps syncing. Only a server that speaks a newer protocol stops sync as a whole |
| Expired and revoked tokens | An expired device signs in again and is the same device: it sends its old token and its public key, the server finds the device by both within the account, gives it a new token and keeps its status. The public key alone is not accepted, because every approved device can read it: it would let anyone who can sign in to the account skip approval. A revoked device signs in as a new, pending device |
| Desktop and CLI on one machine | Both sync. The store's write lock already keeps them from interleaving. The CLI runs one sync cycle after each command that writes, and `dirt sync` runs one on demand. The desktop app reloads its list when it regains focus, which is also when it syncs |
| The sign-out guard | Signing out of the only approved device is refused with an explanation, because it would destroy the last copy of the account key. It becomes possible once a recovery phrase exists (step 6) |

Decisions this plan adds:

| Question | Decision |
|---|---|
| Cipher for note blobs | XChaCha20-Poly1305 from the `chacha20poly1305` crate, with a random 24-byte nonce per blob. The account ID, note ID, key version and format version are the associated data, so a blob moved to another note or account fails to decrypt |
| Blob layout | One version byte, the nonce, then the ciphertext with its tag. The plaintext is the note's document in the yrs v2 encoding, which is format version 1 |
| Device key pair | X25519 from `x25519-dalek`, generated at first sign-in. Step 5 only registers the public half; step 6 uses the pair |
| What "changed locally" means | A note is unsent while its revision differs from the revision last pushed. Only local edits raise the revision. Merging a pulled document does not, so a pull never makes a note look unsent |
| Replay | A pulled or returned blob whose change number is lower than the one already seen for that note is refused and the note is flagged |
| A new account epoch | The cursor and every "last change number seen" are reset and every note is marked unsent. The next cycle pulls everything, merges, and pushes |
| The lost-state check | Before merging a pulled document, the store checks whether it holds more of this install's edits than the local copy. If so the install takes a new CRDT client ID first (amendment B3) |
| Where secrets live | One record per account in the operating system's keychain on desktop and CLI, shared by both; Android's encrypted preferences on Android; memory in tests. The record holds the device token, the device key pair and the account key |
| The transport | The sync code talks to a small trait, not to `reqwest` directly. Production uses `reqwest`; the simulation tests plug the real server in, in the same process |
| The simulation harness | A crate of its own, `dirt-sim`, that depends on both `dirt-core` and `dirt-api`. Neither of those may depend on the other |

## Work, in two pull requests

### 5a: the core, with no interface

- [x] `dirt-core::crypto`: the account key, note encryption and decryption
      with associated data, and the device key pair.
- [x] `dirt-core::store`: unsent notes, marking a note as pushed only if its
      revision has not moved, applying a remote document without marking it
      unsent, replay refusal, the cursor and the epoch, note problems, and the
      client-ID change.
- [x] `dirt-core::api`: the client for the server's protocol, over the
      transport trait, mapping every error to its class.
- [x] `dirt-core::sync`: one cycle of pull, decrypt, merge, then encrypt and
      push, with a refused push merged and retried.
- [x] `dirt-sim`: the real server in-process on Postgres, with two or three
      real client cores. The tests listed below.
- [x] Server: an expired device that signs in again keeps its identity.

**Result (2026-11-11): done.** What building it showed:

- **A damaged blob held the note on the device that wrote it.** A device
  pulls its own pushes back once. If the server's copy of one was damaged,
  the device flagged a note whose good text it was holding. The cycle now
  skips a pulled blob whose change number is the one the device already
  recorded for that note: it has nothing to learn from it.
- **Closing the store left the write-ahead log unfolded.** SQLite folds the
  log into the database file when the last connection closes, but a
  read-only connection cannot do it, and a reader was closing last. A copy
  of the file alone then missed recent writes. The writer now closes last.
- **The public key cannot identify a returning device.** See the decision
  above. `POST /v1/devices` gained an optional `previous_token`.
- **The cycle after a push pulls that push back.** It costs one request and
  moves the cursor; the simulation's "nothing to do" test accounts for it.
- **A push answer is matched to what was sent by note, not by position.** An
  answer with a result missing would otherwise record one note's change
  number against another, which the replay check then reports as a problem.
- **Recording a push never moves a note's change number backwards.** The
  desktop app and the CLI share a store, and the process whose push the
  server took first may record it last.
- **A held note is released by a readable version.** If the server's copy of
  a note is damaged for good, every device that had not read it holds the
  note until a device that has it edits it, which pushes a new version. The
  sync status view of step 7 must say so next to a held note.
- The server's sign-in-again rule, the store's "pushed" rule, the skip above
  and the matching by note were each broken by hand to confirm a test fails.

### 5b: sign-in and the clients

- [ ] `dirt-core::identity`: the Clerk sign-in from trial 3 and device
      registration.
- [ ] `dirt-core::secrets`: the keychain, Android and in-memory stores.
- [ ] `dirt-core::session`: one object with four states (signed out, signing
      in, waiting for approval, ready) that all three clients render.
- [ ] The sync worker: driven by events, with no timer.
- [ ] Desktop, CLI and Android: sign in, open the account's store, move the
      local notes into it after a confirmation, sync on the events of design
      section 8.3, a sync button and the time of the last sync.
- [ ] Delete `dirt-core/src/auth/`, the old `dirt-core/src/sync/` clients and
      what the clients keep for them.
- [ ] Close #234, #211 and #216.

## Tests

Each of these is an exit condition.

- [x] Encryption: a round trip; a flipped bit is rejected; a blob moved to
      another note, another account, or another key version is rejected; two
      encryptions of one document differ.
- [x] Two devices edit the same note offline, both sync, and both end with
      the same text containing both edits.
- [x] Three devices with random edits and deletes, syncing in random order,
      all converge.
- [x] An edit made while a push is in flight is not lost and is pushed by the
      next cycle.
- [x] A delete on one device removes the note on the other; an edit the
      deleter had not seen brings it back on both.
- [x] A blob that will not decrypt holds that note only, and the rest sync.
- [x] A server that replays an older blob is refused.
- [x] A new epoch makes a device merge everything again and lose nothing.
- [x] A device that lost its local state takes a new client ID and loses no
      edit.
- [x] Sync is safe to interrupt: a cycle cut off after any request can run
      again.
- [x] A device whose token expired signs in again as the same device and
      sends what it had not sent; the old token alone, the key alone, or
      another account's token do not resume a device; a revoked device
      starts over.

## Exit

- [ ] Every test above passes locally and in CI.
- [ ] The desktop app signs in through Clerk and syncs with the server on
      `dirt-trial`.
- [ ] #234, #211 and #216 are closed.
