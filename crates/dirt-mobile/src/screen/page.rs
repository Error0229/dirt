//! The page (interface design, sections 6.1, 6.4 and 8.3): a note, full
//! screen, grown out of its card, or a new page grown out of the bar. It
//! saves itself as it is typed; there is no Save button.
//!
//! The top bar has ⌄ on a new page and ‹ on a note, the mic where the
//! build has voice, the paperclip, and the archive box (on an archived
//! note, "back to the list" and the bin). It fades while the person types,
//! and comes back when the page is touched. The note's files are tiles
//! under its text (6.5), `dirt-ui`'s, with the preview over the screen.
//!
//! The mic listens under the bar, with the keyboard down: the grains, the
//! timer, ✕ and ✓, as the dock's sheet has them. What is said is added at
//! the end of the note (6.18, 8.9). Back while it listens keeps what was
//! said, and the page stays.
//!
//! The page is always in the screen, and so is its text field: a page let
//! go keeps its words while it shrinks away, and the next page is given
//! its own.

use dioxus::prelude::*;
use dirt_core::sync::overview::SyncIndicator;
use dirt_ui::components::{GrowPage, IconButton, Listening, MicButton, ScrollArea};
use dirt_ui::events::{use_outcome, Outcome};
use dirt_ui::files::FileTiles;
use dirt_ui::typing::use_typing;
use dirt_ui::Icon;

use super::Screen;

/// The page.
#[component]
pub fn Page() -> Element {
    let screen = use_context::<Screen>();
    // The page's text, however long, in order with its keys and presses
    // (`dirt_ui::typing`).
    let field = use_typing(move |text: String| screen.typed(&text));
    // Back while the page listens keeps what was said, as leaving does,
    // and the page stays.
    let talk_back = use_outcome(move |outcome| {
        if outcome == Outcome::Back {
            screen.talking.end(true);
        }
    });
    let view = screen.page.read().clone();
    let typing = *screen.typing.read();
    let note = view.note;
    let archived = view.archived;
    let talking = screen
        .talking
        .view
        .read()
        .filter(|_| *screen.talking_on_page.read());
    let unheard = screen.talking.unheard.read().clone();
    let mic_tip = unheard
        .as_ref()
        .map_or("Hold to talk, or tap", crate::voice::Unheard::tip);
    let refused = unheard.is_some();
    // A file not yet sent says "uploading" while it can be sent, and "on
    // this device only" while it cannot (6.5).
    let can_send = matches!(
        screen.sync_indicator(),
        SyncIndicator::Synced { .. } | SyncIndicator::Syncing | SyncIndicator::JustSynced
    );
    let bar = match (typing, talking.is_some()) {
        (_, true) => "dm-page-bar talking",
        (true, false) => "dm-page-bar typing",
        (false, false) => "dm-page-bar",
    };

    rsx! {
        GrowPage {
            open: view.open,
            from: view.from.clone(),
            label: if view.new_page { "New note" } else { "Note" },
            on_back: move |()| screen.done(),
            class: "dm-page",
            div {
                class: if talking.is_some() { "dm-page-in talking" } else { "dm-page-in" },
                // Touching the page brings its bar back.
                onpointerdown: move |_| {
                    let mut typing = screen.typing;
                    if *typing.peek() {
                        typing.set(false);
                    }
                },
                div {
                    class: bar,
                    role: "toolbar",
                    "aria-label": "The note",
                    "data-dui-grow-bar": "",
                    // Pressed, Done leaves the keyboard with the field
                    // until the page has closed (`GrowPage`), so that the
                    // web view is not resized under the movement (#322).
                    span { class: "dm-done", "data-dui-keep-focus": "",
                        IconButton {
                            icon: if view.new_page { Icon::ChevronDown } else { Icon::ChevronLeft },
                            label: "Done",
                            onclick: move |_| screen.done(),
                        }
                    }
                    span { class: "dm-grow" }
                    if crate::voice::offered() {
                        MicButton {
                            label: mic_tip,
                            refused,
                            class: "dm-page-mic",
                            on_talk: move |talk| screen.page_talk(talk),
                        }
                    }
                    // While it listens the page's other icons go (7.11).
                    if talking.is_none() {
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
                }
                if let Some(talk) = talking {
                    div {
                        key: "{talk.serial}",
                        class: "dm-page-talk",
                        "data-dui-id": "{talk_back}",
                        "data-dui-back": talk.ended.is_none().then_some(""),
                        Listening {
                            ms: talk.ms,
                            level: talk.level,
                            ended: talk.ended,
                            on_keep: move |()| screen.talking.end(true),
                            on_throw: move |()| screen.talking.end(false),
                            on_gone: move |()| screen.talked(talk.serial),
                        }
                    }
                }
                ScrollArea { class: "dm-page-wrap", inner_class: "dm-page-scroll",
                    div {
                        class: "dm-page-body",
                        "data-dui-grow-body": "",
                        // Nothing is written while it listens (6.18).
                        "inert": talking.is_some().then_some(""),
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
                            "data-dui-typed": field.id(),
                        }
                        {field.carrier()}
                        if let Some(note) = note {
                            FileTiles {
                                key: "{note}",
                                files: screen.files,
                                note,
                                can_send,
                                class: "dm-files",
                                on_open: move |file| crate::files::open_in_another_app(&screen.files, screen.toasts, file),
                                on_save: move |file| crate::files::save_a_copy(&screen.files, screen.toasts, file),
                            }
                        }
                    }
                }
            }
        }
    }
}
