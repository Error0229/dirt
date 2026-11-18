use dioxus::prelude::*;

use crate::app_shell;

#[component]
pub fn App() -> Element {
    rsx! {
        app_shell::AppShell {}
    }
}
