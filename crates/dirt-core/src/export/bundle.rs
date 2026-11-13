//! Gathering an export from a store, and writing it.

use std::collections::HashSet;
use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use thiserror::Error;
use unicode_normalization::UnicodeNormalization as _;

use super::{note_to_export_item, render_notes_export, ExportFile, ExportFormat, ExportNote};
use crate::files::safe_file_name;
use crate::models::NoteId;
use crate::session::Session;
use crate::sync::engine::{file_on_device, FileError};
use crate::sync::overview::first_line;
use crate::{Note, Store};

/// How many notes are read from the store at a time.
const PAGE: usize = 500;

/// The folder of an export that holds the notes' files.
const FILES_FOLDER: &str = "files";

/// Why an export could not be made or written.
#[derive(Debug, Error)]
pub enum ExportError {
    #[error(transparent)]
    Store(#[from] crate::Error),
    #[error(transparent)]
    Text(#[from] serde_json::Error),
    #[error(transparent)]
    Io(#[from] io::Error),
    /// Something is where the export was to be written. An export
    /// replaces nothing.
    #[error("{} is already there", .0.display())]
    AlreadyThere(PathBuf),
}

/// A file that a note names and an export with files does not hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeftOut {
    pub note_id: NoteId,
    /// The note's text, to name the note by.
    pub note_text: String,
    /// The file's name, as the note gives it.
    pub name: String,
    /// Why the export does not hold it, as sentences for the person.
    pub why: String,
}

impl LeftOut {
    /// What to call the file: its name, and the note it is attached to.
    #[must_use]
    pub fn title(&self) -> String {
        let note = first_line(&self.note_text).unwrap_or_else(|| "a note with no text".to_owned());
        format!("{}, attached to: {note}", self.name)
    }
}

/// A file an export holds: where it goes in the export and where the
/// device has it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Held {
    /// From the export's own folder, with `/` between the parts.
    path: String,
    on_device: PathBuf,
    size: u64,
    added_at_ms: i64,
}

/// Every note of a store, ready to be written out, with the notes' files
/// or without.
///
/// Gathering reads the store and, for an export with files, fetches the
/// files this device does not hold. Writing reads only what was gathered:
/// it is plain file work, which takes a while for many files, so a
/// program with a screen does it off the thread that draws.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Export {
    format: ExportFormat,
    notes: Vec<ExportNote>,
    held: Vec<Held>,
    left_out: Vec<LeftOut>,
}

impl Export {
    /// The notes alone. Each note's files are named, with no path: the
    /// export says that it does not hold them.
    pub async fn notes_only(store: &Store, format: ExportFormat) -> Result<Self, ExportError> {
        gather(store, None, format, false).await
    }

    /// The notes and their files.
    ///
    /// A file this device does not hold is fetched through `session`, if
    /// there is one and it is signed in, and kept as any fetched file is.
    /// A file that cannot be had is left out, and [`Self::left_out`] says
    /// which and why. With no `session` the export holds the files the
    /// device has.
    ///
    /// The first failure to reach the server or the storage ends the
    /// fetching: an export made with no network waits for that once, not
    /// once for each file.
    pub async fn with_files(
        store: &Store,
        session: Option<&Session>,
        format: ExportFormat,
    ) -> Result<Self, ExportError> {
        gather(store, session, format, true).await
    }

    #[must_use]
    pub const fn format(&self) -> ExportFormat {
        self.format
    }

    /// How many notes the export holds.
    #[must_use]
    pub const fn note_count(&self) -> usize {
        self.notes.len()
    }

    /// How many files the export holds.
    #[must_use]
    pub const fn file_count(&self) -> usize {
        self.held.len()
    }

    /// How many files the notes name that the export does not hold: all
    /// of them for an export of the notes alone.
    #[must_use]
    pub fn files_not_held(&self) -> usize {
        self.notes
            .iter()
            .flat_map(|note| &note.files)
            .filter(|file| file.path.is_none())
            .count()
    }

    /// The files an export with files was to hold and does not. Empty for
    /// an export of the notes alone, which was to hold none.
    #[must_use]
    pub fn left_out(&self) -> &[LeftOut] {
        &self.left_out
    }

    /// What the export holds, in words: "12 notes and 5 files".
    #[must_use]
    pub fn counts(&self) -> String {
        let notes = counted(self.notes.len(), "note", "notes");
        if self.held.is_empty() && self.left_out.is_empty() {
            notes
        } else {
            format!("{notes} and {}", counted(self.held.len(), "file", "files"))
        }
    }

