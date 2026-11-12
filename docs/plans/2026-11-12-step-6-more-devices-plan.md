---
title: "Step 6: more devices"
type: plan
date: 2026-11-12
status: in progress
parent: docs/plans/2026-11-10-refactor-system-redesign-implementation-plan.md
tracking: "#242"
---

# Step 6: more devices

## Goal

Let an account live on more than one device, and survive the loss of all of
them. After step 5 an account has exactly one device that can read it, that
device cannot sign out, and losing it loses the notes. This step adds:

- handing the account key to a second device, with a short code the person
  compares so that the server cannot put itself in the middle;
- an optional recovery phrase;
- the list of devices, with removal;
- resetting an account and deleting one.

Design sections 5.4, 5.5 and 7.2 are the specification.

## State before this step

- A second device signs in and waits. Nothing can give it the key.
- Signing out is refused on every account, because every account has one
  approved device.
- The server has the device registry and the note blobs. It has no tables
  for the approval exchange or the recovery copy.

## Decisions

The parent plan's "Settle first" items:

| Question | Decision |
|---|---|
| What a waiting device may do | Everything locally: it writes notes to the local store as a signed-out device does. From the server it learns how many devices could approve it and whether a recovery copy exists, so that it can say what the person's options are |
| When a request to join expires | After seven days. An expired request no longer counts against the limit of three, and its device must sign in again |
| Approving from the CLI | `dirt devices approve` runs the same exchange in the terminal, for people who use only the CLI |
| A rejection or a code that does not match | Both remove the waiting device from the account. A mismatch also tells the person that someone may be interfering, on both devices |
| A removed device | It keeps the account key it had. It can no longer reach the server, so it reads nothing new, but what it already holds it can still read. Only key rotation, in step 8, ends that. The device list says so when a device is removed |
| Clerk's consent page | Kept. It is Clerk's, and whether a public client may turn it off is not something this step can settle; step 8 decides with the production instance |
| Sign-in for a CLI with no browser | `dirt auth login --no-browser` already prints the address, which works over SSH with a forwarded port. Clerk's device-code grant stays untested and is moved to step 8, with the production Clerk decisions it depends on |

Decisions this plan adds:

| Question | Decision |
|---|---|
| The model for the exchange | Matrix's short-code verification: commit, then reveal, then compare. The key agreement and the code come from the `vodozemac` crate, which implements them for Matrix. Nothing here invents a primitive |
| Who commits | The new device commits to its temporary key before the approving device sends its own, and reveals it afterwards. Neither a device nor the server can then choose a key to make the codes match |
| What the code covers | Both temporary keys, both devices' registered public keys, both device IDs, the account ID and a random ID for the exchange. A server that swaps any of them makes the codes differ |
| The code | Three groups of four digits, as Matrix shows them. Digits can be read aloud and typed on any keyboard |
| Who confirms | Both devices ask whether the codes match. The approving device must not send the key to an impostor, and the new device must not accept a key from one |
| How the key travels | Encrypted with XChaCha20-Poly1305 under a key that only the two devices can derive from the exchange. The server stores the encrypted copy until the new device says it has it |
| When the new device becomes approved | When it tells the server it has unwrapped and saved the key, not when the key is uploaded. A device is never approved without holding the key |
| The server's part | A mailbox. It stores and relays small messages for one waiting device, and understands none of them |
| The recovery phrase | Twenty-four words from the BIP 39 English list: 256 random bits and a checksum that catches a mistyped word. Because the phrase is as strong as a key, the keys are derived from it with HKDF; no slow password hash is needed |
| Proving the phrase to the server | Two values are derived from the phrase. One encrypts the account key. The other is a claim code: the server keeps its hash, and a waiting device that presents the code is approved. The server learns nothing that decrypts the key |
| Who may touch the recovery copy | An approved device creates, replaces and deletes it. A waiting device may only read it and claim with it (amendment B6) |
| Signing out | Allowed once another approved device exists or a recovery copy does |
| Deleting an account | Any approved device may. Everything the server holds for the account goes, and every device is signed out at its next request |
| Resetting an account | The notes on the server, the recovery copy and every other device go; the device that asked becomes the account's first device and makes a new key. The epoch rises, so the device sends its notes again under the new key |
| Who may reset | An approved device, at once. A waiting device at once if the server holds no notes for the account; otherwise it asks, and the reset happens seven days later unless an approved device cancels it. Every approved device is told at its next sync |
| Why the wait | Someone who takes over the person's email can get a waiting device and nothing more. Without the wait they could wipe the server's copy of an account whose devices are working. With it, the devices that are working see the request and refuse it |
| Telling devices that something needs attention | The answer to a pull says how many devices are waiting and whether a reset is pending, so a device learns both at its next sync without asking for the device list |

