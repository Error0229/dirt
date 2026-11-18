//! The page (interface design, sections 6.1 and 6.4): where a thought is
//! written. It is always there. With no note open it is blank, with the
//! cursor in it; the first character makes the note, which then saves
//! itself as it is typed.
//!
//! The note's icons sit in a gutter beside its first line, just outside
//! its column, so that they travel with it. The files attached to the note
//! are listed under its text.

use std::path::PathBuf;

use dioxus::prelude::*;
use dirt_ui::components::{IconButton, ScrollArea};
use dirt_ui::dom::{self, Caret};
use dirt_ui::Icon;

use super::{Arrival, Screen, Where, PAGE, PAGE_ID};
use crate::components::AttachmentPanel;
use crate::state::AppState;

/// The page.
#[component]
pub fn Page() -> Element {
    let screen = use_context::<Screen>();
    let app = use_context::<AppState>();
    let view = screen.page.read().clone();
    let open = *app.current_note_id.read();
    let serial = view.serial;
    // While Settings is open over the page, Esc closes Settings and leaves
    // the note: the script must not empty the page then.
    let done_keys = !*app.settings_open.read();
    let from = view.from.map(|from| match from {
        Arrival::Above => "above",
        Arrival::Below => "below",
    });
    let take_focus = view.take_focus;
    let text = view.text.clone();

    // A file dropped anywhere on the page is attached to the note, or
    // becomes a note of its own on a blank page (7.3).
    let on_drop = move |event: Event<DragData>| {
        // Left to itself the window would leave the app and show the file.
        event.prevent_default();
        let paths: Vec<PathBuf> = event
            .data_transfer()
            .files()
            .iter()
            .map(dioxus::html::FileData::path)
            .collect();
        screen.attach(paths);
    };

    rsx! {
        main {
            class: "dk-main",
            // Saying that a drop is welcome is what lets one happen.
            ondragover: move |event: Event<DragData>| event.prevent_default(),
            ondrop: on_drop,
            ScrollArea { class: "dk-page-wrap", inner_class: "dk-page",
                div {
                    key: "{view.serial}",
                    class: if open.is_some() { "dk-col" } else { "dk-col blank" },
                    "data-from": from,
                    div {
                        class: "dk-gutter",
                        role: "toolbar",
                        // While a note is written the icons go, and come
                        // back as the pointer nears them (6.4).
                        "data-dui-near": "64",
                        "aria-label": if open.is_some() { "The note" } else { "The page" },
                        if open.is_some() {
                            IconButton {
                                icon: Icon::Check,
                                label: "Done",
                                shortcut: "Esc",
                                onclick: move |_| screen.done(false),
                            }
                        }
                        IconButton {
                            icon: Icon::Paperclip,
                            label: "Attach a file",
                            shortcut: "Ctrl Shift A",
                            onclick: move |_| screen.choose_files(),
                        }
                        if let Some(note) = open {
                            if view.archived {
                                IconButton {
                                    icon: Icon::Unarchive,
                                    label: "Back to the list",
                                    onclick: move |_| screen.unarchive(note),
                                }
                                IconButton {
                                    icon: Icon::Trash,
                                    label: "Delete for good",
                                    shortcut: "Ctrl Shift Del",
                                    danger: true,
                                    onclick: move |_| screen.delete(note),
                                }
                            } else {
                                IconButton {
                                    icon: Icon::Archive,
                                    label: "Archive",
                                    shortcut: "Ctrl Del",
                                    onclick: move |_| screen.archive(note),
                                }
                            }
                        }
                    }
                    textarea {
                        id: PAGE_ID,
                        class: "dk-text",
                        "aria-label": "Note",
                        placeholder: "What\u{2019}s on your mind?",
                        spellcheck: "false",
                        // The keys that empty it, with this computer's
                        // command key (`keys::empties_page`).
                        "data-dui-page": done_keys.then_some(super::keys::COMMAND_KEY),
                        oninput: move |event: FormEvent| screen.typed(serial, &event.value()),
                        onfocus: move |_| screen.focused(Where::Page),
                        onblur: move |_| screen.focused(Where::Elsewhere),
                        // The text is given once, when the page is made,
                        // and never as an attribute: rendered again, the
                        // page would write it over what was typed since.
                        // While the person types, the field keeps its own.
                        onmounted: move |_| {
                            dom::set_value(PAGE, &text);
                            if take_focus {
                                screen.focus_page(Caret::End);
                            }
                        },
                    }
                    if let Some(note) = open {
                        AttachmentPanel { key: "{note}", note_id: note, news: screen.files_news }
                    }
                }
            }
        }
    }
}
