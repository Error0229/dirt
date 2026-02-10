//! Attachment helpers for mobile upload/preview UX.

use base64::prelude::{Engine as _, BASE64_STANDARD};

pub const MAX_TEXT_PREVIEW_BYTES: usize = 256 * 1024;
pub const MAX_MEDIA_PREVIEW_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub enum AttachmentPreview {
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum AttachmentKind {
    Image,
    Audio,
    Video,
    Text,
    File,
}

#[must_use]
pub fn infer_attachment_mime_type(content_type: Option<&str>, file_name: &str) -> String {
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

#[must_use]
pub fn attachment_kind_label(file_name: &str, mime_type: &str) -> &'static str {
    match attachment_kind(file_name, mime_type) {
        AttachmentKind::Image => "image",
        AttachmentKind::Audio => "audio",
        AttachmentKind::Video => "video",
        AttachmentKind::Text => "text",
        AttachmentKind::File => "file",
    }
}

#[must_use]
pub fn build_attachment_preview(
    file_name: &str,
    mime_type: &str,
    bytes: &[u8],
) -> AttachmentPreview {
    match attachment_kind(file_name, mime_type) {
        AttachmentKind::Text => {
            let (content, truncated) = decode_text_preview(bytes);
            AttachmentPreview::Text { content, truncated }
        }
        AttachmentKind::Image | AttachmentKind::Audio | AttachmentKind::Video => {
            if bytes.len() > MAX_MEDIA_PREVIEW_BYTES {
                return AttachmentPreview::Unsupported {
                    mime_type: mime_type.to_string(),
                    reason: "Attachment is too large for in-app preview.".to_string(),
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

    let preview = String::from_utf8_lossy(&bytes[..MAX_TEXT_PREVIEW_BYTES]).to_string();
    (preview, true)
}

fn attachment_kind(file_name: &str, mime_type: &str) -> AttachmentKind {
    let normalized_mime = infer_attachment_mime_type(Some(mime_type), file_name);
    if normalized_mime.starts_with("image/") {
        AttachmentKind::Image
    } else if normalized_mime.starts_with("audio/") {
        AttachmentKind::Audio
    } else if normalized_mime.starts_with("video/") {
        AttachmentKind::Video
    } else if normalized_mime.starts_with("text/") {
        AttachmentKind::Text
    } else {
        AttachmentKind::File
    }
}

fn is_media_mime_type(mime_type: &str) -> bool {
    mime_type.starts_with("image/")
        || mime_type.starts_with("audio/")
        || mime_type.starts_with("video/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mime_inference_prefers_media_extension_over_generic_text() {
        assert_eq!(
            infer_attachment_mime_type(Some("text/plain"), "photo.png"),
            "image/png"
        );
        assert_eq!(
            infer_attachment_mime_type(Some("application/octet-stream"), "report.pdf"),
            "application/pdf"
        );
    }

    #[test]
    fn kind_label_handles_real_media_types() {
        assert_eq!(attachment_kind_label("photo.png", "image/png"), "image");
        assert_eq!(attachment_kind_label("voice.wav", "audio/wav"), "audio");
        assert_eq!(attachment_kind_label("clip.mp4", "video/mp4"), "video");
        assert_eq!(attachment_kind_label("readme.md", "text/plain"), "text");
        assert_eq!(
            attachment_kind_label("archive.zip", "application/zip"),
            "file"
        );
    }

    #[test]
    fn builds_text_and_media_previews() {
        let text = build_attachment_preview("notes.txt", "text/plain", b"hello");
        assert!(matches!(
            text,
            AttachmentPreview::Text {
                content,
                truncated: false
            } if content == "hello"
        ));

        let image = build_attachment_preview("photo.png", "image/png", b"abc");
        assert!(matches!(image, AttachmentPreview::MediaDataUri { .. }));
    }

    #[test]
    fn marks_large_media_as_unsupported() {
        let bytes = vec![0_u8; MAX_MEDIA_PREVIEW_BYTES + 1];
        let preview = build_attachment_preview("video.mp4", "video/mp4", &bytes);
        assert!(matches!(preview, AttachmentPreview::Unsupported { .. }));
    }
}
