//! A note that was said, not typed (interface design 6.18).
//!
//! Its recording is one of its files, and its document marks which one,
//! how long it lasts, and whether its words have been made. The mark is
//! what puts the mic on its row, and what names it ("Voice · 0:12") until
//! it has its words.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Whether a recording's words have been made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Words {
    /// Not yet. Only the device that recorded it makes them.
    Waiting,
    /// They are at the start of the note.
    Made,
    /// The model found none: unclear speech, or none at all. The recording
    /// stays, so that nothing said is lost.
    NoneFound,
}

/// The mark on a note that one of its files is its voice recording.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceMark {
    /// The recording: one of the note's files.
    #[serde(rename = "f")]
    pub file_id: Uuid,
    /// How long it lasts.
    #[serde(rename = "ms")]
    pub duration_ms: u64,
    #[serde(rename = "w")]
    pub words: Words,
}

impl VoiceMark {
    /// The mark as the note's document holds it. Short names, because the
    /// document is sent whole on every edit; a field this version does not
    /// know is ignored when read, so that a later one can add one.
    #[must_use]
    pub fn to_entry(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Reads a mark back. `None` for one that cannot be read.
    #[must_use]
    pub fn from_entry(entry: &str) -> Option<Self> {
        serde_json::from_str(entry).ok()
    }

    /// How long it lasts, as the note is named by it until it has words:
    /// `0:12`, `1:05`, `10:00`. In whole seconds, as the timer counted
    /// them while it listened and as a player shows it, and never less
    /// than one.
    #[must_use]
    pub fn length(&self) -> String {
        let seconds = (self.duration_ms / 1000).max(1);
        format!("{}:{:02}", seconds / 60, seconds % 60)
    }
}

/// What a note's row shows of its recording.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Voice {
    pub duration_ms: u64,
    pub words: Words,
}

impl From<VoiceMark> for Voice {
    fn from(mark: VoiceMark) -> Self {
        Self {
            duration_ms: mark.duration_ms,
            words: mark.words,
        }
    }
}

impl Voice {
    /// How long it lasts: `0:12` (see [`VoiceMark::length`]).
    #[must_use]
    pub fn length(&self) -> String {
        VoiceMark {
            file_id: Uuid::nil(),
            duration_ms: self.duration_ms,
            words: self.words,
        }
        .length()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mark_is_read_back_as_it_was_written() {
        let mark = VoiceMark {
            file_id: Uuid::now_v7(),
            duration_ms: 12_345,
            words: Words::NoneFound,
        };
        let entry = mark.to_entry();
        assert!(entry.contains("\"w\":\"none_found\""), "{entry}");
        assert_eq!(VoiceMark::from_entry(&entry), Some(mark));
        assert_eq!(VoiceMark::from_entry("{\"later\":1}"), None);
    }

    #[test]
    fn a_length_is_minutes_and_seconds() {
        let length = |ms| {
            VoiceMark {
                file_id: Uuid::nil(),
                duration_ms: ms,
                words: Words::Waiting,
            }
            .length()
        };
        assert_eq!(length(0), "0:01");
        assert_eq!(length(12_400), "0:12");
        assert_eq!(length(12_999), "0:12");
        assert_eq!(length(65_000), "1:05");
        assert_eq!(length(600_000), "10:00");
    }
}
