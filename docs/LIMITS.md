# Limits

Every number the server enforces, why it is that number, and what was
measured to check it. The numbers themselves live in one place in the code,
`dirt_proto::api::limits`, which the server and the clients both read.

This is the limits review of step 8 of the redesign
(`docs/plans/2026-11-12-step-8-hardening-plan.md`, part 8c), carried out on
2026-11-12.

## How the measurements were made

`crates/dirt-sim/tests/limits.rs` holds them. They print numbers and assert
nothing, so they are ignored in an ordinary test run:

```bash
docker compose up -d
export DATABASE_URL=postgres://postgres:dirt@127.0.0.1:55432/postgres
cargo test --release -p dirt-sim --test limits -- --ignored --nocapture --test-threads 1
```

They run real client cores against the real server in one process, with
Postgres 18 in Docker on the same machine (a 13th-generation Core i5 laptop
with 16 GB of memory). Nothing crosses a network. The request counts are
therefore exact, and the times are the least an operation can take: a
deployed server adds about 60 ms to each request (design section 7.5).

## What was measured

### One note

| Text | Edits afterwards | Blob on the server |
|---|---|---|
| Empty | 0 | 54 bytes |
| 80 characters | 0 | 165 bytes |
| 280 characters | 0 | 367 bytes |
| 280 characters | 20 | 636 bytes |
| 2,000 characters | 0 | 2,087 bytes |
| 2,000 characters | 200 | 4,704 bytes |
| 20,000 characters | 0 | 20,090 bytes |
| 1,000,000 characters | 0 | 1,000,090 bytes |

A blob is the text, 13 to 50 bytes of document, and 41 bytes of encryption.
The document keeps its history, so each edit adds to it: about 13 bytes for
an edit that adds a word.

Deleting a note adds a marker to its document:

| Devices that had edited the note | Added by the delete |
|---|---|
| 1 | 18 bytes |
| 2 | 21 bytes |
| 5 | 33 bytes |
| 20 | 104 bytes |

### An account holding 20,000 notes

Each note was one line of about 70 characters with a tag.

| What | Requests | Time |
|---|---|---|
| Writing the notes into the local store | none | 14.8 s |
| Sending them all for the first time | 42 | 67 s |
| A new device fetching them all | 41 | 15 s |
| Changing the account's key and sending every note again | 166 | 76 s |
| Another device following that change | 41 | 15 s |

The server held 3.6 MB of blobs for the account, which is 6.8% of what an
account may hold. Postgres used 7.3 MB for those rows and their indexes:
about 360 bytes a note, twice the blob.

### The local store with 20,000 notes in it

| What an app asks for | Time |
|---|---|
| The 100 most recently edited notes | 0.2 ms |
| The 1,000 most recent | 0.8 ms |
| The 5,000 most recent | 3.8 ms |
| All 20,000 | 15 ms |
| The 100 oldest | 6.5 ms |
| Up to 100 notes holding a word that every note holds | 27 ms |
| The same, by the start of the word | 26 ms |
| Up to 100 notes holding a word and carrying a tag | 9 ms |
| Up to 100 notes carrying a tag | 2 ms |
| The one note holding a word | 0.2 ms |
| The tags in use, with their counts | 2.5 ms |
| How many notes there are | 1 ms |

### Letting a device in

| What | Measured | Allowed |
|---|---|---|
| Messages on the server at the end of an approval exchange | 3 | 32 |
| The longest of them | 194 bytes | 1,024 bytes |
| The account key wrapped for a waiting device | 72 bytes | 256 bytes |
| The account key wrapped under a recovery phrase | 72 bytes | 256 bytes |
| The account key wrapped for a device when the key changes | 105 bytes | 256 bytes |

## Each limit and its reason

### Notes

