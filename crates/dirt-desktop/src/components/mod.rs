//! UI Components
//!
//! Reusable UI components for the desktop application.

mod note_editor;
mod note_list;
mod quick_capture;
mod search_bar;
mod sidebar;
mod toolbar;

pub use note_editor::NoteEditor;
pub use note_list::NoteList;
pub use quick_capture::QuickCapture;
pub use search_bar::SearchBar;
pub use sidebar::Sidebar;
pub use toolbar::Toolbar;
