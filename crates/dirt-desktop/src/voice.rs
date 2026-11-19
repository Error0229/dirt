//! Voice on the desktop (interface design 6.18): the microphone, listening
//! on the page and in quick capture, and the words made afterwards.
//!
//! - [`Talking`] is one place that listens: the page, or quick capture's
//!   box. It shows the grains and the timer from the moment of the press,
//!   opens the microphone beside the window's thread, and when it ends
//!   hands the recording to its owner to keep, or throws it away. Nothing
//!   heard is nothing kept.
//! - [`Ear`] is the microphone while it is open, with the recording being
//!   written as the sound comes. One at a time: there is one microphone.
//! - [`use_words_maker`] makes the words of what was said on this device,
//!   in the background, when the app starts and after each thought said
//!   ([`make_words_soon`]); never on a timer.
//!
//! The model and the detector are in the voice folder, which the app ships
//! as one of its assets ([`folder`]). A development build run with
//! `DIRT_VOICE_FROM_WAV` set listens to that WAV file instead of the
//! microphone, at the pace it was recorded: for trying voice on a computer
//! whose microphone is someone else's.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use dioxus::prelude::*;
use dirt_core::models::Note;
use dirt_core::store::WordsPut;
use dirt_core::sync::worker::Trigger;
use dirt_core::voice::{make_words, Transcriber};
use dirt_ui::components::Ended;
use dirt_voice::stand_in::Feed;
use dirt_voice::{reading, Heard, Listening, Recording, VoiceError, VoiceFolder};
use tokio::sync::{broadcast, watch};

use crate::state::AppState;

/// The voice folder, shipped with the app: the model, Silero's detector,
/// and on Windows the engine's libraries. `build.rs` puts it in place from
/// the build cache, as hard links.
static VOICE: Asset = asset!("/voice", AssetOptions::folder());

/// How often the grains and the timer are told what was heard.
const SHOWN_EVERY: Duration = Duration::from_millis(50);

/// The voice folder, if this build has one that can listen.
pub fn folder() -> Option<&'static VoiceFolder> {
    static FOLDER: OnceLock<Option<VoiceFolder>> = OnceLock::new();
    FOLDER
        .get_or_init(|| {
            let shipped = dioxus::asset_resolver::asset_path(VOICE.to_string())
                .ok()
                .and_then(|path| VoiceFolder::at(path).ok());
            let found =
                shipped.or_else(|| cfg!(debug_assertions).then(VoiceFolder::built).flatten());
            if let Some(folder) = &found {
                tracing::info!("Voice from {}", folder.path().display());
            } else {
                tracing::warn!("This build has no voice folder: the mic is not offered");
            }
            found
        })
        .as_ref()
}

/// Whether the mic is offered: the folder has the model, so what is said
/// can become words.
pub fn offered() -> bool {
    folder().is_some_and(VoiceFolder::has_model)
}

// ---- Words, made in the background ----------------------------------------

/// The model, as the core asks for words.
struct Model(&'static VoiceFolder);

impl Transcriber for Model {
    fn words(&self, recording: &[u8]) -> Result<String, String> {
        reading::words_of(self.0, recording).map_err(|error| error.to_string())
    }
}

/// The notes whose words were just made, for the page that may show one.
pub fn words_made() -> &'static broadcast::Sender<Note> {
    static MADE: OnceLock<broadcast::Sender<Note>> = OnceLock::new();
    MADE.get_or_init(|| broadcast::channel(16).0)
}

fn asked() -> &'static watch::Sender<u64> {
    static ASKED: OnceLock<watch::Sender<u64>> = OnceLock::new();
    ASKED.get_or_init(|| watch::Sender::new(0))
}

/// Something was said: make its words when the maker is free.
pub fn make_words_soon() {
    asked().send_modify(|asked| *asked += 1);
}

