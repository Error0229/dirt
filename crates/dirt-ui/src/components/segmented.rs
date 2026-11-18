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
    /// The word is not shown, only the icon: the word is its tooltip and
    /// its name for a screen reader.
    pub icon_only: bool,
}

impl Segment {
    /// An option with a word on it.
    #[must_use]
    pub fn new(value: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
            icon: None,
            icon_only: false,
        }
    }

    /// With an icon before the word.
    #[must_use]
    pub const fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// With its icon alone shown, the word in its tooltip.
    #[must_use]
    pub const fn icon_only(mut self) -> Self {
        self.icon_only = true;
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
                    "aria-label": segment.icon_only.then(|| segment.label.clone()),
                    "data-tip": segment.icon_only.then(|| segment.label.clone()),
                    onmounted: move |event| buttons.mounted(index, event.data()),
                    onclick: {
                        let value = segment.value;
                        move |_| onchange.call(value.clone())
                    },
                    if let Some(icon) = segment.icon {
                        IconSvg { icon, size: IconSize::Small }
                    }
                    if !segment.icon_only {
                        "{segment.label}"
                    }
                }
            }
        }
    }
}
