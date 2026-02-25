use dioxus::prelude::*;

use super::row::SettingRow;
use crate::components::button::{Button, ButtonVariant};

#[derive(Clone, PartialEq, Eq)]
pub(super) struct SyncConflictView {
    pub id: i64,
    pub note_id: String,
    pub resolved_at: String,
    pub details: String,
}

#[component]
pub(super) fn SyncSettingsTab(
    pending_sync_count: usize,
    pending_sync_preview: String,
    sync_conflicts: Vec<SyncConflictView>,
    sync_conflicts_loading: bool,
    sync_conflicts_error: Option<String>,
    on_refresh_sync_conflicts: EventHandler<MouseEvent>,
) -> Element {
    rsx! {
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

        SettingRow {
            label: "Sync Conflicts",
            description: "Recent LWW conflict resolutions",

            div {
                class: "auth-panel",
                div {
                    class: "auth-actions",
                    Button {
                        variant: ButtonVariant::Secondary,
                        disabled: sync_conflicts_loading,
                        onclick: move |event| on_refresh_sync_conflicts.call(event),
                        "Refresh"
                    }
                }

                if sync_conflicts_loading {
                    div {
                        class: "auth-message",
                        "Loading recent conflicts..."
                    }
                } else if let Some(error) = sync_conflicts_error {
                    div {
                        class: "auth-error",
                        "{error}"
                    }
                } else if sync_conflicts.is_empty() {
                    div {
                        class: "auth-hint",
                        "No sync conflicts recorded yet."
                    }
                } else {
                    div {
                        style: "display: flex; flex-direction: column; gap: 8px;",
                        for conflict in sync_conflicts {
                            div {
                                key: "{conflict.id}",
                                style: "padding: 8px; border: 1px solid #37415133; border-radius: 8px;",
                                div {
                                    style: "font-size: 12px; font-weight: 600;",
                                    "Note {conflict.note_id}"
                                }
                                div {
                                    style: "font-size: 11px; opacity: 0.9;",
                                    "Resolved: {conflict.resolved_at}"
                                }
                                div {
                                    style: "font-size: 11px; opacity: 0.9;",
                                    "{conflict.details}"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
