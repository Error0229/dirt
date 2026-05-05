//! Note editor view.
//!
//! Single textarea for the full note body. Save runs the appropriate
//! create/update against `MobileNoteStore`, refreshes the list signal
//! so the new state is visible immediately, then kicks the sync worker
//! so the change reaches the server inside the debounce window.
//!
//! Mutations are last-write-wins on the local DB. The sync engine's
//! merge resolver upstream of the API is responsible for cross-device
//! reconciliation; this view only owns the local edit.

use dioxus::prelude::*;

use crate::data::MobileNoteStore;
use crate::state::{AppState, View};
use crate::ui::{ButtonVariant, UiButton, UiTextarea};

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
                    refresh_notes_signal(&store, &mut state).await;
                    state.trigger_sync();
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
                    refresh_notes_signal(&store, &mut state).await;
                    state.trigger_sync();
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
    };

    let header_label = if is_existing { "Edit note" } else { "New note" };

    rsx! {
        div {
            style: "padding: 12px 16px; display: flex; flex-direction: column; gap: 12px; height: 100vh;",

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
                rows: 14,
                oninput: move |event: FormEvent| draft.set(event.value()),
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
    match store.list_notes().await {
        Ok(refreshed) => state.notes.set(refreshed),
        Err(err) => tracing::warn!("Note refresh after mutation failed: {err}"),
    }
}
