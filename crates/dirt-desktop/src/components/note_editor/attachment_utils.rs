use std::sync::Arc;
use std::time::Duration;

use dioxus::prelude::*;

use dirt_core::models::Attachment;
use dirt_core::NoteId;

use super::attachment_preview::{build_attachment_preview, AttachmentPreview};
use crate::services::{AuthSession, DatabaseService, MediaApiClient};

const ATTACHMENT_LIST_MAX_ATTEMPTS: usize = 3;
const ATTACHMENT_LIST_RETRY_DELAY_MS: u64 = 120;

#[derive(Clone, Copy)]
pub(super) struct UploadSignals {
    pub uploading: Signal<bool>,
    pub upload_error: Signal<Option<String>>,
    pub attachment_refresh_signal: Signal<u64>,
}

#[derive(Clone)]
pub(super) struct UploadContext {
    pub db: Option<Arc<DatabaseService>>,
    pub media_api: Option<Arc<MediaApiClient>>,
    pub auth_session: Option<AuthSession>,
    pub signals: UploadSignals,
}

pub(super) async fn upload_attachment(
    note_id: NoteId,
    file_name: String,
    file_content_type: Option<String>,
    file_bytes: Vec<u8>,
    context: UploadContext,
) -> bool {
    let mut uploading = context.signals.uploading;
    let mut upload_error = context.signals.upload_error;
    let mut attachment_refresh_signal = context.signals.attachment_refresh_signal;

    uploading.set(true);

    let Some(db) = context.db else {
        upload_error.set(Some("Database service is not available.".to_string()));
        uploading.set(false);
        return false;
    };

    let Some(media_api) = context.media_api else {
        upload_error.set(Some(
            "Cloud media is not configured for this build.".to_string(),
        ));
        uploading.set(false);
        return false;
    };
    let access_token = match require_media_access_token(context.auth_session) {
        Ok(token) => token,
        Err(error) => {
            upload_error.set(Some(error));
            uploading.set(false);
            return false;
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
        return false;
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
        return false;
    }

    attachment_refresh_signal.set(attachment_refresh_signal() + 1);
    uploading.set(false);
    true
}

pub(super) async fn load_attachment_preview(
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

pub(super) async fn delete_remote_attachment(
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

pub(super) fn require_media_access_token(
    auth_session: Option<AuthSession>,
) -> Result<String, String> {
    auth_session
        .map(|session| session.access_token)
        .filter(|token| !token.trim().is_empty())
        .ok_or_else(|| "Sign in is required for cloud attachment operations.".to_string())
}

pub(super) fn build_media_object_key(note_id: &NoteId, file_name: &str) -> String {
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

pub(super) fn sanitize_media_token(input: &str) -> String {
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

pub(super) async fn list_attachments_with_retry(
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

pub(super) fn is_retryable_attachment_list_error(error: &dirt_core::Error) -> bool {
    let message = error.to_string().to_ascii_lowercase();
    message.contains("database is locked")
        || message.contains("database busy")
        || message.contains("temporarily unavailable")
}

pub(super) fn infer_attachment_mime_type(content_type: Option<&str>, file_name: &str) -> String {
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

fn is_media_mime_type(mime_type: &str) -> bool {
    mime_type.starts_with("image/")
        || mime_type.starts_with("video/")
        || mime_type.starts_with("audio/")
}

pub(super) fn file_size_i64(len: usize) -> i64 {
    i64::try_from(len).map_or(i64::MAX, |size| size)
}
