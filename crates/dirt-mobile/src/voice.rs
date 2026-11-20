//! Voice on Android (interface design 6.18, 8.9): the microphone for the
//! dock, the recordings the capture sheet keeps, and the words made
//! afterwards, by the app or by the sheet's job.
//!
//! - [`open_ear`] opens the microphone for the dock's [`Talking`]. The
//!   microphone is Android's `AudioRecord`, in Kotlin
//!   (`dev.dirt.capture.Microphone`), which hands each piece of sound to
//!   [`Java_dev_dirt_capture_Microphone_heard`]; `dirt-voice` writes the
//!   recording and listens for speech, as on the desktop. The first press
//!   asks for the microphone, in Android's own dialog.
//! - [`recorder`] makes a WAV the capture sheet kept into what the note
//!   keeps: Ogg Opus, its length, and whether Silero heard speech in it.
//!   The sheet has none of the app's Rust, and records the sound as it is.
//! - [`use_words_maker`] makes the words of what was said on this phone,
//!   in the background, when the app starts or comes back and after each
//!   thought said: never on a timer. The sheet's job does the same when the
//!   app is not running ([`make_words_with`]). One reading at a time in the
//!   process: the model takes about 0.6 GiB while it reads, and the app and
//!   the job share the process.
//!
//! The voice folder ships in the APK and is copied out of it once, on
//! first use (`VoiceFiles.kt`): the engine opens its files by path.

#![allow(unsafe_code)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use dioxus::prelude::*;
use dirt_core::models::VoiceRecording;
use dirt_core::store::WordsPut;
use dirt_core::sync::worker::Trigger;
use dirt_core::voice::{make_words, MadeWords, Transcriber};
use dirt_core::Store;
use dirt_ui::voice::{Ear, Heard};
use dirt_voice::{reading, Listening, VoiceError, VoiceFolder};
use jni::objects::{JClass, JShortArray};
use jni::sys::jint;
use jni::JNIEnv;
use tokio::sync::watch;

pub use dirt_ui::voice::{
    make_words_soon, said_here, was_said_here, words_made, Said, Talking, Unheard,
};

use crate::capture::Recorded;
use crate::services::activity;
use crate::state::AppState;

/// How much sound passes between two tellings of the grains and the timer.
const SHOWN_EVERY_MS: u64 = 50;

/// How long the person may take to answer Android's dialog.
const ANSWER_WITHIN: Duration = Duration::from_secs(300);

/// Whether the mic is offered: the engine is in this build, and the model
/// is among the APK's assets, so what is said can become words.
pub fn offered() -> bool {
    static OFFERED: OnceLock<bool> = OnceLock::new();
    *OFFERED.get_or_init(|| {
        let shipped = dirt_voice::ENGINE && activity::voice_shipped();
        if !shipped {
            tracing::warn!("This build has no voice: the mic is not offered");
        }
        shipped
    })
}

/// The voice folder with the detector in it, for listening; copied out of
/// the APK the first time. `None` in a build without voice.
fn listening_folder() -> Option<VoiceFolder> {
    static FOLDER: OnceLock<VoiceFolder> = OnceLock::new();
    if let Some(folder) = FOLDER.get() {
        return Some(folder.clone());
    }
    let folder = activity::voice_folder("listening").and_then(|path| VoiceFolder::at(path).ok())?;
    Some(FOLDER.get_or_init(|| folder).clone())
}

/// The voice folder with the model in it too: the first time, the model
/// is copied out of the APK, which takes a moment.
fn words_folder() -> Option<VoiceFolder> {
    activity::voice_folder("words")
        .and_then(|path| VoiceFolder::at(path).ok())
        .filter(VoiceFolder::has_model)
}

// ---- The capture sheet's recordings ------------------------------------------

