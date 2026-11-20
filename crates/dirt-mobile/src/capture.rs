//! Capture from outside the app (interface design 8.5, 8.6).
//!
//! The capture sheet, the widget, the tile and the app shortcut are Kotlin,
//! with no web view (`android/native`). When the person presses ✓ the sheet
//! writes the thought to a file of its own in the app's private storage,
//! named by a UUID v7 it makes on the spot, so that the note is dated when
//! it was written:
//!
//! ```text
//! <files>/dirt/waiting/<note id>.txt
//! ```
//!
//! A thought said in the sheet waits the same way, as a WAV of the sound
//! as it was heard (`<note id>.wav`, interface design 8.9): the sheet has
//! none of the app's Rust. Taking it in makes it what a note keeps, Ogg
//! Opus, with the app's [`Recorder`], and keeps nothing in which Silero
//! hears no speech.
//!
//! [`take_in`] makes each such file a note under its ID and removes the
//! file. It is safe to run twice, at once or after a crash: a note that is
//! there already is left alone, and a file is removed only once its note is
//! in the store. Two things call it:
//!
//! - the app, when it starts and each time the person returns to it, so that
//!   nothing waits for long and the list shows what the sheet kept;
//! - the jobs the sheet asks for, through [`android`]'s JNI functions, in a
//!   process where the app's screen may never have started. The capture
//!   job then syncs once, so the thought reaches the person's other devices
//!   without the app being opened; the words job makes the words of what
//!   was said, and asks for the capture job.
//!
//! The sheet and the widget take the theme and the accent the person chose
//! from a small file the app writes ([`write_look`]).
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use dirt_core::models::{Accent, NoteId, ThemeMode};
use dirt_core::Store;

/// The folder of waiting thoughts, under the app's data folder.
pub const WAITING_FOLDER: &str = "waiting";

/// The file the sheet and the widget read the chosen look from, under the
/// app's data folder.
pub const LOOK_FILE: &str = "look";

/// A recording the sheet left whose process ended before it was finished
/// (`<id>.wav.part`) is taken in once it is this old: the sheet stops
/// listening at ten minutes, so no sheet is still writing it.
const UNFINISHED_AFTER: Duration = Duration::from_mins(11);

/// A recording the sheet kept, as a note keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recorded {
    /// Ogg Opus.
    pub file: Vec<u8>,
    pub duration_ms: u64,
    /// Silero heard speech in it. A recording without is not kept.
    pub speech: bool,
}

/// Makes a WAV the sheet kept into what a note keeps: the app's voice
/// (`voice::recorder`). A build without voice has none, and leaves the
/// recordings where they are; its sheet has no mic.
pub type Recorder = Arc<dyn Fn(&[u8]) -> Result<Recorded, String> + Send + Sync>;

/// What became of the waiting thoughts.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TakenIn {
    /// Notes made now.
    pub made: usize,
    /// Of them, recordings: their words are to be made.
    pub said: usize,
    /// Recordings in which no speech was heard: removed, and nothing made.
    pub unheard: usize,
    /// Thoughts taken in already, by this program or the other one: the
    /// note was there, or the file had gone by the time it was read.
    pub already: usize,
    /// Files left where they are: not named by a note's ID, unreadable, or
    /// refused by the store. They are tried again next time.
    pub left: usize,
}

/// What a waiting file holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Typed,
    Said,
}

fn waiting_files(data_folder: &Path) -> Vec<(PathBuf, Kind)> {
    let Ok(entries) = std::fs::read_dir(data_folder.join(WAITING_FOLDER)) else {
        return Vec::new();
    };
    let unfinished_since = SystemTime::now().checked_sub(UNFINISHED_AFTER);
    let mut files: Vec<(PathBuf, Kind)> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            // The sheet writes `<id>.tmp` and `<id>.wav.part`, and renames
            // them, so a `.txt` and a `.wav` are whole.
            let ending = |path: &Path, wanted: &str| {
                path.extension()
                    .is_some_and(|found| found.eq_ignore_ascii_case(wanted))
            };
            let kind = if ending(&path, "txt") {
                Kind::Typed
            } else if ending(&path, "wav") {
                Kind::Said
            } else if ending(&path, "part") && ending(Path::new(path.file_stem()?), "wav") {
                // Its process ended while it listened: what was said is
                // kept all the same, once no sheet can be writing it.
                let modified = entry.metadata().ok()?.modified().ok()?;
                if unfinished_since.is_none_or(|since| modified > since) {
                    return None;
                }
                Kind::Said
            } else {
                return None;
            };
            Some((path, kind))
        })
        .collect();
    // A UUID v7 sorts by time: the oldest first.
    files.sort_by(|(one, _), (other, _)| one.cmp(other));
    files
}

