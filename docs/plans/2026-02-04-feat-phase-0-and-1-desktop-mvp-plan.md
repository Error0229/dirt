---
title: "Phase 0 & 1: Foundation + Desktop MVP"
type: feat
date: 2026-02-04
---

# Phase 0 & 1: Foundation + Desktop MVP

## Overview

This plan covers the complete implementation of Dirt's foundation (Phase 0) and desktop MVP (Phase 1), including project scaffolding, core data layer, Dioxus desktop app, and all MVP features. Special emphasis on **excellent linting and formatting configuration** from day one.

**Duration**: Phase 0 (foundation) + Phase 1 (desktop MVP)
**Target**: Usable desktop note-taking app with local storage on Windows, macOS, and Linux

## Problem Statement / Motivation

We need to transform the Dirt design documents into a working application. The foundation must be solid because:

1. **Code sharing**: dirt-core will be used by desktop, mobile, CLI, and TUI
2. **Offline-first**: SQLite schema must support future PowerSync integration
3. **Quality from start**: Linting and formatting prevent tech debt accumulation
4. **Capture velocity**: The core UX metric—sub-2-second thought capture

## Proposed Solution

Build incrementally with these phases:

1. **Phase 0.A**: Project scaffolding + linting/formatting setup
2. **Phase 0.B**: dirt-core crate (models, database, CRUD)
3. **Phase 0.C**: CI/CD pipeline
4. **Phase 1.A**: Dioxus desktop shell
5. **Phase 1.B**: Core UI (list, editor, CRUD)
6. **Phase 1.C**: Quick capture (hotkey, tray)
7. **Phase 1.D**: Search, tags, settings

---

## Technical Approach

### Architecture

```
┌─────────────────────────────────────────────────┐
│                  dirt-desktop                    │
│  (Dioxus 0.6 desktop app)                       │
├─────────────────────────────────────────────────┤
│  components/    views/    state/                │
│  - NoteList     - Home    - AppState            │
│  - NoteEditor   - Settings - use_context        │
│  - QuickCapture                                 │
│  - SystemTray                                   │
└────────────────────┬────────────────────────────┘
                     │
┌────────────────────▼────────────────────────────┐
│                  dirt-core                       │
│  (Shared Rust library)                          │
├─────────────────────────────────────────────────┤
│  models/        db/           search/           │
│  - Note         - migrations  - fts5            │
│  - Tag          - repository  - index           │
│  - Settings     - connection                    │
└────────────────────┬────────────────────────────┘
                     │
              ┌──────▼──────┐
              │   SQLite    │
              │  (rusqlite) │
              └─────────────┘
```

### Crate Structure

```
dirt/
├── Cargo.toml                    # Workspace root
├── rustfmt.toml                  # Formatting config
├── clippy.toml                   # Linting config
├── .pre-commit-config.yaml       # Pre-commit hooks
├── .github/
│   └── workflows/
│       └── ci.yml                # GitHub Actions CI
├── crates/
│   ├── dirt-core/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── models/
│   │       │   ├── mod.rs
│   │       │   ├── note.rs
│   │       │   ├── tag.rs
│   │       │   └── settings.rs
│   │       ├── db/
│   │       │   ├── mod.rs
│   │       │   ├── connection.rs
│   │       │   ├── migrations.rs
│   │       │   └── repository.rs
│   │       ├── search/
│   │       │   ├── mod.rs
│   │       │   └── fts.rs
│   │       └── error.rs
│   ├── dirt-desktop/
│   │   ├── Cargo.toml
│   │   ├── Dioxus.toml
│   │   └── src/
│   │       ├── main.rs
│   │       ├── app.rs
│   │       ├── components/
│   │       │   ├── mod.rs
│   │       │   ├── note_list.rs
│   │       │   ├── note_card.rs
│   │       │   ├── note_editor.rs
│   │       │   ├── quick_capture.rs
│   │       │   ├── search_bar.rs
│   │       │   ├── tag_badge.rs
│   │       │   └── settings_panel.rs
│   │       ├── views/
│   │       │   ├── mod.rs
│   │       │   ├── home.rs
│   │       │   └── settings.rs
│   │       ├── state/
│   │       │   ├── mod.rs
│   │       │   └── app_state.rs
│   │       ├── hotkey.rs
│   │       ├── tray.rs
│   │       └── theme.rs
│   └── dirt-cli/
│       ├── Cargo.toml
│       └── src/
│           └── main.rs           # Placeholder for Phase 3
├── assets/
│   └── icons/
│       ├── icon.ico
│       ├── icon.icns
│       └── icon.png
└── docs/
    ├── DESIGN.md
    └── plans/
```

