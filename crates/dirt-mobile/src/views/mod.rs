//! Mobile shell views.
//!
//! Each top-level navigation destination is a `#[component]` here.
//! `app_shell` switches between them based on `AppState::view`.

mod editor;
mod list;

pub use editor::Editor;
pub use list::List;
