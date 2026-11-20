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

use dioxus::prelude::*;
use dirt_core::store::WordsPut;
use dirt_core::sync::worker::Trigger;
use dirt_core::voice::{make_words, Transcriber};
use dirt_voice::stand_in::Feed;
use dirt_voice::{reading, Listening, VoiceError, VoiceFolder};
use tokio::sync::watch;

pub use dirt_ui::voice::{
    keeping_settled, make_words_soon, said_here, was_said_here, words_made, Said, Talking, Unheard,
};
use dirt_ui::voice::{Ear, Heard};

use crate::state::AppState;

/// The voice folder, shipped with the app: the model, Silero's detector,
/// and on Windows the engine's libraries. `build.rs` puts it in place from
/// the build cache, as hard links.
static VOICE: Asset = asset!("/voice", AssetOptions::folder());

/// How much sound passes between two tellings of the grains and the timer.
const SHOWN_EVERY_MS: u64 = 50;

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
    dirt_voice::ENGINE && folder().is_some_and(VoiceFolder::has_model)
}

// ---- Words, made in the background ----------------------------------------

/// The model, as the core asks for words.
struct Model(&'static VoiceFolder);

impl Transcriber for Model {
    fn words(&self, recording: &[u8]) -> Result<String, String> {
        reading::words_of(self.0, recording).map_err(|error| error.to_string())
    }
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
        let mut asked = dirt_ui::voice::words_asked();
        loop {
            if asked.changed().await.is_err() {
                break;
            }
            asked.borrow_and_update();
            let Some(store) = app.db_service.peek().clone() else {
                continue;
            };
            let kept = app.settings.peek().voice_recording;
            // Each note is told of as soon as its words are in, so that a
            // page showing it takes them before it saves again.
            let told = |put: &WordsPut| {
                if let WordsPut::Made(note) = put {
                    let _ = words_made().send(note.clone());
                }
                app.reread();
            };
            match make_words(&store, Arc::clone(&model), kept, told).await {
                Ok(made) if made.changed() => {
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

/// The one microphone is in use.
static IN_USE: AtomicBool = AtomicBool::new(false);

/// The microphone while it is open, and the recording being written.
struct DesktopEar {
    listening: Arc<Mutex<Option<Listening>>>,
    feed: Option<Feed>,
    heard: watch::Receiver<Heard>,
}

/// Opens the microphone, or the stand-in for it. It takes a moment the
/// first time, while the engine's libraries load: [`Talking`] calls it
/// beside the window's thread.
pub fn open_ear() -> Result<Box<dyn Ear>, Unheard> {
    DesktopEar::open().map(|ear| Box::new(ear) as Box<dyn Ear>)
}

impl DesktopEar {
    fn open() -> Result<Self, Unheard> {
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
        let (tell, heard) = watch::channel(Heard::default());
        let fed = Arc::clone(&listening);
        // The screen is told as the sound comes, once for each twentieth of
        // a second of it: often enough for the grains, and no more.
        let mut told_at = 0_u64;
        let sink = move |samples: &[f32]| {
            let mut held = fed.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(listening) = held.as_mut() {
                match listening.take(samples) {
                    Ok(now) if now.full || now.ms >= told_at + SHOWN_EVERY_MS => {
                        told_at = now.ms;
                        tell.send_replace(Heard {
                            level: now.level,
                            ms: now.ms,
                            full: now.full,
                        });
                    }
                    Ok(_) => {}
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
}

impl Ear for DesktopEar {
    fn heard(&self) -> watch::Receiver<Heard> {
        self.heard.clone()
    }

    fn finish(mut self: Box<Self>) -> Result<Said, String> {
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
            .map(|recording| Said {
                file: recording.file,
                duration_ms: recording.duration_ms,
                speech: recording.speech,
            })
            .map_err(|error| error.to_string())
    }
}

impl Drop for DesktopEar {
    fn drop(&mut self) {
        if let Some(feed) = self.feed.take() {
            feed.stop();
        }
        IN_USE.store(false, Ordering::SeqCst);
    }
}