| Limit | Number | Why this number |
|---|---|---|
| One note's blob | 1 MiB | A note is plain text, and a million characters of it fit. Three such notes fit in one batch. A note is sent whole each time it changes (design section 11), so a larger one would cost its size on every edit |
| Blobs in one push or one pull page | 3 MB | The hosting platform refuses a request body over 4.5 MB: 4,493,163 bytes were accepted and 4,494,140 refused (trial 4). Blobs travel as base64, which makes 3 MB into 4 MB, and 500 notes add about 60 KB of JSON around them. The server's own limit on a body is 4.5 MB |
| Notes in one push | 500 | An account at its note limit is sent in 40 pushes. One push is one transaction on the account's row |
| Notes per account | 20,000 | Everything that touches the whole account stays well inside the request limit and takes a minute or so: see the table above. The local store answers in milliseconds at this size. A deleted note stops counting when it is purged, so this is a limit on notes kept, not on notes ever written |
| A deleted note is purged | after 30 days | Long enough for a device that was away to come back and for its edits to merge, which brings the note back. Each device counts from when it saw the note deleted, so no two clocks are compared |
| Purged notes per account | 100,000 | A purged note leaves a marker, so that every device learns of the purge, and markers are never removed. One costs the database 181 bytes with its indexes, so the most an account can leave is 18 MB. Someone who deletes five notes a day reaches it in 55 years |
| Blobs per account | 50 MiB | 20,000 one-line notes are 3.6 MB. If every one were 280 characters edited 20 times they would be 12.7 MB. 50 MiB leaves four times that for long and much-edited notes. For notes averaging under 2,621 bytes the note limit is reached first |
| Growth a full account still takes | 1 KiB a push, up to 105% | The server cannot tell a delete from an edit, and a full account must still take deletes. A delete adds 18 to 104 bytes |

A deleted note keeps its document so that the delete can sync, and so that
an edit made on another device without knowing of the delete can bring the
note back. It counts against the note limit until it is purged, thirty days
after a device saw it deleted. The review found that nothing purged them:
an account that had ever written 20,000 notes was full for good. Part 8e of
the step 8 plan built the purge.

**Why markers are kept for ever.** A marker could be dropped once every
device had seen it. The server would have to track how far each device has
pulled, and a device that came back after its marker was dropped would
bring the note back from its own copy, deleted notes reappearing months
later. Keeping the markers costs 181 bytes each and has neither problem.
The limit of 100,000 is what bounds it.

### Requests

| Limit | Number | Why this number |
|---|---|---|
| Requests per account | 600 in ten minutes | The most one device needs at once is 166, for changing the key of a full account. Five devices doing their heaviest work in the same ten minutes need 330. An ordinary sync is a handful of requests |

A device over the limit is told how many seconds to wait, and waits.

The count is a row in the database, because serverless instances share no
memory (design section 7.6). So a refused request still costs one write. The
limit protects the account's share of the work; it does not keep requests
away from the database. That is the firewall's part, below.

### Devices

| Limit | Number | Why this number |
|---|---|---|
| Approved devices per account | 20 | A change of the key carries a copy for each device in one request, and each device stores copies of later keys. A computer counts once for its CLI and its desktop app together. The limit is applied when a device is approved and never when it signs in, so a person who has lost every device can still sign a new one in |
| Devices the recovery phrase lets in past that | 3 | With every place taken by devices the person no longer has, the phrase must still get them in. The device it lets in can remove the lost ones |
| Devices waiting for approval | 3 | Enough for a person setting up their devices. Someone who has only the Clerk sign-in can add no more than this for the account's devices to look at |
| A request to join lapses | after 7 days | Long enough to reach another device. The request can be renewed by signing in again |
| A removed device, or an unanswered request, is forgotten | after 30 days | Until then a removed device is told that it was removed, which is the more useful message. Afterwards its token is unknown |
| Removed devices remembered | 20 | Signing out of a device removes it, so a device that signs in and out would otherwise add a row each time |
| Copies of later keys kept for a device | 100 versions | A device follows the key by opening each copy it missed in turn. Nobody misses 100 changes. A device that did is put back among those waiting and let in again, with the notes it holds |
| A device token unused | stops working after 90 days | A lost device stops being a way in without anyone acting. The device signs in again and stays the device it was |
| Device name | 100 bytes | It is shown in a list |
| A reset asked for by a waiting device | takes effect after 7 days | Time for a device that is still in the account to refuse it |
| A sign-in nonce | 10 minutes | Time to complete a sign-in in the browser |

