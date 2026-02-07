use std::sync::Arc;
use std::time::Duration;

use dioxus::prelude::*;
use dioxus_primitives::scroll_area::{ScrollArea, ScrollDirection, ScrollType};
use dioxus_primitives::separator::Separator;
use dioxus_primitives::toast::{use_toast, ToastOptions, ToastProvider};
use dirt_core::{Note, NoteId};

use crate::data::MobileNoteStore;
use crate::launch::{LaunchIntent, QuickCaptureLaunch};

#[derive(Clone, Copy, PartialEq, Eq)]
enum MobileView {
    List,
    Editor,
    QuickCapture,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MobileSyncState {
    Offline,
    Syncing,
    Synced,
    Error,
}

#[component]
pub fn App() -> Element {
    rsx! {
        ToastProvider {
            AppShell {}
        }
    }
}

#[component]
fn AppShell() -> Element {
    let mut store = use_signal(|| None::<Arc<MobileNoteStore>>);
    let mut notes = use_signal(Vec::<Note>::new);
    let mut selected_note_id = use_signal(|| None::<NoteId>);
    let mut draft_content = use_signal(String::new);
    let mut quick_capture_content = use_signal(String::new);
    let mut view = use_signal(|| MobileView::List);
    let mut status_message = use_signal(|| None::<String>);
    let mut loading = use_signal(|| true);
    let mut saving = use_signal(|| false);
    let mut deleting = use_signal(|| false);
    let mut sync_state = use_signal(|| MobileSyncState::Offline);
    let mut last_sync_at = use_signal(|| None::<i64>);
    let launch: Signal<LaunchIntent> = use_signal(crate::launch::detect_launch_intent_from_runtime);
    let toasts = use_toast();

    use_future(move || async move {
        let launch = launch();

        match MobileNoteStore::open_default().await {
            Ok(note_store) => {
                let note_store = Arc::new(note_store);

                store.set(Some(note_store.clone()));

                if note_store.is_sync_enabled().await {
                    sync_state.set(MobileSyncState::Syncing);
                    match note_store.sync().await {
                        Ok(()) => {
                            sync_state.set(MobileSyncState::Synced);
                            last_sync_at.set(Some(chrono::Utc::now().timestamp_millis()));
                            toasts.info(
                                "Sync connected".to_string(),
                                ToastOptions::new()
                                    .description("Remote sync is active for this mobile database"),
                            );
                        }
                        Err(error) => {
                            tracing::error!("Initial mobile sync failed: {}", error);
                            sync_state.set(MobileSyncState::Error);
                            status_message.set(Some(format!(
                                "Initial sync failed; retrying in background: {error}"
                            )));
                            toasts.error(
                                "Initial sync failed".to_string(),
                                ToastOptions::new()
                                    .description("Changes will keep retrying in the background"),
                            );
                        }
                    }
                } else {
                    sync_state.set(MobileSyncState::Offline);
                }

                match note_store.list_notes().await {
                    Ok(loaded_notes) => {
                        notes.set(loaded_notes);
                    }
                    Err(error) => {
                        status_message.set(Some(format!("Failed to load notes: {error}")));
                    }
                }
            }
            Err(error) => {
                status_message.set(Some(format!("Failed to open database: {error}")));
            }
        }

        if let Some(shared_text) = launch.share_text {
            apply_share_intent(
                shared_text,
                &mut selected_note_id,
                &mut draft_content,
                &mut status_message,
            );
            view.set(MobileView::Editor);
        } else if launch.quick_capture.enabled {
            apply_quick_capture_launch(
                launch.quick_capture,
                &mut quick_capture_content,
                &mut status_message,
            );
            view.set(MobileView::QuickCapture);
        }

        loading.set(false);
    });

    use_future(move || async move {
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;

            let Some(note_store) = store.read().clone() else {
                continue;
            };

            if !note_store.is_sync_enabled().await {
                sync_state.set(MobileSyncState::Offline);
                continue;
            }

            let previous_sync_state = sync_state();
            sync_state.set(MobileSyncState::Syncing);

            match note_store.sync().await {
                Ok(()) => {
                    sync_state.set(MobileSyncState::Synced);
                    last_sync_at.set(Some(chrono::Utc::now().timestamp_millis()));

                    if previous_sync_state == MobileSyncState::Error {
                        toasts.success(
                            "Sync restored".to_string(),
                            ToastOptions::new()
                                .description("Remote sync recovered after a failure"),
                        );
                    }

                    if let Ok(fresh_notes) = note_store.list_notes().await {
                        notes.set(fresh_notes);
                    }
                }
                Err(error) => {
                    tracing::error!("Periodic mobile sync failed: {}", error);
                    sync_state.set(MobileSyncState::Error);

                    if previous_sync_state != MobileSyncState::Error {
                        toasts.error(
                            "Sync failed".to_string(),
                            ToastOptions::new().description("Will continue retrying automatically"),
                        );
                    }
                }
            }
        }
    });

