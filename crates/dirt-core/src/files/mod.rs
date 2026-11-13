//! Files attached to notes.
//!
//! A file is immutable. Changing one means attaching another, so there is
//! never anything to merge. Each file has a random key of its own, and the
//! key lives inside the note's document, next to the file's name, type and
//! size. The server and the storage see none of those: the document is
//! encrypted like any note, and the file is encrypted with its key before
//! it leaves the device (design section 10).
//!
//! This module holds what a file is ([`Attachment`]), how its bytes are
//! encrypted ([`seal`]), where a device keeps them ([`shelf`]) and how a
//! picture gets its small preview ([`thumbnail`]).

pub mod copy;
pub mod seal;
pub mod shelf;
pub mod thumbnail;

pub use copy::{runs_as_a_program, save_copy};

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use chacha20poly1305::aead::{KeyInit, OsRng};
use chacha20poly1305::XChaCha20Poly1305;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// What kind of file a name suggests, as a media type. The name is all
/// there is to go by when a file is attached from a path.
///
/// It is a hint for the app that later opens the file, and nothing relies
/// on it being right: whether a file is a picture is read from the file.
#[must_use]
pub fn media_type_for(name: &str) -> &'static str {
    let extension = name
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "heic" => "image/heic",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "txt" | "log" => "text/plain",
        "md" => "text/markdown",
        "csv" => "text/csv",
        "html" | "htm" => "text/html",
        "json" => "application/json",
        "zip" => "application/zip",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "wav" => "audio/wav",
        "ogg" | "oga" => "audio/ogg",
        "opus" => "audio/opus",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        _ => "application/octet-stream",
    }
}

/// A name under which a file may be saved on this device, made from the
/// name its note gives it.
///
/// The name was typed, or taken from a path, on some device of the
/// account, and reaches this one inside a note. It is not trusted to be a
/// plain name: saved as it stands, `../../.profile` would be written
/// outside the folder the person chose. What comes back has no folder in
/// it, none of the characters a file system refuses, is not one of the
/// names Windows keeps for devices, and is short enough to be a file's
/// name anywhere.
#[must_use]
pub fn safe_file_name(name: &str) -> String {
    // Whatever stands after the last separator of either kind: a name
    // made on Windows may be saved on Linux, and the other way round.
    let last = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let cleaned: String = last
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
        .collect();
    // Windows drops dots and spaces from the end of a name, so `..` and
    // `con.` would come to mean something else.
    let ends = |name: &str| name.trim().trim_end_matches(['.', ' ']).to_owned();
    // Shortened, and then its end looked at again: the cut may have left
    // a dot or a space there.
    let cleaned = ends(&shortened(&ends(&cleaned)));
    let cleaned = cleaned.as_str();
    if cleaned.is_empty() {
        return "file".to_owned();
    }
    let stem = cleaned.split('.').next().unwrap_or_default().trim();
    let kept_for_a_device = matches!(
        stem.to_ascii_uppercase().as_str(),
        "CON" | "PRN" | "AUX" | "NUL"
    ) || (stem.is_ascii()
        && stem.len() == 4
        && (stem[..3].eq_ignore_ascii_case("COM") || stem[..3].eq_ignore_ascii_case("LPT"))
        && stem[3..].chars().all(|c| c.is_ascii_digit()));
    if kept_for_a_device {
        format!("_{cleaned}")
    } else {
        cleaned.to_owned()
    }
}

/// `name`, cut to what a file system takes as one name.
///
/// The limit is in bytes, as file systems count: 255 on most, and a name
/// in Chinese takes three bytes a character. 200 leaves room for what a
/// program may add to it, such as ` (1)`. The ending that says what kind
/// of file it is survives the cut, and no character is cut in two.
fn shortened(name: &str) -> String {
    const MOST_BYTES: usize = 200;
    /// Longer than this, what follows the last dot is not an ending.
    const MOST_ENDING_BYTES: usize = 16;
    if name.len() <= MOST_BYTES {
        return name.to_owned();
    }
    let (stem, ending) = match name.rsplit_once('.') {
        Some((stem, ending)) if !stem.is_empty() && ending.len() <= MOST_ENDING_BYTES => {
            (stem, Some(ending))
        }
        _ => (name, None),
    };
    let mut room = MOST_BYTES - ending.map_or(0, |ending| ending.len() + 1);
    while !stem.is_char_boundary(room) {
        room -= 1;
    }
    let stem = &stem[..room];
    ending.map_or_else(|| stem.to_owned(), |ending| format!("{stem}.{ending}"))
}

