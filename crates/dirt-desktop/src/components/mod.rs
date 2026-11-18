//! UI Components
//!
//! The parts of the old interface that stay until their steps replace
//! them (Settings, quick capture, and the controls they are built of), and
//! the list of a note's files under the page.

mod attachments;
mod quick_capture;
mod settings;

pub use attachments::{AttachmentPanel, FilesNews};
pub use quick_capture::QuickCapture;
pub use settings::SettingsPanel;
pub mod button;
pub mod dialog;
pub mod input;
pub mod select;
pub mod slider;
