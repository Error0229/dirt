//! Note model

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::OnceLock;

use super::Voice;

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
    /// Whether the note is archived: out of the list, still found by a
    /// search and kept in an export
    #[serde(default)]
    pub archived: bool,
    /// The note was said, not typed: how long its recording lasts and
    /// whether its words have been made (interface design 6.18). `None`
    /// for a typed note, and for one whose recording has been taken off.
    #[serde(default)]
    pub voice: Option<Voice>,
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
            archived: false,
            voice: None,
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

    /// The note as a list shows it: title, preview and the tags at its end.
    #[must_use]
    pub fn outline(&self) -> Outline {
        Outline::of(&self.content)
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

/// Where each `#tag` is in `text`, in the order they are written.
///
/// Each is the bytes it takes up, its `#` among them, by the same rule as
/// [`extract_tags`]: for an interface that draws tags differently from the
/// words around them.
#[must_use]
pub fn tag_spans(text: &str) -> Vec<std::ops::Range<usize>> {
    tag_regex().find_iter(text).map(|tag| tag.range()).collect()
}

/// Whether `word` is one tag and nothing else: `#work`.
fn is_tag(word: &str) -> bool {
    tag_regex()
        .find(word)
        .is_some_and(|tag| tag.range() == (0..word.len()))
}

/// How many characters of a note a list shows under its title. Two lines
/// on the desktop and three on a phone are fewer than this.
const PREVIEW_CHARS: usize = 240;

/// A note as a list shows it (interface design 6.2): its first line as the
/// title, the rest as a preview, and the tags at its very end apart.
///
/// Tags at the very end of a note are labels, not prose: they are left out
/// of the title and the preview. A tag in the middle of a sentence stays
/// where it is. A note that is tags and nothing else has no labels: its
/// tags are all it says.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Outline {
    /// The first line that holds anything, without the space around it.
    /// Empty for a note with no words.
    pub title: String,
    /// What follows the title, as one line, cut to a few lines' length.
    pub preview: String,
    /// The tags at the end of the note, as they are written, without
    /// their `#`.
    pub labels: Vec<String>,
}

impl Outline {
    /// The outline of a note with this text.
    #[must_use]
    pub fn of(text: &str) -> Self {
        let (prose, labels) = split_labels(text);
        let mut lines = prose.lines().map(str::trim).filter(|line| !line.is_empty());
        let title = lines.next().unwrap_or_default().to_owned();
        let mut preview = String::new();
        let mut held = 0;
        'lines: for line in lines {
            for word in line.split_whitespace() {
                if held >= PREVIEW_CHARS {
                    break 'lines;
                }
                if !preview.is_empty() {
                    preview.push(' ');
                    held += 1;
                }
                preview.push_str(word);
                held += word.chars().count();
            }
        }
        Self {
            title,
            preview,
            labels,
        }
    }
}

/// Splits a note's text into its prose and the tags at its very end.
fn split_labels(text: &str) -> (&str, Vec<String>) {
    let mut prose = text.trim_end();
    let mut labels = Vec::new();
    loop {
        // The last word, and where it starts.
        let start = prose
            .char_indices()
            .rev()
            .find(|(_, character)| character.is_whitespace())
            .map_or(0, |(at, character)| at + character.len_utf8());
        let word = &prose[start..];
        if !is_tag(word) {
            break;
        }
        labels.push(word[1..].to_owned());
        prose = prose[..start].trim_end();
    }
    // Nothing but tags: they are the note, not labels on it.
    if prose.is_empty() {
        return (text, Vec::new());
    }
    labels.reverse();
    (prose, labels)
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

    #[test]
    fn tag_spans_take_in_the_hash() {
        let text = "ask #Work about #q3-plan, not #9lives";
        let tags: Vec<&str> = tag_spans(text)
            .into_iter()
            .map(|span| &text[span])
            .collect();
        assert_eq!(tags, ["#Work", "#q3-plan"]);
    }

    #[test]
    fn an_outline_is_the_first_line_the_rest_and_the_tags_at_the_end() {
        let outline =
            Outline::of("Sourdough\n78% hydration\nfold every 30 minutes #baking #Bread\n");
        assert_eq!(outline.title, "Sourdough");
        assert_eq!(outline.preview, "78% hydration fold every 30 minutes");
        assert_eq!(outline.labels, ["baking", "Bread"]);
    }

    #[test]
    fn a_tag_inside_a_sentence_is_prose() {
        let outline = Outline::of("Ask #work about the offsite");
        assert_eq!(outline.title, "Ask #work about the offsite");
        assert!(outline.labels.is_empty());

        // Only the tags after the last word are labels.
        let outline = Outline::of("Ask #work about it #todo");
        assert_eq!(outline.title, "Ask #work about it");
        assert_eq!(outline.labels, ["todo"]);

        // A tag stuck to a word is part of the word's line.
        let outline = Outline::of("price#tag");
        assert_eq!(outline.title, "price#tag");
        assert!(outline.labels.is_empty());
    }

    #[test]
    fn a_note_of_tags_alone_keeps_them_as_its_words() {
        let outline = Outline::of("#idea #later");
        assert_eq!(outline.title, "#idea #later");
        assert!(outline.labels.is_empty());
        assert_eq!(Outline::of("#idea").title, "#idea");
        assert_eq!(Outline::of("\n #idea\n").title, "#idea");
    }

    #[test]
    fn the_title_is_the_first_line_that_holds_anything() {
        let outline = Outline::of("\n   \n  Second thoughts  \n\nmore\n");
        assert_eq!(outline.title, "Second thoughts");
        assert_eq!(outline.preview, "more");
        assert_eq!(Outline::of("  \n ").title, "");
        assert_eq!(Outline::of("").preview, "");
    }

    #[test]
    fn a_long_note_gives_a_preview_of_a_few_lines() {
        let long = format!("Title\n{}", "word ".repeat(400));
        let outline = Outline::of(&long);
        assert!(outline.preview.chars().count() <= PREVIEW_CHARS + 5);
        assert!(outline.preview.ends_with("word"));
        assert_eq!(Note::new(long).outline(), outline);
    }
}
