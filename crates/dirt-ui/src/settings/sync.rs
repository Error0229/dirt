//! Settings → Sync (interface design, sections 6.6, 9.4 and 9.5): how
//! sync stands, **Sync now**, and each note or file that cannot sync, with
//! why and what releases it. Pressing such a note closes Settings and
//! opens it.
//!
//! Every word here comes from `dirt_core::sync::overview`, which the other
//! app and the CLI show too.

use dioxus::prelude::*;
use dirt_core::session::SessionState;
use dirt_core::store::{FileProblem, NoteProblem};
use dirt_core::sync::overview::{
    SyncIndicator, SyncOverview, JUST_SYNCED_MS, NOT_SYNCING_SIGNED_OUT, NOT_SYNCING_UNTIL_LET_IN,
};
use dirt_core::sync::worker::Trigger;
use dirt_core::util::{as_sentence, now_ms};
use dirt_core::NoteId;

use super::rows::{Card, Row, RowButton, RowIcon};
use super::{settings_nav, Linked};
use crate::appearance::Platform;
use crate::components::IconButton;
use crate::icons::Icon;

#[component]
pub(super) fn Sync(linked: Linked, open_note: EventHandler<NoteId>, platform: Platform) -> Element {
    let nav = settings_nav();
    let mut overview = use_signal(|| None::<SyncOverview>);
    // The notes that cannot sync are in the store: read again whenever
    // sync says something new, while Settings is open.
    let _read = use_resource(move || {
        let _ = linked.status.read();
        let state = *linked.state.read();
        let open = nav.is_open();
        let session = linked.session.read().clone();
        async move {
            if !open || !state.is_ready() {
                return;
            }
            if let Some(session) = session {
                match session.sync_overview().await {
                    Ok(read) => overview.set(Some(read)),
                    Err(error) => tracing::warn!("Could not read how sync stands: {error}"),
                }
            }
        }
    });

    let state = *linked.state.read();
    let not_syncing = match state {
        _ if linked.session.read().is_none() => Some(NOT_SYNCING_SIGNED_OUT),
        SessionState::SignedOut | SessionState::SigningIn => Some(NOT_SYNCING_SIGNED_OUT),
        SessionState::WaitingForApproval { .. } => Some(NOT_SYNCING_UNTIL_LET_IN),
        SessionState::Ready { .. } => None,
    };
    if let Some(text) = not_syncing {
        return rsx! {
            Card { Row { icon: Icon::Laptop, title: text } }
        };
    }

    let status = linked.status.read().clone();
    let now = now_ms();
    // Until the store has been read, the worker's own figures stand in;
    // the worker's figures are newer for everything but the lists.
    let mut shown = SyncOverview::from(&status);
    if let Some(read) = overview.read().as_ref() {
        shown.problems.clone_from(&read.problems);
        shown.file_problems.clone_from(&read.file_problems);
    }
    // The worker counts the problems at every change; the lists may not
    // have been read yet. "Just synced" lasts a moment and this section
    // is not drawn again when it ends, so its icon is what comes after.
    let indicator = SyncIndicator::from_status(&status, now + JUST_SYNCED_MS);
    let tone = match indicator {
        SyncIndicator::Problem { .. } => "warn",
        SyncIndicator::Synced { .. } => "ok",
        _ => "",
    };
    let icon = match indicator {
        SyncIndicator::Problem { .. } => Icon::Alert,
        SyncIndicator::Offline { .. } => Icon::CloudOff,
        SyncIndicator::Syncing => Icon::Refresh,
        _ => Icon::CloudCheck,
    };
    let headline = shown.headline();
    let last = shown.last_synced_in_full(now);
    let key_change = shown.key_change();

    rsx! {
        Card {
            div { class: "dui-set-row",
                RowIcon { icon, tone }
                div { class: "dui-set-words",
                    div { class: "dui-set-t", role: "status", "{headline}" }
                    div { class: "dui-set-s", "{last}" }
                }
                IconButton {
                    icon: Icon::Refresh,
                    label: "Sync now",
                    shortcut: (platform == Platform::Desktop).then(|| "Ctrl R".to_owned()),
                    disabled: status.syncing,
                    onclick: move |_| {
                        if let Some(session) = linked.session() {
                            session.notify(Trigger::Manual);
                        }
                    },
                }
            }
            if let Some(text) = key_change {
                Row { icon: Icon::Key, title: "The key is being changed", detail: text }
            }
            for problem in shown.problems {
                ProblemNote { key: "{problem.note_id}", problem, open_note }
            }
            for problem in shown.file_problems {
                ProblemFile { key: "{problem.file_id}-{problem.note_id}", problem, open_note }
            }
        }
    }
}

/// A note that cannot sync. One this device holds opens when pressed.
#[component]
fn ProblemNote(problem: NoteProblem, open_note: EventHandler<NoteId>) -> Element {
    let nav = settings_nav();
    let id = problem.note_id;
    let detail = format!("{} {}", as_sentence(&problem.reason), problem.release());
    let title = format!("“{}”", problem.title());
    if problem.text.is_none() {
        return rsx! {
            div { class: "dui-set-row dui-set-problem",
                RowIcon { icon: Icon::Alert, tone: "warn" }
                div { class: "dui-set-words",
                    div { class: "dui-set-t", "{title}" }
                    div { class: "dui-set-s", "{detail}" }
                }
            }
        };
    }
    rsx! {
        RowButton {
            icon: Icon::Alert,
            tone: "warn",
            title,
            detail,
            opens: true,
            onclick: move |_| {
                nav.close();
                open_note.call(id);
            },
        }
    }
}

/// A file the server refused. Pressing it opens the note it is on.
#[component]
fn ProblemFile(problem: FileProblem, open_note: EventHandler<NoteId>) -> Element {
    let nav = settings_nav();
    let id = problem.note_id;
    rsx! {
        RowButton {
            icon: Icon::Alert,
            tone: "warn",
            title: problem.title(),
            detail: format!("{} {}", as_sentence(&problem.reason), problem.release()),
            opens: true,
            onclick: move |_| {
                nav.close();
                open_note.call(id);
            },
        }
    }
}
