//! The app shortcut names the capture sheet by the app's package, written
//! out in `dirt_shortcuts.xml`, since a resource cannot say
//! `${applicationId}`. The package is the one `dx` gives the app: with no
//! identifier in `Dioxus.toml`, `com.example.` and the crate's name in camel
//! case (`com.example.DirtMobile`, which the built manifest showed). This
//! test fails if either side changes without the other: the crate renamed,
//! an identifier set, or the XML edited.

use std::path::PathBuf;

fn read(path: &str) -> String {
    std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path)).unwrap()
}

#[test]
fn the_shortcut_names_the_package_dx_gives_the_app() {
    let dioxus = read("Dioxus.toml");
    assert!(
        !dioxus
            .lines()
            .any(|line| line.trim_start().starts_with("identifier")),
        "Dioxus.toml sets an identifier: the shortcut's targetPackage must be that"
    );
    let camel: String = env!("CARGO_PKG_NAME")
        .split('-')
        .map(|word| {
            let mut letters = word.chars();
            letters.next().map_or_else(String::new, |first| {
                first.to_ascii_uppercase().to_string() + letters.as_str()
            })
        })
        .collect();
    let package = format!("com.example.{camel}");
    let shortcuts = read("android/native/src/main/res/xml/dirt_shortcuts.xml");
    assert!(
        shortcuts.contains(&format!("android:targetPackage=\"{package}\"")),
        "dirt_shortcuts.xml does not name {package}"
    );
}
