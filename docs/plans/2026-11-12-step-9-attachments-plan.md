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

- [x] `compose.yaml` gains an S3-compatible store for tests. CI gets the
      same.
- [x] Tables for files: the account, the file, its size, whether it is
      confirmed, and when it is to be deleted; and for references, a file
      and a note each. A file with no reference left is marked.
- [x] `POST /v1/files`: an upload link for a file of a stated size, counted
      against the account. `GET /v1/files/{id}`: a download link.
- [x] `POST /v1/files/confirm` and `POST /v1/files/release`: review item 1.
- [x] A purge removes the note's references. Deleting or resetting an
      account deletes its files.
- [x] Deleting what is due: when the account next asks for a link, and once
      a day for every account by a scheduled request.
- [x] `docs/DEPLOY.md` gains the storage's set-up.

**Result (2026-11-12): done.** `docs/BACKEND_API.md`, "Files", is the
protocol.

- **The store for tests** is the Versity S3 gateway, in `compose.yaml` and
  in CI, pinned to one build. The server's tests upload and download
  through the links they are given and look in the store to see what is
  there.
- **The size is enforced twice.** The length is part of what an upload
  link's signature covers, and the test store refuses a body of another
  length. The server also asks the storage for the object's size before it
  counts a file as present. The first has not been seen on Cloudflare R2,
  which nobody has an account for yet; the second does not depend on it.
- **Nothing is deleted in the request that decides it.** A file due to go
  has a date. When it comes, the row is removed and the object queued in
  one transaction, and the object is deleted from the storage as a second
  step that is tried until it works. An account's objects are queued the
  same way when it is deleted or reset. A test cuts the store off and sees
  the object wait in the queue.
- **The daily request** is `GET /v1/maintenance/sweep`, asked for by
  `vercel.json` and open only to whoever holds `CRON_SECRET`.
- **A server with no storage** answers `files_unavailable` to file
  requests and is otherwise unchanged. The trial server is one, until the
  owner opens the storage account.
- **Two new codes:** `files_unavailable` and `file_too_large`.
- **The backup check** carries the three new tables. A dump holds the
  files' rows and not the files.

Seventeen tests, against Postgres and the store, after the changes that
review asked for. Each rule was undone in
turn, 25 in all, to see a test fail:

- 22 failed a test at once.
- One showed a case with no test: an object in the store at another size
  than was asked for. The store refuses to take one through an upload
  link, so the server's own look at the size was never reached. A test now
  puts one there with the store's own key, and the server refuses to
  confirm it.
- Two are a pair. A file is checked to be due when the list of due files
  is read and again when its row is taken, so that a file confirmed in
  between is left alone. Either check alone does the work the tests can
  see; with both removed, three tests fail.

What review showed:

- **A file sent again after a reset could be deleted as the old one.** A
  file's ID is its place in the storage. After a reset the device that
  remains sends its files again under the same IDs, to the places whose
  old objects were queued for deletion, and the queue would have deleted
  the new upload. Asking to upload a file now takes its object out of the
  queue, under the account's lock. Found in review.
- **A sweep and a confirmation could wait on each other,** because one
  locked the file and then the account and the other the reverse. And a
  sweep that failed after deleting an object left a row saying the file
  was there. Every request that touches an account's files now takes the
  account's lock first, and a file's row is removed before its object is
  deleted, not after. Confirming also asks the storage inside the lock, so
  what it learns cannot be out of date. A test confirms a file in the
  moment it falls due, twelve times with each side given the first move in
  turn: either the file is kept, with its object, or it is gone, with its
  object; never a row without one. Found in review.

- **An upload could arrive after its file had been swept,** and leave an
  object that nothing remembered. A link works for fifteen minutes and the
  storage knows nothing of the server's rows. With the locking above, a
  sweep could still take a file's row and its place in the queue while a
  link for it was good. An object now waits an hour in the queue before it
  is deleted, which is four times the life of a link. A test issues links,
  resets the account, sweeps, and then uploads: the objects are still
  queued, and go when the hour is over. Found in the second review.
