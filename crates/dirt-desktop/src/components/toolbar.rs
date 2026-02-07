//! Toolbar component with actions

use chrono::Utc;
use dioxus::prelude::*;

use super::button::{Button, ButtonVariant};
use super::create_note_optimistic;
use crate::queries::invalidate_notes_query;
use crate::state::{AppState, SyncStatus};

/// Toolbar with action buttons
#[component]
pub fn Toolbar() -> Element {
    let mut state = use_context::<AppState>();
    let has_selected_note = state.current_note().is_some();
    let sync_status = (state.sync_status)();
    let last_sync_at = (state.last_sync_at)();

    let sync_status_text = format_sync_status_text(sync_status, last_sync_at);
    let sync_status_class = sync_status_class(sync_status);

    let create_note = move |_| {
        create_note_optimistic(&mut state);
    };

    let delete_note = move |_| {
        let note_id = *state.current_note_id.read();
        if let Some(id) = note_id {
            // Update UI immediately (optimistic)
            state.notes.write().retain(|n| n.id != id);
            state.current_note_id.set(None);

            tracing::info!("Deleted note (optimistic): {}", id);

            // Persist in background
            let db = state.db_service.read().clone();
            spawn(async move {
                if let Some(db) = db {
                    if let Err(e) = db.delete_note(&id).await {
                        tracing::error!("Failed to persist delete: {}", e);
                    } else {
                        // Invalidate query to sync state
                        invalidate_notes_query().await;
                    }
                }
            });
        }
    };

    let open_settings = move |_| {
        state.settings_open.set(true);
    };

    rsx! {
        div {
            class: "toolbar",

            Button {
                variant: ButtonVariant::Primary,
                onclick: create_note,
                "+ New Note"
            }

            if has_selected_note {
                Button {
                    variant: ButtonVariant::Destructive,
                    onclick: delete_note,
                    "Delete"
                }
            }

            // Spacer
            div { style: "flex: 1;" }

            div {
                class: "sync-indicator {sync_status_class}",
                title: "{sync_status_text}",
                span { class: "sync-dot" }
                span { class: "sync-label", "{sync_status_text}" }
            }

            // Settings button
            Button {
                variant: ButtonVariant::Secondary,
                onclick: open_settings,
                "Settings"
            }
        }
    }
}

const fn sync_status_class(status: SyncStatus) -> &'static str {
    match status {
        SyncStatus::Synced => "sync-synced",
        SyncStatus::Syncing => "sync-syncing",
        SyncStatus::Offline => "sync-offline",
        SyncStatus::Error => "sync-error",
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
