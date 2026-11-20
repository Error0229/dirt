//! The left of the window (interface design, sections 3, 6.1 to 6.3 and
//! 6.6): Find with its tags, the list in its layers, and the footer with
//! the sync status and Settings.
//!
//! The list is a list box. It has the keyboard's focus as one stop, and
//! says which row is chosen; the chosen row is the note on the page. The
//! arrow keys move through it, showing each note.

use dioxus::prelude::*;
use dirt_core::models::NoteId;
use dirt_core::store::TagRule;
use dirt_ui::components::note_row::row_element_id;
use dirt_ui::components::{
    EmptyList, FindField, IconButton, IconButtonSize, LayerLine, LoadMore, NoteRow, Pile,
    ScrollArea, SyncStatus, TagList, TagOption,
};
use dirt_ui::dom::{self, Caret};
use dirt_ui::find;
use dirt_ui::listing::{ListItem, Listing, LISTED_NOTES};
use dirt_ui::Icon;

use super::focus::Rail;
use super::keys::{command, is_delete};
use super::{Screen, Where};
use crate::state::AppState;

/// The note next to `id` in the list, for the choice to move to when `id`
/// leaves it: the one below, or above at the end.
fn neighbour(rows: &[NoteId], id: NoteId) -> Option<NoteId> {
    let at = rows.iter().position(|row| *row == id)?;
    rows.get(at + 1)
        .or_else(|| at.checked_sub(1).and_then(|before| rows.get(before)))
        .copied()
}

/// Keys in Find (6.1, 7.4).
fn find_key(screen: &Screen, event: &KeyboardEvent) {
    let mut screen = *screen;
    if event.is_composing() || command(event) {
        return;
    }
    let text = screen.find.text.peek().clone();
    let taken = match event.key() {
        Key::Enter => {
            if find::is_lone_tag(&text) {
                screen.pick_first_tag();
            } else if let Some(first) = screen.row_order().first() {
                screen.open(*first, true);
            }
            true
        }
        Key::Tab if !event.modifiers().shift() && *screen.find.tags_open.peek() => {
            screen.pick_first_tag()
        }
        Key::Backspace if text.is_empty() && !screen.find.tags.peek().is_empty() => {
            screen.untag_last();
            true
        }
        Key::ArrowDown => {
            if *screen.find.tags_open.peek() {
                dom::focus(&["#dui-tags button"], Caret::Kept);
            } else {
                screen.focus_list();
            }
            true
        }
        Key::Escape => {
            screen.find_escape();
            true
        }
        Key::Character(ref typed) if typed == "#" => {
            screen.find.tags_open.set(true);
            false
        }
        _ => false,
    };
    if taken {
        event.prevent_default();
        event.stop_propagation();
    }
}

/// Keys in the list (7.4).
fn list_key(screen: &Screen, event: &KeyboardEvent) {
    let screen = *screen;
    // Only the list's own keys: a key pressed on a row's box or tag is that
    // button's, and Enter there presses it.
    if event.is_composing() || *screen.focus.peek() != Where::List {
        return;
    }
    let rows = screen.row_order();
    let open = screen.open_note();
    let at = open.and_then(|open| rows.iter().position(|row| *row == open));
    let modifiers = event.modifiers();
    let show = |index: usize| {
        if let Some(id) = rows.get(index) {
            screen.open(*id, false);
            dom::reveal(&row_element_id(id));
        }
    };
    let taken = match event.key() {
        _ if command(event) => false,
        Key::ArrowDown => {
            show(at.map_or(0, |at| (at + 1).min(rows.len().saturating_sub(1))));
            true
        }
        Key::ArrowUp => {
            match at {
                Some(at) if at > 0 => show(at - 1),
                _ => screen.focus_find(),
            }
            true
        }
        Key::Home => {
            show(0);
            true
        }
        Key::End => {
            show(rows.len().saturating_sub(1));
            true
        }
        Key::Enter => {
            if let Some(open) = open {
                screen.open(open, true);
            }
            true
        }
        Key::Escape => {
            if *screen.find.archived_view.peek() {
                screen.show_archived(false);
            } else {
                screen.focus_page(Caret::Kept);
            }
            true
        }
        _ if is_delete(event) => {
            if let Some(open) = open {
                let next = neighbour(&rows, open);
                if modifiers.shift() {
                    screen.delete_from_list(open, next);
                } else {
                    screen.archive_or_delete_from_list(open, next);
                }
            }
            true
        }
        // "/" goes to Find, as everywhere outside the text.
        Key::Character(ref typed) if typed == "/" && !modifiers.alt() && !modifiers.ctrl() => {
            screen.focus_find();
            true
        }
        // Any letter goes to Find, and is typed there.
        Key::Character(ref typed)
            if !modifiers.alt()
                && !modifiers.meta()
                && typed.chars().count() == 1
                && !typed.chars().all(char::is_whitespace) =>
        {
            screen.type_into_find(typed);
            true
        }
        _ => false,
    };
    if taken {
        event.prevent_default();
        event.stop_propagation();
    }
}