/// Makes the words of every thought said on this device that waits for
/// them: once the store is open, again whenever another store is taken
/// into use, and after each thought said. Each round reads the store's
/// list afresh, so a thought said while a round runs is read in the next.
pub fn use_words_maker(app: &AppState) {
    let app = *app;
    use_effect(move || {
        if app.db_service.read().is_some() {
            make_words_soon();
        }
    });
    use_future(move || async move {
        let Some(folder) = folder().filter(|folder| folder.has_model()) else {
            return;
        };
        let model: Arc<dyn Transcriber> = Arc::new(Model(folder));
        let mut asked = asked().subscribe();
        loop {
            if asked.changed().await.is_err() {
                break;
            }
            asked.borrow_and_update();
            let Some(store) = app.db_service.peek().clone() else {
                continue;
            };
            let kept = app.settings.peek().voice_recording;
            match make_words(&store, Arc::clone(&model), kept).await {
                Ok(made) if made.changed() => {
                    for put in made.put {
                        if let WordsPut::Made(note) = put {
                            let _ = words_made().send(note);
                        }
                    }
                    app.reread();
                    if let Some(session) = app.session.peek().as_ref() {
                        session.notify(Trigger::Changed);
                    }
                }
                Ok(_) => {}
                Err(error) => tracing::error!("Could not make the words of what was said: {error}"),
            }
        }
    });
}

/// Asks the web view whether it plays Ogg Opus. Where it says it cannot
/// (by its documentation, macOS's before 15.4), the preview is handed a
/// recording decoded, as a WAV made in memory.
pub fn play_recordings_where_the_web_view_cannot() {
    spawn(async {
        let mut asked = document::eval(
            "dioxus.send(document.createElement('audio').canPlayType('audio/ogg; codecs=opus'))",
        );
        let answer = asked.recv::<String>().await.unwrap_or_default();
        if answer.is_empty() {
            tracing::info!("The web view cannot play Ogg Opus: recordings are played decoded");
            dirt_ui::files::served::play_ogg_as_wav(|file| {
                dirt_voice::recording::decode(file)
                    .ok()
                    .map(|sound| dirt_voice::recording::wav(&sound))
            });
        }
    });
}

// ---- The microphone ----------------------------------------------------------

/// Why listening could not start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unheard {
    /// The system did not let the app use the microphone.
    Refused,
    /// There is no microphone.
    NoMicrophone,
    /// Something else went wrong, said in words.
    Failed(String),
}

impl Unheard {
    /// What the mic's tooltip says while it shows a slash.
    pub const fn tip(&self) -> &'static str {
        match self {
            Self::Refused if cfg!(windows) => {
                "Allow the microphone for desktop apps in Windows Settings, Privacy & security, Microphone"
            }
            Self::Refused if cfg!(target_os = "macos") => {
                "Allow Dirt the microphone in System Settings, Privacy & Security, Microphone"
            }
            Self::Refused => "The system did not let Dirt use the microphone",
            Self::NoMicrophone => "No microphone was found",
            Self::Failed(_) => "The microphone could not be used",
        }
    }
}

/// The one microphone is in use.
static IN_USE: AtomicBool = AtomicBool::new(false);

/// The microphone while it is open, and the recording being written.
pub struct Ear {
    listening: Arc<Mutex<Option<Listening>>>,
    feed: Option<Feed>,
    heard: watch::Receiver<Heard>,
}

impl Ear {
    /// Opens the microphone, or the stand-in for it. It takes a moment the
    /// first time, while the engine's libraries load: call it beside the
    /// window's thread.
    pub fn open() -> Result<Self, Unheard> {
        let folder =
            folder().ok_or_else(|| Unheard::Failed("voice is not in this build".to_owned()))?;
        if IN_USE.swap(true, Ordering::SeqCst) {
            return Err(Unheard::Failed("the microphone is in use".to_owned()));
        }
        let opened = Self::open_from(folder);
        if opened.is_err() {
            IN_USE.store(false, Ordering::SeqCst);
        }
        opened
    }

    fn open_from(folder: &VoiceFolder) -> Result<Self, Unheard> {
        let failed = |error: VoiceError| match error {
            VoiceError::Refused(_) => Unheard::Refused,
            VoiceError::NoMicrophone => Unheard::NoMicrophone,
            other => Unheard::Failed(other.to_string()),
        };
        let listening = Arc::new(Mutex::new(Some(Listening::start(folder).map_err(failed)?)));
        let (tell, heard) = watch::channel(Heard {
            level: 0.0,
            ms: 0,
            speech: false,
            quiet_ms: None,
            full: false,
        });
        let fed = Arc::clone(&listening);
        let sink = move |samples: &[f32]| {
            let mut held = fed.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(listening) = held.as_mut() {
                match listening.take(samples) {
                    Ok(now) => {
                        tell.send_replace(now);
                    }
                    Err(error) => tracing::error!("The recording could not be written: {error}"),
                }
            }
        };
        let feed = match crate::settings::test_hook("DIRT_VOICE_FROM_WAV") {
            Some(wav) => dirt_voice::stand_in::play_wav(folder, &wav, sink),
            None => dirt_voice::microphone::open(folder, sink),
        }
        .map_err(failed)?;
        Ok(Self {
            listening,
            feed: Some(feed),
            heard,
        })
    }

