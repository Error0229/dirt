//! Note list view.
//!
//! List of the notes in the store in use, most recently edited first,
//! with a line at the top saying how sync is going, a "New note" button,
//! and tap-to-edit on each card.
//! Search and tag filtering land in the M4 follow-up; this view stays
//! deliberately minimal so the shell rewrite can ship without dragging
//! every old feature back at once.

use dioxus::prelude::*;
use dirt_core::models::Note;

use dirt_core::session::SessionState;

use crate::state::{AppState, View};
use crate::ui::{ButtonVariant, UiButton};

const PREVIEW_MAX_CHARS: usize = 140;

#[component]
pub fn List() -> Element {
    let mut state = use_context::<AppState>();
    let notes = (state.notes)();
    let issue = (state.startup_issue)();

    rsx! {
        div {
            style: "padding: 12px 16px; display: flex; flex-direction: column; gap: 12px;",

            SyncBanner {}

            if let Some(message) = issue {
                p {
                    style: "
                        margin: 0;
                        padding: 10px 12px;
                        background: #fee2e2;
                        color: #b91c1c;
                        border-radius: 10px;
                        font-size: 13px;
                        line-height: 1.4;
                    ",
                    "{message}"
                }
            }

            div {
                style: "display: flex; gap: 8px;",
                UiButton {
                    r#type: "button",
                    block: true,
                    variant: ButtonVariant::Primary,
                    onclick: move |_| {
                        state.selected_note_id.set(None);
                        state.view.set(View::Editor);
                    },
                    "New note"
                }
                UiButton {
                    r#type: "button",
                    variant: ButtonVariant::Secondary,
                    onclick: move |_| state.view.set(View::Settings),
                    "Settings"
                }
            }
        }

        div {
            style: "flex: 1; overflow-y: auto; padding: 0 12px 16px 12px;",

            if notes.is_empty() {
                EmptyState {}
            } else {
                for note in notes.iter() {
                    NoteCard { key: "{note.id}", note: note.clone() }
                }
            }
        }
    }
}

/// Says where the notes on screen are kept and how sync is going.
#[component]
fn SyncBanner() -> Element {
    let state = use_context::<AppState>();
    let status = (state.sync_status)();
    let text = match (state.session_state)() {
        SessionState::Ready { .. } => crate::views::settings::sync_failure(&status)
            .unwrap_or_else(|| crate::views::settings::sync_summary(&status)),
        SessionState::WaitingForApproval { .. } => {
            "This device is waiting for approval. Notes written here stay on this device."
                .to_owned()
        }
        SessionState::SignedOut | SessionState::SigningIn => {
            "Not signed in. Notes are kept on this device only.".to_owned()
        }
    };
    rsx! {
        div {
            style: "
                padding: 10px 12px;
                background: #f3f4f6;
                color: #6b7280;
                border-radius: 10px;
                font-size: 13px;
                line-height: 1.4;
            ",
            "{text}"
        }
    }
}

#[component]
fn EmptyState() -> Element {
    rsx! {
        div {
            style: "
                margin-top: 32px;
                padding: 20px;
                background: #ffffff;
                border: 1px solid #e5e7eb;
                border-radius: 12px;
                text-align: center;
                color: #6b7280;
            ",
            "No notes yet. Tap \"New note\" to capture one."
        }
    }
}

#[component]
fn NoteCard(note: Note) -> Element {
    let mut state = use_context::<AppState>();
    let id = note.id;
    let title = note_title(&note);
    let preview = note_preview(&note);

    rsx! {
        button {
            r#type: "button",
            style: "
                display: block;
                width: 100%;
                margin-bottom: 10px;
                padding: 12px;
                border: 1px solid #e5e7eb;
                border-radius: 12px;
                background: #ffffff;
                text-align: left;
                cursor: pointer;
            ",
            onclick: move |_| {
                state.selected_note_id.set(Some(id));
                state.view.set(View::Editor);
            },
            p {
                style: "margin: 0 0 6px 0; font-size: 15px; font-weight: 600; color: #111827;",
                "{title}"
            }
            if !preview.is_empty() {
                p {
                    style: "margin: 0; font-size: 13px; color: #6b7280; line-height: 1.4;",
                    "{preview}"
                }
            }
        }
    }
}

fn note_title(note: &Note) -> String {
    note.content
        .lines()
        .next()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map_or_else(|| "(untitled)".to_string(), str::to_string)
}

fn note_preview(note: &Note) -> String {
    let body: String = note.content.lines().skip(1).collect::<Vec<_>>().join(" ");
    let trimmed = body.trim();
    if trimmed.len() <= PREVIEW_MAX_CHARS {
        trimmed.to_string()
    } else {
        let mut cut = PREVIEW_MAX_CHARS;
        // Walk back to the nearest char boundary so multi-byte
        // characters don't get sliced down the middle.
        while !trimmed.is_char_boundary(cut) {
            cut -= 1;
        }
        format!("{}…", &trimmed[..cut])
    }
}