With every limit at its most, what one account can make the server hold
outside its notes is well under a megabyte: some 60 device rows, 2,300 key
copies of about 105 bytes, 96 messages of 1 KiB, and a day of request counts.

### Files

Step 9 of the redesign. These numbers are proposals: the server enforces
them, and no client attaches a file yet, so nothing has been measured
against use. Part 9f of the step 9 plan measures them. Storage past the
free allowance is paid for, so they are also the owner's to change
(`docs/OWNER_DECISIONS.md`, decision 6).

| Limit | Number | Why this number |
|---|---|---|
| One file | 10 MiB | A photograph from a phone, a short recording, a PDF. It is uploaded in one request, so there is no resuming to build |
| Files per account | 200 MiB | The storage's free allowance is 10 GB for every account together. A file is counted once, however many notes refer to it, and from the moment a link is issued |
| Files a note refers to | 20 | Each adds its key and name to the note's document, which is sent whole on every edit. A picture's thumbnail is a file and counts as one, so a note holds ten pictures. A picture that fits only without its thumbnail is attached without |
| An upload no note confirmed | discarded after 7 days | A device asked for a link and never finished. Its space comes back |
| A file no note refers to | deleted after 30 days | A device that was away with an unsent edit to a purged note keeps the note's text as a new note, and its files with it. The files have to be there still |
| An upload link | 15 minutes | Time to send 10 MiB on a slow connection. It is for one object of one exact size |
| A download link | 5 minutes | A device asks for one each time it needs a file it does not hold |
| An object whose row has gone | deleted an hour later | Four times the life of an upload link. A link issued just before the row went can still be used, and an upload that arrived after the object was deleted would put it back with nothing to remember it by |

### Sizes fixed by a format

A device's public key is 32 bytes (X25519). The hash of a recovery claim is
32 bytes (SHA-256), and the claim code is read up to 64 bytes. A wrapped
account key is read up to 256 bytes; the largest made today is 105. A
message in an approval exchange is read up to 1 KiB; the largest is 194
bytes, and an exchange holds 3 at its end against 32 allowed.

## What the review changed

- **Search took 22 seconds in a store of 20,000 notes** when the word was
  one that most notes held. SQLite read the notes first and asked the
  full-text index about each one in turn. The query now names the order: the
  index first. The same search takes 27 ms. A test reads the query's plan
  and fails if the order changes.
- **An account could have any number of devices.** Only waiting devices were
  limited. The limit is 20 approved devices. It was first applied when a
  device signed in. Review showed that this could lock a person out for
  good: with all 20 lost, no new device could sign in to use the recovery
  phrase or to ask for a reset. It is applied when a device is approved,
  and the recovery phrase lets in 3 more.
- **A removed device's row was kept for ever,** and so was a request to join
  that nobody answered. They are forgotten after 30 days, and no more than
  20 removed devices are kept.
- **Every change of the key left a copy for every device for ever.** One
  account could fill the database with them at the rate its request limit
  allows. The 100 latest versions are kept. Review showed that a device
  further behind than that was stuck: it was told to sign in again, and
  signing in again left it the same approved device with the same old key.
  The server now puts such a device back among those waiting, so that
  signing in again leads to being let in with the present key.
- **The apps' lists stopped at 100 notes.** Search and the tag row already
  asked the store, which looks through every note. The list now has "Show
  older notes", which adds 100 at a time. Showing them all at once was not
  needed: the store can list 20,000 notes in 15 ms, but the app would have
  to draw them, and how long that takes was not measured.

## Requests from outside an account

The server's own limits apply to a device that is signed in. Before that,
the design leaves requests to the hosting platform's firewall (section 7.6).
This is what the firewall does, and what was tried.

### Which requests reach the database

| Request | Reaches the database |
|---|---|
| `GET /healthz` | No |
| Any path under `/v1` with no token | No. Refused first |
| `POST /v1/devices` with a sign-in the server does not accept | No. The sign-in is checked against Clerk's keys first, and the nonce is not spent |
| `POST /v1/nonces` | Yes: one row written, up to 100 expired rows removed |
| Any path under `/v1` with a token, real or made up | Yes: one read to look the token up |

