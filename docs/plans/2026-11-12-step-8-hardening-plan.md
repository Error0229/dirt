---
title: "Step 8: hardening"
type: plan
date: 2026-11-12
status: in progress
parent: docs/plans/2026-11-10-refactor-system-redesign-implementation-plan.md
tracking: "#244"
---

# Step 8: hardening

## Goal

Make the system safe to run for real people. After step 7 it works on three
clients, and four things stand between that and running it:

- a removed device keeps the key that reads the account;
- the only backup is the database provider's own;
- the limits were set before anything was measured, and the desktop app
  cannot reach a note beyond its 100 most recent;
- the hosting, database and sign-in services are trial resources on free
  plans.

Design sections 5.5, 7.4, 7.5 and 7.6 are the specification.

## State before this step

- Every note blob records the key version that encrypted it, and that
  version is always 1. Nothing can change the key.
- `dirt devices remove` says, truthfully, that the removed device keeps the
  key it had.
- The server's quotas exist and are enforced. Nobody has checked the
  numbers against use.
- No dump of the database is taken.

## Decisions

The parent plan's "Settle first" item, key rotation with offline devices,
with its default taken:

| Question | Decision |
|---|---|
| Where the new key is kept for a device that is offline | The server keeps the new key wrapped for each remaining device, per key version. A device that was away walks the versions it missed, one at a time |
| A push under an old key | Refused with `key_changed` once a newer key exists. The device fetches the new key and sends the note again under it. Nothing is lost: the note is on the device |
| An interrupted rotation | The wrapped keys for every remaining device are stored in one request, before any note is re-encrypted. From then on every device can get the new key, so any device can carry on re-encrypting. A rotation that never stored its keys did not happen |
| The recovery phrase | Cancelled when the rotation begins: its copy holds the old key. Every client then says the account has no phrase and offers to make one |

Decisions this plan adds:

