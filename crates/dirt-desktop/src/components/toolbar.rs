//! Compact unified toolbar with integrated search

use dioxus::prelude::*;

use super::button::{Button, ButtonVariant};
use super::create_note_optimistic;
use crate::state::{AppState, SyncStatus};
use crate::time_format::format_relative_time;

/// Compact 44px toolbar: hamburger, search, new note, sync dot, settings
#[component]
pub fn Toolbar() -> Element {
    let mut state = use_context::<AppState>();
    let colors = (state.theme)().palette();
    let sync_status = (state.sync_status)();
    let last_sync_at = (state.last_sync_at)();
    let note_list_visible = (state.note_list_visible)();

    let sync_dot_color = match sync_status {
        SyncStatus::Synced => colors.success,
        SyncStatus::Syncing => colors.warning,
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
            style: "-webkit-app-region: drag;",

            // Hamburger — toggle note list
            Button {
                variant: ButtonVariant::Ghost,
                style: "
                    color: {colors.text_secondary};
                    -webkit-app-region: no-drag;
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
                    font-size: 22px;
                    color: {colors.accent};
                    -webkit-app-region: no-drag;
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
                    color: {colors.text_secondary};
                    -webkit-app-region: no-drag;
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
