//! The window without a title bar (interface design, section 7.6).
//!
//! On Windows the system's title bar goes: the app reaches the top edge,
//! and draws its own minimise, maximise and close at the top right. Any
//! empty part of the top 52 px moves the window, and a double-click there
//! maximises or restores it. Windows keeps its own resize border and its
//! shadow for a window without one, and snaps a window dragged to an edge.
//!
//! Elsewhere the window keeps the system's title bar, which is the plan's
//! fallback for a platform where the window has not been seen to move and
//! resize without it: Linux and macOS are still to be tried
//! (`docs/MANUAL_CHECKLIST.md`).
//!
//! What follows the pointer is the system's: the script says only that a
//! press began on the empty top row (`dirt_ui::events::Outcome::Drag`), or
//! that it was a double-click there, and the system moves the window from
//! that press until the button is let go.

use dioxus::desktop::tao::event::{Event, WindowEvent};
use dioxus::desktop::{use_wry_event_handler, window, WindowBuilder};
use dioxus::prelude::*;
use dirt_ui::events::{use_outcome, Outcome};
use dirt_ui::{Icon, IconSize, IconSvg};

/// How much of the window's top moves it, in CSS pixels.
pub const DRAG_ROW: &str = "52";

/// Whether this build draws the window without the system's title bar.
#[must_use]
pub const fn frameless() -> bool {
    cfg!(windows)
}

/// The window, as the app opens it: titled "Dirt" for the taskbar and the
/// window switcher, and without the system's title bar where the app draws
/// its own.
#[must_use]
pub fn builder() -> WindowBuilder {
    let builder = WindowBuilder::new()
        .with_title("Dirt")
        .with_decorations(!frameless());
    #[cfg(windows)]
    let builder = {
        use dioxus::desktop::tao::platform::windows::WindowBuilderExtWindows;
        // Windows draws the shadow, and on Windows 11 rounds the corners.
        builder.with_undecorated_shadow(true)
    };
    builder
}

/// Listens for presses on the window's empty top row. Returns the ID to
/// put on the element that holds that row, with [`DRAG_ROW`] as its
/// `data-dui-drag`; empty where the system's title bar is kept.
pub fn use_window_drag() -> Option<String> {
    let id = use_outcome(|outcome| match outcome {
        Outcome::Drag => window().drag(),
        Outcome::Maximize => window().toggle_maximized(),
        _ => {}
    });
    frameless().then_some(id)
}

/// Dirt's own minimise, maximise and close (Windows and Linux, 7.6). At
/// rest at 55%, full under the pointer; while a note is written they go,
/// and come back when the pointer comes within 64 px of them.
#[component]
pub fn WindowControls() -> Element {
    let mut maximized = use_signal(|| window().is_maximized());
    use_wry_event_handler(move |event, _| {
        if let Event::WindowEvent {
            event: WindowEvent::Resized(_),
            ..
        } = event
        {
            let now = window().is_maximized();
            if now != *maximized.peek() {
                maximized.set(now);
            }
        }
    });
    if !frameless() {
        return rsx! {};
    }
    let (size_icon, size_label) = if maximized() {
        (Icon::Restore, "Restore")
    } else {
        (Icon::Square, "Maximise")
    };
    rsx! {
        div {
            class: "dk-wctl",
            role: "group",
            "aria-label": "Window",
            "data-dui-near": "64",
            "data-max": maximized().then_some(""),
            button {
                r#type: "button",
                class: "dk-wctl-b",
                "aria-label": "Minimise",
                "data-tip": "Minimise",
                onclick: move |_| window().set_minimized(true),
                IconSvg { icon: Icon::Minus, size: IconSize::Small }
            }
            button {
                r#type: "button",
                class: "dk-wctl-b",
                "aria-label": size_label,
                "data-tip": size_label,
                onclick: move |_| window().toggle_maximized(),
                IconSvg { icon: size_icon, size: IconSize::Small }
            }
            button {
                r#type: "button",
                class: "dk-wctl-b x",
                "aria-label": "Close to tray",
                "data-tip": "Close to tray",
                "data-kbd": "Alt F4",
                // Closing hides the window, as the system's close did: Dirt
                // stays in the tray so that the hotkey still works.
                onclick: move |_| window().close(),
                IconSvg { icon: Icon::X, size: IconSize::Small }
            }
        }
    }
}