- **The hour was checked when the queue was listed and not again when a
  key was taken from it.** In between, a file could be asked for again and
  the account reset, leaving a new entry under the key the sweep held. The
  wait is now asked about again under the account's lock. Found in the
  third review.

Not tried: Cloudflare R2 itself. Everything above ran against the gateway.

### 9b: the core

- [x] The note document holds its attachments: for each, the file's ID,
      name, type, size and key, and its thumbnail's. Two devices adding
      files to one note merge.
- [x] Encrypting and decrypting a file in pieces (review item 4).
- [x] A queue in the local store: files to upload, to confirm and to
      release. It runs in the sync cycle, after notes.
- [x] Fetching a file on demand and keeping it. A file the server does not
      know is uploaded again by a device that holds it (review item 6).
- [x] A purged note's files go from the device. A note kept as new after a
      purge keeps its files and confirms them (review item 1).
- [x] Making a thumbnail of an image when it is attached. The document
      has the place for one, and sync already sends and fetches one as a
      file of its own. A thumbnail counts as one of a note's twenty files
      on the server, so attaching has to count it too.
- [x] `dirt-sim`: two devices, a file added on one and read on the other;
      offline; the purge cases of step 8e with files; a restore. And three
      devices: two were away with edits when the third purged the note,
      each keeps its own new note, both show the file, and purging one of
      the new notes leaves the other's file readable.

**Result so far (2026-11-12): the first two items, in a pull request of
their own.** Nothing is sent yet: this half needs no server.

- **The document** has a fourth map, of files, beside the text, the edit
  counts and the deletion markers. An entry is a file's name, type, size
  and key under its ID, written once and at most removed.
- **Attaching a file and removing one are edits.** Each raises the
  device's count, as a change to the text does. So a device that deletes
  the note without having seen a file attached has not deleted that, and
  the note comes back with the file. A photograph taken on the phone does
  not vanish because the laptop deleted the note it was added to.
- **A file is encrypted in pieces of 64 KiB** with XChaCha20-Poly1305,
  chained by the STREAM construction as the `aead` crate implements it.
  The account and the file's ID are bound into every piece. Tests move,
  drop, repeat and truncate pieces, and none of it opens.
- **The local store** copies an attached file onto a shelf beside its
  database, and knows which files its notes name without opening every
  document. A deleted note still names its files. A note kept as new after
  a purge keeps them.
- **Local schema version 4** adds the table of which note names which
  file.

Thirty tests. Each rule was undone in turn, 16 in all, to see a test
fail, and each did.

What review showed:

- **Notes that moved between stores left their files behind.** Notes
  written before signing in are moved into the account's store, and moved
  back out when the account is deleted, after which the account's folder
  is removed. Only the documents moved. A note arrived naming a file that
  was not there, and after an account was deleted the file was gone for
  good. The move now copies each file the store holds before it copies the
  note. Found in review.

**Result of the second half (2026-11-12): files sync.** Thumbnails
followed in a pull request of their own; see the end of this section.

How a device deals with files, after the notes have synced in a cycle:

1. **It confirms** each reference the server has not taken: a file, and a
   note the server holds that names it. Every device does this for every
   note it holds, whether or not it holds the file. One of them may be the
   device that keeps the note as new after a purge.
2. **If the server answers that it does not have the file and this device
   does, the device sends it** and confirms again. That one rule covers a
   file attached here, an upload that was cut short, and a server restored
   from a backup made before the file was sent. No device keeps a list of
   what it has uploaded: the server is asked.
3. **It releases** each reference that a note has been without, on this
   device, for thirty days, once the server holds the note as this device
   does. Whether the server ever took the reference does not matter: the
   answer to a confirm may have been lost.
4. **It removes from itself the files nothing needs:** no note names them,
   deleted notes included, and nothing about them is left to tell the
   server. A file is removed only when it has stayed unneeded for an hour.
   A file is put on a device a moment before the note that names it is
   written, and the desktop app and the CLI share one store.

A file is fetched when it is wanted and then kept. What the storage returns
is kept only if it opens with the file's key to the size the note says.

What was decided while building:

- **A failure with files does not fail the cycle.** The notes have synced
  by then. An open app tries again with growing pauses, as it does for
  notes, only if the failure was one that trying again may cure and a file
  this device holds is still waiting.
