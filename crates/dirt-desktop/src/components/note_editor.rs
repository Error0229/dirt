//! Note editor component

use std::fmt::Write as _;
use std::time::Duration;

use dioxus::html::HasFileData;
use dioxus::prelude::*;

use dirt_core::models::{Attachment, AttachmentId};
use dirt_core::storage::{MediaStorage, R2Config, R2Storage};
use dirt_core::NoteId;

use crate::queries::invalidate_notes_query;
use crate::services::TranscriptionService;
use crate::state::AppState;

/// Idle save delay - save after 2 seconds of no typing
const IDLE_SAVE_MS: u64 = 2000;
const KIB_BYTES: u64 = 1024;
const MIB_BYTES: u64 = KIB_BYTES * 1024;
const GIB_BYTES: u64 = MIB_BYTES * 1024;

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
    let attachments_error = use_signal(|| None::<String>);
    let attachments_loading = use_signal(|| false);
    let attachment_refresh_version = use_signal(|| 0u64);
    let mut deleting_attachment_id = use_signal(|| None::<AttachmentId>);
    let mut drag_over = use_signal(|| false);

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
        }
    });

    use_effect(move || {
        let note_id = *last_note_id.read();
        let _attachment_refresh_version = attachment_refresh_version();
        let db = state.db_service.read().clone();
        let mut attachment_signal = attachments;
        let mut attachment_error_signal = attachments_error;
        let mut attachment_loading_signal = attachments_loading;

        spawn(async move {
            attachment_error_signal.set(None);

            let Some(note_id) = note_id else {
                attachment_signal.set(Vec::new());
                attachment_loading_signal.set(false);
                return;
            };

            let Some(db) = db else {
                attachment_signal.set(Vec::new());
                attachment_error_signal.set(Some("Database service is not available.".to_string()));
                attachment_loading_signal.set(false);
                return;
            };

            attachment_loading_signal.set(true);
            match db.list_attachments(&note_id).await {
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
        let mut uploading = attachment_uploading;
        let mut content_signal = content;
        let mut version_signal = save_version;
        let mut saved_version_signal = last_saved_version;
        let mut attachment_refresh_signal = attachment_refresh_version;
        let db = state.db_service.read().clone();

        spawn(async move {
            uploading.set(true);

            let Some(db) = db else {
                upload_error.set(Some("Database service is not available.".to_string()));
                uploading.set(false);
                return;
            };

            let file_bytes = match file.read_bytes().await {
                Ok(bytes) => bytes,
                Err(error) => {
                    upload_error.set(Some(format!("Failed to read dropped file: {error}")));
                    uploading.set(false);
                    return;
                }
            };

            let config = match R2Config::from_env() {
                Ok(Some(config)) => config,
                Ok(None) => {
                    upload_error.set(Some(
                        "R2 is not configured. Set R2 env vars before uploading attachments."
                            .to_string(),
                    ));
                    uploading.set(false);
                    return;
                }
                Err(error) => {
                    upload_error.set(Some(format!("Invalid R2 configuration: {error}")));
                    uploading.set(false);
                    return;
                }
            };

            let storage = R2Storage::new(config);
            let object_key = match storage.build_media_key(&note_id.to_string(), &file_name) {
                Ok(key) => key,
                Err(error) => {
                    upload_error.set(Some(format!("Failed to build media key: {error}")));
                    uploading.set(false);
                    return;
                }
            };

            let mime_type = infer_attachment_mime_type(file_content_type.as_deref(), &file_name);

            if let Err(error) = storage
                .upload_bytes(&object_key, file_bytes.as_ref(), Some(&mime_type))
                .await
            {
                upload_error.set(Some(format!("Failed to upload attachment to R2: {error}")));
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

            let image_url = storage
                .public_object_url(&object_key)
                .unwrap_or_else(|| format!("r2://{object_key}"));
            let transcript_markdown = if is_wav_attachment(&mime_type, &file_name) {
                match TranscriptionService::new_from_env() {
                    Ok(service) if service.config_status().enabled => {
                        match service
                            .transcribe_wav_bytes(&file_name, file_bytes.to_vec())
                            .await
                        {
                            Ok(transcript) => build_transcript_markdown(&transcript),
                            Err(error) => {
                                upload_error.set(Some(format!(
                                    "Attachment uploaded, but transcription failed: {error}"
                                )));
                                None
                            }
                        }
                    }
                    Ok(_) => None,
                    Err(error) => {
                        upload_error.set(Some(format!(
                            "Attachment uploaded, but transcription config is invalid: {error}"
                        )));
                        None
                    }
                }
            } else {
                None
            };

            let mut updated_content = content_signal.read().clone();
            if !updated_content.is_empty() && !updated_content.ends_with('\n') {
                updated_content.push('\n');
            }
            let attachment_markdown = build_attachment_markdown(&file_name, &image_url, &mime_type);
            let _ = write!(updated_content, "{attachment_markdown}");
            if let Some(transcript_markdown) = transcript_markdown {
                let _ = write!(updated_content, "\n\n{transcript_markdown}");
            }

            content_signal.set(updated_content.clone());
            version_signal.set(version_signal() + 1);

            let current_version = version_signal();
            match db.update_note(&note_id, &updated_content).await {
                Ok(_) => {
                    saved_version_signal.set(current_version);
                    invalidate_notes_query().await;
                }
                Err(error) => {
                    upload_error.set(Some(format!(
                        "Attachment uploaded but note update failed: {error}"
                    )));
                }
            }

            uploading.set(false);
        });
    };

    let border_color = if drag_over() {
        colors.accent
    } else {
        "transparent"
    };
    let attachment_items = attachments();
    let active_deleting_attachment = deleting_attachment_id();

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

                if let Some(error) = attachment_upload_error() {
                    div {
                        style: "
                            margin-bottom: 8px;
                            color: {colors.accent};
                            font-size: 12px;
                        ",
                        "{error}"
                    }
                }

                if let Some(error) = attachments_error() {
                    div {
                        style: "
                            margin-bottom: 8px;
                            color: {colors.accent};
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
                        for attachment in attachment_items.clone() {
                            div {
                                key: "{attachment.id}",
                                style: "
                                    display: flex;
                                    align-items: center;
                                    justify-content: space-between;
                                    gap: 12px;
                                    font-size: 12px;
                                ",
                                div {
                                    style: "
                                        min-width: 0;
                                        display: flex;
                                        align-items: baseline;
                                        gap: 8px;
                                        color: {colors.text_primary};
                                    ",
                                    span {
                                        style: "
                                            flex: 1;
                                            min-width: 0;
                                            overflow: hidden;
                                            text-overflow: ellipsis;
                                            white-space: nowrap;
                                        ",
                                        "{attachment.filename}"
                                    }
                                    span {
                                        style: "
                                            color: {colors.text_muted};
                                        ",
                                        "{attachment_kind_label(&attachment.mime_type)}"
                                    }
                                }
                                span {
                                    style: "
                                        color: {colors.text_muted};
                                        white-space: nowrap;
                                    ",
                                    "{format_attachment_size(attachment.size_bytes)}"
                                }
                                button {
                                    style: "
                                        border: 1px solid {colors.border};
                                        border-radius: 6px;
                                        background: transparent;
                                        color: {colors.text_muted};
                                        font-size: 11px;
                                        padding: 2px 8px;
                                        cursor: pointer;
                                    ",
                                    disabled: active_deleting_attachment == Some(attachment.id),
                                    onclick: move |_| {
                                        let mut deleting_signal = deleting_attachment_id;
                                        let mut attachment_error_signal = attachments_error;
                                        let mut refresh_signal = attachment_refresh_version;
                                        let db = state.db_service.read().clone();
                                        let attachment_id = attachment.id;

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

fn infer_attachment_mime_type(content_type: Option<&str>, file_name: &str) -> String {
    if let Some(content_type) = content_type {
        let trimmed = content_type.trim();
        if !trimmed.is_empty() && !trimmed.eq_ignore_ascii_case("application/octet-stream") {
            return trimmed.to_string();
        }
    }

    mime_guess::from_path(file_name)
        .first_or_octet_stream()
        .essence_str()
        .to_string()
}

fn build_attachment_markdown(file_name: &str, url: &str, mime_type: &str) -> String {
    if mime_type.starts_with("image/") {
        format!("![{file_name}]({url})")
    } else {
        format!("[{file_name}]({url})")
    }
}

fn is_wav_attachment(mime_type: &str, file_name: &str) -> bool {
    mime_type.eq_ignore_ascii_case("audio/wav")
        || mime_type.eq_ignore_ascii_case("audio/x-wav")
        || file_name
            .rsplit('.')
            .next()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("wav"))
}

fn build_transcript_markdown(transcript: &str) -> Option<String> {
    let trimmed = transcript.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut markdown = String::from("**Transcript**\n");
    for line in trimmed.lines() {
        let line = line.trim();
        if line.is_empty() {
            markdown.push_str(">\n");
        } else {
            markdown.push_str("> ");
            markdown.push_str(line);
            markdown.push('\n');
        }
    }
    Some(markdown.trim_end().to_string())
}

fn attachment_kind_label(mime_type: &str) -> &'static str {
    if mime_type.starts_with("image/") {
        "image"
    } else if mime_type.starts_with("audio/") {
        "audio"
    } else if mime_type.starts_with("video/") {
        "video"
    } else if mime_type.starts_with("text/") {
        "text"
    } else {
        "file"
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
        assert_eq!(infer_attachment_mime_type(None, "photo.jpg"), "image/jpeg");
        assert_eq!(
            infer_attachment_mime_type(None, "unknown.bin"),
            "application/octet-stream"
        );
    }

    #[test]
    fn builds_image_markdown_for_image_mime_types() {
        assert_eq!(
            build_attachment_markdown("photo.png", "https://example.test/photo.png", "image/png"),
            "![photo.png](https://example.test/photo.png)"
        );
    }

    #[test]
    fn builds_link_markdown_for_non_image_mime_types() {
        assert_eq!(
            build_attachment_markdown(
                "notes.pdf",
                "https://example.test/notes.pdf",
                "application/pdf"
            ),
            "[notes.pdf](https://example.test/notes.pdf)"
        );
    }

    #[test]
    fn formats_attachment_sizes_for_ui() {
        assert_eq!(format_attachment_size(512), "512 B");
        assert_eq!(format_attachment_size(1_536), "1.5 KB");
        assert_eq!(format_attachment_size(2_097_152), "2.0 MB");
        assert_eq!(format_attachment_size(-64), "0 B");
    }

    #[test]
    fn maps_attachment_kind_labels_by_mime() {
        assert_eq!(attachment_kind_label("image/png"), "image");
        assert_eq!(attachment_kind_label("audio/wav"), "audio");
        assert_eq!(attachment_kind_label("video/mp4"), "video");
        assert_eq!(attachment_kind_label("text/plain"), "text");
        assert_eq!(attachment_kind_label("application/pdf"), "file");
    }

    #[test]
    fn detects_wav_attachments_by_mime_or_extension() {
        assert!(is_wav_attachment("audio/wav", "memo.bin"));
        assert!(is_wav_attachment("audio/x-wav", "memo.bin"));
        assert!(is_wav_attachment("application/octet-stream", "memo.wav"));
        assert!(!is_wav_attachment("audio/mpeg", "memo.mp3"));
    }

    #[test]
    fn builds_transcript_markdown_when_text_exists() {
        assert_eq!(
            build_transcript_markdown("first line\nsecond line"),
            Some("**Transcript**\n> first line\n> second line".to_string())
        );
        assert_eq!(build_transcript_markdown("   "), None);
    }
}
