//! What the sections are made of (interface design, section 6.9): a card
//! of rows, each with an icon square, a title, one line of grey detail,
//! and at most one control on the right.

use dioxus::prelude::*;

use crate::icons::{Icon, IconSize, IconSvg};

/// A card: the rows of one section, or a group of them.
#[component]
pub(super) fn Card(children: Element) -> Element {
    rsx! {
        div { class: "dui-set-card", {children} }
    }
}

/// The icon square at a row's start.
#[component]
pub(super) fn RowIcon(icon: Icon, #[props(default)] tone: &'static str) -> Element {
    rsx! {
        span { class: "dui-set-ic {tone}", "aria-hidden": "true",
            IconSvg { icon, size: IconSize::Small }
        }
    }
}

/// A row: its icon, its title and detail, and its one control, which are
/// its children.
#[component]
pub(super) fn Row(
    #[props(default)] icon: Option<Icon>,
    title: String,
    #[props(default)] detail: Option<String>,
    /// Red: a row that removes.
    #[props(default)]
    danger: bool,
    children: Element,
) -> Element {
    let tone: &'static str = if danger { "danger" } else { "" };
    rsx! {
        div { class: "dui-set-row",
            if let Some(icon) = icon {
                RowIcon { icon, tone }
            }
            Words { title, detail, danger }
            {children}
        }
    }
}

/// A row that is itself a button: one of the account's actions, a note
/// that cannot sync.
#[component]
pub(super) fn RowButton(
    icon: Icon,
    title: String,
    #[props(default)] detail: Option<String>,
    #[props(default)] danger: bool,
    #[props(default)] tone: &'static str,
    /// A chevron at its end: it opens something.
    #[props(default)]
    opens: bool,
    onclick: EventHandler<MouseEvent>,
) -> Element {
    let tone: &'static str = if danger { "danger" } else { tone };
    rsx! {
        button {
            r#type: "button",
            class: "dui-set-row dui-set-press",
            onclick: move |event| onclick.call(event),
            RowIcon { icon, tone }
            Words { title, detail, danger }
            if opens {
                span { class: "dui-set-go", IconSvg { icon: Icon::ChevronRight, size: IconSize::Small } }
            }
        }
    }
}

/// A row's title and its grey line.
#[component]
pub(super) fn Words(title: String, detail: Option<String>, danger: bool) -> Element {
    rsx! {
        div { class: "dui-set-words",
            div { class: if danger { "dui-set-t danger" } else { "dui-set-t" }, "{title}" }
            if let Some(detail) = detail {
                div { class: "dui-set-s", "{detail}" }
            }
        }
    }
}

/// A row that is a sentence only: "Sign in to use Dirt on more than one
/// device."
#[component]
pub(super) fn Note(text: String) -> Element {
    rsx! {
        div { class: "dui-set-row dui-set-note", "{text}" }
    }
}

/// The spinner that turns while something is waited for.
#[component]
pub(super) fn Spinner(#[props(default)] small: bool) -> Element {
    rsx! {
        span {
            class: if small { "dui-spin sm" } else { "dui-spin" },
            "aria-hidden": "true",
        }
    }
}

/// A button with words, and an icon or a spinner before them. The
/// components' own [`crate::components::Button`] has no tooltip and no
/// icon; Settings needs both.
#[component]
pub(super) fn Btn(
    /// `primary`, `ghost`, `danger` or nothing for a quiet one.
    #[props(default)]
    kind: &'static str,
    #[props(default)] icon: Option<Icon>,
    #[props(default)] spinning: bool,
    #[props(default)] disabled: bool,
    #[props(default)] tip: Option<String>,
    onclick: EventHandler<MouseEvent>,
    children: Element,
) -> Element {
    rsx! {
        button {
            r#type: "button",
            class: "dui-btn sm {kind}",
            disabled,
            "data-tip": tip,
            onclick: move |event| onclick.call(event),
            if spinning {
                Spinner { small: true }
            } else if let Some(icon) = icon {
                IconSvg { icon, size: IconSize::Small }
            }
            {children}
        }
    }
}
