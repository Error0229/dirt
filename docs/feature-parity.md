# Feature Parity Matrix

Last updated: 2026-02-10

This matrix tracks product parity across active Dirt clients.

| Capability | Desktop (`dirt-desktop`) | CLI (`dirt-cli`) | Mobile (`dirt-mobile`) |
| --- | --- | --- | --- |
| Create note | Yes | Yes | Yes |
| List notes | Yes | Yes | Yes |
| Search notes | Yes | Yes | No (`#117`) |
| Tag filtering | Yes | Yes (`dirt list --tag`) | No (`#117`) |
| Edit/delete notes | Yes | Yes | Yes |
| Quick capture | Yes (global hotkey + tray) | Yes (`dirt add ...`) | Partial (widget-style entry intent parsed in app; native Android widget wiring pending `#119`) |
| Share-intent capture | N/A | N/A | Partial (app-side payload support exists; native Android share-sheet integration pending `#119`) |
| Settings (theme/font/hotkey) | Yes | N/A | Partial (sync/auth/runtime settings and diagnostics available; no theme/font/hotkey parity) |
| Auth + sync status UI | Yes | Partial (`sync` command only, env-driven) | Yes (Supabase auth/session controls + sync diagnostics/status) |
| Attachments | Yes | No | Partial (metadata list only; add/open/delete UX pending `#118`) |
| Export JSON | Yes | Yes | No (`#120`) |
| Export Markdown | Yes | Yes | No (`#120`) |

## Follow-up gaps

- Mobile search and tag-filter parity: `#117`
- Mobile attachment UX parity (picker/open/delete): `#118`
- Android-native share-intent and widget launch plumbing: `#119`
- Mobile JSON/Markdown export parity: `#120`
- CLI still has no attachment workflow.
