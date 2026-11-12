# Deploying the Dirt server

`dirt-api` is one axum application. Locally it runs as a program; in
production it runs as a single Vercel function, with Postgres on Neon.

## Run it locally

```bash
docker compose up -d                 # Postgres 18 on 127.0.0.1:55432
cp .env.server.example .env.server   # then set CLERK_ISSUER and CLERK_CLIENT_ID
cargo run -p dirt-api -- migrate     # create the tables
cargo run -p dirt-api                # serve on 0.0.0.0:8080
curl http://127.0.0.1:8080/healthz
```

Run the tests against the same database:

```bash
export DATABASE_URL=postgres://postgres:dirt@127.0.0.1:55432/postgres
cargo test -p dirt-api
```

Each test creates and drops its own database, so the tests never touch the
tables you migrated.

## Configuration

| Variable | Purpose |
|---|---|
| `DATABASE_URL` | Postgres connection string the server uses. On Neon, the **pooled** one |
| `MIGRATE_DATABASE_URL` | Connection string for `dirt-api migrate` only. On Neon, the **direct** one |
| `CLERK_ISSUER` | The Clerk instance that signs ID tokens, with no trailing slash |
| `CLERK_CLIENT_ID` | The client ID of the Clerk OAuth application the clients use. Public, not a secret |
| `DIRT_API_BIND_ADDR` | Where the local program listens. Ignored on Vercel |

The server holds no Clerk secret key. It only checks tokens against Clerk's
published keys, which it fetches from the issuer and keeps for an hour.

The clients are given the server's address and nothing else, as
`DIRT_API_BASE_URL`. All three take it from `.env.client` when they are
built. The CLI and the desktop app also read it from the environment when
they start, and that comes first. The server tells them which Clerk instance
and client ID to sign in with.

## Production: Vercel and Neon

### One-time set-up

1. **Neon:** create a project in AWS US West 2 (Oregon), on Postgres 18. A
   project's region cannot be changed later.
2. **Clerk:** create an OAuth application that is a public client with PKCE
   required, scopes `openid email`, and redirect targets
   `http://127.0.0.1/callback` and `dev.dirt.app://callback`.
3. **Vercel:** create a project whose Root Directory is the repository root.
   `vercel.json` sends every path to the one function and pins it to `pdx1`,
   the region beside the database.
4. Set `DATABASE_URL` (pooled), `CLERK_ISSUER` and `CLERK_CLIENT_ID` on the
   Vercel project.
5. Set the firewall's rate limit, from a folder linked to the project:

   ```bash
   vercel firewall rules add "Per-address limit on the API" \
     --condition '{"type":"path","op":"pre","value":"/v1/"}' \
     --action rate_limit --rate-limit-keys ip \
     --rate-limit-requests 1200 --rate-limit-window 600 --yes
   vercel firewall publish --yes
   ```

   One address may then make 1,200 requests to the API in ten minutes, and is
   answered `429` beyond that. `docs/LIMITS.md` gives the reason for the
   number and what was tried. The free plan allows one such rule.

### Each deploy

1. Apply migrations, from your machine or from CI, against the **direct**
   endpoint:

   ```bash
   MIGRATE_DATABASE_URL='<direct connection string>' cargo run -p dirt-api -- migrate
   ```

2. Deploy the function:

   ```bash
   vercel deploy --prod
   ```

Migrations run first because the server never changes the schema itself. A
migration must therefore leave the previous release working until the new one
is live.

### Things that were measured

From trial 4 of the redesign
(`docs/trials/2026-11-11-trial-4-postgres-on-neon-from-vercel.md`):

- **Rust version:** Vercel's builder ships an older Rust than `sqlx` needs.
  The repository's `rust-toolchain.toml` makes the builder fetch the pinned
  one. A first build takes about three minutes and a cached one under one.
- **Connections:** Vercel runs about one instance per concurrent request, so
  the number of instances sets the number of connections. This is why the
  server must use the pooled endpoint.
- **Waking up:** on Neon's free plan the database sleeps after five idle
  minutes. The first request after that takes about a second.
- **Request size:** Vercel refuses bodies over 4.5 MB. The protocol caps a
  batch at 3 MB of blobs, which is about 4 MB once encoded.

### Plans and their limits

`docs/OWNER_DECISIONS.md` has each of these as a decision, with prices.

- **Vercel Hobby** forbids commercial use. Charging for Dirt requires a paid
  plan.
- **Neon's free plan** covers about 13 hours a day of an awake database,
  shared by all users. Sync is driven by events and not a timer for this
  reason. Anyone who sends one request every five minutes can use the
  month's allowance up, and no rate limit sees it: see "What no rule stops"
  in `docs/LIMITS.md`. The free plan also holds 1 GB, and writes fail when
  it is full.
- **Clerk** needs a production instance on a domain you own before real users
  sign in. Its development instances are capped at 100 users.

## Before the server is public

What has to be true before anyone but the owner is given the server's
address. The first three are the owner's decisions, written up in
`docs/OWNER_DECISIONS.md`.

- [ ] The database is on a plan that cannot be suspended by use (Neon's
      Launch plan), with seven days of history.
