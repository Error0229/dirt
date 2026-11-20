//! Listening, as both apps do it (interface design 6.18, 7.11, 8.9).
//!
//! - [`Talking`] is one place that listens: the desktop's page or quick
//!   capture's box, Android's dock. It shows the grains and the timer from
//!   the moment of the press, opens the microphone beside the window's
//!   thread, and when it ends hands the recording to its owner to keep, or
//!   throws it away. Nothing heard is nothing kept.
//! - [`Ear`] is the microphone while it is open, with the recording being
//!   written as the sound comes. Each app makes its own (cpal on the
//!   desktop, Android's `AudioRecord`), with `dirt-voice`, which this crate
//!   does not depend on.
//! - A recording being finished is counted ([`keeping_settled`]), so that
//!   quitting, and taking another store into use, wait for it to be kept
//!   in the store it was said into.
//! - [`make_words_soon`] asks the app's maker of words to run, and
//!   [`words_made`] tells of each note whose words are in.

use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use dioxus::prelude::*;
use dirt_core::models::{Note, NoteId};
use tokio::sync::{broadcast, watch};

use crate::appearance::Platform;
use crate::components::Ended;

/// What the screen is told while it listens.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Heard {
    /// How loud it is now, from 0 to 1.
    pub level: f32,
    /// How long it has listened, in milliseconds of sound.
    pub ms: u64,
    /// Ten minutes: listening stops, keeping what was said.
    pub full: bool,
}

/// A finished recording.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Said {
    /// The file, Ogg Opus.
    pub file: Vec<u8>,
    pub duration_ms: u64,
    /// Speech was heard in it. A recording without is not kept.
    pub speech: bool,
}

/// The microphone while it is open.
pub trait Ear: Send {
    /// What has been heard so far, as it changes. The sender goes when
    /// the microphone closes.
    fn heard(&self) -> watch::Receiver<Heard>;
    /// Closes the microphone, and gives the recording, ending where the
    /// sound ends: what was still on its way is kept.
    fn finish(self: Box<Self>) -> Result<Said, String>;
}

/// Opens the microphone, beside the window's thread: it may take a moment.
pub type OpenEar = fn() -> Result<Box<dyn Ear>, Unheard>;

/// Why listening could not start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unheard {
    /// Android asked the person for the microphone, and it was allowed:
    /// the press that asked makes nothing, and the next one listens. The
    /// mic shows no slash.
    Asked,
    /// The system did not let the app use the microphone. Android may ask
    /// the person again.
    Refused,
    /// Refused, and Android will not ask again ("Don't allow" twice, or
    /// "Don't ask again"): it is allowed in Android's own settings.
    RefusedForGood,
    /// There is no microphone.
    NoMicrophone,
    /// Something else went wrong, said in words.
    Failed(String),
}

impl Unheard {
    /// What the mic's tooltip, or its long press, says while it shows a
    /// slash: where to allow it (6.18).
    #[must_use]
    pub const fn tip(&self) -> &'static str {
        match (self, Platform::current()) {
            (Self::Refused, Platform::Android) => {
                "Dirt may not use the microphone. Press to be asked again"
            }
            (Self::RefusedForGood, Platform::Android) => {
                "Allow the microphone in Android's Settings: Apps, Dirt, Permissions"
            }
            (Self::Refused | Self::RefusedForGood, _) if cfg!(windows) => {
                "Allow the microphone for desktop apps in Windows Settings, Privacy & security, Microphone"
            }
            (Self::Refused | Self::RefusedForGood, _) if cfg!(target_os = "macos") => {
                "Allow Dirt the microphone in System Settings, Privacy & Security, Microphone"
            }
            (Self::Refused | Self::RefusedForGood, _) => {
                "The system did not let Dirt use the microphone"
            }
            (Self::NoMicrophone, _) => "No microphone was found",
            (Self::Failed(_), _) => "The microphone could not be used",
            (Self::Asked, _) => "Hold to talk, or tap",
        }
    }
}

// ---- Said here, and being kept ------------------------------------------------

/// Notes said in this run, whose words a screen reader is told of when
/// they come (section 12). Not those of a backlog made as the app starts.
fn said() -> &'static Mutex<HashSet<NoteId>> {
    static SAID: OnceLock<Mutex<HashSet<NoteId>>> = OnceLock::new();
    SAID.get_or_init(|| Mutex::new(HashSet::new()))
}