    /// What has been heard so far, as it changes.
    pub fn heard(&self) -> watch::Receiver<Heard> {
        self.heard.clone()
    }

    /// Closes the microphone, and gives the recording, ending where the
    /// sound ends.
    pub fn finish(mut self) -> Result<Recording, String> {
        if let Some(feed) = self.feed.take() {
            feed.stop();
        }
        let listening = self
            .listening
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        listening
            .ok_or_else(|| "nothing was listening".to_owned())?
            .finish()
            .map_err(|error| error.to_string())
    }
}

impl Drop for Ear {
    fn drop(&mut self) {
        if let Some(feed) = self.feed.take() {
            feed.stop();
        }
        IN_USE.store(false, Ordering::SeqCst);
    }
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

/// A place that listens: the page, or quick capture's box.
#[derive(Clone, Copy)]
pub struct Talking {
    pub view: Signal<Option<TalkView>>,
    /// Why the microphone could not be used, the last time it was tried:
    /// the mic shows a slash until it can.
    pub unheard: Signal<Option<Unheard>>,
    ear: Signal<Option<Arc<Mutex<Option<Ear>>>>>,
    then: CopyValue<Then>,
    /// Where a kept recording goes, given with the press.
    keeper: CopyValue<Option<Callback<Recording>>>,
    serial: CopyValue<u64>,
}

impl Talking {
    /// Makes one, for a component.
    pub fn use_new() -> Self {
        Self {
            view: use_signal(|| None),
            unheard: use_signal(|| None),
            ear: use_signal(|| None),
            then: use_hook(|| CopyValue::new(Then::Listen)),
            keeper: use_hook(|| CopyValue::new(None)),
            serial: use_hook(|| CopyValue::new(0)),
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

    /// Starts listening: the view shows at once, and the microphone opens
    /// beside the window's thread. `keep` is given the recording if it is
    /// kept and speech was heard in it.
    pub fn listen(mut self, hands_free: bool, keep: Callback<Recording>) {
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
        self.view.set(Some(TalkView {
            serial,
            ms: 0,
            level: 0.0,
            hands_free,
            ended: None,
        }));
        spawn(async move {
            let opened = tokio::task::spawn_blocking(Ear::open)
                .await
                .unwrap_or_else(|error| Err(Unheard::Failed(error.to_string())));
            let ear = match opened {
                Ok(ear) => ear,
                Err(why) => {
                    tracing::warn!("Could not listen: {why:?}");
                    self.unheard.set(Some(why));
                    self.gone(serial);
                    return;
                }
            };
            self.unheard.set(None);
            let mut heard = ear.heard();
            let ear = Arc::new(Mutex::new(Some(ear)));
            self.ear.set(Some(Arc::clone(&ear)));
            match *self.then.peek() {
                Then::Keep => return self.end(true),
                Then::Throw => return self.end(false),
                Then::Listen => {}
            }
            // What is heard, to the grains and the timer, until it ends.
            loop {
                tokio::time::sleep(SHOWN_EVERY).await;
                if heard.has_changed().is_err() || self.ear.peek().is_none() {
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
                ear.lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .take()
                    .map(Ear::finish)
            })
            .await
            .ok()
            .flatten();
            let recording = match finished {
                Some(Ok(recording)) => Some(recording),
                Some(Err(error)) => {
                    tracing::error!("The recording could not be finished: {error}");
                    None
                }
                None => None,
            };
            let ended = match (&recording, keep) {
                (Some(recording), true) if recording.speech => Ended::Kept,
                (_, true) => Ended::NothingHeard,
                (_, false) => Ended::Thrown,
            };
            if let Some(shown) = self.view.write().as_mut() {
                shown.ended = Some(ended);
            }
            if ended == Ended::Kept {
                if let (Some(recording), Some(keeper)) = (recording, keeper) {
                    keeper.call(recording);
                }
            }
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
