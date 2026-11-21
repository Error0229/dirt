//! The toast (interface design, sections 5.5, 6.7 and 12): dark on light
//! and light on dark, at the bottom, one at a time. It carries at most one
//! action, **Undo**, and a draining bar that shows the time left.
//!
//! An error has a place of its own, above the toast
//! ([`Toasts::show_error`]): it stays until it is dismissed, and the
//! ordinary toasts neither replace it nor clear it, nor it them. It may
//! carry an action of its own ([`Toast::action`]).
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

/// A screen reader is on (Android's TalkBack): toasts stay until they are
/// dismissed, with ✕, so that Undo is still there by the time the reader
/// reaches it (section 12). The app says so as it starts and each time the
/// person comes back to it.
static SCREEN_READER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Says whether a screen reader is on. Toasts shown from now on follow it.
pub fn set_screen_reader(on: bool) {
    SCREEN_READER.store(on, std::sync::atomic::Ordering::Relaxed);
}

fn screen_reader() -> bool {
    SCREEN_READER.load(std::sync::atomic::Ordering::Relaxed)
}

/// How long a toast stays, in milliseconds, or nothing when it stays until
/// it is dismissed: one told to hold, or any while a screen reader is on.
const fn stays_ms(hold: bool, undo: bool, screen_reader: bool) -> Option<u32> {
    if hold || screen_reader {
        None
    } else if undo {
        Some(STAYS_WITH_UNDO_MS)
    } else {
        Some(STAYS_MS)
    }
}

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
    /// An action other than **Undo**: its words, its icon, and what it
    /// does. An error's "Copy text".
    pub action: Option<(String, Icon, Callback<()>)>,
    /// More than the words say, in the tooltip of the words: why an error
    /// happened.
    pub detail: Option<String>,
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
            action: None,
            detail: None,
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

    /// With an action of its own.
    #[must_use]
    pub fn action(
        mut self,
        label: impl Into<String>,
        icon: Icon,
        does: impl Into<Callback<()>>,
    ) -> Self {
        self.action = Some((label.into(), icon, does.into()));
        self
    }

    /// With more said in the tooltip of its words.
    #[must_use]
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

/// The app's one toast. [`crate::UiRoot`] provides it; get it with
/// [`use_toasts`].
#[derive(Clone, Copy)]
pub struct Toasts {
    showing: Signal<Option<(u64, Toast)>>,
    error: Signal<Option<(u64, Toast)>>,
    serial: CopyValue<u64>,
}

impl Toasts {
    pub(crate) fn new() -> Self {
        Self {
            showing: Signal::new(None),
            error: Signal::new(None),
            serial: CopyValue::new(0),
        }
    }

    fn next(mut self) -> u64 {
        let mut serial = self.serial.write();
        *serial += 1;
        *serial
    }

    /// Shows a toast in place of whatever toast is showing. An error that
    /// is showing stays.
    pub fn show(mut self, toast: Toast) {
        let serial = self.next();
        self.showing.set(Some((serial, toast)));
    }

    /// Takes the toast away, if one is showing. An error stays.
    pub fn clear(mut self) {
        self.showing.set(None);
    }

    /// Shows an error, in its own place above the toast, in place of any
    /// error showing. It stays until it is dismissed.
    pub fn show_error(mut self, toast: Toast) {
        let serial = self.next();
        self.error.set(Some((serial, toast.hold(true))));
    }

    /// Takes the error away, if one is showing.
    pub fn clear_error(mut self) {
        self.error.set(None);
    }

    /// Whether an error is showing.
    #[must_use]
    pub fn error_showing(self) -> bool {
        self.error.peek().is_some()
    }

    /// Takes away the toast or the error numbered `serial`, unless a newer
    /// one has taken its place.
    fn dismiss(mut self, serial: u64) {
        let current = self.showing.peek().as_ref().map(|(serial, _)| *serial);
        if current == Some(serial) {
            self.showing.set(None);
        }
        let error = self.error.peek().as_ref().map(|(serial, _)| *serial);
        if error == Some(serial) {
            self.error.set(None);
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
    let error = toasts.error.read().clone();
    rsx! {
        div { class: "dui-toast-layer",
            div { class: "dui-toasts-err", role: "alert",
                if let Some((serial, toast)) = error {
                    ToastView {
                        key: "{serial}",
                        serial,
                        toast,
                        platform,
                    }
                }
            }
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
    let stays = stays_ms(toast.hold, toast.undo.is_some(), screen_reader());
    let hold = stays.is_none();
    // The key that undoes, shown beside Undo where there is a keyboard.
    let undo_keys = match platform {
        Platform::Desktop if cfg!(target_os = "macos") => Some("⌘ Z"),
        Platform::Desktop => Some("Ctrl Z"),
        Platform::Android => None,
    };
    let undo = toast.undo;
    let action = toast.action.clone();
    rsx! {
        div {
            class: "dui-toast",
            "data-dui-toast": "",
            "data-dui-id": "{id}",
            "data-ms": stays.map(|ms| ms.to_string()),
            span {
                class: "dui-toast-words",
                "data-tip": toast.detail,
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
            if let Some((label, icon, does)) = action {
                button {
                    r#type: "button",
                    class: "dui-toast-act",
                    onclick: move |_| does.call(()),
                    IconSvg { icon, size: IconSize::Small }
                    "{label}"
                }
            }
            if hold {
                IconButton {
                    icon: Icon::X,
                    label: "Dismiss",
                    size: IconButtonSize::Small,
                    onclick: move |_| toasts.dismiss(serial),
                }
            } else if undo.is_none() && toast.action.is_none() {
                span { class: "dui-toast-end" }
            }
            if stays.is_some() {
                span { class: "dui-toast-bar" }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_toast_goes_by_itself_unless_it_holds() {
        assert_eq!(stays_ms(false, false, false), Some(STAYS_MS));
        assert_eq!(stays_ms(false, true, false), Some(STAYS_WITH_UNDO_MS));
        assert_eq!(stays_ms(true, false, false), None);
        assert_eq!(stays_ms(true, true, false), None);
    }

    #[test]
    fn a_screen_reader_holds_every_toast() {
        assert_eq!(stays_ms(false, false, true), None);
        assert_eq!(stays_ms(false, true, true), None);
    }
}
