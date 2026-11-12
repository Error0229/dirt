//! Files in the note document: attaching, removing and merging.
//!
//! A file never changes, so there is nothing to merge inside one. What
//! merges is the list of files a note names, and whether naming a file
//! counts as an edit that a delete has to have seen.

use dirt_core::files::{Attachment, FileKey};
use dirt_core::notes::NoteDoc;
use uuid::Uuid;

const LAPTOP: u64 = 7_100_000_000_000_001;
const PHONE: u64 = 7_100_000_000_000_002;

fn file(name: &str, added_at_ms: i64) -> Attachment {
    Attachment {
        file_id: Uuid::now_v7(),
        name: name.to_owned(),
        media_type: "image/jpeg".to_owned(),
        size: 1_234,
        key: FileKey::generate(),
        added_at_ms,
        thumbnail: None,
    }
}

fn names(doc: &NoteDoc) -> Vec<String> {
    doc.attachments()
        .into_iter()
        .map(|attachment| attachment.name)
        .collect()
}

/// A copy of the note on another device, as syncing would give it.
fn copy_for(device: u64, doc: &NoteDoc) -> NoteDoc {
    NoteDoc::from_bytes(device, &doc.encode()).unwrap()
}

#[test]
fn a_file_is_attached_listed_and_removed() {
    let doc = NoteDoc::new(LAPTOP);
    doc.set_text("the whiteboard", 1_000);
    assert!(doc.attachments().is_empty());

    let second = file("second.jpg", 3_000);
    let first = file("first.jpg", 2_000);
    assert!(doc.attach(&second, 3_000));
    assert!(doc.attach(&first, 3_500));

    // In the order they were attached, whatever order they were written in.
    assert_eq!(doc.attachments(), [first.clone(), second.clone()]);
    assert_eq!(doc.text(), "the whiteboard");
    // A file never changes: its ID cannot be given to another.
    let impostor = Attachment {
        file_id: first.file_id,
        ..file("impostor.jpg", 4_000)
    };
    assert!(!doc.attach(&impostor, 4_000));
    assert_eq!(names(&doc), ["first.jpg", "second.jpg"]);

    assert!(doc.detach(first.file_id, 5_000));
    assert_eq!(doc.attachments(), [second]);
    // Removing what is not there writes nothing.
    let before = doc.version_vector();
    assert!(!doc.detach(first.file_id, 6_000));
    assert!(!doc.detach(Uuid::now_v7(), 6_000));
    assert_eq!(doc.version_vector(), before);

    // It all survives being stored and loaded.
    assert_eq!(copy_for(PHONE, &doc).attachments(), doc.attachments());
}

#[test]
fn files_attached_on_two_devices_are_both_kept() {
    let laptop = NoteDoc::new(LAPTOP);
    laptop.set_text("the whiteboard", 1_000);
    let phone = copy_for(PHONE, &laptop);

    laptop.attach(&file("from the laptop.pdf", 2_000), 2_000);
    phone.attach(&file("from the phone.jpg", 2_500), 2_500);
    phone.set_text("the whiteboard, photographed", 2_600);

    // Merged in either order, both devices end with the same note.
    let (to_laptop, to_phone) = (phone.encode(), laptop.encode());
    laptop.merge(&to_laptop).unwrap();
    phone.merge(&to_phone).unwrap();
    for doc in [&laptop, &phone] {
        assert_eq!(names(doc), ["from the laptop.pdf", "from the phone.jpg"]);
        assert_eq!(doc.text(), "the whiteboard, photographed");
    }
    assert_eq!(laptop.attachments(), phone.attachments());
}

#[test]
fn a_file_removed_on_one_device_stays_removed_when_the_other_edits_the_text() {
    let laptop = NoteDoc::new(LAPTOP);
    laptop.set_text("the whiteboard", 1_000);
    let photo = file("photo.jpg", 2_000);
    laptop.attach(&photo, 2_000);
    let phone = copy_for(PHONE, &laptop);

    laptop.detach(photo.file_id, 3_000);
    phone.set_text("the whiteboard, with a caption", 3_100);

    laptop.merge(&phone.encode()).unwrap();
    phone.merge(&laptop.encode()).unwrap();
    for doc in [&laptop, &phone] {
        assert!(doc.attachments().is_empty());
        assert_eq!(doc.text(), "the whiteboard, with a caption");
    }
}

/// Attaching a file is an edit. A device that deletes the note without
/// having seen the file attached has not deleted that: the note comes back,
/// with the file. Otherwise a photograph taken on the phone could vanish
/// because the laptop deleted the note it was added to.
#[test]
fn a_delete_does_not_cover_a_file_it_had_not_seen() {
    let laptop = NoteDoc::new(LAPTOP);
    laptop.set_text("the whiteboard", 1_000);
    let phone = copy_for(PHONE, &laptop);

    phone.attach(&file("photo.jpg", 2_000), 2_000);
    laptop.delete();
    assert!(laptop.is_deleted());

    laptop.merge(&phone.encode()).unwrap();
    phone.merge(&laptop.encode()).unwrap();
    for doc in [&laptop, &phone] {
        assert!(!doc.is_deleted());
        assert_eq!(names(doc), ["photo.jpg"]);
    }

    // A delete made after the file was seen covers it. The document still
    // names the file, so that the note can be brought back whole.
    laptop.delete();
    phone.merge(&laptop.encode()).unwrap();
    assert!(phone.is_deleted());
    assert_eq!(names(&phone), ["photo.jpg"]);
}

/// Removing a file is an edit too: a delete made without having seen the
/// removal does not cover it.
#[test]
fn removing_a_file_brings_back_a_note_deleted_without_seeing_it() {
    let laptop = NoteDoc::new(LAPTOP);
    laptop.set_text("the whiteboard", 1_000);
    let photo = file("photo.jpg", 2_000);
    laptop.attach(&photo, 2_000);
    let phone = copy_for(PHONE, &laptop);

    phone.detach(photo.file_id, 3_000);
    laptop.delete();

    laptop.merge(&phone.encode()).unwrap();
    assert!(!laptop.is_deleted());
    assert!(laptop.attachments().is_empty());
}

#[test]
fn a_note_written_before_files_existed_has_none() {
    // Such a document has no map of files at all.
    let old = NoteDoc::new(LAPTOP);
    old.set_text("written long ago", 1_000);
    let loaded = copy_for(PHONE, &old);
    assert!(loaded.attachments().is_empty());
    // And takes one like any other.
    assert!(loaded.attach(&file("added later.png", 2_000), 2_000));
    assert_eq!(names(&loaded), ["added later.png"]);
}

/// What the store keeps and tells the server about is every file a note
/// names, thumbnails included.
#[test]
fn a_note_names_its_files_and_their_thumbnails() {
    let doc = NoteDoc::new(LAPTOP);
    let plain = file("notes.pdf", 1_000);
    let thumbnail = dirt_core::files::Thumbnail {
        file_id: Uuid::now_v7(),
        key: FileKey::generate(),
        size: 900,
    };
    let photo = Attachment {
        thumbnail: Some(thumbnail.clone()),
        ..file("photo.jpg", 2_000)
    };
    doc.attach(&plain, 1_000);
    doc.attach(&photo, 2_000);

    let mut named = doc.named_files();
    named.sort();
    let mut expected = vec![plain.file_id, photo.file_id, thumbnail.file_id];
    expected.sort();
    assert_eq!(named, expected);

    doc.detach(photo.file_id, 3_000);
    assert_eq!(doc.named_files(), [plain.file_id]);
}
