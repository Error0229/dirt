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

## Backups

Neon keeps point-in-time history. A scheduled dump stored outside Neon is
planned for step 8 of the redesign. After any restore, raise the affected
accounts' `epoch`, so that clients forget the change numbers they have seen
and merge again.
