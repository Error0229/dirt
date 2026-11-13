//! Mobile shell views.
//!
//! Each top-level navigation destination is a `#[component]` here.
//! `app_shell` switches between them based on `AppState::view`.

pub mod attachments;
mod devices;
mod editor;
mod list;
mod settings;

pub use devices::Devices;
pub use editor::Editor;
pub use list::List;
pub use settings::Settings;