---

## Implementation Phases

### Phase 0.A: Project Scaffolding + Linting (Issues #1, #5)

**Goal**: Workspace structure with excellent code quality tooling

#### Tasks

- [ ] Create `Cargo.toml` workspace root
- [ ] Create `crates/dirt-core/Cargo.toml` with dependencies
- [ ] Create `crates/dirt-desktop/Cargo.toml` with Dioxus
- [ ] Create `crates/dirt-cli/Cargo.toml` placeholder
- [ ] Create `rustfmt.toml` with formatting rules
- [ ] Create `clippy.toml` with lint configuration
- [ ] Add `[lints]` section to all `Cargo.toml` files
- [ ] Create `.pre-commit-config.yaml`
- [ ] Create `.gitignore` for Rust
- [ ] Create `rust-toolchain.toml` (pin stable)
- [ ] Create basic `CLAUDE.md` for the project

#### `Cargo.toml` (workspace root)

```toml
[workspace]
resolver = "2"
members = ["crates/*"]

[workspace.package]
version = "0.1.0"
edition = "2021"
license = "MIT"
repository = "https://github.com/Error0229/dirt"

[workspace.dependencies]
# Core
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
thiserror = "2.0"
uuid = { version = "1.0", features = ["v7", "serde"] }
chrono = { version = "0.4", features = ["serde"] }

# Database
rusqlite = { version = "0.32", features = ["bundled", "serde_json"] }

# Async
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }

# Logging
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

# Testing
pretty_assertions = "1.4"

[workspace.lints.rust]
unsafe_code = "warn"

[workspace.lints.clippy]
all = { level = "warn", priority = -1 }
pedantic = { level = "warn", priority = -1 }
nursery = { level = "warn", priority = -1 }

# Allow these pedantic lints (too noisy for new project)
missing_errors_doc = "allow"
missing_panics_doc = "allow"
module_name_repetitions = "allow"
must_use_candidate = "allow"
similar_names = "allow"
too_many_lines = "allow"
significant_drop_tightening = "allow"
future_not_send = "allow"

# Restriction lints (opt-in)
dbg_macro = "warn"
todo = "warn"
unimplemented = "warn"
```

#### `rustfmt.toml`

```toml
edition = "2021"
style_edition = "2021"
max_width = 100
reorder_imports = true
reorder_modules = true
use_small_heuristics = "Default"
```

#### `clippy.toml`

```toml
msrv = "1.75"
avoid-breaking-exported-api = true
allow-unwrap-in-tests = true
allow-expect-in-tests = true
cognitive-complexity-threshold = 25
```

#### `.pre-commit-config.yaml`

```yaml
repos:
  - repo: https://github.com/doublify/pre-commit-rust
    rev: v1.0
    hooks:
      - id: fmt
        args: ["--", "--check"]
      - id: clippy
        args: ["--all-targets", "--", "-D", "warnings"]
      - id: cargo-check
```

#### `rust-toolchain.toml`

```toml
[toolchain]
channel = "stable"
components = ["rustfmt", "clippy"]
```

#### Acceptance Criteria

- [ ] `cargo build --workspace` succeeds
- [ ] `cargo fmt --check` passes
- [ ] `cargo clippy --all-targets -- -D warnings` passes
- [ ] `pre-commit run --all-files` passes
- [ ] All crates have `[lints] workspace = true`

---

### Phase 0.B: dirt-core (Issues #2, #3, #4)

**Goal**: Shared library with Note model, SQLite, and CRUD

#### Data Model