| Question | Decision |
|---|---|
| What the new key is wrapped with | Two things together: a secret only the two devices can compute (X25519 between a key made for this one message and the receiving device's registered key), and the old account key. HKDF-SHA256 turns them into the key that seals the new one with XChaCha20-Poly1305. These are the primitives the system already uses, composed the way a sealed box is |
| Why the old account key is part of it | It is what stops the server making a rotation of its own. The server knows every device's public key and could wrap a key it chose. It does not have the old account key, so a device will not accept what it wraps |
| Why the device's key is part of it | It is what stops a removed device reading the new key. The removed device has the old account key. It does not have the remaining devices' private keys |
| What that leaves open | A removed device and the server acting together, with the server giving the rotating device a false public key for another device. The rotating device has checked the registered key only of devices it compared a code with. `docs/SECURITY.md` says so |
| Who re-encrypts | Whichever approved device syncs. Each sync, while notes under an old key remain, it sends some of them again under the new key. The server lists which |
| When a rotation is finished | When no note is left under an older key. The server sees that in the push that replaces the last one and closes the rotation itself. No device has to say so |
| A note no device can re-encrypt | It stays under the old key and the rotation stays open. Everything written since uses the new key, which is what removing a device needed. The sync status view lists it |
| A device let in during a rotation | It is given the newest key. A note still under the old one is held on that device until it is re-encrypted, as any note it cannot read is |
| When to rotate | Offered when a device is removed, since that is what makes the old key known outside the account. Also on request. Never by itself |
| Reset | A reset already makes a new key and a new epoch. It starts the key versions again at 1 and removes every wrapped key |
| Keeping old keys on a device | A device keeps each key it has had, by version, until no note under it remains on the server. It needs them to read notes not yet re-encrypted, and to unwrap the next key |

## Needs a person

Four decisions in this step are purchases or accounts, which the standing
rule leaves to the owner. Each is written up with the facts and a
recommendation in 8d, and none blocks the rest:

- Where the scheduled database dump is stored. It has to be outside Neon.
- The Neon plan: the free plan's compute allowance cannot carry continuous
  use (design section 7.4).
- The Vercel plan: the free plan forbids commercial use (section 7.5).
- The Clerk production instance, which needs a domain bought, with its
  consent page and the device-code grant for a CLI with no browser.

How security reports are taken (GitHub's private vulnerability reporting)
is a setting on the repository and is the owner's to switch on.

## Build order

### 8a: key rotation in the server and the core

- [x] `dirt-proto`: the rotation requests, `key_changed`, and the key
      version in what a pull and a device's own record report.
- [x] `dirt-api`: migration `0003`; wrapped keys per device per version;
      beginning a rotation in one transaction; the list of notes still
      under an old key; refusing an old key; closing the rotation in the
      push that replaces the last old note; clean-up on removal and reset.
- [x] `dirt-core`: wrapping and unwrapping; keys by version in the device's
      secrets, with the old format still read; the engine fetching a key it
      lacks, sending again after `key_changed`, and re-encrypting; the
      session's "change the key".
- [x] `dirt-sim` tests against the real server:
  - a removed device cannot read what is written after the rotation, with
    everything the server holds given to it;
  - a device that was offline through one rotation, and through two,
    catches up and loses nothing it wrote meanwhile;
  - a rotation interrupted after its keys were stored is finished by
    another device;
  - a key the server wraps by itself is refused;
  - two devices rotating at once: one rotation happens;
  - a push under the old key is refused and arrives under the new one;
  - the recovery phrase stops working, and a new one works;
  - a device let in during a rotation reads everything once it is over;
  - a reset during a rotation.
- [x] `docs/SECURITY.md` and `docs/BACKEND_API.md`.

**Result (2026-11-12): done.**

- **The server** (`dirt-api::keys`, migration `0003`) has three requests:
  change the key, fetch this device's copies, and list the notes still
  under an older key. The account row holds the present key version and a
  count of notes under older ones. A push locks that row, as it always
  has, so a change and the pushes around it happen one at a time.
- **Wrapping** is in `dirt-core::key_change`, with the ring of keys a
  device holds. A wrapped copy is 105 bytes.
- **The engine** follows a change without being told to. A pull says which
  version notes are written under; the engine fetches its copies, opens
  them in order and stores each before using it. A copy that does not open
  ends the cycle with `KeyRejected` and nothing is adopted.
- **The session** has `change_account_key`. It syncs, wraps the new key
  for every approved device, sends the one request, and syncs again, which
  is how this device takes the key too: from its own copy, as the others
  do. It does not keep the key it made in memory and hope.
- **A device's stored secrets** hold the key's version and the keys before
  it. A record written before this step reads as holding the first key.

What building it showed:

- **Two programs on one device.** The desktop app and the CLI share one
  place for their keys. When one of them followed a change and then let go
  of the old key, the other could not tell the stored keys were its own
  moved on, and stopped as if the account had been reset. A later version
  in the device's own secure storage is now simply taken; only another key
  under the same version, or an earlier version, is a reset.
- **A device must hand over the key the account has now.** An approval is
  opened, the key changes, and then the person says the codes match. The
  approving device used the key it held when it opened the approval. It
  now reads its keys when it answers, and the server refuses a copy for a
  waiting device, or under a recovery phrase, that names any version but
  the present one.
- **A removed device as an attacker, not as a caller.** The first test of
  "a removed device cannot open another device's copy" called the opening
  function with the wrong device's key, and passed even with the agreement
  taken out of the key derivation: the function binds the caller's own
  public key, which an attacker would not use. The test now derives the
  sealing key the way an attacker would, from the old account key and
  every public value, and has to guess the agreement.

Eleven rules were each broken by hand to confirm a test fails: that a
copy needs the old account key, that it needs the device's key, that the
server refuses a note under an old key, a change that leaves a device out
or names a removed one, and a replaced key for a waiting device or under a
phrase; that the engine stops at a copy that does not open, that old notes
are sent again, that the recovery copy goes, and that an approving device
hands over its present key. The two that were not caught at first are the
last two findings above.

What review showed, each fixed with a test that fails without the fix:

- **A device approved while the key was being changed could be left
  without the new key.** The change looked at which devices were approved
  while holding a lock on the account's row; approving a device took no
  lock. An approval that committed just after that look was missed, and
  the device could not sync until it signed in again. Approving a device,
  by the exchange or by the recovery phrase, now takes the same lock.
  Found in review.
- **A fetched key could be stored over a key another program had just
  written.** The engine checked that the stored record was the same
  sign-in, which a reset does not change, and then wrote. If the CLI reset
  the account while the desktop app was fetching a key, the app's write
  replaced the reset's key, which was its only copy; a sign-out could be
  undone the same way. The write now replaces only the keys the engine
  started from. Secure storage cannot do that in one step, so the check
  and the write are made under a lock on a file in the data folder, which
  every other write of the device's secrets takes too. Found in review.
- **Sending notes again could stop at the first page of the list.** The
  server listed the first 500 notes under an older key, and a device that
  held none of them looked no further. A device let in after one change of
  the key holds none of the notes still under the key before it, so its
  own notes, further down, stayed under the old key after the next
  change. The list is now paged and a device reads every page. Found in
  review.
- **A test that depended on timing.** "Two devices changing the key at
  once" ran both side by side and expected one to be refused. In CI one
  finished before the other had looked at the account, and both made a
  change, one after the other, which is correct. The test now holds the
  second request until the first change is made.
- **The lints were held to a compiler older than the workspace builds
  with.** `clippy.toml` named 1.76 as the oldest compiler; the
  dependencies need 1.95, as the parent plan's amendment A7 records. The
  file lock needs 1.89, which is how it showed. The setting now says 1.95,
  and five places where the lints then asked for a newer idiom were
  changed.

Not in this part: any client's interface. `dirt devices remove` still says
that changing the key is not available. That is 8b.

### 8b: key rotation in the three clients

- [ ] Desktop and Android: "Change the account's key" in Devices, offered
      after removing a device; the progress of a rotation; the prompt for a
      new recovery phrase.
- [ ] CLI: `dirt auth rotate-key`, and `dirt devices remove --rotate-key`.
- [ ] Checked on real devices against `dirt-trial`, with one device removed
      and one offline during the rotation.

### 8c: the limits review

- [ ] Each quota and limit in `dirt-proto` against measured use, with the
      reason for its number.
- [ ] The desktop app's list and search reaching every note, from step 7.
      Measured first: how many notes the list can show before it slows.
- [ ] Unauthenticated traffic: what the hosting platform's firewall does,
      checked and not assumed.

### 8d: backups and the production decisions

- [ ] A scheduled dump of the database, and a restore from one, tried.
- [ ] The four decisions under "Needs a person", each written up.
- [ ] `docs/DEPLOY.md` for a production deployment.

## Exit

- [ ] Every test above passes locally and in CI.
- [ ] A device removed from an account on `dirt-trial` cannot read a note
      written after the key was changed.
- [ ] A dump has been restored into an empty database and a client has
      synced against it.
- [ ] The owner has the four decisions in front of them.
