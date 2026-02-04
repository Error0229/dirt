# Dirt - Claude Code Guidelines

## Project Overview

Dirt ("Do I Remember That?") is a cross-platform note-taking app focused on capturing fleeting thoughts with minimal friction. Target capture velocity: under 2 seconds from thought to saved note.

## Tech Stack

- **UI Framework**: Dioxus 0.6 (Rust-native, cross-platform)
- **Database**: SQLite with FTS5 for full-text search
- **Sync** (future): PowerSync + Turso
- **Media Storage** (future): Cloudflare R2
- **CLI/TUI** (future): Ratatui

## Project Structure

```
crates/
├── dirt-core/     # Shared business logic, models, database
├── dirt-desktop/  # Dioxus desktop application
└── dirt-cli/      # Command-line interface (placeholder)
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

# Run tests
cargo test --all

# Format code
cargo fmt --all

# Lint code
cargo clippy --all-targets --all-features -- -D warnings
```

## Code Conventions

### Rust Style
- Follow Rust 2021 edition idioms
- Use `thiserror` for error types in libraries
- Use `anyhow` for error handling in binaries
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
- Use soft deletes (`is_deleted` flag) for sync compatibility
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

- `docs/DESIGN.md` - Comprehensive design document
- `docs/brainstorms/` - Feature brainstorms
- `docs/plans/` - Implementation plans

## Key Design Decisions

1. **Offline-first**: All data stored locally in SQLite, sync is additive
2. **Plain text only**: No rich text editor complexity
3. **Tags over folders**: Flat organization with `#tag` extraction
4. **Global hotkey**: System-wide capture shortcut
5. **CLI-first**: Quick capture from terminal: `dirt "my thought"`