    let on_new_note = move |_| {
        selected_note_id.set(None);
        draft_content.set(String::new());
        status_message.set(None);
        view.set(MobileView::Editor);
    };

    let on_open_quick_capture = move |_| {
        quick_capture_content.set(String::new());
        status_message.set(None);
        view.set(MobileView::QuickCapture);
    };

    let on_back_to_list = move |_| {
        view.set(MobileView::List);
    };

    let on_save_note = move |_| {
        if saving() {
            return;
        }

        let Some(note_store) = store.read().clone() else {
            status_message.set(Some("Database is not ready yet".to_string()));
            return;
        };

        let content = draft_content().trim().to_string();
        if content.is_empty() {
            status_message.set(Some("Note content cannot be empty".to_string()));
            return;
        }

        let current_note_id = selected_note_id();
        saving.set(true);
        status_message.set(Some("Saving note...".to_string()));

        spawn(async move {
            let save_result = if let Some(note_id) = current_note_id {
                note_store.update_note(&note_id, &content).await
            } else {
                note_store.create_note(&content).await
            };

            match save_result {
                Ok(saved_note) => {
                    selected_note_id.set(Some(saved_note.id));
                    draft_content.set(saved_note.content);

                    match note_store.list_notes().await {
                        Ok(fresh_notes) => {
                            notes.set(fresh_notes);
                            status_message.set(Some("Note saved".to_string()));
                        }
                        Err(error) => {
                            status_message
                                .set(Some(format!("Saved, but failed to refresh list: {error}")));
                        }
                    }
                }
                Err(error) => {
                    status_message.set(Some(format!("Failed to save note: {error}")));
                }
            }

            saving.set(false);
        });
    };

    let on_save_quick_capture = move |_| {
        if saving() {
            return;
        }

        let Some(note_store) = store.read().clone() else {
            status_message.set(Some("Database is not ready yet".to_string()));
            return;
        };

        let content = quick_capture_content().trim().to_string();
        if content.is_empty() {
            status_message.set(Some("Quick capture cannot be empty".to_string()));
            return;
        }

        saving.set(true);
        status_message.set(Some("Saving quick capture...".to_string()));

        spawn(async move {
            match note_store.create_note(&content).await {
                Ok(_) => match note_store.list_notes().await {
                    Ok(fresh_notes) => {
                        notes.set(fresh_notes);
                        quick_capture_content.set(String::new());
                        view.set(MobileView::List);
                        status_message.set(Some("Quick capture saved".to_string()));
                    }
                    Err(error) => {
                        status_message
                            .set(Some(format!("Saved, but failed to refresh list: {error}")));
                    }
                },
                Err(error) => {
                    status_message.set(Some(format!("Failed to save quick capture: {error}")));
                }
            }

            saving.set(false);
        });
    };