- **A server with no storage is not asked again and again.** It answers
  `files_unavailable`, files wait, and the next thing that happens asks
  once more. This is `dirt-api` until its owner opens a storage account.
- **A file the server refuses is listed with the reason** and not counted
  as waiting: the account's files are full, or the note has twenty. It is
  asked about again each cycle, which costs one request. Once the account
  is known to be full no more uploads are asked for in that cycle.
- **A device that does not hold a missing file keeps asking, once a cycle.**
  After a restore another device has the file to send, and this one's
  reference has to be recorded once it has. If no device holds the file any
  more, the asking never ends. It is one small request in a cycle that is
  made anyway.
- **An entry this version cannot read still counts as a file the note
  names.** A later version wrote it. Left out, this device would release
  the reference after thirty days and the file would be deleted under the
  device that wants it.
- **A new epoch offers every reference again** (a reset, or a restore).
- **Signing out is refused while a file is only on this device,** as it is
  for a note that has not been sent. Signing out deletes the account's
  store. The note that names the file may have synced, and would go on
  naming a file that existed nowhere. Found while reading the code back.
  The three clients say what would be lost in one set of words, from
  `dirt_core::sync::overview`, which also counts files among what is
  waiting to be sent.
- **Local schema version 5:** what this device has told the server about
  each file a note names, and which files on it are waiting to be removed.

Fifteen tests with whole devices, the real server and a real file
storage: the cases the plan lists, and the storage failing while the server
answers, an upload whose answer was lost, an open app trying again, a
server with no storage, a full account, a note written before signing in,
a stored object with one byte changed, a release for a file the server has
forgotten, and signing out. Eighteen more tests of the core. Each rule was
undone in turn, 32 in all, to see a test fail.

What review showed:

- **A reference that came to be named again was not confirmed again.**
  Two devices see a file removed from a note. One releases the reference
  after its wait. If the other then merges a document that names the file
  again, its own record still said the server had the reference, and it
  never offered it again. The server could then delete a file that a note
  names. Nothing in the apps names a removed file again today, since every
  attachment gets a new ID, but the document allows it. A reference that
  is named again is now confirmed again, whatever this device recorded
  before.
- **A file the server refused was shown nowhere.** It was recorded with its
  reason and left out of what is waiting, so a device with one refused
  file said that nothing was waiting to be sent. The overview that the
  three clients show now lists each such file by its name and its note,
  with the reason, and says that this device is the only place it is.

- **An open app stopped trying for a file when a whole cycle failed.** It
  tried again after a cycle in which only the file failed. If that next
  cycle could not reach the server at all, it looked only at whether notes
  were waiting, found none, and stopped until something happened. A failed
  cycle is now tried again while a file this device holds is waiting too.
- **A refusal that came after the upload was dropped.** The server counts a
  note's other files only once it has found the new one in the storage, so
  "the note has too many files" can come with the confirm that follows the
  upload. Only the first answer was looked at for a refusal. Both are now.
  It can happen without any device being at fault: a file removed from a
  note still counts on the server until its reference is released.

Not checked on a real device: no client attaches a file yet.

**Thumbnails (2026-11-12). 9b is done.**

- **A picture gets a thumbnail when it is attached:** a JPEG within 256
  pixels a side, made by the device, kept as a second file with a key of
  its own. Whether a file is a picture is read from the file, not from
  what it is called. JPEG, PNG, GIF and WebP are read. Anything else, and
  a picture that cannot be read, is attached without one.
- **It is upright.** A camera stores a photograph as its sensor lay and
  notes which way up it was held. The note is followed, and not copied:
  the thumbnail carries nothing of the original but its pixels.
- **What is see-through is shown on white.** A JPEG has no transparency.
- **A small picture is not made larger.**
- **Opening a picture is bounded.** It may take 128 MiB, which is a
  photograph of about forty million pixels, and no side may be longer than
  20,000. A larger picture has no thumbnail. The library's own limit turned
  out not to cover the opened picture itself on the path that also reads
  which way up it is, so the size the file claims is checked before
  anything is set aside for it. A test turned the limit down to find that
  out; the first version would have set aside 1.4 GB for a file of a few
  hundred bytes.
