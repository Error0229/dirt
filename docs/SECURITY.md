# Security

What Dirt protects, from whom, how, and what it does not protect. The design
behind it is `docs/plans/2026-11-10-system-redesign-design.md`, sections 3.4
and 5. The server's protocol is `docs/BACKEND_API.md`.

## In one paragraph

Notes are encrypted on the device before they leave it, under a key the
server never sees. The server stores ciphertext and relays it. Signing in
proves who the person is; it does not give access to notes. A device reads an
account's notes only once it holds the account key, which it gets from a
device that already has it or from the person's recovery phrase.

## Who is kept out

| Who | What they get |
|---|---|
| Someone with a copy of the server's database | Ciphertext, and hashes of device tokens. No notes |
| The operator of the server | The same. Also what the server sees while running: see "What the server does see" |
| Someone who takes over the person's email or sign-in | A device that is waiting for approval. It reads nothing. It can ask for the account to be reset, which the person's working devices are told about and can refuse for seven days |
| A server that puts itself between two devices when one approves the other | Nothing, if the person compares the code the two devices show. See "Approving a device" |
| Someone with a stolen device token | Ciphertext only. The token stops working when the device is removed |
| A device that was removed, once the account's key has been changed | What it already had, and nothing written since. See "Changing the account's key" |
| A server that tries to change an account's key to one it knows | Nothing. A device does not accept a key that was not wrapped with the key it replaces, which the server has never had |

## Who is not kept out

- **Someone with the unlocked device.** The local database is not encrypted,
  so that search works on it. It relies on the operating system's disk
  encryption and screen lock. The keys are in the operating system's
  keychain, or in Android's encrypted preferences, which protect them from
  other apps and not from the device's own user.
- **A device that was removed, for what it already has.** Removing a device
  stops it reaching the server, so it reads nothing new. It keeps the notes
  it had and the account key it had. Changing the account's key means that
  key reads nothing written afterwards; it cannot take back what the device
  already holds.
- **A removed device and the server acting together, against a key
  change.** See the end of "Changing the account's key".
- **A server in the middle of an approval, if nobody compares the codes.**
  The comparison is the protection. Pressing "they match" without looking
  gives it away.

## What the server does see

How many notes an account has, how large each one's ciphertext is, when each
changes, which devices an account has and when they were last seen, the
person's email address as the sign-in service reports it, and network
metadata such as addresses. It does not see note text, tags or titles: those
are inside the ciphertext. It cannot tell a deleted note from a live one,
until a device asks for the note to be purged thirty days after it was
deleted: the server then knows that it was.

**Files.** A file attached to a note is encrypted on the device with a key
of its own, which is kept inside the note's document. It is encrypted in
pieces of 64 KiB with XChaCha20-Poly1305, the cipher the notes use, and the
pieces are chained so that one that is moved, repeated or left out does not
open, nor a file that was cut short. The account and the file are bound
into every piece. The server and the storage see neither the key, nor the
file's name or type, nor its contents. The server does see that a file exists, how large it is, and
which notes refer to it, because devices have to tell it that for files to
be cleaned up. So it knows how many files each note has.

A picture gets a thumbnail, made on the device that attaches it and
encrypted as a second file with a key of its own. To the server the two are
two files of one note. The thumbnail holds the picture's pixels made small
and nothing else: not where or when a photograph was taken, which the
picture itself still carries inside its own encryption. A file is tried as
a picture by reading it, with a limit on how much memory that may take, so
a file that falsely claims to be an enormous picture is attached without a
thumbnail and costs nothing more.

The storage is a third party and is not trusted. A device sends it the
sealed file and the link the server signed, and nothing else: not its
token, and no header of its own. What the storage returns is kept on a
device only if every piece opens with the file's key and the result is the
size the note says. Anything else is discarded and reported as damaged. The
storage, or anyone who can write to it, can therefore withhold a file or
destroy it, and cannot replace it with another.

A link to the storage is the permission to send or fetch one file, for
fifteen minutes or five. Neither a device nor the server writes one to a
log or shows one on a screen: a request to the storage that fails is
reported without the address it went to.

A file's name is whatever the device that attached it gave it, and reaches
other devices inside the note. No client uses it as a path. Where a copy is
saved under that name, only the name's last part is used, without the
characters a file system refuses and never as one of the names Windows
keeps for devices, so a note cannot make a device write outside the folder
its owner chose.

An app opens a file by handing a copy of it to whatever the system uses
for files of its kind. For a program, a script, a shortcut or a document
that carries macros, that is the system itself: the file would run. A file
reaches a device from another device of the account, and one of those may
be in the wrong hands. So the apps do not offer to open such a file, or one
with no ending to say what it is. It can be saved, and run from where it
was saved by somebody who means to. The list of kinds is
`dirt_core::files::runs_as_a_program`. A copy made for opening is the
store's file under the name the note gives it. It is kept in the store's
own folder, closed to other users of the computer, and not in the system's
shared place for temporary files. It is removed when the app next starts,
and with the account's folder at sign-out.

