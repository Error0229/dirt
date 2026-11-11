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

#[cfg(test)]
mod tests {
    use super::*;

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
