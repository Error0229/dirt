//! Something that scrolls, with the scrollbar the app draws (interface
//! design, section 6.15).
//!
//! The bar is the same on every platform, lies over the content instead of
//! beside it, and is seen only while scrolling or when the pointer comes
//! to the edge.
//!
//! The content still scrolls natively, so the keyboard and screen readers
//! are untouched. Only the drawing of the bar is the app's own.

use dioxus::prelude::*;

/// How a row that leaves an animated list is drawn leaving.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ListExit {
    /// It tears away to the left, and the gap closes after it: a delete.
    #[default]
    Tear,
    /// It fades where it is.
    Fade,
    /// It is simply gone: a list being narrowed by Find.
    None,
}

impl ListExit {
    const fn name(self) -> &'static str {
        match self {
            Self::Tear => "tear",
            Self::Fade => "fade",
            Self::None => "none",
        }
    }
}

/// An area that scrolls up and down, with the drawn scrollbar.
///
/// With `list`, its rows are animated: every element inside with a
/// `data-key` slides to its new place when the rows change, settles in
/// when it is new, and leaves as `exit` says. The script watches the page
/// for that; nothing calls it.
#[component]
pub fn ScrollArea(
    /// Classes for the outer element, which is what the app lays out.
    #[props(default)]
    class: String,
    /// Classes for the scrolling element, which is what pads the content.
    #[props(default)]
    inner_class: String,
    #[props(default)] list: bool,
    #[props(default)] exit: ListExit,
    children: Element,
) -> Element {
    rsx! {
        div { class: "dui-scroll-wrap {class}",
            div {
                class: "dui-scroll {inner_class}",
                "data-dui-scroll": "",
                "data-dui-list": list.then_some(""),
                "data-dui-exit": list.then_some(exit.name()),
                {children}
            }
            // The bar. The script sizes and moves its thumb.
            div { class: "dui-sb", "aria-hidden": "true",
                div { class: "dui-sb-thumb" }
            }
        }
    }
}