/// The key that encrypts one file.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct FileKey([u8; 32]);

impl FileKey {
    /// A new random key, for a file that is being attached.
    #[must_use]
    pub fn generate() -> Self {
        Self(XChaCha20Poly1305::generate_key(&mut OsRng).into())
    }

    /// A key read back from a note's document. `None` if it has the wrong
    /// length.
    #[must_use]
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        Some(Self(bytes.try_into().ok()?))
    }

    /// The raw key, for the note's document and for nothing else.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl std::fmt::Debug for FileKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FileKey([REDACTED])")
    }
}

impl PartialEq for FileKey {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for FileKey {}

/// A small picture of an image file, kept as a file of its own so that a
/// list of attachments can be shown without fetching the files themselves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thumbnail {
    pub file_id: Uuid,
    pub key: FileKey,
    /// The size of the thumbnail before encryption, in bytes.
    pub size: u64,
}

/// One file, as the document of the note it is attached to names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    /// Made on the device that attached the file. It names the file on the
    /// server and in the storage, which know nothing else about it.
    pub file_id: Uuid,
    /// The file's name, as the person would recognise it.
    pub name: String,
    /// What kind of file it is, such as `image/png`.
    pub media_type: String,
    /// The size of the file before encryption, in bytes.
    pub size: u64,
    pub key: FileKey,
    /// When it was attached, for the order attachments are shown in.
    pub added_at_ms: i64,
    pub thumbnail: Option<Thumbnail>,
}

/// One file as the storage holds it: an attachment, or its thumbnail.
/// Each is sent, fetched and kept by itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePart {
    pub file_id: Uuid,
    pub key: FileKey,
    /// The size before encryption, in bytes.
    pub size: u64,
}

impl FilePart {
    /// The size of the file as it is stored: encrypted.
    #[must_use]
    pub const fn stored_size(&self) -> u64 {
        seal::sealed_size(self.size)
    }
}

impl Attachment {
    /// The size of the file as it is stored: encrypted.
    #[must_use]
    pub const fn stored_size(&self) -> u64 {
        seal::sealed_size(self.size)
    }

    /// The files this attachment is made of: itself, and its thumbnail if
    /// it has one.
    #[must_use]
    pub fn parts(&self) -> Vec<FilePart> {
        let mut parts = vec![FilePart {
            file_id: self.file_id,
            key: self.key.clone(),
            size: self.size,
        }];
        if let Some(thumbnail) = &self.thumbnail {
            parts.push(FilePart {
                file_id: thumbnail.file_id,
                key: thumbnail.key.clone(),
                size: thumbnail.size,
            });
        }
        parts
    }

    /// The attachment as the note's document holds it, under its file ID.
    ///
    /// The names are short because the document is sent whole on every
    /// edit. A field this version does not know is ignored when read, so a
    /// later version can add one.
    pub(crate) fn to_entry(&self) -> String {
        let entry = Entry {
            n: self.name.clone(),
            t: self.media_type.clone(),
            s: self.size,
            k: STANDARD.encode(self.key.as_bytes()),
            a: self.added_at_ms,
            th: self.thumbnail.as_ref().map(|thumbnail| ThumbnailEntry {
                f: thumbnail.file_id,
                k: STANDARD.encode(thumbnail.key.as_bytes()),
                s: thumbnail.size,
            }),
        };
        serde_json::to_string(&entry).unwrap_or_default()
    }

