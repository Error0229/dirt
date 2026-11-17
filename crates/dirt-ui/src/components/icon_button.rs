//! The icon button (interface design, sections 4.5 and 6.10).
//!
//! Every action in the main screens is one. It has no label on screen. Its
//! tooltip says what it does and gives its shortcut, and is its name for a
//! screen reader.

use dioxus::prelude::*;

use crate::icons::{Icon, IconSize, IconSvg};

/// How large an icon button is on the desktop. On Android every icon
/// button is 40 dp, whatever is asked for: a finger needs that much.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IconButtonSize {
    /// 28 px with a 15 px icon, inside a row or a field.
    Small,
    /// 32 px with an 18 px icon.
    #[default]
    Regular,
}

/// A button that is an icon. `label` is its tooltip (a long press shows it
/// on Android) and its accessible name; `shortcut` is shown in the tooltip
/// as key chips, its keys separated by spaces: `"Ctrl ,"`.
#[component]
pub fn IconButton(
    icon: Icon,
    label: String,
    #[props(default)] shortcut: Option<String>,
    #[props(default)] size: IconButtonSize,
    /// Red under the pointer: delete, remove.
    #[props(default)]
    danger: bool,
    /// For a button that stays on, such as Tags while its list is open.
    #[props(default)]
    pressed: Option<bool>,
    #[props(default)] disabled: bool,
    /// More classes, for where the button sits.
    #[props(default)]
    class: String,
    onclick: EventHandler<MouseEvent>,
) -> Element {
    let small = size == IconButtonSize::Small;
    let classes = format!(
        "dui-icon-btn{}{}{}{}",
        if small { " sm" } else { "" },
        if danger { " danger" } else { "" },
        if class.is_empty() { "" } else { " " },
        class
    );
    rsx! {
        button {
            r#type: "button",
            class: "{classes}",
            "aria-label": "{label}",
            "aria-pressed": pressed.map(|pressed| if pressed { "true" } else { "false" }),
            "aria-keyshortcuts": shortcut.as_deref().map(|keys| keys.replace(' ', "+")),
            "data-tip": "{label}",
            "data-kbd": shortcut,
            disabled,
            onclick: move |event| onclick.call(event),
            IconSvg {
                icon,
                size: if small { IconSize::Small } else { IconSize::Regular },
            }
        }
    }
}
