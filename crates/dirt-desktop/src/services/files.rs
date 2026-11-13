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

use dirt_core::files::{runs_as_a_program, safe_file_name, save_copy};
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
        clear(&store.path().join(OPENED));
    }
}

/// Makes `folder` if it is not there, for its owner alone.
fn make_private(folder: &Path) -> io::Result<()> {
    std::fs::create_dir_all(folder)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Set whether or not it was just made: one made before, by this
        // program or anything else, is closed too.
        std::fs::set_permissions(folder, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn clear(folder: &Path) {
    match std::fs::remove_dir_all(folder) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        // A program still has one of them open. It goes next time.
        Err(error) => tracing::debug!("copies of opened files were not all removed: {error}"),
    }
}

/// Copies a file out of the store whose database is at `store`, for
/// another program to show, and returns where the copy is.
///
/// Refused for a file the system would run and not show: see
/// [`runs_as_a_program`]. The app does not offer to open such a file, and
/// this is where that is enforced.
pub fn copy_for_opening(
    store: &Path,
    held: &Path,
    file_id: Uuid,
    name: &str,
) -> io::Result<PathBuf> {
    copy_into(&opened_folder(store), held, file_id, name)
}

fn copy_into(folder: &Path, held: &Path, file_id: Uuid, name: &str) -> io::Result<PathBuf> {
    if runs_as_a_program(name) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a file that can run as a program is saved, not opened",
        ));
    }
    // Closed to other users before anything is put in it.
    make_private(folder)?;
    // A folder for each file, so that two files of one name do not meet.
    let folder = folder.join(file_id.to_string());
    std::fs::create_dir_all(&folder)?;
    // The name comes out of a note. It is used as a plain name only.
    let copy = folder.join(safe_file_name(name));
    // Opened before: the copy is made again, in case it was changed.
    save_copy(held, &copy, true)?;
    Ok(copy)
}

/// Hands a file to the system, to open with whatever shows files of its
/// kind.
pub fn open_with_system(path: &Path) -> io::Result<()> {
    open::that_detached(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_is_copied_out_under_its_own_name_to_be_opened() {
        let folder = tempfile::tempdir().unwrap();
        let held = folder.path().join("0192d3a8-0000-7000-8000-000000000001");
        std::fs::write(&held, b"a photograph").unwrap();
        let opened = folder.path().join("opened");
        let file = Uuid::from_u128(1);

        let copy = copy_into(&opened, &held, file, "whiteboard.jpg").unwrap();

        assert_eq!(copy, opened.join(file.to_string()).join("whiteboard.jpg"));
        assert_eq!(std::fs::read(&copy).unwrap(), b"a photograph");
        // The program that showed it changed it. The store's file is as
        // it was, and opening again gives the file again.
        std::fs::write(&copy, b"scribbled over").unwrap();
        assert_eq!(std::fs::read(&held).unwrap(), b"a photograph");
        let again = copy_into(&opened, &held, file, "whiteboard.jpg").unwrap();
        assert_eq!(again, copy);
        assert_eq!(std::fs::read(&copy).unwrap(), b"a photograph");

        // Another file of the same name has a place of its own.
        let other = copy_into(&opened, &held, Uuid::from_u128(2), "whiteboard.jpg").unwrap();
        assert_ne!(other, copy);

        // A name from a note is a name and never a path.
        let escaped = copy_into(&opened, &held, Uuid::from_u128(3), "../../escaped.jpg").unwrap();
        assert_eq!(
            escaped,
            opened
                .join(Uuid::from_u128(3).to_string())
                .join("escaped.jpg")
        );
        assert!(!folder.path().join("escaped.jpg").exists());

        clear(&opened);
        assert!(!opened.exists());
        // Clearing what is not there is no failure.
        clear(&opened);
    }

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

    /// On a computer with several users, nobody but the owner can look
    /// into the folder the copies are in, however it came to be there.
    #[cfg(unix)]
    #[test]
    fn the_folder_of_copies_is_closed_to_other_users() {
        use std::os::unix::fs::PermissionsExt;

        let folder = tempfile::tempdir().unwrap();
        let held = folder.path().join("held");
        std::fs::write(&held, b"a photograph").unwrap();
        let opened = folder.path().join("opened");
        // Made beforehand, open to everybody.
        std::fs::create_dir(&opened).unwrap();
        std::fs::set_permissions(&opened, std::fs::Permissions::from_mode(0o777)).unwrap();

        copy_into(&opened, &held, Uuid::from_u128(1), "whiteboard.jpg").unwrap();

        let mode = std::fs::metadata(&opened).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700, "{mode:o}");
    }

    /// A file the system would run is not copied out to be opened, whatever
    /// asks for it.
    #[test]
    fn a_program_is_not_copied_out_to_be_opened() {
        let folder = tempfile::tempdir().unwrap();
        let held = folder.path().join("held");
        std::fs::write(&held, b"MZ").unwrap();
        let opened = folder.path().join("opened");

        for name in ["setup.exe", "photo.jpg.lnk", "run.bat", "README"] {
            let error = copy_into(&opened, &held, Uuid::from_u128(1), name).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{name}");
        }
        assert!(!opened.exists(), "nothing was copied anywhere");
    }
}