```rust
// crates/dirt-core/src/models/note.rs
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A unique identifier for a note, using UUID v7 (time-sortable)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NoteId(Uuid);

impl NoteId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }

    pub fn as_str(&self) -> String {
        self.0.to_string()
    }
}

impl Default for NoteId {
    fn default() -> Self {
        Self::new()
    }
}

/// A note in the system
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub id: NoteId,
    pub content: String,
    pub created_at: i64,    // Unix timestamp ms
    pub updated_at: i64,    // Unix timestamp ms
    pub is_deleted: bool,   // Soft delete for sync
}

impl Note {
    pub fn new(content: impl Into<String>) -> Self {
        let now = chrono::Utc::now().timestamp_millis();
        Self {
            id: NoteId::new(),
            content: content.into(),
            created_at: now,
            updated_at: now,
            is_deleted: false,
        }
    }

    /// Extract #tags from content
    pub fn tags(&self) -> Vec<String> {
        extract_tags(&self.content)
    }

    /// Get first line as title preview
    pub fn title_preview(&self, max_len: usize) -> String {
        self.content
            .lines()
            .next()
            .unwrap_or("")
            .chars()
            .take(max_len)
            .collect()
    }
}

/// Extract #tags from text
/// Valid tags: #[a-zA-Z][a-zA-Z0-9_-]*
pub fn extract_tags(text: &str) -> Vec<String> {
    let re = regex::Regex::new(r"#([a-zA-Z][a-zA-Z0-9_-]*)").unwrap();
    re.captures_iter(text)
        .map(|cap| cap[1].to_lowercase())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect()
}
```

#### SQLite Schema

```sql
-- crates/dirt-core/src/db/migrations/001_initial.sql

-- Notes table
CREATE TABLE IF NOT EXISTS notes (
    id TEXT PRIMARY KEY,
    content TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    is_deleted INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_notes_updated ON notes(updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_notes_created ON notes(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_notes_deleted ON notes(is_deleted);

-- Tags table
CREATE TABLE IF NOT EXISTS tags (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE COLLATE NOCASE,
    created_at INTEGER NOT NULL
);

-- Note-Tag junction table
CREATE TABLE IF NOT EXISTS note_tags (
    note_id TEXT NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
    tag_id TEXT NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    PRIMARY KEY (note_id, tag_id)
);

CREATE INDEX IF NOT EXISTS idx_note_tags_tag ON note_tags(tag_id);

-- Full-text search
CREATE VIRTUAL TABLE IF NOT EXISTS notes_fts USING fts5(
    content,
    content=notes,
    content_rowid=rowid
);

-- Triggers to keep FTS in sync
CREATE TRIGGER IF NOT EXISTS notes_ai AFTER INSERT ON notes BEGIN
    INSERT INTO notes_fts(rowid, content) VALUES (NEW.rowid, NEW.content);
END;

CREATE TRIGGER IF NOT EXISTS notes_ad AFTER DELETE ON notes BEGIN
    INSERT INTO notes_fts(notes_fts, rowid, content) VALUES('delete', OLD.rowid, OLD.content);
END;

CREATE TRIGGER IF NOT EXISTS notes_au AFTER UPDATE ON notes BEGIN
    INSERT INTO notes_fts(notes_fts, rowid, content) VALUES('delete', OLD.rowid, OLD.content);
    INSERT INTO notes_fts(rowid, content) VALUES (NEW.rowid, NEW.content);
END;

-- Settings table (local only)
CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- Schema version
CREATE TABLE IF NOT EXISTS schema_version (
    version INTEGER PRIMARY KEY
);

INSERT OR IGNORE INTO schema_version (version) VALUES (1);
```

#### Repository Trait

```rust
// crates/dirt-core/src/db/repository.rs
use crate::models::{Note, NoteId};
use crate::error::Result;

pub trait NoteRepository: Send + Sync {
    /// Create a new note
    fn create(&self, content: &str) -> Result<Note>;

    /// Get a note by ID
    fn get(&self, id: &NoteId) -> Result<Option<Note>>;

    /// List notes (excluding deleted), newest first
    fn list(&self, limit: usize, offset: usize) -> Result<Vec<Note>>;

    /// Update a note's content
    fn update(&self, id: &NoteId, content: &str) -> Result<Note>;

    /// Soft delete a note
    fn delete(&self, id: &NoteId) -> Result<()>;

    /// Search notes by content
    fn search(&self, query: &str, limit: usize) -> Result<Vec<Note>>;

    /// List notes by tag
    fn list_by_tag(&self, tag: &str, limit: usize, offset: usize) -> Result<Vec<Note>>;

    /// Get all tags with note counts
    fn list_tags(&self) -> Result<Vec<(String, usize)>>;
}
```

#### Key Behaviors (from spec analysis)

