# Dirt - Claude Code Guidelines

## Project Overview

Dirt ("Do I Remember That?") is a cross-platform note-taking app focused on capturing fleeting thoughts with minimal friction. Target capture velocity: under 2 seconds from thought to saved note.

The project is also a system design case study. Multi-user accounts, three clients, sync and end-to-end encryption are all real requirements. Do not propose cutting scope to reduce complexity; justify each part against the requirements instead.

## Status: both redesigns are built

Two redesigns are built on `main`:

- **The system redesign** (nine steps): accounts, three clients on one core, sync and end-to-end encryption, files, export. The owner has taken the six decisions that were theirs (`docs/OWNER_DECISIONS.md`).
- **The interface redesign** (ten steps, finished 2026-11-20): a new interface for the desktop and Android apps, with archive, undo, voice, capture from outside the app, the file preview and the pile.

There are two servers: production at `https://api.dirt.catjam.dev`, and `dirt-trial`, where changes are tried and data is thrown away (`docs/DEPLOY.md`, "What exists"). Try things on the trial server, never on production.

Devices sign in through Clerk and sync their notes, encrypted, through the new server. A second device is let in by comparing a code with one that is already in, or by the recovery phrase. `docs/SECURITY.md` describes what is protected.

What is left:

- The items of `docs/MANUAL_CHECKLIST.md` marked "Person": a real microphone, real phones, TalkBack and Narrator, WebKit on macOS and Linux, release builds, and whatever acts on somebody's screen.
- The short list at the end of `docs/OWNER_DECISIONS.md`.

## The system: standing rules

Read these before changing architecture, sync, login or storage:

- `docs/plans/2026-11-10-system-redesign-design.md` - requirements, decisions and the target design
- `docs/plans/2026-11-10-refactor-system-redesign-implementation-plan.md` - build order, trials and clean-up

Rules the redesign set, which still hold:

- Nothing the design lists for deletion (section 14) is left. Do not bring any of it back.
- The CLI and the desktop app on one computer are one device and must name one server. Both get it from `dirt_core::config::ServerAddress`: `DIRT_API_BASE_URL` in the environment, or else the address built in from `.env.client`.
- What the three clients show about sync comes from `dirt_core::sync::overview`. Change the words there, not in a client.
- No SQL, HTTP or cryptography in a client crate. They belong in `dirt-core`.
- `dirt-api` must not depend on `dirt-core`. The two share only `dirt-proto`. `dirt-sim`, the test harness, is the one crate that depends on both.
- `rusqlite` is held at 0.39 by `sqlx` 0.9: see the comment in the root `Cargo.toml` before raising either.
- The server's SQL is written with `sqlx::query` and `sqlx::query_as` on string literals, never the compile-time `query!` macros, so building needs no database.
- Anything planned for later belongs in the design document now, with its place in the build order.
- A program opens its own store with `Store::open_in`, which closes the data folder to the computer's other users first. `Store::open` is for a database file the person named, whose folder is theirs.
- When a document goes out of date, archive it under `docs/archive/` with a banner; do not leave it in place.

## The interface: standing rules

Read these before changing anything a person sees or touches:

- `docs/ui/design.md` - the design: principles, tokens, motion, components, flows, and what was settled as each part was built. `docs/ui/mockup.html` shows it working
- `docs/plans/2026-11-16-interface-redesign-implementation-plan.md` - how it was built, step by step, and what each step ran

Rules it set, which still hold:

