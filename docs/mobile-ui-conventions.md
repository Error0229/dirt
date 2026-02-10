# Mobile UI Component Conventions

This project uses the official Dioxus component ecosystem for mobile UI behavior.

## Rules

1. Prefer `dioxus_primitives` components when a primitive exists (`ScrollArea`, `Separator`, `Label`, `Toast`, etc).
2. For baseline HTML controls (`button`, `input`, `textarea`), use shared wrappers in `crates/dirt-mobile/src/ui.rs`:
   - `UiButton`
   - `UiInput`
   - `UiTextarea`
3. Avoid introducing raw control tags directly in `app.rs`; route new controls through shared wrappers for consistent interaction and styling.
4. Keep one shared style source (`MOBILE_UI_STYLES`) for those wrappers instead of repeating inline style blocks.

## Why

- Reduces style drift across list/editor/settings flows.
- Keeps behavior consistent with official Dioxus component patterns.
- Makes future mobile UI changes easier to review and maintain.