## Protocol additions

| Call | Who | What |
|---|---|---|
| `POST /v1/enrolments/{device_id}/messages` | The waiting device, or an approved device of the account | Adds one message to the exchange |
| `GET /v1/enrolments/{device_id}/messages?after=N` | The same | The messages after number N |
| `PUT /v1/enrolments/{device_id}/key` | An approved device | Stores the encrypted account key for the waiting device |
| `GET /v1/enrolments/{device_id}/key` | The waiting device | The encrypted key |
| `POST /v1/enrolments/{device_id}/complete` | The waiting device | Says the key is saved. The server approves the device and deletes its copy and the messages |
| `PUT /v1/recovery` | An approved device | Stores the account key encrypted under the phrase, and the hash of the claim code |
| `GET /v1/recovery` | Any device of the account | The encrypted key |
| `DELETE /v1/recovery` | An approved device | Removes the recovery copy |
| `POST /v1/recovery/claim` | A waiting device | Presents the claim code; the server approves the device |
| `DELETE /v1/account` | An approved device | Deletes the account |
| `POST /v1/account/reset` | Any device of the account | Resets the account, or asks for a reset |
| `DELETE /v1/account/reset` | An approved device | Cancels a reset that was asked for |

`GET /v1/devices/me` gains the number of devices that could approve this one
and whether a recovery copy exists. The answer to `GET /v1/notes/pull` gains
the number of waiting devices and the time a pending reset takes effect.

## Work, in two pull requests

### 6a: the server and the core, with no interface

- [ ] Server: the enrolment mailbox, the encrypted key, completion.
- [ ] Server: requests to join expire after seven days.
- [ ] Server: the recovery copy and the claim.
- [ ] Server: account deletion, reset, the delayed reset and its
      cancellation.
- [ ] Server: what a waiting device learns, and what a pull says needs
      attention.
- [ ] `dirt-core::crypto`: the exchange (commitment, code, key wrap) and the
      recovery phrase.
- [ ] `dirt-core::session`: approving a device, being approved, the device
      list and removal, the recovery phrase, signing out when it is safe,
      reset and deletion.
- [ ] `dirt-sim`: the tests below.

### 6b: the interfaces

- [ ] Desktop, CLI and Android: the approval screens, the device list, the
      recovery phrase, reset and deletion. The approval screen is the one
      place that polls, every few seconds and only while it is open.
- [ ] `docs/SECURITY.md`, written fresh: what is protected, from whom, and
      what is not.

## Tests

Each of these is an exit condition.

- [ ] A second device is approved with matching codes, receives the key,
      and reads the account's notes.
- [ ] A server that swaps the approving device's key, the new device's key,
      or a registered public key makes the two codes differ.
- [ ] A server that changes the new device's key after its commitment is
      caught by the approving device before any code is shown.
- [ ] The encrypted key cannot be opened by the server, nor by a third
      device that watched every message.
- [ ] A device is not approved until it says it has the key. One that stops
      before that can run the exchange again.
- [ ] A rejection or a mismatch removes the waiting device.
- [ ] A request to join expires after seven days and stops counting against
      the limit.
- [ ] A waiting device with the recovery phrase becomes approved and reads
      the notes. A wrong phrase, or the right key with a wrong claim code,
      gets nothing.
- [ ] A waiting device cannot create, replace or delete the recovery copy.
- [ ] Signing out is refused on the only approved device with no recovery
      copy, and allowed once there is another approved device or a copy.
- [ ] A reset by an approved device leaves one device, a new key and the
      device's own notes on the server, re-encrypted.
- [ ] A reset asked for by a waiting device takes effect at once on an
      account with no notes, and otherwise only after seven days, unless an
      approved device cancels it.
- [ ] Deleting an account removes everything the server holds for it and
      signs every device out.

## Exit

- [ ] Every test above passes locally and in CI.
- [ ] Two real devices, the desktop app and the Android emulator, join one
      account on `dirt-trial` by comparing the code, and a note written on
      one appears on the other.
- [ ] The parent plan's exit: the enrolment tests pass, including a server
      that swaps a key.
