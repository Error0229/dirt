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

pub mod seal;
pub mod shelf;
pub mod thumbnail;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use chacha20poly1305::aead::{KeyInit, OsRng};
use chacha20poly1305::XChaCha20Poly1305;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zeroize::{Zeroize, ZeroizeOnDrop};

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
    fn a_key_is_not_printed() {
        let shown = format!("{:?}", photo());
        assert!(shown.contains("REDACTED"));
        assert!(!shown.contains(&STANDARD.encode(photo().key.as_bytes())));
    }
}
