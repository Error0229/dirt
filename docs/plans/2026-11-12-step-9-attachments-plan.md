---
title: "Step 9: attachments"
type: plan
date: 2026-11-12
status: planned; the design review is done, nothing is built
parent: docs/plans/2026-11-10-refactor-system-redesign-implementation-plan.md
tracking: "#245"
---

# Step 9: attachments

## Goal

A file can be attached to a note on any of the three clients, reaches the
person's other devices, and is readable by nobody else: not the server and
not the storage it sits in. Design section 10 is the specification. This
plan begins with the design review that the parent plan asks for, because
section 10 was written before deleted notes were purged, before the
account's key could be changed and before a restore had been tried.

## State before this step

- A note is one CRDT document holding its text, its edit counts and its
  deletion markers (`dirt-core::notes`). Nothing else.
- The server stores one blob per note in Postgres. It has no object storage
  and has never issued a link to any.
- Deleted notes are purged thirty days after a device saw them deleted
  (step 8e). A purged note keeps a marker on the server.
- The server's database is dumped daily once the owner has chosen where
  (step 8d). Nothing outside the database is backed up.

## The design review

Section 10 leaves these open or was overtaken by later steps. Each has a
default, taken here unless the owner says otherwise.

### 1. When a removed file is finally deleted

The gap check's question. "Thirty days after its removal has settled" does
not say what settled means when a device that was away can bring a note
back.

A file stops being wanted in two ways, and they differ:

- **Its note is deleted.** The note's document still holds the file's key,
  so that the note can come back whole. The file has to outlive the note's
  thirty days.
- **It is removed from a note that lives on.** The document no longer names
  it. No device that was away can bring the reference back by merging: a
  removal in the document wins over every edit that did not re-add the
  file, and nothing re-adds one.

**Decision: the server knows which notes refer to a file, and a file's
last thirty days start when the last of them stops.**

One note is not enough. Step 8e keeps an unsent edit to a purged note as a
new note, and two devices that were both away each make their own. Both
new notes hold the same files. So the server keeps a list of references, a
file and a note each, and a file is wanted for as long as it has one.

| Event | What the server does |
|---|---|
| A device confirms the file for a note | Adds that reference. If the file was marked for deletion, clears the mark |
| A note is purged | Removes that note's references |
| A device says the file was removed from a note | Removes that reference |
| A file's last reference is removed | Marks the file to be deleted thirty days on |
| The mark's time comes | Deletes the object, then the row |

The thirty days after the last reference goes are for the device that was
away with an unsent edit. Its new note keeps the files: the device confirms
them for the new note, which adds a reference and clears the mark. Without
the wait the files would already be gone. If two devices do this, the file
has two references, and purging one of the new notes later leaves the
other's files alone.

A file is counted against the account once, however many notes refer to it.

A device asks for a file's removal by the same rule as a purge: it has
itself seen the note without the file for thirty days, with nothing unsent.
So a file removed from a living note goes sixty days after the person
removed it, and a file of a deleted note sixty days after the delete.

**What this costs in privacy.** The server learns which notes refer to a
file, and so how many files each note has and how large. It already sees
each note's size. `docs/SECURITY.md` will say so.

**Rejected:** giving each new note its own copies of the files. The device
would have to hold every file to upload it again, and the account would be
charged twice for the same bytes.

**Rejected:** files that the server cannot link to notes, deleted only when
a device asks. A device that purged a note and stopped before asking would
leave the files for ever, and the server could not tell.

### 2. The storage, and whose account it is

Section 10 names Cloudflare R2, because it charges nothing for downloads
and devices download straight from it. That stands.

**The owner has to open it.** R2 needs a billing profile before it issues a
key, free allowance or not (`docs/OWNER_DECISIONS.md`, decision 1, found
the same). This is a sixth decision for the owner and is written up there
with this plan.

Until then everything is built and tested against a local store that
speaks the same protocol, in `compose.yaml` beside Postgres. The server
speaks only the S3 protocol, so nothing in the code is R2's.

**Not possible until the owner decides:** trying attachments on real
devices against `dirt-trial`. Devices fetch from the storage directly, so
it has to be reachable from a phone.

### 3. Keeping a file's bytes away from the server, and bounded

- The server signs a link that allows one upload of one object of one
  exact size. The size is part of what is signed, so a device cannot use a
  link for a larger file than it asked for.
- The server counts the size against the account when it signs, not when
  the upload ends. An upload that never happens holds its space for a week
  and is then discarded.
- A download link is good for five minutes. A device asks for one each time
  it needs the file and does not have it.

### 4. Encrypting a file

Each file gets a random key and is encrypted in pieces of 64 KiB, so that a
device never holds a whole file in memory twice and a piece that is damaged
or moved is detected (the STREAM construction, with XChaCha20-Poly1305,
which the system already uses). The account, the file and the format
version are bound into every piece.

A thumbnail is a second, small file with its own key, made on the device
for the image types it can read. Other types show their name and size.

### 5. Changing the account's key

