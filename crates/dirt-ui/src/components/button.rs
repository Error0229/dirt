//! Buttons with words (interface design, section 6.10).

use dioxus::prelude::*;

/// The kind of a button. A flow has at most one primary button on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonKind {
    /// The one main action of a flow: **Sign in**, **They match**.
    Primary,
    /// A secondary action in Settings: **Make one**, **Export**.
    #[default]
    Quiet,
    /// The other choice beside a primary: **They don't**, **Sign out**.
    Ghost,
    /// Something that removes: red text.
    Danger,
    /// Danger's last step, the final confirm: a red fill.
    DangerFill,
}

impl ButtonKind {
    const fn class(self) -> &'static str {
        match self {
            Self::Primary => " primary",
            Self::Quiet => "",
            Self::Ghost => " ghost",
            Self::Danger => " danger",
            Self::DangerFill => " danger-fill",
        }
    }
}

/// A button with words on it.
#[component]
pub fn Button(
    #[props(default)] kind: ButtonKind,
    /// 28 px high on the desktop instead of 34, inside a row.
    #[props(default)]
    small: bool,
    #[props(default)] disabled: bool,
    onclick: EventHandler<MouseEvent>,
    children: Element,
) -> Element {
    let classes = format!("dui-btn{}{}", kind.class(), if small { " sm" } else { "" });
    rsx! {
        button {
            r#type: "button",
            class: "{classes}",
            disabled,
            onclick: move |event| onclick.call(event),
            {children}
        }
    }
}