    /// The files left out, as lines for the person: how many, and then
    /// each reason with the files it goes for under it. Empty if none was
    /// left out. At most `most_named` files are named, and the rest are
    /// counted.
    #[must_use]
    pub fn left_out_lines(&self, most_named: usize) -> Vec<String> {
        if self.left_out.is_empty() {
            return Vec::new();
        }
        let mut lines = vec![if self.left_out.len() == 1 {
            "1 file is not in the export.".to_owned()
        } else {
            format!("{} files are not in the export.", self.left_out.len())
        }];
        let mut reasons: Vec<&str> = Vec::new();
        for file in &self.left_out {
            if !reasons.contains(&file.why.as_str()) {
                reasons.push(&file.why);
            }
        }
        let mut named = 0;
        for reason in reasons {
            lines.push(reason.to_owned());
            for file in self.left_out.iter().filter(|file| file.why == reason) {
                if named < most_named {
                    lines.push(format!("  {}", file.title()));
                }
                named += 1;
            }
        }
        if named > most_named {
            lines.push(format!("  and {} more", named - most_named));
        }
        lines
    }

    /// The name of the notes' text in an export with files: `notes.json`
    /// or `notes.md`.
    #[must_use]
    pub fn notes_file_name(&self) -> String {
        format!("notes.{}", self.format.extension())
    }

    /// The notes as one text.
    pub fn notes_text(&self) -> Result<String, ExportError> {
        Ok(render_notes_export(&self.notes, self.format)?)
    }

    /// Writes the export as a folder: the notes' text and the files.
    ///
    /// Nothing may be at `folder`: an export replaces nothing and is not
    /// mixed into what is there. It takes the name by making the folder,
    /// which fails if anything has the name at that moment. Looking first
    /// and writing after would not do, and neither would making the
    /// export elsewhere and renaming it: on Windows a folder renamed onto
    /// a file takes the file's place.
    ///
    /// The files are written first and the notes' text last, so a folder
    /// with no notes in it is one that was not finished. If the writing
    /// fails, what was written is removed. `folder`'s parent is made if
    /// it is not there.
    pub fn write_folder(&self, folder: &Path) -> Result<(), ExportError> {
        let text = self.notes_text()?;
        if let Some(parent) = folder
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }
        fs::create_dir(folder).map_err(|error| {
            if error.kind() == io::ErrorKind::AlreadyExists {
                ExportError::AlreadyThere(folder.to_path_buf())
            } else {
                ExportError::Io(error)
            }
        })?;
        let written = self.fill_folder(folder, &text);
        if written.is_err() {
            // The folder is this call's own: it was made just now, where
            // nothing was.
            let _ = fs::remove_dir_all(folder);
        }
        Ok(written?)
    }

    fn fill_folder(&self, folder: &Path, text: &str) -> io::Result<()> {
        for held in &self.held {
            // The path was made here, from names made safe, and has no
            // part that leads out of the folder.
            let target = held
                .path
                .split('/')
                .fold(folder.to_path_buf(), |path, part| path.join(part));
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            write_new(&target, |to| {
                io::copy(&mut fs::File::open(&held.on_device)?, to).map(|_| ())
            })?;
        }
        write_new(&folder.join(self.notes_file_name()), |to| {
            to.write_all(text.as_bytes())
        })
    }

    /// Writes the export as one zip file holding what the folder would:
    /// for a device that can save one file where the person says and not
    /// a folder.
    ///
    /// Nothing may be at `target`, and nothing is left there if the
    /// writing fails. Nothing is compressed: an export is mostly
    /// pictures, which do not get smaller. `now_ms` is the time given to
    /// the notes' text. A file has the time it was attached.
    pub fn write_zip(&self, target: &Path, now_ms: i64) -> Result<(), ExportError> {
        let text = self.notes_text()?;
        let file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(target)
            .map_err(|error| {
                if error.kind() == io::ErrorKind::AlreadyExists {
                    ExportError::AlreadyThere(target.to_path_buf())
                } else {
                    ExportError::Io(error)
                }
            })?;
        let written = self.fill_zip(file, &text, now_ms);
        if written.is_err() {
            // Half an archive under the name of a whole one is worse than
            // none. It was made just now, by this call.
            let _ = fs::remove_file(target);
        }
        Ok(written?)
    }

    fn fill_zip(&self, file: fs::File, text: &str, now_ms: i64) -> io::Result<()> {
        let mut zip = zip::ZipWriter::new(io::BufWriter::new(file));
        zip.start_file(self.notes_file_name(), entry(text.len() as u64, now_ms))?;
        zip.write_all(text.as_bytes())?;
        for held in &self.held {
            zip.start_file(held.path.as_str(), entry(held.size, held.added_at_ms))?;
            io::copy(&mut fs::File::open(&held.on_device)?, &mut zip)?;
        }
        zip.finish()?.flush()
    }
}

