//! Settings → Sync tab: how sync is going, and the sync button.
//!
//! Sync runs when the app starts, when its window regains focus, after a
//! note changes, and when this button is pressed. Nothing syncs on a timer,
//! so an app left open and untouched shows what it had when it last synced.

use dioxus::prelude::*;
use dirt_core::api::ErrorClass;
use dirt_core::session::SessionState;
use dirt_core::store::NoteProblem;
use dirt_core::sync::worker::Trigger;

use super::account_settings::session_of;
use super::row::SettingRow;
use crate::components::button::{Button, ButtonVariant};
use crate::state::AppState;
use crate::time_format::format_short_time;

#[component]
pub(super) fn SyncSettingsTab() -> Element {
    let state = use_context::<AppState>();
    let session = session_of(&state);
    let session_state = (state.session_state)();
    let status = (state.sync_status)();
    let mut problems = use_signal(Vec::<NoteProblem>::new);

    // The reasons are in the store; read them again whenever a sync ends.
    use_effect(move || {
        let _ = (state.sync_status)();
        let store = state.db_service.read().clone();
        spawn(async move {
            let list = match store {
                Some(store) => store.note_problems().await.unwrap_or_default(),
                None => Vec::new(),
            };
            problems.set(list);
        });
    });

    let Some(session) = session else {
        return rsx! {
            div { class: "settings-row",
                div { class: "auth-hint",
                    "This build of Dirt was made without a server to sync with. Notes are kept on this device only."
                }
            }
        };
    };
    if !session_state.is_ready() {
        let text = match session_state {
            SessionState::WaitingForApproval { .. } => {
                "This device is waiting for approval and does not sync yet."
            }
            _ => "Sign in on the Account tab to sync your notes between devices.",
        };
        return rsx! {
            div { class: "settings-row",
                div { class: "auth-hint", "{text}" }
            }
        };
    }

    let last_synced = status.last_synced_ms.map_or_else(
        || "Never synced".to_owned(),
        |at| match format_short_time(at).as_str() {
            "now" => "Synced just now".to_owned(),
            ago => format!("Synced {ago} ago"),
        },
    );
    let failure = status.failure.as_ref().map(|failure| match failure.class {
        ErrorClass::TryAgainLater => {
            "The server could not be reached. Your notes are saved and will be sent later."
                .to_owned()
        }
        ErrorClass::StopUntilSignedIn => {
            "This device has to sign in again. See the Account tab.".to_owned()
        }
        ErrorClass::UpdateTheApp => {
            "This version of Dirt is too old to sync. Update the app.".to_owned()
        }
        ErrorClass::ProblemWithOneNote => failure.message.clone(),
    });

    rsx! {
        SettingRow {
            label: "Sync",
            description: "Notes sync when you open or return to Dirt, after you change one, and when you press the button",

            div { class: "auth-panel",
                div { class: "auth-status",
                    if status.syncing { "Syncing..." } else { "{last_synced}" }
                }
                if status.unsent > 0 {
                    div { class: "auth-hint", "{status.unsent} note(s) waiting to be sent" }
                }
                if let Some(text) = failure {
                    div { class: "auth-error", "{text}" }
                }
                for problem in problems() {
                    div { class: "auth-error",
                        "A note is not syncing: {problem.reason}."
                    }
                }
                div { class: "auth-actions",
                    Button {
                        variant: ButtonVariant::Secondary,
                        disabled: status.syncing,
                        onclick: move |_| session.notify(Trigger::Manual),
                        "Sync now"
                    }
                }
            }
        }
    }
}