/// `note` was said here, now.
pub fn said_here(note: NoteId) {
    said()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(note);
}

/// Whether `note` was said here in this run; asked once, when its words
/// come.
pub fn was_said_here(note: NoteId) -> bool {
    said()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(&note)
}

/// Recordings being said or finished and not yet handed to the store's
/// queue.
static KEEPING: AtomicUsize = AtomicUsize::new(0);

fn keeping() -> &'static watch::Sender<usize> {
    static NEWS: OnceLock<watch::Sender<usize>> = OnceLock::new();
    NEWS.get_or_init(|| watch::Sender::new(0))
}

fn keeping_one() {
    let now = KEEPING.fetch_add(1, Ordering::SeqCst) + 1;
    keeping().send_replace(now);
}

fn kept_one() {
    let now = KEEPING.fetch_sub(1, Ordering::SeqCst).saturating_sub(1);
    keeping().send_replace(now);
}

/// Waits until every recording being finished has been handed to the
/// store's queue, for at most `at_most`, and says whether it was.
///
/// Quitting, and taking another store into use, wait for this before they
/// wait for the queue: a recording is only in memory until then.
pub async fn keeping_settled(at_most: Duration) -> bool {
    let mut news = keeping().subscribe();
    tokio::time::timeout(at_most, news.wait_for(|count| *count == 0))
        .await
        .is_ok_and(|seen| seen.is_ok())
}

// ---- Words ---------------------------------------------------------------------

/// The notes whose words were just made, for a page that may show one.
pub fn words_made() -> &'static broadcast::Sender<Note> {
    static MADE: OnceLock<broadcast::Sender<Note>> = OnceLock::new();
    MADE.get_or_init(|| broadcast::channel(16).0)
}

fn asked() -> &'static watch::Sender<u64> {
    static ASKED: OnceLock<watch::Sender<u64>> = OnceLock::new();
    ASKED.get_or_init(|| watch::Sender::new(0))
}

/// Something was said, or a store was opened: the app's maker of words
/// runs when it is free.
pub fn make_words_soon() {
    asked().send_modify(|asked| *asked += 1);
}

/// What the app's maker of words waits on: each change is a round to run.
#[must_use]
pub fn words_asked() -> watch::Receiver<u64> {
    asked().subscribe()
}

// ---- One place that listens ----------------------------------------------------

/// What the listening view shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TalkView {
    /// A new serial is a new view, made afresh.
    pub serial: u64,
    pub ms: u64,
    pub level: f32,
    /// It listens on with no hand on the mic, until ✓ or ✕.
    pub hands_free: bool,
    pub ended: Option<Ended>,
}

/// What was asked of a view before its microphone was open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Then {
    #[default]
    Listen,
    Keep,
    Throw,
}

type HeldEar = Arc<Mutex<Option<Box<dyn Ear>>>>;

/// A place that listens.
#[derive(Clone, Copy)]
pub struct Talking {
    pub view: Signal<Option<TalkView>>,
    /// Why the microphone could not be used, the last time it was tried:
    /// the mic shows a slash until it can.
    pub unheard: Signal<Option<Unheard>>,
    ear: Signal<Option<HeldEar>>,
    then: CopyValue<Then>,
    /// Where a kept recording goes, given with the press.
    keeper: CopyValue<Option<Callback<Said>>>,
    serial: CopyValue<u64>,
    open: CopyValue<OpenEar>,
}

impl Talking {
    /// Makes one, for a component, with the app's way of opening the
    /// microphone.
    pub fn use_new(open: OpenEar) -> Self {
        Self {
            view: use_signal(|| None),
            unheard: use_signal(|| None),
            ear: use_signal(|| None),
            then: use_hook(|| CopyValue::new(Then::Listen)),
            keeper: use_hook(|| CopyValue::new(None)),
            serial: use_hook(|| CopyValue::new(0)),
            open: use_hook(|| CopyValue::new(open)),
        }
    }

    /// Whether it is listening, or drawing how it ended.
    pub fn is_shown(self) -> bool {
        self.view.peek().is_some()
    }

    /// Whether it is listening and has not ended.
    pub fn is_listening(self) -> bool {
        self.view.peek().is_some_and(|view| view.ended.is_none())
    }

