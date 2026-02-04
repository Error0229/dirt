//! Quick capture floating window component

use dioxus::desktop::{window, LogicalSize, WindowBuilder};
use dioxus::prelude::*;

use crate::services::DatabaseService;

/// Standalone quick capture window - opens as a floating dialog
/// This component is designed to work independently without shared context
#[component]
pub fn QuickCaptureWindow() -> Element {
    let mut content = use_signal(String::new);
    let mut is_saving = use_signal(|| false);

    // Initialize database directly (not shared with main window)
    let db = use_signal(|| {
        DatabaseService::new()
            .map_err(|e| tracing::error!("Quick capture: Failed to init database: {}", e))
            .ok()
    });

    let save_and_close = move |_| {
        let text = content.read().trim().to_string();
        if !text.is_empty() && !*is_saving.read() {
            is_saving.set(true);
            if let Some(ref db) = *db.read() {
                match db.create_note(&text) {
                    Ok(note) => {
                        tracing::info!("Quick captured note: {}", note.id);
                    }
                    Err(e) => {
                        tracing::error!("Failed to create note: {}", e);
                    }
                }
            }
        }
        // Close this window
        window().close();
    };

    let cancel = move |_| {
        window().close();
    };

    let handle_keydown = move |evt: Event<KeyboardData>| {
        // Escape to close without saving
        if evt.key() == Key::Escape {
            window().close();
        }
        // Ctrl/Cmd+Enter to save
        if evt.key() == Key::Enter && (evt.modifiers().meta() || evt.modifiers().ctrl()) {
            let text = content.read().trim().to_string();
            if !text.is_empty() && !*is_saving.read() {
                is_saving.set(true);
                if let Some(ref db) = *db.read() {
                    match db.create_note(&text) {
                        Ok(note) => {
                            tracing::info!("Quick captured note: {}", note.id);
                        }
                        Err(e) => {
                            tracing::error!("Failed to create note: {}", e);
                        }
                    }
                }
            }
            window().close();
        }
    };

    rsx! {
        div {
            style: "
                width: 100%;
                height: 100%;
                background: #ffffff;
                padding: 16px;
                box-sizing: border-box;
                font-family: system-ui, -apple-system, sans-serif;
                display: flex;
                flex-direction: column;
            ",

            h3 {
                style: "margin: 0 0 12px 0; font-size: 13px; color: #666; font-weight: 500;",
                "⚡ Quick Capture"
            }

            textarea {
                style: "
                    flex: 1;
                    width: 100%;
                    border: 1px solid #e0e0e0;
                    border-radius: 8px;
                    padding: 12px;
                    font-size: 14px;
                    resize: none;
                    outline: none;
                    font-family: inherit;
                    box-sizing: border-box;
                ",
                value: "{content}",
                placeholder: "Capture a thought... (Ctrl+Enter to save, Esc to cancel)",
                autofocus: true,
                oninput: move |evt| content.set(evt.value()),
                onkeydown: handle_keydown,
            }

            div {
                style: "display: flex; justify-content: flex-end; gap: 8px; margin-top: 12px;",

                button {
                    style: "padding: 6px 14px; border: 1px solid #e0e0e0; border-radius: 6px; background: white; cursor: pointer; font-size: 13px;",
                    onclick: cancel,
                    "Cancel"
                }

                button {
                    style: "padding: 6px 14px; border: none; border-radius: 6px; background: #4f46e5; color: white; cursor: pointer; font-size: 13px;",
                    onclick: save_and_close,
                    "Save"
                }
            }
        }
    }
}

/// Opens the quick capture floating window
pub fn open_quick_capture_window() {
    let cfg = dioxus::desktop::Config::new().with_window(
        WindowBuilder::new()
            .with_title("Quick Capture")
            .with_inner_size(LogicalSize::new(420.0, 200.0))
            .with_resizable(false)
            .with_always_on_top(true)
            .with_decorations(true)
            .with_focused(true),
    );

    let dom = VirtualDom::new(QuickCaptureWindow);
    window().new_window(dom, cfg);
}
