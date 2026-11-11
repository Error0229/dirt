# dirt

> Do I Remember That? A cross-platform note-taking app focused on capturing
> fleeting thoughts with under 2 seconds of friction.

## Status

A redesign is in progress. The code on `main` is the system **before** that
redesign; it works, and this README describes how to run it.

- **What is changing and why:**
  [docs/plans/2026-11-10-system-redesign-design.md](docs/plans/2026-11-10-system-redesign-design.md)
- **Build order:**
  [docs/plans/2026-11-10-refactor-system-redesign-implementation-plan.md](docs/plans/2026-11-10-refactor-system-redesign-implementation-plan.md)

The redesign replaces sync, login and both database schemas. It adds automatic
merging of concurrent edits and end-to-end encryption.

## Workspace layout

| Crate | Purpose |
|---|---|
| `dirt-core` | Models, local SQLite repository, sync engine, login client. Shared by every client. |
| `dirt-api` | axum HTTP server. Email-code login under `/v1/auth/*`, note sync at `POST /v1/notes/push` and `GET /v1/notes/pull`, and `/healthz`. Backed by Turso. |
| `dirt-desktop` | Dioxus desktop app. Syncs in the background. |
| `dirt-mobile` | Dioxus Android app, with login and a background sync worker. |
| `dirt-cli` | `dirt add`, `dirt list`, `dirt search`, `dirt sync` and more. |
| `dirt-vercel` | Workspace-root package that hosts the Vercel serverless entry point at `api/axum.rs`. |

## Quick start

### Local-only capture (no sync)

Works on a fresh clone with no configuration:

```bash
cargo run -p dirt-cli "first thought"
cargo run -p dirt-cli list
```

The CLI saves to a SQLite file under your platform's data directory. Sync
stays off until a server address is configured and you sign in with
`dirt auth login`. The session is stored in the OS keychain, in a slot shared
by `dirt-cli` and `dirt-desktop`, so signing in from either enables sync in
both.

### Run the desktop app

```bash
export DIRT_API_BASE_URL="https://dirt-api.vercel.app"
cargo run -p dirt-desktop
# Then sign in from Settings → Account.
```

`DIRT_API_BASE_URL` is read when the app is **built** and baked into the
binary. The CLI reads the same variable at run time.

There is no "Sync now" button. Sync runs on three triggers: app startup, a
30-second timer, and 1.5 seconds after each local change. Failures back off
(5 s, 15 s, 60 s, then 300 s).

If no server address is configured or you are not signed in, the sync
indicator shows red and Settings explains which. Local capture still works.

### Run the API server

```bash
cp .env.server.example .env.server
# Fill in TURSO_DATABASE_URL and TURSO_AUTH_TOKEN.
cargo run -p dirt-api
# Listens on 0.0.0.0:8080 by default.
```

`RESEND_API_KEY` and `RESEND_FROM_ADDRESS` are optional and must be set
together. Without them the server does not send email; it writes each login
code to its debug log instead. `.env.server.example` documents every variable.

The deploy guide and API reference for this version are archived and partly
out of date; see [docs/archive/](docs/archive/). New ones are written when the
server is rebuilt.

## How the current code works

- **Offline-first.** Every change is written to local SQLite first. Clients
  work fully offline and sync when they can.
- **Server-stamped ordering.** Pull ordering uses the server's timestamp only,
  so a wrong device clock cannot disturb it.
- **Email-code sessions.** Clients sign in through
  `/v1/auth/{request,verify}` and keep the session token in the OS keychain.
- **Visible failures.** A misconfigured server address shows as a sync error
  with a reason, never as a silent "offline".

### Known limitations

The redesign exists to fix these.

- **Concurrent edits are lost silently.** If two devices change the same note
  before syncing, the server keeps whichever push arrives last.
- **One local database for all accounts.** Signing in as a different account
  on the same machine mixes notes (issue #234).
- **Server rate limits are unreliable.** They live in process memory, which
  serverless instances do not share.

## Testing

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
bash .github/scripts/security-guard.sh   # secret-leak guardrails
```

Eight CLI tests are ignored on Windows because the `libsql` integration is
flaky there. They run on macOS and Linux.

## Documentation

- [docs/DESIGN.md](docs/DESIGN.md): product vision and standing decisions.
- [docs/plans/](docs/plans/): the redesign and its implementation plan.
- [docs/mobile-ui-conventions.md](docs/mobile-ui-conventions.md): conventions
  for the Android interface.
- [docs/archive/](docs/archive/): documents for the system before the
  redesign.

## License

MIT, as declared in `Cargo.toml`. The repository does not contain a `LICENSE`
file yet.