    let on_delete_note = move |_| {
        if deleting() {
            return;
        }

        let Some(note_store) = store.read().clone() else {
            status_message.set(Some("Database is not ready yet".to_string()));
            return;
        };
        let Some(note_id) = selected_note_id() else {
            status_message.set(Some("Select a note to delete".to_string()));
            return;
        };

        deleting.set(true);
        status_message.set(Some("Deleting note...".to_string()));

        spawn(async move {
            match note_store.delete_note(&note_id).await {
                Ok(()) => {
                    selected_note_id.set(None);
                    draft_content.set(String::new());
                    view.set(MobileView::List);

                    match note_store.list_notes().await {
                        Ok(fresh_notes) => {
                            notes.set(fresh_notes);
                            status_message.set(Some("Note deleted".to_string()));
                        }
                        Err(error) => {
                            status_message.set(Some(format!(
                                "Deleted, but failed to refresh list: {error}"
                            )));
                        }
                    }
                }
                Err(error) => {
                    status_message.set(Some(format!("Failed to delete note: {error}")));
                }
            }

            deleting.set(false);
        });
    };

    rsx! {
        style {
            r#"
            .toast-container {
                position: fixed;
                inset: auto 12px 12px 12px;
                z-index: 9999;
                pointer-events: none;
            }
            .toast-list {
                margin: 0;
                padding: 0;
                list-style: none;
                display: flex;
                flex-direction: column;
                gap: 8px;
            }
            .toast {
                pointer-events: auto;
                border-radius: 10px;
                border: 1px solid #d1d5db;
                background: #ffffff;
                box-shadow: 0 10px 30px rgba(17, 24, 39, 0.12);
                padding: 10px 12px;
                color: #111827;
                display: flex;
                gap: 10px;
                align-items: flex-start;
            }
            .toast[data-type='success'] { border-color: #10b981; }
            .toast[data-type='error'] { border-color: #ef4444; }
            .toast[data-type='warning'] { border-color: #f59e0b; }
            .toast[data-type='info'] { border-color: #3b82f6; }
            .toast-content { flex: 1; }
            .toast-title { font-size: 13px; font-weight: 700; }
            .toast-description { font-size: 12px; color: #4b5563; margin-top: 2px; }
            .toast-close {
                border: 0;
                background: transparent;
                color: #6b7280;
                font-size: 16px;
                line-height: 1;
                padding: 0;
            }
            "#
        }

        div {
            style: "
                height: 100vh;
                display: flex;
                flex-direction: column;
                background: #f6f8fb;
                color: #111827;
                font-family: system-ui, sans-serif;
            ",

            div {
                style: "
                    padding: 14px 16px;
                    display: flex;
                    justify-content: space-between;
                    align-items: center;
                    background: #ffffff;
                ",
                h1 {
                    style: "margin: 0; font-size: 22px;",
                    "Dirt"
                }
                div {
                    style: "display: flex; flex-direction: column; align-items: flex-end; gap: 2px;",
                    p {
                        style: "margin: 0; color: #6b7280; font-size: 12px;",
                        "F4.5 sync notifications"
                    }
                    p {
                        style: "margin: 0; color: #4b5563; font-size: 11px;",
                        "{sync_state_label(sync_state(), last_sync_at())}"
                    }
                }
            }

            Separator {
                decorative: true,
                style: "height: 1px; background: #e5e7eb;",
            }

            if let Some(message) = status_message() {
                p {
                    style: "margin: 0; padding: 10px 16px; font-size: 13px; color: #374151;",
                    "{message}"
                }
                Separator {
                    decorative: true,
                    style: "height: 1px; background: #e5e7eb;",
                }
            }

            if loading() {
                div {
                    style: "
                        flex: 1;
                        display: flex;
                        align-items: center;
                        justify-content: center;
                        color: #6b7280;
                    ",
                    "Loading notes..."
                }
            } else if view() == MobileView::List {
                div {
                    style: "padding: 12px 16px; display: flex; gap: 8px;",
                    button {
                        type: "button",
                        style: "
                            flex: 1;
                            border: 0;
                            border-radius: 10px;
                            padding: 12px;
                            background: #111827;
                            color: #ffffff;
                            font-weight: 600;
                            font-size: 14px;
                        ",
                        onclick: on_new_note,
                        "New note"
                    }
                    button {
                        type: "button",
                        style: "
                            flex: 1;
                            border: 1px solid #2563eb;
                            border-radius: 10px;
                            padding: 12px;
                            background: #eff6ff;
                            color: #1d4ed8;
                            font-weight: 600;
                            font-size: 14px;
                        ",
                        onclick: on_open_quick_capture,
                        "Quick capture"
                    }
                }

                ScrollArea {
                    direction: ScrollDirection::Vertical,
                    scroll_type: ScrollType::Auto,
                    tabindex: "0",
                    style: "flex: 1; padding: 0 12px 16px 12px;",

                    if notes().is_empty() {
                        div {
                            style: "
                                margin-top: 24px;
                                padding: 20px;
                                background: #ffffff;
                                border: 1px solid #e5e7eb;
                                border-radius: 12px;
                                text-align: center;
                                color: #6b7280;
                            ",
                            "No notes yet. Create your first note."
                        }
                    } else {
                        for note in notes() {
                            {
                                let note_id = note.id;
                                let note_content = note.content.clone();
                                let title = note_title(&note);
                                let preview = note_preview(&note);
                                let updated = relative_time(note.updated_at);
                                let selected = selected_note_id() == Some(note_id);
                                let border_color = if selected { "#2563eb" } else { "#e5e7eb" };
                                let card_style = format!(
                                    "margin-bottom: 10px;\
                                     width: 100%;\
                                     border: 1px solid {border_color};\
                                     background: #ffffff;\
                                     border-radius: 12px;\
                                     padding: 12px;\
                                     text-align: left;"
                                );

                                rsx! {
                                    button {
                                        key: "{note_id}",
                                        type: "button",
                                        style: "{card_style}",
                                        onclick: move |_| {
                                            selected_note_id.set(Some(note_id));
                                            draft_content.set(note_content.clone());
                                            status_message.set(None);
                                            view.set(MobileView::Editor);
                                        },

                                        p {
                                            style: "
                                                margin: 0 0 6px 0;
                                                font-size: 15px;
                                                font-weight: 600;
                                                color: #111827;
                                            ",
                                            "{title}"
                                        }
                                        p {
                                            style: "
                                                margin: 0 0 6px 0;
                                                font-size: 13px;
                                                color: #6b7280;
                                            ",
                                            "{preview}"
                                        }
                                        p {
                                            style: "margin: 0; font-size: 12px; color: #9ca3af;",
                                            "Updated {updated}"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            } else if view() == MobileView::QuickCapture {
                div {
                    style: "
                        padding: 10px 12px;
                        display: flex;
                        gap: 8px;
                        background: #ffffff;
                    ",
                    button {
                        type: "button",
                        style: "
                            border: 1px solid #d1d5db;
                            border-radius: 8px;
                            padding: 10px 12px;
                            background: #ffffff;
                            font-weight: 600;
                        ",
                        onclick: on_back_to_list,
                        "Cancel"
                    }
                    button {
                        type: "button",
                        style: "
                            border: 0;
                            border-radius: 8px;
                            padding: 10px 12px;
                            background: #2563eb;
                            color: #ffffff;
                            font-weight: 600;
                        ",
                        disabled: saving(),
                        onclick: on_save_quick_capture,
                        if saving() { "Saving..." } else { "Save capture" }
                    }
                }

                Separator {
                    decorative: true,
                    style: "height: 1px; background: #e5e7eb;",
                }

                textarea {
                    style: "
                        flex: 1;
                        margin: 12px;
                        border: 1px solid #93c5fd;
                        border-radius: 12px;
                        padding: 14px;
                        line-height: 1.5;
                        font-size: 15px;
                        resize: none;
                        background: #eff6ff;
                    ",
                    value: "{quick_capture_content}",
                    placeholder: "Quick capture: write and save...",
                    oninput: move |event: Event<FormData>| {
                        quick_capture_content.set(event.value());
                    },
                }
            } else {
                div {
                    style: "
                        padding: 10px 12px;
                        display: flex;
                        gap: 8px;
                        background: #ffffff;
                    ",
                    button {
                        type: "button",
                        style: "
                            border: 1px solid #d1d5db;
                            border-radius: 8px;
                            padding: 10px 12px;
                            background: #ffffff;
                            font-weight: 600;
                        ",
                        onclick: on_back_to_list,
                        "Back"
                    }
                    button {
                        type: "button",
                        style: "
                            border: 0;
                            border-radius: 8px;
                            padding: 10px 12px;
                            background: #2563eb;
                            color: #ffffff;
                            font-weight: 600;
                        ",
                        disabled: saving(),
                        onclick: on_save_note,
                        if saving() { "Saving..." } else { "Save" }
                    }
                    if selected_note_id().is_some() {
                        button {
                            type: "button",
                            style: "
                                margin-left: auto;
                                border: 1px solid #ef4444;
                                border-radius: 8px;
                                padding: 10px 12px;
                                background: #ffffff;
                                color: #b91c1c;
                                font-weight: 600;
                            ",
                            disabled: deleting(),
                            onclick: on_delete_note,
                            if deleting() { "Deleting..." } else { "Delete" }
                        }
                    }
                }

                Separator {
                    decorative: true,
                    style: "height: 1px; background: #e5e7eb;",
                }

                textarea {
                    style: "
                        flex: 1;
                        margin: 12px;
                        border: 1px solid #d1d5db;
                        border-radius: 12px;
                        padding: 14px;
                        line-height: 1.5;
                        font-size: 15px;
                        resize: none;
                        background: #ffffff;
                    ",
                    value: "{draft_content}",
                    placeholder: "Write your note...",
                    oninput: move |event: Event<FormData>| {
                        draft_content.set(event.value());
                    },
                }
            }
        }
    }
}

fn apply_quick_capture_launch(
    launch: QuickCaptureLaunch,
    quick_capture_content: &mut Signal<String>,
    status_message: &mut Signal<Option<String>>,
) {
    if let Some(seed) = launch.seed_text {
        quick_capture_content.set(seed);
    } else {
        quick_capture_content.set(String::new());
    }
    status_message.set(Some("Quick capture mode ready".to_string()));
}

fn apply_share_intent(
    shared_text: String,
    selected_note_id: &mut Signal<Option<NoteId>>,
    draft_content: &mut Signal<String>,
    status_message: &mut Signal<Option<String>>,
) {
    selected_note_id.set(None);
    draft_content.set(shared_text);
    status_message.set(Some("Shared text ready to save".to_string()));
}

fn sync_state_label(state: MobileSyncState, last_sync_at: Option<i64>) -> String {
    match state {
        MobileSyncState::Offline => "Sync: local-only mode".to_string(),
        MobileSyncState::Syncing => "Sync: syncing...".to_string(),
        MobileSyncState::Synced => last_sync_at
            .map(|timestamp| format!("Sync: updated {}", relative_time(timestamp)))
            .unwrap_or_else(|| "Sync: connected".to_string()),
        MobileSyncState::Error => "Sync: retrying after error".to_string(),
    }
}

fn note_title(note: &Note) -> String {
    let title = note.title_preview(48);
    if title.trim().is_empty() {
        "Untitled note".to_string()
    } else {
        title
    }
}

fn note_preview(note: &Note) -> String {
    let preview = note
        .content
        .lines()
        .skip(1)
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .trim()
        .chars()
        .take(80)
        .collect::<String>();

    if preview.is_empty() {
        "Tap to open".to_string()
    } else {
        preview
    }
}

fn relative_time(updated_at_ms: i64) -> String {
    let now = chrono::Utc::now().timestamp_millis();
    let delta = (now - updated_at_ms).max(0);

    if delta < 60_000 {
        "just now".to_string()
    } else if delta < 3_600_000 {
        format!("{}m ago", delta / 60_000)
    } else if delta < 86_400_000 {
        format!("{}h ago", delta / 3_600_000)
    } else {
        format!("{}d ago", delta / 86_400_000)
    }
}