/// What makes the sheet's WAV recordings what a note keeps, with the
/// detector in `folder`.
pub fn recorder(folder: VoiceFolder) -> crate::capture::Recorder {
    Arc::new(move |wav: &[u8]| {
        let samples = crate::capture::samples_of_wav(wav)
            .ok_or_else(|| "not a WAV the sheet writes".to_owned())?;
        let mut listening = Listening::start(&folder).map_err(|error| error.to_string())?;
        for piece in samples.chunks(1600) {
            listening.take(piece).map_err(|error| error.to_string())?;
        }
        let recording = listening.finish().map_err(|error| error.to_string())?;
        Ok(Recorded {
            file: recording.file,
            duration_ms: recording.duration_ms,
            speech: recording.speech,
        })
    })
}

/// The recorder for the app, if this build has voice.
pub fn app_recorder() -> Option<crate::capture::Recorder> {
    offered().then(listening_folder).flatten().map(recorder)
}

// ---- Words ---------------------------------------------------------------------

/// One reading at a time in the process, the app's or the job's.
static READING: Mutex<()> = Mutex::new(());

/// The model, as the core asks for words.
enum Model {
    /// The app's: the folder is asked of the activity at the first
    /// reading, which copies the model out of the APK the first time.
    App,
    /// The job's: Kotlin gave the folder.
    At(VoiceFolder),
}

impl Transcriber for Model {
    fn words(&self, recording: &[u8]) -> Result<String, String> {
        let _one = READING.lock().unwrap_or_else(PoisonError::into_inner);
        let folder = match self {
            Self::App => {
                words_folder().ok_or_else(|| "the speech model is not on this phone".to_owned())?
            }
            Self::At(folder) => folder.clone(),
        };
        reading::words_of(&folder, recording).map_err(|error| error.to_string())
    }
}

/// Makes the words of every thought said on this phone that waits for
/// them, with the model in `folder`: the sheet's job, with no screen.
pub async fn make_words_with(
    store: &Store,
    folder: &str,
    kept: VoiceRecording,
) -> dirt_core::Result<MadeWords> {
    let folder = match VoiceFolder::at(PathBuf::from(folder)) {
        Ok(folder) if folder.has_model() => folder,
        _ => return Ok(MadeWords::default()),
    };
    make_words(store, Arc::new(Model::At(folder)), kept, |_| {}).await
}

/// Makes the words of every thought said on this phone that waits for
/// them: once the store is open, again whenever another store is taken
/// into use, and whenever something was said ([`make_words_soon`]). Each
/// round reads the store's list afresh.
pub fn use_words_maker(app: &AppState) {
    let app = *app;
    use_effect(move || {
        if app.store.read().is_some() {
            make_words_soon();
        }
    });
    use_future(move || async move {
        if !offered() {
            return;
        }
        let model: Arc<dyn Transcriber> = Arc::new(Model::App);
        let mut asked = dirt_ui::voice::words_asked();
        loop {
            if asked.changed().await.is_err() {
                break;
            }
            asked.borrow_and_update();
            let Some(store) = app.store.peek().clone() else {
                continue;
            };
            let kept = app.settings.peek().voice_recording;
            let told = |put: &WordsPut| {
                if let WordsPut::Made(note) = put {
                    let _ = words_made().send(note.clone());
                }
                app.reread();
            };
            match make_words(store.core(), Arc::clone(&model), kept, told).await {
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

// ---- The microphone ----------------------------------------------------------

/// The one microphone is in use.
static IN_USE: AtomicBool = AtomicBool::new(false);

/// Where the sound Kotlin hands over goes while the dock listens.
type Sink = Box<dyn FnMut(&[f32]) + Send>;

fn sink() -> &'static Mutex<Option<Sink>> {
    static SINK: OnceLock<Mutex<Option<Sink>>> = OnceLock::new();
    SINK.get_or_init(|| Mutex::new(None))
}

/// `dev.dirt.capture.Microphone.heard(samples, count)`: a piece of sound,
/// from the microphone's thread.
#[no_mangle]
pub extern "system" fn Java_dev_dirt_capture_Microphone_heard<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    samples: JShortArray<'local>,
    count: jint,
) {
    // A panic must not cross into Java: it would end the process.
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let count = usize::try_from(count).unwrap_or_default();
        let mut piece = vec![0_i16; count];
        if env.get_short_array_region(&samples, 0, &mut piece).is_err() {
            return;
        }
        let piece: Vec<f32> = piece
            .iter()
            .map(|sample| f32::from(*sample) / 32768.0)
            .collect();
        if let Some(sink) = sink()
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_mut()
        {
            sink(&piece);
        }
    }));
}

