//! Compact note card component

use dioxus::prelude::*;

use dirt_core::NoteId;

use super::delete_note_optimistic;
use crate::state::AppState;
use crate::time_format::format_short_time;

/// Compact 48px note card
#[component]
pub fn NoteCard(
    note_id: NoteId,
    title: String,
    preview: String,
    updated_at_ms: i64,
    is_selected: bool,
    onclick: EventHandler<MouseEvent>,
) -> Element {
    let mut state = use_context::<AppState>();
    let colors = (state.theme)().palette();
    let mut hovered = use_signal(|| false);
    let relative_time = format_short_time(updated_at_ms);
    let is_hovered = hovered();

    let bg = if is_selected {
        colors.bg_tertiary
    } else {
        "transparent"
    };

    let left_border = if is_selected {
        format!("3px solid {}", colors.accent)
    } else {
        "3px solid transparent".to_string()
    };

    let delete_note = move |evt: MouseEvent| {
        evt.stop_propagation();
        delete_note_optimistic(&mut state, note_id);
    };

    rsx! {
        div {
            class: "note-card",
            style: "
                padding: 8px 10px;
                border-left: {left_border};
                border-bottom: 1px solid {colors.border};
                background: {bg};
                cursor: pointer;
                overflow: hidden;
                transition: background 0.08s;
                display: flex;
                flex-direction: column;
                justify-content: center;
            ",
            onclick: move |evt| onclick.call(evt),
            onmouseenter: move |_| hovered.set(true),
            onmouseleave: move |_| hovered.set(false),

            // Row 1: title + time/delete
            div {
                style: "
                    display: flex;
                    align-items: center;
                    gap: 6px;
                    min-width: 0;
                ",

                // Title
                span {
                    style: "
                        flex: 1;
                        font-size: 13px;
                        font-weight: 500;
                        color: {colors.text_primary};
                        overflow: hidden;
                        text-overflow: ellipsis;
                        white-space: nowrap;
                        min-width: 0;
                    ",
                    if title.is_empty() { "Untitled" } else { "{title}" }
                }

                // Delete icon on hover, timestamp otherwise
                if is_hovered {
                    button {
                        style: "
                            background: none;
                            border: none;
                            cursor: pointer;
                            color: {colors.error};
                            font-size: 13px;
                            padding: 0 2px;
                            line-height: 1;
                            flex-shrink: 0;
                            opacity: 0.7;
                        ",
                        onclick: delete_note,
                        "×"
                    }
                } else {
                    span {
                        style: "
                            font-size: 11px;
                            color: {colors.text_muted};
                            white-space: nowrap;
                            flex-shrink: 0;
                        ",
                        "{relative_time}"
                    }
                }
            }

            // Row 2: preview/tags — only if non-empty
            if !preview.is_empty() {
                div {
                    style: "
                        font-size: 11px;
                        color: {colors.text_secondary};
                        overflow: hidden;
                        text-overflow: ellipsis;
                        white-space: nowrap;
                        margin-top: 1px;
                        line-height: 1.2;
                    ",
                    "{preview}"
                }
            }
        }
    }
}
