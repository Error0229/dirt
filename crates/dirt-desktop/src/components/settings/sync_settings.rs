use dioxus::prelude::*;

use super::row::SettingRow;
use crate::state::SyncStatus;

#[component]
pub(super) fn SyncSettingsTab(
    sync_status: SyncStatus,
    sync_issue: Option<String>,
    pending_sync_count: usize,
    pending_sync_preview: String,
) -> Element {
    rsx! {
        SettingRow {
            label: "Sync Health",
            description: "Current cloud sync runtime status",

            div {
                class: "auth-panel",
                div {
                    class: "auth-hint",
                    "Status: {sync_status_label(sync_status)}"
                }
                if let Some(issue) = sync_issue {
                    div {
                        class: "auth-error",
                        "{issue}"
                    }
                }
            }
        }

        SettingRow {
            label: "Offline Queue",
            description: "Pending local changes waiting for sync",

            div {
                class: "auth-panel",
                div {
                    class: "auth-hint",
                    "Pending changes: {pending_sync_count}"
                }
                if pending_sync_count > 0 {
                    div {
                        class: "auth-hint",
                        "Pending note IDs: {pending_sync_preview}"
                    }
                }
            }
        }
    }
}

const fn sync_status_label(status: SyncStatus) -> &'static str {
    match status {
        SyncStatus::Synced => "Synced",
        SyncStatus::Syncing => "Syncing",
        SyncStatus::Offline => "Offline",
        SyncStatus::Error => "Error",
    }
}