On Android a file is lent to another app through a file provider that is
closed to other apps, hands out one address at a time for reading only,
and can lend from one folder: the one a copy is put in to be opened. It
cannot reach the notes or the files themselves. A file that comes into the
app, chosen by the person or shared by another app, is copied into the
app's cache up to the size a file may be and no further, under a name of
the app's own; what the other app calls it travels beside it and is only
ever used as a name.

A file that a device holds is kept beside the notes' database, as it was
attached: not encrypted, as the notes there are not. What protects both is
the device, and on a computer with several users, the data folder.

**The data folder is its owner's alone.** On Linux and macOS a new folder
gets the permissions the system gives any folder, which on many computers
lets every user read it, and a person's home is often open to the others.
So the apps and the CLI make the data folder for its owner only, and close
one that is already there and open, made by an earlier version. They do it
each time they start, before they write anything there. Closing the folder
closes everything in it. Only what others were allowed is taken away: what
the owner allowed themselves is left as it was.

Three things are outside this:

- A database file given by hand (`dirt --db-path`, `DIRT_DB_PATH`) is in a
  folder of the person's choosing, which is left as they made it.
- A folder that cannot be closed is used as it is, and the log says so. It
  may be on a drive that keeps no permissions, where refusing would leave
  the person with no notes at all.
- On Windows a person's data folder is already theirs alone, and on
  Android an app's is. Nothing is changed there.

The folder named by `DIRT_DATA_DIR` is the data folder and is treated as
one: it is closed like any other. It should be a folder for Dirt and not
one that holds other things.

**An export leaves Dirt's care.** It is every note and every file, not
encrypted, in a folder or a zip file that the person puts where they like.
Nothing here protects it after that. What the apps do see to:

- A file's place in the export is made from its name as a saved copy's is:
  the last part only, without what a file system refuses. Two files of one
  note that would have the same name, by any rule a file system compares
  names by, get different ones. So an export writes inside its own folder
  and nowhere else, whatever a note calls a file.
- An export replaces nothing and is mixed into nothing. It takes its
  folder's name by making the folder, which fails if anything has that
  name at that moment. The files are written first and the notes' text
  last, and if the writing fails, what was written is removed.
- A file the device does not hold is fetched as any file is, and checked as
  any file is. One that cannot be had is left out, and the export's own
  text and the app both say which.
- On Android the zip file is made in the app's own folder, beside the
  notes, and copied by Android to the place the person chooses. The app's
  copy is removed when that is done, at the next start if the app was
  stopped meanwhile, and with the account at sign-out.

**Backups.** The operator keeps dumps of the server's database for about
thirty days. A dump holds what the server holds, so it holds no note that
anyone but the account's devices can read. It is encrypted to a key the
operator keeps off every server, and the storage it sits in cannot read it.
Two things follow from having backups at all:

- A deleted account stays in the dumps taken before it was deleted until
  they are thirty days old.
- If the operator ever restores a dump, the server goes back to what it
  knew then. A device removed since is in the account again until it is
  removed again, and a deleted account exists again until the operator
  deletes it again. `docs/DEPLOY.md`, "After a restore", is what the
  operator has to do and tell people.

## The keys

| Key | Made | Kept | Used for |
|---|---|---|---|
| Account key, 256 bits | At random, by the account's first device | In each approved device's secure storage. Never on the server unencrypted | Encrypting every note |
| Device key pair, X25519 | At random, by each device at first sign-in | Private half in the device's secure storage; public half on the server | Identifying the device in an approval, and receiving a new account key when the key is changed |
| Device token, 256 bits | By the server, at sign-in | On the device. The server keeps only its SHA-256 hash | Proving the device to the server on each request |

Each note is encrypted with XChaCha20-Poly1305 under the account key, with a
fresh random nonce. The account, the note's ID, the key version and the
format version are bound into the encryption, so a blob moved to another note
or another account does not decrypt. A device refuses a version of a note
older than one it has seen.

No cipher or key agreement is implemented in this repository. The ciphers
come from the RustCrypto crates, the key agreement and short code from
`vodozemac`, and the word list from `bip39`.

## Signing in

The app opens the sign-in service's page (Clerk) in the system browser, using
OAuth's authorization-code flow with PKCE as a public client: there is no
client secret to steal. The app receives an ID token and shows it to the Dirt
server once. The server checks its signature, issuer, audience, expiry and a
nonce the server itself issued for this one sign-in, then registers the
device and gives it a device token. After that the sign-in service is not
involved.

The browser comes back to the app with an address that carries the
authorization code. On a computer that is a request to a port the app
listens on, on this machine only. On Android it is the custom address
`dev.dirt.app://callback`, which any app may claim; PKCE is what makes the
code useless to an app that takes it. The Android app opens the page in a
custom tab, which is the browser's own tab laid over the app: the page and
what is typed into it stay in the browser, out of the app's reach. The app
holds the returned address in memory until the sign-in is finished and
writes it to no file.

The first device of an account is approved at once and makes the account
key. Every later device waits.

## Approving a device

