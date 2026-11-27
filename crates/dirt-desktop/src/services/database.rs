//! Where the desktop app keeps its note stores.

use std::path::PathBuf;

use dirt_core::store::StoreName;
use dirt_core::{Result, Store};

/// The folder the desktop app and the CLI keep their stores in:
/// `DIRT_DATA_DIR` if it is set, which is for trying things out beside a
/// real installation, and otherwise `<data dir>/dirt`.
pub fn data_folder() -> PathBuf {
    if let Some(folder) = std::env::var_os("DIRT_DATA_DIR").filter(|value| !value.is_empty()) {
        return PathBuf::from(folder);
    }
    dirs::data_dir()
        .unwrap_or_else(|| panic!("Failed to resolve desktop data directory"))
        .join("dirt")
}

/// Opens the store that holds the notes kept on this device only, creating
/// it on first run. A build with no server uses nothing else.
pub fn open_local_store() -> Result<Store> {
    Store::open_in(&data_folder(), &StoreName::Local)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    /// Version 0.1.0 gave the app the identifier `dev.catjam.dirt`
    /// (`Dioxus.toml`), where development builds had `com.dirt.app`. What a
    /// computer that ran those builds already holds is found by names that
    /// do not come from the identifier, so it is found the same after the
    /// change: the data folder (`<data dir>/dirt`, shared with the CLI),
    /// the one-copy lock, and the web view's folder, which dioxus-desktop
    /// names after the program (`%LOCALAPPDATA%\dirt-desktop` on Windows).
    /// The keychain's entry is held by `dirt_core::secrets`' own test. Any
    /// of these moved would lose a person's notes or settings, or let two
    /// copies run; change one only with a move of what it names.
    /// The web view keeps its own data (what the page stores, its caches)
    /// where dioxus-desktop puts it when the app names no folder: on
    /// Windows `%LOCALAPPDATA%\<program>`, the program's name without
    /// `.exe`. So the program stays `dirt-desktop.exe` (the installer
    /// installs it under that name too) and the app names no folder of its
    /// own; either changed, the web view would begin again empty.
    #[test]
    fn the_web_views_folder_keeps_its_name() {
        assert_eq!(env!("CARGO_BIN_NAME"), "dirt-desktop");
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut stack = vec![source];
        while let Some(folder) = stack.pop() {
            for entry in std::fs::read_dir(folder).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|ending| ending == "rs") {
                    let text = std::fs::read_to_string(&path).unwrap();
                    assert!(
                        !text.contains(concat!("with_data", "_directory")),
                        "{} names the web view's folder: it would move",
                        path.display()
                    );
                }
            }
        }
    }

    #[test]
    fn what_a_computer_holds_is_not_named_after_the_bundle_identifier() {
        let dioxus =
            std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Dioxus.toml"))
                .unwrap();
        let identifier = dioxus
            .lines()
            .find_map(|line| line.trim().strip_prefix("identifier = "))
            .map(|value| value.trim_matches('"'))
            .expect("Dioxus.toml names the bundle's identifier");
        assert_eq!(identifier, "dev.catjam.dirt");

        if std::env::var_os("DIRT_DATA_DIR").is_none() {
            let folder = super::data_folder();
            assert_eq!(folder, dirs::data_dir().unwrap().join("dirt"));
        }
        assert_eq!(crate::SINGLE_INSTANCE, "dirt-desktop-single-instance");
        assert_eq!(env!("CARGO_PKG_NAME"), "dirt-desktop");
        for name in [crate::SINGLE_INSTANCE, env!("CARGO_PKG_NAME")] {
            assert!(!name.contains(identifier) && !name.contains("catjam"));
        }
    }
}
