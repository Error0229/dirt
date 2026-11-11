//! Data models for Dirt

mod note;
mod settings;

pub use note::{extract_tags, Note, NoteId};
pub use settings::{Settings, ThemeMode};
