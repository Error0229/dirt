# dirt

> Do I Remember That? A cross-platform note-taking app focused on capturing
> fleeting thoughts with under 2 seconds of friction.

## Status

`main` holds two finished redesigns: the system's, in nine steps, and the
desktop and Android apps' interface, in ten. A server runs it at
`https://api.dirt.catjam.dev`. That server is the owner's own for now: its
database is on a free plan that heavy use can suspend, so it is not yet
offered to anyone else
([docs/OWNER_DECISIONS.md](docs/OWNER_DECISIONS.md)). What is left needs a
person: the items of a checklist marked "Person", tried by hand on real
devices ([docs/MANUAL_CHECKLIST.md](docs/MANUAL_CHECKLIST.md)), and an
audit of how the apps use Dioxus, with three bugs to check and fix (issue
#303).

- **The system's design:**
  [docs/plans/2026-11-10-system-redesign-design.md](docs/plans/2026-11-10-system-redesign-design.md),
  built by
  [docs/plans/2026-11-10-refactor-system-redesign-implementation-plan.md](docs/plans/2026-11-10-refactor-system-redesign-implementation-plan.md)
- **The interface's design:** [docs/ui/design.md](docs/ui/design.md), with
  a working mockup, [docs/ui/mockup.html](docs/ui/mockup.html), built by
  [docs/plans/2026-11-16-interface-redesign-implementation-plan.md](docs/plans/2026-11-16-interface-redesign-implementation-plan.md)

**What it does.** Devices sign in through Clerk and sync their notes
through the new server, which stores only ciphertext it cannot read. A
second device is let into an account by comparing a short code with a
device that is already in, or with the account's recovery phrase. A file
attached to a note is encrypted on the device and kept in object storage
the server only signs links for. Everything can be exported, notes and
files, at any time. The desktop app, the Android app and the CLI are all on
the new core.

**How it looks and feels.** A thought is typed on a page that is always
there, or said aloud and turned into text on the device that heard it.
Notes are archived rather than deleted, and both can be undone. Each note
is a grain of sand, and under the list lies the pile they make. On the
desktop, quick capture opens over any app from Ctrl Alt N; on Android, a
capture sheet opens from a home-screen widget, the app's icon, a Quick
Settings tile or a hardware key, over the lock screen too, and a second
widget shows the pile. Files open in a preview inside the app.
[docs/SECURITY.md](docs/SECURITY.md) says what is protected and what is not,
and [docs/feature-parity.md](docs/feature-parity.md) what each client does.

## Workspace layout

| Crate | Purpose |
|---|---|
| `dirt-proto` | Types shared by the server and the clients. |
| `dirt-ui` | What the two apps share of the interface: the tokens, the fonts, the icons, the motion script, the components and the pile, with a gallery of them (`make run-gallery-desktop`). |
| `dirt-voice` | Speech turned into text on the device, with NVIDIA's Parakeet TDT-CTC 110M through sherpa-onnx: the recording, the pauses, the words. The CLI and the server do not build it. |
| `dirt-core` | Everything the clients share: notes as mergeable documents, the local store (notes, tags, search, settings), files attached to notes, encryption, sign-in, the session, sync, and export. |
| `dirt-desktop` | Dioxus desktop app. |
| `dirt-mobile` | Dioxus Android app, and in `android/native` the Kotlin that runs without its screen: the capture sheet, its jobs, the tile and the widgets. |
| `dirt-cli` | `dirt add`, `dirt list`, `dirt search`, `dirt attach`, `dirt export` and more. |
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

Installed, the same is `dirt "first thought"` and `dirt list`. The list is
numbered, and a number names a note to the next command:

```bash
dirt list               # 1  01a216c7-da  Call the dentist ...  2m ago
dirt show 1             # the whole note, then its ID, times, tags and files
dirt edit 1             # in $VISUAL or $EDITOR
dirt edit dentist       # or by words from it; several matches are listed, numbered
dirt edit               # nothing named: the latest note
dirt delete 1           # dirt restore 1 brings it back
```

A note is also named by its ID or the start of it, as the list shows it.
`dirt help edit` gives the rules, and system design 8.5 the reasons.

Notes are saved to `dirt/stores/local/notes.db` under your platform's data
directory (`%APPDATA%` on Windows). The desktop app opens the same file, so
both show the same notes, and both can run at the same time. `--db-path` or
`DIRT_DB_PATH` selects another file, which never syncs. A `dirt.db` left by
an earlier version is not read and not changed.

### Sign in and sync

Point the CLI at a server, then sign in through the browser:

```bash
export DIRT_API_BASE_URL=https://your-server.example
dirt auth login      # opens the browser; --no-browser prints the address
dirt auth status     # the server, who is signed in, last sync, notes waiting
dirt sync            # one sync, now
dirt sync --verbose  # and each note that could not be synced, with what releases it
```

The CLI built in this repository also takes the address from `.env.client`,
as the desktop app does, so the two name the same server without the
variable. The variable, when set, comes first for both.

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
dirt devices remove <ID>  # remove one; --change-key also changes the account's key
dirt auth change-key      # change the key, so a removed device reads nothing new
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

### Run the Android app

```bash
make run-mobile-android    # dx serve for the emulator; or:
dx build --platform android --package dirt-mobile --target aarch64-linux-android
```

Gradle may need another JDK than the computer's: set
`DIRT_ANDROID_JAVA_HOME` to it (JDK 21 works) and the `make` targets hand
it to `dx` and Gradle for that command only.

The first build fetches the speech model the app ships for voice (108 MB,
NVIDIA's Parakeet TDT-CTC 110M) and the engine's libraries, each checked
against a pinned hash, into a build cache outside `target/`; `CLAUDE.md`
says where, and how to build without the model.

The server to sync with is given when the app is built: set
`DIRT_API_BASE_URL` in `.env.client` (see `.env.client.example`). The same
variable in the environment when the app starts comes before it. Without
either the app keeps its notes on the device and Settings says so. With one,
Settings → Account signs in through the browser, and Settings → Sync shows
when the app last synced, each note that could not be synced with what
releases it, and the sync button.

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
  Change the account's key afterwards and that key reads nothing written
  from then on. It cannot take back what the device already holds.
- **The only device of an account cannot sign out without a recovery
  phrase.** Its copy of the account key would be the last.
- **Lose every device and the phrase, lose the notes.** That is what the
  server being unable to read them means. The account can be reset and
  started again.
- **The desktop app does not watch the file.** It reloads its list when its
  window regains focus. If the CLI changes a note that is open on the
  desktop's page, the page does not show the change until the note is opened
  again; its next save is merged with it and does not undo it.

## Testing

```bash
docker compose up -d   # the server's tests and the sync simulation need Postgres
export DATABASE_URL=postgres://postgres:dirt@127.0.0.1:55432/postgres
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
bash .github/scripts/security-guard.sh   # secret-leak guardrails
```

The store's two-process tests start copies of the test program as worker
processes, so they run wherever `cargo test` does.

## Documentation

- [docs/DESIGN.md](docs/DESIGN.md): product vision and standing decisions.
- [docs/SECURITY.md](docs/SECURITY.md): what is protected, from whom, and
  what is not.
- [docs/feature-parity.md](docs/feature-parity.md): what each of the three
  clients does, feature by feature.
- [docs/MANUAL_CHECKLIST.md](docs/MANUAL_CHECKLIST.md): what to try by hand
  on real devices before a release.
- [docs/ui/design.md](docs/ui/design.md): the interface of the two apps,
  with its mockup.
- [docs/LIMITS.md](docs/LIMITS.md): every limit the server enforces, and
  why.
- [docs/OWNER_DECISIONS.md](docs/OWNER_DECISIONS.md): what only the owner
  could decide, and what is still theirs.
- [docs/plans/](docs/plans/): the two redesigns and their implementation
  plans.
- [docs/archive/](docs/archive/): superseded documents, kept as a record.

## License

MIT, as declared in `Cargo.toml`. The repository does not contain a `LICENSE`
file yet.
