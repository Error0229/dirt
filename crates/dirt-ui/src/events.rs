//! How the script tells Rust that something ended.
//!
//! What follows a finger runs in the script from first touch to release
//! (the plan's rule: a call into Rust for every movement is too slow on a
//! phone). When it ends in a way the app must know about, the script says
//! so once, over one channel that [`crate::UiRoot`] opens. A component
//! that wants to hear gives its element an ID with [`use_outcome`].

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use dioxus::prelude::*;
use serde::Deserialize;

/// How something the script ran ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    /// A card was swiped past the mark and has flown off.
    Swipe,
    /// A panel or sheet was closed from the script: Esc, or a sheet
    /// dragged down past the mark.
    Dismiss,
    /// A toast's time ran out and it has closed.
    Expired,
    /// The end of a list has come near while scrolling: there is room for
    /// more rows.
    More,
    /// Paste was chosen in the app's own menu. Reading the clipboard is
    /// the app's to do; it hands the text back with [`crate::dom::paste`].
    Paste,
    /// The pointer came onto an element that waits for it
    /// (`data-dui-reach`): the handle at the window's edge, which brings
    /// the list back while a note is written.
    Reach,
    /// A press began on an empty part of the window's top row
    /// (`data-dui-drag`): the system moves the window from here until the
    /// button is let go.
    Drag,
    /// A double-click on an empty part of the window's top row: maximise
    /// or restore, as a title bar's double-click does.
    Maximize,
    /// A press began on one of the window's edges that the app draws
    /// (`data-dui-resize`): the system resizes the window from that edge
    /// until the button is let go.
    Resize,
    /// A list was pulled down past the mark and let go: sync now. See
    /// [`crate::components::PullToSync`].
    Pull,
    /// Android's Back, handed to the element that takes it: the one with
    /// `data-dui-back` that comes last in the page. A panel or sheet that
    /// is open takes it first, as [`Outcome::Dismiss`].
    Back,
    /// The file preview asks for the next file: →, or a swipe to the left.
    Next,
    /// The file preview asks for the file before: ←, or a swipe to the
    /// right.
    Previous,
    /// Nothing in the file preview moves, and the file shown is drawn: the
    /// files either side may be read ahead now (design 6.16).
    Still,
    /// The desktop's quick capture has begun to shrink to its grain: the
    /// window carries it from here to where it goes (sections 5.5, 7.2).
    Away,
    /// Quick capture has finished leaving, or fading: its window can hide.
    /// Listening has finished drawing how it ended: it can go. A page that
    /// grows ([`crate::components::GrowPage`]) has shrunk away after it
    /// closed, and its field has let the keyboard go: it can be inert.
    Gone,
    /// A press on the mic began: listen now (section 6.18).
    Listen,
    /// The mic was tapped, or slid up while held: listen on, hands-free.
    Lock,
    /// The mic was let go after a hold: keep what was said.
    Keep,
    /// The mic was slid to the left while held, or the press was taken
    /// away: throw it away.
    Throw,
}

/// One message from the script: which element, and how it ended.
#[derive(Debug, Clone, Deserialize)]
pub struct Message {
    /// The element's `data-dui-id`.
    pub id: String,
    /// How it ended.
    pub kind: Outcome,
    /// Which closing of a page that grows has gone ([`Outcome::Gone`]), as
    /// the page named it (`data-dui-closing`): one from an earlier closing
    /// must not end a later one.
    #[serde(default)]
    pub closing: Option<u64>,
}

/// Who is listening for which element. [`crate::UiRoot`] provides one.
#[derive(Clone, Copy)]
pub struct Bus {
    listeners: CopyValue<HashMap<String, Callback<Message>>>,
    received: Signal<u64>,
}

impl Bus {
    pub(crate) fn new() -> Self {
        Self {
            listeners: CopyValue::new(HashMap::new()),
            received: Signal::new(0),
        }
    }

    fn listen(mut self, listener: Callback<Message>) -> String {
        // Never used twice in the process: the desktop's two windows each
        // have a root, and an ID names one element in one of them.
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = format!("dui-{}", NEXT.fetch_add(1, Ordering::Relaxed) + 1);
        self.listeners.write().insert(id.clone(), listener);
        id
    }

    fn forget(mut self, id: &str) {
        // The root may have gone first, taking the listeners with it.
        if let Ok(mut listeners) = self.listeners.try_write() {
            listeners.remove(id);
        }
    }

    /// Hands a message to whoever listens for its element.
    pub(crate) fn deliver(mut self, message: &Message) {
        *self.received.write() += 1;
        let listener = self.listeners.read().get(&message.id).copied();
        if let Some(listener) = listener {
            listener.call(message.clone());
        }
    }

    /// How many messages the script has sent since the app started. The
    /// gallery shows it, to show that a finger moving sends none.
    #[must_use]
    pub fn received(self) -> u64 {
        (self.received)()
    }
}

/// Listens for how the script's work on one element ends. Returns the ID
/// to put on that element as `data-dui-id`.
///
/// Outside a [`crate::UiRoot`] nothing is heard, and the ID is empty.
pub fn use_outcome(mut on_outcome: impl FnMut(Outcome) + 'static) -> String {
    use_message(move |message| on_outcome(message.kind))
}

/// As [`use_outcome`], with the whole message: what it says besides how it
/// ended ([`Message::closing`]).
pub fn use_message(mut on_message: impl FnMut(Message) + 'static) -> String {
    let listener = use_callback(move |message: Message| on_message(message));
    let bus = try_use_context::<Bus>();
    let id = use_hook(|| bus.map(|bus| bus.listen(listener)).unwrap_or_default());
    use_drop({
        let id = id.clone();
        move || {
            if let Some(bus) = bus {
                bus.forget(&id);
            }
        }
    });
    id
}

/// The channel's Rust end, the end of the eval that installs the script:
/// its messages, one at a time, for as long as the page lives. A root
/// lives as long as its page.
pub(crate) const CONNECT: &str =
    "DirtUi.connect(message => dioxus.send(message)); await new Promise(() => {});";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_names_its_closing_only_when_it_has_one() {
        let plain: Message = serde_json::from_str(r#"{"id":"dui-1","kind":"back"}"#).unwrap();
        assert_eq!(plain.kind, Outcome::Back);
        assert_eq!(plain.closing, None);
        let gone: Message =
            serde_json::from_str(r#"{"id":"dui-1","kind":"gone","closing":3}"#).unwrap();
        assert_eq!(gone.kind, Outcome::Gone);
        assert_eq!(gone.closing, Some(3));
    }
}
