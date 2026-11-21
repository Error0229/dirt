//! The gallery: every component on one screen, for looking at them while
//! they are built.
//!
//! It is for development only, and is in a build only when the `gallery`
//! feature is on. Each app has a feature of the same name that shows the
//! gallery in place of the app:
//!
//! ```text
//! cd crates/dirt-desktop && dx serve --features gallery
//! make run-gallery-android
//! ```
//!
//! The theme, the accent, the text size and reduced motion can be switched
//! at its top. Its list of thirty rows can be shuffled, added to and taken
//! from, to show the list's animation, and it says how long each frame of
//! a shuffle took. Its card can be swiped; beside it the gallery counts
//! the pointer's movements the script handled and the messages Rust got.
//! The parts of a list of notes (Find with its tags, rows, a layer's line,
//! the sync status) are shown with notes made up for them.

use dioxus::prelude::*;
use dirt_core::models::{Accent, ThemeMode};
use dirt_core::store::{AttachedFile, FileStanding};
use serde::Deserialize;

use crate::appearance::{theme_name, Appearance, Platform, TextSize};
use crate::components::{
    use_toasts, AccentDots, Banner, Button, ButtonKind, ConfirmInPlace, FindField, IconButton,
    IconButtonSize, LayerLine, ListExit, NoteCard, NoteRow, NoteSummary, Panel, Pile, PullToSync,
    ScrollArea, Segment, Segmented, Sheet, Slider, Swipeable, Switch, SyncStatus, TagList,
    TagOption, TagStrip, TextSizeSlider, Toast,
};
use crate::components::{Ended, Listening, MicButton, Talk};
use crate::events::Bus;
use crate::files::{FileMini, FileTile, ShelfFrame};
use crate::icons::{Icon, IconSize, IconSvg};
use crate::tokens::NEUTRALS;
use crate::UiRoot;

const STYLES: &str = include_str!("style/gallery.css");

/// How many rows the list starts with.
const ROWS: u64 = 30;

/// Things a person might have written, for the list's rows.
const THOUGHTS: [&str; 10] = [
    "Ask Jordan about the Q3 offsite",
    "Sourdough: 78% hydration, fold every 30 minutes",
    "The bike shop closes at six on Saturdays",
    "Idea: a map of every bench with a view",
    "Call the landlord about the boiler",
    "Read the chapter on tides again",
    "Birthday: a kite, not a book",
    "Renew the library card before the trip",
    "That song from the cafe: ask what it was",
    "Water the fig on Thursdays",
];

