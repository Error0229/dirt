//! UI Components
//!
//! Reusable UI components for the desktop application.

mod note_actions;
mod note_card;
mod note_editor;
mod note_list;
mod quick_capture;
mod settings;
mod toolbar;

pub use note_actions::{create_note_optimistic, delete_note_optimistic, update_note_content};
pub use note_card::NoteCard;
pub use note_editor::NoteEditor;
pub use note_list::NoteList;
pub use quick_capture::QuickCapture;
pub use settings::SettingsPanel;
pub use toolbar::Toolbar;
pub mod button;
pub mod dialog;
pub mod input;
pub mod select;
pub mod slider;
