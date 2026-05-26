# dirt

> Do I Remember That? — a cross-platform note-taking app focused on
> capturing fleeting thoughts with under-2-second friction.

## Workspace layout

| Crate | Purpose |
|---|---|
| `dirt-core` | Models, SQLite repository, sync engine. Platform-agnostic. |
| `dirt-api` | axum HTTP server. `POST /v1/notes/push`, `GET /v1/notes/pull`, `/healthz`. Bearer-authed. Backed by Turso. |
| `dirt-desktop` | Dioxus desktop app. Auto-syncs in the background. |
| `dirt-mobile` | Dioxus Android app. Magic-link login wired up; mobile sync worker mirrors desktop. |
| `dirt-cli` | `dirt add`, `dirt list`, `dirt sync`, etc. Local-first; sync runs when `DIRT_API_BASE_URL` is configured and a session is stored via `dirt auth login`. |
| `dirt-vercel` | Workspace-root crate that hosts the Vercel serverless entry point at `api/axum.rs`. Pulls in `dirt-api` by path. |

## Quick start

### Local-only capture (no sync)

Works on a fresh clone with zero env config:

```bash
cargo run -p dirt-cli "first thought"
cargo run -p dirt-cli list
```

The CLI captures locally to a SQLite file under your platform's data
dir. Sync stays off until `DIRT_API_BASE_URL` is configured and you
sign in with `dirt auth login` (the session lands in your OS keyring;
the same slot is shared across `dirt-cli` and `dirt-desktop`, so a
login from either gets you sync on both).

### Run the desktop app

```bash
export DIRT_API_BASE_URL="https://dirt-api.vercel.app"
cargo run -p dirt-desktop
# Then sign in from Settings → Account.
```

The window opens, the local DB initializes, and a background sync
worker starts immediately. There is no manual "Sync now" button —
sync runs on three triggers: app startup, a 30 s periodic timer, and
a 1.5 s debounce after every successful local mutation. Failures back
off exponentially (5 s → 15 s → 60 s → 300 s).

If `DIRT_API_BASE_URL` is missing or the user is not signed in, the
sync indicator shows red and the Settings → Sync / Account tab explains
which one. Local capture still works.

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
- **Magic-link sessions (Phase 2).** Clients sign in via
  `/v1/auth/{request,verify}` and persist the resulting session token
  in the OS keychain. The session middleware on `dirt-api` resolves
  the bearer to a user id before any note-shaped handler runs. The
  Phase-1 `DIRT_CLIENT_TOKEN` shared bearer has been retired from all
  three clients.
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
