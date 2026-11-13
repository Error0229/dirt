//! Where each store lives on disk.
//!
//! There is one database per account, in a folder named after the account,
//! so that two accounts used on one computer can never see each other's
//! notes. Notes written before anyone has signed in go to the `local` store
//! and are moved into the account's store at first sign-in (design section
//! 6.5).
//!
//! ```text
//! <data folder>/stores/local/notes.db
//! <data folder>/stores/account-<id>/notes.db
//! ```
//!
//! The data folder is its owner's alone: see [`close_data_folder`].

use std::io;
use std::path::{Path, PathBuf};

use crate::{Error, Result};

const DATABASE_FILE: &str = "notes.db";

/// Which store to open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreName {
    /// Notes written while nobody is signed in.
    Local,
    /// The notes of one account.
    Account(String),
}

impl StoreName {
    /// The store of the account with this ID.
    ///
    /// The ID becomes a folder name, so it may hold only letters, digits,
    /// hyphens and underscores, and at most 64 of them.
    pub fn account(id: &str) -> Result<Self> {
        let valid = !id.is_empty()
            && id.len() <= 64
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if valid {
            Ok(Self::Account(id.to_owned()))
        } else {
            Err(Error::InvalidInput(format!(
                "an account ID must be 1 to 64 letters, digits, hyphens or underscores: {id:?}"
            )))
        }
    }

    fn folder(&self) -> String {
        match self {
            Self::Local => "local".to_owned(),
            // The prefix keeps an account that happens to be called "local"
            // out of the local store's folder.
            Self::Account(id) => format!("account-{id}"),
        }
    }
}

/// The database file of a store under `data_folder`, which is the
/// application's own data folder (for example `%APPDATA%\dirt`).
#[must_use]
pub fn store_path(data_folder: &Path, name: &StoreName) -> PathBuf {
    data_folder
        .join("stores")
        .join(name.folder())
        .join(DATABASE_FILE)
}

/// Makes the application's data folder if it is not there, and closes it
/// to every user of the computer but its owner.
///
/// On Linux and macOS a new folder gets the permissions the system gives
/// any new folder, which on many computers lets every user read it, and a
/// person's home is often open to the others too. The notes and their
/// files are in this folder as they are, not encrypted. So the folder is
/// made for its owner alone, and one that is already there and open to
/// others, made by an earlier version, is closed. Closing the folder
/// closes everything in it: nobody else can pass through it.
///
/// Only what others were allowed is taken away. What the owner allowed
/// themselves is left as they set it.
///
/// Every program that keeps something in the data folder calls this
/// before it does. A database file given by hand, which is in a folder of
/// the person's choosing, is not this function's business: that folder is
/// left as the person made it.
///
/// On Windows a person's own data folder is already theirs alone, and on
/// Android an app's is. Nothing is changed there.
///
/// A folder that cannot be closed is used as it is, and the log says so:
/// it may be on a drive that keeps no permissions, where refusing would
/// leave the person with no notes at all.
pub fn close_data_folder(data_folder: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt as _, PermissionsExt as _};

        /// What the owner may do, and nobody else anything.
        const OWNER_ONLY: u32 = 0o700;

        // Made closed, not made and then closed: there is no moment at
        // which it is there and open.
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(OWNER_ONLY)
            .create(data_folder)?;
        let mode = std::fs::metadata(data_folder)?.permissions().mode();
        if mode & !OWNER_ONLY & 0o777 != 0 {
            let closed = std::fs::Permissions::from_mode(mode & OWNER_ONLY);
            if let Err(error) = std::fs::set_permissions(data_folder, closed) {
                tracing::warn!(
                    "{} could not be closed to the other users of this computer: {error}",
                    data_folder.display()
                );
            }
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(data_folder)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_data_folder_is_made_with_the_folders_above_it() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("a").join("dirt");
        close_data_folder(&folder).unwrap();
        assert!(folder.is_dir());
        // And again, now that it is there.
        close_data_folder(&folder).unwrap();
    }

    #[cfg(unix)]
    mod permissions {
        use std::os::unix::fs::PermissionsExt as _;
        use std::path::Path;

        use super::super::{close_data_folder, store_path, StoreName};
        use crate::Store;

        fn mode(path: &Path) -> u32 {
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777
        }

        fn set_mode(path: &Path, mode: u32) {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
        }

        #[test]
        fn a_new_data_folder_is_its_owners_alone() {
            let root = tempfile::tempdir().unwrap();
            let folder = root.path().join("dirt");
            close_data_folder(&folder).unwrap();
            assert_eq!(mode(&folder), 0o700);
        }

        #[test]
        fn a_data_folder_that_was_open_is_closed() {
            for (before, after) in [
                (0o755, 0o700),
                (0o750, 0o700),
                (0o777, 0o700),
                (0o701, 0o700),
                // What the owner took from themselves stays taken.
                (0o555, 0o500),
                (0o700, 0o700),
            ] {
                let root = tempfile::tempdir().unwrap();
                let folder = root.path().join("dirt");
                std::fs::create_dir(&folder).unwrap();
                set_mode(&folder, before);
                close_data_folder(&folder).unwrap();
                assert_eq!(mode(&folder), after, "from {before:o}");
                // So that the folder can be removed.
                set_mode(&folder, 0o700);
            }
        }

        #[test]
        fn opening_a_store_in_the_data_folder_closes_the_folder() {
            let root = tempfile::tempdir().unwrap();
            let folder = root.path().join("dirt");
            std::fs::create_dir(&folder).unwrap();
            set_mode(&folder, 0o755);
            let store = Store::open_in(&folder, &StoreName::Local).unwrap();
            assert_eq!(mode(&folder), 0o700);
            assert_eq!(store.path(), store_path(&folder, &StoreName::Local));
        }

        #[test]
        fn a_database_given_by_hand_leaves_its_folder_as_it_was() {
            let root = tempfile::tempdir().unwrap();
            let folder = root.path().join("shared");
            std::fs::create_dir(&folder).unwrap();
            set_mode(&folder, 0o755);
            let _store = Store::open(folder.join("notes.db")).unwrap();
            assert_eq!(mode(&folder), 0o755);
        }
    }

    #[test]
    fn each_store_has_its_own_folder() {
        let root = Path::new("data");
        let local = store_path(root, &StoreName::Local);
        let alice = store_path(root, &StoreName::account("user_alice").unwrap());
        let bob = store_path(root, &StoreName::account("user_bob").unwrap());
        assert!(local.ends_with("stores/local/notes.db"));
        assert!(alice.ends_with("stores/account-user_alice/notes.db"));
        assert_ne!(alice, bob);
        assert_ne!(local, alice);
    }

    #[test]
    fn an_account_called_local_does_not_share_the_local_store() {
        let root = Path::new("data");
        assert_ne!(
            store_path(root, &StoreName::Local),
            store_path(root, &StoreName::account("local").unwrap())
        );
    }

    #[test]
    fn an_account_id_cannot_leave_the_stores_folder() {
        for id in ["", "../escape", "a/b", "a\\b", "a b", "..", "a.b"] {
            assert!(StoreName::account(id).is_err(), "accepted {id:?}");
        }
        assert!(StoreName::account(&"x".repeat(65)).is_err());
        assert!(StoreName::account(&"x".repeat(64)).is_ok());
    }
}
