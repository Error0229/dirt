//! Speech turned into text on the device that heard it (interface design,
//! 6.18). Nothing here leaves the device, and nothing here reads or writes
//! a note: this crate takes sound and gives back a recording and words.
//!
//! - [`Listening`] takes the sound while a person speaks: it writes the
//!   recording as it goes ([`recording`]), says how loud it is for the
//!   grains on screen, and whether speech has been heard and how long the
//!   quiet after it has lasted ([`hearing`]).
//! - [`reading::words_of`] turns a finished recording into words, later,
//!   in the background: it loads the model, reads the recording in pieces
//!   cut at pauses ([`cut`]), and lets the model go.
//! - [`VoiceFolder`] is where the model and the detector are: shipped with
//!   the app, put together when it is built (`build.rs`).
//! - The microphone ([`microphone`], the desktop's) and a stand-in for it
//!   that plays a WAV file ([`stand_in`]) hand [`Listening`] its sound.
//!
//! The engine is sherpa-onnx, through its prebuilt libraries. On Windows
//! they are loaded from the voice folder when first used, so a program
//! that links this crate must have the linker load `sherpa-onnx-c-api.dll`
//! late: `/DELAYLOAD:sherpa-onnx-c-api.dll`, as `dirt-desktop`'s build
//! script says. Without it the program would not start where the library
//! is not beside it.

pub mod cut;
mod engine;
pub mod folder;
pub mod hearing;
#[cfg(feature = "microphone")]
pub mod microphone;
pub mod reading;
pub mod recording;
pub mod stand_in;

pub use folder::VoiceFolder;
pub use hearing::{Heard, Listening, Recording};

/// Whether this build has the speech engine: without it listening and
/// reading say voice is not in this build, and an app offers no mic.
pub const ENGINE: bool = engine::LINKED;

/// The rate everything here works at: 16,000 samples a second, one
/// channel. The microphone's sound is brought to it first.
pub const RATE: u32 = 16_000;

/// Samples a second, as a `usize`.
pub(crate) const RATE_USIZE: usize = 16_000;

/// The longest a recording may be: listening stops here, keeping what was
/// said (6.18).
pub const MOST_SECONDS: u32 = 600;

/// What can go wrong.
#[derive(Debug, thiserror::Error)]
pub enum VoiceError {
    /// The voice folder is missing something, or is not there.
    #[error("voice is not in this build: {0}")]
    NotInstalled(String),
    /// The engine or a model could not be loaded.
    #[error("the speech engine could not start: {0}")]
    Engine(String),
    /// A recording could not be written or read.
    #[error("the recording could not be read: {0}")]
    Recording(String),
    /// There is no microphone.
    #[error("there is no microphone")]
    NoMicrophone,
    /// The system did not let the app use the microphone.
    #[error("the microphone may not be used: {0}")]
    Refused(String),
    /// The microphone failed.
    #[error("the microphone failed: {0}")]
    Microphone(String),
}

/// What the functions here return.
pub type Result<T> = std::result::Result<T, VoiceError>;
