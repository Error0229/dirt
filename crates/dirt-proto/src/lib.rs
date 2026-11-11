//! Types shared by the Dirt server and its clients.
//!
//! This crate holds what both sides must agree on and nothing else: no HTTP,
//! no SQL and no cryptography. [`NoteId`] identifies a note; [`api`] holds
//! the requests, responses, error codes and limits of the protocol.

pub mod api;

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A note's identifier: a UUID v7, made on the device that creates the note.
///
/// A version 7 UUID starts with the time it was made, so identifiers sort by
/// creation time and the creation time never needs storing separately.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct NoteId(Uuid);

impl NoteId {
    /// Makes a new identifier stamped with the current time.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }

    /// Wraps a UUID read from storage or from the wire.
    #[must_use]
    pub const fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// The identifier as a UUID, for storage that has a UUID type.
    #[must_use]
    pub const fn as_uuid(&self) -> Uuid {
        self.0
    }

    /// The identifier as text, in the usual hyphenated form.
    #[must_use]
    pub fn as_str(&self) -> String {
        self.0.to_string()
    }

    /// When the identifier was made, in Unix milliseconds.
    ///
    /// Returns `None` for an identifier that is not a version 7 UUID, which
    /// can only come from outside this program.
    #[must_use]
    pub fn created_at_ms(&self) -> Option<i64> {
        let (seconds, nanos) = self.0.get_timestamp()?.to_unix();
        let millis = seconds
            .checked_mul(1000)?
            .checked_add(u64::from(nanos / 1_000_000))?;
        i64::try_from(millis).ok()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_id_carries_the_time_it_was_made() {
        let before = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let created = NoteId::new().created_at_ms().unwrap();
        let after = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let created = u128::try_from(created).unwrap();
        assert!(before <= created && created <= after);
    }

    #[test]
    fn a_known_id_gives_a_known_time() {
        // The first 48 bits of a version 7 UUID are Unix milliseconds.
        let id: NoteId = "01932aaa-0000-7000-8000-000000000123".parse().unwrap();
        assert_eq!(id.created_at_ms(), Some(0x0193_2aaa_0000));
    }

    #[test]
    fn an_id_that_is_not_version_7_has_no_time() {
        let id: NoteId = "936da01f-9abd-4d9d-80c7-02af85c822a8".parse().unwrap();
        assert_eq!(id.created_at_ms(), None);
    }

    #[test]
    fn ids_sort_by_creation_time() {
        let earlier: NoteId = "01932aaa-0000-7000-8000-000000000000".parse().unwrap();
        let later: NoteId = "01932aab-0000-7000-8000-000000000000".parse().unwrap();
        assert!(earlier < later);
    }

    #[test]
    fn text_form_round_trips() {
        let id = NoteId::new();
        assert_eq!(id.as_str().parse::<NoteId>().unwrap(), id);
        assert_eq!(id.to_string(), id.as_str());
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(serde_json::from_str::<NoteId>(&json).unwrap(), id);
    }

    #[test]
    fn text_that_is_not_an_id_is_rejected() {
        assert!("not-an-id".parse::<NoteId>().is_err());
    }
}
