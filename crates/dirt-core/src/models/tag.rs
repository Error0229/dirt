//! Tag model

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use uuid::Uuid;

/// A unique identifier for a tag
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TagId(Uuid);

impl TagId {
    /// Create a new unique tag ID
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

impl Default for TagId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for TagId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for TagId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

/// A tag for organizing notes
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tag {
    /// Unique identifier
    pub id: TagId,
    /// Tag name (stored in lowercase)
    pub name: String,
    /// Creation timestamp (Unix ms)
    pub created_at: i64,
}

impl Tag {
    /// Create a new tag with the given name
    ///
    /// The name is automatically converted to lowercase.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: TagId::new(),
            name: name.into().to_lowercase(),
            created_at: chrono::Utc::now().timestamp_millis(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tag_new_lowercase() {
        let tag = Tag::new("Hello");
        assert_eq!(tag.name, "hello");
    }

    #[test]
    fn test_tag_id_unique() {
        let id1 = TagId::new();
        let id2 = TagId::new();
        assert_ne!(id1, id2);
    }
}
