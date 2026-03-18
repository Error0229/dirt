//! Compact unified toolbar with integrated search

use chrono::Utc;
use dioxus::prelude::*;

use super::button::{Button, ButtonVariant};
use super::create_note_optimistic;
use crate::state::{AppState, SyncStatus};

/// Compact 36px toolbar: hamburger, search, new note, sync dot, settings
#[component]
pub fn Toolbar() -> Element {
    let mut state = use_context::<AppState>();
    let colors = (state.theme)().palette();
    let sync_status = (state.sync_status)();
    let last_sync_at = (state.last_sync_at)();
    let note_list_visible = (state.note_list_visible)();

    let sync_dot_color = match sync_status {
        SyncStatus::Synced => colors.success,
        SyncStatus::Syncing => "#c4a95c",
        SyncStatus::Offline => colors.text_muted,
        SyncStatus::Error => colors.error,
    };
    let sync_title = format_sync_status_text(sync_status, last_sync_at);

    let toggle_list = move |_| {
        state.note_list_visible.set(!note_list_visible);
    };

    let create_note = move |_| {
        create_note_optimistic(&mut state);
    };

    let open_settings = move |_| {
        state.settings_open.set(true);
    };

    rsx! {
        div {
            class: "toolbar",
            style: "
                height: 44px;
                min-height: 44px;
                display: flex;
                align-items: center;
                gap: 8px;
                padding: 0 12px;
                background: {colors.bg_secondary};
                border-bottom: 1px solid {colors.border};
                -webkit-app-region: drag;
            ",

            // Hamburger — toggle note list
            Button {
                variant: ButtonVariant::Ghost,
                style: "
                    width: 36px; height: 36px;
                    padding: 0;
                    display: flex; align-items: center; justify-content: center;
                    font-size: 18px;
                    color: {colors.text_secondary};
                    -webkit-app-region: no-drag;
                    flex-shrink: 0;
                    border-radius: 6px;
                ",
                onclick: toggle_list,
                if note_list_visible { "◧" } else { "☰" }
            }

            // Search input — flex: 1
            input {
                r#type: "text",
                placeholder: "Search notes...",
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
                    width: 36px; height: 36px;
                    padding: 0;
                    display: flex; align-items: center; justify-content: center;
                    font-size: 22px;
                    color: {colors.accent};
                    -webkit-app-region: no-drag;
                    flex-shrink: 0;
                    border-radius: 6px;
                ",
                onclick: create_note,
                "+"
            }

            // Sync dot
            div {
                title: "{sync_title}",
                style: "
                    width: 10px; height: 10px;
                    border-radius: 50%;
                    background: {sync_dot_color};
                    flex-shrink: 0;
                    -webkit-app-region: no-drag;
                ",
            }

            // Settings
            Button {
                variant: ButtonVariant::Ghost,
                style: "
                    width: 36px; height: 36px;
                    padding: 0;
                    display: flex; align-items: center; justify-content: center;
                    font-size: 18px;
                    color: {colors.text_secondary};
                    -webkit-app-region: no-drag;
                    flex-shrink: 0;
                ",
                onclick: open_settings,
                "⚙"
            }
        }
    }
}

fn format_sync_status_text(status: SyncStatus, last_sync_at: Option<i64>) -> String {
    match status {
        SyncStatus::Synced => last_sync_at.map_or_else(
            || "Synced".to_string(),
            |timestamp| format!("Synced {}", format_relative_time(timestamp)),
        ),
        SyncStatus::Syncing => "Syncing...".to_string(),
        SyncStatus::Offline => "Offline".to_string(),
        SyncStatus::Error => "Sync error".to_string(),
    }
}

fn format_relative_time(timestamp_ms: i64) -> String {
    let now = Utc::now().timestamp_millis();
    let diff = now.saturating_sub(timestamp_ms);
    let minute = 60_000;
    let hour = 60 * minute;
    let day = 24 * hour;

    if diff < minute {
        "just now".to_string()
    } else if diff < hour {
        format!("{}m ago", diff / minute)
    } else if diff < day {
        format!("{}h ago", diff / hour)
    } else {
        format!("{}d ago", diff / day)
    }
}
