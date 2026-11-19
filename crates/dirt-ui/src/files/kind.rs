//! What kind of file a file is, as the preview sees it (interface design,
//! section 6.16), and the short word for its kind on its tile (6.5).
//!
//! The name and the media type the note gives are all there is to go by:
//! nothing is read from the file to decide. A file taken for the wrong
//! kind shows what the web view makes of it, which is nothing harmful: a
//! picture that is not one shows no picture, and text is shown as text.

/// How the preview shows a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewKind {
    /// JPEG, PNG, GIF and WebP: the picture, as large as fits.
    Picture,
    /// A card with a play button and the sound's shape.
    Audio,
    /// The video, with the web view's own controls.
    Video,
    /// The text, in mono on a card.
    Text,
    /// The pages, drawn by the bundled renderer.
    Pdf,
}

/// Endings shown as text: plain text, data and code. A script among them
/// is shown, never run: showing it is how to look at one safely.
const TEXT_ENDINGS: &[&str] = &[
    "txt", "md", "markdown", "csv", "tsv", "json", "log", "ini", "cfg", "conf", "toml", "yaml",
    "yml", "xml", "html", "htm", "css", "rs", "js", "mjs", "ts", "tsx", "jsx", "py", "rb", "go",
    "java", "kt", "kts", "swift", "c", "h", "cc", "cpp", "hpp", "cs", "sh", "bash", "zsh", "ps1",
    "sql", "lua", "pl", "php", "r", "dart", "scala", "gradle", "diff", "patch", "srt", "vtt",
];

/// The file's ending, in lower case: what follows its last dot.
fn ending(name: &str) -> String {
    name.rsplit_once('.')
        .map(|(stem, ending)| {
            if stem.is_empty() {
                String::new()
            } else {
                ending.to_ascii_lowercase()
            }
        })
        .unwrap_or_default()
}

/// How a file of this name and media type is previewed, or `None` for a
/// file that is not: a click on its tile opens it in another app (6.16).
///
/// HEIC is not previewed: no web view the apps run in draws it. Nor is
/// a `.mov` of `QuickTime`'s, which Chromium does not promise to play, nor SVG,
/// which the design does not list.
#[must_use]
pub fn preview_kind(name: &str, media_type: &str) -> Option<PreviewKind> {
    let media_type = media_type.trim().to_ascii_lowercase();
    let ending = ending(name);
    match media_type.as_str() {
        "image/jpeg" | "image/png" | "image/gif" | "image/webp" => {
            return Some(PreviewKind::Picture)
        }
        "application/pdf" => return Some(PreviewKind::Pdf),
        "audio/mpeg" | "audio/mp4" | "audio/aac" | "audio/wav" | "audio/x-wav" | "audio/ogg"
        | "audio/opus" | "audio/webm" | "audio/flac" => return Some(PreviewKind::Audio),
        "video/mp4" | "video/webm" => return Some(PreviewKind::Video),
        _ => {}
    }
    match ending.as_str() {
        "jpg" | "jpeg" | "png" | "gif" | "webp" => Some(PreviewKind::Picture),
        "pdf" => Some(PreviewKind::Pdf),
        "mp3" | "m4a" | "aac" | "wav" | "ogg" | "oga" | "opus" | "flac" => Some(PreviewKind::Audio),
        "mp4" | "m4v" | "webm" => Some(PreviewKind::Video),
        ending if TEXT_ENDINGS.contains(&ending) => Some(PreviewKind::Text),
        _ if media_type.starts_with("text/") => Some(PreviewKind::Text),
        _ => None,
    }
}

/// The word for a file's kind on its tile: "JPEG", "PDF". The ending in
/// capitals, which is what most people know a kind by, and "File" for a
/// name with none.
#[must_use]
pub fn kind_word(name: &str) -> String {
    match ending(name).as_str() {
        "" => "File".to_owned(),
        "jpg" => "JPEG".to_owned(),
        ending if ending.len() > 8 => "File".to_owned(),
        ending => ending.to_ascii_uppercase(),
    }
}

/// The type the web view is told a file has when it is shown.
///
/// It is the one the note gives, unless that is empty or says nothing, and
/// then one taken from the kind. A picture or a sound with no type of its
/// own is still drawn or played by what is in it.
#[must_use]
pub fn served_type(name: &str, media_type: &str) -> String {
    let given = media_type.trim();
    if !given.is_empty() && given != "application/octet-stream" {
        return given.to_owned();
    }
    match preview_kind(name, given) {
        Some(PreviewKind::Text) => "text/plain; charset=utf-8".to_owned(),
        Some(PreviewKind::Pdf) => "application/pdf".to_owned(),
        _ => "application/octet-stream".to_owned(),
    }
}

/// A file's bytes as text to show: UTF-8, with anything that is not
/// written as the replacement character and a byte-order mark left off.
#[must_use]
pub fn as_text(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    String::from_utf8_lossy(bytes).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_kind_is_told_by_its_type_or_its_name() {
        use PreviewKind::{Audio, Pdf, Picture, Text, Video};
        let cases = [
            ("photo.JPG", "image/jpeg", Some(Picture)),
            ("photo", "image/webp", Some(Picture)),
            ("scan.png", "application/octet-stream", Some(Picture)),
            ("receipt.pdf", "application/pdf", Some(Pdf)),
            ("receipt", "application/pdf", Some(Pdf)),
            ("standup.m4a", "audio/mp4", Some(Audio)),
            ("voice.opus", "audio/opus", Some(Audio)),
            ("clip.mp4", "video/mp4", Some(Video)),
            ("packing-list.txt", "text/plain", Some(Text)),
            ("notes.md", "text/markdown", Some(Text)),
            ("data.json", "application/json", Some(Text)),
            ("main.rs", "application/octet-stream", Some(Text)),
            ("odd.weird", "text/x-something", Some(Text)),
            // Shown, never run.
            ("install.sh", "application/octet-stream", Some(Text)),
            // Not previewed: opened in another app.
            ("IMG_0001.HEIC", "image/heic", None),
            ("holiday.mov", "video/quicktime", None),
            ("logo.svg", "image/svg+xml", None),
            ("report.docx", "application/octet-stream", None),
            ("archive.zip", "application/zip", None),
            ("README", "application/octet-stream", None),
            (".txt", "application/octet-stream", None),
        ];
        for (name, media_type, kind) in cases {
            assert_eq!(preview_kind(name, media_type), kind, "{name} {media_type}");
        }
    }

    #[test]
    fn a_tile_names_the_kind_in_a_word() {
        assert_eq!(kind_word("receipt-oct.pdf"), "PDF");
        assert_eq!(kind_word("tile-sample.jpg"), "JPEG");
        assert_eq!(kind_word("kitchen.PNG"), "PNG");
        assert_eq!(kind_word("README"), "File");
        assert_eq!(kind_word("weird.averyverylongending"), "File");
    }

    #[test]
    fn text_is_read_as_utf8_whatever_is_in_it() {
        assert_eq!(
            as_text(b"\xEF\xBB\xBFPacking for Kyoto"),
            "Packing for Kyoto"
        );
        assert_eq!(as_text(b"caf\xC3\xA9"), "caf\u{e9}");
        assert_eq!(as_text(b"half \xFF"), "half \u{fffd}");
    }

    #[test]
    fn the_web_view_is_told_a_type_it_can_use() {
        assert_eq!(served_type("a.png", "image/png"), "image/png");
        assert_eq!(served_type("a.pdf", ""), "application/pdf");
        assert_eq!(
            served_type("main.rs", "application/octet-stream"),
            "text/plain; charset=utf-8"
        );
    }
}
