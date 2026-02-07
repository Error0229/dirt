//! Attachment model

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use uuid::Uuid;

use crate::error::{Error, Result};

use super::note::NoteId;

/// A unique identifier for an attachment, using UUID v7.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AttachmentId(Uuid);

impl AttachmentId {
    /// Create a new unique attachment ID using UUID v7.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }

    /// Get the string representation of this ID.
    #[must_use]
    pub fn as_str(&self) -> String {
        self.0.to_string()
    }
}

impl Default for AttachmentId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for AttachmentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for AttachmentId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

/// Attachment metadata persisted for a note.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    /// Unique attachment identifier.
    pub id: AttachmentId,
    /// Parent note identifier.
    pub note_id: NoteId,
    /// Original file name.
    pub filename: String,
    /// Content MIME type.
    pub mime_type: String,
    /// Attachment size in bytes.
    pub size_bytes: i64,
    /// Cloudflare R2 object key.
    pub r2_key: String,
    /// Creation timestamp (Unix ms).
    pub created_at: i64,
    /// Soft delete flag for sync.
    pub is_deleted: bool,
}

impl Attachment {
    /// Create a new attachment metadata record.
    pub fn new(
        note_id: NoteId,
        filename: impl Into<String>,
        mime_type: impl Into<String>,
        size_bytes: i64,
        r2_key: impl Into<String>,
    ) -> Result<Self> {
        let filename = filename.into().trim().to_string();
        let mime_type = mime_type.into().trim().to_string();
        let r2_key = r2_key.into().trim().to_string();

        if filename.is_empty() {
            return Err(Error::InvalidInput(
                "Attachment filename cannot be empty".to_string(),
            ));
        }
        if mime_type.is_empty() {
            return Err(Error::InvalidInput(
                "Attachment mime_type cannot be empty".to_string(),
            ));
        }
        if r2_key.is_empty() {
            return Err(Error::InvalidInput(
                "Attachment r2_key cannot be empty".to_string(),
            ));
        }
        if size_bytes < 0 {
            return Err(Error::InvalidInput(
                "Attachment size_bytes cannot be negative".to_string(),
            ));
        }

        Ok(Self {
            id: AttachmentId::new(),
            note_id,
            filename,
            mime_type,
            size_bytes,
            r2_key,
            created_at: chrono::Utc::now().timestamp_millis(),
            is_deleted: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attachment_id_unique() {
        let id1 = AttachmentId::new();
        let id2 = AttachmentId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_attachment_id_parse() {
        let id = AttachmentId::new();
        let parsed: AttachmentId = id.as_str().parse().unwrap();
        assert_eq!(id, parsed);
    }

    #[test]
    fn test_attachment_new() {
        let attachment = Attachment::new(
            NoteId::new(),
            "image.png",
            "image/png",
            1234,
            "notes/note/image.png",
        )
        .unwrap();

        assert_eq!(attachment.filename, "image.png");
        assert_eq!(attachment.mime_type, "image/png");
        assert_eq!(attachment.size_bytes, 1234);
        assert_eq!(attachment.r2_key, "notes/note/image.png");
        assert!(!attachment.is_deleted);
    }

    #[test]
    fn test_attachment_validation() {
        let note_id = NoteId::new();

        assert!(Attachment::new(note_id, "", "image/png", 1, "key").is_err());
        assert!(Attachment::new(note_id, "file", "", 1, "key").is_err());
        assert!(Attachment::new(note_id, "file", "image/png", 1, "").is_err());
        assert!(Attachment::new(note_id, "file", "image/png", -1, "key").is_err());
    }
}