- [ ] Sign-in goes through a Clerk production instance on a domain the
      owner holds, and the server runs on that domain.
- [ ] Dumps are being stored: the "Database backup" workflow has run and a
      dump from it has been restored once, as below.
- [ ] The firewall's rate limit is set (step 5 of the one-time set-up).
- [ ] Migrations are applied and `/healthz` answers.
- [ ] `docs/MANUAL_CHECKLIST.md` has been run against this server, on a real
      phone as well as the emulator.
- [ ] GitHub's private vulnerability reporting is switched on, and
      `docs/SECURITY.md` says to use it.

## Backups

There are two, and the first is the one to reach for.

1. **Neon's own history.** It restores the database to any moment in its
   window: six hours on the free plan, up to seven days on Launch. Use it
   for a mistake that was just made. **Then raise the epochs,** before the
   server is used again:

   ```bash
   RESTORE_DATABASE_URL='<direct connection string>' scripts/raise-epochs.sh
   ```
2. **A dump stored outside Neon,** taken once a day. It is for when Neon
   itself is what was lost, or the mistake is older than the window.

### Setting up the daily dump

`.github/workflows/backup.yml` runs `scripts/backup-database.sh` at 03:17
UTC every day, and when started by hand. It does nothing, and says so,
until all of these exist in the repository's settings for Actions:

| Name | Kind | What it is |
|---|---|---|
| `BACKUP_DATABASE_URL` | Secret | The database's **direct** connection string |
| `BACKUP_AGE_RECIPIENT` | Variable | The public half of the encryption key, `age1…` |
| `BACKUP_S3_ENDPOINT` | Variable | The storage's S3 address |
| `BACKUP_S3_BUCKET` | Variable | The bucket's name |
| `BACKUP_S3_ACCESS_KEY_ID` | Secret | A key that may add files to that bucket, and nothing else |
| `BACKUP_S3_SECRET_ACCESS_KEY` | Secret | Its secret |

Make the encryption key once, on a computer of your own:

```bash
age-keygen --output dirt-backup-key.txt   # prints the public half
```

Keep `dirt-backup-key.txt` somewhere that is neither GitHub nor the bucket:
a password manager, and a printed copy. **Without it no dump can be read.**
The public half is not a secret.

How long dumps are kept is a rule on the bucket, not something the workflow
does: a job that can delete dumps could delete all of them. Thirty days is
enough to notice a problem and short enough that a deleted account does not
stay in the dumps for long.

A dump holds every account's encrypted notes, its device records and its
e-mail address. Request counts and sign-in nonces are left out. It is
encrypted before it leaves the workflow, so the storage is trusted only to
keep it.

GitHub switches a scheduled workflow off after 60 days without a commit to
the repository, and e-mails the person who last changed it. A failed run is
e-mailed too. Either e-mail means dumps have stopped.

### Restoring a dump

Into an **empty** database, never over a live one. `pg_restore` and `psql`
must be version 18 or later.

```bash
RESTORE_DATABASE_URL='<direct connection string of an empty database>' \
BACKUP_AGE_IDENTITY=dirt-backup-key.txt \
  scripts/restore-database.sh dirt-20261112T031700Z.dump.age
```

The script refuses a database that already has tables, restores in one
transaction so that a failure leaves nothing behind, and then raises every
account's epoch by running `scripts/raise-epochs.sh`. Point the server's
`DATABASE_URL` at the restored database and deploy.

CI runs `scripts/check-backup.sh` on every change: it dumps a database with
the server's schema, restores it and compares the two. A change to the
schema that the scripts cannot carry is found there, not on the day of a
restore.

### After a restore

This holds for a restore by either means.

**Nothing a device holds is lost, provided the epochs were raised.** A
device that sees a new epoch forgets the change numbers it has seen and
merges everything again, so every note written since the moment restored to
is sent again by a device that has it. A device that is told the epoch it
already holds notices nothing, and what the restore lost stays lost. That
is why `scripts/raise-epochs.sh` gives every restore an epoch of its own,
taken from the clock: one higher would be missed by an account that was
reset in between, and by devices that synced between two restores of the
same dump.

What a restore does undo is what only the server knew. Tell the people who
use the server that it happened, and what to check:

| Since the dump | After the restore | What the person does |
|---|---|---|
| A device was removed | **It is in the account again,** and its token works | Open Devices and remove it again. If its key was never changed, it can read the notes until then |
| A device was let in | The server does not know it | Sign in again on it and let it in from another device. Its notes are still on it |
| The account's key was changed | The server is back at the old key. Every device says it has to sign in again, and signing in again does not help | Start the account again (`dirt auth reset`, or Devices in the apps) from a device that holds the notes. They are sent again under a new key. Then sign the other devices in again |
| The account was reset | The server holds the notes from before the reset, under a key no device has. Devices list them as notes they cannot read | The same: start the account again from a device that holds the notes |
| A recovery phrase was made | The phrase opens nothing | Make a new one |
| The account was deleted | **It exists again,** with its notes as they were | The operator deletes it again. Keep a record of deletions since the oldest dump you might restore |

The first row and the last are the ones that matter for privacy. A restore
is a reason to tell people, not something to do quietly.