Section 10 says a change of the key re-encrypts notes only. That stands and
needs saying plainly: a device that was removed still knows the keys of the
files it had seen. It cannot fetch them, because the server issues it no
links. `docs/SECURITY.md` will say that a removed device together with
whoever holds the storage could read those files, as it says of notes.

### 6. Restoring a backup

Step 8d showed that a restore sets the server back while the devices keep
what they hold. Files are not in the database dump.

- A file uploaded after the dump has no row after a restore. **A device
  that still holds the file uploads it again** when it finds the server
  does not know it. This is the same rule as for notes, and is why a device
  keeps a file it uploaded until the note is purged.
- A file deleted after the dump has a row and no object. A device asking
  for it is told it is gone, and shows the attachment as lost.
- R2 keeps no history. **A file exists in the storage and on the devices
  that have fetched it, and nowhere else.** `docs/DEPLOY.md` will say so.

### 7. Export

An export today is one file of every note. With attachments it has to hold
the files or say that it does not. Default: the CLI and the desktop app
export into a folder, notes and files; the Android app, which can write one
file, writes a zip of the same.

### 8. Voice memos

Section 10 says voice memos are audio attachments. Any client can attach an
audio file. Recording one inside an app is a feature of its own, with a
microphone permission and a recorder on each platform, and no document asks
for it. **It is not in this step.** It needs a decision, like `dirt tui`.

### 9. Limits

Proposed, to be measured in part 9f as step 8c measured the others:

| Limit | Proposed | Why |
|---|---|---|
| One file | 10 MiB | A photograph from a phone, a short recording, a PDF. Uploaded in one request, with no resuming to build |
| Files per account | 200 MiB | R2's free allowance is 10 GB for everyone together |
| Files per note | 20 | Each adds its key and name to the note's document, which is sent whole on every edit |
| An upload not confirmed | Discarded after 7 days | Section 10 |

## Needs a person

- **The storage account** (review item 2). Until it exists, the trial
  server has no attachments.
- **Whether to record voice memos in the apps** (review item 8).
- **The limits' numbers**, since storage past the free allowance is paid
  for.

## Build order

Each part is one pull request, with its tests, and none needs the owner's
account.

### 9a: the server

- [ ] `compose.yaml` gains an S3-compatible store for tests. CI gets the
      same.
- [ ] Tables for files: the account, the file, its size, whether it is
      confirmed, and when it is to be deleted; and for references, a file
      and a note each. A file with no reference left is marked.
- [ ] `POST /v1/files`: an upload link for a file of a stated size, counted
      against the account. `GET /v1/files/{id}`: a download link.
- [ ] `POST /v1/files/confirm` and `POST /v1/files/release`: review item 1.
- [ ] A purge removes the note's references. Deleting or resetting an
      account deletes its files.
- [ ] Deleting what is due: when the account next asks for a link, and once
      a day for every account by a scheduled request.
- [ ] `docs/DEPLOY.md` gains the storage's set-up.

### 9b: the core

- [ ] The note document holds its attachments: for each, the file's ID,
      name, type, size and key, and its thumbnail's. Two devices adding
      files to one note merge.
- [ ] Encrypting and decrypting a file in pieces (review item 4).
- [ ] A queue in the local store: files to upload, to confirm and to
      release. It runs in the sync cycle, after notes.
- [ ] Fetching a file on demand and keeping it. A file the server does not
      know is uploaded again by a device that holds it (review item 6).
- [ ] A purged note's files go from the device. A note kept as new after a
      purge keeps its files and confirms them (review item 1).
- [ ] `dirt-sim`: two devices, a file added on one and read on the other;
      offline; the purge cases of step 8e with files; a restore. And three
      devices: two were away with edits when the third purged the note,
      each keeps its own new note, both show the file, and purging one of
      the new notes leaves the other's file readable.

### 9c: the CLI

- [ ] `dirt attach <note> <file>`, `dirt files <note>`, `dirt save <note>
      <file> [path]`, `dirt detach <note> <file>`.
- [ ] `dirt sync --verbose` and `dirt sync status` say what is waiting to
      be uploaded.

### 9d: the desktop app

- [ ] Attach from a file dialog and by dropping a file on a note. A row of
      attachments under the note, with thumbnails for images. Open, save
      and remove.
- [ ] The sync status view counts files waiting.

### 9e: the Android app

- [ ] Attach from Android's file chooser. A shared image becomes a note
      with the image attached. Open with another app, save, remove.

### 9f: export, limits and the documents

- [ ] Export with files (review item 7).
- [ ] The limits measured, and their rows in `docs/LIMITS.md`.
- [ ] `docs/SECURITY.md`, `docs/BACKEND_API.md`, `docs/feature-parity.md`
      and `docs/MANUAL_CHECKLIST.md`.

## Exit

- [ ] Every test above passes locally and in CI.
- [ ] A file attached on one device opens on another, in `dirt-sim` with
      the real server and a real S3-compatible store.
- [ ] No file's bytes, name or key reach the server: a test reads what the
      server and the store hold.
- [ ] On real devices against `dirt-trial`: needs the owner's storage
      account.
