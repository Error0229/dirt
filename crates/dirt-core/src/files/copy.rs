//! Handing a file out of the store: a copy saved where the person says,
//! and what may be opened without asking.

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
