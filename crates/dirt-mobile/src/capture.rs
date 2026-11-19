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
//! [`take_in`] makes each such file a note under its ID and removes the
//! file. It is safe to run twice, at once or after a crash: a note that is
//! there already is left alone, and a file is removed only once its note is
//! in the store. Two things call it:
//!
//! - the app, when it starts and each time the person returns to it, so that
//!   nothing waits for long and the list shows what the sheet kept;
//! - the job the sheet asks for, through [`android`]'s JNI function, in a
//!   process where the app's screen may never have started. It then syncs
//!   once, so the thought reaches the person's other devices without the
//!   app being opened.
//!
//! The sheet and the widget take the theme and the accent the person chose
//! from a small file the app writes ([`write_look`]).
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use dirt_core::models::{Accent, NoteId, ThemeMode};
use dirt_core::Store;

/// The folder of waiting thoughts, under the app's data folder.
pub const WAITING_FOLDER: &str = "waiting";

/// The file the sheet and the widget read the chosen look from, under the
/// app's data folder.
pub const LOOK_FILE: &str = "look";

/// What became of the waiting thoughts.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TakenIn {
    /// Notes made now.
    pub made: usize,
    /// Thoughts taken in already, by this program or the other one: the
    /// note was there, or the file had gone by the time it was read.
    pub already: usize,
    /// Files left where they are: not named by a note's ID, unreadable, or
    /// refused by the store. They are tried again next time.
    pub left: usize,
}

fn waiting_files(data_folder: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(data_folder.join(WAITING_FOLDER)) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        // The sheet writes `<id>.tmp` and renames it, so a `.txt` is whole.
        .filter(|path| path.extension().is_some_and(|kind| kind == "txt"))
        .collect();
    // A UUID v7 sorts by time: the oldest first.
    files.sort();
    files
}

/// Makes a note of every waiting thought. See the module's documentation.
pub async fn take_in(store: &Store, data_folder: &Path) -> TakenIn {
    let mut taken = TakenIn::default();
    for file in waiting_files(data_folder) {
        let Some(id) = file
            .file_stem()
            .and_then(|stem| stem.to_str())
            .and_then(|stem| stem.parse::<NoteId>().ok())
        else {
            taken.left += 1;
            continue;
        };
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
    //! The job's way into the app's Rust: `dev.dirt.capture.Core.takeIn`.
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
    use dirt_core::store::StoreName;
    use dirt_core::Store;
    use jni::objects::{JClass, JObject, JString};
    use jni::sys::jstring;
    use jni::JNIEnv;

    use super::take_in;
    use crate::bootstrap_config::load_bootstrap_config;
    use crate::services::secure_storage::EncryptedPrefs;

    /// Opens what is needed, takes the waiting thoughts in, and syncs once
    /// if the device is signed in and let in. Returns a line for the log:
    /// counts and states, never a note's words.
    fn run(env: &JNIEnv<'_>, context: &JObject<'_>, files_dir: &str) -> String {
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
            let taken = take_in(&store, &data_folder).await;
            said.push(format!(
                "made {}, taken in already {}, left {}",
                taken.made, taken.already, taken.left
            ));
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
            run(&env, &context, &files_dir)
        }))
        .unwrap_or_else(|_| "failed: a panic".to_owned());
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

        let taken = take_in(&store, folder.path()).await;
        assert_eq!((taken.made, taken.already, taken.left), (1, 0, 1));
        let notes = store.list_notes(10, 0).await.unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].content, "buy salt #home");
        assert_eq!(notes[0].id.as_uuid(), id);
        assert_eq!(notes[0].updated_at, i64::try_from(hour_ago).unwrap());
        assert!(!waiting.join(format!("{id}.txt")).exists());

        // The file comes back (a crash before it was removed): no second note.
        std::fs::write(waiting.join(format!("{id}.txt")), "buy salt #home").unwrap();
        let again = take_in(&store, folder.path()).await;
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
        let (by_app, by_job) =
            tokio::join!(take_in(&app, folder.path()), take_in(&job, folder.path()));
        assert_eq!(by_app.made + by_job.made, 60, "{by_app:?} {by_job:?}");
        assert_eq!(by_app.left + by_job.left, 0, "{by_app:?} {by_job:?}");
        assert_eq!(app.note_count().await.unwrap(), 60);
        assert_eq!(std::fs::read_dir(&waiting).unwrap().count(), 0);
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
