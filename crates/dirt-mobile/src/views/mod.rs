//! The old screens, until step 5 of the interface's plan replaces them:
//! Settings and Devices, and the list of a note's files under its page.
//!
//! The main screen is the new one, in `crate::screen`.

pub mod attachments;
mod devices;
mod settings;

pub use devices::Devices;
pub use settings::Settings;
