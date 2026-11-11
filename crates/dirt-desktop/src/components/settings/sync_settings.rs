use dioxus::prelude::*;

#[component]
pub(super) fn SyncSettingsTab() -> Element {
    rsx! {
        div {
            class: "settings-row",
            div {
                class: "auth-hint",
                "Sync is unavailable while Dirt's sync is being rebuilt. Your notes are stored on this device only."
            }
        }
    }
}
