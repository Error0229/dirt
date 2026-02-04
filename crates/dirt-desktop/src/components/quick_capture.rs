//! Quick capture overlay component

use dioxus::prelude::*;

use crate::state::AppState;

/// Quick capture overlay for rapid note entry
#[component]
pub fn QuickCapture(on_close: EventHandler<()>) -> Element {
    let mut state = use_context::<AppState>();
    let mut content = use_signal(String::new);

    let save_and_close = move |_: Event<MouseData>| {
        let text = content.read().trim().to_string();
        if !text.is_empty() {
            if let Some(ref db) = *state.db_service.read() {
                match db.create_note(&text) {
                    Ok(note) => {
                        tracing::info!("Quick captured note: {}", note.id);
                        let mut notes = state.notes.write();
                        notes.insert(0, note);
                    }
                    Err(e) => {
                        tracing::error!("Failed to create note: {}", e);
                    }
                }
            }
        }
        on_close.call(());
    };

    let cancel = move |_: Event<MouseData>| {
        on_close.call(());
    };

    let handle_keydown = move |evt: Event<KeyboardData>| {
        // Escape to close without saving
        if evt.key() == Key::Escape {
            on_close.call(());
        }
        // Ctrl/Cmd+Enter to save
        if evt.key() == Key::Enter && (evt.modifiers().meta() || evt.modifiers().ctrl()) {
            // Need to trigger save manually here
            let text = content.read().trim().to_string();
            if !text.is_empty() {
                if let Some(ref db) = *state.db_service.read() {
                    match db.create_note(&text) {
                        Ok(note) => {
                            tracing::info!("Quick captured note: {}", note.id);
                            let mut notes = state.notes.write();
                            notes.insert(0, note);
                        }
                        Err(e) => {
                            tracing::error!("Failed to create note: {}", e);
                        }
                    }
                }
            }
            on_close.call(());
        }
    };

    rsx! {
        div {
            class: "quick-capture-overlay",
            style: "position: fixed; top: 0; left: 0; right: 0; bottom: 0; background: rgba(0,0,0,0.5); display: flex; align-items: center; justify-content: center; z-index: 1000;",
            onclick: cancel,

            div {
                class: "quick-capture-window",
                style: "background: white; border-radius: 12px; padding: 20px; width: 500px; max-width: 90%; box-shadow: 0 20px 60px rgba(0,0,0,0.3);",
                onclick: move |evt| evt.stop_propagation(),

                h3 {
                    style: "margin: 0 0 12px 0; font-size: 14px; color: #666; font-weight: 500;",
                    "Quick Capture"
                }

                textarea {
                    class: "quick-capture-input",
                    style: "width: 100%; height: 120px; border: 1px solid #e0e0e0; border-radius: 8px; padding: 12px; font-size: 14px; resize: none; outline: none; font-family: inherit;",
                    value: "{content}",
                    placeholder: "Capture a thought... (Ctrl+Enter to save)",
                    autofocus: true,
                    oninput: move |evt| content.set(evt.value()),
                    onkeydown: handle_keydown,
                }

                div {
                    class: "quick-capture-actions",
                    style: "display: flex; justify-content: flex-end; gap: 8px; margin-top: 12px;",

                    button {
                        style: "padding: 8px 16px; border: 1px solid #e0e0e0; border-radius: 6px; background: white; cursor: pointer;",
                        onclick: cancel,
                        "Cancel"
                    }

                    button {
                        style: "padding: 8px 16px; border: none; border-radius: 6px; background: #4f46e5; color: white; cursor: pointer;",
                        onclick: save_and_close,
                        "Save"
                    }
                }
            }
        }
    }
}
