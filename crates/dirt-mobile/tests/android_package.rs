//! The app shortcuts name the capture sheet by the app's package, written
//! out in `dirt_shortcuts.xml`, since a resource cannot say
//! `${applicationId}`. The package is the bundle's identifier in
//! `Dioxus.toml`, which `dx` makes the applicationId (with none, it would
//! be `com.example.` and the crate's name in camel case, as development
//! builds before 0.1.0 were). This test fails if either side changes
//! without the other.

use std::path::PathBuf;

fn read(path: &str) -> String {
    std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path)).unwrap()
}

/// The identifier `[bundle]` gives in `Dioxus.toml`.
fn identifier() -> String {
    let dioxus = read("Dioxus.toml");
    let mut in_bundle = false;
    for line in dioxus.lines().map(str::trim) {
        if line.starts_with('[') {
            in_bundle = line == "[bundle]";
        } else if let Some(value) = line.strip_prefix("identifier") {
            if in_bundle {
                let value = value.trim_start().trim_start_matches('=').trim();
                return value.trim_matches('"').to_owned();
            }
        }
    }
    panic!("Dioxus.toml gives the app no identifier: dx would make one up");
}

#[test]
fn the_app_is_dev_catjam_dirt() {
    // Changing it makes another app on Android, beside this one: people
    // would have to sync, uninstall and sign in again (docs/RELEASING.md).
    assert_eq!(identifier(), "dev.catjam.dirt");
}

#[test]
fn the_shortcuts_name_the_apps_package() {
    let package = identifier();
    let shortcuts = read("android/native/src/main/res/xml/dirt_shortcuts.xml");
    let named = shortcuts.matches("android:targetPackage=").count();
    let right = shortcuts
        .matches(&format!("android:targetPackage=\"{package}\""))
        .count();
    assert!(named > 0, "dirt_shortcuts.xml names no package");
    assert_eq!(
        named, right,
        "dirt_shortcuts.xml does not name {package} in each shortcut"
    );
}

/// The launcher shows the native module's icon and name, not the ones `dx`
/// generates under its own default names (`ic_launcher`, `app_name`, which
/// says `DirtMobile`).
#[test]
fn the_launcher_shows_dirt_and_its_icon() {
    let manifest = read("android/AndroidManifest.xml");
    for wanted in [
        "android:icon=\"@mipmap/dirt_launcher\"",
        "android:roundIcon=\"@mipmap/dirt_launcher_round\"",
        "android:label=\"@string/dirt_app_name\"",
    ] {
        assert!(manifest.contains(wanted), "the manifest lacks {wanted}");
    }
    assert!(!manifest.contains("@string/app_name") && !manifest.contains("@mipmap/ic_launcher"));
    let strings = read("android/native/src/main/res/values/dirt_capture.xml");
    assert!(strings.contains("<string name=\"dirt_app_name\">Dirt</string>"));
    let res = "android/native/src/main/res";
    for density in ["mdpi", "hdpi", "xhdpi", "xxhdpi", "xxxhdpi"] {
        for name in ["dirt_launcher", "dirt_launcher_round"] {
            let png = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join(format!("{res}/mipmap-{density}/{name}.png"));
            assert!(
                png.is_file(),
                "{} is missing: run tools/icons/make-icons.mjs",
                png.display()
            );
        }
    }
    for name in ["dirt_launcher", "dirt_launcher_round"] {
        assert!(read(&format!("{res}/mipmap-anydpi-v26/{name}.xml")).contains("<adaptive-icon"));
    }
}
