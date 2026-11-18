//! UI Components
//!
//! The parts of the old interface that stay until their steps replace
//! them (quick capture, and the button it is built of), and the list of a
//! note's files under the page.

mod attachments;
mod quick_capture;

pub use attachments::{AttachmentPanel, FilesNews};
pub use quick_capture::QuickCapture;
pub mod button;
