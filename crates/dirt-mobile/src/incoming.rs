//! Files that come into the app from outside it: ones the person chose to
//! attach on Android's screen for that, and ones another app shared.
//!
//! The activity copies each into the app's cache before this code hears of
//! it, and says what the file is called, what kind it is and where the
//! copy is, or why there is none. Attaching puts the file on the note
//! store's shelf, after which the copy in the cache is of no use and is
//! removed.
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

use std::path::PathBuf;

use dirt_core::api::limits;
use dirt_core::files::{media_type_for, safe_file_name};
use dirt_core::models::NoteId;
use dirt_core::sync::overview::size_words;
use dirt_core::util::as_sentence;
use dirt_core::Store;

/// What the activity says of a file too large to be worth copying in.
const TOO_LARGE: &str = "too_large";

/// One file that came in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Incoming {
    /// What the app that had the file calls it. It goes into a note as a
    /// name and is never used as a path.
    pub name: String,
    /// What kind of file the other app says it is, or nothing.
    pub media_type: String,
    /// Where the copy is, or why there is none.
    pub copy: Result<PathBuf, String>,
}

/// What was asked of the app from outside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Capture {
    /// Another app shared this: text, files, or both. It becomes a note.
    Shared { text: String, files: Vec<Incoming> },
    /// The home-screen widget was pressed: a blank note to write in.
    Blank,
}

/// The files the person chose to attach.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Picked {
    /// The person backed out without choosing.
    Cancelled,
    Files(Vec<Incoming>),
}

