//! Quick capture floating window component

use dioxus::desktop::{window, LogicalSize, WindowBuilder};
use dioxus::prelude::*;

use super::button::{Button, ButtonVariant};
use crate::services::DatabaseService;
use crate::theme::{is_system_dark_mode, ResolvedTheme};

/// Standalone quick capture window - opens as a floating dialog
/// This component is designed to work independently without shared context
#[component]
pub fn QuickCaptureWindow() -> Element {
    let mut content = use_signal(String::new);
    let mut is_saving = use_signal(|| false);
    let mut db_service: Signal<Option<DatabaseService>> = use_signal(|| None);
    let mut db_initialized = use_signal(|| false);

    // Detect system theme for standalone window
    let theme = if is_system_dark_mode() {
        ResolvedTheme::Dark
    } else {
        ResolvedTheme::Light
    };
    let theme_attr = match theme {
        ResolvedTheme::Light => "light",
        ResolvedTheme::Dark => "dark",
    };

    // Initialize database asynchronously
    use_future(move || async move {
        if db_initialized() {
            return;
        }

        match DatabaseService::new().await {
            Ok(db) => {
                db_service.set(Some(db));
                db_initialized.set(true);
                tracing::debug!("Quick capture database initialized");
            }
            Err(e) => {
                tracing::error!("Quick capture: Failed to init database: {}", e);
                db_initialized.set(true);
            }
        }
    });

    let save_and_close = move |_| {
        let text = content.read().trim().to_string();
        if !text.is_empty() && !*is_saving.read() {
            is_saving.set(true);
            let db = db_service.read().clone();
            spawn(async move {
                if let Some(db) = db {
                    match db.create_note(&text).await {
                        Ok(note) => {
                            tracing::info!("Quick captured note: {}", note.id);
                        }
                        Err(e) => {
                            tracing::error!("Failed to create note: {}", e);
                        }
                    }
                }
                // Close this window
                window().close();
            });
        } else if text.is_empty() {
            window().close();
        }
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
                let db = db_service.read().clone();
                spawn(async move {
                    if let Some(db) = db {
                        match db.create_note(&text).await {
                            Ok(note) => {
                                tracing::info!("Quick captured note: {}", note.id);
                            }
                            Err(e) => {
                                tracing::error!("Failed to create note: {}", e);
                            }
                        }
                    }
                    window().close();
                });
            } else if text.is_empty() {
                window().close();
            }
        }
    };

    rsx! {
        // Load theme CSS
        document::Link { rel: "stylesheet", href: asset!("/assets/dx-components-theme.css") }
        document::Link { rel: "stylesheet", href: asset!("/assets/theme-overrides.css") }

        div {
            "data-theme": "{theme_attr}",
            class: "quick-capture-container",
            style: "
                width: 100%;
                height: 100%;
                background: var(--primary-color);
                color: var(--secondary-color-4);
                padding: 16px;
                box-sizing: border-box;
                font-family: system-ui, -apple-system, sans-serif;
                display: flex;
                flex-direction: column;
            ",

            h3 {
                style: "margin: 0 0 12px 0; font-size: 13px; color: var(--secondary-color-5); font-weight: 500;",
                "Quick Capture"
            }

            textarea {
                class: "input",
                style: "
                    flex: 1;
                    width: 100%;
                    border: 1px solid var(--primary-color-6);
                    border-radius: 8px;
                    padding: 12px;
                    font-size: 14px;
                    resize: none;
                    outline: none;
                    font-family: inherit;
                    box-sizing: border-box;
                    background: var(--primary-color-1);
                    color: var(--secondary-color-4);
                ",
                value: "{content}",
                placeholder: "Capture a thought... (Ctrl+Enter to save, Esc to cancel)",
                autofocus: true,
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
