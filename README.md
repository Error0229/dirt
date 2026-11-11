# dirt

> Do I Remember That? A cross-platform note-taking app focused on capturing
> fleeting thoughts with under 2 seconds of friction.

## Status

A redesign is in progress, and `main` is partway through it.

- **What is changing and why:**
  [docs/plans/2026-11-10-system-redesign-design.md](docs/plans/2026-11-10-system-redesign-design.md)
- **Build order:**
  [docs/plans/2026-11-10-refactor-system-redesign-implementation-plan.md](docs/plans/2026-11-10-refactor-system-redesign-implementation-plan.md)

**Where it stands: step 3 of 9.** The desktop app, the CLI and the Android app
run on the new local core. **Sync is off.** Notes stay on the device they were
written on until the new server and protocol arrive in steps 4 and 5. The
server in this repository is still the old one, and no client syncs notes with
it.

## Workspace layout

| Crate | Purpose |
|---|---|
| `dirt-proto` | Types shared by the server and the clients. |
| `dirt-core` | Everything the clients share: notes as mergeable documents, the local store (notes, tags, search, settings), export, and the old sign-in client. |
| `dirt-desktop` | Dioxus desktop app. |
| `dirt-mobile` | Dioxus Android app. |
| `dirt-cli` | `dirt add`, `dirt list`, `dirt search`, `dirt export` and more. |
| `dirt-api` | The old axum HTTP server, backed by Turso: email-code login under `/v1/auth/*` and the old note endpoints. Step 4 replaces it. |
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

Settings → Account still signs in to the old server when `DIRT_API_BASE_URL`
was set at build time. Signing in does not sync anything at this step.

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

- **No sync** until step 5.
- **The desktop app does not watch the file.** It shows a note written by the
  CLI only when it next reloads its list, for example on restart. If the CLI
  changes a note that is open in the desktop editor, the desktop app's next
  save replaces that change.
- **Signing in does nothing for notes yet.** The account screen still talks to
  the old server. Per-account stores are opened by the new sign-in in step 5
  (issue #234).
- **The old server's rate limits are unreliable.** They live in process
  memory, which serverless instances do not share. Step 4 replaces them.

## Testing

```bash
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