/// The note's ID a waiting file is named by: what comes before its first
/// dot.
fn id_of(file: &Path) -> Option<NoteId> {
    let name = file.file_name()?.to_str()?;
    name.split('.').next()?.parse::<NoteId>().ok()
}

/// The samples of a WAV of 16 kHz, one channel, 16 bits, as the sheet
/// writes it (`WavWriter` in `android/native`). One whose process ended
/// before it was finished says it holds nothing; what follows its header
/// is read all the same.
#[must_use]
pub fn samples_of_wav(wav: &[u8]) -> Option<Vec<f32>> {
    if wav.len() < 12 || &wav[..4] != b"RIFF" || &wav[8..12] != b"WAVE" {
        return None;
    }
    let mut at = 12;
    while at + 8 <= wav.len() {
        let id = &wav[at..at + 4];
        let size =
            usize::try_from(u32::from_le_bytes(wav[at + 4..at + 8].try_into().ok()?)).ok()?;
        let body = at + 8;
        if id == b"fmt " {
            let format = wav.get(body..body + 16)?;
            let channels = u16::from_le_bytes([format[2], format[3]]);
            let rate = u32::from_le_bytes([format[4], format[5], format[6], format[7]]);
            let bits = u16::from_le_bytes([format[14], format[15]]);
            if channels != 1 || rate != 16_000 || bits != 16 {
                return None;
            }
        }
        if id == b"data" {
            let end = if size == 0 {
                wav.len()
            } else {
                body.saturating_add(size).min(wav.len())
            };
            return Some(
                wav.get(body..end)?
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|pair| f32::from(i16::from_le_bytes(*pair)) / 32768.0)
                    .collect(),
            );
        }
        at = body.saturating_add(size).saturating_add(size & 1);
    }
    None
}

/// Makes a note of every waiting thought, and of every recording with
/// `recorder`. See the module's documentation.
pub async fn take_in(store: &Store, data_folder: &Path, recorder: Option<&Recorder>) -> TakenIn {
    let mut taken = TakenIn::default();
    for (file, kind) in waiting_files(data_folder) {
        let Some(id) = id_of(&file) else {
            taken.left += 1;
            continue;
        };
        if kind == Kind::Said {
            take_in_said(store, &file, id, recorder, &mut taken).await;
            continue;
        }
        let text = match std::fs::read_to_string(&file) {
            Ok(text) => text,
            // The other one took it in between the listing and now.
            Err(error) if error.kind() == ErrorKind::NotFound => {
                taken.already += 1;
                continue;
            }
            Err(_) => {
                taken.left += 1;
                continue;
            }
        };
        let text = text.trim();
        if text.is_empty() {
            let _ = std::fs::remove_file(&file);
            continue;
        }
        match store.take_in_note(id, text).await {
            Ok(Some(_)) => taken.made += 1,
            Ok(None) => taken.already += 1,
            Err(_) => {
                taken.left += 1;
                continue;
            }
        }
        let _ = std::fs::remove_file(&file);
    }
    taken
}

