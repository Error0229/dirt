use dioxus::prelude::*;

/// Stacked row layout for settings sections.
/// Label and description on top, control below.
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
                class: "settings-row-header",
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
