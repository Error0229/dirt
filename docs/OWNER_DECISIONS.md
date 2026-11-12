# Decisions for the owner

Six things are the owner's to decide. Five have to be decided before Dirt
runs for people other than its owner; the sixth before attachments can be
tried on real devices. Each costs money, opens an account in the owner's name, or changes a
setting only the owner can change, so none was taken while the redesign was
being built. Each is written up here with what is known, what it costs and
a recommendation.

The facts were read from each provider's own pages on 2026-11-12. Prices
change: check them again before paying.

| Decision | Recommended | Costs | Blocks |
|---|---|---|---|
| [Where dumps are kept](#1-where-the-database-dumps-are-kept) | Backblaze B2 | Nothing at first | Backups running at all |
| [The database plan](#2-the-database-plan) | Neon's Launch plan | About $19 a month at most, usually far less | A public launch |
| [The hosting plan](#3-the-hosting-plan) | Stay on Vercel's Hobby plan | Nothing | Charging money |
| [The sign-in service](#4-the-sign-in-service-for-production) | A Clerk production instance on a domain | A domain's yearly fee | More than 100 users |
| [How security reports are taken](#5-how-security-reports-are-taken) | Switch on GitHub's private reporting | Nothing | Nothing; do it now |
| [The storage for attachments](#6-the-storage-for-attachments) | Cloudflare R2 | Nothing at first; needs a billing profile | Trying attachments on real devices |

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

**The owner does:** opens the storage account, makes the bucket, its rule
and its key; makes the encryption key with `age-keygen` and stores the
private half somewhere that is not GitHub and not the bucket; sets the
workflow's settings.

**Not tried:** the upload step. No bucket exists until this is decided. The
dump, the encryption and the restore were tried: see the step 8 plan.

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

Two things about the production instance could not be settled from Clerk's
pages, and have to be looked at in its dashboard when it exists:

- **The consent page.** After signing in, Clerk shows a page asking the
  person to allow Dirt. The pages read do not say whether it can be switched
  off for an app that is the instance's own.
- **A computer with no browser.** `dirt auth login --no-browser` prints an
  address to open, and the browser then has to reach a port on the computer
  the CLI runs on. On a server reached over SSH that needs a forwarded
  port. The usual answer is the device-code grant, where the person types a
  short code on another device. Clerk's pages do not mention it. If Clerk
  does not offer it, the CLI needs another way, such as pasting back the
  address the browser ends on. That would be new work and is not planned.

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

**How much.** The plan for step 9 proposes 200 MiB of files for an account
and 10 MiB for one file. Fifty accounts filled to the top are the free
10 GB; past that, a full account costs about a third of a cent a month.
Anyone can sign up, so the bill has no ceiling of its own. It grows by that
much for each account that fills itself.

**The owner does:** opens the R2 account, makes a bucket and a token
limited to it, and gives the trial server the bucket's address and the
token. `docs/DEPLOY.md` will list the settings when part 9a is built.

**Until then:** attachments are built and tested against a store on the
developer's machine that speaks the same protocol. They cannot be tried on
a phone.

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
