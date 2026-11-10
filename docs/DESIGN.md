# Dirt: product design

> "Do I Remember That?" A cross-platform app for capturing fleeting thoughts.

This document holds what does not change with the architecture: the problem,
the principles, and the standing product decisions.

- **Architecture and its decisions:**
  [plans/2026-11-10-system-redesign-design.md](plans/2026-11-10-system-redesign-design.md)
- **Build order:**
  [plans/2026-11-10-refactor-system-redesign-implementation-plan.md](plans/2026-11-10-refactor-system-redesign-implementation-plan.md)
- **Earlier design, roadmap and decision records:**
  [archive/DESIGN-2026-02.md](archive/DESIGN-2026-02.md)

## Vision

### Core problem

Ideas flash in the mind and disappear. The friction of opening an app, finding
the right note, and typing kills most thoughts before they are captured.

### Solution

**Capture velocity** is the only metric that matters. Time from "I had a
thought" to "it's safely recorded" should be **under 2 seconds**.

### Guiding principles

1. **Speed over features.** Every feature must justify its impact on capture
   latency.
2. **Plain text first.** Formatting is a distraction from capture.
3. **Offline-first.** Never lose a thought because of the network.
4. **Cross-platform parity.** The same experience everywhere.
5. **No lock-in.** Notes are plain text, exportable at any time.

### Non-goals

- Bi-directional linking. This is not a knowledge-management tool.
- Rich text editing.
- Real-time collaboration or live typing between devices.
- Complex organisational hierarchies.
- Server-side features that need to read notes, such as server search. Notes
  are end-to-end encrypted in the redesign.

## Platforms in scope

| Platform | Client |
|---|---|
| Windows, macOS, Linux | Desktop app (Dioxus) |
| Android | Mobile app (Dioxus) |
| Any terminal | CLI (clap) |

iOS, a web app and a terminal interface are not in scope.

## Standing product decisions

These come from the original design and still hold. Their full rationale is in
the archived document, under the numbers given.

| Decision | Summary | Original record |
|---|---|---|
| Dioxus for every graphical client | One Rust codebase shares models and logic across desktop, Android and CLI | DD-001 |
| Plain text only | No markdown rendering or rich text | DD-003 |
| Tags are `#hashtags` in the note text | Nothing to manage; works from the CLI | DD-004 |
| Desktop and Android first | No iOS until the app proves its value | DD-005 |
| IDs are UUID v7, generated on the client | Works offline and sorts by time | DD-007 |

These original decisions are superseded by the redesign:

| Original decision | What replaced it |
|---|---|
| DD-002: Turso embedded replicas for sync | Encrypted push and pull through `dirt-api`, with a CRDT per note |
| DD-008: soft deletes with an `is_deleted` flag | A deletion marker inside each note's CRDT document |
| DD-009: backend-issued short-lived credentials | A device token per device, with keys held only on devices |
