//! What the desktop app does with a note's file outside the store: a copy
//! for another program to show.
//!
//! A file in the store has its ID for a name and must not be changed. So a
//! file is opened by copying it to a folder of its own under the name the
//! note gives it, and handing that copy to the system. A program that
//! changes what it was given changes the copy.

use std::io;
use std::path::{Path, PathBuf};

use dirt_core::files::{runs_as_a_program, safe_file_name, save_copy};
use uuid::Uuid;

/// The folder the copies are made in. It is this program's own, in the
/// system's place for what need not be kept.
fn opened_folder() -> PathBuf {
    std::env::temp_dir().join("dirt-opened-files")
}

/// Removes the copies made for opening in earlier runs. Called when the
/// app starts: a copy is needed for as long as the program showing it runs,
/// which the app cannot know, so they are kept until the next start.
pub fn clear_opened_copies() {
    clear(&opened_folder());
}

fn clear(folder: &Path) {
    match std::fs::remove_dir_all(folder) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        // A program still has one of them open. It goes next time.
        Err(error) => tracing::debug!("copies of opened files were not all removed: {error}"),
    }
}

/// Copies a file out of the store for another program to show, and returns
/// where the copy is.
///
/// Refused for a file the system would run and not show: see
/// [`runs_as_a_program`]. The app does not offer to open such a file, and
/// this is where that is enforced.
pub fn copy_for_opening(held: &Path, file_id: Uuid, name: &str) -> io::Result<PathBuf> {
    copy_into(&opened_folder(), held, file_id, name)
}

fn copy_into(folder: &Path, held: &Path, file_id: Uuid, name: &str) -> io::Result<PathBuf> {
    if runs_as_a_program(name) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a file that can run as a program is saved, not opened",
        ));
    }
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
