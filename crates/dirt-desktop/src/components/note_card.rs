//! Note card component

use dioxus::prelude::*;

use super::card::{Card, CardContent};
use crate::state::AppState;

/// A single note row rendered in the note list.
#[component]
pub fn NoteCard(
    title: String,
    preview: String,
    is_selected: bool,
    onclick: EventHandler<MouseEvent>,
) -> Element {
    let state = use_context::<AppState>();
    let colors = (state.theme)().palette();

    let bg = if is_selected {
        colors.bg_tertiary
    } else {
        colors.bg_primary
    };
    let border_left = if is_selected {
        format!("3px solid {}", colors.accent)
    } else {
        "3px solid transparent".to_string()
    };

    rsx! {
        div {
            class: if is_selected { "note-item selected" } else { "note-item" },
            style: "
                border-bottom: 1px solid {colors.border_light};
                border-left: {border_left};
                cursor: pointer;
                background: {bg};
                transition: background 0.15s;
            ",
            onclick: move |evt| onclick.call(evt),

            Card {
                style: "
                padding: 0;
                gap: 0;
                border: none;
                border-radius: 0;
                box-shadow: none;
            ",

                CardContent {
                    style: "
                        padding: 12px 16px;
                    ",

                    div {
                        class: "note-title",
                        style: "
                            font-weight: 500;
                            margin-bottom: 4px;
                            overflow: hidden;
                            text-overflow: ellipsis;
                            white-space: nowrap;
                            color: {colors.text_primary};
                        ",
                        "{title}"
                    }

                    div {
                        class: "note-preview",
                        style: "
                            font-size: 12px;
                            color: {colors.text_secondary};
                            overflow: hidden;
                            text-overflow: ellipsis;
                            white-space: nowrap;
                        ",
                        "{preview}"
                    }
                }
            }
        }
    }
}