- Where the mockup and the design document disagree, the mockup holds for how long a movement takes and on which curve, and the document for everything else. Correct the document in the pull request that finds the disagreement.
- What the design leaves open or gets wrong is settled by whoever builds it, and written into the design document.
- The interface is built for English. Other languages are later work (system design, section 16.3).
- Each app works after every pull request. A change to what a client does changes its rows in `docs/feature-parity.md` and its items in `docs/MANUAL_CHECKLIST.md`.
- What the two apps share of the interface is in `dirt-ui`: the tokens, the fonts, the icons, the motion script and the components. A component both apps need goes there, not into an app. It is a client crate: no SQL, HTTP or cryptography in it.
- The colours are written once, in `dirt_ui::tokens`, and the style sheet is made from them. A colour that changes is checked by `dirt_ui::contrast`'s tests, and the table in design section 4.1 changes with it. The Android capture sheet's and widgets' colours are made from the same tokens and held to them by a test.
- Anything that follows a finger or a pointer runs in `dirt-ui`'s script (`crates/dirt-ui/src/js/dirt-ui.js`) from first touch to release, and tells Rust only how it ended (`dirt_ui::events`). The script that animates a list watches the page; it is not called from Rust.
- A note's text field has no `oninput`: it is heard through `dirt_ui::typing` (`data-dui-typed` and its carrier), which tells Rust each change in order and in pieces. Dioxus sends an `input` from any text field with the field's whole text, which fails past a few hundred kilobytes (design 6.4).
- `dirt-ui`'s classes begin with `dui-`, the desktop's with `dk-` and Android's with `dm-`.
- Speech becomes text on the device that heard it, with the model shipped in the app, and in `dirt-voice` alone. It reads sound and gives words; it has no SQL, HTTP or cryptography, and `dirt-core` does not depend on it (an app hands the core its engine as a `dirt_core::voice::Transcriber`). Only the device that recorded a thought makes its words.
- The speech model and the engine's libraries are never committed. Their builds fetch them by an address and a SHA-256 written in `crates/dirt-voice/build.rs` and `crates/dirt-voice/sherpa-onnx-sys/build.rs`, into the build cache. Change a pin only after trying what it pins.
- What runs on Android without the app's screen (the capture sheet, its jobs, the Quick Settings tile, the two widgets) is Kotlin in the native module, `crates/dirt-mobile/android/native`, and reaches the app's Rust only through `Core` (`dirt-mobile::capture`). It shows nothing of a note but the pile widget's count of them.

## Tech Stack

What the code on `main` uses today:

- **UI Framework**: Dioxus 0.7 (desktop and Android). The interface's tokens, fonts (Inter and JetBrains Mono, bundled), icons, motion and components are in `dirt-ui`, which both apps use, with PDF.js bundled for the file preview
- **Android outside the web view**: Kotlin in the native module (`crates/dirt-mobile/android/native`), declared with `#[manganis::ffi]`: the capture sheet, its two JobScheduler jobs, the Quick Settings tile, the bar widget and the pile widget
- **CLI**: clap
- **Notes**: one `yrs` CRDT document per note (`dirt-core::notes`)
- **Local database**: SQLite through `rusqlite` with the bundled build, FTS5 for full-text search (`dirt-core::store`)
- **Server**: `dirt-api`, axum on Vercel's serverless Rust runtime
- **Server database**: Postgres 18 through `sqlx`; Neon in production, Docker locally
- **Login**: Clerk in the system browser (OAuth with PKCE, no Clerk SDK). The server checks the ID token once and issues a device token
- **Sync**: encrypted note documents (XChaCha20-Poly1305), pulled, merged and pushed by `dirt-core::sync`. It runs on events (start, return to the app, a change, the sync button) and never on a timer
- **Secrets**: the device token, device key and account key in the operating system's keychain (`keyring`) on desktop and CLI, and in Android's encrypted preferences
- **More devices**: Matrix's short-code verification from `vodozemac` hands the account key to a new device; a 24-word `bip39` recovery phrase is the other way in

- **Files**: a file attached to a note is encrypted in pieces with a key kept in the note's document, and goes straight to S3-compatible storage by a link the server signs (`dirt-core::files`, `dirt-core::sync::engine::files`, `dirt-api::files`). A picture gets a thumbnail, made on the device with the `image` crate
- **Export**: every note as one text, JSON or Markdown, with each note's files beside it: a folder from the desktop app and the CLI, and from the Android app the same as one zip file, written with the `zip` crate (`dirt-core::export`)
- **Voice**: `dirt-voice`, sherpa-onnx's prebuilt libraries with NVIDIA's Parakeet TDT-CTC 110M (English, CC BY 4.0) and Silero's detector. A recording, Ogg Opus written with `opus` and `ogg`, is kept as a note with the recording as its file at once (`Store::keep_voice_note`); its words are made afterwards on that device (`dirt_core::voice`). The desktop's microphone is `cpal`; Android's is `AudioRecord`, in Kotlin, and its capture sheet records a WAV beside its typed thoughts, which the app or the words job takes in (`dirt-mobile::capture`). The model and the libraries ship in the app as the voice folder (`crates/dirt-desktop/voice/`; on Android the native module's `assets/voice` and `jniLibs`, copied out of the APK on first use; made by the build, not in git)

