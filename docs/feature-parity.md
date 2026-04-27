# Feature Parity Matrix

Tracks product parity across active Dirt clients during Phase 1.

| Capability | Desktop (`dirt-desktop`) | CLI (`dirt-cli`) | Mobile (`dirt-mobile`) |
| --- | --- | --- | --- |
| Create note | Yes | Yes | Pending — shell rewrite for Phase 1 |
| List notes | Yes | Yes | Pending |
| Search notes | Yes | Yes | Pending (`#117`) |
| Tag filtering | Yes | Yes (`dirt list --tag`) | Pending (`#117`) |
| Edit/delete notes | Yes | Yes | Pending |
| Quick capture | Yes (global hotkey + tray) | Yes (`dirt add ...`) | Pending — native Android widget (`#119`) |
| Share-intent capture | N/A | N/A | Pending — Android share-sheet (`#119`) |
| Settings (theme/font/hotkey) | Yes | N/A | Pending |
| Sync status UI | Yes (toolbar dot + Settings → Sync tab) | Partial (`dirt sync` exit code + stdout summary) | Pending |
| Auto-sync (background) | Yes (startup + 30 s timer + post-mutation, with exponential backoff on errors) | No (manual `dirt sync` only) | Pending |
| Attachments | Pending — UI deferred until R2 backend lands | No | Pending |
| Export JSON | Yes | Yes | Pending (`#120`) |
| Export Markdown | Yes | Yes | Pending (`#120`) |

## Phase 1 deferred work

- **Mobile shell rewrite.** The Android `app_shell.rs` is
  `cfg(target_os = "android")`-only and does not compile against the
  post-Supabase `dirt-core` shape. The mobile sync worker rebuild is
  the next milestone.
- **Attachments UI.** The desktop `AttachmentPanel` was removed in
  the Supabase teardown because it depended on `MediaApiClient` /
  `auth_session` / `dirt_core::media::*`, all of which were deleted
  with R2 support. Reintroducing attachments is a separate later PR.
- **Mobile parity tickets:** search/tag filter (`#117`), attachments
  (`#118`), Android share-intent and widget plumbing (`#119`),
  JSON/Markdown export (`#120`).
- **CLI**: still has no attachment workflow. Will follow whatever
  shape the desktop attachments PR lands.
