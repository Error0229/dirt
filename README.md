# dirt

> Do I Remember That? A cross-platform note-taking app focused on capturing
> fleeting thoughts with under 2 seconds of friction.

## Status

A redesign is in progress, and `main` is partway through it.

- **What is changing and why:**
  [docs/plans/2026-11-10-system-redesign-design.md](docs/plans/2026-11-10-system-redesign-design.md)
- **Build order:**
  [docs/plans/2026-11-10-refactor-system-redesign-implementation-plan.md](docs/plans/2026-11-10-refactor-system-redesign-implementation-plan.md)

**Where it stands: step 6 of 9.** Devices sign in through Clerk and sync
their notes through the new server, which stores only ciphertext it cannot
read. A second device is let into an account by comparing a short code with
a device that is already in, or with the account's recovery phrase.
[docs/SECURITY.md](docs/SECURITY.md) says what that protects and what it does
not.

## Workspace layout

| Crate | Purpose |
|---|---|
| `dirt-proto` | Types shared by the server and the clients. |
| `dirt-core` | Everything the clients share: notes as mergeable documents, the local store (notes, tags, search, settings), note encryption, sign-in, the session, sync, and export. |
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
`DIRT_DB_PATH` selects another file, which never syncs. A `dirt.db` left by
an earlier version is not read and not changed.

### Sign in and sync

Point the CLI at a server, then sign in through the browser:

```bash
dirt config init --api-base-url https://your-server.example
dirt auth login      # opens the browser; --no-browser prints the address
dirt auth status     # who is signed in, last sync, notes waiting
dirt sync            # one sync, now
```

Signing in opens a store for the account, `stores/account-<id>/`, and asks
whether to move the notes written before into it. After that, every `dirt`
command that changes a note syncs once before it returns; the note is saved
first, so a slow or missing network costs nothing but a warning.

The CLI and the desktop app on one computer are one device. They share the
data folder and the keychain entry, so signing in with either signs in both.

### More than one device

The first device to sign in to an account makes the key that encrypts its
notes. Every later device signs in and then waits to be let in:

```bash
dirt auth join          # on the new device: ask to be let in
dirt devices approve    # on a device that is already in: answer
```

Both show the same code of three groups of four digits. Say on each whether
they match. If they do, the new device gets the key; if they do not, it is
turned away, because someone may be between the two.

```bash
dirt devices              # list the account's devices
dirt devices remove <ID>  # remove one
dirt auth phrase new      # make a recovery phrase: 24 words, shown once
dirt auth recover         # on a new device: get in with the phrase
```

The recovery phrase is optional. Without it, losing every device loses the
notes. The desktop app has all of this in Settings → Devices, and the
Android app under Settings → Devices and recovery phrase.

### Run the desktop app

```bash
cd crates/dirt-desktop && dx serve
```

`cargo run -p dirt-desktop` starts it too, but without its style sheets:
`dx` is what bundles them.

The server to sync with is fixed when the app is built: set
`DIRT_API_BASE_URL` in `.env.client` (see `.env.client.example`). Without
it the app keeps its notes on the device and Settings says so. With it,
Settings → Account signs in through the browser, and Settings → Sync shows
when the app last synced and has the sync button.

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

- **Local first.** Every change is one transaction in a local SQLite file.
  Nothing waits on a network: sync runs afterwards, in the background.
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
  `stores/account-<id>/`. A device that is signed out, or signed in but not
  yet approved, uses `local`. Signing out removes the account's store from
  the device.
- **Signing in is Clerk's job, and nothing else is.** The app opens Clerk's
  page in the system browser and receives an ID token. It shows that token to
  the server once, to register the device; from then on the device uses its
  own token and Clerk is out of the path.
- **The server cannot read notes.** Each note leaves the device encrypted
  under the account key, which the first device makes and the server never
  sees. The key, the device's token and its key pair live in the operating
  system's keychain, or in Android's encrypted preferences.
- **Sync runs when something happens.** It runs when the app starts, when you
  come back to it, a moment after a note changes, and when you press the
  sync button. It retries by itself only while a note is waiting to be sent.
  There is no timer, so an app left open and untouched shows what it had when
  it last synced.

### Known limitations

- **A removed device keeps what it had.** Removing a device stops it reaching
  the server. It keeps the notes it already had and the key that reads them.
  Changing the account's key, which ends that, is step 8.
- **The only device of an account cannot sign out without a recovery
  phrase.** Its copy of the account key would be the last.
- **Lose every device and the phrase, lose the notes.** That is what the
  server being unable to read them means. The account can be reset and
  started again.
- **The desktop app does not watch the file.** It reloads its list when its
  window regains focus. If the CLI changes a note that is open in the desktop
  editor, the desktop app's next save replaces that change.

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
- [docs/SECURITY.md](docs/SECURITY.md): what is protected, from whom, and
  what is not.
- [docs/plans/](docs/plans/): the redesign and its implementation plan.
- [docs/mobile-ui-conventions.md](docs/mobile-ui-conventions.md): conventions
  for the Android interface.
- [docs/archive/](docs/archive/): documents for the system before the
  redesign.

## License

MIT, as declared in `Cargo.toml`. The repository does not contain a `LICENSE`
file yet.