- **A thumbnail counts as one of a note's twenty files,** as the server
  counts it. So a note holds ten pictures. A picture that fits only without
  its thumbnail is attached without.
- **The work is done away from the thread that writes,** so that making a
  large photograph small does not hold up a note being saved.
- The `image` crate was already in the workspace's lock file, for the
  desktop app. `dirt-core` uses it with four formats and nothing else.

### 9c: the CLI

- [x] `dirt attach <note> <file>`, `dirt files <note>`, `dirt save <note>
      <file> [path]`, `dirt detach <note> <file>`.
- [x] `dirt sync --verbose` and `dirt auth status` say what is waiting to
      be uploaded. (There is no `dirt sync status`; the status is under
      `dirt auth`.)

**Result (2026-11-12).**

- **A file is named by the start of its ID or by its name**, if no other
  file of the note has that name. `dirt files` prints the first thirteen
  characters of each ID, as `dirt list` does for notes.
- **`dirt files` says where a file is only when there is something to
  say:** waiting to be sent, not on this device, or refused with the
  reason. The words are in `dirt_core::sync::overview`, for the apps to
  use too.
- **`dirt save` does not replace a file that is there** unless `--force`.
- **A file's name is never used as a path.** It comes out of a note, where
  another device put it. `dirt_core::files::safe_file_name` keeps the last
  part of it, without the characters a file system refuses and never as
  one of the names Windows keeps for devices. `dirt attach` cleans the
  name it writes into a note the same way. The apps will save through the
  same function.
- **A command that changes a note says when a file did not go.** `dirt
  attach` printed the file's ID and nothing else when the upload failed.
- **What kind of file it is** is taken from the name when `--type` does
  not say. It is a hint for whatever opens the file later.

Tried for real, which no test here does: the `dirt` program itself, twice
on one computer as two devices, against the real server on this machine
with the file storage from `compose.yaml`. This is the first time the
core's own HTTP path to the storage ran; the tests reach the storage
through the harness. A picture and a text file were attached on one
device, listed and saved on the other and compared byte for byte, a file
was removed, and a file was attached with the storage stopped and sent
once it was back. `docs/MANUAL_CHECKLIST.md` has the run.

What that showed:

- **A failed upload wrote its signed link into the log.** The link is the
  permission to send the file, for fifteen minutes. A request to the
  storage that fails is now reported without its address, by the devices
  and by the server, which signs addresses for its own requests.
- **The CLI's warnings went to standard output**, where a script reading
  the ID of a new note or file would have read them too. They go to
  standard error.
- **Two devices on one computer cannot be told apart by `localhost` and
  `127.0.0.1`:** the clients read the one as the other, on purpose. They
  need two ports.

What review showed:

- **`dirt save` looked whether a file was there, fetched, and then wrote.**
  A file that came to be there while it fetched was replaced without
  `--force`. The copy is now refused by the file system at the moment it
  is written, and a copy that fails half-way leaves nothing behind.
- **`dirt save --force` onto the device's own copy of a file emptied it.**
  Replacing a file opened it and emptied it before anything was read, so
  with the destination being the file itself, or a link to it, there was
  nothing left to copy and the device's copy was gone. A forced save now
  writes beside the destination and then takes its name. Nothing that was
  there is opened, and it stays whole until the new file is. What is
  written beside it has a short name of its own: one made from the
  destination's name would be too long where the destination's already is
  as long as a name may be.
- **A name was cut to 200 characters, and file systems count bytes.** A
  hundred Chinese characters are 300 bytes, more than one name may be on
  Linux, so the file could be attached and not saved. A name is now cut
  to 200 bytes, between characters, keeping the ending that says what
  kind of file it is.

**For trying a client by hand** there is now `cargo run -p dirt-sim
--example serve`: the real server on two ports of this machine, with the
file storage and a sign-in that needs no account and no browser. It is what
the desktop and Android steps will be tried against, until there is a
storage account for `dirt-trial`.

### 9d: the desktop app

