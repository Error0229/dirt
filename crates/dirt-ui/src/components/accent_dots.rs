//! The accent dots (interface design, sections 4.1.1 and 6.11): a 24 px
//! circle for each accent, in the colour it would be in the current theme.
//! No names on screen: each dot's tooltip gives its name.

use dioxus::prelude::*;
use dirt_core::models::Accent;

use super::radio::{step, use_buttons};

/// What a dot is called: its accent's name, and "(default)" for Clay.
fn dot_name(accent: Accent) -> String {
    if accent == Accent::default() {
        format!("{} (default)", accent.label())
    } else {
        accent.label().to_owned()
    }
}

/// The row of dots that chooses the accent.
#[component]
pub fn AccentDots(value: Accent, onchange: EventHandler<Accent>) -> Element {
    let buttons = use_buttons();
    let count = Accent::ALL.len();
    let chosen = Accent::ALL
        .iter()
        .position(|accent| *accent == value)
        .unwrap_or(0);
    rsx! {
        div {
            class: "dui-dots",
            role: "radiogroup",
            "aria-label": "Accent",
            onkeydown: move |event| {
                if let Some(to) = step(&event.key(), chosen, count) {
                    event.prevent_default();
                    onchange.call(Accent::ALL[to]);
                    buttons.focus(to);
                }
            },
            for (index , accent) in Accent::ALL.into_iter().enumerate() {
                button {
                    key: "{accent.name()}",
                    r#type: "button",
                    class: "dui-dot",
                    role: "radio",
                    "aria-checked": if index == chosen { "true" } else { "false" },
                    "aria-label": dot_name(accent),
                    "data-tip": dot_name(accent),
                    tabindex: if index == chosen { "0" } else { "-1" },
                    style: "--c:var(--dot-{accent.name()})",
                    onmounted: move |event| buttons.mounted(index, event.data()),
                    onclick: move |_| onchange.call(accent),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_clay_says_it_is_the_default() {
        assert_eq!(dot_name(Accent::Clay), "Clay (default)");
        assert_eq!(dot_name(Accent::Moss), "Moss");
    }
}