/// How long each frame took while the list moved, as the script measured.
#[derive(Debug, Clone, PartialEq, Deserialize)]
struct Frames {
    #[serde(rename = "frames")]
    count: u32,
    ms: f64,
    median: f64,
    p95: f64,
    longest: f64,
    late: u32,
    hz: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Row {
    id: u64,
    text: String,
    /// Came back by Undo: it drops in with a bounce.
    back: bool,
}

fn row(id: u64) -> Row {
    let thought = THOUGHTS[usize::try_from(id).unwrap_or(0) % THOUGHTS.len()];
    Row {
        id,
        text: format!("{thought} ({id})"),
        back: false,
    }
}

/// A different order each time, the same on every machine.
fn shuffle(rows: &mut [Row], seed: &mut u64) {
    for index in (1..rows.len()).rev() {
        // xorshift: enough to mix up a list.
        *seed ^= *seed << 13;
        *seed ^= *seed >> 7;
        *seed ^= *seed << 17;
        let other = usize::try_from(*seed % (index as u64 + 1)).unwrap_or(0);
        rows.swap(index, other);
    }
}

/// The gallery. `initial` is how the device's settings say it should look;
/// `on_appearance` is told each time the person chooses otherwise, so that
/// the app can keep the choice on the device.
#[component]
pub fn Gallery(initial: Appearance, on_appearance: EventHandler<Appearance>) -> Element {
    let appearance = use_signal(|| initial);
    let reduced = use_signal(|| false);
    rsx! {
        UiRoot { appearance: appearance(), reduced_motion: reduced(),
            style { "{STYLES}" }
            Page { appearance, reduced, on_appearance }
        }
    }
}

#[component]
fn Page(
    appearance: Signal<Appearance>,
    reduced: Signal<bool>,
    on_appearance: EventHandler<Appearance>,
) -> Element {
    let platform = Platform::current();
    let mut choose = move |next: Appearance| {
        appearance.set(next);
        on_appearance.call(next);
    };
    let now = appearance();
    let mut panel = use_signal(|| false);
    let mut sheet = use_signal(|| false);
    rsx! {
        div { class: "gal dui-steps-back",
            header { class: "gal-top",
                h1 { "Dirt UI" }
                Segmented {
                    label: "Theme",
                    segments: vec![
                        Segment::new("system", "Auto").icon(Icon::Monitor),
                        Segment::new("light", "Light").icon(Icon::Sun),
                        Segment::new("dark", "Dark").icon(Icon::Moon),
                    ],
                    value: theme_name(now.theme),
                    onchange: move |value: String| {
                        let theme = match value.as_str() {
                            "light" => ThemeMode::Light,
                            "dark" => ThemeMode::Dark,
                            _ => ThemeMode::System,
                        };
                        choose(Appearance { theme, ..appearance() });
                    },
                }
                AccentDots {
                    value: now.accent,
                    onchange: move |accent: Accent| choose(Appearance { accent, ..appearance() }),
                }
                TextSizeSlider {
                    value: now.text_size,
                    onchange: move |text_size: TextSize| choose(Appearance { text_size, ..appearance() }),
                }
                label { class: "gal-switch",
                    Switch {
                        checked: reduced(),
                        label: "Reduced motion",
                        onchange: move |on| reduced.set(on),
                    }
                    "Reduced motion"
                }
            }
            ScrollArea { class: "gal-page", inner_class: "gal-in",
                Buttons {}
                Feedback { platform }
                Controls { appearance, on_appearance }
                Surfaces { panel, sheet }
                AnimatedList {}
                ListParts {}
                Piles {}
                SwipeCard {}
                VoiceShown {}
                FileTilesShown {}
                AndroidList {}
                Icons {}
                Type {}
                Colours {}
            }
        }
        // Beside the screen, not inside it: the screen steps back behind
        // a sheet, and what is drawn over it must not step back with it.
        Panel { open: panel(), label: "Settings", on_close: move |()| panel.set(false), SettingsLike {} }
        Sheet { open: sheet(), title: "Settings", on_close: move |()| sheet.set(false), SettingsLike {} }
    }
}

/// Six sections of rows, as Settings will have, to fill a panel or sheet.
#[component]
fn SettingsLike() -> Element {
    rsx! {
        ScrollArea { class: "gal-fill", inner_class: "gal-settings",
            for section in ["Account", "Devices", "Sync", "Appearance", "Voice", "Export"] {
                section { key: "{section}", "data-dui-rise": "",
                    h3 { "{section}" }
                    div { class: "gal-card gal-rows",
                        div { class: "gal-setting",
                            div {
                                div { class: "gal-t", "{section}" }
                                div { class: "gal-s", "One line of grey detail" }
                            }
                            Button { small: true, onclick: move |_| {}, "Change" }
                        }
                        div { class: "gal-setting",
                            div {
                                div { class: "gal-t", "Another row" }
                                div { class: "gal-s", "At most one control on the right" }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn Buttons() -> Element {
    let toasts = use_toasts();
    let mut tags = use_signal(|| false);
    let say = move |what: &'static str| move |_| toasts.show(Toast::new(what));
    rsx! {
        h2 { "Icon buttons" }
        div { class: "gal-card gal-line",
            IconButton { icon: Icon::Search, label: "Find", shortcut: "Ctrl F", onclick: say("Find") }
            IconButton { icon: Icon::Mic, label: "Talk", shortcut: "hold Ctrl Alt N", onclick: say("Talk") }
            IconButton { icon: Icon::Paperclip, label: "Attach a file", shortcut: "Ctrl Shift A", onclick: say("Attach a file") }
            IconButton { icon: Icon::Archive, label: "Archive", shortcut: "Ctrl Del", onclick: say("Archive") }
            IconButton { icon: Icon::Trash, label: "Delete for good", shortcut: "Ctrl Shift Del", danger: true, onclick: say("Delete for good") }
            IconButton {
                icon: Icon::Hash,
                label: "Tags",
                shortcut: "#",
                pressed: tags(),
                onclick: move |_| tags.toggle(),
            }
            IconButton { icon: Icon::Settings, label: "Settings", shortcut: "Ctrl ,", onclick: say("Settings") }
            IconButton { icon: Icon::Refresh, label: "Sync now", disabled: true, onclick: say("Sync now") }
            IconButton { icon: Icon::Check, label: "Done", shortcut: "Esc", size: IconButtonSize::Small, onclick: say("Done") }
            IconButton { icon: Icon::X, label: "Remove", size: IconButtonSize::Small, danger: true, onclick: say("Remove") }
        }
        h2 { "Buttons" }
        div { class: "gal-card gal-line",
            Button { kind: ButtonKind::Primary, onclick: say("Primary"), "Sign in" }
            Button { onclick: say("Quiet"), "Export" }
            Button { kind: ButtonKind::Ghost, onclick: say("Ghost"), "They don't" }
            Button { kind: ButtonKind::Danger, onclick: say("Danger"), "Delete the account" }
            Button { kind: ButtonKind::DangerFill, onclick: say("The final confirm"), "Remove" }
            Button { kind: ButtonKind::Primary, disabled: true, onclick: say("Never"), "Waiting for browser" }
        }
        div { class: "gal-card gal-line",
            Button { kind: ButtonKind::Primary, small: true, onclick: say("Primary"), "Review" }
            Button { small: true, onclick: say("Quiet"), "Make one" }
            Button { kind: ButtonKind::Ghost, small: true, onclick: say("Ghost"), "Sign out" }
            Button { kind: ButtonKind::Danger, small: true, onclick: say("Danger"), "Remove" }
        }
    }
}

#[component]
fn Feedback(platform: Platform) -> Element {
    let toasts = use_toasts();
    let mut undone = use_signal(|| 0u32);
    let mut joining = use_signal(|| false);
    let undo = use_callback(move |()| undone += 1);
    rsx! {
        h2 { "Toast and banner" }
        div { class: "gal-card gal-line",
            Button { onclick: move |_| toasts.show(Toast::new("12 notes exported")), "Toast" }
            Button {
                onclick: move |_| toasts.show(Toast::new("Archived").icon(Icon::Archive).undo(undo)),
                "Toast with Undo"
            }
            Button {
                onclick: move |_| toasts.show(Toast::new("Note deleted").icon(Icon::Trash).undo(undo).hold(true)),
                "Toast that stays"
            }
            Button {
                onclick: move |_| {
                    toasts
                        .show_error(
                            Toast::new("A note could not be saved")
                                .icon(Icon::Alert)
                                .detail("The disk is full.")
                                .action("Copy text", Icon::Copy, Callback::new(|()| {})),
                        );
                },
                "An error"
            }
            Button {
                kind: ButtonKind::Ghost,
                onclick: move |_| joining.toggle(),
                "A device asks to join"
            }
            span { class: "gal-quiet", "Undone {undone} times" }
        }
        Banner {
            open: joining(),
            title: "Work laptop wants to join",
            detail: "Compare its code with this device's".to_owned(),
            action: "Review",
            on_action: move |()| {
                joining.set(false);
                toasts.show(Toast::new("Work laptop is in").icon(Icon::Check));
            },
            on_dismiss: move |()| joining.set(false),
            platform,
        }
    }
}

#[component]
fn Controls(appearance: Signal<Appearance>, on_appearance: EventHandler<Appearance>) -> Element {
    let mut format = use_signal(|| "markdown".to_owned());
    let mut keep = use_signal(|| true);
    let mut volume = use_signal(|| 40.0);
    rsx! {
        h2 { "Segmented control, slider, switch, dots" }
        div { class: "gal-card gal-rows",
            div { class: "gal-setting",
                div {
                    div { class: "gal-t", "Export as" }
                    div { class: "gal-s", "Chosen: {format}" }
                }
                Segmented {
                    label: "Export format",
                    segments: vec![Segment::new("json", "JSON"), Segment::new("markdown", "Markdown")],
                    value: format(),
                    small: true,
                    onchange: move |value| format.set(value),
                }
            }
            div { class: "gal-setting",
                div {
                    div { class: "gal-t", "Keep the recording" }
                    div { class: "gal-s", if keep() { "Kept with the note" } else { "Text only" } }
                }
                Switch {
                    checked: keep(),
                    label: "Keep the recording",
                    onchange: move |on| keep.set(on),
                }
            }
            div { class: "gal-setting",
                div {
                    div { class: "gal-t", "A slider" }
                    div { class: "gal-s", "Let go at {volume}" }
                }
                Slider {
                    label: "A slider",
                    min: 0.0,
                    max: 100.0,
                    value: volume(),
                    onchange: move |value| volume.set(value),
                }
            }
            div { class: "gal-setting",
                div {
                    div { class: "gal-t", "Accent" }
                    div { class: "gal-s", "{appearance().accent.label()}" }
                }
                AccentDots {
                    value: appearance().accent,
                    onchange: move |accent: Accent| {
                        let next = Appearance { accent, ..appearance() };
                        appearance.set(next);
                        on_appearance.call(next);
                    },
                }
            }
        }
    }
}

#[component]
fn Surfaces(panel: Signal<bool>, sheet: Signal<bool>) -> Element {
    let toasts = use_toasts();
    let mut asking = use_signal(|| false);
    let mut devices = use_signal(|| vec!["Pixel 8", "Work laptop"]);
    rsx! {
        h2 { "Panel, sheet, confirmation in place" }
        div { class: "gal-card gal-line",
            Button { onclick: move |_| panel.set(true), "Open the panel" }
            Button { onclick: move |_| sheet.set(true), "Open the sheet" }
            span { class: "gal-quiet", "The panel is the desktop's, the sheet Android's. Both are here to be looked at." }
        }
        div { class: "gal-card gal-rows gal-flush",
            for device in devices() {
                if device == "Pixel 8" {
                    ConfirmInPlace {
                        key: "{device}",
                        asking: asking(),
                        question: "Remove Pixel 8? It stops getting notes.",
                        confirm: "Remove",
                        on_confirm: move |()| {
                            asking.set(false);
                            devices.write().retain(|name| *name != "Pixel 8");
                            toasts.show(Toast::new("Pixel 8 removed"));
                        },
                        on_cancel: move |()| asking.set(false),
                        div { class: "gal-setting",
                            span { class: "gal-ic", IconSvg { icon: Icon::Phone } }
                            div { class: "gal-grow",
                                div { class: "gal-t", "Pixel 8" }
                                div { class: "gal-s", "Seen 2 hours ago" }
                            }
                            Button {
                                kind: ButtonKind::Danger,
                                small: true,
                                onclick: move |_| asking.set(true),
                                "Remove"
                            }
                        }
                    }
                } else {
                    div { key: "{device}", class: "gal-setting",
                        span { class: "gal-ic", IconSvg { icon: Icon::Laptop } }
                        div { class: "gal-grow",
                            div { class: "gal-t", "{device}" }
                            div { class: "gal-s", "This device" }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn AnimatedList() -> Element {
    let mut rows = use_signal(|| (1..=ROWS).rev().map(row).collect::<Vec<_>>());
    let mut next = use_signal(|| ROWS + 1);
    let mut seed = use_signal(|| 0x2545_F491_4F6C_DD1D_u64);
    let mut removed = use_signal(Vec::<(usize, Row)>::new);
    let mut frames = use_signal(|| None::<Frames>);
    let count = rows.read().len();
    let measured = frames().map_or_else(
        || "Shuffle, and the time each frame took is written here.".to_owned(),
        |measured| {
            format!(
                "{} frames in {} ms: median {} ms ({} Hz), 95th percentile {} ms, longest {} ms, {} late",
                measured.count,
                measured.ms,
                measured.median,
                measured.hz,
                measured.p95,
                measured.longest,
                measured.late,
            )
        },
    );
    rsx! {
        h2 { "A list that animates" }
        div { class: "gal-card gal-line",
            Button {
                kind: ButtonKind::Primary,
                onclick: move |_| {
                    // The script times the frames from now; the rows move
                    // in the meantime.
                    spawn(async move {
                        let measured = document::eval("return await DirtUi.frames(700);")
                            .join::<Frames>()
                            .await;
                        match measured {
                            Ok(measured) => {
                                tracing::info!(
                                    "gallery: a shuffle of {count} rows drew {} frames in {} ms: median {} ms ({} Hz), 95th percentile {} ms, longest {} ms, {} late",
                                    measured.count,
                                    measured.ms,
                                    measured.median,
                                    measured.hz,
                                    measured.p95,
                                    measured.longest,
                                    measured.late,
                                );
                                frames.set(Some(measured));
                            }
                            Err(error) => tracing::warn!("gallery: the frames were not measured: {error}"),
                        }
                    });
                    let mut order = seed();
                    shuffle(&mut rows.write(), &mut order);
                    seed.set(order);
                },
                "Shuffle"
            }
            Button {
                onclick: move |_| {
                    let id = next();
                    next.set(id + 1);
                    rows.write().insert(0, row(id));
                },
                "Add"
            }
            Button {
                disabled: count == 0,
                onclick: move |_| {
                    let at = count.min(3) - 1;
                    let gone = rows.write().remove(at);
                    removed.write().push((at, gone));
                },
                "Remove"
            }
            Button {
                kind: ButtonKind::Ghost,
                disabled: removed.read().is_empty(),
                onclick: move |_| {
                    if let Some((at, mut back)) = removed.write().pop() {
                        back.back = true;
                        let at = at.min(rows.read().len());
                        rows.write().insert(at, back);
                    }
                },
                "Undo"
            }
            span { class: "gal-quiet", "{count} rows" }
        }
        p { class: "gal-frames", id: "gal-frames", "{measured}" }
        ScrollArea {
            class: "gal-list",
            inner_class: "gal-list-in",
            list: true,
            exit: ListExit::Tear,
            for row in rows() {
                div {
                    key: "{row.id}",
                    class: "gal-item",
                    "data-key": "{row.id}",
                    "data-dui-enter": row.back.then_some("pop"),
                    span { class: "gal-item-t", "{row.text}" }
                    span { class: "gal-item-r", "{row.id}m" }
                }
            }
        }
    }
}

/// The parts of a list of notes: Find with its tokens and its list of
/// tags, rows with what Find matched, a layer's line, and the sync status
/// in each of its states. The notes are made up; nothing is stored.
#[component]
fn ListParts() -> Element {
    let mut tags = use_signal(|| vec!["work".to_owned()]);
    let mut open = use_signal(|| true);
    let mut any = use_signal(|| false);
    let mut chosen = use_signal(|| None::<usize>);
    let mut words = use_signal(String::new);
    let clock = crate::time::Clock::now();
    let found = crate::text::find_words(&words());
    let notes: Vec<dirt_core::models::Note> = [
        "Ask Jordan about the Q3 offsite
bring the budget #work",
        "Sourdough: 78% hydration
fold every 30 minutes #baking #bread #weekend",
        "Call the landlord about the #home boiler",
    ]
    .iter()
    .enumerate()
    .map(|(index, text)| {
        let mut note = dirt_core::models::Note::new(*text);
        note.updated_at -= i64::try_from(index).unwrap_or_default() * 3_600_000;
        note.archived = index == 2;
        note
    })
    .collect();
    let picked = tags();
    let offers = ["home", "baking", "idea"]
        .iter()
        .filter(|tag| !picked.iter().any(|chosen| chosen == *tag))
        .map(|tag| TagOption {
            tag: (*tag).to_owned(),
            notes: Some(2),
            picked: false,
        });
    let options: Vec<TagOption> = picked
        .iter()
        .map(|tag| TagOption {
            tag: tag.clone(),
            notes: None,
            picked: true,
        })
        .chain(offers)
        .collect();
    let mut toggle = move |tag: String| {
        let mut chosen_tags = tags.write();
        if let Some(at) = chosen_tags.iter().position(|chosen| *chosen == tag) {
            chosen_tags.remove(at);
        } else {
            chosen_tags.push(tag);
        }
    };
    let states = [
        dirt_core::sync::overview::SyncIndicator::Synced {
            last_synced_ms: Some(clock.now_ms - 120_000),
        },
        dirt_core::sync::overview::SyncIndicator::Syncing,
        dirt_core::sync::overview::SyncIndicator::JustSynced,
        dirt_core::sync::overview::SyncIndicator::Offline { waiting: 2 },
        dirt_core::sync::overview::SyncIndicator::Problem { notes: 1, files: 0 },
        dirt_core::sync::overview::SyncIndicator::SignedOut,
    ];
    rsx! {
        h2 { "A list of notes" }
        div { class: "gal-card gal-notes",
            FindField {
                tags: picked.clone(),
                has_text: !words().is_empty(),
                tags_open: open(),
                oninput: move |text: String| words.set(text),
                onkeydown: move |_| {},
                on_remove_tag: move |tag: String| toggle(tag),
                on_tags: move |()| open.toggle(),
            }
            TagList {
                open: open(),
                options,
                any: any(),
                on_pick: move |tag: String| toggle(tag),
                on_rule: move |rule: bool| any.set(rule),
            }
            div { "data-dui-list": "", "data-dui-exit": "none", role: "listbox", "aria-label": "Notes",
                div { class: "dui-sel-ind", "aria-hidden": "true" }
                for (index , note) in notes.iter().enumerate().take(1) {
                    NoteRow {
                        key: "{note.id}",
                        summary: NoteSummary::of(note, &found, clock).waiting(true),
                        selected: chosen() == Some(index),
                        on_open: move |()| chosen.set(Some(index)),
                        on_tag: move |tag: String| toggle(tag),
                        on_archive: move |()| {},
                        on_delete: move |()| {},
                    }
                }
                LayerLine { name: "This week" }
                for (index , note) in notes.iter().enumerate().skip(1) {
                    NoteRow {
                        key: "{note.id}",
                        summary: NoteSummary::of(note, &found, clock).files(index == 1, None),
                        selected: chosen() == Some(index),
                        on_open: move |()| chosen.set(Some(index)),
                        on_tag: move |tag: String| toggle(tag),
                        on_archive: move |()| {},
                        on_delete: move |()| {},
                    }
                }
            }
        }
        div { class: "gal-card gal-line",
            for (index , state) in states.into_iter().enumerate() {
                SyncStatus { key: "{index}", indicator: state, now_ms: clock.now_ms, shortcut: "Ctrl R", onclick: move |_| {} }
            }
        }
    }
}

/// A made-up pile of `count` thoughts, made one by one over the past
/// `days`, `archived` of them archived.
fn made_up_pile(count: u64, days: u64, archived: u64) -> Vec<dirt_core::models::PileNote> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let mut notes: Vec<dirt_core::models::PileNote> = (0..count)
        .map(|index| {
            let made = now - days * 86_400 + index * days * 86_400 / count.max(1);
            let time = uuid::Timestamp::from_unix(uuid::NoContext, made, 0);
            dirt_core::models::PileNote {
                id: dirt_core::models::NoteId::from_uuid(uuid::Uuid::new_v7(time)),
                edited_at: i64::try_from(made * 1000).unwrap_or_default(),
                archived: index < archived,
            }
        })
        .collect();
    notes.sort_unstable_by_key(|note| note.id);
    notes
}

/// The pile under a list (6.17): a pinch of a fortnight's thoughts, which
/// a button adds to, and a year's.
#[component]
fn Piles() -> Element {
    let mut small = use_signal(|| made_up_pile(12, 14, 2));
    let year = use_hook(|| made_up_pile(1_250, 365, 3));
    let offset = crate::time::Clock::now().utc_offset_secs;
    let show = |notes: &[dirt_core::models::PileNote]| {
        (
            std::sync::Arc::new(crate::pile::Heap::of(notes, crate::pile::LIST_WIDTH)),
            dirt_core::models::PileSummary::of(notes, offset),
        )
    };
    let (small_heap, small_summary) = show(&small.read());
    let (year_heap, year_summary) = show(&year);
    rsx! {
        h2 { "The pile" }
        div { class: "gal-card gal-line",
            div { "data-dui-list": "",
                Pile { heap: small_heap, summary: small_summary, on_archived: move |()| {} }
            }
            div { "data-dui-list": "",
                Pile { heap: year_heap, summary: year_summary, on_archived: move |()| {} }
            }
            Button {
                onclick: move |_| {
                    let more = made_up_pile(1, 0, 0);
                    small.write().extend(more);
                },
                "Add a thought"
            }
        }
    }
}

#[component]
fn SwipeCard() -> Element {
    let toasts = use_toasts();
    let bus = use_context::<Bus>();
    let mut cards = use_signal(|| vec![1u32, 2, 3]);
    let mut next = use_signal(|| 4u32);
    rsx! {
        h2 { "A card that follows a finger" }
        p { class: "gal-frames",
            "Pointer movements the script handled: "
            b { id: "gal-moves", "data-dui-stat": "moves", "0" }
            " · Messages the script sent: "
            b { id: "gal-told", "data-dui-stat": "told", "0" }
            " · Messages Rust received: "
            b { id: "gal-received", "{bus.received()}" }
        }
        div { class: "gal-cards", "data-dui-list": "", "data-dui-exit": "none",
            for card in cards() {
                div { key: "{card}", class: "gal-card-slot", "data-key": "{card}",
                    Swipeable {
                        on_swipe: move |()| {
                            cards.write().retain(|other| *other != card);
                            let back = undo_for(move || {
                                let mut all = cards.write();
                                if !all.contains(&card) {
                                    all.push(card);
                                    all.sort_unstable();
                                }
                            });
                            toasts.show(Toast::new("Archived").icon(Icon::Archive).undo(back));
                        },
                        div { class: "gal-swipe-card",
                            div { class: "gal-item-t", "Card {card}: swipe me to the left" }
                            div { class: "gal-s",
                                "It follows the finger. Past 96 px it flies off; before, it springs back."
                            }
                        }
                    }
                }
            }
        }
        div { class: "gal-line gal-gap",
            Button {
                onclick: move |_| {
                    let card = next();
                    next.set(card + 1);
                    cards.write().push(card);
                },
                "Another card"
            }
        }
    }
}

/// What listening looks like (6.18). Nothing here listens: the level is
/// made up, rising and falling as a voice would.
#[component]
fn VoiceShown() -> Element {
    let mut view = use_signal(|| None::<(u64, f32, Option<Ended>)>);
    let mut serial = use_signal(|| 0_u64);
    let mut start = move || {
        if view.peek().is_some() {
            return;
        }
        view.set(Some((0, 0.0, None)));
        let mine = {
            let mut serial = serial.write();
            *serial += 1;
            *serial
        };
        spawn(async move {
            let mut ms = 0_u64;
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                ms += 50;
                if *serial.peek() != mine || !matches!(*view.peek(), Some((_, _, None))) {
                    break;
                }
                #[allow(clippy::cast_precision_loss)]
                let at = ms as f32 / 1000.0;
                let level = ((at * 5.1).sin() * (at * 1.3).sin()).abs() * 0.9;
                view.set(Some((ms, level, None)));
            }
        });
    };
    let mut end = move |how: Ended| {
        let now = *view.peek();
        if let Some((ms, level, None)) = now {
            view.set(Some((ms, level, Some(how))));
        }
    };
    rsx! {
        h2 { "Voice" }
        p { class: "gal-s",
            "Hold the mic and let go to keep; tap it for hands-free; held, slide up to lock or left to throw it away."
        }
        div { class: "gal-line",
            MicButton {
                label: "Hold to talk, or tap",
                on_talk: move |talk| match talk {
                    Talk::Listen | Talk::Lock => start(),
                    Talk::Keep => end(Ended::Kept),
                    Talk::Throw => end(Ended::Thrown),
                },
            }
            MicButton { label: "The microphone may not be used", refused: true, on_talk: |_| {} }
            Button { onclick: move |_| end(Ended::NothingHeard), "Nothing heard" }
        }
        if let Some((ms, level, ended)) = view() {
            Listening {
                ms,
                level,
                ended,
                on_keep: move |()| end(Ended::Kept),
                on_throw: move |()| end(Ended::Thrown),
                on_gone: move |()| view.set(None),
            }
        }
    }
}

/// A file attached to no note, for the tiles below.
fn a_file(
    id: u128,
    name: &str,
    size: u64,
    on_device: bool,
    standing: FileStanding,
) -> AttachedFile {
    AttachedFile {
        attachment: dirt_core::files::Attachment {
            file_id: uuid::Uuid::from_u128(id),
            name: name.to_owned(),
            media_type: dirt_core::files::media_type_for(name).to_owned(),
            size,
            key: dirt_core::files::FileKey::generate(),
            added_at_ms: 0,
            thumbnail: None,
        },
        on_device,
        thumbnail_on_device: false,
        standing,
    }
}

/// The file tile in each of its states. The preview needs a note's files
/// in a store, and is tried in the apps.
#[component]
fn FileTilesShown() -> Element {
    let toasts = use_toasts();
    let mut folded = use_signal(|| true);
    let said = move |words: &'static str| {
        move |file: AttachedFile| {
            toasts.show(Toast::new(format!("{words} {}", file.attachment.name)));
        }
    };
    let tiles = [
        (
            a_file(1, "receipt-oct.pdf", 219_000, true, FileStanding::Taken),
            false,
            true,
        ),
        (
            a_file(
                2,
                "kitchen-sketch.png",
                940_000,
                true,
                FileStanding::NotTaken,
            ),
            false,
            true,
        ),
        (
            a_file(3, "standup.m4a", 3_200_000, true, FileStanding::NotTaken),
            false,
            false,
        ),
        (
            a_file(4, "tile-sample.jpg", 2_100_000, false, FileStanding::Taken),
            true,
            true,
        ),
        (
            a_file(
                5,
                "install.sh",
                1_000,
                true,
                FileStanding::Refused("the account's storage for files is full".to_owned()),
            ),
            false,
            true,
        ),
    ];
    rsx! {
        h2 { "File tiles" }
        p { class: "gal-s",
            "Sent; being sent; on this device only; being fetched; a script, which is never opened and could not be sent."
        }
        div { class: "dui-files",
            for (file , fetching , can_send) in tiles.clone() {
                FileTile {
                    key: "{file.attachment.file_id}",
                    whereabouts: file.whereabouts(can_send),
                    file,
                    thumbnail: None,
                    fetching,
                    on_preview: said("Preview"),
                    on_open: said("Open"),
                    on_save: said("Save"),
                    on_remove: said("Remove"),
                }
            }
        }
        h2 { "The shelf" }
        p { class: "gal-s",
            "A note's files at the foot of its page: its line folds and unfolds them. Folded, a square for each previews it."
        }
        ShelfFrame {
            note: "gallery",
            count: tiles.len(),
            folded: folded(),
            on_fold: move |fold: bool| folded.set(fold),
            minis: rsx! {
                for (file , fetching , _) in tiles.clone() {
                    FileMini {
                        key: "{file.attachment.file_id}",
                        file,
                        thumbnail: None,
                        fetching,
                        on_preview: said("Preview"),
                        on_show: move |_| folded.set(false),
                    }
                }
            },
            div { class: "dui-files",
                for (file , fetching , can_send) in tiles {
                    FileTile {
                        key: "{file.attachment.file_id}",
                        whereabouts: file.whereabouts(can_send),
                        file,
                        thumbnail: None,
                        fetching,
                        on_preview: said("Preview"),
                        on_open: said("Open"),
                        on_save: said("Save"),
                        on_remove: said("Remove"),
                    }
                }
            }
        }
    }
}

/// Android's list: cards in a list that is pulled down to sync, and the
/// strip of tags.
#[component]
fn AndroidList() -> Element {
    let mut picked = use_signal(|| vec!["travel".to_owned()]);
    let mut busy = use_signal(|| false);
    let clock = crate::time::Clock::now();
    let notes = [
        "Plan the trip\nflights first, then the hotel #travel #work",
        "Pack the bags #travel",
        "Quarterly report, due Friday #work",
    ];
    let options: Vec<TagOption> = ["travel", "work", "home"]
        .into_iter()
        .map(|tag| TagOption {
            tag: tag.to_owned(),
            notes: None,
            picked: picked.read().iter().any(|chosen| chosen == tag),
        })
        .collect();
    rsx! {
        h2 { "Android: cards, pull to sync, the strip of tags" }
        TagStrip {
            open: true,
            options,
            on_pick: move |tag: String| {
                let mut all = picked.write();
                if all.contains(&tag) {
                    all.retain(|chosen| *chosen != tag);
                } else {
                    all.push(tag);
                }
            },
            on_rule: move |_| {},
        }
        ScrollArea { class: "gal-pull",
            PullToSync {
                busy: busy(),
                on_pull: move |()| {
                    busy.set(true);
                    spawn(async move {
                        // The script holds the list while this is set.
                        let _ = document::eval("await new Promise(done => setTimeout(done, 1500));").await;
                        busy.set(false);
                    });
                },
                div { class: "gal-cards", "data-dui-list": "", "data-dui-exit": "fold",
                    for text in notes {
                        NoteCard {
                            key: "{text}",
                            summary: NoteSummary::of(&dirt_core::models::Note::new(text), &[], clock),
                            on_open: move |()| {},
                            on_tag: move |_| {},
                            on_swipe: move |()| {},
                        }
                    }
                }
            }
        }
    }
}

/// What one toast's **Undo** does, made where the toast is shown.
fn undo_for(undo: impl FnMut() + 'static) -> Callback<()> {
    let mut undo = undo;
    Callback::new(move |()| undo())
}

#[component]
fn Icons() -> Element {
    rsx! {
        h2 { "Icons" }
        div { class: "gal-card gal-icons",
            for icon in Icon::ALL {
                span { key: "{icon:?}", class: "gal-icon", "data-tip": "{icon:?}",
                    IconSvg { icon }
                }
            }
            span { class: "gal-icon", "data-tip": "Small, 15 px", IconSvg { icon: Icon::Feather, size: IconSize::Small } }
            span { class: "gal-icon", "data-tip": "Large, 22 px", IconSvg { icon: Icon::Feather, size: IconSize::Large } }
            span { class: "gal-icon", "data-tip": dirt_core::sync::overview::NOT_ON_SERVER_YET, span { class: "gal-wait" } }
        }
    }
}

#[component]
fn Type() -> Element {
    rsx! {
        h2 { "Type" }
        div { class: "gal-card gal-type",
            div { class: "gal-title", "Notes" }
            p { class: "gal-note",
                "A note's text, at the size chosen above. Sourdough: 78% hydration, fold every 30 minutes, "
                mark { "bake" }
                " at 240°. "
                span { class: "gal-tag", "#cooking" }
            }
            div { class: "gal-row-t", "Note title in the list, weight 560" }
            div { class: "gal-row-s", "The preview under it, in the second text colour, two lines at most." }
            div { class: "gal-ui", "Interface text and buttons, 13.5 px at weight 550" }
            div { class: "gal-s", "Hints, times and captions, 12.5 px" }
            div { class: "gal-label", "Section label" }
            div { class: "gal-mono",
                kbd { "Ctrl" }
                " "
                kbd { "Alt" }
                " "
                kbd { "N" }
                span { " 01 abandon  02 ability  03 able" }
            }
        }
    }
}

#[component]
fn Colours() -> Element {
    let accents = [
        "accent",
        "accent-hover",
        "accent-soft",
        "on-accent",
        "accent-inverse",
    ];
    rsx! {
        h2 { "Colours" }
        div { class: "gal-card gal-swatches",
            for name in NEUTRALS.iter().map(|neutral| neutral.name).chain(accents) {
                div { key: "{name}", class: "gal-swatch",
                    span { style: "background:var(--{name})" }
                    "--{name}"
                }
            }
        }
    }
}
