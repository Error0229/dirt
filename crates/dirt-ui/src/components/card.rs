//! A note as a card of Android's list (interface design, sections 6.2, 8.2
//! and 6.19).
//!
//! It shows what the desktop's row shows ([`NoteSummary`]), laid out for a
//! finger: the title and three lines of the rest, and along the bottom the
//! edit time, a paperclip if it has files, the hollow dot if it has not
//! reached the server, and its tags. A picture attached to it shows as a
//! thumbnail.
//!
//! A tap opens it. A swipe to the left archives it, or deletes it if it is
//! archived already: the card follows the finger in the script, and Rust
//! hears once, when it has flown off. A long press shows the edit time in
//! full, as a tooltip would. The note's own page has a button for what the
//! swipe does (section 12).

use dioxus::prelude::*;
use dirt_core::models::{Grain, NoteId};
use dirt_core::sync::overview::NOT_ON_SERVER_YET;

use crate::components::note_row::{Marked, NoteSummary, TagLinks};
use crate::components::swipe::Swipeable;
use crate::icons::{Icon, IconSize, IconSvg};
use crate::pile::shade;

/// The ID a card's element has in the page: what the note's page grows
/// out of and shrinks back into.
#[must_use]
pub fn card_element_id(note: &NoteId) -> String {
    format!("dui-card-{note}")
}

/// A note as a card.
#[component]
pub fn NoteCard(
    summary: NoteSummary,
    /// It has just arrived from another device: it flashes once.
    #[props(default)]
    flash: bool,
    /// It has just come back by Undo: it drops in with a bounce.
    #[props(default)]
    back: bool,
    /// It is a new thought just let go: its grain settles onto it (5.5).
    #[props(default)]
    settle: bool,
    on_open: EventHandler<()>,
    on_tag: EventHandler<String>,
    /// It was swiped away: archive it, or delete it if it is archived.
    on_swipe: EventHandler<()>,
) -> Element {
    let NoteSummary {
        id,
        title,
        untitled,
        preview,
        labels,
        time,
        time_in_full,
        has_files,
        waiting,
        archived,
        thumbnail,
    } = summary;
    let classes = format!(
        "dui-card{}{}",
        if flash { " flash" } else { "" },
        if archived { " arch" } else { "" },
    );
    let name = if title.is_empty() {
        untitled.clone()
    } else {
        title.iter().map(|piece| piece.text.as_str()).collect()
    };
    rsx! {
        div {
            class: "{classes}",
            id: card_element_id(&id),
            role: "listitem",
            "data-key": "{id}",
            "data-dui-enter": back.then_some("pop"),
            // Its grain's shade, for the script that throws grains.
            "data-dui-grain": shade(&Grain::of(&id)),
            "data-dui-settle": settle.then_some(""),
            Swipeable {
                icon: if archived { Icon::Trash } else { Icon::Archive },
                danger: archived,
                on_swipe: move |()| on_swipe.call(()),
                div {
                    class: "dui-card-in",
                    role: "button",
                    tabindex: "0",
                    "aria-label": "{name}",
                    "aria-description": "{time_in_full}",
                    // A long press says when it was edited, in full.
                    "data-tip": "{time_in_full}",
                    onclick: move |_| on_open.call(()),
                    onkeydown: move |event: KeyboardEvent| {
                        if matches!(event.key(), Key::Enter) {
                            on_open.call(());
                        }
                    },
                    if let Some(picture) = thumbnail {
                        img {
                            class: "dui-card-thumb",
                            src: "{picture}",
                            alt: "",
                            draggable: "false",
                        }
                    }
                    div { class: "dui-card-t",
                        if title.is_empty() {
                            "{untitled}"
                        } else {
                            Marked { pieces: title }
                        }
                    }
                    if !preview.is_empty() {
                        div { class: "dui-card-s",
                            Marked { pieces: preview }
                        }
                    }
                    div { class: "dui-card-meta",
                        span { class: "dui-card-time", "{time}" }
                        if has_files {
                            span {
                                class: "dui-card-mark",
                                role: "img",
                                "aria-label": "Has files",
                                IconSvg { icon: Icon::Paperclip, size: IconSize::Small }
                            }
                        }
                        if waiting {
                            span {
                                class: "dui-wait",
                                role: "img",
                                "aria-label": NOT_ON_SERVER_YET,
                            }
                        }
                        if !labels.is_empty() {
                            span { class: "dui-card-tags",
                                TagLinks { labels, on_tag }
                            }
                        }
                    }
                }
            }
        }
    }
}