/// How one file is put in the archive: as it is, with its time.
fn entry(size: u64, at_ms: i64) -> zip::write::SimpleFileOptions {
    zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .last_modified_time(zip_time(at_ms))
        .unix_permissions(0o644)
        // The format's older fields count a size in 32 bits.
        .large_file(size >= u64::from(u32::MAX))
}

/// A time as a zip file records one: to two seconds, from 1980 to 2107,
/// with no zone, so UTC is written. A time outside those years is written
/// as the format's earliest.
fn zip_time(at_ms: i64) -> zip::DateTime {
    use chrono::{Datelike as _, Timelike as _};
    chrono::DateTime::from_timestamp_millis(at_ms)
        .and_then(|time| {
            zip::DateTime::from_date_and_time(
                u16::try_from(time.year()).ok()?,
                u8::try_from(time.month()).ok()?,
                u8::try_from(time.day()).ok()?,
                u8::try_from(time.hour()).ok()?,
                u8::try_from(time.minute()).ok()?,
                u8::try_from(time.second()).ok()?,
            )
            .ok()
        })
        .unwrap_or_default()
}

/// Writes a file that must not be there yet.
fn write_new(target: &Path, fill: impl FnOnce(&mut fs::File) -> io::Result<()>) -> io::Result<()> {
    let mut to = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)?;
    fill(&mut to)
}

fn counted(count: usize, one: &str, many: &str) -> String {
    match count {
        0 => format!("no {many}"),
        1 => format!("1 {one}"),
        _ => format!("{count} {many}"),
    }
}

/// The names taken in one note's folder of an export.
#[derive(Default)]
struct Names(HashSet<String>);

impl Names {
    /// A name for a file the note calls `name`: safe to write on any
    /// system, and not one given before.
    ///
    /// A note may hold two files of one name. Names are also the same to
    /// Windows and macOS whatever the case of their letters, and to macOS
    /// however an accented letter is written, as one character or as a
    /// letter and its accent. A second file whose name is the same in any
    /// of those ways is numbered: `photo (2).jpg`.
    fn place(&mut self, name: &str) -> String {
        let safe = safe_file_name(name);
        let mut placed = safe.clone();
        let mut number = 1;
        while !self
            .0
            .insert(placed.nfd().collect::<String>().to_lowercase())
        {
            number += 1;
            placed = match safe.rsplit_once('.') {
                Some((stem, ending)) if !stem.is_empty() => format!("{stem} ({number}).{ending}"),
                _ => format!("{safe} ({number})"),
            };
        }
        placed
    }
}

/// Every note that is not deleted, as the store lists them.
async fn all_notes(store: &Store) -> Result<Vec<Note>, crate::Error> {
    let mut notes = Vec::new();
    loop {
        let page = store.list_notes(PAGE, notes.len()).await?;
        let read = page.len();
        notes.extend(page);
        if read < PAGE {
            return Ok(notes);
        }
    }
}

