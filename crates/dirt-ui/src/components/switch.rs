//! The switch (interface design, sections 4.1 and 6.11). Unchecked, it is
//! outlined in `--border-control`, which is told from its background at
//! 3:1; a hairline colour would not be.

use dioxus::prelude::*;

/// A switch. `label` is its name for a screen reader; the words a person
/// sees are the row's, beside it.
#[component]
pub fn Switch(
    checked: bool,
    label: String,
    #[props(default)] disabled: bool,
    /// What it would become.
    onchange: EventHandler<bool>,
) -> Element {
    rsx! {
        button {
            r#type: "button",
            class: "dui-switch",
            role: "switch",
            "aria-checked": if checked { "true" } else { "false" },
            "aria-label": "{label}",
            disabled,
            onclick: move |_| onchange.call(!checked),
        }
    }
}
