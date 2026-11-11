//! Compact unified toolbar with integrated search

use dioxus::prelude::*;

use super::button::{Button, ButtonVariant};
use super::create_note;
use crate::state::AppState;

/// Compact 44px toolbar: hamburger, search, new note, settings
#[component]
pub fn Toolbar() -> Element {
    let mut state = use_context::<AppState>();
    let colors = (state.theme)().palette();
    let note_list_visible = (state.note_list_visible)();

    let toggle_list = move |_| {
        state.note_list_visible.set(!note_list_visible);
    };

    let create_note = move |_| {
        create_note(&state);
    };

    let open_settings = move |_| {
        state.settings_open.set(true);
    };

    rsx! {
        div { class: "toolbar", style: "-webkit-app-region: drag;",

            // Hamburger — toggle note list
            Button {
                variant: ButtonVariant::Ghost,
                style: "
                    color: {colors.text_secondary};
                    -webkit-app-region: no-drag;
                ",
                onclick: toggle_list,
                if note_list_visible {
                    "◧"
                } else {
                    "☰"
                }
            }

            // Search input — flex: 1
            input {
                r#type: "text",
                placeholder: "...",
                value: "{state.search_query}",
                style: "
                    flex: 1;
                    height: 34px;
                    padding: 0 12px;
                    border: 1px solid {colors.border};
                    border-radius: 8px;
                    background: {colors.bg_primary};
                    color: {colors.text_primary};
                    font-size: 14px;
                    outline: none;
                    -webkit-app-region: no-drag;
                    min-width: 80px;
                ",
                oninput: move |evt: FormEvent| {
                    state.search_query.set(evt.value());
                },
            }

            // New note button
            Button {
                variant: ButtonVariant::Ghost,
                style: "
                    font-size: 22px;
                    color: {colors.accent};
                    -webkit-app-region: no-drag;
                ",
                onclick: create_note,
                "+"
            }

            // Settings
            Button {
                variant: ButtonVariant::Ghost,
                style: "
                    color: {colors.text_secondary};
                    -webkit-app-region: no-drag;
                ",
                onclick: open_settings,
                "⚙"
            }
        }
    }
}
