//! Main application component

use dioxus::prelude::*;

use crate::state::AppState;
use crate::theme::Theme;
use crate::views::Home;

/// Root application component
#[component]
pub fn App() -> Element {
    // Initialize global state
    let notes = use_signal(Vec::new);
    let current_note_id = use_signal(|| None);
    let search_query = use_signal(String::new);
    let active_tag_filter = use_signal(|| None::<String>);
    let theme = use_signal(Theme::default);

    use_context_provider(|| AppState {
        notes,
        current_note_id,
        search_query,
        active_tag_filter,
        theme,
    });

    let theme_class = if theme().is_dark() { "dark" } else { "light" };

    rsx! {
        div {
            class: "app-container {theme_class}",
            style: "min-height: 100vh; font-family: system-ui, -apple-system, sans-serif;",
            Home {}
        }
    }
}