### What Vercel's firewall does on the free plan

From Vercel's documentation, read on 2026-11-12:

- Attacks at the network level and floods of requests are detected and
  blocked without any setting, on every plan. Traffic blocked this way is
  not charged for. Requests served before an attack is recognised are.
- A project may have three rules of its own, one of which may be a rate
  limit. A rate limit counts by address or by the client's TLS fingerprint,
  over a fixed window of ten seconds to ten minutes. One million requests
  let through by a rate limit are included.
- "Attack Mode" puts a challenge in front of every request. The apps are not
  browsers and would fail it, so it is a last resort for an attack under way.

### What was tried

On the trial deployment, from one address:

1. With no rule set, 300 requests to `/v1/devices/me` without a token, 20 at
   a time. All 300 were answered by the server, in 20 seconds. A burst of
   that size is not an attack to the platform, and nothing slowed it.
2. A rule was added: one address may make 1,200 requests to paths under
   `/v1/` in ten minutes.
3. 1,400 more requests. Exactly 1,200 were answered by the server. The other
   200 were answered `429 Too Many Requests` by the firewall, with the
   header `X-Vercel-Mitigated: deny`, and did not reach the server.
   `/healthz`, which the rule does not cover, went on answering.

The firewall's refusal carries no body of the protocol and does not say how
long to wait. A client reads the status alone as `rate_limited` and backs
off, as it does for the server's own limit.

### The rule

One address may make 1,200 requests to `/v1/` in ten minutes. That is twice
what an account may make. One person's devices behind one address meet the
server's limit first, which tells them when to come back. The firewall's
limit is for what the server's cannot see: requests with no account.

Many phones can share one address on a mobile network. At this limit they
share two requests a second between them, which is plenty while Dirt has
few users and will need raising when it has many. `docs/DEPLOY.md` has the
command that sets the rule. It is set on the trial project.

### What no rule stops

The free database plan allows 100 compute-unit hours a month. At the
smallest size that is 400 hours awake, and a month has 730. The database
sleeps after five idle minutes, which the free plan does not let us change.

**One request every five minutes that reaches the database keeps it awake
all month.** The allowance is then used up on the seventeenth day, and the
database is suspended until the next month. For those two weeks nobody can
sync. The request can be `POST /v1/nonces`, which needs no account, and 288
of them a day is far below anything a rate limit could be set to.

Keeping such requests away from the database was considered: a nonce the
server can check by a signature, with nothing stored, and tokens it can
reject the same way. It was not built, because it only moves the problem.
Anyone can sign up, and one signed-in device syncing every five minutes
does the same thing, inside every limit above.

What does close it is a database plan with no monthly allowance. Neon's
Launch plan has no minimum fee and charges $0.106 a compute-unit hour, so a
database of the smallest size that never sleeps costs about $19 a month.
The attack then costs that much instead of two weeks without sync. This is
the owner's decision, and part 8d of the step 8 plan writes it up with the
others.

The same holds for storage. The free plan holds 1 GB for a project, and
when it is full, writes that grow the database fail. An account may hold 50
MiB of blobs, so about twenty accounts filled on purpose would fill it. A
typical account is far smaller: the 20,000 one-line notes above take 7.3 MB.

## Sources

- Vercel: [rate limiting](https://vercel.com/docs/vercel-firewall/vercel-waf/rate-limiting),
  [custom rules](https://vercel.com/docs/vercel-firewall/vercel-waf/custom-rules),
  [DDoS mitigation](https://vercel.com/docs/vercel-firewall/ddos-mitigation),
  [the firewall](https://vercel.com/docs/vercel-firewall/vercel-waf),
  [the Hobby plan](https://vercel.com/docs/plans/hobby).
- Neon: [plans](https://neon.com/docs/introduction/plans),
  [pricing](https://neon.com/pricing),
  [free plan limits](https://neon.com/faqs/free-plan-limits-and-quotas).
