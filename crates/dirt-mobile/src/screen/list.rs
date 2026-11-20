//! The list (interface design, sections 3, 6.1 to 6.3, 6.6, 6.14 and 8.2):
//! the title with 🔍, the sync status and Settings; Find, which slides down
//! under the title, with the strip of tags under it; the cards, in their
//! layers; and the dock at the bottom.
//!
//! The list is pulled down to sync, and a card is swiped left to archive
//! it. Both follow the finger in `dirt-ui`'s script, which says only how
//! they ended. The dock's mic is held to talk, or tapped for hands-free,
//! and listens in a sheet over the lower half of the screen (6.18, 8.9).

use dioxus::prelude::*;
use dirt_core::models::Layer;
use dirt_core::store::TagRule;
use dirt_ui::components::{
    EmptyList, FindField, IconButton, LayerLine, Listening, LoadMore, MicButton, NoteCard,
    NoteSummary, Pile, PullToSync, ScrollArea, SyncStatus, TagOption, TagStrip,
};
use dirt_ui::events::{use_outcome, Outcome};
use dirt_ui::{find, Icon};

use super::{Listing, Screen, BAR_ID, LISTED_NOTES};
use crate::state::AppState;

/// One thing in the list: a note's card, or the line a group starts with.
enum Item {
    Line(String),
    Note(Box<NoteSummary>),
}

/// The list's cards and lines, in order: grouped in layers by when each
/// note was last edited (6.17), or, while finding, the archived notes after
/// the rest under a line of their own (6.19).
fn items(listing: &Listing, clock: dirt_ui::time::Clock) -> Vec<Item> {
    let mut items = Vec::with_capacity(listing.notes.len() + 8);
    let mut layer = None;
    let mut archived_line = false;
    for note in &listing.notes {
        if listing.finding {
            if note.archived && !archived_line {
                archived_line = true;
                items.push(Item::Line("Archived".to_owned()));
            }
        } else if !listing.archived_view {
            let here = Layer::of(note.updated_at, clock.now_ms, clock.utc_offset_secs);
            if layer != Some(here) {
                layer = Some(here);
                if let Some(name) = here.name() {
                    items.push(Item::Line(name));
                }
            }
        }
        let summary = NoteSummary::of(note, &listing.words, clock)
            .files(
                listing.with_files.contains(&note.id),
                listing.first_file.get(&note.id).map(String::as_str),
            )
            .waiting(listing.waiting.contains(&note.id))
            .unread(listing.unread.get(&note.id).map(String::as_str))
            .thumbnail(listing.thumbnails.get(&note.id).cloned());
        items.push(Item::Note(Box::new(summary)));
    }
    items
}

/// Keys in Find. On a phone's keyboard that is Enter (the keyboard's
/// search key) and Backspace.
fn find_key(screen: &Screen, event: &KeyboardEvent) {
    if event.is_composing() {
        return;
    }
    let text = screen.find_text.peek().clone();
    let taken = match event.key() {
        Key::Enter => {
            if find::is_lone_tag(&text) {
                screen.pick_first_tag();
            } else if let Some(first) = screen
                .listing
                .peek()
                .as_ref()
                .and_then(|listing| listing.notes.first().map(|note| note.id))
            {
                screen.open(first);
            }
            true
        }
        Key::Backspace if text.is_empty() && !screen.tags.peek().is_empty() => {
            screen.untag_last();
            true
        }
        _ => false,
    };
    if taken {
        event.prevent_default();
        event.stop_propagation();
    }
}

