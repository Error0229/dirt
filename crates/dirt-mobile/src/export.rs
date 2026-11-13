//! Exporting every note and its files as one zip file.
//!
//! What the export holds is `dirt-core`'s, the same as the desktop app and
//! the CLI write into a folder. Android lets an app save one file where
//! the person says, on its own screen for that, and not a folder, so here
//! the folder is one archive.
//!
//! The archive is made in a folder of the app's own, and the activity
//! copies it to the place the person chooses
//! (`services::activity::save_copy`). The app's copy is removed when that
//! is done, and whatever an earlier run left is removed at the next start.
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

use std::path::{Path, PathBuf};

use dirt_core::export::{suggested_export_zip_name, Export, ExportError, ExportFormat};
use dirt_core::session::Session;
use dirt_core::Store;

use crate::config::default_mobile_data_directory;

/// How many files that could not be exported the screen names before it
/// counts the rest.
const MOST_NAMED: usize = 5;

/// What kind of file an export is, for Android's screen for saving one.
pub const MIME_TYPE: &str = "application/zip";

/// The folder an export is made in before Android is given it. It is
/// beside the notes, inside the app, where no other app can read.
pub fn export_folder() -> PathBuf {
    default_mobile_data_directory().join("export")
}

/// Removes what earlier exports left: the app was stopped while the person
/// was still choosing where one should go.
pub fn clear_exports(folder: &Path) {
    dirt_core::files::clear_copies(folder);
}

/// An export ready to be saved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Archive {
    /// The name offered to the person, which they may change.
    pub name: String,
    /// Where the app has it.
    pub path: PathBuf,
    /// What it holds, in words: "12 notes and 5 files".
    pub counts: String,
    /// The files it could not be given, with why, as lines. Empty if it
    /// has them all.
    pub left_out: Vec<String>,
}

impl Archive {
    /// Removes the app's copy, once Android has made its own or the
    /// person has backed out.
    pub fn discard(&self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Every note in `store` and its files, as an archive in `folder`. `None`
/// if there are no notes, and then nothing is written.
///
/// A file this device does not hold is fetched through `session` first.
/// One that cannot be had is left out, and the archive says which.
pub async fn make_archive(
    store: &Store,
    session: Option<&Session>,
    format: ExportFormat,
    folder: &Path,
    now_ms: i64,
) -> Result<Option<Archive>, ExportError> {
    let export = Export::with_files(store, session, format).await?;
    if export.note_count() == 0 {
        return Ok(None);
    }
    let name = suggested_export_zip_name(now_ms);
    let archive = Archive {
        path: folder.join(&name),
        name,
        counts: export.counts(),
        left_out: export.left_out_lines(MOST_NAMED),
    };
    // Writing copies every file. Not on the thread that draws.
    let (folder, path) = (folder.to_path_buf(), archive.path.clone());
    tokio::task::spawn_blocking(move || {
        std::fs::create_dir_all(&folder)?;
        export.write_zip(&path, now_ms)
    })
    .await
    .map_err(|error| ExportError::Io(std::io::Error::other(error)))??;
    Ok(Some(archive))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::MobileNoteStore;

    #[tokio::test(flavor = "multi_thread")]
    async fn an_export_is_one_archive_named_for_the_moment() {
        let (folder, store) = MobileNoteStore::open_temporary();
        let note = store.create_note("Mobile export #one").await.unwrap();
        store.create_note("Mobile export #two").await.unwrap();
        let source = folder.path().join("plan.txt");
        std::fs::write(&source, b"the plan").unwrap();
        store
            .core()
            .attach_file(&note.id, &source, "plan.txt", "text/plain")
            .await
            .unwrap();
        let exports = folder.path().join("export");

        let archive = make_archive(store.core(), None, ExportFormat::Markdown, &exports, 456)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(archive.name, "dirt-export-456.zip");
        assert_eq!(archive.path, exports.join("dirt-export-456.zip"));
        assert_eq!(archive.counts, "2 notes and 1 file");
        assert!(archive.left_out.is_empty());
        // It is a zip file: those begin with these four bytes.
        let bytes = std::fs::read(&archive.path).unwrap();
        assert_eq!(&bytes[..4], b"PK\x03\x04");
        // Nothing is compressed, so what it holds can be seen in it.
        let holds = |text: &str| {
            bytes
                .windows(text.len())
                .any(|part| part == text.as_bytes())
        };
        assert!(holds("notes.md"));
        assert!(holds("Mobile export #one"));
        assert!(holds("Mobile export #two"));
        assert!(holds(&format!("files/{}/plan.txt", note.id)));
        assert!(holds("the plan"));

        archive.discard();
        assert!(!archive.path.exists());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn an_export_says_which_files_it_lacks() {
        let (folder, store) = MobileNoteStore::open_temporary();
        let note = store.create_note("Trip").await.unwrap();
        let source = folder.path().join("map.txt");
        std::fs::write(&source, b"north is up").unwrap();
        let file = store
            .core()
            .attach_file(&note.id, &source, "map.txt", "text/plain")
            .await
            .unwrap();
        std::fs::remove_file(store.core().shelf().path(file.file_id)).unwrap();

        let archive = make_archive(
            store.core(),
            None,
            ExportFormat::Json,
            &folder.path().join("export"),
            1,
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(archive.counts, "1 note and no files");
        assert_eq!(
            archive.left_out,
            [
                "1 file is not in the export.",
                "The file is not on this device, and this device is not signed in to fetch it.",
                "  map.txt, attached to: Trip",
            ]
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn with_no_notes_there_is_nothing_to_export() {
        let (folder, store) = MobileNoteStore::open_temporary();
        let exports = folder.path().join("export");
        let archive = make_archive(store.core(), None, ExportFormat::Json, &exports, 1)
            .await
            .unwrap();
        assert_eq!(archive, None);
        assert!(!exports.exists());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn an_export_reaches_past_the_notes_the_list_shows() {
        let (folder, store) = MobileNoteStore::open_temporary();
        for number in 0..130 {
            store.create_note(&format!("note {number}")).await.unwrap();
        }
        let archive = make_archive(
            store.core(),
            None,
            ExportFormat::Json,
            &folder.path().join("export"),
            1,
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(archive.counts, "130 notes");
    }

    #[test]
    fn what_earlier_exports_left_is_removed() {
        let folder = tempfile::tempdir().unwrap();
        let exports = folder.path().join("export");
        std::fs::create_dir_all(&exports).unwrap();
        std::fs::write(exports.join("dirt-export-1.zip"), b"left").unwrap();
        clear_exports(&exports);
        assert!(!exports.exists());
        // And a folder that is not there is no trouble.
        clear_exports(&exports);
    }
}
