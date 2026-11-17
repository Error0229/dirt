//! The toast (interface design, sections 5.5, 6.7 and 12): dark on light
//! and light on dark, at the bottom, one at a time. It carries at most one
//! action, **Undo**, and a draining bar that shows the time left.
//!
//! A component shows one with [`use_toasts`]. The script opens it, drains
//! its bar, waits while the pointer or the keyboard is on it, and tells
//! Rust when its time has run out. It is announced to a screen reader: the
//! place it appears in is a polite live region.

use dioxus::prelude::*;

use crate::appearance::Platform;
use crate::components::icon_button::{IconButton, IconButtonSize};
use crate::events::{use_outcome, Outcome};
use crate::icons::{Icon, IconSize, IconSvg};

/// How long a toast stays, in milliseconds.
const STAYS_MS: u32 = 2600;

/// How long a toast with **Undo** stays.
const STAYS_WITH_UNDO_MS: u32 = 6000;

/// A toast to show.
#[derive(Clone, PartialEq)]
pub struct Toast {
    /// What it says, in a few words: "Archived".
    pub text: String,
    /// An icon before the words.
    pub icon: Option<Icon>,
    /// What **Undo** does. A toast with it stays for six seconds.
    pub undo: Option<Callback<()>>,
    /// Stays until it is dismissed. For Android while a screen reader is
    /// on, where six seconds are not enough to reach **Undo** (section 12).
    pub hold: bool,
}

impl Toast {
    /// A toast that only says something.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            icon: None,
            undo: None,
            hold: false,
        }
    }

    /// With an icon before the words.
    #[must_use]
    pub const fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// With **Undo**.
    #[must_use]
    pub fn undo(mut self, undo: impl Into<Callback<()>>) -> Self {
        self.undo = Some(undo.into());
        self
    }

    /// Staying until dismissed.
    #[must_use]
    pub const fn hold(mut self, hold: bool) -> Self {
        self.hold = hold;
        self
    }
}

/// The app's one toast. [`crate::UiRoot`] provides it; get it with
/// [`use_toasts`].
#[derive(Clone, Copy)]
pub struct Toasts {
    showing: Signal<Option<(u64, Toast)>>,
    serial: CopyValue<u64>,
}

impl Toasts {
    pub(crate) fn new() -> Self {
        Self {
            showing: Signal::new(None),
            serial: CopyValue::new(0),
        }
    }

    /// Shows a toast in place of whatever toast is showing.
    pub fn show(mut self, toast: Toast) {
        let serial = {
            let mut serial = self.serial.write();
            *serial += 1;
            *serial
        };
        self.showing.set(Some((serial, toast)));
    }

    /// Takes the toast away, if one is showing.
    pub fn clear(mut self) {
        self.showing.set(None);
    }

    /// Takes away the toast numbered `serial`, unless a newer one has
    /// taken its place.
    fn dismiss(mut self, serial: u64) {
        let current = self.showing.peek().as_ref().map(|(serial, _)| *serial);
        if current == Some(serial) {
            self.showing.set(None);
        }
    }
}

/// The app's toasts. Call inside a [`crate::UiRoot`].
#[must_use]
pub fn use_toasts() -> Toasts {
    use_context::<Toasts>()
}

/// Where toasts appear: bottom centre, above everything.
#[component]
pub(crate) fn ToastLayer(platform: Platform) -> Element {
    let toasts = use_toasts();
    let showing = toasts.showing.read().clone();
    rsx! {
        div { class: "dui-toast-layer",
            div { class: "dui-toasts", role: "status", "aria-live": "polite",
                if let Some((serial, toast)) = showing {
                    ToastView {
                        key: "{serial}",
                        serial,
                        toast,
                        platform,
                    }
                }
            }
        }
    }
}

#[component]
fn ToastView(serial: u64, toast: Toast, platform: Platform) -> Element {
    let toasts = use_toasts();
    let id = use_outcome(move |outcome| {
        if outcome == Outcome::Expired {
            toasts.dismiss(serial);
        }
    });
    let stays = if toast.hold {
        None
    } else if toast.undo.is_some() {
        Some(STAYS_WITH_UNDO_MS)
    } else {
        Some(STAYS_MS)
    };
    // The key that undoes, shown beside Undo where there is a keyboard.
    let undo_keys = match platform {
        Platform::Desktop if cfg!(target_os = "macos") => Some("⌘ Z"),
        Platform::Desktop => Some("Ctrl Z"),
        Platform::Android => None,
    };
    let undo = toast.undo;
    rsx! {
        div {
            class: "dui-toast",
            "data-dui-toast": "",
            "data-dui-id": "{id}",
            "data-ms": stays.map(|ms| ms.to_string()),
            span { class: "dui-toast-words",
                if let Some(icon) = toast.icon {
                    IconSvg { icon }
                }
                "{toast.text}"
            }
            if let Some(undo) = undo {
                button {
                    r#type: "button",
                    class: "dui-toast-act",
                    "data-dui-undo": "",
                    onclick: move |_| {
                        undo.call(());
                        toasts.dismiss(serial);
                    },
                    IconSvg { icon: Icon::Undo, size: IconSize::Small }
                    "Undo"
                }
                if let Some(keys) = undo_keys {
                    kbd { "aria-hidden": "true", "{keys}" }
                }
            }
            if toast.hold {
                IconButton {
                    icon: Icon::X,
                    label: "Dismiss",
                    size: IconButtonSize::Small,
                    onclick: move |_| toasts.dismiss(serial),
                }
            } else if undo.is_none() {
                span { class: "dui-toast-end" }
            }
            if stays.is_some() {
                span { class: "dui-toast-bar" }
            }
        }
    }
}