/// The left of the window.
#[component]
pub fn Side() -> Element {
    let mut screen = use_context::<Screen>();
    let app = use_context::<AppState>();

    let tags = screen.find.tags.read().clone();
    let text = screen.find.text.read().clone();
    let tags_open = *screen.find.tags_open.read();
    let rule = *screen.find.rule.read();
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
            .flatten()
            .filter(|offer| !tags.contains(&offer.tag))
            .map(|offer| TagOption {
                tag: offer.tag.clone(),
                notes: Some(offer.notes),
                picked: false,
            }),
    );

    let none = Listing::default();
    let read = screen.listing.read();
    let listing = read.as_ref().unwrap_or(&none);
    let clock = *screen.clock.read();
    let open = *app.current_note_id.read();
    let listed = |id: NoteId| listing.notes.iter().any(|note| note.id == id);
    let chosen_row = open.filter(|id| listed(*id)).map(|id| row_element_id(&id));
    let exit = screen.exit.read().name();
    let returned = *screen.returned.read();
    let flashing = screen.flashing.read().clone();
    let limit = *screen.find.limit.read();
    let settling = *screen.settling.read();
    let pile = listing.pile.clone();
    // The pile lies under the last note, when nothing is being found and
    // the list holds every note (6.17).
    let show_pile =
        !listing.finding && !listing.archived_view && !listing.more && pile.summary.thoughts > 0;
    let empty =
        listing.notes.is_empty() && !listing.finding && !listing.archived_view && !show_pile;
    let archived_count = listing.counts.archived;
    let items = listing.items(clock);
    let indicator = screen.sync_indicator();

    rsx! {
        aside { class: "dk-side", "aria-label": "Notes",
            div { class: "dk-find",
                FindField {
                    tags,
                    has_text: !text.is_empty(),
                    tags_open,
                    oninput: move |typed: String| screen.find_typed(typed),
                    onkeydown: move |event: KeyboardEvent| find_key(&screen, &event),
                    on_remove_tag: move |tag: String| screen.toggle_tag(&tag),
                    on_tags: move |()| {
                        let now = *screen.find.tags_open.peek();
                        screen.find.tags_open.set(!now);
                        dom::focus(&["#dui-find"], Caret::End);
                    },
                    on_focus: move |inside: bool| {
                        screen.focused(if inside { Where::Find } else { Where::Elsewhere });
                    },
                }
            }
            TagList {
                open: tags_open,
                options,
                any: rule == TagRule::Any,
                on_pick: move |tag: String| screen.toggle_tag(&tag),
                on_rule: move |any: bool| {
                    screen.find.rule.set(if any { TagRule::Any } else { TagRule::All });
                },
            }
            ScrollArea { class: "dk-list-wrap",
                div {
                    id: "dk-list",
                    class: "dk-list",
                    role: "listbox",
                    "aria-label": if listing.archived_view { "Archived notes" } else { "Notes" },
                    tabindex: "0",
                    "aria-activedescendant": chosen_row,
                    "data-dui-list": "",
                    "data-dui-exit": exit,
                    onkeydown: move |event: KeyboardEvent| list_key(&screen, &event),
                    onfocus: move |_| screen.focused(Where::List),
                    onblur: move |_| screen.focused(Where::Elsewhere),
                    div { class: "dui-sel-ind", "aria-hidden": "true" }
                    if listing.archived_view {
                        div { class: "dk-arch-head", "data-key": "archived head",
                            IconButton {
                                icon: Icon::ChevronLeft,
                                label: "Back to the list",
                                shortcut: "Esc",
                                size: IconButtonSize::Small,
                                onclick: move |_| screen.show_archived(false),
                            }
                            span { "Archived" }
                            span { class: "dk-arch-n", "{archived_count}" }
                        }
                    }
                    for item in items {
                        match item {
                            ListItem::Line(name) => rsx! {
                                LayerLine { key: "line {name}", name }
                            },
                            ListItem::Note(summary) => {
                                let id = summary.id;
                                rsx! {
                                    NoteRow {
                                        key: "{id}",
                                        summary: *summary,
                                        selected: open == Some(id),
                                        flash: flashing.contains(&id),
                                        back: returned == Some(id),
                                        settle: settling == Some(id),
                                        on_open: move |()| screen.open(id, true),
                                        on_tag: move |tag: String| screen.narrow_to(&tag),
                                        on_archive: move |()| screen.archive(id),
                                        on_delete: move |()| screen.delete(id),
                                    }
                                }
                            }
                        }
                    }
                    if listing.more {
                        LoadMore {
                            key: "{limit}",
                            on_more: move |()| {
                                let now = *screen.find.limit.peek();
                                screen.find.limit.set(now + LISTED_NOTES);
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
            footer { class: "dk-foot",
                SyncStatus {
                    indicator,
                    now_ms: clock.now_ms,
                    shortcut: "Ctrl R",
                    onclick: move |_| screen.sync_now(),
                }
                span { class: "dk-grow" }
                IconButton {
                    icon: Icon::Plus,
                    label: "New page",
                    shortcut: "Ctrl N",
                    class: "dk-new",
                    onclick: move |_| screen.new_page(),
                }
                IconButton {
                    icon: Icon::Settings,
                    label: "Settings",
                    shortcut: "Ctrl ,",
                    onclick: move |_| screen.open_settings(),
                }
            }
            Rail {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_that_leaves_gives_way_to_the_one_below_or_at_the_end_above() {
        let rows: Vec<NoteId> = (0..3).map(|_| NoteId::new()).collect();
        assert_eq!(neighbour(&rows, rows[0]), Some(rows[1]));
        assert_eq!(neighbour(&rows, rows[2]), Some(rows[1]));
        assert_eq!(neighbour(&rows[..1], rows[0]), None);
        assert_eq!(neighbour(&rows, NoteId::new()), None);
    }
}
