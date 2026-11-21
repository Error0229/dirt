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
/// those files. No layout writes a colour of its own, or one of Android's
/// (`@android:color/`), in any of its folders (`layout`, `layout-night`, and
/// so on); a drawable may write only white, in an icon tinted where it is
/// used.
#[test]
fn layouts_and_drawables_name_only_the_tokens() {
    let res = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("android/native/src/main/res");
    let mut looked_at = 0;
    for folder in std::fs::read_dir(&res).unwrap() {
        let folder = folder.unwrap().path();
        let name = folder.file_name().unwrap().to_string_lossy().into_owned();
        let drawable = name.starts_with("drawable");
        if !drawable && !name.starts_with("layout") {
            continue;
        }
        for entry in std::fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            let text = std::fs::read_to_string(&path).unwrap();
            looked_at += 1;
            assert!(
                !text.contains("@android:color/"),
                "{} names one of Android's colours: name one of dirt_colours.xml",
                path.display()
            );
            for colour in colour_literals(&text) {
                assert!(
                    drawable
                        && colour
                            .bytes()
                            .all(|digit| digit.eq_ignore_ascii_case(&b'f')),
                    "{} writes the colour #{colour}: name one of dirt_colours.xml",
                    path.display()
                );
            }
        }
    }
    assert!(
        looked_at > 0,
        "no layouts or drawables in {}",
        res.display()
    );
}

/// The colours written as `#` and hex digits in an attribute's value or an
/// element's text: what follows a `#` just after a quote or a `>`.
fn colour_literals(text: &str) -> Vec<&str> {
    text.match_indices('#')
        .filter(|(at, _)| text[..*at].trim_end().ends_with(['"', '\'', '>']))
        .map(|(at, _)| {
            let after = &text[at + 1..];
            let end = after
                .find(|digit: char| !digit.is_ascii_hexdigit())
                .unwrap_or(after.len());
            &after[..end]
        })
        .filter(|digits| matches!(digits.len(), 3 | 4 | 6 | 8))
        .collect()
}

#[test]
fn colour_literals_are_found_where_they_are_written() {
    let xml = r##"<shape a:color="#FF112233" b:tint= "#fff"><color>#80000000</color><!-- #315 --></shape>"##;
    assert_eq!(colour_literals(xml), ["FF112233", "fff", "80000000"]);
}
