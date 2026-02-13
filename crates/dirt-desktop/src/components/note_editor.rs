//! Note editor component

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use dioxus::html::HasFileData;
use dioxus::prelude::*;
use rfd::AsyncFileDialog;

use dirt_core::models::{Attachment, AttachmentId};
use dirt_core::NoteId;

use super::button::{Button, ButtonVariant};
use super::dialog::{DialogContent, DialogDescription, DialogRoot, DialogTitle};
use crate::queries::invalidate_notes_query;
use crate::services::{
    cleanup_temp_voice_memo, discard_voice_memo_recording, start_voice_memo_recording,
    stop_voice_memo_recording, transition_voice_memo_state, AuthSession, DatabaseService,
    MediaApiClient, VoiceMemoRecorderEvent, VoiceMemoRecorderState,
};
use crate::state::AppState;

/// Idle save delay - save after 2 seconds of no typing
const IDLE_SAVE_MS: u64 = 2000;
const KIB_BYTES: u64 = 1024;
const MIB_BYTES: u64 = KIB_BYTES * 1024;
const GIB_BYTES: u64 = MIB_BYTES * 1024;
const MAX_TEXT_PREVIEW_BYTES: usize = 256 * 1024;
const MAX_MEDIA_PREVIEW_BYTES: usize = 8 * 1024 * 1024;
const ATTACHMENT_LIST_MAX_ATTEMPTS: usize = 3;
const ATTACHMENT_LIST_RETRY_DELAY_MS: u64 = 120;

#[derive(Clone, Copy, PartialEq, Eq)]
enum AttachmentKind {
    Image,
    Video,
    Audio,
    Text,
    File,
}

#[derive(Clone, PartialEq, Eq, Default)]
enum AttachmentPreview {
    #[default]
    None,
    Text {
        content: String,
        truncated: bool,
    },
    MediaDataUri {
        mime_type: String,
        data_uri: String,
    },
    Unsupported {
        mime_type: String,
        reason: String,
    },
}

#[derive(Clone, Copy)]
struct UploadSignals {
    uploading: Signal<bool>,
    upload_error: Signal<Option<String>>,
    attachment_refresh_signal: Signal<u64>,
}

#[derive(Clone)]
struct UploadContext {
    db: Option<Arc<DatabaseService>>,
    media_api: Option<Arc<MediaApiClient>>,
    auth_session: Option<AuthSession>,
    signals: UploadSignals,
}

