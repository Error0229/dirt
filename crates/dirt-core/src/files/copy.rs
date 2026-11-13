//! Handing a file out of the store: a copy saved where the person says, a
//! copy for another program to show, and what may be opened without asking.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Writes a copy of `held` at `target`.
///
/// Unless `replace` is set, the copy is made only if nothing is at
/// `target` at the moment it is opened, and the error is then
/// [`io::ErrorKind::AlreadyExists`]. Looking first and writing after
/// would not do: fetching a file takes a while, and something may have
/// come to be there meanwhile, put by another program.
///
/// With `replace`, the copy is written beside `target` and then given its
/// name. What was at `target` is never opened, so it is never emptied:
/// were `target` the device's own copy of the file, or a link to it,
/// emptying it first would leave nothing to copy and destroy the file.
/// And the old file is there whole until the new one is.
pub fn save_copy(held: &Path, target: &Path, replace: bool) -> io::Result<()> {
    let written = if replace {
        beside(target)
    } else {
        target.to_path_buf()
    };
    let mut to = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&written)?;
    let copied = fs::File::open(held)
        .and_then(|mut from| io::copy(&mut from, &mut to))
        .and_then(|_| {
            drop(to);
            if replace {
                fs::rename(&written, target)
            } else {
                Ok(())
            }
        });
    if copied.is_err() {
        // Half a file under the name of the whole is worse than none.
        // What is removed is this call's own: it was made just now.
        let _ = fs::remove_file(&written);
    }
    copied
}

/// A name in `target`'s folder that nothing has, to write a file under
/// before it takes `target`'s place.
///
/// The name is short and owes nothing to `target`'s own. A name made
/// from it would be longer than it, and a destination may already be as
/// long as a name can be.
fn beside(target: &Path) -> PathBuf {
    let moment = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    target.with_file_name(format!(".dirt-save-{}-{moment}", std::process::id()))
}

/// Copies a file out of the store into `folder`, for another program to
/// show, and returns where the copy is.
///
/// A file in the store has its ID for a name and must not be changed. So
/// a file is opened by copying it to a folder of its own under the name
/// the note gives it, and handing that copy over. A program that changes
/// what it was given changes the copy.
///
/// `folder` is the app's own place for such copies. It is made if it is
/// not there, and closed to everybody but its owner where the system has
/// such a thing, whether or not it was there before. It should not be in
/// a place the users of a computer share.
///
/// Refused for a file the system would run and not show: see
/// [`runs_as_a_program`]. An app does not offer to open such a file, and
/// this is where that is enforced.
pub fn copy_for_opening(
    folder: &Path,
    held: &Path,
    file_id: uuid::Uuid,
    name: &str,
) -> io::Result<PathBuf> {
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
    fs::create_dir_all(&folder)?;
    // The name comes out of a note. It is used as a plain name only.
    let copy = folder.join(super::safe_file_name(name));
    // Opened before: the copy is made again, in case it was changed.
    save_copy(held, &copy, true)?;
    Ok(copy)
}

