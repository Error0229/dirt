---
date: 2026-02-04
topic: dirt-app-initial-design
---

# Dirt - Quick Thought Capture App

"Do I Remember That?" - A cross-platform app for capturing fleeting thoughts with zero friction.

## What We're Building

A minimalist note-taking app optimized for **capture velocity**—the time from "I had a thought" to "it's safely recorded" should be under 2 seconds. Plain text first, media in roadmap.

### Core Capture Flows
1. **Global hotkey → floating window** (desktop) - Press combo anywhere, type, dismiss
2. **System tray → click to capture** (desktop) - Discoverable fallback
3. **CLI: `dirt add "thought"`** (terminal) - For developers already in terminal
4. **Mobile quick action** (Android) - Home screen widget or notification shade

### Retrieval
- Optional `#tags` for organization (not required)
- Full-text search
- Chronological default view

### Editor
- Plain text only (no markdown, no rich text)
- Global config for font, size, theme
- Focus on speed, not formatting

## Tech Stack

| Layer | Technology | Rationale |
|-------|------------|-----------|
| Desktop/Mobile UI | Dioxus | Native Rust, tiny bundles, code sharing with CLI |
| CLI/TUI | Ratatui | Pure Rust, shares data layer with GUI |
| Local Storage | SQLite | Proven, portable, offline-first foundation |
| Sync Layer | PowerSync | Production-ready offline-first, free tier |
| Backend DB | Turso | Edge SQLite, embedded replicas, generous free tier |
| Media Storage | Cloudflare R2 | Zero egress fees (future: images, audio, files) |

## Key Decisions

1. **Dioxus over Tauri**: Full Rust stack enables code sharing between GUI and CLI/TUI. Smaller bundles. Steeper learning curve but unified codebase.

2. **PowerSync over Triplit**: Triplit folded as a company (Sept 2025). PowerSync is enterprise-backed, production-proven.

3. **Sync from day one**: Multi-device capture is core to the use case. Can't compromise on this.

4. **Plain text only**: Aligns with "dirt simple" philosophy. Formatting adds complexity without serving the capture-velocity goal.

5. **Desktop + Android first**: Skip iOS initially to avoid $99/year Apple fee until app proves value. Linux desktop is free.

6. **Text-first, media in roadmap**: Ship core experience fast. Add image paste, voice memos, file attachments incrementally.

## Platform Targets (v1)

- [x] Windows
- [x] macOS
- [x] Linux
- [x] Android
- [ ] iOS (roadmap)
- [x] CLI (all platforms)
- [ ] TUI (roadmap, lower priority)

## Data Model (Initial)

```sql
CREATE TABLE notes (
    id TEXT PRIMARY KEY,           -- UUID, generated client-side
    content TEXT NOT NULL,
    tags TEXT,                     -- JSON array of tag strings
    created_at INTEGER NOT NULL,   -- Unix timestamp
    updated_at INTEGER NOT NULL,
    is_deleted INTEGER DEFAULT 0   -- Soft delete for sync
);

CREATE INDEX idx_notes_updated ON notes(updated_at);
CREATE INDEX idx_notes_created ON notes(created_at DESC);
```

## Open Questions

1. **Hotkey conflict handling**: What if the user's chosen hotkey conflicts with another app?
2. **Tag autocomplete**: Should we suggest existing tags as user types `#`?
3. **Note size limits**: Cap at some length, or truly unlimited?
4. **Export format**: Plain text files? JSON? Markdown with frontmatter?

## Deployment Costs (Annual)

| Item | Cost |
|------|------|
| Apple Developer (when iOS added) | $99 |
| Windows code signing | ~$120 |
| Google Play (one-time) | $25 |
| PowerSync free tier | $0 |
| Turso free tier | $0 |
| Cloudflare R2 free tier | $0 |
| **Total (without iOS)** | **~$145** |

## Next Steps

→ `/workflows:plan` for implementation details