/// Makes a note of one recording the sheet kept.
async fn take_in_said(
    store: &Store,
    file: &Path,
    id: NoteId,
    recorder: Option<&Recorder>,
    taken: &mut TakenIn,
) {
    let Some(recorder) = recorder.cloned() else {
        taken.left += 1;
        return;
    };
    let wav = match std::fs::read(file) {
        Ok(wav) => wav,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            taken.already += 1;
            return;
        }
        Err(_) => {
            taken.left += 1;
            return;
        }
    };
    // Writing Ogg Opus, and listening for speech, hold the processor.
    let recorded = tokio::task::spawn_blocking(move || recorder(&wav)).await;
    let recorded = match recorded {
        Ok(Ok(recorded)) => recorded,
        Ok(Err(error)) => {
            tracing::warn!("A recording from the sheet could not be read: {error}");
            taken.left += 1;
            return;
        }
        Err(_) => {
            taken.left += 1;
            return;
        }
    };
    if recorded.speech {
        match store
            .take_in_voice_note(id, recorded.file, recorded.duration_ms)
            .await
        {
            Ok(Some(_)) => {
                taken.made += 1;
                taken.said += 1;
            }
            Ok(None) => taken.already += 1,
            Err(_) => {
                taken.left += 1;
                return;
            }
        }
    } else {
        taken.unheard += 1;
    }
    let _ = std::fs::remove_file(file);
}

/// Writes the theme and the accent the person chose where the sheet and the
/// widget read them: one line, "<theme> <accent>", such as "dark moss".
/// It says nothing about any note.
pub fn write_look(data_folder: &Path, theme: ThemeMode, accent: Accent) -> std::io::Result<()> {
    let theme = match theme {
        ThemeMode::System => "auto",
        ThemeMode::Light => "light",
        ThemeMode::Dark => "dark",
    };
    let line = format!("{theme} {}\n", accent.name());
    let path = data_folder.join(LOOK_FILE);
    if std::fs::read_to_string(&path).is_ok_and(|now| now == line) {
        return Ok(());
    }
    std::fs::create_dir_all(data_folder)?;
    std::fs::write(path, line)
}

#[cfg(target_os = "android")]
pub mod android {
    //! The jobs' way into the app's Rust: `dev.dirt.capture.Core.takeIn`,
    //! and `makeWords`.
    //!
    //! It runs where the app's screen may never have started, so nothing
    //! here uses what the screen sets up: not `ndk_context`, which the
    //! activity sets as it starts and clears as it goes (the secrets are
    //! opened with the `Context` Kotlin hands over), and not the data
    //! folder worked out from the process (Kotlin gives its files folder).
    #![allow(unsafe_code)]

    use std::path::Path;

    use dirt_core::config::ServerAddress;
    use dirt_core::secrets::slot_name;
    use dirt_core::session::Session;
    use dirt_core::store::StoreName;
    use dirt_core::Store;
    use dirt_voice::VoiceFolder;
    use jni::objects::{JClass, JObject, JString};
    use jni::sys::jstring;
    use jni::JNIEnv;

    use super::take_in;
    use crate::bootstrap_config::load_bootstrap_config;
    use crate::services::secure_storage::EncryptedPrefs;

