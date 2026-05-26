//! Note model

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt;
use std::str::FromStr;
use std::sync::OnceLock;

/// Returns the pre-compiled regex for extracting `#tag` patterns from note content.
fn tag_regex() -> &'static Regex {
    static TAG_REGEX: OnceLock<Regex> = OnceLock::new();
    TAG_REGEX.get_or_init(|| Regex::new(r"#([a-zA-Z][a-zA-Z0-9_-]*)").expect("Invalid regex"))
}
use uuid::Uuid;

use crate::error::Error;

/// A unique identifier for a note, using UUID v7 (time-sortable)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NoteId(Uuid);

impl NoteId {
    /// Create a new unique note ID using UUID v7
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }

    /// Get the string representation of this ID
    #[must_use]
    pub fn as_str(&self) -> String {
        self.0.to_string()
    }
}

impl Default for NoteId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for NoteId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for NoteId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

/// A note in the system.
///
/// `updated_at` is the client's wall clock at the moment of a local mutation,
/// kept as a hint for the UI (sorting, "edited X ago"). `server_updated_at`
/// is the authoritative timestamp stamped by the server on accept; it drives
/// pull cursors and conflict resolution. `deleted_at` doubles as a tombstone
/// marker and the moment of deletion (replaces the prior `is_deleted` boolean).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    /// Unique identifier
    pub id: NoteId,
    /// Owning tenant. Carries the active user's id since Phase 2.x — the
    /// pre-Phase-2 `SOLO_USER_ID` only appears on a brand-new install
    /// that has never signed in, or in tests.
    pub user_id: String,
    /// Plain text content
    pub content: String,
    /// Creation timestamp (Unix ms, client clock)
    pub created_at: i64,
    /// Last local update timestamp (Unix ms, client clock) — hint only
    pub updated_at: i64,
    /// Server-authoritative update timestamp (Unix ms). `None` until the note
    /// has been successfully pushed or pulled.
    pub server_updated_at: Option<i64>,
    /// Tombstone timestamp (Unix ms). `None` = live note.
    pub deleted_at: Option<i64>,
}

impl Note {
    /// Create a new note scoped to `user_id`.
    ///
    /// Returns `Err(Error::InvalidInput)` for an empty `user_id` — that
    /// would silently fall back to a sentinel and re-introduce the
    /// cross-account leak this whole refactor is closing. Callers in
    /// signed-out contexts pass [`SOLO_USER_ID`] explicitly so the
    /// substitution is visible at the call site.
    pub fn new_for_user(
        content: impl Into<String>,
        user_id: impl Into<String>,
    ) -> crate::Result<Self> {
        let user_id = user_id.into();
        if user_id.is_empty() {
            return Err(Error::InvalidInput(
                "Note user_id must not be empty; pass SOLO_USER_ID for signed-out captures".into(),
            ));
        }
        let now = chrono::Utc::now().timestamp_millis();
        Ok(Self {
            id: NoteId::new(),
            user_id,
            content: content.into(),
            created_at: now,
            updated_at: now,
            server_updated_at: None,
            deleted_at: None,
        })
    }

    /// Extract #tags from content
    #[must_use]
    pub fn tags(&self) -> Vec<String> {
        extract_tags(&self.content)
    }

    /// Get first line as title preview, truncated to `max_len` characters
    #[must_use]
    pub fn title_preview(&self, max_len: usize) -> String {
        self.content
            .lines()
            .next()
            .unwrap_or("")
            .chars()
            .take(max_len)
            .collect()
    }

    /// Check if note content is empty (whitespace-only counts as empty)
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.content.trim().is_empty()
    }

    /// True if this note has been tombstoned (locally or pulled as deleted).
    #[must_use]
    pub const fn is_deleted(&self) -> bool {
        self.deleted_at.is_some()
    }
}