async fn gather(
    store: &Store,
    session: Option<&Session>,
    format: ExportFormat,
    with_files: bool,
) -> Result<Export, ExportError> {
    // A session that is signed out, or waiting to be let in, fetches
    // nothing.
    let session = session.filter(|session| session.state().is_ready());
    let have_files = store.notes_with_files().await?;
    let mut export = Export {
        format,
        notes: Vec::new(),
        held: Vec::new(),
        left_out: Vec::new(),
    };
    // Why the server or the storage could not be reached, once one of
    // them could not.
    let mut unreachable: Option<String> = None;

    for note in all_notes(store).await? {
        let mut item = note_to_export_item(&note);
        let attached = if have_files.contains(&note.id) {
            match store.attached_files(&note.id).await {
                Ok(attached) => attached,
                // Purged since it was listed, by a sync that ran meanwhile.
                Err(crate::Error::NotFound(_)) => Vec::new(),
                Err(other) => return Err(other.into()),
            }
        } else {
            Vec::new()
        };
        let mut names = Names::default();
        for file in attached {
            let attachment = file.attachment;
            let path = if with_files {
                let found = match (session, &unreachable) {
                    (Some(session), None) => session.fetch_file(&note.id, attachment.file_id).await,
                    _ => file_on_device(store, &note.id, attachment.file_id).await,
                };
                match found {
                    Ok(on_device) => {
                        let path = format!(
                            "{FILES_FOLDER}/{}/{}",
                            note.id,
                            names.place(&attachment.name)
                        );
                        export.held.push(Held {
                            path: path.clone(),
                            on_device,
                            size: attachment.size,
                            added_at_ms: attachment.added_at_ms,
                        });
                        Some(path)
                    }
                    Err(error) => {
                        let why = match (&unreachable, &error) {
                            // Not asked for, after the one that showed
                            // there is nobody to ask.
                            (Some(why), FileError::Missing) => why.clone(),
                            _ => error.advice(session.is_some()),
                        };
                        if matches!(error, FileError::Api(_)) {
                            unreachable.get_or_insert_with(|| why.clone());
                        }
                        export.left_out.push(LeftOut {
                            note_id: note.id,
                            note_text: note.content.clone(),
                            name: attachment.name.clone(),
                            why,
                        });
                        None
                    }
                }
            } else {
                None
            };
            item.files.push(ExportFile {
                id: attachment.file_id.to_string(),
                name: attachment.name,
                media_type: attachment.media_type,
                size: attachment.size,
                path,
            });
        }
        export.notes.push(item);
    }
    Ok(export)
}

#[cfg(test)]
mod tests {
    use std::io::Read as _;

    use super::*;

    struct Fixture {
        folder: tempfile::TempDir,
        store: Store,
    }

    impl Fixture {
        fn new() -> Self {
            let folder = tempfile::tempdir().unwrap();
            let store = Store::open(folder.path().join("store").join("notes.db")).unwrap();
            Self { folder, store }
        }

        /// Attaches a file of these bytes to the note, under `name`.
        async fn attach(&self, note: &NoteId, name: &str, bytes: &[u8]) -> uuid::Uuid {
            let source = self.folder.path().join("source");
            fs::write(&source, bytes).unwrap();
            self.store
                .attach_file(note, &source, name, crate::files::media_type_for(name))
                .await
                .unwrap()
                .file_id
        }

        fn out(&self, name: &str) -> PathBuf {
            self.folder.path().join("out").join(name)
        }
    }

