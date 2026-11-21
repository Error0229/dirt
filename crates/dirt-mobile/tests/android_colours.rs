//! The colours of the app's native parts (the capture sheet, the widget,
//! the tile) are the design's tokens, written once in `dirt_ui::tokens`.
//! These files are made from them; this test fails when they differ, and
//! with `DIRT_WRITE_ANDROID_COLOURS=1` writes them again.

use std::path::PathBuf;

use dirt_ui::tokens::{android_colours, Scheme};

fn file(folder: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("android/native/src/main/res")
        .join(folder)
        .join("dirt_colours.xml")
}

#[test]
fn colours_are_the_tokens() {
    let write = std::env::var_os("DIRT_WRITE_ANDROID_COLOURS").is_some();
    for (folder, scheme) in [("values", Scheme::Light), ("values-night", Scheme::Dark)] {
        let path = file(folder);
        let made = android_colours(scheme);
        if write {
            std::fs::write(&path, &made).unwrap();
        }
        let kept = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(
            kept == made,
            "{} is not what dirt_ui::tokens makes: run this test with DIRT_WRITE_ANDROID_COLOURS=1",
            path.display()
        );
    }
}

/// The widgets, the sheet and the tile name their colours only through
/// those files: no layout or drawable writes a colour of its own, except
/// white in an icon tinted where it is used.
#[test]
fn layouts_and_drawables_name_only_the_tokens() {
    let res = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("android/native/src/main/res");
    for folder in ["layout", "drawable"] {
        let entries = std::fs::read_dir(res.join(folder)).unwrap();
        for entry in entries {
            let path = entry.unwrap().path();
            let text = std::fs::read_to_string(&path).unwrap();
            for after in text.split("=\"#").skip(1) {
                let colour = after.split('"').next().unwrap_or_default();
                assert!(
                    colour == "FFFFFFFF",
                    "{} writes the colour #{colour}: name one of dirt_colours.xml",
                    path.display()
                );
            }
        }
    }
}
