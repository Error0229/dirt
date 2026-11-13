//! What the desktop app does with a note's file outside the store: a copy
//! for another program to show.
//!
//! A file in the store has its ID for a name and must not be changed. So a
//! file is opened by copying it to a folder of its own under the name the
//! note gives it, and handing that copy to the system. A program that
//! changes what it was given changes the copy.
//!
//! The copies are kept in the store's own folder, beside the files they
//! are copies of, and not in the system's place for temporary files. On a
//! computer with several users that place is shared: a copy there could be
//! read by the others, and the folder could be made by one of them
//! beforehand. Beside the store, a copy is exactly as private as the file
//! it came from, and it goes when the account's folder goes at sign-out.
//! The folder is also closed to everybody but its owner where the system
//! has such a thing.

use std::io;
use std::path::{Path, PathBuf};

use std::sync::Arc;

use dirt_core::files::{clear_copies, media_type_for, safe_file_name, save_copy};
use dirt_core::util::as_sentence;
use dirt_core::{NoteId, Store};
use uuid::Uuid;

/// The name of the folder, inside a store's folder, that the copies are
/// made in.
const OPENED: &str = "opened";

/// The folder for copies of the files of the store whose database is at
/// `store`.
fn opened_folder(store: &Path) -> PathBuf {
    store
        .parent()
        .map_or_else(|| PathBuf::from(OPENED), |folder| folder.join(OPENED))
}

/// Removes the copies made for opening in earlier runs, of every store
/// under `data_folder`. Called when the app starts: a copy is needed for
/// as long as the program showing it runs, which the app cannot know, so
/// they are kept until the next start.
pub fn clear_opened_copies(data_folder: &Path) {
    let Ok(stores) = std::fs::read_dir(data_folder.join("stores")) else {
        return;
    };
    for store in stores.flatten() {
        clear_copies(&store.path().join(OPENED));
    }
}

/// Copies a file out of the store whose database is at `store`, for
/// another program to show, and returns where the copy is. Refused for a
/// file the system would run: see [`dirt_core::files::copy_for_opening`].
pub fn copy_for_opening(
    store: &Path,
    held: &Path,
    file_id: Uuid,
    name: &str,
) -> io::Result<PathBuf> {
    dirt_core::files::copy_for_opening(&opened_folder(store), held, file_id, name)
}

/// Attaches the files at `paths` to a note, one after another, and returns
/// a line for each that could not be attached.
///
/// This is run as a task of its own. Attaching a file is two steps, the
/// copy onto the shelf and the note that names it; a task that belonged to
/// a part of the window would be dropped between them if the person turned
/// to another note, and the files after it would not be attached at all.
pub async fn attach_all(store: Arc<Store>, note: NoteId, paths: Vec<PathBuf>) -> Vec<String> {
    let mut failed = Vec::new();
    for path in paths {
        let given = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        // The name goes into the note as a name, never as a path.
        let name = safe_file_name(&given);
        let attached = store
            .attach_file(&note, &path, &name, media_type_for(&name))
            .await;
        if let Err(error) = attached {
            failed.push(format!("{name}: {}", as_sentence(&error)));
        }
    }
    failed
}

/// Writes the copy a person asked for in a save dialog.
///
/// `was_there` says whether something was at `target` when the dialog
/// closed. If so, the dialog asked about replacing it and the person said
/// yes. If not, nobody agreed to replace anything: a file that has come to
/// be there since, while the file was being fetched, is left alone and the
/// copy is not made.
pub fn save_where_asked(held: &Path, target: &Path, was_there: bool) -> Result<(), String> {
    save_copy(held, target, was_there).map_err(|error| {
        if error.kind() == io::ErrorKind::AlreadyExists {
            format!(
                "A file appeared at {} while this one was being fetched. It was not \
                 replaced. Save again to choose.",
                target.display()
            )
        } else {
            as_sentence(&error)
        }
    })
}