| Behavior | Decision |
|----------|----------|
| Auto-save | On blur or 500ms debounce after typing stops |
| Deletion | Soft delete with `is_deleted` flag |
| Tag parsing | `#[a-zA-Z][a-zA-Z0-9_-]*`, case-insensitive storage |
| Note size | No hard limit (reasonable for SQLite) |
| ID generation | UUID v7, client-side |
| Title | First line of content (derived, not stored) |

#### Tasks

- [ ] Create `dirt-core` crate structure
- [ ] Implement `NoteId` type
- [ ] Implement `Note` model with tag extraction
- [ ] Implement `Tag` model
- [ ] Implement `Settings` model
- [ ] Create migration system
- [ ] Implement SQLite connection pool
- [ ] Implement `SqliteNoteRepository`
- [ ] Implement FTS5 search
- [ ] Add tag management (create, link, unlink)
- [ ] Add comprehensive unit tests

#### Acceptance Criteria

- [ ] Can create, read, update, delete notes
- [ ] Tags auto-extracted from content
- [ ] Full-text search works
- [ ] Filter by tag works
- [ ] All operations are atomic
- [ ] 100% test coverage for repository

---

### Phase 0.C: CI/CD Pipeline (Issue #5)

**Goal**: Automated quality checks on every push/PR

#### `.github/workflows/ci.yml`

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

env:
  CARGO_TERM_COLOR: always
  RUSTFLAGS: "-D warnings"

jobs:
  check:
    name: Check
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@v2

      - name: Check formatting
        run: cargo fmt --all -- --check

      - name: Run clippy
        run: cargo clippy --all-targets --all-features -- -D warnings

      - name: Build
        run: cargo build --all-targets

      - name: Run tests
        run: cargo test --all-targets

  # Cross-platform build check
  build:
    name: Build (${{ matrix.os }})
    runs-on: ${{ matrix.os }}
    strategy:
      matrix:
        os: [ubuntu-latest, windows-latest, macos-latest]
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2

      - name: Build
        run: cargo build --release
```

#### Tasks

- [ ] Create `.github/workflows/ci.yml`
- [ ] Test CI passes locally with `act` (optional)
- [ ] Verify CI runs on push and PR

#### Acceptance Criteria

- [ ] CI runs on every push to main
- [ ] CI runs on every PR
- [ ] Builds on Linux, macOS, Windows
- [ ] All checks must pass to merge

---

### Phase 1.A: Dioxus Desktop Shell (Issue #6)

**Goal**: Basic Dioxus desktop window with layout

#### `Dioxus.toml`

```toml
[application]
name = "Dirt"
default_platform = "desktop"
out_dir = "dist"
asset_dir = "assets"

[web.app]
title = "Dirt"

[web.watcher]
reload_html = true
watch_path = ["src", "assets"]

[bundle]
identifier = "com.dirt.app"
publisher = "Dirt"
icon = ["assets/icons/icon.ico", "assets/icons/icon.icns", "assets/icons/icon.png"]

[bundle.windows]
# Windows-specific bundle settings

[bundle.macos]
minimum_system_version = "10.13"

[bundle.linux]
# Linux-specific bundle settings
```

#### App Shell

```rust
// crates/dirt-desktop/src/main.rs
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use dioxus::prelude::*;

mod app;
mod components;
mod state;
mod views;
mod hotkey;
mod tray;
mod theme;

fn main() {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter("dirt=debug")
        .init();

    dioxus::launch(app::App);
}
```

```rust
// crates/dirt-desktop/src/app.rs
use dioxus::prelude::*;
use crate::state::AppState;
use crate::views::Home;
use crate::theme::Theme;

pub fn App() -> Element {
    // Initialize global state
    let notes = use_signal(Vec::new);
    let current_note_id = use_signal(|| None);
    let search_query = use_signal(String::new);
    let active_tag_filter = use_signal(|| None);
    let theme = use_signal(Theme::default);

    use_context_provider(|| AppState {
        notes,
        current_note_id,
        search_query,
        active_tag_filter,
        theme,
    });

    // Load notes on mount
    use_effect(move || {
        // TODO: Load notes from dirt-core
    });

    rsx! {
        div {
            class: "app-container",
            class: if theme().is_dark() { "dark" } else { "light" },
            Home {}
        }
    }
}
```

#### Tasks

- [ ] Create dirt-desktop crate with Dioxus dependencies
- [ ] Create `Dioxus.toml` configuration
- [ ] Create main.rs with app launch
- [ ] Create app.rs with context providers
- [ ] Create basic Home view layout
- [ ] Create placeholder icons
- [ ] Verify `dx serve --platform desktop` works

#### Acceptance Criteria

- [ ] App launches on all platforms
- [ ] Window has correct title "Dirt"
- [ ] App closes cleanly
- [ ] Hot reload works for RSX changes

---

### Phase 1.B: Core UI (Issues #7, #8, #9)

**Goal**: Note list, editor, and CRUD operations

#### NoteList Component

```rust
// crates/dirt-desktop/src/components/note_list.rs
use dioxus::prelude::*;
use crate::state::AppState;
use crate::components::NoteCard;

