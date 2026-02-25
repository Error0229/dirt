use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use dioxus::prelude::*;

use super::attachment_utils::{file_size_i64, infer_attachment_mime_type};
use crate::theme::ColorPalette;

const KIB_BYTES: u64 = 1024;
const MIB_BYTES: u64 = KIB_BYTES * 1024;
const GIB_BYTES: u64 = MIB_BYTES * 1024;
const MAX_TEXT_PREVIEW_BYTES: usize = 256 * 1024;
const MAX_MEDIA_PREVIEW_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum AttachmentKind {
    Image,
    Video,
    Audio,
    Text,
    File,
}

#[derive(Clone, PartialEq, Eq, Default)]
pub(super) enum AttachmentPreview {
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

pub(super) fn render_preview_content(
    preview: AttachmentPreview,
    preview_title: &str,
    colors: &ColorPalette,
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

pub(super) fn build_attachment_preview(
    file_name: &str,
    mime_type: &str,
    bytes: &[u8],
) -> AttachmentPreview {
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

pub(super) fn decode_text_preview(bytes: &[u8]) -> (String, bool) {
    if bytes.len() <= MAX_TEXT_PREVIEW_BYTES {
        return (String::from_utf8_lossy(bytes).to_string(), false);
    }

    let content = String::from_utf8_lossy(&bytes[..MAX_TEXT_PREVIEW_BYTES]).to_string();
    (content, true)
}

pub(super) fn attachment_kind(file_name: &str, mime_type: &str) -> AttachmentKind {
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

pub(super) fn attachment_kind_label(file_name: &str, mime_type: &str) -> &'static str {
    match attachment_kind(file_name, mime_type) {
        AttachmentKind::Image => "image",
        AttachmentKind::Video => "video",
        AttachmentKind::Audio => "audio",
        AttachmentKind::Text => "text",
        AttachmentKind::File => "file",
    }
}

pub(super) fn format_attachment_size(size_bytes: i64) -> String {
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