- [x] Attach from a file dialog and by dropping a file on a note. A row of
      attachments under the note, with thumbnails for images. Open, save
      and remove.
- [x] The sync status view counts files waiting. (Done with the sync
      itself in 9b: the view's words come from the core.)

**Result (2026-11-13).**

- **Under the open note:** a row for each file, with its thumbnail or its
  kind, its name, its size and what kind of file it is, and where it stands
  when there is something to say. "Attach a file" opens a file dialog that
  takes several. Files dropped anywhere on the note are attached.
- **A thumbnail is fetched by itself.** A picture attached on another
  device shows its thumbnail here without the picture being fetched.
- **"Open" hands a copy to the system.** The store's file has its ID for a
  name and must not be changed, so a copy is made under the name the note
  gives it, in a folder of its own, and that is what the system is given.
  The copies are removed when the app next starts.
- **A file the system would run is not opened.** A program, a script, a
  shortcut, a document with macros, a file with no ending. It is offered
  for saving only. The list is in the core, for Android to use too, and
  the copy for opening refuses such a file whatever asks.
- **"Save a copy"** asks where first, with the file's name proposed, and
  then fetches. The dialog itself asks before replacing a file.
- **Moved into the core** for both apps: writing a copy without ever
  emptying what is there (9c's review), the words for a file that cannot
  be had, and a thumbnail's bytes as something a web view can show.

Tried for real against the server on this machine, with the CLI as the
second device and as a check on the desktop's own store: the rows and
thumbnails as drawn, attaching through the file dialog, removing, and a
thumbnail arriving from the other device. `docs/MANUAL_CHECKLIST.md` has
the run and what it left for a person: dropping a file, opening one in
another program, and answering the save dialog.

What review showed:

- **Copies made for opening were in the system's place for temporary
  files.** On a computer with several users that place is shared: a copy
  could be read by the others, and the folder could be made by one of them
  beforehand. The copies are now in the store's own folder, beside the
  files they are copies of, in a folder closed to everybody but its owner.
  They are as private as the store, and go with the account's folder at
  sign-out as well as at the next start.
- **A thumbnail that was slow to fetch kept the whole list from showing.**
  The rows waited for every thumbnail, and a thumbnail on a server out of
  reach can take minutes to give up. The rows now show at once, from what
  the device holds, and each thumbnail appears when it is had.

- **Turning to another note cut an attach short.** The attaching belonged
  to the panel of the note, and the panel is made afresh for each note, so
  files chosen and not yet attached were dropped, and one could be left
  copied and not named. Attaching is now a task of its own that finishes
  whatever the window does.
- **A saved copy could replace a file nobody was asked about.** The save
  dialog asks before replacing a file that is there. If nothing was there,
  nobody agreed to anything, and a file that turned up while this one was
  fetched was replaced all the same. Now it is left alone and the person
  is told.

**Why those are left for a person.** While the app was being driven, a
person was at the machine. The save dialog had opened in their own folder
with one of their files selected. Windows asks before replacing a file, so
nothing was lost, but a script has no business answering a dialog like
that on a desktop somebody is using. The test window is now moved off the
screen as soon as it exists, where it can still be listed and captured,
and file dialogs and "Open" are not driven at all.

### 9e: the Android app

- [ ] Attach from Android's file chooser. A shared image becomes a note
      with the image attached. Open with another app, save, remove.
- [ ] Measure on the phone how long a twelve-megapixel photograph takes to
      attach, most of which is making its thumbnail, and write it into
      `docs/LIMITS.md`. If it is long enough to notice, the app says that
      it is attaching.
- [ ] A photograph saved as HEIC is attached without a thumbnail, because
      the core does not read HEIC. See what the emulator's and a real
      phone's camera save, and say so in `docs/feature-parity.md`.

### 9f: export, limits and the documents

- [ ] On Linux and macOS, make the data folder for its owner alone, and
      close one that is already there. Today it gets the permissions the
      system gives any new folder, so on a computer whose home folders are
      open to its other users, they can read the notes and files. Found in
      review of 9d, where only the folder of copies made for opening was
      closed. A database file given by hand (`--db-path`) is left as the
      person made it.

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
