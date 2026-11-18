//! The sync status (interface design, section 6.6): an icon that says how
//! sync stands, and for the three states that need the person a few words
//! beside it.
//!
//! Which state holds, its words and its tooltip all come from
//! `dirt_core::sync::overview::SyncIndicator`, so the three clients say
//! the same thing. This component chooses the icon by the state and
//! writes no words of its own.

use dioxus::prelude::*;
use dirt_core::sync::overview::SyncIndicator;

use crate::appearance::Platform;
use crate::icons::{Icon, IconSvg};

/// The icon of a state, and the name the style sheet colours it by.
const fn looks(indicator: SyncIndicator) -> (Icon, &'static str) {
    match indicator {
        SyncIndicator::Synced { .. } => (Icon::CloudCheck, "synced"),
        SyncIndicator::Syncing => (Icon::Refresh, "syncing"),
        SyncIndicator::JustSynced => (Icon::Check, "done"),
        SyncIndicator::Offline { .. } => (Icon::CloudOff, "offline"),
        SyncIndicator::Problem { .. } => (Icon::Alert, "problem"),
        SyncIndicator::SignedOut => (Icon::Laptop, "out"),
    }
}

/// The sync status, as a button: pressing it syncs now, or goes to where
/// a problem is explained. What a press does is the app's to say; the
/// shortcut is shown in the tooltip only for the states where a press
/// syncs.
///
/// A screen reader hears the state's words when the state changes
/// (section 12). Synced, syncing and just synced pass through each other
/// with every edit, so the three are announced as one: "Synced" is said
/// when sync comes back from being offline or from a problem, not after
/// each note.
#[component]
pub fn SyncStatus(
    indicator: SyncIndicator,
    /// Now, in Unix milliseconds, for "Last synced 2 minutes ago".
    now_ms: i64,
    /// The keys that sync now, as key chips: `"Ctrl R"`.
    #[props(default)]
    shortcut: Option<String>,
    onclick: EventHandler<MouseEvent>,
    /// On Android it is an icon in the top bar, with no words: a dot on it
    /// says that notes wait (grey) or that something needs the person
    /// (amber). Its words are its name for a screen reader, and what a
    /// long press shows.
    #[props(default = Platform::current())]
    platform: Platform,
) -> Element {
    let (icon, state) = looks(indicator);
    let words = indicator.short();
    let compact = platform == Platform::Android;
    // On Android the words are shown nowhere else, so a long press gives
    // everything the status says.
    let tooltip = if compact {
        Some(indicator.in_full(now_ms))
    } else {
        indicator.tooltip(now_ms)
    };
    let quiet = indicator.is_quiet();
    let dot = compact
        && matches!(
            indicator,
            SyncIndicator::Offline { waiting: 1.. } | SyncIndicator::Problem { .. }
        );
    let syncs = !matches!(
        indicator,
        SyncIndicator::Problem { .. } | SyncIndicator::SignedOut
    );
    let announced = if quiet {
        SyncIndicator::JustSynced.short()
    } else {
        words.clone()
    };
    rsx! {
        button {
            r#type: "button",
            class: if compact { "dui-sync compact" } else { "dui-sync" },
            "data-s": state,
            "aria-label": "{words}",
            "aria-description": tooltip.clone(),
            "data-tip": tooltip,
            "data-kbd": if syncs { shortcut } else { None },
            onclick: move |event| onclick.call(event),
            // One element for each state, so that a new state's icon
            // arrives as itself and is not the old one redrawn.
            for state in [state] {
                span { key: "{state}", class: "dui-sync-icon",
                    IconSvg { icon }
                }
            }
            if !quiet && !compact {
                span { class: "dui-sync-words", "{words}" }
            }
            if dot {
                span { class: "dui-sync-dot", "aria-hidden": "true" }
            }
        }
        span { class: "dui-sr", role: "status", "aria-live": "polite", "{announced}" }
    }
}
