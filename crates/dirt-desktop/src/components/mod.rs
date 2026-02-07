//! UI Components
//!
//! Reusable UI components for the desktop application.

mod note_actions;
mod note_card;
mod note_editor;
mod note_list;
mod quick_capture;
mod search_bar;
mod settings;
mod sidebar;
mod toolbar;

pub use note_card::NoteCard;
pub use note_editor::NoteEditor;
pub use note_list::NoteList;
pub use quick_capture::QuickCapture;
pub use search_bar::SearchBar;
pub use settings::SettingsPanel;
pub use sidebar::Sidebar;
pub use toolbar::Toolbar;
pub use note_actions::create_note_optimistic;
pub mod button;
pub mod card;
pub mod dialog;
pub mod input;
pub mod select;
pub mod slider;
