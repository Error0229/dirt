# dirt

> Do I Remember That? — a cross-platform note-taking app focused on
> capturing fleeting thoughts with under-2-second friction.

## Workspace layout

| Crate | Purpose |
|---|---|
| `dirt-core` | Models, SQLite repository, sync engine. Platform-agnostic. |
| `dirt-api` | axum HTTP server. `POST /v1/notes/push`, `GET /v1/notes/pull`, `/healthz`. Bearer-authed. Backed by Turso. |
| `dirt-desktop` | Dioxus desktop app. Auto-syncs in the background. |
| `dirt-mobile` | Android shell (in transition — Phase 1 ships server + desktop + CLI; the mobile sync worker is the next milestone). |
| `dirt-cli` | `dirt add`, `dirt list`, `dirt sync`, etc. Local-first; sync only runs when `DIRT_API_BASE_URL` and `DIRT_CLIENT_TOKEN` are set. |
| `dirt-vercel` | Workspace-root crate that hosts the Vercel serverless entry point at `api/axum.rs`. Pulls in `dirt-api` by path. |

## Quick start

### Local-only capture (no sync)

Works on a fresh clone with zero env config:

```bash
cargo run -p dirt-cli "first thought"
cargo run -p dirt-cli list
```

The CLI captures locally to a SQLite file under your platform's data
dir. Sync stays off until both `DIRT_API_BASE_URL` and
`DIRT_CLIENT_TOKEN` are set.

### Run the desktop app

```bash
export DIRT_API_BASE_URL="https://dirt-api.vercel.app"
export DIRT_CLIENT_TOKEN="$(cat .env.client | grep DIRT_CLIENT_TOKEN | cut -d= -f2)"
cargo run -p dirt-desktop
```

The window opens, the local DB initializes, and a background sync
worker starts immediately. There is no manual "Sync now" button —
sync runs on three triggers: app startup, a 30 s periodic timer, and
a 1.5 s debounce after every successful local mutation. Failures back
off exponentially (5 s → 15 s → 60 s → 300 s).

If either env var is missing, the sync indicator shows red and the
Settings → Sync tab explains which one. Local capture still works.

### Run the API server

```bash
cp .env.server.example .env.server
# Fill in TURSO_DATABASE_URL, TURSO_AUTH_TOKEN, DIRT_SERVER_TOKEN.
cargo run -p dirt-api
# Listens on 0.0.0.0:8080 by default.
```

See [docs/DEPLOY.md](docs/DEPLOY.md) for the production deploy and
rollback procedure, and [docs/BACKEND_API.md](docs/BACKEND_API.md)
for the full API contract + 15-minute self-host recipe.

## Key design points

- **Offline-first.** Every mutation lands in local SQLite first. Sync
  is additive; clients work fully offline and reconcile on reconnect.
- **Server-authoritative timestamps.** Pull ordering uses
  `server_updated_at_ms` exclusively; a misbehaving client clock can't
  poison the cursor.
- **Single shared bearer token in Phase 1.** `DIRT_CLIENT_TOKEN` on
  every client must equal `DIRT_SERVER_TOKEN` on the server. Per-user
  identity comes in Phase 2 (magic-link auth).
- **No silent failures.** Misconfigured env vars surface visibly
  (`SyncStatus::Error` with a populated `sync_issue`) instead of
  pretending to be offline. The "no fallback" rule is a project-wide
  invariant.

## Testing

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
bash .github/scripts/security-guard.sh   # secret-leak guardrails
```

A handful of CLI tests are `#[cfg_attr(windows, ignore)]` because
libsql's file-handle behavior is flaky on Windows; they pass on
macOS and Linux.

## Documentation index

- [docs/BACKEND_API.md](docs/BACKEND_API.md) — endpoint reference,
  error codes, self-host recipe.
- [docs/DEPLOY.md](docs/DEPLOY.md) — Vercel deploy, snapshots, rollback.
- [docs/DESIGN.md](docs/DESIGN.md) — architecture overview.
- [docs/SECURITY_BASELINE.md](docs/SECURITY_BASELINE.md),
  [docs/SECURITY_OPERATIONS.md](docs/SECURITY_OPERATIONS.md) —
  security posture and operational runbooks.
- [docs/feature-parity.md](docs/feature-parity.md) — parity status
  with peer apps.

## License

MIT. See [LICENSE](LICENSE).
