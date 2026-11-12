//! Settings → Sync tab: how sync is going, and the sync button.
//!
//! Sync runs when the app starts, when its window regains focus, after a
//! note changes, and when this button is pressed. Nothing syncs on a timer,
//! so an app left open and untouched shows what it had when it last synced.
//!
//! What is shown, and in which words, comes from `dirt-core`'s sync
//! overview, which the Android app and `dirt sync --verbose` show too.

use dioxus::prelude::*;
use dirt_core::session::SessionState;
use dirt_core::store::NoteProblem;
use dirt_core::sync::overview::SyncOverview;
use dirt_core::sync::worker::Trigger;
use dirt_core::util::{as_sentence, now_ms};

use super::account_settings::session_of;
use super::row::SettingRow;
use crate::components::button::{Button, ButtonVariant};
use crate::state::AppState;

#[component]
pub(super) fn SyncSettingsTab() -> Element {
    let state = use_context::<AppState>();
    let session = session_of(&state);
    let session_state = (state.session_state)();
    let mut overview = use_signal(|| None::<SyncOverview>);

    // The counts and the notes that could not be synced are in the store;
    // read them again whenever sync reports something new.
    use_effect(move || {
        let _ = (state.sync_status)();
        let _ = (state.session_state)();
        let session = session_of(&state);
        spawn(async move {
            let read = match session {
                Some(session) => session.sync_overview().await.ok(),
                None => None,
            };
            overview.set(read);
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

    // Until the store has been read, the worker's own figures stand in.
    let status = (state.sync_status)();
    let shown = overview().unwrap_or_else(|| SyncOverview::from(&status));
    let last_synced = shown.last_synced(now_ms());
    let waiting = shown.waiting();
    let failure = shown
        .failure
        .as_ref()
        .map(dirt_core::sync::worker::SyncFailure::advice);
    let problem_count = shown.problem_count();

    rsx! {
        SettingRow {
            label: "Sync",
            description: "Notes sync when you open or return to Dirt, after you change one, and when you press the button",

            div { class: "auth-panel",
                div { class: "auth-status", "{last_synced}" }
                if let Some(text) = waiting {
                    div { class: "auth-hint", "{text}" }
                }
                if let Some(text) = failure {
                    div { class: "auth-error", "{text}" }
                }
                if let Some(text) = problem_count {
                    div { class: "auth-error", "{text}" }
                }
                for problem in shown.problems {
                    ProblemNote { key: "{problem.note_id}", problem }
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

/// One note that could not be synced: which it is, why, and what releases
/// it. A note that is on this device opens when its name is pressed.
#[component]
fn ProblemNote(problem: NoteProblem) -> Element {
    let mut state = use_context::<AppState>();
    let id = problem.note_id;
    let title = problem.title();
    let reason = as_sentence(&problem.reason);
    let release = problem.release();
    // Only a note in the list can be opened: not one this device never
    // held, and not a deleted one.
    let on_this_device = (state.notes)().iter().any(|note| note.id == id);

    rsx! {
        div { class: "sync-problem",
            if on_this_device {
                button {
                    class: "sync-problem-title sync-problem-open",
                    r#type: "button",
                    title: "Open this note",
                    onclick: move |_| {
                        // The note must not be hidden by a search or a tag.
                        state.search_query.set(String::new());
                        state.active_tag_filter.set(None);
                        state.current_note_id.set(Some(id));
                        state.settings_open.set(false);
                    },
                    "{title}"
                }
            } else {
                div { class: "sync-problem-title", "{title}" }
            }
            div { class: "auth-hint", "{reason}" }
            div { class: "auth-hint", "{release}" }
        }
    }
}
