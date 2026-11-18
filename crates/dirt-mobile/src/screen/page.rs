//! The page (interface design, sections 6.1, 6.4 and 8.3): a note, full
//! screen, grown out of its card, or a new page grown out of the bar. It
//! saves itself as it is typed; there is no Save button.
//!
//! The top bar has ⌄ on a new page and ‹ on a note, the paperclip, and the
//! archive box (on an archived note, "back to the list" and the bin). It
//! fades while the person types, and comes back when the page is touched.
//! The note's files are under its text.
//!
//! The page is always in the screen, and so is its text field: a page let
//! go keeps its words while it shrinks away, and the next page is given
//! its own.

use dioxus::prelude::*;
use dirt_ui::components::{GrowPage, IconButton, ScrollArea};
use dirt_ui::Icon;

use super::Screen;
use crate::views::attachments::AttachmentPanel;

/// The page.
#[component]
pub fn Page() -> Element {
    let screen = use_context::<Screen>();
    let view = screen.page.read().clone();
    let typing = *screen.typing.read();
    let note = view.note;
    let archived = view.archived;

    rsx! {
        GrowPage {
            open: view.open,
            from: view.from.clone(),
            label: if view.new_page { "New note" } else { "Note" },
            on_back: move |()| screen.done(),
            class: "dm-page",
            div {
                class: "dm-page-in",
                // Touching the page brings its bar back.
                onpointerdown: move |_| {
                    let mut typing = screen.typing;
                    if *typing.peek() {
                        typing.set(false);
                    }
                },
                div {
                    class: if typing { "dm-page-bar typing" } else { "dm-page-bar" },
                    role: "toolbar",
                    "aria-label": "The note",
                    "data-dui-grow-bar": "",
                    IconButton {
                        icon: if view.new_page { Icon::ChevronDown } else { Icon::ChevronLeft },
                        label: "Done",
                        onclick: move |_| screen.done(),
                    }
                    span { class: "dm-grow" }
                    IconButton {
                        icon: Icon::Paperclip,
                        label: "Attach a file",
                        onclick: move |_| screen.choose_files(),
                    }
                    if note.is_some() {
                        if archived {
                            IconButton {
                                icon: Icon::Unarchive,
                                label: "Back to the list",
                                onclick: move |_| screen.unarchive_open(),
                            }
                            IconButton {
                                icon: Icon::Trash,
                                label: "Delete for good",
                                danger: true,
                                onclick: move |_| screen.put_away_open(true),
                            }
                        } else {
                            IconButton {
                                icon: Icon::Archive,
                                label: "Archive",
                                onclick: move |_| screen.put_away_open(false),
                            }
                        }
                    }
                }
                ScrollArea { class: "dm-page-wrap", inner_class: "dm-page-scroll",
                    div { class: "dm-page-body", "data-dui-grow-body": "",
                        textarea {
                            id: "dm-text",
                            class: "dm-text",
                            "aria-label": "Note",
                            placeholder: "What\u{2019}s on your mind?",
                            spellcheck: "false",
                            // The text is given when a page opens, by
                            // `dirt_ui::dom`, and never as an attribute:
                            // rendered again, the page would write it over
                            // what was typed since.
                            oninput: move |event: FormEvent| screen.typed(&event.value()),
                        }
                        if let Some(note) = note {
                            AttachmentPanel {
                                key: "{note}",
                                note_id: note,
                                changed: screen.files_changed,
                            }
                        }
                    }
                }
            }
        }
    }
}
