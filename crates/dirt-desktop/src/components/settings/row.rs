use dioxus::prelude::*;

/// Shared row layout for settings sections.
#[component]
pub(super) fn SettingRow(
    #[props(into)] label: String,
    #[props(into)] description: String,
    children: Element,
) -> Element {
    rsx! {
        div {
            class: "settings-row",

            div {
                class: "settings-row-info",
                div {
                    class: "settings-row-label",
                    "{label}"
                }
                div {
                    class: "settings-row-description",
                    "{description}"
                }
            }
            div {
                class: "settings-row-control",
                {children}
            }
        }
    }
}
