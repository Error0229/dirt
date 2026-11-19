//! Making the words of the thoughts said on this device, in the background
//! (interface design 6.18).
//!
//! A recording is kept as a note the moment the person lets go
//! ([`crate::Store::keep_voice_note`]); its words come afterwards. This is
//! the part that comes afterwards: for each recording this device made that
//! waits, it reads the file from the shelf, hands it to whatever turns
//! sound into words, and puts the words at the start of the note.
//!
//! What turns sound into words is the app's to give, as a [`Transcriber`]:
//! the engine is in `dirt-voice`, which this crate does not depend on, so
//! that the CLI and the server's tests never build it. The work is done on
//! a thread of its own, since reading takes a second or more and holds the
//! processor; the store's threads are not held meanwhile.
//!
//! It runs when something happens, never on a timer: as the app starts
//! (words left to make when it last closed are made then), and after a
//! thought is said. Words that cannot be made leave the note as it was
//! kept, a recording named by its length, and they are tried again at the
//! next of those.

use std::sync::Arc;

use crate::models::{NoteId, VoiceRecording};
use crate::store::{Store, WordsPut};
use crate::Result;

/// Turns a recording into words.
pub trait Transcriber: Send + Sync + 'static {
    /// The words of a recording: an Ogg Opus file as it was kept. Empty
    /// when none were found. An error is told in words, and the recording
    /// waits for the next try.
    fn words(&self, recording: &[u8]) -> std::result::Result<String, String>;
}

/// What one round of making words did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MadeWords {
    /// Notes whose words were put in, or found to be none.
    pub put: Vec<WordsPut>,
    /// Recordings whose words could not be made, with why.
    pub failed: Vec<(NoteId, String)>,
}

impl MadeWords {
    /// Some note changed, and whatever shows the notes reads them again.
    #[must_use]
    pub fn changed(&self) -> bool {
        self.put.iter().any(|put| !matches!(put, WordsPut::Nothing))
    }
}

/// Makes the words of every recording made on this device that waits for
/// them, one at a time, the oldest first. Each is read once in a round: one
/// that fails waits for the next round.
///
/// `kept` is what Settings → Voice says to keep. It is this device's
/// setting, which an app keeps in its device's store, not in the store of
/// the notes.
pub async fn make_words(
    store: &Store,
    transcriber: Arc<dyn Transcriber>,
    kept: VoiceRecording,
) -> Result<MadeWords> {
    let text_only = kept == VoiceRecording::TextOnly;
    let mut made = MadeWords::default();
    for waiting in store.voice_waiting().await? {
        let note = waiting.note;
        let Some(recording) = store.file_bytes(waiting.file_id).await? else {
            let why = "the recording is not on this device".to_owned();
            store.voice_not_read(&note, &why).await?;
            made.failed.push((note, why));
            continue;
        };
        match read_aside(Arc::clone(&transcriber), recording).await {
            Ok(words) => {
                made.put
                    .push(store.put_voice_words(&note, &words, text_only).await?);
            }
            Err(why) => {
                tracing::warn!("The words of note {note} could not be made: {why}");
                store.voice_not_read(&note, &why).await?;
                made.failed.push((note, why));
            }
        }
    }
    Ok(made)
}

/// Reads a recording on a thread of its own, and waits for it there
/// without holding anything else.
async fn read_aside(
    transcriber: Arc<dyn Transcriber>,
    recording: Vec<u8>,
) -> std::result::Result<String, String> {
    let (answer, wait) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name("dirt-voice-words".to_owned())
        .spawn(move || {
            let _ = answer.send(transcriber.words(&recording));
        })
        .map_err(|error| error.to_string())?;
    wait.await
        .unwrap_or_else(|_| Err("the reading stopped before it ended".to_owned()))
}