/// Extract #tags from text
///
/// Valid tags match the pattern: `#[a-zA-Z][a-zA-Z0-9_-]*`
/// Tags are returned in lowercase and deduplicated.
///
/// # Examples
///
/// ```
/// use dirt_core::models::extract_tags;
///
/// let tags = extract_tags("Hello #world this is #Rust-lang");
/// assert!(tags.contains(&"world".to_string()));
/// assert!(tags.contains(&"rust-lang".to_string()));
/// ```
#[must_use]
pub fn extract_tags(text: &str) -> Vec<String> {
    tag_regex()
        .captures_iter(text)
        .map(|cap| cap[1].to_lowercase())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SOLO_USER_ID;

    #[test]
    fn test_note_id_unique() {
        let id1 = NoteId::new();
        let id2 = NoteId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_note_id_parse() {
        let id = NoteId::new();
        let parsed: NoteId = id.as_str().parse().unwrap();
        assert_eq!(id, parsed);
    }

    #[test]
    fn test_note_new_for_user() {
        let note = Note::new_for_user("Hello world", SOLO_USER_ID).unwrap();
        assert_eq!(note.content, "Hello world");
        assert_eq!(note.user_id, SOLO_USER_ID);
        assert!(!note.is_deleted());
        assert!(note.deleted_at.is_none());
        assert!(note.server_updated_at.is_none());
        assert!(note.created_at > 0);
        assert_eq!(note.created_at, note.updated_at);
    }

    #[test]
    fn test_note_new_for_user_rejects_empty_user_id() {
        let err = Note::new_for_user("hi", "").unwrap_err();
        assert!(matches!(err, Error::InvalidInput(_)));
    }

    #[test]
    fn test_note_new_for_user_stamps_arbitrary_user_id() {
        let uid = "01932aaa-0000-7000-8000-000000000001";
        let note = Note::new_for_user("hi", uid).unwrap();
        assert_eq!(note.user_id, uid);
    }

    #[test]
    fn test_is_deleted_reflects_tombstone() {
        let mut note = Note::new_for_user("Delete me", SOLO_USER_ID).unwrap();
        assert!(!note.is_deleted());
        note.deleted_at = Some(1_700_000_000_000);
        assert!(note.is_deleted());
    }

    #[test]
    fn test_extract_tags_basic() {
        let tags = extract_tags("Hello #world");
        assert_eq!(tags, vec!["world"]);
    }

    #[test]
    fn test_extract_tags_multiple() {
        let tags = extract_tags("#hello #world #rust");
        assert_eq!(tags.len(), 3);
        assert!(tags.contains(&"hello".to_string()));
        assert!(tags.contains(&"world".to_string()));
        assert!(tags.contains(&"rust".to_string()));
    }

    #[test]
    fn test_extract_tags_with_dashes_underscores() {
        let tags = extract_tags("#my-tag #another_tag");
        assert!(tags.contains(&"my-tag".to_string()));
        assert!(tags.contains(&"another_tag".to_string()));
    }

    #[test]
    fn test_extract_tags_lowercase() {
        let tags = extract_tags("#Hello #WORLD");
        assert!(tags.contains(&"hello".to_string()));
        assert!(tags.contains(&"world".to_string()));
    }

    #[test]
    fn test_extract_tags_deduplication() {
        let tags = extract_tags("#hello #Hello #HELLO");
        assert_eq!(tags.len(), 1);
        assert!(tags.contains(&"hello".to_string()));
    }

    #[test]
    fn test_extract_tags_invalid() {
        // Tags starting with numbers are invalid
        let tags = extract_tags("#123 #456test");
        assert!(tags.is_empty());
    }

    #[test]
    fn test_title_preview() {
        let note =
            Note::new_for_user("First line\nSecond line\nThird line", SOLO_USER_ID).unwrap();
        assert_eq!(note.title_preview(50), "First line");
        assert_eq!(note.title_preview(5), "First");
    }

    #[test]
    fn test_is_empty() {
        let empty = Note::new_for_user("   ", SOLO_USER_ID).unwrap();
        assert!(empty.is_empty());

        let not_empty = Note::new_for_user("Hello", SOLO_USER_ID).unwrap();
        assert!(!not_empty.is_empty());
    }
}
