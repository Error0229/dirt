//! Where a device keeps the files of one store.
//!
//! Files sit beside the store's database, in a folder of their own, each
//! named by its ID. They are kept as they are, not encrypted: the notes in
//! the database beside them are not either. What protects both is the
//! device.
//!
//! A file is written under another name first and given its own only when
//! it is whole, so a file that is there is always complete.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use uuid::Uuid;

/// The folder that holds one store's files.
#[derive(Debug, Clone)]
pub struct Shelf {
    folder: PathBuf,
}

impl Shelf {
    /// The shelf that belongs to the store whose database is at `store`.
    #[must_use]
    pub fn beside(store: &Path) -> Self {
        let folder = store
            .parent()
            .map_or_else(|| PathBuf::from("files"), |parent| parent.join("files"));
        Self { folder }
    }

    /// Where the file with this ID is, or would be.
    #[must_use]
    pub fn path(&self, file: Uuid) -> PathBuf {
        self.folder.join(file.to_string())
    }

    /// Whether the device holds the file.
    #[must_use]
    pub fn holds(&self, file: Uuid) -> bool {
        self.path(file).is_file()
    }

    /// A place to write a file that is not whole yet. Give it to
    /// [`Self::keep`] when it is.
    pub fn unfinished(&self, file: Uuid) -> io::Result<PathBuf> {
        fs::create_dir_all(&self.folder)?;
        Ok(self.folder.join(format!("{file}.part")))
    }

    /// Makes an unfinished file the file.
    pub fn keep(&self, file: Uuid) -> io::Result<()> {
        fs::rename(self.unfinished(file)?, self.path(file))
    }

    /// A place of its own to write a file that is being fetched. Two
    /// programs on one device may fetch the same file at once, and each
    /// writes to its own place. Give it to [`Self::keep_from`].
    pub fn unfinished_of_my_own(&self, file: Uuid) -> io::Result<PathBuf> {
        fs::create_dir_all(&self.folder)?;
        Ok(self
            .folder
            .join(format!("{file}.{}.part", Uuid::now_v7().simple())))
    }

    /// Makes the unfinished file at `unfinished` the file. If the shelf
    /// came to hold the file in the meantime, that copy stays and this one
    /// is dropped: they are the same file.
    pub fn keep_from(&self, file: Uuid, unfinished: &Path) -> io::Result<()> {
        if self.holds(file) {
            return fs::remove_file(unfinished);
        }
        fs::rename(unfinished, self.path(file))
    }

    /// Removes unfinished files that were last written before `before`:
    /// what a program left that stopped while it was fetching or copying.
    /// Returns how many.
    pub fn clear_unfinished(&self, before: std::time::SystemTime) -> io::Result<usize> {
        let entries = match fs::read_dir(&self.folder) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
            Err(error) => return Err(error),
        };
        let mut removed = 0;
        for entry in entries {
            let entry = entry?;
            let unfinished = entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "part");
            if unfinished && entry.metadata()?.modified()? < before {
                fs::remove_file(entry.path())?;
                removed += 1;
            }
        }
        Ok(removed)
    }

    /// Puts a file made on this device, from its bytes, on the shelf.
    pub fn put(&self, file: Uuid, contents: &[u8]) -> io::Result<()> {
        let unfinished = self.unfinished(file)?;
        match fs::write(&unfinished, contents) {
            Ok(()) => self.keep(file),
            Err(error) => {
                let _ = fs::remove_file(&unfinished);
                Err(error)
            }
        }
    }

    /// Copies a file from elsewhere on the device onto the shelf. Returns
    /// its size.
    pub fn add(&self, file: Uuid, source: &Path) -> io::Result<u64> {
        let unfinished = self.unfinished(file)?;
        let copied = fs::copy(source, &unfinished);
        match copied {
            Ok(size) => {
                self.keep(file)?;
                Ok(size)
            }
            Err(error) => {
                let _ = fs::remove_file(&unfinished);
                Err(error)
            }
        }
    }

    /// Every file the shelf holds whole.
    pub fn list(&self) -> io::Result<Vec<Uuid>> {
        let entries = match fs::read_dir(&self.folder) {
            Ok(entries) => entries,
            // No file was ever put on it.
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut files = Vec::new();
        for entry in entries {
            // An unfinished file, or anything else that is there, has a
            // name that is not an ID.
            if let Some(file) = entry?
                .file_name()
                .to_str()
                .and_then(|name| name.parse().ok())
            {
                files.push(file);
            }
        }
        Ok(files)
    }

    /// Removes the file, and anything unfinished under its name. A file
    /// that is not there is no error.
    pub fn remove(&self, file: Uuid) -> io::Result<()> {
        for path in [self.path(file), self.folder.join(format!("{file}.part"))] {
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_is_there_only_once_it_is_whole() {
        let folder = tempfile::tempdir().unwrap();
        let shelf = Shelf::beside(&folder.path().join("notes.db"));
        let file = Uuid::now_v7();
        assert!(!shelf.holds(file));

        // Being written: not there yet.
        let unfinished = shelf.unfinished(file).unwrap();
        fs::write(&unfinished, b"half of").unwrap();
        assert!(!shelf.holds(file));
        fs::write(&unfinished, b"half of it, and the rest").unwrap();
        shelf.keep(file).unwrap();
        assert!(shelf.holds(file));
        assert_eq!(
            fs::read(shelf.path(file)).unwrap(),
            b"half of it, and the rest"
        );
        assert!(!unfinished.exists());

        // The files are beside the database, in a folder of their own.
        assert_eq!(
            shelf.path(file).parent().unwrap(),
            folder.path().join("files")
        );
    }

    #[test]
    fn a_file_is_copied_in_and_removed() {
        let folder = tempfile::tempdir().unwrap();
        let shelf = Shelf::beside(&folder.path().join("notes.db"));
        let source = folder.path().join("a photo.jpg");
        fs::write(&source, b"not really a photo").unwrap();
        let file = Uuid::now_v7();

        assert_eq!(shelf.add(file, &source).unwrap(), 18);
        assert!(shelf.holds(file));
        assert!(
            source.exists(),
            "the person's own file is left where it was"
        );

        shelf.remove(file).unwrap();
        assert!(!shelf.holds(file));
        // Removing again, or removing what was never there, is no error.
        shelf.remove(file).unwrap();
        shelf.remove(Uuid::now_v7()).unwrap();

        // A file that cannot be read is not half there.
        let missing = folder.path().join("no such file");
        assert!(shelf.add(file, &missing).is_err());
        assert!(!shelf.holds(file));
        assert!(!folder
            .path()
            .join("files")
            .join(format!("{file}.part"))
            .exists());
    }
}
