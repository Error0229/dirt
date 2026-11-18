//! Focus while writing (interface design, section 7.7): the way back to
//! the list in each style.
//!
//! The screen's root says whether a note is being written and in which
//! style (`data-dui-writing`, `data-focus`); the style sheet lays that out
//! and the script plays what depends on where the rows are. What is here
//! is what a person uses to come back: the handle at the left edge (Slide
//! and Zen) and the rail of grains (Rail).

use chrono::{Datelike, Local, TimeZone};
use dioxus::prelude::*;
use dirt_core::models::{FocusStyle, Grain, NoteId};
use dirt_ui::components::{IconButton, IconButtonSize};
use dirt_ui::events::{use_outcome, Outcome};
use dirt_ui::Icon;

use super::Screen;
use crate::state::AppState;

/// The handle at the window's left edge, in Slide and Zen. It grows as the
/// pointer nears it, and the list comes back when the pointer reaches it.
/// The script watches the pointer and says only that it arrived.
#[component]
pub fn Edge() -> Element {
    let screen = use_context::<Screen>();
    let id = use_outcome(move |outcome| {
        if outcome == Outcome::Reach {
            screen.calm();
        }
    });
    rsx! {
        div {
            class: "dk-edge",
            "aria-hidden": "true",
            "data-dui-id": id,
            "data-dui-near": "60",
            "data-dui-reach": "",
        }
    }
}

/// One thing on the rail: a note's grain, or the start of a month.
#[derive(Debug, Clone, PartialEq)]
enum Stop {
    Month {
        initial: String,
        key: String,
    },
    Note {
        id: NoteId,
        title: String,
        shade: String,
        shape: String,
    },
}

/// The month an edit time falls in, here.
fn month_of(at_ms: i64) -> Option<(i32, u32)> {
    let at = Local.timestamp_millis_opt(at_ms).single()?;
    Some((at.year(), at.month()))
}

/// The first letter of a month's name.
fn initial(month: u32) -> &'static str {
    ["J", "F", "M", "A", "M", "J", "J", "A", "S", "O", "N", "D"]
        .get(month.saturating_sub(1) as usize)
        .copied()
        .unwrap_or("")
}

/// The rail's stops, newest at the top: a grain for each note the list
/// holds, and a short line with the month's initial where a month starts.
fn stops(notes: &[(NoteId, String, i64)]) -> Vec<Stop> {
    let mut stops = Vec::with_capacity(notes.len() + 4);
    let mut month = None;
    for (id, title, edited) in notes {
        let here = month_of(*edited);
        if month.is_some() && here != month {
            if let Some((year, number)) = here {
                stops.push(Stop::Month {
                    initial: initial(number).to_owned(),
                    key: format!("month {year} {number}"),
                });
            }
        }
        month = here;
        let grain = Grain::of(id);
        // A slightly irregular round, one of a few, the same everywhere
        // for the same note.
        let shape = [
            "50%",
            "46% 54% 52% 48%",
            "54% 46% 48% 52%",
            "48% 52% 55% 45%",
        ][(grain.seed >> 16) as usize % 4];
        stops.push(Stop::Note {
            id: *id,
            title: if title.is_empty() {
                "Untitled".to_owned()
            } else {
                title.clone()
            },
            shade: format!(
                "hsl({} {}% {}%)",
                grain.hue, grain.saturation, grain.lightness
            ),
            shape: shape.to_owned(),
        });
    }
    stops
}

/// The rail (Rail style): the list folded to a column of grains, one for
/// each note it holds. A grain's tooltip is its note's title, and a click
/// opens that note without leaving focus. The chevron at the top brings
/// the list back.
#[component]
pub fn Rail() -> Element {
    let screen = use_context::<Screen>();
    let app = use_context::<AppState>();
    if screen.focus_style() != FocusStyle::Rail {
        return rsx! {};
    }
    let notes: Vec<(NoteId, String, i64)> = screen
        .listing
        .read()
        .as_ref()
        .map(|listing| {
            listing
                .notes
                .iter()
                .map(|note| (note.id, note.outline().title, note.updated_at))
                .collect()
        })
        .unwrap_or_default();
    let open = *app.current_note_id.read();
    let stops = stops(&notes);
    rsx! {
        nav {
            class: "dk-rail",
            "aria-label": "Notes",
            // A press here leaves the cursor in the note.
            "data-dui-keep-focus": "",
            "data-dui-rail": "",
            IconButton {
                icon: Icon::ChevronRight,
                label: "Show the list",
                shortcut: "Esc",
                size: IconButtonSize::Small,
                onclick: move |_| screen.calm(),
            }
            div { class: "dk-rail-dots",
                for stop in stops {
                    match stop {
                        Stop::Month { initial, key } => rsx! {
                            span {
                                key: "{key}",
                                class: "dk-rail-month",
                                "aria-hidden": "true",
                                "data-dui-stop": "",
                                "{initial}"
                            }
                        },
                        Stop::Note { id, title, shade, shape } => rsx! {
                            button {
                                key: "{id}",
                                r#type: "button",
                                class: if open == Some(id) { "dk-rail-dot on" } else { "dk-rail-dot" },
                                style: "--g:{shade};--gr:{shape}",
                                "aria-label": "{title}",
                                "aria-current": (open == Some(id)).then_some("true"),
                                "data-tip": "{title}",
                                onclick: move |_| screen.open_from_rail(id),
                            }
                        },
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_month_starts_with_its_initial_and_the_first_month_has_none() {
        let day = 24 * 60 * 60 * 1000;
        let november = Local
            .with_ymd_and_hms(2026, 11, 10, 12, 0, 0)
            .unwrap()
            .timestamp_millis();
        let october = Local
            .with_ymd_and_hms(2026, 10, 10, 12, 0, 0)
            .unwrap()
            .timestamp_millis();
        let notes = vec![
            (NoteId::new(), "today".to_owned(), november),
            (NoteId::new(), String::new(), november - day),
            (NoteId::new(), "older".to_owned(), october),
        ];
        let stops = stops(&notes);
        assert_eq!(stops.len(), 4);
        assert!(matches!(&stops[2], Stop::Month { initial, .. } if initial == "O"));
        assert!(matches!(&stops[1], Stop::Note { title, .. } if title == "Untitled"));
    }
}
