//! The segmented control (interface design, sections 5.5 and 6.11): two or
//! three exclusive options with a thumb that slides between them.

use dioxus::prelude::*;

use super::radio::{step, use_buttons};
use crate::icons::{Icon, IconSize, IconSvg};

/// One option of a segmented control.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    /// What choosing it reports.
    pub value: String,
    /// The word on it.
    pub label: String,
    /// An icon before the word.
    pub icon: Option<Icon>,
}

impl Segment {
    /// An option with a word on it.
    #[must_use]
    pub fn new(value: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
            icon: None,
        }
    }

    /// With an icon before the word.
    #[must_use]
    pub const fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }
}

/// A segmented control. `label` names the group for a screen reader. The
/// arrow keys move the choice, as in any group of radio buttons.
#[component]
pub fn Segmented(
    label: String,
    segments: Vec<Segment>,
    /// The chosen option's value.
    value: String,
    onchange: EventHandler<String>,
    #[props(default)] small: bool,
) -> Element {
    let buttons = use_buttons();
    let count = segments.len();
    let chosen = segments
        .iter()
        .position(|segment| segment.value == value)
        .unwrap_or(0);
    let values: Vec<String> = segments
        .iter()
        .map(|segment| segment.value.clone())
        .collect();
    rsx! {
        div {
            class: if small { "dui-seg sm" } else { "dui-seg" },
            role: "radiogroup",
            "aria-label": "{label}",
            style: "--n:{count};--i:{chosen}",
            onkeydown: move |event| {
                if let Some(to) = step(&event.key(), chosen, count) {
                    event.prevent_default();
                    if let Some(value) = values.get(to) {
                        onchange.call(value.clone());
                    }
                    buttons.focus(to);
                }
            },
            span { class: "dui-seg-thumb", "aria-hidden": "true" }
            for (index , segment) in segments.into_iter().enumerate() {
                button {
                    key: "{segment.value}",
                    r#type: "button",
                    role: "radio",
                    "aria-checked": if index == chosen { "true" } else { "false" },
                    tabindex: if index == chosen { "0" } else { "-1" },
                    onmounted: move |event| buttons.mounted(index, event.data()),
                    onclick: {
                        let value = segment.value;
                        move |_| onchange.call(value.clone())
                    },
                    if let Some(icon) = segment.icon {
                        IconSvg { icon, size: IconSize::Small }
                    }
                    "{segment.label}"
                }
            }
        }
    }
}
