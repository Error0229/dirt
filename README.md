# dirt

> Do I Remember That? A cross-platform note-taking app focused on capturing
> fleeting thoughts with under 2 seconds of friction.

## Status

A redesign is in progress, and `main` is partway through it.

- **What is changing and why:**
  [docs/plans/2026-11-10-system-redesign-design.md](docs/plans/2026-11-10-system-redesign-design.md)
- **Build order:**
  [docs/plans/2026-11-10-refactor-system-redesign-implementation-plan.md](docs/plans/2026-11-10-refactor-system-redesign-implementation-plan.md)

**Where it stands: step 5 of 9, half done.** The desktop app, the CLI and the
Android app run on the new local core, the new server is built, and the sync
core (note encryption, the sync cycle and the server client) is built and
tested against the real server. **Sync is still off:** no client calls it
yet. Notes stay on the device they were written on until the second half of
step 5 adds sign-in and connects the clients.

## Workspace layout

| Crate | Purpose |
|---|---|
| `dirt-proto` | Types shared by the server and the clients. |
| `dirt-core` | Everything the clients share: notes as mergeable documents, the local store (notes, tags, search, settings), note encryption, the sync cycle, export, and the old sign-in client. |
| `dirt-desktop` | Dioxus desktop app. |
| `dirt-mobile` | Dioxus Android app. |
| `dirt-cli` | `dirt add`, `dirt list`, `dirt search`, `dirt export` and more. |
| `dirt-api` | The server, on axum and Postgres: it checks a Clerk sign-in, keeps the device registry, and stores encrypted note blobs it cannot read. See [docs/BACKEND_API.md](docs/BACKEND_API.md). |
| `dirt-sim` | Tests only: two or three real client cores syncing through the real server in one process, with lost answers, tampered blobs, replays and restored backups. |
| `dirt-vercel` | Workspace-root package that hosts the Vercel serverless entry point at `api/axum.rs`. |

## Quick start

### Capture from the terminal

Works on a fresh clone with no configuration:

```bash
cargo run -p dirt-cli "first thought"
cargo run -p dirt-cli list
```

Notes are saved to `dirt/stores/local/notes.db` under your platform's data
directory (`%APPDATA%` on Windows). The desktop app opens the same file, so
both show the same notes, and both can run at the same time. `--db-path` or
`DIRT_DB_PATH` selects another file.

`dirt sync` reports that sync is unavailable. A `dirt.db` left by an earlier
version is not read and not changed.

### Run the desktop app

```bash
cargo run -p dirt-desktop
```

Settings → Account still holds the old email-code sign-in. It works only
against a deployment of the old server, and signing in does not sync
anything. Step 5 replaces it with sign-in through Clerk.

### Run the API server

```bash
docker compose up -d                 # Postgres 18 for local work
cp .env.server.example .env.server   # then set CLERK_ISSUER and CLERK_CLIENT_ID
cargo run -p dirt-api -- migrate     # create the tables
cargo run -p dirt-api                # listens on 0.0.0.0:8080
```

`.env.server.example` documents every variable.
[docs/BACKEND_API.md](docs/BACKEND_API.md) describes the protocol and
[docs/DEPLOY.md](docs/DEPLOY.md) how to deploy to Vercel and Neon.

## How the current code works

- **Local first, and for now local only.** Every change is one transaction in
  a local SQLite file. Nothing waits on a network.
- **A note is a document that merges.** Each note is stored as a CRDT
  document. Its text, tags, search entry and last-edited time are derived from
  the document on every change. An edit records only the parts of the text
  that changed, so two copies of a note edited separately can be merged
  without losing either edit. A delete is a marker in the document, which is
  why it can be undone.
- **One file, several processes.** The store has one thread that writes and
  two that read, so a write never blocks a read. Every write begins by taking
  the write lock, which is what lets the desktop app and the CLI share the
  file.
- **A store per account.** Stores live in `stores/local/` and
  `stores/account-<id>/`. Only `local` is used until sign-in opens an account
  store in step 5.

### Known limitations

- **No sync** until the second half of step 5.
- **The desktop app does not watch the file.** It shows a note written by the
  CLI only when it next reloads its list, for example on restart. If the CLI
  changes a note that is open in the desktop editor, the desktop app's next
  save replaces that change.
- **Signing in does nothing for notes yet.** The account screens still hold
  the old email-code sign-in, which this repository's server no longer
  answers. Step 5 replaces it and opens a store per account (issue #234).
- **The server cannot yet approve a second device.** The first device of an
  account is approved at once; later ones wait for step 6.

## Testing

```bash
docker compose up -d   # the server's tests and the sync simulation need Postgres
export DATABASE_URL=postgres://postgres:dirt@127.0.0.1:55432/postgres
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
bash .github/scripts/security-guard.sh   # secret-leak guardrails
```

The store's two-process tests start copies of the test program as worker
processes, so they run wherever `cargo test` does.

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