#[component]
pub fn NoteList() -> Element {
    let state = use_context::<AppState>();
    let notes = state.notes.read();

    rsx! {
        div { class: "note-list",
            if notes.is_empty() {
                div { class: "empty-state",
                    p { "No notes yet" }
                    p { class: "hint", "Press Ctrl+N to create your first note" }
                }
            } else {
                for note in notes.iter().filter(|n| !n.is_deleted) {
                    NoteCard {
                        key: "{note.id.as_str()}",
                        note: note.clone()
                    }
                }
            }
        }
    }
}
```

#### NoteEditor Component

```rust
// crates/dirt-desktop/src/components/note_editor.rs
use dioxus::prelude::*;
use dirt_core::models::Note;

#[component]
pub fn NoteEditor(
    note: Option<Note>,
    on_save: EventHandler<String>,
    on_close: EventHandler<()>,
) -> Element {
    let mut content = use_signal(|| note.as_ref().map(|n| n.content.clone()).unwrap_or_default());
    let mut is_dirty = use_signal(|| false);

    // Auto-save with debounce
    let save_timeout = use_signal(|| None::<i32>);

    let handle_input = move |evt: Event<FormData>| {
        content.set(evt.value().clone());
        is_dirty.set(true);

        // Debounced auto-save (500ms)
        // TODO: Implement with use_future or similar
    };

    let handle_save = move |_| {
        if is_dirty() {
            on_save.call(content());
            is_dirty.set(false);
        }
    };

    let handle_keydown = move |evt: Event<KeyboardData>| {
        // Escape to close
        if evt.key() == Key::Escape {
            if is_dirty() {
                on_save.call(content());
            }
            on_close.call(());
        }
        // Cmd/Ctrl+S to save
        if evt.key() == Key::Character("s".to_string()) && evt.modifiers().meta() {
            handle_save(());
        }
    };

    rsx! {
        div { class: "note-editor",
            textarea {
                class: "editor-content",
                value: "{content}",
                oninput: handle_input,
                onkeydown: handle_keydown,
                onblur: handle_save,
                autofocus: true,
                placeholder: "Start typing...",
            }
            div { class: "editor-status",
                if is_dirty() {
                    span { class: "unsaved", "Unsaved" }
                } else {
                    span { class: "saved", "Saved" }
                }
            }
        }
    }
}
```

#### Tasks

- [ ] Create NoteList component
- [ ] Create NoteCard component
- [ ] Create NoteEditor component
- [ ] Create empty state design
- [ ] Wire up CRUD to dirt-core
- [ ] Implement auto-save with 500ms debounce
- [ ] Add keyboard shortcuts (Ctrl+N new, Escape close)
- [ ] Add delete with confirmation dialog
- [ ] Display relative timestamps ("2 minutes ago")

#### Acceptance Criteria

- [ ] Notes display in reverse chronological order
- [ ] Can create new notes
- [ ] Can edit existing notes
- [ ] Changes auto-save
- [ ] Can delete notes (soft delete)
- [ ] Empty state shown when no notes
- [ ] Relative timestamps update

---

### Phase 1.C: Quick Capture (Issues #10, #11)

**Goal**: Global hotkey and system tray for rapid capture

#### Global Hotkey

```rust
// crates/dirt-desktop/src/hotkey.rs
use global_hotkey::{GlobalHotKeyManager, HotKeyState, hotkey::HotKey};

pub struct HotkeyManager {
    manager: GlobalHotKeyManager,
    capture_hotkey: HotKey,
}

impl HotkeyManager {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let manager = GlobalHotKeyManager::new()?;

        // Default: Ctrl+Shift+D (Windows/Linux) or Cmd+Shift+D (macOS)
        let capture_hotkey = HotKey::new(
            Some(Modifiers::CONTROL | Modifiers::SHIFT),
            Code::KeyD,
        );