## Project Structure

```
api/axum.rs        # Vercel serverless entry point (root package `dirt-vercel`)
crates/
├── dirt-proto/    # Types shared by the server and the clients
├── dirt-core/     # Notes as CRDT documents, the local store, files, encryption, sign-in, the session, sync, export
├── dirt-api/      # The server: device registry and encrypted note blobs on Postgres
├── dirt-sim/      # Tests only: real client cores syncing through the real server in one process
├── dirt-ui/       # What the two apps share of the interface: tokens, fonts, icons, motion, components, and a gallery of them
├── dirt-voice/    # Speech turned into text on the device: the recording, the pauses, the words; its vendored sherpa-onnx-sys
├── dirt-desktop/  # Dioxus desktop application
├── dirt-mobile/   # Dioxus Android application; android/native is its Kotlin module
└── dirt-cli/      # Command-line interface (`dirt`)
scripts/           # Dumping the server's database and restoring it
docs/
├── DESIGN.md      # Product vision and standing decisions
├── ui/            # The interface's design and its mockup
├── plans/         # The two redesigns and their implementation plans
├── trials/        # The system redesign's trials
├── brainstorms/   # Feature brainstorms
└── archive/       # Superseded documents, kept as a record
```

## Development Commands

```bash
# Build all crates
cargo build

# Run desktop app. Plain cargo runs it without its style sheets; dx bundles them:
cd crates/dirt-desktop && dx serve
# For a quick check that it starts:
cargo run -p dirt-desktop

# Show the interface's gallery: every component of dirt-ui on one screen, in
# place of the app. For development only. It reads and writes the theme, the
# accent and the text size in the app's own settings; set DIRT_DATA_DIR to
# keep a real installation's settings out of it
make run-gallery-desktop     # dx serve --platform desktop --package dirt-desktop --features gallery
make run-gallery-android     # dx serve --platform android --package dirt-mobile --features gallery

# Run CLI
cargo run -p dirt-cli

# Run the API server (needs .env.server; see .env.server.example)
cargo run -p dirt-api

# Run the real server on this machine for trying a client by hand, with the
# file storage from compose.yaml and a sign-in that needs no account. The
# comment at the top of crates/dirt-sim/examples/serve.rs says how to use it
cargo run -p dirt-sim --example serve

# Run tests (dirt-api's and dirt-sim's need Postgres: `docker compose up -d`
# and DATABASE_URL as compose.yaml shows)
cargo test --all

# The voice engine's tests that read speech with the model (ignored by
# default, since most of CI builds without it)
cargo test -p dirt-voice -- --include-ignored

# Format code
cargo fmt --all

# Lint code (--workspace is required: without it cargo lints only the root package)
cargo clippy --workspace --all-targets --all-features -- -D warnings

# Lint the Android code, as CI does: what is Android's alone is linted
# nowhere else. Needs the NDK's clang as the linker and CC for the target
# (see .github/workflows/ci.yml); with and without the gallery
cargo clippy -p dirt-mobile --target x86_64-linux-android --all-targets -- -D warnings
cargo clippy -p dirt-mobile --target x86_64-linux-android --all-targets --features gallery -- -D warnings

# Build the Android app for the emulator (debug), or for a phone
dx build --platform android --package dirt-mobile --target x86_64-linux-android
dx build --platform android --package dirt-mobile --target aarch64-linux-android

# List Makefile shortcuts
make help

# Run or build the Android app
make run-mobile-android
make build-mobile-android
```

