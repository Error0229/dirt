//! Voice (interface design, sections 5.5 and 6.18): the mic, and what
//! shows while it listens.
//!
//! **The mic** ([`MicButton`]) is one gesture everywhere, and the script
//! follows it from the press to the release (`dirt-ui.js`, "Voice"). It
//! tells Rust four things: a press began, so start listening at once
//! ([`Talk::Listen`]); it was a tap, or the press slid up 60 px, so go on
//! listening with no hand on it ([`Talk::Lock`]); it was let go after a
//! hold, so keep it ([`Talk::Keep`]); it slid 90 px to the left, or the
//! press was taken away, so throw it away ([`Talk::Throw`]).
//!
//! **While listening** ([`Listening`]) nothing is written: a row of grains
//! moves with the voice, and a timer shows from the first second, with ✕
//! and ✓. The grains are drawn by the script from the level Rust gives
//! (`data-dui-level`), like sand on a drum: still in silence, most in the
//! middle, higher as it gets louder. When it ends, Rust says how
//! (`data-dui-voice`), and the script plays it and says `gone`: the grains
//! gather into one, which the new note's row takes to settle onto the pile;
//! or they blow off to the left; or, when nothing was heard, the row shakes
//! once.

use dioxus::prelude::*;
use dirt_core::models::Grain;

use crate::components::icon_button::IconButton;
use crate::events::{use_outcome, Outcome};
use crate::icons::{Icon, IconSvg};
use crate::pile::shade;

/// What the mic's gesture asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Talk {
    /// A press began: listen now.
    Listen,
    /// A tap, or a slide up while held: go on listening hands-free, until
    /// ✓ or ✕.
    Lock,
    /// Let go after holding: keep what was said.
    Keep,
    /// Slid to the left, or the press was taken away: throw it away.
    Throw,
}

/// How listening ended, as it is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    /// Kept: the grains gather into one.
    Kept,
    /// Thrown away: they blow off to the left.
    Thrown,
    /// Nothing was heard: it shakes once, and nothing is made.
    NothingHeard,
}

impl Ended {
    /// The name the script knows it by.
    const fn name(self) -> &'static str {
        match self {
            Self::Kept => "kept",
            Self::Thrown => "thrown",
            Self::NothingHeard => "nothing",
        }
    }

    /// What a screen reader is told (section 12).
    const fn said(self) -> &'static str {
        match self {
            Self::Kept => "Kept",
            Self::Thrown => "Thrown away",
            Self::NothingHeard => "Nothing heard",
        }
    }
}

/// The time listened, as it shows: nothing in the first second, then
/// `0:01`, `0:02`, … `10:00`.
#[must_use]
pub fn timer(ms: u64) -> String {
    let seconds = ms / 1000;
    if seconds == 0 {
        return String::new();
    }
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// The shades the listening grains are drawn in: nine of the pile's.
fn sand() -> String {
    (0..9_u32)
        .map(|index| {
            let seed = index.wrapping_mul(0x9e37_79b9) ^ 0x5bd1_e995;
            let within = |value: u32, span: u32| u8::try_from(value % span).unwrap_or_default();
            shade(&Grain {
                hue: 20 + u16::from(within(seed, 24)),
                saturation: 30 + within(seed >> 5, 32),
                lightness: 48 + within(seed >> 10, 24),
                seed,
            })
        })
        .collect::<Vec<_>>()
        .join(";")
}

/// The mic: hold to talk and let go to keep, or tap for hands-free.
///
/// `refused` shows it with a slash: the system did not let the app use
/// the microphone, and its tooltip says where to allow it (`label`). It
/// still takes a press, which asks again.
#[component]
pub fn MicButton(
    label: String,
    #[props(default)] shortcut: Option<String>,
    #[props(default)] refused: bool,
    /// More classes, for where it sits.
    #[props(default)]
    class: String,
    on_talk: EventHandler<Talk>,
) -> Element {
    let outcome = use_outcome(move |outcome| {
        let talk = match outcome {
            Outcome::Listen => Talk::Listen,
            Outcome::Lock => Talk::Lock,
            Outcome::Keep => Talk::Keep,
            Outcome::Throw => Talk::Throw,
            _ => return,
        };
        on_talk.call(talk);
    });
    let classes = format!(
        "dui-icon-btn dui-mic{}{}{}",
        if refused { " refused" } else { "" },
        if class.is_empty() { "" } else { " " },
        class
    );
    rsx! {
        button {
            r#type: "button",
            class: "{classes}",
            "aria-label": "{label}",
            "aria-keyshortcuts": shortcut.as_deref().map(|keys| keys.replace(' ', "+")),
            "data-tip": "{label}",
            "data-kbd": shortcut,
            // The press is the script's, and so is a click from the
            // keyboard, which it takes for a tap.
            "data-dui-mic": "",
            "data-dui-id": outcome,
            IconSvg { icon: if refused { Icon::MicOff } else { Icon::Mic } }
        }
    }
}

/// What shows while listening: the grains and the timer, ✕ and ✓.
#[component]
pub fn Listening(
    /// How long it has listened, in milliseconds.
    ms: u64,
    /// How loud it is now, from 0 to 1.
    level: f32,
    /// How it ended, once it has: drawn, then `on_gone`.
    #[props(default)]
    ended: Option<Ended>,
    on_keep: EventHandler<()>,
    on_throw: EventHandler<()>,
    /// The ending has been drawn: it can go.
    on_gone: EventHandler<()>,
    #[props(default)] class: String,
) -> Element {
    let outcome = use_outcome(move |outcome| {
        if outcome == Outcome::Gone {
            on_gone.call(());
        }
    });
    let sand = use_hook(sand);
    let state = ended.map_or("listening", Ended::name);
    let said = ended.map_or("Listening", Ended::said);
    let time = timer(ms);
    rsx! {
        div {
            class: "dui-voice {class}",
            role: "group",
            "aria-label": "Listening",
            "data-dui-voice": state,
            "data-dui-level": format!("{:.3}", level.clamp(0.0, 1.0)),
            "data-dui-sand": "{sand}",
            "data-dui-id": outcome,
            span { class: "dui-voice-said", role: "status", "aria-live": "polite", "{said}" }
            canvas { class: "dui-voice-grains", "aria-hidden": "true" }
            span { class: "dui-voice-time", "aria-hidden": "true", "{time}" }
            IconButton {
                icon: Icon::X,
                label: "Throw away",
                shortcut: "Esc",
                disabled: ended.is_some(),
                onclick: move |_| on_throw.call(()),
            }
            button {
                r#type: "button",
                class: "dui-voice-keep",
                "aria-label": "Keep it",
                "aria-keyshortcuts": "Enter",
                "data-tip": "Keep it",
                "data-kbd": "Enter",
                disabled: ended.is_some(),
                onclick: move |_| on_keep.call(()),
                IconSvg { icon: Icon::Check }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_timer_shows_from_the_first_second() {
        assert_eq!(timer(0), "");
        assert_eq!(timer(999), "");
        assert_eq!(timer(1_000), "0:01");
        assert_eq!(timer(65_400), "1:05");
        assert_eq!(timer(600_000), "10:00");
    }

    #[test]
    fn the_grains_are_nine_shades_of_the_piles_sand() {
        let sand = sand();
        let shades: Vec<&str> = sand.split(';').collect();
        assert_eq!(shades.len(), 9);
        assert!(shades.iter().all(|shade| shade.starts_with("hsl(")));
    }
}
