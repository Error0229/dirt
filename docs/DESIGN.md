# Dirt - Design Document

> "Do I Remember That?" - A cross-platform app for capturing fleeting thoughts.

**Version**: 0.1.0 (Pre-release)
**Last Updated**: 2026-02-04
**Status**: Design Phase

---

## Table of Contents

1. [Vision & Philosophy](#vision--philosophy)
2. [Tech Stack](#tech-stack)
3. [Architecture Overview](#architecture-overview)
4. [Feature Roadmap](#feature-roadmap)
5. [Data Model](#data-model)
6. [Platform Support](#platform-support)
7. [Deployment & Distribution](#deployment--distribution)
8. [Design Decisions](#design-decisions)
9. [Open Questions](#open-questions)

---

## Vision & Philosophy

### Core Problem
Ideas flash in the mind and disappear. The friction of opening an app, finding the right note, and typing kills most thoughts before they're captured.

### Solution
**Capture velocity** is the only metric that matters. Time from "I had a thought" to "it's safely recorded" should be **under 2 seconds**.

### Guiding Principles

1. **Speed over features** - Every feature must justify its impact on capture latency
2. **Plain text first** - Formatting is a distraction from capture
3. **Offline-first** - Never lose a thought because of network
4. **Cross-platform parity** - Same experience everywhere
5. **No lock-in** - Your notes are plain text, exportable anytime

### Non-Goals

- Bi-directional linking (this isn't a PKM/Zettelkasten tool)
- Rich text editing or WYSIWYG
- Real-time collaboration
- Complex organizational hierarchies

---

## Tech Stack

| Layer | Technology | Version | Rationale |
|-------|------------|---------|-----------|
| **Desktop/Mobile UI** | Dioxus | 0.6.x | Native Rust, <5MB bundles, code sharing with CLI |
| **CLI** | Ratatui + clap | latest | Pure Rust, shares data layer with GUI |
| **Local Storage** | SQLite (rusqlite) | 3.x | Proven, portable, offline foundation |
| **Sync Layer** | Turso Embedded Replicas | latest | Native libSQL sync, Rust-first |
| **Backend DB** | Turso (libSQL) | latest | Edge SQLite, embedded replicas |
| **Media Storage** | Cloudflare R2 | - | Zero egress fees, S3-compatible |
| **Auth** | Supabase Auth | latest | Email/password auth with JWT sessions |

### Why This Stack?

- **Dioxus over Tauri**: Full Rust enables sharing models, sync logic, and utilities between GUI and CLI. Single language, single build system.
- **Turso Embedded Replicas over PowerSync**: PowerSync has no Rust SDK and doesn't support Turso as a backend. Turso's native embedded replicas provide local SQLite reads with microsecond latency, automatic background sync to cloud, and offline capability—all with first-class Rust support via the `libsql` crate.
- **Turso over raw Postgres**: SQLite everywhere means same queries work locally and in cloud. Embedded replicas for edge performance.

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                        User Interfaces                          │
├─────────────┬─────────────┬─────────────┬─────────────┬────────┤
│   Desktop   │   Android   │     iOS     │     CLI     │   TUI  │
│  (Dioxus)   │  (Dioxus)   │  (Dioxus)   │   (clap)    │(ratatui)│
└──────┬──────┴──────┬──────┴──────┬──────┴──────┬──────┴───┬────┘
       │             │             │             │          │
       └─────────────┴──────┬──────┴─────────────┴──────────┘
                            │
                    ┌───────▼───────┐
                    │   dirt-core   │  ← Shared Rust library
                    │  (lib crate)  │
                    ├───────────────┤
                    │ • Note CRUD   │
                    │ • Search      │
                    │ • Tag mgmt    │
                    │ • Sync logic  │
                    │ • Config      │
                    └───────┬───────┘
                            │
              ┌─────────────┼─────────────┐
              │             │             │
       ┌──────▼──────┐            ┌──────▼──────┐
       │   libSQL    │            │ Cloudflare  │
       │  (local +   │◄──sync───► │     R2      │
       │  embedded   │            │  (media)    │
       │  replica)   │            └─────────────┘
       └──────┬──────┘
              │
         ┌────▼────┐
         │  Turso  │
         │ (cloud) │
         └─────────┘
```

### Crate Structure

```
dirt/
├── Cargo.toml              # Workspace root
├── crates/
│   ├── dirt-core/          # Shared library (models, db, sync)
│   ├── dirt-desktop/       # Dioxus desktop app
│   ├── dirt-mobile/        # Dioxus mobile app (Android/iOS)
│   ├── dirt-cli/           # CLI tool
│   └── dirt-tui/           # TUI interface (future)
├── docs/
│   ├── DESIGN.md           # This file
│   └── brainstorms/
└── assets/
    └── icons/
```

---

## Feature Roadmap

### Phase 0: Foundation (Current)
**Goal**: Project setup, basic infrastructure

| ID | Feature | Priority | Status | Blocks |
|----|---------|----------|--------|--------|
| F0.1 | Project scaffolding (Cargo workspace) | P0 | Todo | - |
| F0.2 | dirt-core crate with Note model | P0 | Todo | F0.1 |
| F0.3 | SQLite schema + migrations | P0 | Todo | F0.2 |
| F0.4 | Basic CRUD operations | P0 | Todo | F0.3 |
| F0.5 | CI/CD pipeline (GitHub Actions) | P1 | Todo | F0.1 |

### Phase 1: Desktop MVP
**Goal**: Usable desktop app with local storage

| ID | Feature | Priority | Status | Blocks |
|----|---------|----------|--------|--------|
| F1.1 | Dioxus desktop app shell | P0 | Todo | F0.4 |
| F1.2 | Note list view (chronological) | P0 | Todo | F1.1 |
| F1.3 | Note editor (plain text) | P0 | Todo | F1.1 |
| F1.4 | Create/edit/delete notes | P0 | Todo | F1.2, F1.3 |
| F1.5 | Global hotkey capture | P0 | Todo | F1.4 |
| F1.6 | System tray with quick capture | P1 | Todo | F1.4 |
| F1.7 | Full-text search | P1 | Todo | F1.4 |
| F1.8 | Tag support (#tags in content) | P1 | Todo | F1.4 |
| F1.9 | Tag filtering | P2 | Todo | F1.8 |
| F1.10 | Settings UI (font, size, theme) | P2 | Todo | F1.1 |
| F1.11 | Dark/light theme | P2 | Todo | F1.10 |

### Phase 2: Sync
**Goal**: Multi-device sync with offline support

| ID | Feature | Priority | Status | Blocks |
|----|---------|----------|--------|--------|
| F2.1 | Migrate rusqlite to libsql | P0 | In Progress | F1.4 |
| F2.2 | Turso backend setup | P0 | In Progress | - |
| F2.3 | User authentication | P0 | Todo | F2.2 |
| F2.4 | Embedded replicas + sync | P0 | Todo | F2.1 |
| F2.5 | Conflict resolution (LWW) | P0 | Todo | F2.4 |
| F2.6 | Sync status indicator | P1 | Todo | F2.4 |
| F2.7 | Offline queue visualization | P2 | Todo | F2.6 |

### Phase 3: CLI
**Goal**: Terminal interface for power users

| ID | Feature | Priority | Status | Blocks |
|----|---------|----------|--------|--------|
| F3.1 | `dirt add "thought"` command | P0 | Todo | F0.4 |
| F3.2 | `dirt list` command | P0 | Todo | F0.4 |
| F3.3 | `dirt search <query>` command | P1 | Todo | F3.2 |
| F3.4 | `dirt edit <id>` command | P1 | Todo | F3.2 |
| F3.5 | `dirt delete <id>` command | P1 | Todo | F3.2 |
| F3.6 | `dirt sync` command | P1 | Todo | F2.4, F3.1 |
| F3.7 | `dirt export` command | P2 | Todo | F3.2 |
| F3.8 | Shell completions (bash, zsh, fish) | P2 | Todo | F3.1 |

### Phase 4: Android
**Goal**: Mobile capture experience

| ID | Feature | Priority | Status | Blocks |
|----|---------|----------|--------|--------|
| F4.1 | Dioxus Android app shell | P0 | Done | F1.4 |
| F4.2 | Note list + editor (mobile UI) | P0 | Done | F4.1 |
| F4.3 | Quick capture widget | P1 | In Progress | F4.2 |
| F4.4 | Share intent receiver | P1 | Todo | F4.2 |
| F4.5 | Push notifications for sync | P2 | Todo | F2.4, F4.1 |

### Phase 5: Media Support
**Goal**: Attach images, audio, files to notes

| ID | Feature | Priority | Status | Blocks |
|----|---------|----------|--------|--------|
| F5.1 | Cloudflare R2 integration | P0 | Todo | F2.3 |
| F5.2 | Image paste/drop (desktop) | P0 | Todo | F5.1, F1.4 |
| F5.3 | Image picker (mobile) | P0 | Todo | F5.1, F4.2 |
| F5.4 | Image thumbnail generation | P1 | Todo | F5.2 |
| F5.5 | Voice memo recording | P1 | Todo | F5.1 |
| F5.6 | Voice transcription (optional) | P2 | Todo | F5.5 |
| F5.7 | File attachments | P2 | Todo | F5.1 |

### Phase 6: Polish & Distribution
**Goal**: Production-ready release

| ID | Feature | Priority | Status | Blocks |
|----|---------|----------|--------|--------|
| F6.1 | macOS code signing + notarization | P0 | Todo | F1.4 |
| F6.2 | Windows code signing | P0 | Todo | F1.4 |
| F6.3 | Linux packaging (AppImage, Flatpak) | P1 | Todo | F1.4 |
| F6.4 | Auto-updater | P1 | Todo | F6.1, F6.2 |
| F6.5 | Google Play release | P0 | Todo | F4.2 |
| F6.6 | Homebrew formula for CLI | P2 | Todo | F3.1 |
| F6.7 | Landing page / website | P2 | Todo | - |

### Future (Not Scheduled)

| ID | Feature | Notes |
|----|---------|-------|
| F7.1 | iOS support | Requires $99/year Apple Developer |
| F7.2 | TUI interface (Ratatui) | For terminal enthusiasts |
| F7.3 | Web app | Consider if demand exists |
| F7.4 | Note templates | Quick capture with structure |
| F7.5 | Reminders / scheduled notes | Time-based surfacing |
| F7.6 | Import from other apps | Apple Notes, Google Keep, etc. |
| F7.7 | End-to-end encryption | Privacy-focused users |

---

## Data Model

### Notes Table

```sql
CREATE TABLE notes (
    id TEXT PRIMARY KEY,           -- UUID v7 (sortable, client-generated)
    content TEXT NOT NULL,         -- Plain text content
    created_at INTEGER NOT NULL,   -- Unix timestamp (ms)
    updated_at INTEGER NOT NULL,   -- Unix timestamp (ms)
    is_deleted INTEGER DEFAULT 0,  -- Soft delete for sync
    user_id TEXT NOT NULL          -- For multi-user (future)
);

CREATE INDEX idx_notes_updated ON notes(updated_at);
CREATE INDEX idx_notes_created ON notes(created_at DESC);
CREATE INDEX idx_notes_user ON notes(user_id);
```

### Tags Table (Normalized)

```sql
CREATE TABLE tags (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL
);

CREATE TABLE note_tags (
    note_id TEXT NOT NULL REFERENCES notes(id),
    tag_id TEXT NOT NULL REFERENCES tags(id),
    PRIMARY KEY (note_id, tag_id)
);

CREATE INDEX idx_note_tags_tag ON note_tags(tag_id);
```

### Attachments Table (Phase 5)

```sql
CREATE TABLE attachments (
    id TEXT PRIMARY KEY,
    note_id TEXT NOT NULL REFERENCES notes(id),
    filename TEXT NOT NULL,
    mime_type TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    r2_key TEXT NOT NULL,          -- Cloudflare R2 object key
    created_at INTEGER NOT NULL,
    is_deleted INTEGER DEFAULT 0
);

CREATE INDEX idx_attachments_note ON attachments(note_id);
```

### Settings Table (Local Only)

```sql
CREATE TABLE settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- Example settings:
-- font_family: "JetBrains Mono"
-- font_size: 14
-- theme: "dark"
-- hotkey: "CommandOrControl+Shift+D"
```

---

## Platform Support

| Platform | Phase | UI Framework | Status |
|----------|-------|--------------|--------|
| Windows | 1 | Dioxus | Planned |
| macOS | 1 | Dioxus | Planned |
| Linux | 1 | Dioxus | Planned |
| Android | 4 | Dioxus | Planned |
| iOS | Future | Dioxus | Not scheduled |
| CLI | 3 | clap | Planned |
| TUI | Future | Ratatui | Not scheduled |
| Web | Future | Dioxus | Not scheduled |

---

## Deployment & Distribution

### Desktop

| Platform | Method | Signing | Cost |
|----------|--------|---------|------|
| macOS | DMG direct download | Apple notarization required | $99/year (Apple Dev) |
| Windows | MSI/NSIS installer | MS Trusted Signing | ~$120/year |
| Linux | AppImage + Flatpak | None required | $0 |

### Mobile

| Platform | Method | Cost |
|----------|--------|------|
| Android | Google Play Store | $25 one-time |
| iOS | App Store (future) | $99/year (shared with macOS) |

### CLI

| Method | Platform | Cost |
|--------|----------|------|
| GitHub Releases | All | $0 |
| Homebrew tap | macOS/Linux | $0 |
| crates.io | All | $0 |
| AUR | Arch Linux | $0 |

### Auto-Updates

Desktop apps will use GitHub Releases as the update source with Tauri-style update checking:

1. App checks GitHub API for latest release on startup
2. If newer version exists, prompt user
3. Download and apply update (platform-specific)

### Backend Services

| Service | Tier | Cost | Limits |
|---------|------|------|--------|
| Turso | Free | $0 | Unlimited DBs, 5GB storage, 500M reads, 10M writes |
| Cloudflare R2 | Free | $0 | 10GB storage, 1M writes, 10M reads |

**Total infrastructure cost**: $0/month for hobby use

---

## Design Decisions

### DD-001: Dioxus over Tauri

**Decision**: Use Dioxus for all GUI platforms instead of Tauri.

**Rationale**:
- Full Rust stack enables code sharing between GUI, CLI, and TUI
- Smaller bundles (50KB web, <5MB desktop vs Tauri's 3-4MB)
- Native rendering, not WebView—better performance
- Growing ecosystem with production apps (Huawei, Airbus)

**Trade-offs**:
- Younger ecosystem than Tauri
- Steeper learning curve (Rust UI patterns vs familiar web tech)
- Mobile support still maturing (v0.6)

**Alternatives considered**: Tauri 2.0, Flutter, Electron

---

### DD-002: Turso Embedded Replicas over PowerSync

**Decision**: Use Turso's native embedded replicas for offline-first sync instead of PowerSync.

**Rationale**:
- PowerSync has no official Rust SDK (only JS, Flutter, Kotlin, Swift, .NET)
- PowerSync does not support Turso as a backend database (only PostgreSQL, MongoDB, MySQL)
- Turso's `libsql` crate provides first-class Rust support with API similar to rusqlite
- Embedded replicas provide local SQLite reads (microsecond latency) with automatic cloud sync
- Simpler architecture: no separate sync middleware layer needed
- Same SQLite dialect everywhere (local and cloud)

**Trade-offs**:
- Turso's offline sync is still in public beta (as of Feb 2026)
- Must implement our own conflict resolution (LWW or custom)
- Less battle-tested than PowerSync for complex sync scenarios

**Alternatives considered**: PowerSync (no Rust SDK), Triplit (folded Sept 2025), Electric SQL, custom sync

---

### DD-003: Plain Text Only

**Decision**: No markdown rendering or rich text in v1.

**Rationale**:
- Aligns with "capture velocity" goal—formatting is friction
- Simpler implementation
- Plain text is universally portable
- Can add markdown rendering later without breaking existing notes

**Trade-offs**:
- Power users may want code blocks, lists, etc.
- Less visually appealing for long-form notes

---

### DD-004: Tags via #hashtags in Content

**Decision**: Parse #tags from note content rather than separate tag field.

**Rationale**:
- Zero friction—just type naturally
- Works in CLI: `dirt add "Great idea #work #urgent"`
- No UI required for tag management
- Standard pattern (Twitter, Obsidian, Bear)

**Trade-offs**:
- Tags mixed with content (some prefer separation)
- Need regex parsing on save
- Tag renaming affects content

---

### DD-005: Desktop + Android First

**Decision**: Skip iOS for initial release.

**Rationale**:
- Avoid $99/year Apple Developer fee until app proves value
- Android covers most mobile users
- Desktop is the primary use case (PC-first requirement)
- Can add iOS later if demand exists

**Trade-offs**:
- Excludes iOS-only users
- May lose some potential users

---

### DD-006: Sync from Day One

**Decision**: Implement PowerSync + Turso sync immediately, not as later phase.

**Rationale**:
- Multi-device capture is core to the value proposition
- "Thought on phone, view on desktop" is essential
- Retrofitting sync is harder than building it in

**Trade-offs**:
- More complex initial implementation
- Requires backend services from start
- Authentication needed earlier

---

### DD-007: Client-Generated UUIDs

**Decision**: Use UUID v7 (time-sortable) generated on client.

**Rationale**:
- Works offline—no server roundtrip for ID
- Time-sortable for efficient indexing
- No conflicts between devices
- Standard format, widely supported

**Trade-offs**:
- Slightly larger than auto-increment integers
- Requires UUID library

---

### DD-008: Soft Deletes

**Decision**: Use `is_deleted` flag instead of hard deletes.

**Rationale**:
- Required for reliable sync (delete must propagate)
- Enables "recently deleted" recovery feature
- Audit trail for debugging sync issues

**Trade-offs**:
- Data never truly deleted (privacy consideration)
- Need periodic cleanup job
- Queries must filter `WHERE is_deleted = 0`

---

## Open Questions

### OQ-001: Authentication Provider (Resolved)
**Question**: Which auth service to use?
**Decision**: Supabase Auth
**Resolved on**: 2026-02-07

### OQ-002: Hotkey Conflicts
**Question**: How to handle when user's chosen hotkey conflicts with another app?
**Options**: Detect and warn, allow anyway, require unique
**Decision needed by**: Phase 1 (F1.5)

### OQ-003: Note Size Limits
**Question**: Should we cap note length?
**Options**: Unlimited, 100KB, 1MB
**Decision needed by**: Phase 1

### OQ-004: Export Format
**Question**: What format for note export?
**Options**: Plain text files, JSON, Markdown with frontmatter
**Decision needed by**: Phase 3 (F3.7)

### OQ-005: Tag Autocomplete
**Question**: Should we suggest existing tags as user types `#`?
**Options**: Yes (requires UI work), No (keep simple)
**Decision needed by**: Phase 1 (F1.8)

---

## Appendix: Priority Definitions

| Priority | Meaning |
|----------|---------|
| P0 | Must have for phase completion |
| P1 | Should have, high impact |
| P2 | Nice to have, lower impact |
| P3 | Future consideration |

---

*This document is the source of truth for Dirt's design. Update it as decisions are made.*
