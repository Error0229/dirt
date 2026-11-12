# Dirt - Claude Code Guidelines

## Project Overview

Dirt ("Do I Remember That?") is a cross-platform note-taking app focused on capturing fleeting thoughts with minimal friction. Target capture velocity: under 2 seconds from thought to saved note.

The project is also a system design case study. Multi-user accounts, three clients, sync and end-to-end encryption are all real requirements. Do not propose cutting scope to reduce complexity; justify each part against the requirements instead.

## Status: redesign in progress

`main` is partway through an approved redesign: steps 1 to 6 of 9 are done. Devices sign in through Clerk and sync their notes, encrypted, through the new server. A second device is let in by comparing a code with one that is already in, or by the recovery phrase. `docs/SECURITY.md` describes what is protected. Read both documents before changing architecture, sync, login or storage:

- `docs/plans/2026-11-10-system-redesign-design.md` - requirements, decisions and the target design
- `docs/plans/2026-11-10-refactor-system-redesign-implementation-plan.md` - build order, trials and clean-up

Rules while the redesign is under way:

- Nothing the design lists for deletion (section 14) is left. Do not bring any of it back.
- The CLI and the desktop app on one computer are one device and must name one server. Both get it from `dirt_core::config::ServerAddress`: `DIRT_API_BASE_URL` in the environment, or else the address built in from `.env.client`.
- What the three clients show about sync comes from `dirt_core::sync::overview`. Change the words there, not in a client.
- No SQL, HTTP or cryptography in a client crate. They belong in `dirt-core`.
- `dirt-api` must not depend on `dirt-core`. The two share only `dirt-proto`. `dirt-sim`, the test harness, is the one crate that depends on both.
- `rusqlite` is held at 0.39 by `sqlx` 0.9: see the comment in the root `Cargo.toml` before raising either.
- The server's SQL is written with `sqlx::query` and `sqlx::query_as` on string literals, never the compile-time `query!` macros, so building needs no database.
- Anything planned for later belongs in the design document now, with its place in the build order.
- When a document goes out of date, archive it under `docs/archive/` with a banner; do not leave it in place.

## Tech Stack

What the code on `main` uses today:

- **UI Framework**: Dioxus 0.7 (desktop and Android)
- **CLI**: clap
- **Notes**: one `yrs` CRDT document per note (`dirt-core::notes`)
- **Local database**: SQLite through `rusqlite` with the bundled build, FTS5 for full-text search (`dirt-core::store`)
- **Server**: `dirt-api`, axum on Vercel's serverless Rust runtime
- **Server database**: Postgres 18 through `sqlx`; Neon in production, Docker locally
- **Login**: Clerk in the system browser (OAuth with PKCE, no Clerk SDK). The server checks the ID token once and issues a device token
- **Sync**: encrypted note documents (XChaCha20-Poly1305), pulled, merged and pushed by `dirt-core::sync`. It runs on events (start, return to the app, a change, the sync button) and never on a timer
- **Secrets**: the device token, device key and account key in the operating system's keychain (`keyring`) on desktop and CLI, and in Android's encrypted preferences
- **More devices**: Matrix's short-code verification from `vodozemac` hands the account key to a new device; a 24-word `bip39` recovery phrase is the other way in

What the redesign still adds: the clients' remaining screens and feature parity, changing the account key, backups, and attachments. See the design's build order.

## Project Structure

```
api/axum.rs        # Vercel serverless entry point (root package `dirt-vercel`)
crates/
├── dirt-proto/    # Types shared by the server and the clients
├── dirt-core/     # Notes as CRDT documents, the local store, encryption, sign-in, the session, sync, export
├── dirt-api/      # The server: device registry and encrypted note blobs on Postgres
├── dirt-sim/      # Tests only: real client cores syncing through the real server in one process
├── dirt-desktop/  # Dioxus desktop application
├── dirt-mobile/   # Dioxus Android application
└── dirt-cli/      # Command-line interface (`dirt`)
docs/
├── DESIGN.md      # Product vision and standing decisions
├── plans/         # Redesign and implementation plan
└── archive/       # Documents for the system before the redesign
```

## Development Commands

```bash
# Build all crates
cargo build

# Run desktop app. Plain cargo runs it without its style sheets; dx bundles them:
cd crates/dirt-desktop && dx serve
# For a quick check that it starts:
cargo run -p dirt-desktop

# Run CLI
cargo run -p dirt-cli

# Run the API server (needs .env.server; see .env.server.example)
cargo run -p dirt-api

# Run tests (dirt-api's and dirt-sim's need Postgres: `docker compose up -d`
# and DATABASE_URL as compose.yaml shows)
cargo test --all

# Format code
cargo fmt --all

# Lint code (--workspace is required: without it cargo lints only the root package)
cargo clippy --workspace --all-targets --all-features -- -D warnings

# List Makefile shortcuts
make help

# Run or build the Android app
make run-mobile-android
make build-mobile-android
```

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
- Prefer inline styles during development, extract to CSS later

### Database
- All IDs use UUID v7 (time-sortable, client-generated)
- Timestamps stored as Unix milliseconds (i64)
- A note's creation time is read from its ID and is not stored
- A note's row holds its CRDT document plus values derived from it (text, edit time, deleted flag); tags and the search index are derived too. Every write updates all of them in one `BEGIN IMMEDIATE` transaction
- A deleted note keeps its row and document, marked deleted, so the delete can sync and be undone
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
- `docs/SECURITY.md` - What is protected, from whom, and what is not
- `docs/BACKEND_API.md` - The server's protocol
- `docs/feature-parity.md` - What each client does. Change a client's row in the pull request that changes the client
- `docs/MANUAL_CHECKLIST.md` - What to try by hand on real devices
- `docs/plans/` - The redesign and its implementation plan
- `docs/brainstorms/` - Feature brainstorms
- `docs/archive/` - Superseded documents, kept as a record

## Key Design Decisions

1. **Offline-first**: All data stored locally in SQLite, sync is additive
2. **Plain text only**: No rich text editor complexity
3. **Tags over folders**: Flat organization with `#tag` extraction
4. **Global hotkey**: System-wide capture shortcut
5. **CLI-first**: Quick capture from terminal: `dirt "my thought"`
