# Decisions for the owner

Six things were the owner's to decide. Each costs money, opens an account
in the owner's name, or changes a setting only the owner can change, so none
was taken while the redesign was being built. **The owner took all six on
2026-11-13.** Each is written up here with what was known, what it costs,
the recommendation, and then what was decided and what was set up.

The facts were read from each provider's own pages on 2026-11-12. Prices
change: check them again before paying.

| Decision | Recommended | Decided | Costs today |
|---|---|---|---|
| [Where dumps are kept](#1-where-the-database-dumps-are-kept) | Backblaze B2 | Backblaze B2. Running | Nothing |
| [The database plan](#2-the-database-plan) | Neon's Launch plan before a public launch | The free plan, for now | Nothing |
| [The hosting plan](#3-the-hosting-plan) | Stay on Vercel's Hobby plan | Hobby | Nothing |
| [The sign-in service](#4-the-sign-in-service-for-production) | A Clerk production instance on a domain | Done, on `dirt.catjam.dev` | Nothing more: the owner had the domain |
| [How security reports are taken](#5-how-security-reports-are-taken) | Switch on GitHub's private reporting | Switched on | Nothing |
| [The storage for attachments](#6-the-storage-for-attachments) | Cloudflare R2 | Cloudflare R2 | Nothing |

What the decisions made is one more server, for real use, beside the trial
server. `docs/DEPLOY.md`, "What exists", lists both.

[What is still the owner's](#what-is-still-the-owners) is at the end.

## 1. Where the database dumps are kept

**What is built.** `.github/workflows/backup.yml` dumps the database once a
day, encrypts the dump and stores it in any storage that speaks the S3
protocol. It does nothing until its settings exist. `docs/DEPLOY.md`,
"Backups", lists them.

**Why outside Neon.** Neon's own history is the first thing to restore
from. The dump is for the day Neon itself is the problem: the project
deleted, the account closed, or the history window passed.

**What the storage is trusted with.** Only keeping the file. The dump is
encrypted before it leaves the workflow, to a key whose private half the
owner keeps and GitHub never has. Whoever reads the bucket learns how big
the database is and nothing else.

| | Backblaze B2 | Cloudflare R2 | GitHub Actions artifacts |
|---|---|---|---|
| Free allowance | The first 10 GB | 10 GB a month | Kept 90 days by default |
| Beyond it | From $6.95 a terabyte a month | $0.015 a gigabyte a month | Not applicable |
| Taking data out | Free up to three times what is stored | Free | Free |
| A key that can add files and not delete them | Yes: a key can be limited to one bucket and to writing | Not found: a token is "Object Read & Write" or "Object Read only" | No keys |
| Deleting old dumps by rule | Yes, per bucket, in days | Yes, in days | By the retention setting |
| To start | An account. Whether the free allowance needs a card is not stated | "You must purchase R2 before you can generate an API token" | Nothing |
| Who can fetch a dump | Whoever holds a key that can read | Whoever holds a token | Not stated for a public repository |

**Recommendation: Backblaze B2.** The deciding point is the key. The
workflow's key sits in GitHub's secrets, and a key that can only add files
means that someone who steals it can fill the bucket and cannot read or
destroy the dumps already there. Set a rule on the bucket that hides a dump
30 days after it is uploaded and deletes it a day later.

GitHub's artifacts are not recommended although they cost nothing. This
repository is public, GitHub's pages do not say who may download a public
repository's artifacts, and an encrypted copy of every account's data
should not depend on that.

**How large.** A dump is at most the size of the database, which on Neon's
free plan is 1 GB. Thirty of those are 30 GB, about 20 cents a month on B2.
Today's dumps are a few kilobytes.

**Decided (2026-11-13): Backblaze B2, and it runs.** A private bucket holds
the dumps of the production database. The workflow stored its first dump,
and that dump was fetched and restored into an empty database with the
owner's key.

**The recommendation had one thing wrong.** A B2 key that may only write
can still do two things to a dump whose name it knows: hide it, and upload
another file under its name. The bucket's rule deletes a hidden file a day
later, and a replaced file counts as hidden. So the key alone did not keep
a thief from destroying dumps. Tried on the new bucket: the key hid a test
file and replaced it.

What closes it is Object Lock. The bucket keeps every file for 30 days
from its upload, whatever is done to its name, and Backblaze's pages say a
rule does not delete a locked file. The rule then removes each dump on the
day after its lock ends. Tried with the workflow's key: it could add a
file, and could not list the bucket, read a file or delete a version. The
lock is in governance mode, which the owner's own login can pass, so the
owner can still delete a dump. `docs/DEPLOY.md`, "Setting up the daily
dump", has the commands.

**Still the owner's:** the private half of the encryption key. It is a
file on the owner's computer and nowhere else. It belongs in a password
manager and on paper: without it no dump can be read.

## 2. The database plan

**The problem with the free plan.** Neon's free plan gives a project 100
compute-unit hours a month, which is 400 hours awake at the smallest size.
A month has 730. The database sleeps after five idle minutes and wakes for
any request that reaches it.

- Used honestly it is enough: sync runs on events, so the database is awake
  for about five minutes after each one.
- **It can be used up on purpose by anyone.** One request every five
  minutes keeps the database awake all month, the allowance is gone on the
  seventeenth day, and the database is suspended until the next month. No
  rate limit can see a request every five minutes. `docs/LIMITS.md`, "What
  no rule stops", has the detail.
- It holds 1 GB. When that is full, writes that grow the database fail.
- Its history reaches back six hours.

**The Launch plan** has no monthly fee and charges for what is used:

| | Free | Launch |
|---|---|---|
| Compute | 100 unit-hours a month, then suspended | $0.106 a unit-hour |
| Storage | 1 GB, then writes fail | $0.35 a gigabyte a month |
| History to restore from | 6 hours | Up to 7 days, at $0.20 a gigabyte a month of changes |
| Sleeping when idle | After 5 minutes, always | After 5 minutes; can be switched off |

A database of the smallest size that never sleeps costs 0.25 x 730 x $0.106,
about $19 a month. That is the most the trickle above can cost, where on
the free plan it costs two weeks without sync. With honest use the database
is asleep most of the day and the bill is a few dollars.

**Recommendation: move to Launch before telling anyone else the server's
address.** Until then the free plan is fine: only the owner is affected if
it is suspended. Set the history window to seven days when moving.

**Not checked:** whether Launch lets the compute size be capped at the
smallest, which is what bounds the bill at $19.

**Decided (2026-11-13): the free plan, for now.** The production database
is a Neon project of its own on the free plan, beside the trial's. What is
said above still holds: anyone who learns the server's address can use the
month's allowance up, and the history reaches back six hours. The daily
dump is what reaches further. Moving to Launch stays the thing to do before
the address is given to people who would miss the server for two weeks.

## 3. The hosting plan

Vercel's Hobby plan "is for personal, non-commercial use". Vercel's
definition: "Commercial usage is defined as any Deployment that is used for
the purpose of financial gain of anyone." Taking payment from visitors and
showing advertising are its examples. "Asking for Donations does not fall
under commercial usage."

The Pro plan is $20 a month for one developer seat and includes $20 of
usage. It also raises the firewall's allowance from one rate-limit rule to
forty.

**Recommendation: stay on Hobby** for as long as Dirt is free to use and
shows no advertising. Move to Pro on the day it charges for anything.

Nothing measured so far needs Pro. The one rate-limit rule Hobby allows is
the one `docs/LIMITS.md` uses.

**Decided (2026-11-13): Hobby.** The production server is a Hobby project.
The one rule is one for each project, so the production project has its
own.

## 4. The sign-in service for production

The server on `dirt-trial` signs people in through a Clerk development
instance. Clerk caps those at 100 users, and their users cannot be moved to
another instance.

A production instance needs "a domain you own" and the ability to add DNS
records to it. Clerk says a host's own domain, such as `*.vercel.app`, will
not do. Clerk's free plan covers 50,000 monthly retained users for an app
and asks for no card.

So the cost is a domain. The server then runs on that domain too, which
also gives the clients an address that does not belong to a hosting
company.

**Recommendation: buy a domain and make the production instance when Dirt
is to have users other than its owner.** Nothing in the code changes: the
server is given the instance's address and client ID, and tells the clients
(`docs/DEPLOY.md`).

**Decided (2026-11-13): done, on the owner's domain.** The production
instance signs people in at `accounts.dirt.catjam.dev`, and its tokens come
from `https://clerk.dirt.catjam.dev`. The production server is at
`https://api.dirt.catjam.dev`. `docs/DEPLOY.md` has how it was set up,
which for a Clerk application made through Vercel is not what Clerk's pages
describe.

- **How people sign in:** with a code sent to their email address, and
  nothing else. Passwords are off, as on the trial instance.
- **Google is not offered,** though the trial instance offers it. In
  production Clerk wants the application's own Google credentials, which
  are made in a Google Cloud account. That account is the owner's to open.

The two things that could not be settled from Clerk's pages were looked at
in the dashboard:

- **The consent page can be switched off,** for each OAuth application. It
  is left on. Dirt's client is a public one: any program on the computer can
  start a sign-in in its name, and the page is where the person sees that
  happen. Switching it off saves one press at each sign-in and is the
  owner's to weigh.
- **Clerk offers the device-code grant,** also for each OAuth application,
  and it is off. So a computer with no browser can be served the usual way:
  the CLI shows a short code and the person enters it on another device.
  The CLI does not do this, and nothing plans it yet: the design lists it
  under details not yet decided (section 13). Until it is built,
  `dirt auth login --no-browser` with a forwarded port is what there is.

## 5. How security reports are taken

`docs/SECURITY.md` says to open an issue, which is public. A report of a
real weakness should not be.

GitHub's private vulnerability reporting lets "owners and administrators of
public repositories" switch on a "Report a vulnerability" button on the
repository's Advisories page. A reporter fills in a summary, details, a
proof of concept and the impact; only the maintainers see it.

**Recommendation: switch it on now.** It is in the repository's Settings,
under Advanced Security, to the right of "Private vulnerability reporting".
When it is on, `docs/SECURITY.md`'s last section should say to use that
button, and that is a two-line change.

**Decided (2026-11-13): switched on,** and `docs/SECURITY.md` says to use
it.

## 6. The storage for attachments

**What it is for.** Step 9 of the redesign adds files attached to notes.
A device encrypts a file and sends it straight to object storage, through a
link the server signs; other devices fetch it the same way. The server
never handles the bytes, and the storage holds only ciphertext.

**Why it is separate from the dumps' storage.** Devices download from it
directly, so what matters is the price of taking data out, and that a
phone can reach it. For the dumps what matters is a key that cannot delete.

| | Cloudflare R2 | Backblaze B2 |
|---|---|---|
| Free allowance | 10 GB a month | The first 10 GB |
| Beyond it | $0.015 a gigabyte a month | From $6.95 a terabyte a month |
| Taking data out | Free | Free up to three times what is stored, then $0.01 a gigabyte |
| To start | "You must purchase R2 before you can generate an API token" | An account |

**Recommendation: Cloudflare R2,** as the design says. Downloads cost
nothing however often devices fetch.

If one account is preferred to two, R2 can hold the dumps as well. The
cost is the point made under decision 1: R2's tokens are "Object Read &
Write" or "Object Read only", so the backup workflow's key could delete
dumps as well as add them.

**How much.** An account may hold 200 MiB of files, and one file may be
10 MiB. `docs/LIMITS.md` has what was measured for those numbers; they are
the owner's to change. An account may also hold 20,000 files however small,
which is about what the database is asked to keep and not about the
storage. Fifty accounts filled to the top are the free
10 GB; past that, a full account costs about a third of a cent a month.
Anyone can sign up, so the bill has no ceiling of its own. It grows by that
much for each account that fills itself.

**Decided (2026-11-13): Cloudflare R2.** The owner's Cloudflare account
already had R2. Two private buckets were made in it, in western North
America beside the servers: `dirt-files` for the production server and
`dirt-trial-files` for the trial server.

**Not done yet: the two tokens.** Each server needs a token that may read
and write objects in its own bucket and nothing else. Cloudflare's
command-line program made the buckets and cannot make tokens: its sign-in
is not allowed to. They are made in Cloudflare's dashboard, under R2,
"Manage API Tokens", and that needs the owner signed in. Until each
server has its token it syncs notes and answers `files_unavailable` to
anything about files, as before.

**Until then:** attachments are built and tested against a store on the
developer's machine that speaks the same protocol. They cannot be tried on
a phone.

## What is still the owner's

Nothing here blocks the owner's own use of the production server.

| What | Why it is the owner's | When |
|---|---|---|
| Keep the dumps' private key | It is one file on the owner's computer. Without it no dump can be read | Now |
| Sign in to Cloudflare's dashboard for the two R2 tokens | Only the dashboard makes them | Before attachments work on either server |
| Sign in to the production server for the first time | The first device of an account makes its key and should be the owner's own | When the owner starts using it |
| Google sign-in on the production server | Needs credentials from a Google Cloud account | If it is wanted |
| Neon's Launch plan | Costs money | Before the address is given to others |
| The consent page at sign-in | A trade of one press against one check, on the owner's instance | Whenever |
| Sign-in by device code for the CLI | New work, not planned | If it is wanted |
| The rest of `docs/MANUAL_CHECKLIST.md` | Needs a person, a phone and a Mac | Before the address is given to others |

## Sources

- Backblaze: [pricing](https://www.backblaze.com/cloud-storage/pricing),
  [lifecycle rules](https://www.backblaze.com/docs/cloud-storage-lifecycle-rules),
  [application keys](https://www.backblaze.com/docs/cloud-storage-application-keys).
- Cloudflare R2: [pricing](https://developers.cloudflare.com/r2/pricing/),
  [object lifecycles](https://developers.cloudflare.com/r2/buckets/object-lifecycles/),
  [API tokens](https://developers.cloudflare.com/r2/api/tokens/).
- GitHub: [workflow artifacts](https://docs.github.com/en/actions/concepts/workflows-and-actions/workflow-artifacts),
  [private vulnerability reporting](https://docs.github.com/en/code-security/security-advisories/working-with-repository-security-advisories/configuring-private-vulnerability-reporting-for-a-repository).
- Neon: [plans](https://neon.com/docs/introduction/plans).
- Vercel: [pricing](https://vercel.com/pricing),
  [the Hobby plan](https://vercel.com/docs/plans/hobby),
  [fair use](https://vercel.com/docs/limits/fair-use-guidelines),
  [rate limiting](https://vercel.com/docs/vercel-firewall/vercel-waf/rate-limiting).
- Clerk: [pricing](https://clerk.com/pricing),
  [deploying to production](https://clerk.com/docs/deployments/overview),
  [environments](https://clerk.com/docs/deployments/environments),
  [Clerk as an identity provider](https://clerk.com/docs/advanced-usage/clerk-idp).