A waiting device gets the account key from an approved one. The server
carries their messages and could change any of them, so the exchange is built
to make that useless. It is the short-code verification Matrix uses, and the
key agreement and the code come from Matrix's own implementation.

1. The new device makes a temporary key pair and sends a **commitment**: a
   hash that fixes its public key without showing it.
2. The approving device makes its own temporary key pair and sends its public
   key.
3. The new device **reveals** its public key. The approving device checks it
   against the commitment.
4. Each device derives a shared secret, and from it a **code** of three
   groups of four digits. Both show it. The person says on each device
   whether the two are the same.
5. If they are, the approving device encrypts the account key under a key
   derived from the shared secret and leaves it with the server. The new
   device opens it, saves it, and tells the server, which only then marks it
   approved.

Why the commitment matters: whoever sits in the middle has to fix its key
towards the approving device before it sees that device's key. It cannot try
keys until both sides show the same code. It gets one guess at a code of
about forty bits.

The code covers both temporary keys, both devices' IDs and registered public
keys, the account and a random ID for the exchange. A server that swaps any
of them changes the code.

If the codes differ, the person says so, the waiting device is removed from
the account, and both devices say that someone may be interfering.

## The recovery phrase

Optional. An approved device can make one: twenty-four words that stand for
256 random bits, with a checksum that catches a mistyped word. It is shown
once and stored nowhere.

Two values are derived from the phrase with HKDF, each bound to the account.
One is a key under which the account key is encrypted; the server keeps that
encrypted copy. The other is a claim code; the server keeps only its hash.
A waiting device where the person types the phrase opens the copy on the
device, then presents the claim code, and the server approves it. Neither
thing the server keeps helps it open the account key.

A waiting device may read the copy and claim with it. It may not create,
replace or delete it. Otherwise someone who took over the person's email
could destroy the way back in, or set a claim code of their own.

Making a new phrase ends the old one. So does removing it.

## Changing the account's key

A device that is removed can no longer reach the server, but it still has
the account key. Changing the key is what makes that key worthless for
anything written afterwards. The apps offer it when a device is removed, and
it can be asked for at any time.

The device that changes the key makes a new one at random and wraps it once
for every device that stays, itself included. The server stores the wrapped
copies and moves the account to the next key version, in one step. From then
on:

- the server refuses a note written under the old key;
- each device fetches its own copy at its next sync, opens it, and writes
  under the new key;
- whichever devices sync send the notes they hold again under the new key,
  until none is left under the old one.

A device that was away fetches its copies when it comes back. If the key was
changed more than once meanwhile, it opens them in order.

**What a wrapped copy is sealed with.** Two secrets together, through
HKDF-SHA256:

- A secret that only the sender and the receiving device can compute: an
  X25519 agreement between a key pair made for that one copy and the
  receiving device's registered key. A removed device has the old account
  key but not the other devices' private keys, so it cannot open their
  copies.
- The account key being replaced. Every device of the account has it; the
  server never has. The server knows every device's public key, so it could
  wrap a key of its own choosing for each of them. A device does not accept
  it: the copy does not open without the old account key. The device then
  stops syncing and says it has to sign in again, and adopts nothing.

The new key is sealed with XChaCha20-Poly1305, and the account, the
receiving device, the key version and both public keys are bound in, so a
copy cannot be given to another device or passed off as another version.

**The recovery phrase stops working** when the key changes, because its copy
on the server holds the old key. The apps say the account has no phrase and
offer to make one.

**What this does not stop.** A removed device and the server acting
together. The device that changes the key learns the other devices' public
keys from the server. It has checked, by comparing a code, only the keys of
the devices it let in or was let in by. If the server gives it a false key
for one of the others and passes what it receives to a removed device, the
two together hold both secrets and can open that copy. Neither can alone.
Closing this would take every device checking every other device's key,
which is what comparing codes does for a pair.

A note that no device can send again, because no device can read it, stays
under the old key. Everything written since the change is under the new one.

## Losing things

| Lost | What happens |
|---|---|
| One device of several | Sign in on a new one and approve it from another. Remove the lost one from the device list, and change the account's key |
| Every device, with a recovery phrase | Sign in on a new device and type the phrase |
| Every device, with no phrase | The notes on the server cannot be read by anyone. The account can be reset, which deletes them, and started again |
| The recovery phrase | Nothing, while a device still works. Make a new one |

**Signing out** deletes the account's notes and keys from the device. It is
refused on an account's only approved device unless a recovery phrase
exists, because that device's copy of the key would be the last.

**Resetting** an account deletes its notes on the server, its recovery copy
and every other device, and makes the device that asked the first device of
a fresh start, with a new key. An approved device resets at once. A waiting
device resets at once only if the server holds no notes for the account;
otherwise it asks, every approved device is told at its next sync, and the
reset happens seven days later unless one of them refuses it.

**Deleting** an account removes everything the server holds for it. The
device that did it keeps its copies of the notes, outside any account.

## Reporting a problem

Open an issue on the repository. There is no private channel yet: GitHub's
private vulnerability reporting is not switched on for it. Step 8 of the
redesign, the hardening step, decides how reports are taken before anyone
relies on a public server.
