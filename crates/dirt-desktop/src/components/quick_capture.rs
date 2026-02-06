//! Quick capture component
//!
//! When triggered via global hotkey, the main window resizes to a compact
//! capture box (420x200). This component fills that window entirely.
//! On save/cancel it restores the original window size and hides to tray.

use dioxus::desktop::{window, LogicalPosition, LogicalSize};
use dioxus::prelude::*;

use super::button::{Button, ButtonVariant};
use crate::queries::invalidate_notes_query;
use crate::state::AppState;

/// Hide window first, then restore original geometry while invisible
fn hide_and_restore(state: &mut AppState) {
    let win = window();
    // Hide immediately so the user never sees the resize
    win.set_visible(false);
    if let Some((w, h, x, y)) = (state.saved_window_geometry)() {
        let tao_win = &win.window;
        tao_win.set_inner_size(LogicalSize::new(w, h));
        tao_win.set_outer_position(LogicalPosition::new(x, y));
    }
    state.saved_window_geometry.set(None);
}

/// Quick capture — fills the entire (resized) window
#[component]
pub fn QuickCapture() -> Element {
    let mut state = use_context::<AppState>();
    let mut content = use_signal(String::new);
    let mut is_saving = use_signal(|| false);

    let colors = (state.theme)().palette();

    let mut close = move || {
        content.set(String::new());
        state.quick_capture_open.set(false);
        hide_and_restore(&mut state);
    };

    let save_and_close = move |_| {
        let text = content.read().trim().to_string();
        if text.is_empty() {
            close();
            return;
        }
        if *is_saving.read() {
            return;
        }
        is_saving.set(true);
        let db = state.db_service.read().clone();
        spawn(async move {
            if let Some(db) = db {
                match db.create_note(&text).await {
                    Ok(note) => {
                        tracing::info!("Quick captured note: {}", note.id);
                        invalidate_notes_query().await;
                    }
                    Err(e) => {
                        tracing::error!("Failed to create note: {}", e);
                    }
                }
            }
            is_saving.set(false);
            content.set(String::new());
            state.quick_capture_open.set(false);
            hide_and_restore(&mut state);
        });
    };

    let cancel = move |_: MouseEvent| {
        close();
    };

    let handle_keydown = move |evt: Event<KeyboardData>| {
        if evt.key() == Key::Escape {
            close();
            return;
        }
        if evt.key() == Key::Enter && (evt.modifiers().meta() || evt.modifiers().ctrl()) {
            let text = content.read().trim().to_string();
            if text.is_empty() {
                close();
                return;
            }
            if *is_saving.read() {
                return;
            }
            is_saving.set(true);
            let db = state.db_service.read().clone();
            spawn(async move {
                if let Some(db) = db {
                    match db.create_note(&text).await {
                        Ok(note) => {
                            tracing::info!("Quick captured note: {}", note.id);
                            invalidate_notes_query().await;
                        }
                        Err(e) => {
                            tracing::error!("Failed to create note: {}", e);
                        }
                    }
                }
                is_saving.set(false);
                content.set(String::new());
                state.quick_capture_open.set(false);
                hide_and_restore(&mut state);
            });
        }
    };

    rsx! {
        div {
            style: "
                width: 100%;
                height: 100%;
                background: {colors.bg_primary};
                color: {colors.text_primary};
                padding: 16px;
                box-sizing: border-box;
                font-family: system-ui, -apple-system, sans-serif;
                display: flex;
                flex-direction: column;
            ",

            h3 {
                style: "margin: 0 0 12px 0; font-size: 13px; color: {colors.text_secondary}; font-weight: 500;",
                "Quick Capture"
            }

            textarea {
                class: "input",
                style: "
                    flex: 1;
                    width: 100%;
                    border: 1px solid {colors.border};
                    border-radius: 8px;
                    padding: 12px;
                    font-size: 14px;
                    resize: none;
                    outline: none;
                    font-family: inherit;
                    box-sizing: border-box;
                    background: {colors.bg_secondary};
                    color: {colors.text_primary};
                ",
                value: "{content}",
                placeholder: "Capture a thought... (Ctrl+Enter to save, Esc to cancel)",
                autofocus: true,
                onmounted: move |evt: MountedEvent| async move {
                    _ = evt.set_focus(true).await;
                },
                oninput: move |evt| content.set(evt.value()),
                onkeydown: handle_keydown,
            }

            div {
                style: "display: flex; justify-content: flex-end; gap: 8px; margin-top: 12px;",

                Button {
                    variant: ButtonVariant::Secondary,
                    onclick: cancel,
                    style: "padding: 5px 12px; font-size: 13px;",
                    "Cancel"
                }

                Button {
                    variant: ButtonVariant::Primary,
                    onclick: save_and_close,
                    style: "padding: 5px 12px; font-size: 13px;",
                    "Save"
                }
            }
        }
    }
}
