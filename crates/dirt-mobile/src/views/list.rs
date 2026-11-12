//! Note list view.
//!
//! List of the notes in the store in use, most recently edited first,
//! with a line at the top saying how sync is going, a "New note" button,
//! a search box, a row of tags, and tap-to-edit on each card.
//!
//! The list holds the most recently edited notes. Searching and narrowing
//! to a tag ask the store, which looks through every note, so an old note
//! is found though the list does not hold it.
//!
//! A hundred notes are shown at first. A button under the last one shows a
//! hundred more, so every note can be reached by reading down the list as
//! well as by searching.

use dioxus::prelude::*;
use dirt_core::models::Note;

use dirt_core::session::SessionState;

use crate::data::NOTES_PAGE;
use crate::state::{AppState, View};
use crate::ui::{ButtonVariant, UiButton, UiInput};

const PREVIEW_MAX_CHARS: usize = 140;

#[component]
pub fn List() -> Element {
    let mut state = use_context::<AppState>();
    let notes = (state.notes)();
    let issue = (state.startup_issue)();
    let query = (state.search_query)();
    let active_tag = (state.tag_filter)();
    let narrowed = !query.trim().is_empty() || active_tag.is_some();
    let listed = (state.listed_notes)();

    // Both come from the store, and are read again when the notes change.
    let tags = use_resource(move || {
        let store = (state.store)();
        let _ = (state.notes)();
        async move {
            match store {
                Some(store) => store.list_tags().await.unwrap_or_default(),
                None => Vec::new(),
            }
        }
    });
    let found = use_resource(move || {
        let store = (state.store)();
        let query = (state.search_query)();
        let tag = (state.tag_filter)();
        let limit = (state.listed_notes)();
        let _ = (state.notes)();
        async move {
            if query.trim().is_empty() && tag.is_none() {
                return None;
            }
            store?.find_notes(&query, tag.as_deref(), limit).await.ok()
        }
    });
    // A tag can stop being in use while the list is narrowed to it: its
    // last note is deleted, or edited to drop the tag. The row of tags,
    // "All" included, is gone then, so the list widens by itself. It would
    // otherwise go on showing nothing, with no way to change that.
    use_effect(move || {
        let Some(in_use) = tags() else {
            return;
        };
        let stale = state
            .tag_filter
            .peek()
            .as_ref()
            .is_some_and(|active| !in_use.iter().any(|(tag, _)| tag == active));
        if stale {
            state.tag_filter.set(None);
        }
    });
    let tags = tags().unwrap_or_default();
    // `None` while the store has not answered yet.
    let shown: Option<Vec<Note>> = if narrowed {
        found().flatten()
    } else {
        Some(notes.clone())
    };
    // The store is not asked how many notes there are, or how many a
    // search matches. A list as long as it was allowed to be is taken to
    // have more below it.
    let more = shown.as_ref().is_some_and(|shown| shown.len() >= listed);
    let more_label = if narrowed {
        "Show more notes"
    } else {
        "Show older notes"
    };

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

            if !notes.is_empty() || narrowed {
                UiInput {
                    r#type: "search",
                    placeholder: "Search notes",
                    value: "{query}",
                    oninput: move |event: FormEvent| state.search_query.set(event.value()),
                }
            }

            if !tags.is_empty() {
                div {
                    style: "display: flex; gap: 6px; overflow-x: auto; padding-bottom: 2px;",
                    TagChip { label: "All".to_owned(), tag: None }
                    for (tag, count) in tags {
                        TagChip {
                            key: "{tag}",
                            label: format!("#{tag} {count}"),
                            tag: Some(tag),
                        }
                    }
                }
            }
        }

        div {
            style: "flex: 1; overflow-y: auto; padding: 0 12px 16px 12px;",

            match shown {
                Some(shown) if shown.is_empty() && narrowed => rsx! {
                    Notice { text: "No notes match." }
                },
                Some(shown) if shown.is_empty() => rsx! { EmptyState {} },
                Some(shown) => rsx! {
                    for note in shown {
                        NoteCard { key: "{note.id}", note }
                    }
                    if more {
                        UiButton {
                            r#type: "button",
                            block: true,
                            variant: ButtonVariant::Secondary,
                            onclick: move |_| {
                                let longer = *state.listed_notes.peek() + NOTES_PAGE;
                                state.listed_notes.set(longer);
                                // A search is made again by itself, because
                                // it reads the number. The list is read here.
                                spawn(async move {
                                    let store = state.store.peek().clone();
                                    if let Some(store) = store {
                                        if let Ok(notes) = store.list_notes(longer).await {
                                            state.notes.set(notes);
                                        }
                                    }
                                });
                            },
                            "{more_label}"
                        }
                    }
                },
                None => rsx! {},
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
        SessionState::Ready { .. } => {
            let overview = dirt_core::sync::overview::SyncOverview::from(&status);
            overview.failure.as_ref().map_or_else(
                || overview.last_synced(dirt_core::util::now_ms()),
                dirt_core::sync::worker::SyncFailure::advice,
            )
        }
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
        Notice { text: "No notes yet. Tap \"New note\" to capture one." }
    }
}

/// A line that stands where the notes would be.
#[component]
fn Notice(text: String) -> Element {
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
            "{text}"
        }
    }
}

/// One tag in the row above the list. Pressing it narrows the list to that
/// tag; pressing "All", or the tag again, shows every note.
#[component]
fn TagChip(label: String, tag: Option<String>) -> Element {
    let mut state = use_context::<AppState>();
    let active = (state.tag_filter)() == tag;
    let (background, colour, border) = if active {
        ("#111827", "#ffffff", "#111827")
    } else {
        ("#ffffff", "#374151", "#d1d5db")
    };
    rsx! {
        button {
            r#type: "button",
            style: "
                flex-shrink: 0;
                padding: 6px 10px;
                border: 1px solid {border};
                border-radius: 999px;
                background: {background};
                color: {colour};
                font-size: 13px;
                white-space: nowrap;
            ",
            onclick: move |_| {
                let next = if active { None } else { tag.clone() };
                state.tag_filter.set(next);
            },
            "{label}"
        }
    }
}

#[component]
fn NoteCard(note: Note) -> Element {
    let mut state = use_context::<AppState>();
    let id = note.id;
    let title = note_title(&note);
    let preview = note_preview(&note);
    // The editor opens a note from the list's notes. A note that was found
    // by searching may not be among them, so it is added when it is opened.
    let found = note;

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
                let listed = state.notes.peek().iter().any(|note| note.id == id);
                if !listed {
                    state.notes.write().push(found.clone());
                }
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
