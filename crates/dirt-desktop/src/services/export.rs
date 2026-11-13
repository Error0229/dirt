//! Exporting every note and its files into a folder.
//!
//! What an export holds is `dirt-core`'s, the same as the CLI writes. This
//! module gives it a place: a new folder, named for the moment, inside the
//! folder the person chose.

use std::path::{Path, PathBuf};

use dirt_core::export::{suggested_export_folder_name, Export, ExportError, ExportFormat};
use dirt_core::session::Session;
use dirt_core::Store;

/// How many files that could not be exported the settings name before
/// they count the rest.
const MOST_NAMED: usize = 5;

/// An export that was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exported {
    /// The folder it is in.
    pub folder: PathBuf,
    /// What to tell the person: what it holds and where it is, and then
    /// any files it could not be given, with why. One line each.
    pub said: String,
}

/// Exports every note and its files into a new folder inside `parent`.
///
/// A file this device does not hold is fetched through `session` first.
/// One that cannot be had is left out, and what is said names it.
pub async fn export_into(
    store: &Store,
    session: Option<&Session>,
    format: ExportFormat,
    parent: &Path,
    now_ms: i64,
) -> Result<Exported, ExportError> {
    let export = Export::with_files(store, session, format).await?;
    let folder = parent.join(suggested_export_folder_name(now_ms));
    let mut said = vec![format!(
        "Exported {} to {}.",
        export.counts(),
        folder.display()
    )];
    said.extend(export.left_out_lines(MOST_NAMED));
    // Copying takes as long as the disk it goes to takes. Not on the
    // window's own thread, which would stop drawing for that long.
    let written = folder.clone();
    tokio::task::spawn_blocking(move || export.write_folder(&written))
        .await
        .map_err(|error| ExportError::Io(std::io::Error::other(error)))??;
    Ok(Exported {
        folder,
        said: said.join("\n"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn an_export_goes_into_a_new_folder_named_for_the_moment() {
        let folder = tempfile::tempdir().unwrap();
        let store = Store::open(folder.path().join("store").join("notes.db")).unwrap();
        let note = store.create_note("Desktop export #one").await.unwrap();
        store.create_note("Desktop export #two").await.unwrap();
        let source = folder.path().join("plan.txt");
        std::fs::write(&source, b"the plan").unwrap();
        store
            .attach_file(&note.id, &source, "plan.txt", "text/plain")
            .await
            .unwrap();
        let chosen = folder.path().join("chosen");
        std::fs::create_dir(&chosen).unwrap();
        std::fs::write(chosen.join("mine.txt"), b"mine").unwrap();

        let exported = export_into(&store, None, ExportFormat::Markdown, &chosen, 456)
            .await
            .unwrap();
        assert_eq!(exported.folder, chosen.join("dirt-export-456"));
        assert_eq!(
            exported.said,
            format!(
                "Exported 2 notes and 1 file to {}.",
                exported.folder.display()
            )
        );
        let notes = std::fs::read_to_string(exported.folder.join("notes.md")).unwrap();
        assert!(notes.contains("Desktop export #one"));
        assert!(notes.contains("Desktop export #two"));
        assert_eq!(
            std::fs::read(
                exported
                    .folder
                    .join("files")
                    .join(note.id.to_string())
                    .join("plan.txt")
            )
            .unwrap(),
            b"the plan"
        );
        // What was in the chosen folder is as it was.
        assert_eq!(std::fs::read(chosen.join("mine.txt")).unwrap(), b"mine");

        // The same moment again is the same folder, which is there.
        let again = export_into(&store, None, ExportFormat::Markdown, &chosen, 456).await;
        assert!(matches!(again, Err(ExportError::AlreadyThere(_))));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_file_that_could_not_be_exported_is_said() {
        let folder = tempfile::tempdir().unwrap();
        let store = Store::open(folder.path().join("store").join("notes.db")).unwrap();
        let note = store.create_note("Trip").await.unwrap();
        let source = folder.path().join("map.txt");
        std::fs::write(&source, b"north is up").unwrap();
        let file = store
            .attach_file(&note.id, &source, "map.txt", "text/plain")
            .await
            .unwrap();
        std::fs::remove_file(store.shelf().path(file.file_id)).unwrap();

        let exported = export_into(&store, None, ExportFormat::Json, folder.path(), 1)
            .await
            .unwrap();
        assert_eq!(
            exported.said,
            format!(
                "Exported 1 note and no files to {}.\n\
                 1 file is not in the export.\n\
                 The file is not on this device, and this device is not signed in to fetch it.\n  \
                 map.txt, attached to: Trip",
                exported.folder.display()
            )
        );
        assert!(exported.folder.join("notes.json").is_file());
    }
}
