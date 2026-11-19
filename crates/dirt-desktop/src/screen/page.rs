//! The page (interface design, sections 6.1 and 6.4): where a thought is
//! written. It is always there. With no note open it is blank, with the
//! cursor in it; the first character makes the note, which then saves
//! itself as it is typed.
//!
//! The note's icons sit in a gutter beside its first line, just outside
//! its column, so that they travel with it. The note's files are tiles
//! under its text (6.5); files dropped anywhere on the window come to it
//! (the screen takes the drop).

use dioxus::prelude::*;
use dirt_core::sync::overview::SyncIndicator;
use dirt_ui::components::{IconButton, ScrollArea};
use dirt_ui::dom::{self, Caret};
use dirt_ui::files::FileTiles;
use dirt_ui::Icon;

use super::{Arrival, Screen, Where, PAGE, PAGE_ID};
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
    let done_keys = !app.nav.is_open();
    let from = view.from.map(|from| match from {
        Arrival::Above => "above",
        Arrival::Below => "below",
    });
    let take_focus = view.take_focus;
    let text = view.text.clone();
    // A file not yet sent says "uploading" while it can be sent, and "on
    // this device only" while it cannot (6.5).
    let can_send = matches!(
        screen.sync_indicator(),
        SyncIndicator::Synced { .. } | SyncIndicator::Syncing | SyncIndicator::JustSynced
    );

    rsx! {
        main {
            class: "dk-main",
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
                        FileTiles {
                            key: "{note}",
                            files: screen.files,
                            can_send,
                            class: "dk-files",
                            on_open: move |file| screen.open_file(file),
                            on_save: move |file| screen.save_file(file),
                        }
                    }
                }
            }
        }
    }
}
