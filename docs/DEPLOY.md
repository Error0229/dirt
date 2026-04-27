# Deploying dirt-api

Phase 1 ships `dirt-api` as a single Rust binary fronted by Turso.
This doc walks the production deploy + the rollback path. The
"15-minute self-host" recipe in [BACKEND_API.md](./BACKEND_API.md)
covers first-time provisioning; this doc covers everything that
happens *after* you have a Vercel project and a Turso database.

## Required environment

Set these three on the deployment target. Never bake them into client
binaries; they're server-only.

| Var | Where it comes from | Purpose |
|---|---|---|
| `TURSO_DATABASE_URL` | `turso db show <name> --url` | libSQL endpoint the server reads/writes |
| `TURSO_AUTH_TOKEN` | `turso db tokens create <name> --expiration none` | Long-lived database-scoped token (NOT a platform token) |
| `DIRT_SERVER_TOKEN` | `openssl rand -base64 32 \| tr -d '=' \| tr '+/' '-_'` | Shared bearer that every client must present |

`DIRT_SERVER_TOKEN` must be at least 32 characters
(`openssl rand -hex 32` produces a 64-char hex string with ~256 bits of
entropy). Server comparison is constant-time (`subtle::ConstantTimeEq`)
so the length only matters for entropy.

## Vercel deploy

The repo is structured so the workspace root is the Vercel project
root: `vercel.json` rewrites every path to the `axum` serverless
function defined by `api/axum.rs`, which the workspace's root
`Cargo.toml` builds as `dirt-vercel`.

```bash
# One-time link to your Vercel project.
vercel link

# Set the env (per environment).
vercel env add DIRT_SERVER_TOKEN production
vercel env add TURSO_DATABASE_URL production
vercel env add TURSO_AUTH_TOKEN production

# Ship.
vercel --prod
```

The first deploy takes ~10 minutes (cold Rust toolchain). Subsequent
deploys are 2–4 minutes.

After deploy, smoke against the deployed URL:

```bash
curl -s "$VERCEL_URL/healthz"
# ok

curl -s "$VERCEL_URL/v1/notes/pull?limit=1" \
  -H "Authorization: Bearer $DIRT_SERVER_TOKEN" | jq '.notes | length'
```

A non-zero exit or a non-`ok` body means the deploy isn't ready.
Wait or check `vercel logs <deployment-url>`.

## Pre-deploy checklist

Before promoting to production:

- **Take a Turso snapshot.** Backups for paid Turso plans are
  automatic; for the free tier, dump explicitly:
  ```bash
  turso db shell <name> ".dump" > snapshots/dirt-$(date +%Y%m%d-%H%M%S).sql
  ```
  Keep at least the last 7 days. The snapshot is the only rollback
  path for data corruption — schema rollback is not supported in
  Phase 1.
- **Dry-run the restore.** Phase 1 ships are gated on a verified
  rollback. Restore the most recent snapshot to a scratch DB:
  ```bash
  turso db create dirt-rollback-test --location nrt
  cat snapshots/<latest>.sql | turso db shell dirt-rollback-test
  turso db shell dirt-rollback-test "SELECT COUNT(*) FROM notes"
  turso db destroy dirt-rollback-test --yes
  ```
  If `COUNT(*)` matches what you expect from production, the
  rollback path is live. Untested rollback is no rollback.
- **Run the workspace tests.** `cargo test --workspace` must pass
  before promoting. Phase 1 ignores 8 Windows-flaky CLI tests by
  default.
- **Run the security guardrails.** `bash .github/scripts/security-guard.sh`
  must exit clean — it scans for accidental secret literals and
  blocks server-only env-var names from leaking into client crates.

## Server rollback

`dirt-api` is stateless; rolling back is just redeploying the prior
version.

```bash
# List recent deployments.
vercel ls dirt-api

# Promote a previous one to production.
vercel promote <deployment-url>
```

If the previous deploy is incompatible with the current Turso schema,
restore the matching snapshot first:

```bash
turso db shell <name> ".restore snapshots/<chosen>.sql"
```

Note that **schema is one-way** in Phase 1. The server's schema is
created by `bootstrap()` on first start and only ever extended. If
you need to roll back a schema change, restore from snapshot, deploy
the prior server image, and accept that anything written between
snapshot time and rollback is lost.

## Client migration failure

Clients run their own `dirt-core` migrations on every start. Phase 1
schema is `5`; future commits add `migrate_v6` etc. If a client
binary boots against a SQLite file with `schema_version > compiled_version`
(i.e. a newer client was installed first), the migration runner
errors out and the client refuses to start. The user-facing recovery
is to either upgrade the client to a version that knows about the
newer schema, or wipe the local DB:

- **Desktop**: `%APPDATA%\dirt\dirt.db` (Windows),
  `~/Library/Application Support/dirt/dirt.db` (macOS),
  `~/.local/share/dirt/dirt.db` (Linux).
- **CLI**: same paths via `dirs::data_dir()`. Override with
  `--db-path` or `DIRT_DB_PATH`.
- **Mobile**: app data dir; the platform installer wipes this on
  uninstall.

A built-in "Reset local DB" button is on the Phase 2 roadmap;
in Phase 1 the recovery is manual file deletion.

## Operational notes

- **Cold-start cost.** Vercel's Rust runtime cold-starts in 200–500ms
  for `dirt-api`. The TLS handshake + first Turso connection adds
  another 200–400ms. After ~5 minutes of idleness Vercel may freeze
  the function; the next request pays the cold-start cost again.
- **Logs.** Structured JSON via `tracing-subscriber`. `vercel logs`
  shows them. The deploy intentionally leaves request-body content
  out of logs so accidental token leakage is bounded to URLs and
  status codes.
- **Body size.** `/v1/notes/push` rejects bodies over 8 MiB
  (`PUSH_BODY_LIMIT`). The 500-note batch limit gives the same
  effective cap on item count, but the 8 MiB ceiling protects
  against pathological notes too.
- **Turso quotas.** The free tier covers a personal deploy; the
  busiest device generates roughly one request every 30 seconds plus
  one per typed note (debounced 1.5 s). Three devices in active use
  is comfortably under the free-tier 500/sec limit.