fn files_in(list: &serde_json::Value) -> Vec<Incoming> {
    let text = |entry: &serde_json::Value, field: &str| {
        entry
            .get(field)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    list.as_array()
        .map(|entries| {
            entries
                .iter()
                .map(|entry| {
                    let path = text(entry, "path");
                    Incoming {
                        name: text(entry, "name"),
                        media_type: text(entry, "mime"),
                        copy: if path.is_empty() {
                            Err(text(entry, "problem"))
                        } else {
                            Ok(PathBuf::from(path))
                        },
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Reads what `MainActivity.takeCapture` gave. `None` for something that
/// cannot be read, which is passed over.
pub fn parse_capture(given: &str) -> Option<Capture> {
    let value: serde_json::Value = serde_json::from_str(given).ok()?;
    let text = value
        .get("text")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_owned();
    let files = value.get("files").map(files_in).unwrap_or_default();
    Some(if text.is_empty() && files.is_empty() {
        Capture::Blank
    } else {
        Capture::Shared { text, files }
    })
}

/// Reads what `MainActivity.takePicked` gave.
pub fn parse_picked(given: &str) -> Picked {
    match serde_json::from_str::<serde_json::Value>(given) {
        Ok(list) if list.is_array() => Picked::Files(files_in(&list)),
        _ => Picked::Cancelled,
    }
}

/// The text of a note made from what another app shared: the text that
/// came, or, for files alone, what the files are called. A note has to
/// have some text to be found by.
pub fn note_text(text: &str, files: &[Incoming]) -> String {
    let text = text.trim();
    if !text.is_empty() {
        return text.to_owned();
    }
    files
        .iter()
        .map(|file| safe_file_name(&file.name))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A line for each file the activity could not copy into the app: too
/// large, or not to be read.
pub fn not_copied_in(files: &[Incoming]) -> Vec<String> {
    files
        .iter()
        .filter_map(|file| {
            let problem = file.copy.as_ref().err()?;
            let name = safe_file_name(&file.name);
            Some(if problem == TOO_LARGE {
                let most = u64::try_from(limits::MAX_FILE_BYTES).unwrap_or_default();
                format!(
                    "{name} is too large to attach. A file may be {} at most.",
                    size_words(most)
                )
            } else {
                format!("{name} could not be read: {}", as_sentence(problem))
            })
        })
        .collect()
}

/// Whether what another app shared has anything to make a note of: some
/// text, or a file that came in whole. A share of files alone, none of
/// which could be read, makes no note: it would be a note called after a
/// file it does not have.
pub fn worth_a_note(text: &str, files: &[Incoming]) -> bool {
    !text.trim().is_empty() || files.iter().any(|file| file.copy.is_ok())
}

/// Attaches files that came in to a note, one after another, and returns a
/// line for each that could not be attached. Each copy in the cache is
/// removed, attached or not.
///
/// Run as a task of its own: turning away from the note must not drop
/// files the person has already chosen.
pub async fn attach_incoming(store: Store, note: NoteId, files: Vec<Incoming>) -> Vec<String> {
    let mut failed = not_copied_in(&files);
    for file in files {
        let Ok(copy) = file.copy else {
            continue;
        };
        // The name goes into the note as a name, never as a path.
        let name = safe_file_name(&file.name);
        let media_type = if file.media_type.trim().is_empty() {
            media_type_for(&name).to_owned()
        } else {
            file.media_type.trim().to_owned()
        };
        if let Err(error) = store.attach_file(&note, &copy, &name, &media_type).await {
            failed.push(format!("{name}: {}", as_sentence(&error)));
        }
        discard(&copy);
    }
    failed
}

/// Removes a copy in the cache, and the folder it had to itself.
fn discard(copy: &std::path::Path) {
    let _ = std::fs::remove_file(copy);
    if let Some(folder) = copy.parent() {
        // Only if it is empty: it held this one file.
        let _ = std::fs::remove_dir(folder);
    }
}

/// Removes the copies of files that came in and were not attached: the
/// person shared something while the notes could not be opened.
pub fn discard_all(files: &[Incoming]) {
    for file in files {
        if let Ok(copy) = &file.copy {
            discard(copy);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_the_activity_hands_over_is_read() {
        assert_eq!(
            parse_capture(r#"{"text":"","files":[]}"#),
            Some(Capture::Blank)
        );
        assert_eq!(
            parse_capture(r#"{"text":"  a thought \n","files":[]}"#),
            Some(Capture::Shared {
                text: "a thought".to_owned(),
                files: Vec::new()
            })
        );
        let shared = parse_capture(
            r#"{"text":"","files":[
                {"name":"IMG_1.jpg","mime":"image/jpeg","path":"/cache/incoming/1/file"},
                {"name":"film.mp4","mime":"video/mp4","problem":"too_large"}]}"#,
        );
        assert_eq!(
            shared,
            Some(Capture::Shared {
                text: String::new(),
                files: vec![
                    Incoming {
                        name: "IMG_1.jpg".to_owned(),
                        media_type: "image/jpeg".to_owned(),
                        copy: Ok(PathBuf::from("/cache/incoming/1/file")),
                    },
                    Incoming {
                        name: "film.mp4".to_owned(),
                        media_type: "video/mp4".to_owned(),
                        copy: Err("too_large".to_owned()),
                    },
                ],
            })
        );
        assert_eq!(parse_capture("not json"), None);

        assert_eq!(parse_picked("cancelled"), Picked::Cancelled);
        assert_eq!(parse_picked("[]"), Picked::Files(Vec::new()));
        let Picked::Files(files) =
            parse_picked(r#"[{"name":"a.txt","mime":"","path":"/cache/incoming/2/file"}]"#)
        else {
            panic!("a list of files");
        };
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].media_type, "");
    }

    #[test]
    fn a_share_with_nothing_readable_makes_no_note() {
        let lost = Incoming {
            name: "22".to_owned(),
            media_type: "image/png".to_owned(),
            copy: Err("no access".to_owned()),
        };
        let huge = Incoming {
            name: "film.mp4".to_owned(),
            media_type: "video/mp4".to_owned(),
            copy: Err("too_large".to_owned()),
        };
        let whole = Incoming {
            name: "a.jpg".to_owned(),
            media_type: String::new(),
            copy: Ok(PathBuf::from("/cache/incoming/1/file")),
        };
        assert!(!worth_a_note("", &[lost.clone(), huge.clone()]));
        assert!(!worth_a_note("  ", &[]));
        // Some text, or one file that came in, is worth a note.
        assert!(worth_a_note("see this", std::slice::from_ref(&lost)));
        assert!(worth_a_note("", &[lost.clone(), whole]));
        // What did not come in is said either way.
        assert_eq!(
            not_copied_in(&[lost, huge]),
            [
                "22 could not be read: No access.",
                "film.mp4 is too large to attach. A file may be 10.0 MiB at most.",
            ]
        );
    }

    #[test]
    fn a_note_made_from_shared_files_alone_is_called_after_them() {
        let file = |name: &str| Incoming {
            name: name.to_owned(),
            media_type: String::new(),
            copy: Err(String::new()),
        };
        assert_eq!(
            note_text(" the whiteboard ", &[file("a.jpg")]),
            "the whiteboard"
        );
        assert_eq!(
            note_text("", &[file("a.jpg"), file("b.jpg")]),
            "a.jpg\nb.jpg"
        );
        // A name is a name, whatever the other app sent.
        assert_eq!(note_text("", &[file("../../etc/passwd")]), "passwd");
        assert_eq!(note_text("", &[]), "");
    }

    /// Files are attached in turn. One that could not be copied in is
    /// said, and does not stop the others. No copy is left in the cache.
    #[tokio::test(flavor = "multi_thread")]
    async fn incoming_files_are_attached_and_their_copies_removed() {
        let folder = tempfile::tempdir().unwrap();
        let store = Store::open(folder.path().join("store").join("notes.db")).unwrap();
        let note = store.create_note("the meeting").await.unwrap().id;
        let incoming = |number: u32, name: &str, mime: &str, contents: &[u8]| {
            let own = folder.path().join("incoming").join(number.to_string());
            std::fs::create_dir_all(&own).unwrap();
            let copy = own.join("file");
            std::fs::write(&copy, contents).unwrap();
            Incoming {
                name: name.to_owned(),
                media_type: mime.to_owned(),
                copy: Ok(copy),
            }
        };
        let files = vec![
            incoming(1, "minutes.txt", "", b"who said what"),
            Incoming {
                name: "film.mp4".to_owned(),
                media_type: "video/mp4".to_owned(),
                copy: Err("too_large".to_owned()),
            },
            Incoming {
                name: "gone.pdf".to_owned(),
                media_type: "application/pdf".to_owned(),
                copy: Err("the file could not be opened".to_owned()),
            },
            incoming(
                2,
                "../../photo of it.heic",
                "image/heic",
                b"not really a photograph",
            ),
        ];

        let failed = tokio::spawn(attach_incoming(store.clone(), note, files))
            .await
            .unwrap();

        assert_eq!(
            failed,
            [
                "film.mp4 is too large to attach. A file may be 10.0 MiB at most.",
                "gone.pdf could not be read: The file could not be opened.",
            ]
        );
        let attached: Vec<(String, String)> = store
            .note_attachments(&note)
            .await
            .unwrap()
            .into_iter()
            .map(|attachment| (attachment.name, attachment.media_type))
            .collect();
        assert_eq!(
            attached,
            [
                // The kind is taken from the name when the other app
                // did not say.
                ("minutes.txt".to_owned(), "text/plain".to_owned()),
                ("photo of it.heic".to_owned(), "image/heic".to_owned()),
            ]
        );
        // Nothing is left where the files came in.
        let left: Vec<_> = std::fs::read_dir(folder.path().join("incoming"))
            .unwrap()
            .collect();
        assert!(left.is_empty(), "{left:?}");
    }

    /// The activity copies a file into the app before this code can say
    /// whether it is too large, so it has to know the limit itself. Its
    /// number is held to the one the server enforces.
    #[test]
    fn the_activity_knows_how_large_a_file_may_be() {
        let activity = include_str!("../android/MainActivity.kt");
        let line = format!(
            "private const val MOST_FILE_BYTES = {}L",
            limits::MAX_FILE_BYTES
        );
        assert!(
            activity.contains(&line),
            "MainActivity.kt should hold: {line}"
        );
    }
}
