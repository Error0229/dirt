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

## Who is not kept out

- **Someone with the unlocked device.** The local database is not encrypted,
  so that search works on it. It relies on the operating system's disk
  encryption and screen lock. The keys are in the operating system's
  keychain, or in Android's encrypted preferences, which protect them from
  other apps and not from the device's own user.
- **A device that was removed, for what it already has.** Removing a device
  stops it reaching the server, so it reads nothing new. It keeps the account
  key it had and the notes it had. Changing the account key is what ends
  that, and is not built yet (step 8 of the redesign).
- **A server in the middle of an approval, if nobody compares the codes.**
  The comparison is the protection. Pressing "they match" without looking
  gives it away.

## What the server does see

How many notes an account has, how large each one's ciphertext is, when each
changes, which devices an account has and when they were last seen, the
person's email address as the sign-in service reports it, and network
metadata such as addresses. It does not see note text, tags or titles: those
are inside the ciphertext.

## The keys

| Key | Made | Kept | Used for |
|---|---|---|---|
| Account key, 256 bits | At random, by the account's first device | In each approved device's secure storage. Never on the server unencrypted | Encrypting every note |
| Device key pair, X25519 | At random, by each device at first sign-in | Private half in the device's secure storage; public half on the server | Identifying the device in an approval. Later, receiving a new account key |
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

## Losing things

| Lost | What happens |
|---|---|
| One device of several | Sign in on a new one and approve it from another. Remove the lost one from the device list |
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