/// Hands a file to the system, to open with whatever shows files of its
/// kind.
pub fn open_with_system(path: &Path) -> io::Result<()> {
    open::that_detached(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The copies are beside the store they came from, so they are as
    /// private as it is and go when its folder goes. Starting the app
    /// clears them for every store, and touches nothing else.
    #[test]
    fn copies_are_kept_with_their_store_and_cleared_at_the_next_start() {
        let data = tempfile::tempdir().unwrap();
        let held = data.path().join("held");
        std::fs::write(&held, b"a photograph").unwrap();
        let mut copies = Vec::new();
        for store in ["local", "account-1"] {
            let folder = data.path().join("stores").join(store);
            std::fs::create_dir_all(folder.join("files")).unwrap();
            std::fs::write(folder.join("files").join("kept"), b"the store's own").unwrap();
            let database = folder.join("notes.db");
            let copy = copy_for_opening(&database, &held, Uuid::from_u128(7), "p.jpg").unwrap();
            assert!(
                copy.starts_with(folder.join("opened")),
                "{}",
                copy.display()
            );
            copies.push(copy);
        }
        // Not in the place every user of the computer shares.
        for copy in &copies {
            assert!(!copy.starts_with(std::env::temp_dir().join("dirt-opened-files")));
        }

        clear_opened_copies(data.path());

        for copy in &copies {
            assert!(!copy.exists());
        }
        for store in ["local", "account-1"] {
            let folder = data.path().join("stores").join(store);
            assert!(!folder.join("opened").exists());
            assert!(folder.join("files").join("kept").exists());
        }
        // A data folder with no stores yet is no failure.
        clear_opened_copies(&data.path().join("nothing here"));
    }

    /// Files are attached one after another, and one that cannot be does
    /// not stop the rest.
    #[tokio::test(flavor = "multi_thread")]
    async fn files_are_attached_in_turn_and_a_failure_is_said() {
        let folder = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(folder.path().join("store").join("notes.db")).unwrap());
        let note = store.create_note("the meeting").await.unwrap().id;
        let first = folder.path().join("first.txt");
        let last = folder.path().join("last.txt");
        std::fs::write(&first, b"one").unwrap();
        std::fs::write(&last, b"three").unwrap();
        let missing = folder.path().join("missing.txt");

        // As the window runs it: a task of its own, which nothing that
        // happens in the window can stop half-way.
        let failed = tokio::spawn(attach_all(
            Arc::clone(&store),
            note,
            vec![first, missing, last],
        ))
        .await
        .unwrap();

        assert_eq!(failed.len(), 1, "{failed:?}");
        assert!(failed[0].starts_with("missing.txt: "), "{failed:?}");
        let names: Vec<String> = store
            .note_attachments(&note)
            .await
            .unwrap()
            .into_iter()
            .map(|attachment| attachment.name)
            .collect();
        assert_eq!(names, ["first.txt", "last.txt"]);
    }

    /// A save dialog asks before replacing a file that is there. For a
    /// place where nothing was, nobody was asked, so nothing that turns
    /// up there meanwhile is replaced.
    #[test]
    fn a_saved_copy_replaces_only_what_the_dialog_asked_about() {
        let folder = tempfile::tempdir().unwrap();
        let held = folder.path().join("held");
        std::fs::write(&held, b"the file from the note").unwrap();

        // Nothing there when asked, and nothing since: written.
        let fresh = folder.path().join("fresh.txt");
        save_where_asked(&held, &fresh, false).unwrap();
        assert_eq!(std::fs::read(&fresh).unwrap(), b"the file from the note");

        // Nothing there when asked; something by the time of writing.
        let appeared = folder.path().join("appeared.txt");
        std::fs::write(&appeared, b"somebody else's").unwrap();
        let said = save_where_asked(&held, &appeared, false).unwrap_err();
        assert!(said.contains("was not replaced"), "{said}");
        assert_eq!(std::fs::read(&appeared).unwrap(), b"somebody else's");

        // There when asked, and the person agreed.
        save_where_asked(&held, &appeared, true).unwrap();
        assert_eq!(std::fs::read(&appeared).unwrap(), b"the file from the note");
    }
}
