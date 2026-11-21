//! A page that grows out of what opened it (interface design, sections 3,
//! 5.5 and 10).
//!
//! Android's note grows out of its card and shrinks back into it, and the
//! new page grows out of the bar at the bottom.
//!
//! The page is always in the screen, so that it can move in and out;
//! `open` says which way. The script sees the class change and plays the
//! movement from the element named by `from`, as it is on screen then. It
//! tells Rust Android's Back, which the page takes while it is open, and
//! when a page that closed has gone.
//!
//! The keyboard (#322): going or rising, it resizes the web view, which
//! held the movement's frames back and moved the place a page shrank to.
//! So a closing page stays live, not `inert`, and its field keeps the
//! keyboard, until the script has shrunk it away, blurred the field and
//! said so ([`Outcome::Gone`]); the script owns that end, since only it
//! knows when the movement's first frame was drawn. And a page given
//! `focus` hands its field the keyboard once it has grown, from the script,
//! unless the person has put it somewhere meanwhile.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use dioxus::prelude::*;

use crate::events::{use_outcome, Outcome};

/// How long a closing page stays live at most if the script never says it
/// has gone (no script, or a web view that has stopped drawing). The script
/// says so at the end of its movement (340 ms into a card), or 400 ms past
/// it by a timer of its own; this is well beyond both, and beyond a first
/// frame held back by a third of a second, as the emulator's was.
const LIVE_AT_MOST_WHILE_CLOSING: Duration = Duration::from_millis(2000);

/// What a page grows out of and shrinks back into: the ID of an element in
/// the page, and what kind of element it is.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum GrowFrom {
    /// A note's card ([`crate::components::card_element_id`]): 440 ms out
    /// of it, 340 ms back.
    Card(String),
    /// The bar a new page is asked for from: the glide curve over 560 ms
    /// out of it, 320 ms back.
    Bar(String),
    /// A new page let go with words on it (section 5.5): it closes to a
    /// 7 px circle where its words began, and the grain of the card named
    /// here (marked `data-dui-settle`) is tossed from there onto it.
    Grain(String),
    /// Nothing on screen: the page fades.
    #[default]
    Nowhere,
}

impl GrowFrom {
    fn parts(&self) -> (Option<&str>, Option<&'static str>) {
        match self {
            Self::Card(id) => (Some(id), Some("card")),
            Self::Bar(id) => (Some(id), Some("bar")),
            Self::Grain(id) => (Some(id), Some("grain")),
            Self::Nowhere => (None, None),
        }
    }
}

/// Where a page is in its closing: whether it was open when last drawn,
/// which closing this is, and whether it is still live after it.
#[derive(Debug, Clone, Copy)]
struct Closing {
    open: bool,
    number: u64,
    live: bool,
}

/// A page over the whole screen. Inside it, the element with
/// `data-dui-grow-bar` (its top bar) fades in after it, and the element
/// with `data-dui-grow-body` (its text) rises in behind it.
#[component]
pub fn GrowPage(
    open: bool,
    /// What it grows out of as it opens, and goes back into as it closes.
    from: GrowFrom,
    /// Its name for a screen reader.
    label: String,
    /// Android's Back, while it is open.
    on_back: EventHandler<()>,
    /// A selector for its field, to be given the keyboard once it has
    /// grown (a note opened from its card). `None`: the app gives it
    /// itself, or not at all.
    #[props(default)]
    focus: Option<String>,
    #[props(default)] class: String,
    children: Element,
) -> Element {
    // Live while it closes, until the script says it has gone or the time
    // runs out. Told apart in the render itself, as `open` changes, not in
    // an effect: the render that closes it must not draw it inert even once
    // (the field would lose the focus), and `gone` must find the closing
    // counted however late an effect would run. Each closing has its
    // number, so that what ends an earlier one does not end a later one.
    let state = use_hook(|| {
        Rc::new(Cell::new(Closing {
            open,
            number: 0,
            live: false,
        }))
    });
    let mut drawn = use_signal(|| 0_u64);
    let id = use_outcome({
        let state = Rc::clone(&state);
        move |outcome| match outcome {
            Outcome::Back => on_back.call(()),
            Outcome::Gone => {
                let now = state.get();
                if !now.open && now.live {
                    state.set(Closing { live: false, ..now });
                    drawn += 1;
                }
            }
            _ => {}
        }
    });
    let (from_id, kind) = from.parts();
    let was = state.get();
    if was.open && !open {
        let number = was.number + 1;
        state.set(Closing {
            open,
            number,
            live: true,
        });
        let state = Rc::clone(&state);
        spawn(async move {
            tokio::time::sleep(LIVE_AT_MOST_WHILE_CLOSING).await;
            let now = state.get();
            if now.number == number && now.live {
                state.set(Closing { live: false, ..now });
                drawn += 1;
            }
        });
    } else if was.open != open {
        state.set(Closing {
            open,
            live: false,
            ..was
        });
    }
    // Read so that `gone`, or the time running out, draws it again.
    let _ = drawn.read();
    // Hidden from a screen reader when it is inert, once its field has let
    // the focus go, not while the focus is still inside it.
    let inert = !open && !state.get().live;
    rsx! {
        section {
            class: if open { "dui-grow on {class}" } else { "dui-grow {class}" },
            role: "dialog",
            "aria-label": "{label}",
            "aria-hidden": inert.then_some("true"),
            "inert": inert.then_some(""),
            "data-dui-grow": "",
            "data-dui-from": from_id,
            "data-dui-from-kind": kind,
            "data-dui-grow-focus": focus,
            "data-dui-id": "{id}",
            "data-dui-back": open.then_some(""),
            {children}
        }
    }
}
