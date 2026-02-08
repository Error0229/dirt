//! Note card component

use dioxus::prelude::*;

use super::button::{Button, ButtonVariant};
use super::card::{Card, CardContent};
use crate::state::AppState;

const MINUTE_MS: i64 = 60_000;
const HOUR_MS: i64 = 60 * MINUTE_MS;
const DAY_MS: i64 = 24 * HOUR_MS;
const WEEK_MS: i64 = 7 * DAY_MS;

fn format_relative_time(timestamp_ms: i64) -> String {
    let now_ms = chrono::Utc::now().timestamp_millis();
    let delta = (now_ms - timestamp_ms).max(0);

    if delta < MINUTE_MS {
        return "just now".to_string();
    }
    if delta < HOUR_MS {
        let minutes = delta / MINUTE_MS;
        return if minutes == 1 {
            "1 minute ago".to_string()
        } else {
            format!("{minutes} minutes ago")
        };
    }
    if delta < DAY_MS {
        let hours = delta / HOUR_MS;
        return if hours == 1 {
            "1 hour ago".to_string()
        } else {
            format!("{hours} hours ago")
        };
    }
    if delta < WEEK_MS {
        let days = delta / DAY_MS;
        return if days == 1 {
            "1 day ago".to_string()
        } else {
            format!("{days} days ago")
        };
    }

    let weeks = delta / WEEK_MS;
    if weeks == 1 {
        "1 week ago".to_string()
    } else {
        format!("{weeks} weeks ago")
    }
}

/// A single note row rendered in the note list.
#[component]
pub fn NoteCard(
    title: String,
    preview: String,
    updated_at_ms: i64,
    is_selected: bool,
    onclick: EventHandler<MouseEvent>,
) -> Element {
    let state = use_context::<AppState>();
    let colors = (state.theme)().palette();
    let relative_time = format_relative_time(updated_at_ms);

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
        Button {
            variant: ButtonVariant::Ghost,
            class: if is_selected { "note-item selected" } else { "note-item" },
            style: "
                width: 100%;
                padding: 0;
                border-bottom: 1px solid {colors.border_light};
                border-left: {border_left};
                background: {bg};
                transition: background 0.15s;
                border-radius: 0;
                text-align: left;
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

                    div {
                        class: "note-timestamp",
                        style: "
                            margin-top: 4px;
                            font-size: 11px;
                            color: {colors.text_muted};
                            white-space: nowrap;
                        ",
                        "{relative_time}"
                    }
                }
            }
        }
    }
}