        manager.register(capture_hotkey)?;

        Ok(Self { manager, capture_hotkey })
    }
}
```

#### Quick Capture Window

```rust
// crates/dirt-desktop/src/components/quick_capture.rs
use dioxus::prelude::*;

#[component]
pub fn QuickCapture(
    on_save: EventHandler<String>,
    on_close: EventHandler<()>,
) -> Element {
    let mut content = use_signal(String::new);

    let handle_submit = move |_| {
        let text = content();
        if !text.trim().is_empty() {
            on_save.call(text);
        }
        on_close.call(());
    };

    let handle_keydown = move |evt: Event<KeyboardData>| {
        match evt.key() {
            Key::Escape => on_close.call(()),
            Key::Enter if evt.modifiers().meta() || evt.modifiers().control() => {
                handle_submit(());
            }
            _ => {}
        }
    };

    rsx! {
        div { class: "quick-capture-overlay",
            div { class: "quick-capture-window",
                textarea {
                    class: "quick-capture-input",
                    value: "{content}",
                    oninput: move |e| content.set(e.value().clone()),
                    onkeydown: handle_keydown,
                    autofocus: true,
                    placeholder: "Capture a thought... (Ctrl+Enter to save)",
                }
                div { class: "quick-capture-actions",
                    button {
                        class: "btn-save",
                        onclick: handle_submit,
                        "Save"
                    }
                    button {
                        class: "btn-cancel",
                        onclick: move |_| on_close.call(()),
                        "Cancel"
                    }
                }
            }
        }
    }
}
```

#### Tasks

- [ ] Add `global-hotkey` crate dependency
- [ ] Implement hotkey registration (Ctrl+Shift+D default)
- [ ] Create QuickCapture floating window component
- [ ] Wire hotkey to show QuickCapture
- [ ] Handle hotkey conflicts gracefully (warn user)
- [ ] Add `tray-icon` crate for system tray
- [ ] Implement system tray with menu (New Note, Open, Quit)
- [ ] Keep app running when window closed (minimize to tray)

#### Acceptance Criteria

- [ ] Global hotkey works when app is in background
- [ ] Quick capture window appears in <200ms
- [ ] Ctrl+Enter saves and closes
- [ ] Escape closes without saving
- [ ] System tray icon visible
- [ ] Can capture from tray menu
- [ ] App stays in tray when window closed

---

### Phase 1.D: Search, Tags, Settings (Issues #12-16)

**Goal**: Complete the MVP feature set

#### Search Bar

```rust
// crates/dirt-desktop/src/components/search_bar.rs
use dioxus::prelude::*;
use crate::state::AppState;

#[component]
pub fn SearchBar() -> Element {
    let state = use_context::<AppState>();
    let mut local_query = use_signal(String::new);

    // Debounced search (300ms)
    use_effect(move || {
        let query = local_query();
        // TODO: Debounce and update state.search_query
    });

    rsx! {
        div { class: "search-bar",
            input {
                r#type: "text",
                class: "search-input",
                placeholder: "Search notes...",
                value: "{local_query}",
                oninput: move |e| local_query.set(e.value().clone()),
            }
            if !local_query().is_empty() {
                button {
                    class: "search-clear",
                    onclick: move |_| local_query.set(String::new()),
                    "×"
                }
            }
        }
    }
}
```

#### Settings Panel

```rust
// crates/dirt-desktop/src/components/settings_panel.rs
use dioxus::prelude::*;
use crate::state::AppState;
use crate::theme::Theme;