    /// Opens what is needed and takes the waiting thoughts in. Then, for
    /// the capture job, syncs once if the device is signed in and let in;
    /// for the words job (`voice_dir` given), makes the words of what was
    /// said on this phone, and does not sync. Returns a line for the log:
    /// counts and states, never a note's words.
    #[allow(clippy::too_many_lines)]
    fn run(
        env: &JNIEnv<'_>,
        context: &JObject<'_>,
        files_dir: &str,
        voice_dir: Option<&str>,
    ) -> String {
        let data_folder = Path::new(files_dir).join("dirt");
        let server =
            match ServerAddress::on_android(load_bootstrap_config().dirt_api_base_url.as_deref()) {
                Ok(server) => server.map(|address| address.url),
                Err(error) => return format!("no server: {error}"),
            };
        let vm = match env.get_java_vm() {
            Ok(vm) => vm,
            Err(error) => return format!("no Java VM: {error}"),
        };
        // One thread is enough: the store has threads of its own, and one
        // sync is one request after another.
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => return format!("no runtime: {error}"),
        };
        runtime.block_on(async {
            let mut said = Vec::new();
            let session = match server.as_deref().map(|url| {
                EncryptedPrefs::open_with(vm, context, &slot_name(url))
                    .map_err(|error| error.to_string())
                    .and_then(|secrets| {
                        crate::app_shell::session_with(url, secrets, data_folder.clone(), false)
                    })
            }) {
                Some(Ok(session)) => Some(session),
                // Which store the device writes in is the session's to say:
                // signed in, the account's. Without it, a thought taken into
                // the local store would be where the app, signed in, never
                // shows it. The files wait for the app, which says what is
                // wrong.
                Some(Err(error)) => return format!("no session, so nothing taken in: {error}"),
                None => None,
            };
            // Signed out the session's store is the local one; and a build
            // with no server has only that.
            let store = session.as_ref().map_or_else(
                || Store::open_in(&data_folder, &StoreName::Local),
                |session| Ok(session.store()),
            );
            let store = match store {
                Ok(store) => store,
                Err(error) => return format!("no store: {error}"),
            };
            let recorder = voice_dir
                .filter(|dir| !dir.is_empty())
                .and_then(|dir| VoiceFolder::at(dir).ok())
                .map(crate::voice::recorder);
            let taken = take_in(&store, &data_folder, recorder.as_ref()).await;
            said.push(format!(
                "made {} ({} said), nothing heard {}, taken in already {}, left {}",
                taken.made, taken.said, taken.unheard, taken.already, taken.left
            ));
            if let Some(dir) = voice_dir {
                // What Settings → Voice says to keep is the device's, in
                // its own store, whoever is signed in.
                let device = session
                    .as_ref()
                    .map_or_else(|| store.clone(), Session::device_store);
                let kept = device
                    .load_settings()
                    .await
                    .map(|settings| settings.voice_recording)
                    .unwrap_or_default();
                match crate::voice::make_words_with(&store, dir, kept).await {
                    Ok(made) => said.push(format!(
                        "words put in {}, not made {}",
                        made.put.len(),
                        made.failed.len()
                    )),
                    Err(error) => said.push(format!("no words: {error}")),
                }
                return said.join("; ");
            }
            if let Some(session) = &session {
                if session.state().is_ready() {
                    match session.sync_now().await {
                        Ok(_) => said.push("synced".to_owned()),
                        Err(error) => said.push(format!("not synced: {error}")),
                    }
                } else {
                    said.push(format!("not synced: {:?}", session.state()));
                }
            }
            said.join("; ")
        })
    }

    /// `dev.dirt.capture.Core.takeIn(context, filesDir)`.
    ///
    /// Exported by name for JNI: `dx` links the app with `--export-dynamic`,
    /// which puts a `#[no_mangle]` function of this binary crate into the
    /// library's table of symbols.
    #[no_mangle]
    pub extern "system" fn Java_dev_dirt_capture_Core_takeIn<'local>(
        mut env: JNIEnv<'local>,
        _class: JClass<'local>,
        context: JObject<'local>,
        files_dir: JString<'local>,
    ) -> jstring {
        // A panic must not cross into Java: it would end the process.
        let said = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let files_dir: String = match env.get_string(&files_dir) {
                Ok(text) => text.into(),
                Err(error) => return format!("no files folder: {error}"),
            };
            run(&env, &context, &files_dir, None)
        }))
        .unwrap_or_else(|_| "failed: a panic".to_owned());
        answer(&env, &said)
    }

    /// `dev.dirt.capture.Core.makeWords(context, filesDir, voiceDir)`.
    #[no_mangle]
    pub extern "system" fn Java_dev_dirt_capture_Core_makeWords<'local>(
        mut env: JNIEnv<'local>,
        _class: JClass<'local>,
        context: JObject<'local>,
        files_dir: JString<'local>,
        voice_dir: JString<'local>,
    ) -> jstring {
        let said = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let files_dir: String = match env.get_string(&files_dir) {
                Ok(text) => text.into(),
                Err(error) => return format!("no files folder: {error}"),
            };
            let voice_dir: String = match env.get_string(&voice_dir) {
                Ok(text) => text.into(),
                Err(error) => return format!("no voice folder: {error}"),
            };
            run(&env, &context, &files_dir, Some(&voice_dir))
        }))
        .unwrap_or_else(|_| "failed: a panic".to_owned());
        answer(&env, &said)
    }

    /// Hands a line back to Java, clearing any exception a call left.
    fn answer(env: &JNIEnv<'_>, said: &str) -> jstring {
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_describe();
            let _ = env.exception_clear();
        }
        env.new_string(said)
            .map_or(std::ptr::null_mut(), JString::into_raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id_at(ms: u64) -> uuid::Uuid {
        uuid::Uuid::new_v7(uuid::Timestamp::from_unix(
            uuid::NoContext,
            ms / 1000,
            u32::try_from(ms % 1000).unwrap() * 1_000_000,
        ))
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_waiting_thought_becomes_one_note_dated_when_it_was_written() {
        let folder = tempfile::tempdir().unwrap();
        let store = Store::open(folder.path().join("notes.db")).unwrap();
        let waiting = folder.path().join(WAITING_FOLDER);
        std::fs::create_dir_all(&waiting).unwrap();
        let hour_ago = u64::try_from(chrono_now_ms() - 3_600_000).unwrap();
        let id = id_at(hour_ago);
        std::fs::write(waiting.join(format!("{id}.txt")), "  buy salt #home \n").unwrap();
        std::fs::write(
            waiting.join(format!("{}.tmp", id_at(hour_ago + 5))),
            "half written",
        )
        .unwrap();
        std::fs::write(waiting.join("not-an-id.txt"), "x").unwrap();

        let taken = take_in(&store, folder.path(), None).await;
        assert_eq!((taken.made, taken.already, taken.left), (1, 0, 1));
        let notes = store.list_notes(10, 0).await.unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].content, "buy salt #home");
        assert_eq!(notes[0].id.as_uuid(), id);
        assert_eq!(notes[0].updated_at, i64::try_from(hour_ago).unwrap());
        assert!(!waiting.join(format!("{id}.txt")).exists());

        // The file comes back (a crash before it was removed): no second note.
        std::fs::write(waiting.join(format!("{id}.txt")), "buy salt #home").unwrap();
        let again = take_in(&store, folder.path(), None).await;
        assert_eq!((again.made, again.already), (0, 1));
        assert_eq!(store.list_notes(10, 0).await.unwrap().len(), 1);
    }

    /// The app and the job take thoughts in at the same moment, from two
    /// stores open on the same file: every thought becomes one note, none
    /// twice, and every file goes.
    #[tokio::test(flavor = "multi_thread")]
    async fn two_taking_in_at_once_make_each_thought_once() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("notes.db");
        let app = Store::open(&path).unwrap();
        let job = Store::open(&path).unwrap();
        let waiting = folder.path().join(WAITING_FOLDER);
        std::fs::create_dir_all(&waiting).unwrap();
        let start = u64::try_from(chrono_now_ms()).unwrap() - 60_000;
        for n in 0..60_u64 {
            std::fs::write(
                waiting.join(format!("{}.txt", id_at(start + n))),
                format!("thought {n}"),
            )
            .unwrap();
        }
        let (by_app, by_job) = tokio::join!(
            take_in(&app, folder.path(), None),
            take_in(&job, folder.path(), None)
        );
        assert_eq!(by_app.made + by_job.made, 60, "{by_app:?} {by_job:?}");
        assert_eq!(by_app.left + by_job.left, 0, "{by_app:?} {by_job:?}");
        assert_eq!(app.note_count().await.unwrap(), 60);
        assert_eq!(std::fs::read_dir(&waiting).unwrap().count(), 0);
    }

    /// A recorder standing in for the app's voice: speech is any sound
    /// that is not silence, and the file is the samples' count.
    fn stand_in_recorder() -> Recorder {
        Arc::new(|wav: &[u8]| {
            let samples = samples_of_wav(wav).ok_or("not a WAV")?;
            Ok(Recorded {
                file: format!("{} samples", samples.len()).into_bytes(),
                duration_ms: samples.len() as u64 / 16,
                speech: samples.iter().any(|sample| *sample != 0.0),
            })
        })
    }

    fn wav(samples: &[i16], finished: bool) -> Vec<u8> {
        let bytes = u32::try_from(samples.len() * 2).unwrap();
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(if finished { 36 + bytes } else { 0 }).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16_u32.to_le_bytes());
        wav.extend_from_slice(&[1, 0, 1, 0]);
        wav.extend_from_slice(&16_000_u32.to_le_bytes());
        wav.extend_from_slice(&32_000_u32.to_le_bytes());
        wav.extend_from_slice(&[2, 0, 16, 0]);
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(if finished { bytes } else { 0 }).to_le_bytes());
        for sample in samples {
            wav.extend_from_slice(&sample.to_le_bytes());
        }
        wav
    }

    #[test]
    fn a_wav_the_sheet_writes_is_read_even_unfinished() {
        assert_eq!(
            samples_of_wav(&wav(&[16384, -32768], true)),
            Some(vec![0.5, -1.0])
        );
        assert_eq!(
            samples_of_wav(&wav(&[16384, -32768], false)),
            Some(vec![0.5, -1.0])
        );
        assert_eq!(samples_of_wav(b"not a wav"), None);
    }

    /// A recording the sheet kept becomes a voice note dated when it was
    /// said, once; one with nothing heard in it makes nothing; one still
    /// being written is left; and a build with no voice leaves them all.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_recording_from_the_sheet_becomes_one_voice_note() {
        let folder = tempfile::tempdir().unwrap();
        let store = Store::open(folder.path().join("notes.db")).unwrap();
        let waiting = folder.path().join(WAITING_FOLDER);
        std::fs::create_dir_all(&waiting).unwrap();
        let minute_ago = u64::try_from(chrono_now_ms() - 60_000).unwrap();
        let said = id_at(minute_ago);
        let quiet = id_at(minute_ago + 1);
        let writing = id_at(minute_ago + 2);
        std::fs::write(waiting.join(format!("{said}.wav")), wav(&[900; 1600], true)).unwrap();
        std::fs::write(waiting.join(format!("{quiet}.wav")), wav(&[0; 1600], true)).unwrap();
        std::fs::write(
            waiting.join(format!("{writing}.wav.part")),
            wav(&[900; 16], false),
        )
        .unwrap();

        let without = take_in(&store, folder.path(), None).await;
        assert_eq!((without.made, without.left), (0, 2));

        let taken = take_in(&store, folder.path(), Some(&stand_in_recorder())).await;
        assert_eq!(
            (taken.made, taken.said, taken.unheard, taken.left),
            (1, 1, 1, 0)
        );
        let notes = store.list_notes(10, 0).await.unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].id.as_uuid(), said);
        assert_eq!(notes[0].updated_at, i64::try_from(minute_ago).unwrap());
        assert_eq!(notes[0].voice.map(|voice| voice.duration_ms), Some(100));
        assert!(!waiting.join(format!("{said}.wav")).exists());
        assert!(!waiting.join(format!("{quiet}.wav")).exists());
        assert!(waiting.join(format!("{writing}.wav.part")).exists());

        std::fs::write(waiting.join(format!("{said}.wav")), wav(&[900; 1600], true)).unwrap();
        let again = take_in(&store, folder.path(), Some(&stand_in_recorder())).await;
        assert_eq!((again.made, again.already), (0, 1));
        assert_eq!(store.list_notes(10, 0).await.unwrap().len(), 1);
    }

    #[test]
    fn the_look_is_one_line_and_written_only_when_it_changes() {
        let folder = tempfile::tempdir().unwrap();
        write_look(folder.path(), ThemeMode::Dark, Accent::Moss).unwrap();
        let path = folder.path().join(LOOK_FILE);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "dark moss\n");
        let written = std::fs::metadata(&path).unwrap().modified().unwrap();
        write_look(folder.path(), ThemeMode::Dark, Accent::Moss).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            written
        );
        write_look(folder.path(), ThemeMode::System, Accent::Clay).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "auto clay\n");
    }

    fn chrono_now_ms() -> i64 {
        i64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis(),
        )
        .unwrap()
    }
}