/// Makes `folder` if it is not there, for its owner alone.
fn make_private(folder: &Path) -> io::Result<()> {
    fs::create_dir_all(folder)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Set whether or not it was just made: one made before, by this
        // program or anything else, is closed too.
        fs::set_permissions(folder, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Removes a folder of copies made for opening, and everything in it. A
/// folder that is not there is no failure, and neither is one a program
/// still has a file open in: it goes the next time.
pub fn clear_copies(folder: &Path) {
    match fs::remove_dir_all(folder) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => tracing::debug!("copies of opened files were not all removed: {error}"),
    }
}

/// Whether a file of this name is something the system would run, and
/// not show, if it were asked to open it.
///
/// A file reaches a device inside a note, from another device of the
/// account. Opening it hands it to whatever the system uses for files of
/// its kind, and for these kinds that is the system itself: the file
/// runs. An app opens the others with one press and offers only to save
/// these, so that running one is something the person does knowingly and
/// outside Dirt.
///
/// The list is of the kinds the common desktop systems run or interpret
/// on a double click. A file with no ending is on it: on Linux and macOS
/// such a file may be a program.
#[must_use]
pub fn runs_as_a_program(name: &str) -> bool {
    const RUN: &[&str] = &[
        // Windows: programs, installers, scripts and what stands for them.
        "exe",
        "com",
        "scr",
        "pif",
        "cpl",
        "msi",
        "msp",
        "msix",
        "appx",
        "bat",
        "cmd",
        "ps1",
        "psm1",
        "vbs",
        "vbe",
        "js",
        "jse",
        "wsf",
        "wsh",
        "hta",
        "lnk",
        "url",
        "reg",
        "inf",
        "gadget",
        "application",
        "jar",
        "scf",
        "library-ms",
        "search-ms",
        "chm",
        "msc",
        // macOS.
        "app",
        "command",
        "pkg",
        "dmg",
        "workflow",
        "action",
        "terminal",
        "scpt",
        "applescript",
        // Linux and others.
        "sh",
        "bash",
        "zsh",
        "fish",
        "run",
        "bin",
        "appimage",
        "desktop",
        "deb",
        "rpm",
        "py",
        "pyw",
        "pl",
        "rb",
        "php",
        // Android: opening one offers to install it.
        "apk",
        "apks",
        "apkm",
        "xapk",
        "aab",
        // Documents that carry programs of their own.
        "docm",
        "dotm",
        "xlsm",
        "xltm",
        "xlam",
        "pptm",
        "potm",
        "ppam",
        "ppsm",
        "sldm",
    ];
    let name = super::safe_file_name(name);
    match name.rsplit_once('.') {
        Some((stem, ending)) if !stem.is_empty() => {
            RUN.contains(&ending.trim().to_ascii_lowercase().as_str())
        }
        // No ending at all, or nothing but one.
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file that came to be there after a caller looked, while it was
    /// fetching, is not replaced: the copy is refused at the moment of
    /// writing.
    #[test]
    fn a_copy_is_not_written_over_a_file_that_appeared_meanwhile() {
        let folder = tempfile::tempdir().unwrap();
        let held = folder.path().join("held");
        fs::write(&held, b"the file from the note").unwrap();
        let target = folder.path().join("target.txt");

        save_copy(&held, &target, false).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"the file from the note");

        fs::write(&target, b"somebody else's").unwrap();
        let refused = save_copy(&held, &target, false).unwrap_err();
        assert_eq!(refused.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&target).unwrap(), b"somebody else's");

        // Asked to replace it: replaced whole, with nothing of the old left.
        fs::write(&target, b"somebody else's, and much longer than the file").unwrap();
        save_copy(&held, &target, true).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"the file from the note");

        // A copy that cannot be made leaves nothing under the name.
        let fresh = folder.path().join("fresh.txt");
        assert!(save_copy(&folder.path().join("no such file"), &fresh, false).is_err());
        assert!(!fresh.exists());
    }

    /// Replacing the device's own copy of the file, or a link to it, must
    /// not empty that copy before reading it.
    #[test]
    fn a_forced_copy_onto_the_file_itself_leaves_it_whole() {
        let folder = tempfile::tempdir().unwrap();
        let held = folder.path().join("held");
        fs::write(&held, b"the file from the note").unwrap();

        save_copy(&held, &held, true).unwrap();
        assert_eq!(fs::read(&held).unwrap(), b"the file from the note");

        let link = folder.path().join("link.txt");
        fs::hard_link(&held, &link).unwrap();
        save_copy(&held, &link, true).unwrap();
        assert_eq!(fs::read(&held).unwrap(), b"the file from the note");
        assert_eq!(fs::read(&link).unwrap(), b"the file from the note");
        // The copy is a file of its own now: changing it leaves the other.
        fs::write(&link, b"changed").unwrap();
        assert_eq!(fs::read(&held).unwrap(), b"the file from the note");

        // Nothing that was written on the way is left beside them.
        let mut names: Vec<String> = fs::read_dir(folder.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, ["held", "link.txt"]);

        // A forced copy that cannot be made leaves what was there as it was.
        let kept = folder.path().join("kept.txt");
        fs::write(&kept, b"still here").unwrap();
        assert!(save_copy(&folder.path().join("no such file"), &kept, true).is_err());
        assert_eq!(fs::read(&kept).unwrap(), b"still here");
        assert_eq!(fs::read_dir(folder.path()).unwrap().count(), 3);
    }

    /// What is written beside a destination on the way has a short name
    /// of its own, so a destination with the longest of names can be
    /// replaced.
    #[test]
    fn a_forced_copy_replaces_a_file_with_the_longest_of_names() {
        let folder = tempfile::tempdir().unwrap();
        let held = folder.path().join("held");
        fs::write(&held, b"the file from the note").unwrap();
        let target = folder.path().join(format!("{}.txt", "n".repeat(251)));
        fs::write(&target, b"an earlier copy").unwrap();

        save_copy(&held, &target, true).unwrap();

        assert_eq!(fs::read(&target).unwrap(), b"the file from the note");
        assert_eq!(fs::read_dir(folder.path()).unwrap().count(), 2);
    }

    #[test]
    fn a_file_is_copied_out_under_its_own_name_to_be_opened() {
        let folder = tempfile::tempdir().unwrap();
        let held = folder.path().join("0192d3a8-0000-7000-8000-000000000001");
        fs::write(&held, b"a photograph").unwrap();
        let opened = folder.path().join("opened");
        let file = uuid::Uuid::from_u128(1);

        let copy = copy_for_opening(&opened, &held, file, "whiteboard.jpg").unwrap();

        assert_eq!(copy, opened.join(file.to_string()).join("whiteboard.jpg"));
        assert_eq!(fs::read(&copy).unwrap(), b"a photograph");
        // The program that showed it changed it. The store's file is as
        // it was, and opening again gives the file again.
        fs::write(&copy, b"scribbled over").unwrap();
        assert_eq!(fs::read(&held).unwrap(), b"a photograph");
        let again = copy_for_opening(&opened, &held, file, "whiteboard.jpg").unwrap();
        assert_eq!(again, copy);
        assert_eq!(fs::read(&copy).unwrap(), b"a photograph");

        // Another file of the same name has a place of its own.
        let other =
            copy_for_opening(&opened, &held, uuid::Uuid::from_u128(2), "whiteboard.jpg").unwrap();
        assert_ne!(other, copy);

        // A name from a note is a name and never a path.
        let third = uuid::Uuid::from_u128(3);
        let escaped = copy_for_opening(&opened, &held, third, "../../escaped.jpg").unwrap();
        assert_eq!(escaped, opened.join(third.to_string()).join("escaped.jpg"));
        assert!(!folder.path().join("escaped.jpg").exists());

        clear_copies(&opened);
        assert!(!opened.exists());
        // Clearing what is not there is no failure.
        clear_copies(&opened);
    }

    /// A file the system would run is not copied out to be opened,
    /// whatever asks for it.
    #[test]
    fn a_program_is_not_copied_out_to_be_opened() {
        let folder = tempfile::tempdir().unwrap();
        let held = folder.path().join("held");
        fs::write(&held, b"MZ").unwrap();
        let opened = folder.path().join("opened");

        for name in [
            "setup.exe",
            "photo.jpg.lnk",
            "run.bat",
            "README",
            "game.apk",
        ] {
            let error =
                copy_for_opening(&opened, &held, uuid::Uuid::from_u128(1), name).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{name}");
        }
        assert!(!opened.exists(), "nothing was copied anywhere");
    }

    /// On a computer with several users, nobody but the owner can look
    /// into the folder the copies are in, however it came to be there.
    #[cfg(unix)]
    #[test]
    fn the_folder_of_copies_is_closed_to_other_users() {
        use std::os::unix::fs::PermissionsExt;

        let folder = tempfile::tempdir().unwrap();
        let held = folder.path().join("held");
        fs::write(&held, b"a photograph").unwrap();
        let opened = folder.path().join("opened");
        // Made beforehand, open to everybody.
        fs::create_dir(&opened).unwrap();
        fs::set_permissions(&opened, fs::Permissions::from_mode(0o777)).unwrap();

        copy_for_opening(&opened, &held, uuid::Uuid::from_u128(1), "whiteboard.jpg").unwrap();

        let mode = fs::metadata(&opened).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700, "{mode:o}");
    }

    #[test]
    fn what_the_system_would_run_is_not_opened_with_one_press() {
        for shown in [
            "photo.jpg",
            "Whiteboard.PNG",
            "minutes of the meeting.pdf",
            "voice memo.m4a",
            "notes.txt",
            "table.xlsx",
            "archive.zip",
            "page.html",
            "白板.jpg",
        ] {
            assert!(!runs_as_a_program(shown), "{shown}");
        }
        for run in [
            "setup.exe",
            "SETUP.EXE",
            "photo.jpg.exe",
            "invoice.pdf.lnk",
            "run me.bat",
            "script.ps1",
            "installer.msi",
            "macro.docm",
            "tool.sh",
            "Thing.app",
            "program.AppImage",
            "game.apk",
            // The ending is what is left when a name is made safe: a
            // space or a dot after it changes nothing.
            "setup.exe ",
            "setup.exe.",
            "folder/evil.cmd",
            // No ending: on some systems that is a program.
            "README",
            "run",
            ".profile",
            "",
        ] {
            assert!(runs_as_a_program(run), "{run}");
        }
    }
}