/// Plain text note editor with auto-save
#[component]
pub fn NoteEditor() -> Element {
    let mut state = use_context::<AppState>();
    let current_note = state.current_note();
    let colors = (state.theme)().palette();

    // Local state for the editor content
    let mut content = use_signal(String::new);
    let mut last_note_id = use_signal(|| None::<NoteId>);

    // Version-based save tracking
    let mut save_version = use_signal(|| 0u64);
    let mut last_saved_version = use_signal(|| 0u64);
    let mut attachment_upload_error = use_signal(|| None::<String>);
    let attachment_uploading = use_signal(|| false);
    let attachments = use_signal(Vec::<Attachment>::new);
    let mut attachments_error = use_signal(|| None::<String>);
    let attachments_loading = use_signal(|| false);
    let attachment_refresh_version = use_signal(|| 0u64);
    let attachment_load_request_id = use_hook(|| Arc::new(AtomicU64::new(0)));
    let mut deleting_attachment_id = use_signal(|| None::<AttachmentId>);
    let mut drag_over = use_signal(|| false);
    let mut preview_open = use_signal(|| false);
    let mut preview_loading = use_signal(|| false);
    let mut preview_title = use_signal(String::new);
    let mut preview_error = use_signal(|| None::<String>);
    let mut preview_content = use_signal(AttachmentPreview::default);
    let mut voice_memo_state = use_signal(VoiceMemoRecorderState::default);
    let mut voice_memo_started_at = use_signal(|| None::<Instant>);

    // Sync content when selected note changes
    use_effect(move || {
        let current = state.current_note();
        let current_id = current.as_ref().map(|n| n.id);

        if current_id != *last_note_id.read() {
            if let Some(note) = current {
                content.set(note.content);
            } else {
                content.set(String::new());
            }
            last_note_id.set(current_id);
            // Reset save tracking for new note
            save_version.set(0);
            last_saved_version.set(0);
            deleting_attachment_id.set(None);
            let was_recording = voice_memo_state() != VoiceMemoRecorderState::Idle;
            voice_memo_state.set(VoiceMemoRecorderState::Idle);
            voice_memo_started_at.set(None);
            if was_recording {
                spawn(async move {
                    let _ = discard_voice_memo_recording().await;
                });
            }
        }
    });

    use_effect(move || {
        let note_id = *last_note_id.read();
        let _attachment_refresh_version = attachment_refresh_version();
        let request_id = attachment_load_request_id.fetch_add(1, Ordering::SeqCst) + 1;
        let db = state.db_service.read().clone();
        let mut attachment_signal = attachments;
        let mut attachment_error_signal = attachments_error;
        let mut attachment_loading_signal = attachments_loading;
        let request_id_signal = attachment_load_request_id.clone();

        spawn(async move {
            if request_id_signal.load(Ordering::SeqCst) != request_id {
                return;
            }
            attachment_error_signal.set(None);

            let Some(note_id) = note_id else {
                if request_id_signal.load(Ordering::SeqCst) == request_id {
                    attachment_signal.set(Vec::new());
                    attachment_loading_signal.set(false);
                }
                return;
            };

            let Some(db) = db else {
                if request_id_signal.load(Ordering::SeqCst) == request_id {
                    attachment_signal.set(Vec::new());
                    attachment_error_signal
                        .set(Some("Database service is not available.".to_string()));
                    attachment_loading_signal.set(false);
                }
                return;
            };

            if request_id_signal.load(Ordering::SeqCst) == request_id {
                attachment_loading_signal.set(true);
            }

            let load_result = list_attachments_with_retry(db.as_ref(), &note_id).await;

            if request_id_signal.load(Ordering::SeqCst) != request_id {
                return;
            }

            match load_result {
                Ok(list) => attachment_signal.set(list),
                Err(error) => {
                    attachment_signal.set(Vec::new());
                    attachment_error_signal
                        .set(Some(format!("Failed to load attachments: {error}")));
                }
            }
            attachment_loading_signal.set(false);
        });
    });

    // Auto-save with proper debounce using version tracking
    use_effect(move || {
        let current_version = save_version();
        if current_version == 0 || current_version == last_saved_version() {
            return; // Nothing to save
        }

        let note_id = *last_note_id.read();
        let content_to_save = content.read().clone();
        if let Some(id) = note_id {
            state.enqueue_pending_change(id);
        }

        spawn(async move {
            // Wait for idle period
            tokio::time::sleep(Duration::from_millis(IDLE_SAVE_MS)).await;

            // Check if version changed during sleep (user typed more)
            if save_version() != current_version {
                return; // Stale, a newer version is pending
            }

            // Perform save to DB
            if let Some(id) = note_id {
                let db = state.db_service.read().clone();
                if let Some(db) = db {
                    match db.update_note(&id, &content_to_save).await {
                        Ok(_) => {
                            tracing::debug!("Auto-saved note: {}", id);
                            last_saved_version.set(current_version);
                            // Invalidate query to keep other views in sync
                            invalidate_notes_query().await;
                        }
                        Err(e) => {
                            tracing::error!("Failed to save note: {}", e);
                        }
                    }
                }
            }
        });
    });

    // Helper to perform immediate save
    let mut perform_save_now = move || {
        let current_version = save_version();
        if current_version == 0 || current_version == last_saved_version() {
            return; // Nothing to save
        }

        let note_id = *last_note_id.read();
        let content_to_save = content.read().clone();
        if let Some(id) = note_id {
            state.enqueue_pending_change(id);
        }

        spawn(async move {
            if let Some(id) = note_id {
                let db = state.db_service.read().clone();
                if let Some(db) = db {
                    match db.update_note(&id, &content_to_save).await {
                        Ok(_) => {
                            tracing::debug!("Saved note on blur/shortcut: {}", id);
                            last_saved_version.set(current_version);
                            invalidate_notes_query().await;
                        }
                        Err(e) => {
                            tracing::error!("Failed to save note: {}", e);
                        }
                    }
                }
            }
        });
    };

    let on_input = move |evt: Event<FormData>| {
        let new_content = evt.value();
        content.set(new_content.clone());
        save_version.set(save_version() + 1);

        // Optimistic update: update local state immediately
        if let Some(id) = *last_note_id.read() {
            let mut notes = state.notes.write();
            if let Some(note) = notes.iter_mut().find(|n| n.id == id) {
                note.content = new_content;
                note.updated_at = chrono::Utc::now().timestamp_millis();
            }
        }
    };

    let on_blur = move |_| {
        perform_save_now();
    };

    let on_keydown = move |evt: Event<KeyboardData>| {
        // Ctrl+S to save immediately
        if evt.modifiers().ctrl() && evt.key() == Key::Character("s".to_string()) {
            evt.prevent_default();
            perform_save_now();
        }
    };

    let on_drag_over = move |evt: Event<DragData>| {
        evt.prevent_default();
        drag_over.set(true);
    };

    let on_drag_leave = move |_: Event<DragData>| {
        drag_over.set(false);
    };

    let on_drop_attachment = move |evt: Event<DragData>| {
        evt.prevent_default();
        drag_over.set(false);
        attachment_upload_error.set(None);

        if attachment_uploading() {
            return;
        }

        let Some(note_id) = *last_note_id.read() else {
            attachment_upload_error.set(Some(
                "Select a note before dropping an attachment.".to_string(),
            ));
            return;
        };

        let mut files = evt.files();
        let Some(file) = files.pop() else {
            return;
        };

        let file_name = file.name();
        let file_content_type = file.content_type();

        if file_name.trim().is_empty() {
            attachment_upload_error.set(Some(
                "Dropped file is missing a valid filename.".to_string(),
            ));
            return;
        }

        let mut upload_error = attachment_upload_error;
        let signals = UploadSignals {
            uploading: attachment_uploading,
            upload_error,
            attachment_refresh_signal: attachment_refresh_version,
        };
        let upload_context = UploadContext {
            db: state.db_service.read().clone(),
            media_api: state.media_api_client.read().clone(),
            auth_session: (state.auth_session)(),
            signals,
        };

        spawn(async move {
            let file_bytes = match file.read_bytes().await {
                Ok(bytes) => bytes.to_vec(),
                Err(error) => {
                    upload_error.set(Some(format!("Failed to read dropped file: {error}")));
                    return;
                }
            };

            upload_attachment(
                note_id,
                file_name,
                file_content_type,
                file_bytes,
                upload_context,
            )
            .await;
        });
    };

    let on_pick_attachment = move |_: MouseEvent| {
        attachment_upload_error.set(None);

        if attachment_uploading() {
            return;
        }

        let Some(note_id) = *last_note_id.read() else {
            attachment_upload_error.set(Some(
                "Select a note before uploading attachments.".to_string(),
            ));
            return;
        };

        let mut upload_error = attachment_upload_error;
        let signals = UploadSignals {
            uploading: attachment_uploading,
            upload_error,
            attachment_refresh_signal: attachment_refresh_version,
        };
        let upload_context = UploadContext {
            db: state.db_service.read().clone(),
            media_api: state.media_api_client.read().clone(),
            auth_session: (state.auth_session)(),
            signals,
        };

        spawn(async move {
            let Some(file) = AsyncFileDialog::new().pick_file().await else {
                return;
            };

            let file_name = file.file_name();
            if file_name.trim().is_empty() {
                upload_error.set(Some("Selected file has an empty filename.".to_string()));
                return;
            }

            let file_bytes = file.read().await;
            let file_content_type = mime_guess::from_path(&file_name)
                .first_raw()
                .map(str::to_string);

            upload_attachment(
                note_id,
                file_name,
                file_content_type,
                file_bytes,
                upload_context,
            )
            .await;
        });
    };

    let on_start_voice_memo = move |_| {
        attachment_upload_error.set(None);

        if attachment_uploading() || voice_memo_state() != VoiceMemoRecorderState::Idle {
            return;
        }

        let Some(_note_id) = *last_note_id.read() else {
            attachment_upload_error.set(Some(
                "Select a note before recording a voice memo.".to_string(),
            ));
            return;
        };

        voice_memo_state.set(transition_voice_memo_state(
            voice_memo_state(),
            VoiceMemoRecorderEvent::StartRequested,
        ));

        spawn(async move {
            match start_voice_memo_recording().await {
                Ok(()) => {
                    voice_memo_state.set(transition_voice_memo_state(
                        voice_memo_state(),
                        VoiceMemoRecorderEvent::StartSucceeded,
                    ));
                    voice_memo_started_at.set(Some(Instant::now()));
                }
                Err(error) => {
                    voice_memo_state.set(transition_voice_memo_state(
                        voice_memo_state(),
                        VoiceMemoRecorderEvent::StartFailed,
                    ));
                    voice_memo_started_at.set(None);
                    attachment_upload_error.set(Some(format!(
                        "Voice memo recording failed to start: {error}"
                    )));
                }
            }
        });
    };

    let on_stop_voice_memo = move |_| {
        attachment_upload_error.set(None);

        if attachment_uploading() || voice_memo_state() != VoiceMemoRecorderState::Recording {
            return;
        }

        let Some(note_id) = *last_note_id.read() else {
            attachment_upload_error.set(Some(
                "Select a note before attaching a voice memo.".to_string(),
            ));
            return;
        };

        voice_memo_state.set(transition_voice_memo_state(
            voice_memo_state(),
            VoiceMemoRecorderEvent::StopRequested,
        ));

        let signals = UploadSignals {
            uploading: attachment_uploading,
            upload_error: attachment_upload_error,
            attachment_refresh_signal: attachment_refresh_version,
        };
        let upload_context = UploadContext {
            db: state.db_service.read().clone(),
            media_api: state.media_api_client.read().clone(),
            auth_session: (state.auth_session)(),
            signals,
        };

        spawn(async move {
            match stop_voice_memo_recording().await {
                Ok(recorded) => {
                    upload_attachment(
                        note_id,
                        recorded.file_name.clone(),
                        Some(recorded.mime_type.clone()),
                        recorded.bytes,
                        upload_context,
                    )
                    .await;
                    cleanup_temp_voice_memo(recorded.temp_path.as_path());
                    voice_memo_state.set(transition_voice_memo_state(
                        voice_memo_state(),
                        VoiceMemoRecorderEvent::StopSucceeded,
                    ));
                    voice_memo_started_at.set(None);
                }
                Err(error) => {
                    voice_memo_state.set(transition_voice_memo_state(
                        voice_memo_state(),
                        VoiceMemoRecorderEvent::StopFailed,
                    ));
                    voice_memo_started_at.set(None);
                    attachment_upload_error
                        .set(Some(format!("Failed to finalize voice memo: {error}")));
                }
            }
        });
    };

    let on_discard_voice_memo = move |_| {
        attachment_upload_error.set(None);

        if voice_memo_state() == VoiceMemoRecorderState::Idle {
            return;
        }

        voice_memo_state.set(transition_voice_memo_state(
            voice_memo_state(),
            VoiceMemoRecorderEvent::DiscardRequested,
        ));
        voice_memo_started_at.set(None);

        spawn(async move {
            if let Err(error) = discard_voice_memo_recording().await {
                attachment_upload_error.set(Some(format!(
                    "Failed to discard voice memo recording: {error}"
                )));
            }
        });
    };

    let border_color = if drag_over() {
        colors.accent
    } else {
        "transparent"
    };
    let attachment_items = attachments();
    let active_deleting_attachment = deleting_attachment_id();
    let voice_memo_state_value = voice_memo_state();
    let voice_memo_status = match voice_memo_state_value {
        VoiceMemoRecorderState::Idle => None,
        VoiceMemoRecorderState::Starting => Some("Requesting microphone access...".to_string()),
        VoiceMemoRecorderState::Recording => {
            let elapsed = voice_memo_started_at().map_or(0_u64, elapsed_millis_u64);
            Some(format!(
                "Recording voice memo... {}",
                format_recording_duration(elapsed)
            ))
        }
        VoiceMemoRecorderState::Stopping => Some("Finalizing voice memo...".to_string()),
    };

    rsx! {
        div {
            class: "note-editor",
            style: "
                flex: 1;
                display: flex;
                flex-direction: column;
                padding: 16px;
                background: {colors.bg_primary};
            ",

            if current_note.is_some() {
                if attachment_uploading() {
                    div {
                        style: "
                            margin-bottom: 8px;
                            color: {colors.text_muted};
                            font-size: 12px;
                        ",
                        "Uploading attachment..."
                    }
                }

                if let Some(status) = &voice_memo_status {
                    div {
                        style: "
                            margin-bottom: 8px;
                            color: {colors.text_muted};
                            font-size: 12px;
                        ",
                        "{status}"
                    }
                }

                if let Some(error) = attachment_upload_error() {
                    div {
                        style: "
                            margin-bottom: 8px;
                            color: {colors.error};
                            font-size: 12px;
                        ",
                        "{error}"
                    }
                }

                if let Some(error) = attachments_error() {
                    div {
                        style: "
                            margin-bottom: 8px;
                            color: {colors.error};
                            font-size: 12px;
                        ",
                        "{error}"
                    }
                }

                textarea {
                    class: "editor-textarea",
                    style: "
                        flex: 1;
                        width: 100%;
                        border: 1px dashed {border_color};
                        border-radius: 8px;
                        outline: none;
                        resize: none;
                        font-family: inherit;
                        font-size: inherit;
                        line-height: 1.6;
                        background: transparent;
                        color: {colors.text_primary};
                    ",
                    value: "{content}",
                    placeholder: "Start typing...",
                    oninput: on_input,
                    onblur: on_blur,
                    onkeydown: on_keydown,
                    ondragover: on_drag_over,
                    ondragleave: on_drag_leave,
                    ondrop: on_drop_attachment,
                }

                div {
                    style: "
                        margin-top: 8px;
                        border-top: 1px solid {colors.border};
                        padding-top: 8px;
                        display: flex;
                        flex-direction: column;
                        gap: 6px;
                        min-width: 0;
                        overflow-x: hidden;
                    ",
                    div {
                        style: "
                            display: flex;
                            align-items: center;
                            justify-content: space-between;
                            gap: 12px;
                        ",
                        div {
                            style: "
                                font-size: 12px;
                                color: {colors.text_muted};
                                text-transform: uppercase;
                                letter-spacing: 0.04em;
                            ",
                            "Attachments"
                        }

                        div {
                            style: "
                                display: flex;
                                align-items: center;
                                gap: 8px;
                            ",
                            Button {
                                variant: ButtonVariant::Secondary,
                                onclick: on_pick_attachment,
                                disabled: attachment_uploading()
                                    || voice_memo_state_value == VoiceMemoRecorderState::Stopping,
                                style: "padding: 3px 10px; font-size: 12px;",
                                "+ Upload"
                            }
                            if voice_memo_state_value == VoiceMemoRecorderState::Idle {
                                Button {
                                    variant: ButtonVariant::Ghost,
                                    onclick: on_start_voice_memo,
                                    disabled: attachment_uploading(),
                                    style: "padding: 3px 10px; font-size: 12px;",
                                    "Record"
                                }
                            } else if voice_memo_state_value == VoiceMemoRecorderState::Starting {
                                Button {
                                    variant: ButtonVariant::Ghost,
                                    disabled: true,
                                    style: "padding: 3px 10px; font-size: 12px;",
                                    "Starting..."
                                }
                            } else if voice_memo_state_value == VoiceMemoRecorderState::Recording {
                                Button {
                                    variant: ButtonVariant::Secondary,
                                    onclick: on_stop_voice_memo,
                                    disabled: attachment_uploading(),
                                    style: "padding: 3px 10px; font-size: 12px;",
                                    "Stop & attach"
                                }
                                Button {
                                    variant: ButtonVariant::Ghost,
                                    onclick: on_discard_voice_memo,
                                    disabled: attachment_uploading(),
                                    style: "padding: 3px 10px; font-size: 12px;",
                                    "Discard"
                                }
                            } else {
                                Button {
                                    variant: ButtonVariant::Ghost,
                                    disabled: true,
                                    style: "padding: 3px 10px; font-size: 12px;",
                                    "Attaching..."
                                }
                            }
                        }
                    }

                    if attachments_loading() {
                        div {
                            style: "
                                font-size: 12px;
                                color: {colors.text_muted};
                            ",
                            "Loading attachments..."
                        }
                    } else if attachment_items.is_empty() {
                        div {
                            style: "
                                font-size: 12px;
                                color: {colors.text_muted};
                            ",
                            "No attachments yet"
                        }
                    } else {
                        for attachment in attachment_items {
                            div {
                                key: "{attachment.id}",
                                style: "
                                    display: grid;
                                    grid-template-columns: minmax(0, 1fr) auto auto;
                                    align-items: center;
                                    column-gap: 12px;
                                    min-width: 0;
                                    width: 100%;
                                    overflow: hidden;
                                    font-size: 12px;
                                ",
                                div {
                                    style: "
                                        min-width: 0;
                                        overflow: hidden;
                                        display: flex;
                                        align-items: baseline;
                                        gap: 8px;
                                        color: {colors.text_primary};
                                    ",
                                    span {
                                        style: "
                                            display: block;
                                            flex: 1 1 auto;
                                            min-width: 0;
                                            max-width: 100%;
                                            overflow: hidden;
                                            text-overflow: ellipsis;
                                            white-space: nowrap;
                                        ",
                                        "{attachment.filename}"
                                    }
                                    span {
                                        style: "
                                            color: {colors.text_muted};
                                            white-space: nowrap;
                                            flex-shrink: 0;
                                        ",
                                        "{attachment_kind_label(&attachment.filename, &attachment.mime_type)}"
                                    }
                                }
                                span {
                                    style: "
                                        color: {colors.text_muted};
                                        white-space: nowrap;
                                        flex-shrink: 0;
                                    ",
                                    "{format_attachment_size(attachment.size_bytes)}"
                                }
                                div {
                                    style: "
                                        display: flex;
                                        align-items: center;
                                        gap: 6px;
                                        flex-shrink: 0;
                                    ",
                                    Button {
                                        variant: ButtonVariant::Ghost,
                                        style: "padding: 2px 8px; font-size: 11px;",
                                        onclick: {
                                            let attachment = attachment.clone();
                                            move |_| {
                                                attachments_error.set(None);
                                                preview_open.set(true);
                                                preview_loading.set(true);
                                                preview_error.set(None);
                                                preview_title.set(attachment.filename.clone());
                                                preview_content.set(AttachmentPreview::None);

                                                let mut preview_loading_signal = preview_loading;
                                                let mut preview_error_signal = preview_error;
                                                let mut preview_content_signal = preview_content;
                                                let attachment = attachment.clone();
                                                let media_api = state.media_api_client.read().clone();
                                                let auth_session = (state.auth_session)();

                                                spawn(async move {
                                                    match load_attachment_preview(
                                                        &attachment,
                                                        media_api,
                                                        auth_session,
                                                    )
                                                    .await
                                                    {
                                                        Ok(preview) => {
                                                            preview_content_signal.set(preview);
                                                        }
                                                        Err(error) => {
                                                            preview_error_signal.set(Some(error));
                                                        }
                                                    }
                                                    preview_loading_signal.set(false);
                                                });
                                            }
                                        },
                                        "Open"
                                    }
                                    Button {
                                        variant: ButtonVariant::Ghost,
                                        style: "padding: 2px 8px; font-size: 11px;",
                                        disabled: active_deleting_attachment == Some(attachment.id),
                                        onclick: move |_| {
                                            let mut deleting_signal = deleting_attachment_id;
                                            let mut attachment_error_signal = attachments_error;
                                            let mut refresh_signal = attachment_refresh_version;
                                            let db = state.db_service.read().clone();
                                            let attachment_id = attachment.id;
                                            let object_key = attachment.r2_key.clone();
                                            let media_api = state.media_api_client.read().clone();
                                            let auth_session = (state.auth_session)();

                                            spawn(async move {
                                                attachment_error_signal.set(None);
                                                deleting_signal.set(Some(attachment_id));

                                                let Some(db) = db else {
                                                    attachment_error_signal.set(Some(
                                                        "Database service is not available.".to_string(),
                                                    ));
                                                    deleting_signal.set(None);
                                                    return;
                                                };

                                                match db.delete_attachment(&attachment_id).await {
                                                    Ok(()) => {
                                                        refresh_signal.set(refresh_signal() + 1);

                                                        if let Err(error) = delete_remote_attachment(
                                                            &object_key,
                                                            media_api,
                                                            auth_session,
                                                        )
                                                        .await
                                                        {
                                                            attachment_error_signal.set(Some(format!(
                                                                "Attachment removed locally, but failed to delete remote object: {error}"
                                                            )));
                                                        }
                                                    }
                                                    Err(error) => {
                                                        attachment_error_signal.set(Some(format!(
                                                            "Failed to delete attachment: {error}"
                                                        )));
                                                    }
                                                }

                                                deleting_signal.set(None);
                                            });
                                        },
                                        if active_deleting_attachment == Some(attachment.id) {
                                            "Deleting..."
                                        } else {
                                            "Delete"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                DialogRoot {
                    open: preview_open(),
                    on_open_change: move |open: bool| {
                        preview_open.set(open);
                        if !open {
                            preview_loading.set(false);
                            preview_error.set(None);
                            preview_content.set(AttachmentPreview::None);
                            preview_title.set(String::new());
                        }
                    },

                    DialogContent {
                        style: "width: min(920px, 94vw); max-height: 88vh; overflow: hidden; text-align: left;",

                        div {
                            style: "display: flex; align-items: center; justify-content: space-between; gap: 12px;",
                            DialogTitle {
                                style: "flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;",
                                "{preview_title}"
                            }
                            Button {
                                variant: ButtonVariant::Ghost,
                                onclick: move |_| preview_open.set(false),
                                style: "padding: 4px 8px; font-size: 16px;",
                                "x"
                            }
                        }

                        DialogDescription {
                            style: "margin-top: -8px;",
                            "Attachment preview"
                        }

                        div {
                            style: "
                                border: 1px solid {colors.border};
                                border-radius: 8px;
                                padding: 12px;
                                min-height: 180px;
                                max-height: 60vh;
                                overflow: auto;
                                background: {colors.bg_secondary};
                                color: {colors.text_primary};
                            ",

                            if preview_loading() {
                                div { "Loading preview..." }
                            } else if let Some(error) = preview_error() {
                                div {
                                    style: "color: {colors.error};",
                                    "{error}"
                                }
                            } else {
                                {render_preview_content(preview_content(), &preview_title(), colors)}
                            }
                        }
                    }
                }
            } else {
                div {
                    class: "editor-placeholder",
                    style: "
                        flex: 1;
                        display: flex;
                        align-items: center;
                        justify-content: center;
                        color: {colors.text_muted};
                    ",
                    "Select a note or create a new one"
                }
            }
        }
    }
}

async fn upload_attachment(
    note_id: NoteId,
    file_name: String,
    file_content_type: Option<String>,
    file_bytes: Vec<u8>,
    context: UploadContext,
) {
    let mut uploading = context.signals.uploading;
    let mut upload_error = context.signals.upload_error;
    let mut attachment_refresh_signal = context.signals.attachment_refresh_signal;

    uploading.set(true);

    let Some(db) = context.db else {
        upload_error.set(Some("Database service is not available.".to_string()));
        uploading.set(false);
        return;
    };

    let Some(media_api) = context.media_api else {
        upload_error.set(Some(
            "Cloud media is not configured for this build.".to_string(),
        ));
        uploading.set(false);
        return;
    };
    let access_token = match require_media_access_token(context.auth_session) {
        Ok(token) => token,
        Err(error) => {
            upload_error.set(Some(error));
            uploading.set(false);
            return;
        }
    };

    let object_key = build_media_object_key(&note_id, &file_name);

    let mime_type = infer_attachment_mime_type(file_content_type.as_deref(), &file_name);

    if let Err(error) = media_api
        .upload(&access_token, &object_key, &mime_type, file_bytes.as_ref())
        .await
    {
        upload_error.set(Some(format!("Failed to upload attachment: {error}")));
        uploading.set(false);
        return;
    }

    if let Err(error) = db
        .create_attachment(
            &note_id,
            &file_name,
            &mime_type,
            file_size_i64(file_bytes.len()),
            &object_key,
        )
        .await
    {
        upload_error.set(Some(format!("Failed to save attachment metadata: {error}")));
        uploading.set(false);
        return;
    }

    attachment_refresh_signal.set(attachment_refresh_signal() + 1);
    uploading.set(false);
}

async fn load_attachment_preview(
    attachment: &Attachment,
    media_api: Option<Arc<MediaApiClient>>,
    auth_session: Option<AuthSession>,
) -> Result<AttachmentPreview, String> {
    let Some(media_api) = media_api else {
        return Err("Cloud media is not configured for this build.".to_string());
    };
    let access_token = require_media_access_token(auth_session)?;
    let (bytes, downloaded_content_type) = media_api
        .download(&access_token, &attachment.r2_key)
        .await
        .map_err(|error| format!("Failed to download attachment: {error}"))?;

    let content_type_hint = downloaded_content_type
        .as_deref()
        .or(Some(attachment.mime_type.as_str()));
    let mime_type = infer_attachment_mime_type(content_type_hint, &attachment.filename);

    Ok(build_attachment_preview(
        &attachment.filename,
        &mime_type,
        &bytes,
    ))
}

async fn delete_remote_attachment(
    object_key: &str,
    media_api: Option<Arc<MediaApiClient>>,
    auth_session: Option<AuthSession>,
) -> Result<(), String> {
    let Some(media_api) = media_api else {
        return Ok(());
    };
    let access_token = require_media_access_token(auth_session)?;
    media_api.delete(&access_token, object_key).await
}

fn require_media_access_token(auth_session: Option<AuthSession>) -> Result<String, String> {
    auth_session
        .map(|session| session.access_token)
        .filter(|token| !token.trim().is_empty())
        .ok_or_else(|| "Sign in is required for cloud attachment operations.".to_string())
}

fn build_media_object_key(note_id: &NoteId, file_name: &str) -> String {
    let stem = file_name
        .trim()
        .rsplit_once('.')
        .map_or_else(|| file_name.trim(), |(left, _)| left);
    let ext = file_name
        .trim()
        .rsplit_once('.')
        .map_or("", |(_, right)| right);

    let safe_stem = sanitize_media_token(stem);
    let safe_stem = if safe_stem.is_empty() {
        "file".to_string()
    } else {
        safe_stem
    };
    let safe_ext = sanitize_media_token(ext);
    let safe_name = if safe_ext.is_empty() {
        safe_stem
    } else {
        format!("{safe_stem}.{safe_ext}")
    };
    let now = chrono::Utc::now().timestamp_millis();
    format!("notes/{note_id}/{now}-{safe_name}")
}

fn sanitize_media_token(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut last_dash = false;

    for ch in input.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }

    out.trim_matches('-').to_string()
}

async fn list_attachments_with_retry(
    db: &DatabaseService,
    note_id: &NoteId,
) -> Result<Vec<Attachment>, dirt_core::Error> {
    let mut attempt = 0usize;

    loop {
        match db.list_attachments(note_id).await {
            Ok(attachments) => return Ok(attachments),
            Err(error) => {
                let should_retry = attempt + 1 < ATTACHMENT_LIST_MAX_ATTEMPTS
                    && is_retryable_attachment_list_error(&error);

                if !should_retry {
                    return Err(error);
                }

                attempt += 1;
                let delay_ms = ATTACHMENT_LIST_RETRY_DELAY_MS * u64::try_from(attempt).unwrap_or(1);
                tracing::warn!(
                    note_id = %note_id,
                    attempt,
                    "Transient attachment load failure, retrying in {}ms: {}",
                    delay_ms,
                    error
                );
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }
        }
    }
}

fn is_retryable_attachment_list_error(error: &dirt_core::Error) -> bool {
    let message = error.to_string().to_ascii_lowercase();
    message.contains("database is locked")
        || message.contains("database busy")
        || message.contains("temporarily unavailable")
}

fn infer_attachment_mime_type(content_type: Option<&str>, file_name: &str) -> String {
    let extension_guess = mime_guess::from_path(file_name)
        .first_raw()
        .map(str::to_string);

    if let Some(content_type) = content_type {
        let trimmed = content_type.trim();
        if !trimmed.is_empty() {
            let normalized = trimmed.to_ascii_lowercase();

            if normalized != "application/octet-stream"
                && !(normalized.starts_with("text/")
                    && extension_guess.as_deref().is_some_and(is_media_mime_type))
            {
                return trimmed.to_string();
            }
        }
    }

    extension_guess.unwrap_or_else(|| {
        mime_guess::from_path(file_name)
            .first_or_octet_stream()
            .essence_str()
            .to_string()
    })
}

fn render_preview_content(
    preview: AttachmentPreview,
    preview_title: &str,
    colors: &crate::theme::ColorPalette,
) -> Element {
    match preview {
        AttachmentPreview::None => rsx! {
            div {
                style: "color: {colors.text_muted};",
                "No preview available."
            }
        },
        AttachmentPreview::Text { content, truncated } => rsx! {
            div {
                if truncated {
                    div {
                        style: "font-size: 12px; color: {colors.text_muted}; margin-bottom: 8px;",
                        "Preview truncated to 256 KB"
                    }
                }
                pre {
                    style: "margin: 0; white-space: pre-wrap; word-break: break-word; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 12px; line-height: 1.5;",
                    "{content}"
                }
            }
        },
        AttachmentPreview::MediaDataUri {
            mime_type,
            data_uri,
        } => rsx! {
            if mime_type.starts_with("image/") {
                img {
                    src: "{data_uri}",
                    alt: "{preview_title}",
                    style: "display: block; max-width: 100%; max-height: 56vh; margin: 0 auto; border-radius: 6px;",
                }
            } else if mime_type.starts_with("video/") {
                video {
                    src: "{data_uri}",
                    controls: true,
                    style: "display: block; width: 100%; max-height: 56vh; border-radius: 6px;",
                }
            } else if mime_type.starts_with("audio/") {
                audio {
                    src: "{data_uri}",
                    controls: true,
                    style: "width: 100%;",
                }
            }
        },
        AttachmentPreview::Unsupported { mime_type, reason } => rsx! {
            div {
                style: "display: flex; flex-direction: column; gap: 6px; color: {colors.text_secondary};",
                div { "MIME type: {mime_type}" }
                div { "{reason}" }
            }
        },
    }
}

fn build_attachment_preview(file_name: &str, mime_type: &str, bytes: &[u8]) -> AttachmentPreview {
    match attachment_kind(file_name, mime_type) {
        AttachmentKind::Text => {
            let (content, truncated) = decode_text_preview(bytes);
            AttachmentPreview::Text { content, truncated }
        }
        AttachmentKind::Image | AttachmentKind::Video | AttachmentKind::Audio => {
            if bytes.len() > MAX_MEDIA_PREVIEW_BYTES {
                return AttachmentPreview::Unsupported {
                    mime_type: mime_type.to_string(),
                    reason: format!(
                        "Attachment is too large for in-app preview (limit: {}).",
                        format_attachment_size(file_size_i64(MAX_MEDIA_PREVIEW_BYTES))
                    ),
                };
            }

            let encoded = BASE64_STANDARD.encode(bytes);
            AttachmentPreview::MediaDataUri {
                mime_type: mime_type.to_string(),
                data_uri: format!("data:{mime_type};base64,{encoded}"),
            }
        }
        AttachmentKind::File => AttachmentPreview::Unsupported {
            mime_type: mime_type.to_string(),
            reason: "This file type does not have an in-app preview yet.".to_string(),
        },
    }
}

fn decode_text_preview(bytes: &[u8]) -> (String, bool) {
    if bytes.len() <= MAX_TEXT_PREVIEW_BYTES {
        return (String::from_utf8_lossy(bytes).to_string(), false);
    }

    let content = String::from_utf8_lossy(&bytes[..MAX_TEXT_PREVIEW_BYTES]).to_string();
    (content, true)
}

fn attachment_kind(file_name: &str, mime_type: &str) -> AttachmentKind {
    let normalized_mime = infer_attachment_mime_type(Some(mime_type), file_name);
    if normalized_mime.starts_with("image/") {
        AttachmentKind::Image
    } else if normalized_mime.starts_with("video/") {
        AttachmentKind::Video
    } else if normalized_mime.starts_with("audio/") {
        AttachmentKind::Audio
    } else if normalized_mime.starts_with("text/") {
        AttachmentKind::Text
    } else {
        AttachmentKind::File
    }
}

fn is_media_mime_type(mime_type: &str) -> bool {
    mime_type.starts_with("image/")
        || mime_type.starts_with("video/")
        || mime_type.starts_with("audio/")
}

fn attachment_kind_label(file_name: &str, mime_type: &str) -> &'static str {
    match attachment_kind(file_name, mime_type) {
        AttachmentKind::Image => "image",
        AttachmentKind::Video => "video",
        AttachmentKind::Audio => "audio",
        AttachmentKind::Text => "text",
        AttachmentKind::File => "file",
    }
}

fn format_attachment_size(size_bytes: i64) -> String {
    let bytes = u64::try_from(size_bytes).unwrap_or(0);

    if bytes < KIB_BYTES {
        format!("{bytes} B")
    } else if bytes < MIB_BYTES {
        format_scaled_one_decimal(bytes, KIB_BYTES, "KB")
    } else if bytes < GIB_BYTES {
        format_scaled_one_decimal(bytes, MIB_BYTES, "MB")
    } else {
        format_scaled_one_decimal(bytes, GIB_BYTES, "GB")
    }
}

fn format_scaled_one_decimal(bytes: u64, unit: u64, suffix: &str) -> String {
    let mut whole = bytes / unit;
    let mut tenth = ((bytes % unit) * 10 + (unit / 2)) / unit;

    if tenth == 10 {
        whole += 1;
        tenth = 0;
    }

    format!("{whole}.{tenth} {suffix}")
}

fn format_recording_duration(duration_ms: u64) -> String {
    let total_seconds = duration_ms / 1_000;
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    format!("{minutes:02}:{seconds:02}")
}

fn elapsed_millis_u64(started_at: Instant) -> u64 {
    u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn file_size_i64(len: usize) -> i64 {
    i64::try_from(len).map_or(i64::MAX, |size| size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infers_attachment_mime_type_with_fallback() {
        assert_eq!(
            infer_attachment_mime_type(Some("image/gif"), "x.bin"),
            "image/gif"
        );
        assert_eq!(
            infer_attachment_mime_type(Some("application/octet-stream"), "file.pdf"),
            "application/pdf"
        );
        assert_eq!(
            infer_attachment_mime_type(Some("text/plain"), "photo.png"),
            "image/png"
        );
        assert_eq!(infer_attachment_mime_type(None, "photo.jpg"), "image/jpeg");
        assert_eq!(
            infer_attachment_mime_type(None, "unknown.bin"),
            "application/octet-stream"
        );
    }

    #[test]
    fn maps_attachment_kind_labels_by_mime_or_extension() {
        assert_eq!(attachment_kind_label("photo.png", "image/png"), "image");
        assert_eq!(attachment_kind_label("voice.wav", "audio/wav"), "audio");
        assert_eq!(attachment_kind_label("clip.mp4", "video/mp4"), "video");
        assert_eq!(attachment_kind_label("readme.md", "text/plain"), "text");
        assert_eq!(attachment_kind_label("photo.png", "text/plain"), "image");
        assert_eq!(
            attachment_kind_label("archive.zip", "application/zip"),
            "file"
        );
    }

    #[test]
    fn builds_preview_for_text_and_media() {
        let text_preview = build_attachment_preview("notes.txt", "text/plain", b"hello");
        assert!(matches!(
            text_preview,
            AttachmentPreview::Text {
                content,
                truncated: false,
            } if content == "hello"
        ));

        let image_preview = build_attachment_preview("photo.png", "image/png", b"abc");
        assert!(matches!(
            image_preview,
            AttachmentPreview::MediaDataUri { .. }
        ));
    }

    #[test]
    fn previews_large_media_as_unsupported() {
        let bytes = vec![0_u8; MAX_MEDIA_PREVIEW_BYTES + 1];
        let preview = build_attachment_preview("movie.mp4", "video/mp4", &bytes);
        assert!(matches!(preview, AttachmentPreview::Unsupported { .. }));
    }

    #[test]
    fn truncates_large_text_preview() {
        let bytes = vec![b'a'; MAX_TEXT_PREVIEW_BYTES + 12];
        let (content, truncated) = decode_text_preview(&bytes);
        assert!(truncated);
        assert_eq!(content.len(), MAX_TEXT_PREVIEW_BYTES);
    }

    #[test]
    fn formats_attachment_sizes_for_ui() {
        assert_eq!(format_attachment_size(512), "512 B");
        assert_eq!(format_attachment_size(1_536), "1.5 KB");
        assert_eq!(format_attachment_size(2_097_152), "2.0 MB");
        assert_eq!(format_attachment_size(-64), "0 B");
    }

    #[test]
    fn formats_recording_duration_as_minutes_and_seconds() {
        assert_eq!(format_recording_duration(0), "00:00");
        assert_eq!(format_recording_duration(12_345), "00:12");
        assert_eq!(format_recording_duration(120_000), "02:00");
    }
}