/// The list's side of the screen.
#[component]
#[allow(clippy::too_many_lines)]
pub fn Main() -> Element {
    let mut screen = use_context::<Screen>();
    let app = use_context::<AppState>();

    let find_open = *screen.find_open.read();
    let tags = screen.tags.read().clone();
    let text = screen.find_text.read().clone();
    let tags_open = *screen.tags_open.read();
    let rule = *screen.rule.read();
    let mut options: Vec<TagOption> = tags
        .iter()
        .map(|tag| TagOption {
            tag: tag.clone(),
            notes: None,
            picked: true,
        })
        .collect();
    // The offers are read again after each pick; until they are, a tag
    // just picked may still be among them.
    options.extend(
        screen
            .tag_offers
            .read()
            .iter()
            .filter(|offer| !tags.contains(&offer.tag))
            .map(|offer| TagOption {
                tag: offer.tag.clone(),
                notes: Some(offer.notes),
                picked: false,
            }),
    );

    // Nothing is said to be empty before the store has answered.
    let read = screen.listing.read().is_some();
    let listing = screen.listing.read().clone().unwrap_or_default();
    let clock = *screen.clock.read();
    let exit = screen.exit.read().name();
    let returned = *screen.returned.read();
    let flashing = screen.flashing.read().clone();
    let limit = *screen.limit.read();
    let settling = *screen.settling.read();
    let pile = listing.pile.clone();
    // The pile lies under the last card, when nothing is being found and
    // the list holds every note (6.17).
    let show_pile =
        !listing.finding && !listing.archived_view && !listing.more && pile.summary.thoughts > 0;
    let empty = read
        && listing.notes.is_empty()
        && !listing.finding
        && !listing.archived_view
        && !show_pile;
    let archived_count = listing.counts.archived;
    let archived_view = listing.archived_view;
    let items = items(&listing, clock);
    let indicator = screen.sync_indicator();
    let syncing = matches!(indicator, dirt_core::sync::overview::SyncIndicator::Syncing);
    let issue = app.startup_issue.read().clone();

    // Android's Back closes Find, and the list of archived notes.
    let find_back = use_outcome(move |outcome| {
        if outcome == Outcome::Back {
            screen.find_back();
        }
    });
    let archived_back = use_outcome(move |outcome| {
        if outcome == Outcome::Back {
            screen.show_archived(false);
        }
    });
    // Back while it listens keeps what was said, as leaving does.
    let talk_back = use_outcome(move |outcome| {
        if outcome == Outcome::Back {
            screen.talking.end(true);
        }
    });
    let talking = screen
        .talking
        .view
        .read()
        .filter(|_| !*screen.talking_on_page.read());
    let unheard = screen.talking.unheard.read().clone();
    let mic_tip = unheard
        .as_ref()
        .map_or("Hold to talk, or tap", crate::voice::Unheard::tip);
    let refused = unheard.is_some();

    rsx! {
        div { class: if talking.is_some() { "dm talking" } else { "dm" },
            header { class: "dm-top",
                h1 { "Notes" }
                IconButton {
                    icon: Icon::Search,
                    label: "Find",
                    pressed: find_open,
                    onclick: move |_| {
                        if *screen.find_open.peek() {
                            screen.close_find();
                        } else {
                            screen.open_find();
                        }
                    },
                }
                SyncStatus {
                    indicator,
                    now_ms: clock.now_ms,
                    onclick: move |_| screen.sync_now(),
                }
                IconButton {
                    icon: Icon::Settings,
                    label: "Settings",
                    onclick: move |_| screen.open_settings(),
                }
            }
            div {
                class: if find_open { "dm-find on" } else { "dm-find" },
                "inert": if find_open { None } else { Some("") },
                "data-dui-id": "{find_back}",
                "data-dui-back": find_open.then_some(""),
                FindField {
                    tags: tags.clone(),
                    has_text: !text.is_empty(),
                    tags_open,
                    oninput: move |typed: String| screen.find_typed(typed),
                    onkeydown: move |event: KeyboardEvent| find_key(&screen, &event),
                    on_remove_tag: move |tag: String| screen.toggle_tag(&tag),
                    on_tags: move |()| {
                        let now = *screen.tags_open.peek();
                        screen.tags_open.set(!now);
                    },
                    on_close: move |()| screen.close_find(),
                }
            }
            TagStrip {
                open: tags_open && find_open,
                options,
                any: rule == TagRule::Any,
                on_pick: move |tag: String| screen.toggle_tag(&tag),
                on_rule: move |any: bool| {
                    screen.rule.set(if any { TagRule::Any } else { TagRule::All });
                },
            }
            if let Some(message) = issue {
                div { class: "dm-alarm", role: "alert",
                    span { class: "dm-grow", "{message}" }
                    IconButton {
                        icon: Icon::X,
                        label: "Dismiss",
                        onclick: move |_| {
                            let mut issue = app.startup_issue;
                            issue.set(None);
                        },
                    }
                }
            }
            ScrollArea { class: "dm-list-wrap", inner_class: "dm-scroll",
                PullToSync { busy: syncing, on_pull: move |()| screen.pulled(),
                    div {
                        id: "dm-list",
                        class: "dm-list",
                        role: "list",
                        "aria-label": if archived_view { "Archived notes" } else { "Notes" },
                        "data-dui-list": "",
                        "data-dui-exit": exit,
                        "data-dui-id": "{archived_back}",
                        "data-dui-back": archived_view.then_some(""),
                        if archived_view {
                            div { class: "dm-arch-head", "data-key": "archived head",
                                IconButton {
                                    icon: Icon::ChevronLeft,
                                    label: "Back to the list",
                                    onclick: move |_| screen.show_archived(false),
                                }
                                span { "Archived" }
                                span { class: "dm-arch-n", "{archived_count}" }
                            }
                        }
                        for item in items {
                            match item {
                                Item::Line(name) => rsx! {
                                    LayerLine { key: "line {name}", name }
                                },
                                Item::Note(summary) => {
                                    let id = summary.id;
                                    rsx! {
                                        NoteCard {
                                            key: "{id}",
                                            summary: *summary,
                                            flash: flashing.contains(&id),
                                            back: returned == Some(id),
                                            settle: settling == Some(id),
                                            on_open: move |()| screen.open(id),
                                            on_tag: move |tag: String| screen.narrow_to(&tag),
                                            on_swipe: move |()| screen.swiped(id),
                                        }
                                    }
                                }
                            }
                        }
                        if listing.more {
                            LoadMore {
                                key: "{limit}",
                                on_more: move |()| {
                                    let now = *screen.limit.peek();
                                    screen.limit.set(now + LISTED_NOTES);
                                },
                            }
                        }
                        if show_pile {
                            Pile {
                                heap: pile.heap,
                                summary: pile.summary,
                                on_archived: move |()| screen.show_archived(true),
                            }
                        }
                        if empty {
                            EmptyList {}
                        }
                    }
                }
            }
            if let Some(view) = talking {
                div {
                    key: "{view.serial}",
                    class: "dm-talk",
                    "data-dui-id": "{talk_back}",
                    "data-dui-back": view.ended.is_none().then_some(""),
                    Listening {
                        ms: view.ms,
                        level: view.level,
                        ended: view.ended,
                        on_keep: move |()| screen.talking.end(true),
                        on_throw: move |()| screen.talking.end(false),
                        on_gone: move |()| screen.talked(view.serial),
                    }
                }
            }
            footer { class: "dm-dock",
                button {
                    id: BAR_ID,
                    r#type: "button",
                    class: "dm-bar",
                    onclick: move |_| screen.new_page(),
                    "What\u{2019}s on your mind?"
                }
                if crate::voice::offered() {
                    MicButton {
                        label: mic_tip,
                        refused,
                        class: "dm-mic",
                        on_talk: move |talk| screen.talk(talk),
                    }
                }
            }
        }
    }
}