The first build of `dirt-voice` (the desktop app, the workspace) fetches the speech model (108 MB) and the engine's libraries into a build cache outside `target/`: `DIRT_BUILD_CACHE`, or else `%LOCALAPPDATA%\dirt\build-cache`, `~/Library/Caches/dirt-build` or `~/.cache/dirt-build`. It is kept across `cargo clean` and shared by checkouts. `DIRT_VOICE_MODEL=skip` builds without the model, for checks that do not ship the app: the mic is then not offered. `DIRT_VOICE=off` builds without voice at all and fetches nothing. A development build that cannot fetch (offline, or a target with no pinned libraries) is built without voice and says why; a release build, or `DIRT_VOICE=required` (CI), fails instead, so that a build meant to ship voice never ships without it. On Linux the microphone needs ALSA's headers (`libasound2-dev`). On Windows a program that links `dirt-voice` must load `sherpa-onnx-c-api.dll` late (`/DELAYLOAD`, as `dirt-desktop`'s build script does); `dirt-voice` loads it from the voice folder.

Android builds run Gradle, which may need a different JDK from the machine's default. Do not change `JAVA_HOME` for that. Set the `DIRT_ANDROID_JAVA_HOME` environment variable to the JDK to use (JDK 21 works with the Gradle project `dx` generates); the two Android `make` targets pass it to `dx` and Gradle as `JAVA_HOME` for that command only. Any new script that runs `dx` for Android should do the same.

## Code Conventions

### Rust Style
- Follow Rust 2021 edition idioms
- Define error types with `thiserror` in every crate; `anyhow` is not used
- Prefer `impl Trait` over generics where appropriate
- Document public APIs with doc comments

### Dioxus Patterns
- Use `#[component]` macro for all components
- Use `use_signal` for local state
- Use `use_context` / `use_context_provider` for global state
- Keep components small and focused
- Styles are in style sheets made from the tokens (`dirt-ui`'s, and each app's `screen.css`); an inline `style` only sets a custom property the sheet reads (`--i`, `--n`)

### Database
- All IDs use UUID v7 (time-sortable, client-generated)
- Timestamps stored as Unix milliseconds (i64)
- A note's creation time is read from its ID and is not stored
- A note's row holds its CRDT document plus values derived from it (text, edit time, deleted flag, archived flag); tags and the search index are derived too. Every write updates all of them in one `BEGIN IMMEDIATE` transaction
- A deleted note keeps its row and document, marked deleted, so the delete can sync and be undone. Thirty days after a device saw it deleted, the device asks the server to purge it, and the row goes from every device (`dirt-core::sync`, design section 6.2)
- Archiving a note is not an edit: it moves no edit time and does not bring back a note another device deleted. An archived note is left out of the list and kept by search, tag counts and export
- Tests that need a store open one on a temporary file; in-memory databases do not support the store's connection model
- FTS5 for full-text search

### Tags
- Extracted from content using `#tag` syntax
- Regex: `#[a-zA-Z][a-zA-Z0-9_-]*`
- Stored lowercase, display preserves original case

## Linting Configuration

Workspace lints are defined in root `Cargo.toml`:
- clippy::all, clippy::pedantic, clippy::nursery at warn level
- Specific allows for pedantic lints that are too noisy
- `dbg_macro` and `todo` at warn level

## Pre-commit Hooks

Install with:
```bash
pip install pre-commit
pre-commit install
```

Hooks run:
- `cargo fmt` on commit
- `cargo clippy` on commit
- `cargo test` on push

## Design Documents

- `docs/DESIGN.md` - Product vision, non-goals and standing product decisions
- `docs/ui/design.md` - The design system for the desktop and Android apps: tokens, motion, components, flows. `docs/ui/mockup.html` shows it working
- `docs/SECURITY.md` - What is protected, from whom, and what is not
- `docs/BACKEND_API.md` - The server's protocol
- `docs/LIMITS.md` - Every limit the server enforces, with its reason and what was measured. Change a number in `dirt_proto::api::limits` and its row here together
- `docs/DEPLOY.md` - Running the server, backups, and what to do after a restore
- `docs/OWNER_DECISIONS.md` - What only the owner could decide, what was decided, and what is still the owner's: plans, storage, the sign-in service
- `docs/feature-parity.md` - What each client does. Change a client's row in the pull request that changes the client
- `docs/MANUAL_CHECKLIST.md` - What to try by hand on real devices
- `docs/plans/` - The system redesign and its implementation plan, and the interface redesign's plan
- `docs/brainstorms/` - Feature brainstorms
- `docs/archive/` - Superseded documents, kept as a record

## Key Design Decisions

1. **Offline-first**: All data stored locally in SQLite, sync is additive
2. **Plain text only**: No rich text editor complexity
3. **Tags over folders**: Flat organization with `#tag` extraction
4. **Global hotkey**: System-wide capture shortcut
5. **CLI-first**: Quick capture from terminal: `dirt "my thought"`
