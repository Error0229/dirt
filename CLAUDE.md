# Dirt - Claude Code Guidelines

## Project Overview

Dirt ("Do I Remember That?") is a cross-platform note-taking app focused on capturing fleeting thoughts with minimal friction. Target capture velocity: under 2 seconds from thought to saved note.

The project is also a system design case study. Multi-user accounts, three clients, sync and end-to-end encryption are all real requirements. Do not propose cutting scope to reduce complexity; justify each part against the requirements instead.

## Status: redesign in progress

`main` is partway through an approved redesign: steps 1 to 4 of 9 are done, and the first half of step 5 (the sync core and its simulation tests). The clients run on the new local core and the new server is built, but no client talks to it yet: sync is off until the second half of step 5. Read both documents before changing architecture, sync, login or storage:

- `docs/plans/2026-11-10-system-redesign-design.md` - requirements, decisions and the target design
- `docs/plans/2026-11-10-refactor-system-redesign-implementation-plan.md` - build order, trials and clean-up

Rules while the redesign is under way:

- Do not extend code the design lists for deletion (section 14): the old HTTP clients `dirt-core/src/sync/api_client.rs` and `session_client.rs`, and the old login client in `dirt-core/src/auth/`. Step 5 replaces both. The new sync cycle is `dirt-core/src/sync/engine.rs`.
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
- **Login**: the server accepts a Clerk ID token and issues a device token. The clients still hold the old email-code sign-in until step 5
- **Sync**: none in the clients at present. The sync cycle, note encryption (XChaCha20-Poly1305) and the server client exist in `dirt-core` and are exercised by `dirt-sim`; nothing calls them from a client yet

What the redesign still moves to: Clerk sign-in in the clients, event-driven sync of encrypted documents, and end-to-end encryption. See the design's decision table.

## Project Structure

```
api/axum.rs        # Vercel serverless entry point (root package `dirt-vercel`)
crates/
├── dirt-proto/    # Types shared by the server and the clients
├── dirt-core/     # Notes as CRDT documents, the local store, encryption, the sync cycle, export, old login client
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

# Run desktop app
cargo run -p dirt-desktop
# Or use dx CLI for hot reload:
cd crates/dirt-desktop && dx serve

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
- `docs/plans/` - The redesign and its implementation plan
- `docs/brainstorms/` - Feature brainstorms
- `docs/archive/` - Superseded documents, kept as a record

## Key Design Decisions

1. **Offline-first**: All data stored locally in SQLite, sync is additive
2. **Plain text only**: No rich text editor complexity
3. **Tags over folders**: Flat organization with `#tag` extraction
4. **Global hotkey**: System-wide capture shortcut
5. **CLI-first**: Quick capture from terminal: `dirt "my thought"`