    /// Reads an entry back. `None` for one that cannot be read: written by
    /// a version that changed the layout, or damaged.
    pub(crate) fn from_entry(file_id: &str, entry: &str) -> Option<Self> {
        let file_id = file_id.parse().ok()?;
        let entry: Entry = serde_json::from_str(entry).ok()?;
        let key = FileKey::from_bytes(&STANDARD.decode(entry.k).ok()?)?;
        // A thumbnail that cannot be read does not make the file unreadable.
        let thumbnail = entry.th.and_then(|thumbnail| {
            Some(Thumbnail {
                file_id: thumbnail.f,
                key: FileKey::from_bytes(&STANDARD.decode(thumbnail.k).ok()?)?,
                size: thumbnail.s,
            })
        });
        Some(Self {
            file_id,
            name: entry.n,
            media_type: entry.t,
            size: entry.s,
            key,
            added_at_ms: entry.a,
            thumbnail,
        })
    }
}

#[derive(Serialize, Deserialize)]
struct Entry {
    n: String,
    t: String,
    s: u64,
    k: String,
    a: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    th: Option<ThumbnailEntry>,
}

#[derive(Serialize, Deserialize)]
struct ThumbnailEntry {
    f: Uuid,
    k: String,
    s: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn photo() -> Attachment {
        Attachment {
            file_id: Uuid::now_v7(),
            name: "a photo of the whiteboard.jpg".to_owned(),
            media_type: "image/jpeg".to_owned(),
            size: 2_400_000,
            key: FileKey::generate(),
            added_at_ms: 1_700_000_000_000,
            thumbnail: Some(Thumbnail {
                file_id: Uuid::now_v7(),
                key: FileKey::generate(),
                size: 9_000,
            }),
        }
    }

    #[test]
    fn an_attachment_reads_back_from_its_entry() {
        let attachment = photo();
        let entry = attachment.to_entry();
        let read = Attachment::from_entry(&attachment.file_id.to_string(), &entry).unwrap();
        assert_eq!(read, attachment);

        let plain = Attachment {
            thumbnail: None,
            ..photo()
        };
        let read = Attachment::from_entry(&plain.file_id.to_string(), &plain.to_entry()).unwrap();
        assert_eq!(read, plain);
    }

    #[test]
    fn an_entry_from_a_later_version_is_read_for_what_this_one_knows() {
        let attachment = photo();
        let mut entry: serde_json::Value = serde_json::from_str(&attachment.to_entry()).unwrap();
        entry["something_new"] = serde_json::json!({ "x": 1 });
        let read = Attachment::from_entry(&attachment.file_id.to_string(), &entry.to_string());
        assert_eq!(read, Some(attachment));
    }

    #[test]
    fn an_entry_that_cannot_be_read_is_no_attachment() {
        let attachment = photo();
        let id = attachment.file_id.to_string();
        assert_eq!(Attachment::from_entry(&id, "not an entry"), None);
        assert_eq!(
            Attachment::from_entry("not an id", &attachment.to_entry()),
            None
        );
        // A key of the wrong length.
        let mut entry: serde_json::Value = serde_json::from_str(&attachment.to_entry()).unwrap();
        entry["k"] = serde_json::json!("c2hvcnQ=");
        assert_eq!(Attachment::from_entry(&id, &entry.to_string()), None);
    }

    #[test]
    fn a_thumbnail_that_cannot_be_read_leaves_the_file_readable() {
        let attachment = photo();
        let mut entry: serde_json::Value = serde_json::from_str(&attachment.to_entry()).unwrap();
        entry["th"]["k"] = serde_json::json!("c2hvcnQ=");
        let read =
            Attachment::from_entry(&attachment.file_id.to_string(), &entry.to_string()).unwrap();
        assert_eq!(read.thumbnail, None);
        assert_eq!(read.name, attachment.name);
    }

    #[test]
    fn a_name_says_what_kind_of_file_it_is() {
        assert_eq!(media_type_for("Whiteboard.JPG"), "image/jpeg");
        assert_eq!(media_type_for("minutes.final.pdf"), "application/pdf");
        assert_eq!(media_type_for("voice memo.m4a"), "audio/mp4");
        assert_eq!(media_type_for("no extension"), "application/octet-stream");
        assert_eq!(media_type_for("archive.xyz"), "application/octet-stream");
        assert_eq!(media_type_for(""), "application/octet-stream");
    }

