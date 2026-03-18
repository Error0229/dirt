//! Compact note card component

use dioxus::prelude::*;

use dirt_core::NoteId;

use crate::queries::invalidate_notes_query;
use crate::state::AppState;

const MINUTE_MS: i64 = 60_000;
const HOUR_MS: i64 = 60 * MINUTE_MS;
const DAY_MS: i64 = 24 * HOUR_MS;
const WEEK_MS: i64 = 7 * DAY_MS;

fn format_short_time(timestamp_ms: i64) -> String {
    let now_ms = chrono::Utc::now().timestamp_millis();
    let delta = (now_ms - timestamp_ms).max(0);

    if delta < MINUTE_MS {
        return "now".to_string();
    }
    if delta < HOUR_MS {
        return format!("{}m", delta / MINUTE_MS);
    }
    if delta < DAY_MS {
        return format!("{}h", delta / HOUR_MS);
    }
    if delta < WEEK_MS {
        return format!("{}d", delta / DAY_MS);
    }
    format!("{}w", delta / WEEK_MS)
}

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
        let id = note_id;
        state.notes.write().retain(|n| n.id != id);
        if (state.current_note_id)() == Some(id) {
            state.current_note_id.set(None);
        }
        state.enqueue_pending_change(id);

        let db = state.db_service.read().clone();
        spawn(async move {
            if let Some(db) = db {
                if let Err(e) = db.delete_note(&id).await {
                    tracing::error!("Failed to persist delete: {}", e);
                } else {
                    invalidate_notes_query().await;
                }
            }
        });
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
