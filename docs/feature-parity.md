# Feature Parity Matrix

Last updated: 2026-02-08

This matrix tracks product parity across active Dirt clients.

| Capability | Desktop (`dirt-desktop`) | CLI (`dirt-cli`) | Mobile (`dirt-mobile`) |
| --- | --- | --- | --- |
| Create note | Yes | Yes | Android shell only (not implemented) |
| List/search notes | Yes | Yes | Android shell only (not implemented) |
| Edit/delete notes | Yes | Yes | Android shell only (not implemented) |
| Quick capture | Yes (global hotkey + tray) | Yes (single-command capture) | Android shell only (not implemented) |
| Settings (theme/font/hotkey) | Yes | N/A | Android shell only (not implemented) |
| Auth + sync status UI | Yes | Partial (`sync` command only) | Android shell only (not implemented) |
| Attachments | Yes | No | Android shell only (not implemented) |
| Export JSON | Yes | Yes | No |
| Export Markdown | Yes | Yes | No |

## Follow-up gaps

- Mobile app needs baseline note CRUD/search UI before feature parity work can start.
- CLI does not support attachments yet.
- CLI has no interactive auth/session UX; it only runs `sync` with env-based credentials.
