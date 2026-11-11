//! Note model

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::OnceLock;

/// Returns the pre-compiled regex for extracting `#tag` patterns from note content.
fn tag_regex() -> &'static Regex {
    static TAG_REGEX: OnceLock<Regex> = OnceLock::new();
    TAG_REGEX.get_or_init(|| Regex::new(r"#([a-zA-Z][a-zA-Z0-9_-]*)").expect("Invalid regex"))
}
pub use dirt_proto::NoteId;

/// A note as the interfaces see it: the text and two times.
///
/// The store keeps each note as a CRDT document (see [`crate::notes`]) and
/// derives this from it. Deleted notes are never handed out as a `Note`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    /// Unique identifier
    pub id: NoteId,
    /// Plain text content
    pub content: String,
    /// When the note was created (Unix ms), read from its identifier
    pub created_at: i64,
    /// When the note was last edited on any device (Unix ms), for display
    pub updated_at: i64,
}

impl Note {
    /// Makes a note value that is not stored anywhere, for tests and previews.
    /// Use [`crate::Store::create_note`] to create a real one.
    #[must_use]
    pub fn new(content: impl Into<String>) -> Self {
        let id = NoteId::new();
        let now = id
            .created_at_ms()
            .unwrap_or_else(|| chrono::Utc::now().timestamp_millis());
        Self {
            id,
            content: content.into(),
            created_at: now,
            updated_at: now,
        }
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
    fn test_note_new() {
        let note = Note::new("Hello world");
        assert_eq!(note.content, "Hello world");
        assert!(note.created_at > 0);
        assert_eq!(note.created_at, note.updated_at);
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
        let note = Note::new("First line\nSecond line\nThird line");
        assert_eq!(note.title_preview(50), "First line");
        assert_eq!(note.title_preview(5), "First");
    }

    #[test]
    fn test_is_empty() {
        let empty = Note::new("   ");
        assert!(empty.is_empty());

        let not_empty = Note::new("Hello");
        assert!(!not_empty.is_empty());
    }
}