    /// A name from a note is saved as a plain name in the folder the
    /// person chose, whatever it says.
    #[test]
    fn a_name_from_a_note_cannot_leave_the_folder_it_is_saved_in() {
        assert_eq!(safe_file_name("photo of it.jpg"), "photo of it.jpg");
        assert_eq!(safe_file_name("../../.profile"), ".profile");
        assert_eq!(safe_file_name("..\\..\\start.bat"), "start.bat");
        assert_eq!(safe_file_name("/etc/passwd"), "passwd");
        assert_eq!(safe_file_name("C:\\Windows\\system.ini"), "system.ini");
        assert_eq!(safe_file_name("C:evil.txt"), "Cevil.txt");
        // Nothing usable is left: a name is made up.
        for name in ["", "..", ".", "a/..", "   ", "???", "dir/"] {
            assert_eq!(safe_file_name(name), "file", "{name:?}");
        }
        // Names Windows keeps for devices. Written to, they are not files.
        assert_eq!(safe_file_name("NUL"), "_NUL");
        assert_eq!(safe_file_name("con.txt"), "_con.txt");
        assert_eq!(safe_file_name("LPT1.jpg"), "_LPT1.jpg");
        assert_eq!(safe_file_name("com9"), "_com9");
        assert_eq!(safe_file_name("console.txt"), "console.txt");
        assert_eq!(safe_file_name("command.com"), "command.com");
        // What a file system refuses, or shows as something else.
        assert_eq!(safe_file_name("a<b>c:d\"e|f?g*.txt"), "abcdefg.txt");
        assert_eq!(safe_file_name("line\nbreak\u{0}.txt"), "linebreak.txt");
        assert_eq!(safe_file_name("trailing. . "), "trailing");
        assert_eq!(safe_file_name(&"x".repeat(500)).len(), 200);
        // Names in other scripts are left as they are, whatever their
        // length in bytes.
        assert_eq!(safe_file_name("éé.txt"), "éé.txt");
        assert_eq!(safe_file_name("白板.jpg"), "白板.jpg");
    }

    /// A file system counts a name in bytes, and a name in another
    /// script takes several a character. A name is cut to fit, keeps the
    /// ending that says what it is, and is never cut inside a character.
    #[test]
    fn a_long_name_is_cut_to_what_a_file_system_takes() {
        // A hundred characters, three bytes each, and an ending.
        let long = format!("{}.txt", "白".repeat(100));
        assert_eq!(long.chars().count(), 104);
        let cut = safe_file_name(&long);
        assert!(cut.len() <= 200, "{} bytes", cut.len());
        assert_eq!(cut, format!("{}.txt", "白".repeat(65)));

        // Two bytes a character: the cut falls between two, not in one.
        let cut = safe_file_name(&format!("x{}.jpeg", "é".repeat(150)));
        assert!(cut.len() <= 200, "{} bytes", cut.len());
        assert_eq!(cut.rsplit_once('.').map(|(_, ending)| ending), Some("jpeg"));
        assert_eq!(cut.chars().filter(|c| *c == 'é').count(), 97);

        // No ending, or something after a dot that is too long to be one.
        assert_eq!(safe_file_name(&"白".repeat(100)), "白".repeat(66));
        let sentence = format!("see. {}", "y".repeat(300));
        assert_eq!(safe_file_name(&sentence).len(), 200);
        // A cut that leaves a dot or a space at the end loses it too.
        let spaced = format!("{} tail.{}", "z".repeat(199), "w".repeat(40));
        assert_eq!(safe_file_name(&spaced), "z".repeat(199));
        // Short names are as they were.
        assert_eq!(safe_file_name("白板.jpg"), "白板.jpg");
    }

    #[test]
    fn a_key_is_not_printed() {
        let shown = format!("{:?}", photo());
        assert!(shown.contains("REDACTED"));
        assert!(!shown.contains(&STANDARD.encode(photo().key.as_bytes())));
    }
}
