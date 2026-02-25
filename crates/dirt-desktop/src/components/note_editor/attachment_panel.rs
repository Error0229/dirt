use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use dioxus::html::HasFileData;
use dioxus::prelude::*;
use rfd::AsyncFileDialog;

use dirt_core::models::{Attachment, AttachmentId};
use dirt_core::NoteId;

use super::attachment_preview::{
    attachment_kind_label, format_attachment_size, render_preview_content, AttachmentPreview,
};
use super::attachment_utils::{
    delete_remote_attachment, list_attachments_with_retry, load_attachment_preview,
    upload_attachment, UploadContext, UploadSignals,
};
use super::transcription::{
    apply_voice_memo_transcription_if_enabled, elapsed_millis_u64, format_recording_duration,
    VoiceMemoTranscriptionContext,
};
use crate::components::button::{Button, ButtonVariant};
use crate::components::dialog::{DialogContent, DialogDescription, DialogRoot, DialogTitle};
use crate::services::{
    cleanup_temp_voice_memo, discard_voice_memo_recording, start_voice_memo_recording,
    stop_voice_memo_recording, transition_voice_memo_state, VoiceMemoRecorderEvent,
    VoiceMemoRecorderState,
};
use crate::state::AppState;

#[component]
pub(super) fn AttachmentPanel(
    note_id: Option<NoteId>,
    editor_content: String,
    on_editor_content_change: EventHandler<String>,
) -> Element {
    let state = use_context::<AppState>();
    let colors = (state.theme)().palette();

    let mut last_note_id = use_signal(|| None::<NoteId>);
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

    use_effect(move || {
        if note_id != *last_note_id.read() {
            deleting_attachment_id.set(None);
            let recorder_state = voice_memo_state();
            let should_discard = matches!(
                recorder_state,
                VoiceMemoRecorderState::Starting | VoiceMemoRecorderState::Recording
            );
            if recorder_state != VoiceMemoRecorderState::Stopping {
                voice_memo_state.set(VoiceMemoRecorderState::Idle);
                voice_memo_started_at.set(None);
            }
            if should_discard {
                spawn(async move {
                    let _ = discard_voice_memo_recording().await;
                });
            }
            last_note_id.set(note_id);
        }
    });

    use_effect(move || {
        let _refresh = attachment_refresh_version();
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

        let Some(note_id) = note_id else {
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

            let _ = upload_attachment(
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

        let Some(note_id) = note_id else {
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
            let _ = upload_attachment(
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
        let Some(_note_id) = note_id else {
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
        let Some(note_id) = note_id else {
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
        let transcription_enabled = (state.settings)().voice_memo_transcription_enabled;
        let transcription_service = state.transcription_service.read().clone();
        let db_for_transcription = state.db_service.read().clone();
        let current_note_id = (state.current_note_id)();
        let editor_content = editor_content.clone();
        let on_editor_content_change = on_editor_content_change;

        spawn(async move {
            match stop_voice_memo_recording().await {
                Ok(recorded) => {
                    let file_name = recorded.file_name.clone();
                    let mime_type = recorded.mime_type.clone();
                    let audio_bytes = recorded.bytes.clone();
                    let upload_succeeded = upload_attachment(
                        note_id,
                        file_name.clone(),
                        Some(mime_type.clone()),
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

                    if transcription_enabled && upload_succeeded {
                        apply_voice_memo_transcription_if_enabled(
                            note_id,
                            file_name.as_str(),
                            mime_type.as_str(),
                            audio_bytes,
                            transcription_service,
                            db_for_transcription,
                            VoiceMemoTranscriptionContext {
                                current_note_id,
                                editor_content,
                                on_editor_content_change,
                                upload_error: attachment_upload_error,
                            },
                        )
                        .await;
                    }
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
        if note_id.is_some() {
            if attachment_uploading() {
                div {
                    style: "margin-bottom: 8px; color: {colors.text_muted}; font-size: 12px;",
                    "Uploading attachment..."
                }
            }

            if let Some(status) = &voice_memo_status {
                div {
                    style: "margin-bottom: 8px; color: {colors.text_muted}; font-size: 12px;",
                    "{status}"
                }
            }

            if let Some(error) = attachment_upload_error() {
                div {
                    style: "margin-bottom: 8px; color: {colors.error}; font-size: 12px;",
                    "{error}"
                }
            }

            if let Some(error) = attachments_error() {
                div {
                    style: "margin-bottom: 8px; color: {colors.error}; font-size: 12px;",
                    "{error}"
                }
            }

            div {
                style: "
                    margin-top: 8px;
                    border-top: 1px solid {colors.border};
                    padding-top: 8px;
                    border: 1px dashed {border_color};
                    border-radius: 8px;
                    display: flex;
                    flex-direction: column;
                    gap: 6px;
                    min-width: 0;
                    overflow-x: hidden;
                ",
                ondragover: on_drag_over,
                ondragleave: on_drag_leave,
                ondrop: on_drop_attachment,

                div {
                    style: "display: flex; align-items: center; justify-content: space-between; gap: 12px;",
                    div {
                        style: "font-size: 12px; color: {colors.text_muted}; text-transform: uppercase; letter-spacing: 0.04em;",
                        "Attachments"
                    }

                    div {
                        style: "display: flex; align-items: center; gap: 8px;",
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
                                "Finalizing..."
                            }
                        }
                    }
                }

                if attachments_loading() {
                    div {
                        style: "font-size: 12px; color: {colors.text_muted};",
                        "Loading attachments..."
                    }
                } else if attachment_items.is_empty() {
                    div {
                        style: "font-size: 12px; color: {colors.text_muted};",
                        "No attachments yet"
                    }
                } else {
                    for attachment in attachment_items {
                        div {
                            key: "{attachment.id}",
                            style: "display: grid; grid-template-columns: minmax(0, 1fr) auto auto; align-items: center; column-gap: 12px; min-width: 0; width: 100%; overflow: hidden; font-size: 12px;",
                            div {
                                style: "min-width: 0; overflow: hidden; display: flex; align-items: baseline; gap: 8px; color: {colors.text_primary};",
                                span {
                                    style: "display: block; flex: 1 1 auto; min-width: 0; max-width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;",
                                    "{attachment.filename}"
                                }
                                span {
                                    style: "color: {colors.text_muted}; white-space: nowrap; flex-shrink: 0;",
                                    "{attachment_kind_label(&attachment.filename, &attachment.mime_type)}"
                                }
                            }
                            span {
                                style: "color: {colors.text_muted}; white-space: nowrap; flex-shrink: 0;",
                                "{format_attachment_size(attachment.size_bytes)}"
                            }
                            div {
                                style: "display: flex; align-items: center; gap: 6px; flex-shrink: 0;",
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
                                                match load_attachment_preview(&attachment, media_api, auth_session).await {
                                                    Ok(preview) => preview_content_signal.set(preview),
                                                    Err(error) => preview_error_signal.set(Some(error)),
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
                                                    ).await {
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
                        style: "border: 1px solid {colors.border}; border-radius: 8px; padding: 12px; min-height: 180px; max-height: 60vh; overflow: auto; background: {colors.bg_secondary}; color: {colors.text_primary};",
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
        }
    }
}
