//! Note model

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt;
use std::str::FromStr;
use uuid::Uuid;

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

/// A note in the system
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    /// Unique identifier
    pub id: NoteId,
    /// Plain text content
    pub content: String,
    /// Creation timestamp (Unix ms)
    pub created_at: i64,
    /// Last update timestamp (Unix ms)
    pub updated_at: i64,
    /// Soft delete flag for sync
    pub is_deleted: bool,
}

impl Note {
    /// Create a new note with the given content
    #[must_use]
    pub fn new(content: impl Into<String>) -> Self {
        let now = chrono::Utc::now().timestamp_millis();
        Self {
            id: NoteId::new(),
            content: content.into(),
            created_at: now,
            updated_at: now,
            is_deleted: false,
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
    let re = Regex::new(r"#([a-zA-Z][a-zA-Z0-9_-]*)").expect("Invalid regex");
    re.captures_iter(text)
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
        assert!(!note.is_deleted);
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