    /// Every file under `folder`, by its path from there with `/` between
    /// the parts, with its bytes.
    fn files_under(folder: &Path) -> Vec<(String, Vec<u8>)> {
        fn walk(root: &Path, folder: &Path, found: &mut Vec<(String, Vec<u8>)>) {
            for entry in fs::read_dir(folder).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    walk(root, &path, found);
                } else {
                    let name = path
                        .strip_prefix(root)
                        .unwrap()
                        .components()
                        .map(|part| part.as_os_str().to_string_lossy().into_owned())
                        .collect::<Vec<_>>()
                        .join("/");
                    found.push((name, fs::read(&path).unwrap()));
                }
            }
        }
        let mut found = Vec::new();
        walk(folder, folder, &mut found);
        found.sort();
        found
    }

    /// Every file in the archive at `path`, as [`files_under`] gives a
    /// folder's.
    fn files_in_zip(path: &Path) -> Vec<(String, Vec<u8>)> {
        let mut archive = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
        let mut found = Vec::new();
        for index in 0..archive.len() {
            let mut file = archive.by_index(index).unwrap();
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes).unwrap();
            found.push((file.name().unwrap().into_owned(), bytes));
        }
        found.sort();
        found
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn the_notes_alone_name_each_file_and_hold_none() {
        let fixture = Fixture::new();
        let with = fixture.store.create_note("Trip #travel").await.unwrap();
        fixture.store.create_note("No files here").await.unwrap();
        let file = fixture.attach(&with.id, "ticket.pdf", b"a ticket").await;

        let export = Export::notes_only(&fixture.store, ExportFormat::Json)
            .await
            .unwrap();
        assert_eq!(export.note_count(), 2);
        assert_eq!(export.file_count(), 0);
        assert_eq!(export.files_not_held(), 1);
        // It was to hold none, so none was left out of it.
        assert!(export.left_out().is_empty());
        assert_eq!(export.counts(), "2 notes");

        let read: Vec<ExportNote> = serde_json::from_str(&export.notes_text().unwrap()).unwrap();
        let trip = read
            .iter()
            .find(|note| note.id == with.id.to_string())
            .unwrap();
        assert_eq!(
            trip.files,
            vec![ExportFile {
                id: file.to_string(),
                name: "ticket.pdf".to_owned(),
                media_type: "application/pdf".to_owned(),
                size: 8,
                path: None,
            }]
        );
        let other = read
            .iter()
            .find(|note| note.id != with.id.to_string())
            .unwrap();
        assert!(other.files.is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_folder_holds_the_notes_and_each_file_where_the_notes_say() {
        let fixture = Fixture::new();
        let trip = fixture.store.create_note("Trip #travel").await.unwrap();
        let plan = fixture.store.create_note("Plan").await.unwrap();
        fixture.store.create_note("No files here").await.unwrap();
        fixture.attach(&trip.id, "ticket.pdf", b"a ticket").await;
        fixture.attach(&trip.id, "map.txt", b"north is up").await;
        fixture
            .attach(&plan.id, "ticket.pdf", b"another ticket")
            .await;

        let export = Export::with_files(&fixture.store, None, ExportFormat::Json)
            .await
            .unwrap();
        assert_eq!(export.counts(), "3 notes and 3 files");
        assert_eq!(export.files_not_held(), 0);
        assert!(export.left_out_lines(usize::MAX).is_empty());

        let folder = fixture.out("export");
        export.write_folder(&folder).unwrap();
        let written = files_under(&folder);
        let mut expected = vec![
            (
                format!("files/{}/ticket.pdf", trip.id),
                b"a ticket".to_vec(),
            ),
            (
                format!("files/{}/map.txt", trip.id),
                b"north is up".to_vec(),
            ),
            (
                format!("files/{}/ticket.pdf", plan.id),
                b"another ticket".to_vec(),
            ),
        ];
        expected.sort();
        assert_eq!(
            written
                .iter()
                .filter(|(path, _)| path != "notes.json")
                .cloned()
                .collect::<Vec<_>>(),
            expected
        );

        // Every path the notes give is a file that is there, with the
        // size they give.
        let text = fs::read_to_string(folder.join("notes.json")).unwrap();
        assert_eq!(text, export.notes_text().unwrap());
        let read: Vec<ExportNote> = serde_json::from_str(&text).unwrap();
        let mut named = 0;
        for file in read.iter().flat_map(|note| &note.files) {
            let path = file.path.as_deref().unwrap();
            let bytes = &written.iter().find(|(at, _)| at == path).unwrap().1;
            assert_eq!(bytes.len() as u64, file.size);
            named += 1;
        }
        assert_eq!(named, 3);
        // Nothing was written beside it.
        assert_eq!(fs::read_dir(fixture.out("")).unwrap().count(), 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_zip_holds_what_the_folder_holds() {
        let fixture = Fixture::new();
        let trip = fixture.store.create_note("Trip").await.unwrap();
        fixture.attach(&trip.id, "ticket.pdf", b"a ticket").await;
        fixture.attach(&trip.id, "résumé.txt", b"work").await;
        fixture.store.create_note("No files here").await.unwrap();

        for format in [ExportFormat::Json, ExportFormat::Markdown] {
            let export = Export::with_files(&fixture.store, None, format)
                .await
                .unwrap();
            let folder = fixture.out(&format!("folder-{}", format.extension()));
            let archive = fixture.out(&format!("export-{}.zip", format.extension()));
            export.write_folder(&folder).unwrap();
            export.write_zip(&archive, 1_800_000_000_000).unwrap();
            let in_folder = files_under(&folder);
            assert_eq!(in_folder.len(), 3);
            assert!(in_folder
                .iter()
                .any(|(path, _)| *path == export.notes_file_name()));
            assert_eq!(files_in_zip(&archive), in_folder);
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_zip_gives_each_entry_its_time() {
        let fixture = Fixture::new();
        let trip = fixture.store.create_note("Trip").await.unwrap();
        fixture.attach(&trip.id, "ticket.pdf", b"a ticket").await;
        let export = Export::with_files(&fixture.store, None, ExportFormat::Json)
            .await
            .unwrap();
        let archive = fixture.out("export.zip");
        fs::create_dir_all(archive.parent().unwrap()).unwrap();
        // 2027-01-15 08:00:00 UTC.
        export.write_zip(&archive, 1_800_000_000_000).unwrap();

        let mut read = zip::ZipArchive::new(fs::File::open(&archive).unwrap()).unwrap();
        let notes = read.by_name("notes.json").unwrap().last_modified().unwrap();
        assert_eq!(
            (notes.year(), notes.month(), notes.day(), notes.hour()),
            (2027, 1, 15, 8)
        );
        let name = format!("files/{}/ticket.pdf", trip.id);
        let file = read.by_name(&name).unwrap();
        // Attached now, not in 1980 and not at the time given for the notes.
        assert!(file.last_modified().unwrap().year() >= 2026);
        assert_eq!(file.compression(), zip::CompressionMethod::Stored);
    }

    #[test]
    fn a_time_a_zip_cannot_record_is_its_earliest() {
        for at_ms in [0, -1, i64::MAX, i64::MIN, 4_500_000_000_000] {
            assert_eq!(zip_time(at_ms).year(), 1980, "{at_ms}");
        }
        let time = zip_time(1_800_000_000_000);
        assert_eq!((time.minute(), time.second()), (0, 0));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn files_of_one_name_and_names_that_are_paths_each_get_a_place_inside() {
        let fixture = Fixture::new();
        let note = fixture.store.create_note("Names").await.unwrap();
        fixture.attach(&note.id, "photo.jpg", b"one").await;
        fixture.attach(&note.id, "PHOTO.JPG", b"two").await;
        fixture.attach(&note.id, "photo.jpg", b"three").await;
        fixture.attach(&note.id, "../../outside.txt", b"four").await;
        fixture
            .attach(&note.id, "C:\\Windows\\win.ini", b"five")
            .await;
        // The same letters, the accent written two ways.
        fixture.attach(&note.id, "caf\u{e9}", b"six").await;
        fixture.attach(&note.id, "cafe\u{301}", b"seven").await;
        fixture.attach(&note.id, "notes", b"eight").await;
        fixture.attach(&note.id, "notes", b"nine").await;

        let export = Export::with_files(&fixture.store, None, ExportFormat::Json)
            .await
            .unwrap();
        let folder = fixture.out("export");
        export.write_folder(&folder).unwrap();

        let inside = format!("files/{}/", note.id);
        let written: Vec<(String, Vec<u8>)> = files_under(&folder)
            .into_iter()
            .filter(|(path, _)| path != "notes.json")
            .collect();
        assert_eq!(written.len(), 9);
        for (path, _) in &written {
            assert!(path.starts_with(&inside), "{path}");
            assert!(!path[inside.len()..].contains('/'), "{path}");
        }
        let at = |bytes: &[u8]| {
            let (path, _) = written.iter().find(|(_, held)| held == bytes).unwrap();
            path[inside.len()..].to_owned()
        };
        assert_eq!(at(b"one"), "photo.jpg");
        assert_eq!(at(b"two"), "PHOTO (2).JPG");
        assert_eq!(at(b"three"), "photo (3).jpg");
        assert_eq!(at(b"four"), "outside.txt");
        assert_eq!(at(b"five"), "win.ini");
        assert_eq!(at(b"six"), "caf\u{e9}");
        assert_eq!(at(b"seven"), "cafe\u{301} (2)");
        assert_eq!(at(b"eight"), "notes");
        assert_eq!(at(b"nine"), "notes (2)");
        // Nothing was written beside the export.
        assert_eq!(fs::read_dir(fixture.out("")).unwrap().count(), 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_file_the_device_does_not_hold_is_left_out_and_said() {
        let fixture = Fixture::new();
        let trip = fixture
            .store
            .create_note("Trip to the coast\nmore")
            .await
            .unwrap();
        let here = fixture.attach(&trip.id, "here.txt", b"here").await;
        let away = fixture.attach(&trip.id, "away.txt", b"away").await;
        let gone = fixture.attach(&trip.id, "gone.txt", b"gone").await;
        fs::remove_file(fixture.store.shelf().path(away)).unwrap();
        fs::remove_file(fixture.store.shelf().path(gone)).unwrap();

        let export = Export::with_files(&fixture.store, None, ExportFormat::Json)
            .await
            .unwrap();
        assert_eq!(export.counts(), "1 note and 1 file");
        assert_eq!(export.files_not_held(), 2);
        let why = "The file is not on this device, and this device is not signed in to fetch it.";
        assert_eq!(
            export.left_out(),
            [
                LeftOut {
                    note_id: trip.id,
                    note_text: "Trip to the coast\nmore".to_owned(),
                    name: "away.txt".to_owned(),
                    why: why.to_owned(),
                },
                LeftOut {
                    note_id: trip.id,
                    note_text: "Trip to the coast\nmore".to_owned(),
                    name: "gone.txt".to_owned(),
                    why: why.to_owned(),
                },
            ]
        );
        assert_eq!(
            export.left_out_lines(usize::MAX),
            [
                "2 files are not in the export.",
                why,
                "  away.txt, attached to: Trip to the coast",
                "  gone.txt, attached to: Trip to the coast",
            ]
        );
        assert_eq!(
            export.left_out_lines(1),
            [
                "2 files are not in the export.",
                why,
                "  away.txt, attached to: Trip to the coast",
                "  and 1 more",
            ]
        );

        let folder = fixture.out("export");
        export.write_folder(&folder).unwrap();
        let read: Vec<ExportNote> =
            serde_json::from_str(&fs::read_to_string(folder.join("notes.json")).unwrap()).unwrap();
        let paths: Vec<(String, Option<String>)> = read[0]
            .files
            .iter()
            .map(|file| (file.id.clone(), file.path.clone()))
            .collect();
        assert_eq!(
            paths,
            [
                (
                    here.to_string(),
                    Some(format!("files/{}/here.txt", trip.id))
                ),
                (away.to_string(), None),
                (gone.to_string(), None),
            ]
        );
        assert_eq!(files_under(&folder).len(), 2);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn an_export_with_all_its_files_left_out_says_it_has_none() {
        let fixture = Fixture::new();
        let note = fixture.store.create_note("One").await.unwrap();
        let file = fixture.attach(&note.id, "a.txt", b"a").await;
        fs::remove_file(fixture.store.shelf().path(file)).unwrap();
        let export = Export::with_files(&fixture.store, None, ExportFormat::Json)
            .await
            .unwrap();
        assert_eq!(export.counts(), "1 note and no files");
        assert_eq!(export.left_out_lines(5)[0], "1 file is not in the export.");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_deleted_notes_files_are_not_exported() {
        let fixture = Fixture::new();
        let kept = fixture.store.create_note("Kept").await.unwrap();
        let deleted = fixture.store.create_note("Deleted").await.unwrap();
        fixture.attach(&kept.id, "kept.txt", b"kept").await;
        fixture.attach(&deleted.id, "deleted.txt", b"deleted").await;
        fixture.store.delete_note(&deleted.id).await.unwrap();

        let export = Export::with_files(&fixture.store, None, ExportFormat::Markdown)
            .await
            .unwrap();
        assert_eq!(export.counts(), "1 note and 1 file");
        let folder = fixture.out("export");
        export.write_folder(&folder).unwrap();
        let written = files_under(&folder);
        assert_eq!(
            written
                .iter()
                .map(|(path, _)| path.clone())
                .collect::<Vec<_>>(),
            [format!("files/{}/kept.txt", kept.id), "notes.md".to_owned()]
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn an_export_replaces_nothing() {
        let fixture = Fixture::new();
        let note = fixture.store.create_note("One").await.unwrap();
        fixture.attach(&note.id, "a.txt", b"a").await;
        let export = Export::with_files(&fixture.store, None, ExportFormat::Json)
            .await
            .unwrap();

        // An empty folder, a folder with something in it, and a file.
        let empty = fixture.out("empty");
        fs::create_dir_all(&empty).unwrap();
        let full = fixture.out("full");
        fs::create_dir_all(&full).unwrap();
        fs::write(full.join("mine.txt"), b"mine").unwrap();
        let file = fixture.out("file");
        fs::write(&file, b"mine").unwrap();
        for there in [&empty, &full, &file] {
            let refused = export.write_folder(there).unwrap_err();
            assert!(
                matches!(&refused, ExportError::AlreadyThere(path) if path == there),
                "{refused:?}"
            );
            let refused = export.write_zip(there, 0);
            if there.is_file() {
                assert!(
                    matches!(&refused, Err(ExportError::AlreadyThere(path)) if path == there),
                    "{refused:?}"
                );
            } else {
                assert!(refused.is_err());
            }
        }
        assert_eq!(fs::read(full.join("mine.txt")).unwrap(), b"mine");
        assert_eq!(fs::read(&file).unwrap(), b"mine");
        assert_eq!(fs::read_dir(&empty).unwrap().count(), 0);
        // Nothing was left beside them either.
        assert_eq!(fs::read_dir(fixture.out("")).unwrap().count(), 3);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn an_export_that_fails_part_way_leaves_nothing() {
        let fixture = Fixture::new();
        let note = fixture.store.create_note("One").await.unwrap();
        fixture.attach(&note.id, "a.txt", b"a").await;
        let lost = fixture.attach(&note.id, "b.txt", b"b").await;
        let export = Export::with_files(&fixture.store, None, ExportFormat::Json)
            .await
            .unwrap();
        // Gone between the gathering and the writing.
        fs::remove_file(fixture.store.shelf().path(lost)).unwrap();

        fs::create_dir_all(fixture.out("")).unwrap();
        assert!(matches!(
            export.write_folder(&fixture.out("export")),
            Err(ExportError::Io(_))
        ));
        assert!(matches!(
            export.write_zip(&fixture.out("export.zip"), 0),
            Err(ExportError::Io(_))
        ));
        assert_eq!(fs::read_dir(fixture.out("")).unwrap().count(), 0);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_folder_has_its_files_before_it_has_its_notes() {
        let fixture = Fixture::new();
        let note = fixture.store.create_note("One").await.unwrap();
        fixture.attach(&note.id, "a.txt", b"a").await;
        let export = Export::with_files(&fixture.store, None, ExportFormat::Json)
            .await
            .unwrap();
        // A file is gone between the gathering and the writing, so the
        // writing fails. Looked at before it is cleared away, the folder
        // has no notes in it: nothing says it is an export.
        let lost = fixture.attach(&note.id, "b.txt", b"b").await;
        let export_with_lost = Export::with_files(&fixture.store, None, ExportFormat::Json)
            .await
            .unwrap();
        fs::remove_file(fixture.store.shelf().path(lost)).unwrap();
        let folder = fixture.out("unfinished");
        fs::create_dir_all(&folder).unwrap();
        let text = export_with_lost.notes_text().unwrap();
        assert!(export_with_lost.fill_folder(&folder, &text).is_err());
        assert!(!folder.join("notes.json").exists());
        assert!(folder.join("files").is_dir());

        // Written whole, it has them.
        let whole = fixture.out("whole");
        export.write_folder(&whole).unwrap();
        assert!(whole.join("notes.json").is_file());
    }

    /// Names are told apart before anything is written. Should two files
    /// be given one place all the same, on a file system that compares
    /// names by a rule nobody thought of, the export fails. One file is
    /// never written over another and called both.
    #[test]
    fn two_files_given_one_place_fail_the_export() {
        let root = tempfile::tempdir().unwrap();
        let (first, second) = (root.path().join("first"), root.path().join("second"));
        fs::write(&first, b"first").unwrap();
        fs::write(&second, b"second").unwrap();
        let held = |on_device: &Path| Held {
            path: "files/note/same.txt".to_owned(),
            on_device: on_device.to_path_buf(),
            size: 5,
            added_at_ms: 0,
        };
        let export = Export {
            format: ExportFormat::Json,
            notes: Vec::new(),
            held: vec![held(&first), held(&second)],
            left_out: Vec::new(),
        };
        let folder = root.path().join("export");
        assert!(matches!(
            export.write_folder(&folder),
            Err(ExportError::Io(error)) if error.kind() == io::ErrorKind::AlreadyExists
        ));
        assert!(!folder.exists());
        // A zip file refuses a second entry of one name too.
        let archive = root.path().join("export.zip");
        assert!(export.write_zip(&archive, 0).is_err());
        assert!(!archive.exists());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn an_export_reaches_every_note() {
        let fixture = Fixture::new();
        for number in 0..=PAGE {
            fixture
                .store
                .create_note(&format!("note {number}"))
                .await
                .unwrap();
        }
        let export = Export::notes_only(&fixture.store, ExportFormat::Json)
            .await
            .unwrap();
        assert_eq!(export.note_count(), PAGE + 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_folder_is_made_with_the_folders_above_it() {
        let fixture = Fixture::new();
        fixture.store.create_note("One").await.unwrap();
        let export = Export::with_files(&fixture.store, None, ExportFormat::Markdown)
            .await
            .unwrap();
        let folder = fixture.out("a").join("b").join("export");
        export.write_folder(&folder).unwrap();
        assert_eq!(
            fs::read_to_string(folder.join("notes.md")).unwrap(),
            export.notes_text().unwrap()
        );
        assert_eq!(export.counts(), "1 note");
    }
}
