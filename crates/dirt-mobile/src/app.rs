use dioxus::prelude::*;
use dioxus_primitives::toast::ToastProvider;

#[path = "app_shell.rs"]
mod app_shell;

#[component]
pub fn App() -> Element {
    rsx! {
        ToastProvider {
            app_shell::AppShell {}
        }
    }
}