#[derive(Debug, Clone, PartialEq)]
pub struct AppSettings {
    pub font_family: String,
    pub font_size: u32,
    pub theme: ThemeMode,
    pub capture_hotkey: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ThemeMode {
    Light,
    Dark,
    System,
}

#[component]
pub fn SettingsPanel(on_close: EventHandler<()>) -> Element {
    let state = use_context::<AppState>();
    let mut settings = use_signal(AppSettings::default);

    rsx! {
        div { class: "settings-panel",
            h2 { "Settings" }

            div { class: "setting-group",
                label { "Theme" }
                select {
                    value: "{settings().theme:?}",
                    onchange: move |e| {
                        let mut s = settings();
                        s.theme = match e.value().as_str() {
                            "Light" => ThemeMode::Light,
                            "Dark" => ThemeMode::Dark,
                            _ => ThemeMode::System,
                        };
                        settings.set(s);
                    },
                    option { value: "System", "System" }
                    option { value: "Light", "Light" }
                    option { value: "Dark", "Dark" }
                }
            }

            div { class: "setting-group",
                label { "Font Size" }
                input {
                    r#type: "range",
                    min: "10",
                    max: "24",
                    value: "{settings().font_size}",
                    oninput: move |e| {
                        let mut s = settings();
                        s.font_size = e.value().parse().unwrap_or(14);
                        settings.set(s);
                    }
                }
                span { "{settings().font_size}px" }
            }

            div { class: "setting-group",
                label { "Capture Hotkey" }
                input {
                    r#type: "text",
                    value: "{settings().capture_hotkey}",
                    placeholder: "Ctrl+Shift+D",
                    // TODO: Hotkey capture UI
                }
            }

            div { class: "settings-actions",
                button { onclick: move |_| on_close.call(()), "Close" }
            }
        }
    }
}
```

#### Tasks

- [ ] Create SearchBar component with debounce
- [ ] Wire search to FTS5 queries
- [ ] Highlight search matches in results (optional)
- [ ] Create TagBadge component
- [ ] Display tags on note cards
- [ ] Implement tag click to filter
- [ ] Show active filter indicator
- [ ] Create SettingsPanel component
- [ ] Implement theme switching (light/dark/system)
- [ ] Implement font size adjustment
- [ ] Persist settings to SQLite
- [ ] Apply settings globally via context

#### Acceptance Criteria

- [ ] Search finds notes by content
- [ ] Search is fast (<100ms)
- [ ] Tags visible on notes
- [ ] Can filter by clicking tag
- [ ] Can clear filter
- [ ] Theme changes apply immediately
- [ ] Font size changes apply immediately
- [ ] Settings persist across restarts

---

## Acceptance Criteria (Overall)

### Functional Requirements

- [ ] Can create notes with sub-2-second capture time
- [ ] Can edit and auto-save notes
- [ ] Can delete notes (soft delete)
- [ ] Can search notes via FTS
- [ ] Can filter notes by tag
- [ ] Global hotkey works from anywhere
- [ ] System tray provides quick access
- [ ] Settings persist across sessions

### Non-Functional Requirements

- [ ] App launches in <3 seconds
- [ ] Memory usage <100MB idle
- [ ] All code passes `cargo clippy -- -D warnings`
- [ ] All code passes `cargo fmt --check`
- [ ] CI passes on all platforms
- [ ] No unsafe code without explicit justification

### Quality Gates

- [ ] Unit tests for dirt-core repository
- [ ] Manual testing on Windows, macOS, Linux
- [ ] Pre-commit hooks installed and working

---

## Dependencies & Prerequisites

| Dependency | Purpose | Blocks |
|------------|---------|--------|
| Rust stable | Build toolchain | Everything |
| Dioxus 0.6 | Desktop UI | Phase 1.A+ |
| rusqlite | Database | Phase 0.B+ |
| global-hotkey | Capture hotkey | Phase 1.C |
| tray-icon | System tray | Phase 1.C |

---

## Risk Analysis & Mitigation

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Dioxus mobile immature | Medium | Medium | Focus on desktop first, mobile is Phase 4 |
| Global hotkey platform issues | Medium | High | Test early, have fallback (tray only) |
| SQLite locking issues | Low | High | Use WAL mode, single connection |
| Clippy too strict | Low | Low | Can relax specific lints if needed |

---

## References & Research

### Internal References
- Design document: `docs/DESIGN.md`
- Brainstorm: `docs/brainstorms/2026-02-04-dirt-app-brainstorm.md`
- GitHub issues: #1-#20

### External References
- [Dioxus 0.6 Documentation](https://dioxuslabs.com/learn/0.6/)
- [Dioxus Anti-Patterns](https://dioxuslabs.com/learn/0.6/cookbook/antipatterns/)
- [Clippy Configuration](https://doc.rust-lang.org/clippy/configuration.html)
- [rustfmt Configuration](https://rust-lang.github.io/rustfmt/)
- [global-hotkey crate](https://docs.rs/global-hotkey)
- [tray-icon crate](https://docs.rs/tray-icon)

---

*Plan created: 2026-02-04*
*Status: Ready for implementation*
