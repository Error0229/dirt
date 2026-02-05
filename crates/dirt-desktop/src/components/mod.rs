//! UI Components
//!
//! Reusable UI components for the desktop application.

mod note_editor;
mod note_list;
mod quick_capture;
mod search_bar;
mod settings;
mod sidebar;
mod toolbar;

pub use note_editor::NoteEditor;
pub use note_list::NoteList;
pub use quick_capture::open_quick_capture_window;
pub use search_bar::SearchBar;
pub use settings::SettingsPanel;
pub use sidebar::Sidebar;
pub use toolbar::Toolbar;
pub mod button;
pub mod input;
pub mod select;
pub mod dialog;
pub mod slider;