    /// Whether it is listening, asked where its signals may have gone (as
    /// a screen goes): false then.
    pub fn is_listening_still(self) -> bool {
        self.view
            .try_peek()
            .is_ok_and(|view| view.is_some_and(|view| view.ended.is_none()))
    }

    /// Starts listening: the view shows at once, and the microphone opens
    /// beside the window's thread. `keep` is given the recording if it is
    /// kept and speech was heard in it.
    pub fn listen(mut self, hands_free: bool, keep: Callback<Said>) {
        if self.view.peek().is_some() {
            return;
        }
        let serial = {
            let mut serial = self.serial.write();
            *serial += 1;
            *serial
        };
        self.keeper.set(Some(keep));
        self.then.set(Then::Listen);
        // Counted from now until it is kept or let go: quitting waits.
        keeping_one();
        self.view.set(Some(TalkView {
            serial,
            ms: 0,
            level: 0.0,
            hands_free,
            ended: None,
        }));
        let open = *self.open.peek();
        spawn(async move {
            let opened = tokio::task::spawn_blocking(open)
                .await
                .unwrap_or_else(|error| Err(Unheard::Failed(error.to_string())));
            let ear = match opened {
                Ok(ear) => ear,
                Err(why) => {
                    tracing::warn!("Could not listen: {why:?}");
                    self.unheard.set((why != Unheard::Asked).then_some(why));
                    self.gone(serial);
                    kept_one();
                    return;
                }
            };
            self.unheard.set(None);
            let mut heard = ear.heard();
            let ear: HeldEar = Arc::new(Mutex::new(Some(ear)));
            self.ear.set(Some(Arc::clone(&ear)));
            match *self.then.peek() {
                Then::Keep => return self.end(true),
                Then::Throw => return self.end(false),
                Then::Listen => {}
            }
            // What is heard, to the grains and the timer, as it is told,
            // until it ends: the microphone closed lets go of the telling.
            loop {
                if heard.changed().await.is_err() || self.ear.peek().is_none() {
                    break;
                }
                let now = *heard.borrow_and_update();
                let mut view = self.view;
                let still = view
                    .peek()
                    .is_some_and(|view| view.serial == serial && view.ended.is_none());
                if !still {
                    break;
                }
                if let Some(view) = view.write().as_mut() {
                    view.ms = now.ms;
                    view.level = now.level;
                }
                if now.full {
                    // Ten minutes: it stops, keeping what was said.
                    self.end(true);
                    break;
                }
            }
        });
    }

    /// Listens on with no hand on the mic: a tap, or a slide up.
    pub fn lock(mut self) {
        if let Some(view) = self.view.write().as_mut() {
            view.hands_free = true;
        }
    }

    /// Ends listening: kept (✓, Enter, letting go of a hold) or thrown away
    /// (✕, Esc, a slide to the left). A recording kept with no speech in it
    /// is nothing heard, and is not kept either.
    pub fn end(mut self, keep: bool) {
        let Some(view) = *self.view.peek() else {
            return;
        };
        if view.ended.is_some() {
            return;
        }
        let Some(ear) = self.ear.write().take() else {
            // The microphone is not open yet: it ends as soon as it is.
            self.then.set(if keep { Then::Keep } else { Then::Throw });
            return;
        };
        let keeper = self.keeper.take();
        spawn(async move {
            let finished = tokio::task::spawn_blocking(move || {
                let held = ear.lock().unwrap_or_else(PoisonError::into_inner).take();
                held.map(Ear::finish)
            })
            .await
            .ok()
            .flatten();
            let said = match finished {
                Some(Ok(said)) => Some(said),
                Some(Err(error)) => {
                    tracing::error!("The recording could not be finished: {error}");
                    None
                }
                None => None,
            };
            let ended = match (&said, keep) {
                (Some(said), true) if said.speech => Ended::Kept,
                (_, true) => Ended::NothingHeard,
                (_, false) => Ended::Thrown,
            };
            if let Some(shown) = self.view.write().as_mut() {
                shown.ended = Some(ended);
            }
            if ended == Ended::Kept {
                if let (Some(said), Some(keeper)) = (said, keeper) {
                    keeper.call(said);
                }
            }
            kept_one();
        });
    }

    /// The view `serial` has drawn how it ended: it goes.
    pub fn gone(mut self, serial: u64) {
        if self.view.peek().is_some_and(|view| view.serial == serial) {
            self.view.set(None);
            self.ear.set(None);
        }
    }
}