/// The microphone while the dock listens, and the recording being written.
struct AndroidEar {
    listening: Arc<Mutex<Option<Listening>>>,
    heard: watch::Receiver<Heard>,
    open: bool,
}

/// Asks for the microphone, in Android's own dialog, and waits for the
/// answer. The press that asks makes nothing: allowed, the next listens.
/// Beside the window's thread: it waits for the person.
pub fn ask() -> Unheard {
    static ASKING: AtomicBool = AtomicBool::new(false);
    let already = ASKING.swap(true, Ordering::SeqCst);
    if !activity::ask_for_microphone() {
        if !already {
            ASKING.store(false, Ordering::SeqCst);
        }
        return Unheard::Failed("there is no screen to ask over".to_owned());
    }
    // Pressed again while asking: Android asks again, and the first press
    // waits for the answer.
    if already {
        return Unheard::Asked;
    }
    let began = std::time::Instant::now();
    let answer = loop {
        if began.elapsed() >= ANSWER_WITHIN {
            break Unheard::Refused;
        }
        match activity::take_microphone_answer().as_deref() {
            Some("granted") => break Unheard::Asked,
            Some("refused") => break Unheard::Refused,
            Some(_) => break Unheard::RefusedForGood,
            None => std::thread::sleep(Duration::from_millis(100)),
        }
    };
    ASKING.store(false, Ordering::SeqCst);
    answer
}

/// Opens the microphone for the dock, asking for it first if the app may
/// not use it yet. Called beside the window's thread: it may take a moment.
pub fn open_ear() -> Result<Box<dyn Ear>, Unheard> {
    if !activity::microphone_allowed() {
        return Err(ask());
    }
    let folder = listening_folder()
        .ok_or_else(|| Unheard::Failed("voice is not in this build".to_owned()))?;
    if IN_USE.swap(true, Ordering::SeqCst) {
        return Err(Unheard::Failed("the microphone is in use".to_owned()));
    }
    let opened = AndroidEar::open(&folder);
    if opened.is_err() {
        IN_USE.store(false, Ordering::SeqCst);
    }
    opened.map(|ear| Box::new(ear) as Box<dyn Ear>)
}

impl AndroidEar {
    fn open(folder: &VoiceFolder) -> Result<Self, Unheard> {
        let failed = |error: VoiceError| Unheard::Failed(error.to_string());
        let listening = Arc::new(Mutex::new(Some(Listening::start(folder).map_err(failed)?)));
        let (tell, heard) = watch::channel(Heard::default());
        let fed = Arc::clone(&listening);
        let mut told_at = 0_u64;
        let take = move |samples: &[f32]| {
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
        *sink().lock().unwrap_or_else(PoisonError::into_inner) = Some(Box::new(take));
        match activity::start_microphone() {
            0 => Ok(Self {
                listening,
                heard,
                open: true,
            }),
            answer => {
                sink().lock().unwrap_or_else(PoisonError::into_inner).take();
                Err(if answer == 1 {
                    Unheard::Refused
                } else {
                    Unheard::Failed("the microphone could not be opened".to_owned())
                })
            }
        }
    }

    /// Closes the microphone: Kotlin returns once every piece heard has
    /// been handed over. The sink is not held meanwhile, since handing a
    /// piece over takes it.
    fn close(&mut self) {
        if std::mem::take(&mut self.open) {
            activity::stop_microphone();
        }
        sink().lock().unwrap_or_else(PoisonError::into_inner).take();
    }
}

impl Ear for AndroidEar {
    fn heard(&self) -> watch::Receiver<Heard> {
        self.heard.clone()
    }

    fn finish(mut self: Box<Self>) -> Result<Said, String> {
        self.close();
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

impl Drop for AndroidEar {
    fn drop(&mut self) {
        self.close();
        IN_USE.store(false, Ordering::SeqCst);
    }
}
