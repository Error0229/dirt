//! Note editor view.
//!
//! Single textarea for the full note body. Save runs the appropriate
//! create/update against `MobileNoteStore`, then refreshes the list
//! signal so the new state is visible immediately.
//!
//! Under the text are the note's files. A note that has not been saved yet
//! has none to show, and attaching a file to it saves it first: a file is
//! attached to a note, and there has to be one.

use dioxus::prelude::*;

use crate::data::MobileNoteStore;
use crate::incoming::note_text;
use crate::state::{AppState, View};
use crate::ui::{ButtonVariant, UiButton, UiTextarea};
use crate::views::attachments::{attach, choose_files, AttachmentPanel};

#[component]
pub fn Editor() -> Element {
    let mut state = use_context::<AppState>();
    let selected = (state.selected_note_id)();

    // Seed the draft from the selected note, or start blank for a new
    // note. `use_signal` runs its initializer once per component
    // mount, so navigating list → editor → list → editor does the
    // seeding fresh each time.
    let initial = selected
        .and_then(|id| (state.notes)().into_iter().find(|note| note.id == id))
        .map(|note| note.content)
        .unwrap_or_default();

    let mut draft = use_signal(|| initial);
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    let is_existing = selected.is_some();
    let draft_value = draft();
    let can_save = !draft_value.trim().is_empty() && !busy();

    let save = move |_| {
        if !can_save {
            return;
        }
        busy.set(true);
        error.set(None);

        spawn(async move {
            let Some(store) = (state.store)() else {
                error.set(Some("Local database is not ready yet.".to_string()));
                busy.set(false);
                return;
            };

            let outcome = match selected {
                Some(id) => store.update_note(&id, &draft()).await.map(|_| ()),
                None => store.create_note(&draft()).await.map(|_| ()),
            };

            match outcome {
                Ok(()) => {
                    state.note_changed();
                    refresh_notes_signal(&store, &mut state).await;
                    state.view.set(View::List);
                    state.selected_note_id.set(None);
                }
                Err(err) => {
                    tracing::error!("Failed to save note: {err}");
                    error.set(Some(format!("Save failed: {err}")));
                }
            }

            busy.set(false);
        });
    };

    let delete = move |_| {
        let Some(id) = selected else {
            return;
        };
        busy.set(true);
        error.set(None);

        spawn(async move {
            let Some(store) = (state.store)() else {
                error.set(Some("Local database is not ready yet.".to_string()));
                busy.set(false);
                return;
            };

            match store.delete_note(&id).await {
                Ok(()) => {
                    state.note_changed();
                    refresh_notes_signal(&store, &mut state).await;
                    state.view.set(View::List);
                    state.selected_note_id.set(None);
                }
                Err(err) => {
                    tracing::error!("Failed to delete note: {err}");
                    error.set(Some(format!("Delete failed: {err}")));
                }
            }

            busy.set(false);
        });
    };

    let cancel = move |_| {
        state.view.set(View::List);
        state.selected_note_id.set(None);
        // A note opened from a search was added to the list's notes for
        // the editor to find. The list goes back to what the store holds.
        spawn(async move {
            if let Some(store) = (state.store)() {
                refresh_notes_signal(&store, &mut state).await;
            }
        });
    };

    // A file for a note that is not saved yet. The files are chosen first,
    // so that backing out of choosing saves nothing. Then the note is
    // saved, with its text or, if it has none, with what the files are
    // called; the files are attached; and the editor is on a saved note.
    let attach_to_new = move |_| {
        spawn(async move {
            let Some(files) = choose_files().await else {
                return;
            };
            busy.set(true);
            error.set(None);
            let Some(store) = (state.store)() else {
                crate::incoming::discard_all(&files);
                error.set(Some("Local database is not ready yet.".to_string()));
                busy.set(false);
                return;
            };
            match store.create_note(&note_text(&draft(), &files)).await {
                Ok(note) => {
                    draft.set(note.content.clone());
                    let failed = attach(&state, note.id, files).await;
                    if !failed.is_empty() {
                        error.set(Some(failed.join(" ")));
                    }
                    state.note_changed();
                    refresh_notes_signal(&store, &mut state).await;
                    state.selected_note_id.set(Some(note.id));
                }
                Err(err) => {
                    crate::incoming::discard_all(&files);
                    tracing::error!("Failed to save note: {err}");
                    error.set(Some(format!("Save failed: {err}")));
                }
            }
            busy.set(false);
        });
    };

    let header_label = if is_existing { "Edit note" } else { "New note" };

    rsx! {
        div {
            // At least the height of the screen, with the buttons at the
            // foot of it, and longer when the note's files need the room.
            style: "padding: 12px 16px; display: flex; flex-direction: column; gap: 12px; min-height: 100vh; box-sizing: border-box;",

            div {
                style: "display: flex; align-items: center; justify-content: space-between;",
                h2 { style: "margin: 0; font-size: 16px; font-weight: 600;", "{header_label}" }
                UiButton {
                    r#type: "button",
                    variant: ButtonVariant::Ghost,
                    onclick: cancel,
                    disabled: busy(),
                    "Cancel"
                }
            }

            UiTextarea {
                value: "{draft_value}",
                placeholder: "What's on your mind?",
                rows: 10,
                oninput: move |event: FormEvent| draft.set(event.value()),
            }

            if let Some(note_id) = selected {
                // One panel for each note, made afresh for another.
                for note_id in [note_id] {
                    AttachmentPanel { key: "{note_id}", note_id }
                }
            } else {
                div {
                    UiButton {
                        r#type: "button",
                        variant: ButtonVariant::Outline,
                        disabled: busy(),
                        onclick: attach_to_new,
                        "Attach a file"
                    }
                }
            }

            if let Some(message) = error() {
                p {
                    style: "
                        margin: 0;
                        padding: 8px 10px;
                        background: #fee2e2;
                        color: #b91c1c;
                        border-radius: 8px;
                        font-size: 13px;
                    ",
                    "{message}"
                }
            }

            div {
                style: "display: flex; gap: 8px; margin-top: auto;",
                UiButton {
                    r#type: "button",
                    block: true,
                    variant: ButtonVariant::Primary,
                    disabled: !can_save,
                    onclick: save,
                    if busy() { "Saving..." } else { "Save" }
                }
                if is_existing {
                    UiButton {
                        r#type: "button",
                        variant: ButtonVariant::Danger,
                        disabled: busy(),
                        onclick: delete,
                        "Delete"
                    }
                }
            }
        }
    }
}

async fn refresh_notes_signal(store: &MobileNoteStore, state: &mut AppState) {
    let listed = *state.listed_notes.peek();
    match store.list_notes(listed).await {
        Ok(refreshed) => state.notes.set(refreshed),
        Err(err) => tracing::warn!("Note refresh after mutation failed: {err}"),
    }
}
