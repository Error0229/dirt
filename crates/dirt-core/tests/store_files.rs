//! Files in the local store: attaching one copies it onto the store's shelf
//! and names it in the note, in a way that syncs like any other edit.

use std::path::{Path, PathBuf};

use dirt_core::store::PurgeApplied;
use dirt_core::{Error, NoteId, Store};
use dirt_proto::api::limits;
use uuid::Uuid;

struct Desk {
    folder: tempfile::TempDir,
    store: Store,
}

fn desk() -> Desk {
    let folder = tempfile::tempdir().unwrap();
    let store = Store::open(folder.path().join("store").join("notes.db")).unwrap();
    Desk { folder, store }
}

impl Desk {
    /// A file somewhere else on the device, as the person has it.
    fn a_file(&self, name: &str, contents: &[u8]) -> PathBuf {
        let path = self.folder.path().join(name);
        std::fs::write(&path, contents).unwrap();
        path
    }
}

fn on_the_shelf(store: &Store, file: Uuid) -> Option<Vec<u8>> {
    std::fs::read(store.shelf().path(file)).ok()
}

fn files_on_the_shelf(store: &Store) -> usize {
    let folder = store.shelf().path(Uuid::nil());
    std::fs::read_dir(folder.parent().unwrap()).map_or(0, Iterator::count)
}

#[tokio::test]
async fn attaching_a_file_copies_it_and_names_it_in_the_note() {
    let desk = desk();
    let store = &desk.store;
    let note = store.create_note("the whiteboard").await.unwrap();
    store.mark_note_pushed(&note.id, 1, 1).await.unwrap();
    let source = desk.a_file("photo of it.jpg", b"not really a photograph");

    let attached = store
        .attach_file(&note.id, &source, "photo of it.jpg", "image/jpeg")
        .await
        .unwrap();

    assert_eq!(attached.name, "photo of it.jpg");
    assert_eq!(attached.media_type, "image/jpeg");
    assert_eq!(attached.size, 23);
    assert_eq!(
        store.note_attachments(&note.id).await.unwrap(),
        std::slice::from_ref(&attached)
    );
    // The store has its own copy. The person's file is as it was, and
    // changing or removing it afterwards changes nothing here.
    assert_eq!(
        on_the_shelf(store, attached.file_id).as_deref(),
        Some(&b"not really a photograph"[..])
    );
    std::fs::remove_file(&source).unwrap();
    assert!(store.shelf().holds(attached.file_id));

    // The note is as it was otherwise, and has a change to send.
    assert_eq!(
        store.get_note(&note.id).await.unwrap().unwrap().content,
        "the whiteboard"
    );
    let unsent = store.unsent_notes(0, 10, 1_000_000).await.unwrap();
    assert_eq!(unsent.len(), 1);
    assert_eq!((unsent[0].id, unsent[0].base_change), (note.id, 1));

    // Each file gets a key of its own.
    let again = desk.a_file("another.jpg", b"not really a photograph");
    let second = store
        .attach_file(&note.id, &again, "another.jpg", "image/jpeg")
        .await
        .unwrap();
    assert_ne!(second.file_id, attached.file_id);
    assert_ne!(second.key, attached.key);
    assert_eq!(store.note_attachments(&note.id).await.unwrap().len(), 2);
}

#[tokio::test]
async fn a_file_is_removed_from_the_note_and_kept_on_the_shelf() {
    let desk = desk();
    let store = &desk.store;
    let note = store.create_note("the whiteboard").await.unwrap();
    let source = desk.a_file("photo.jpg", b"bytes");
    let attached = store
        .attach_file(&note.id, &source, "photo.jpg", "image/jpeg")
        .await
        .unwrap();
    store.mark_note_pushed(&note.id, 2, 1).await.unwrap();

    assert!(store.detach_file(&note.id, attached.file_id).await.unwrap());

    assert!(store.note_attachments(&note.id).await.unwrap().is_empty());
    assert_eq!(store.unsent_count().await.unwrap(), 1);
    // Another device may not have fetched it yet, and the removal has yet
    // to sync. The file goes later, when nothing can need it.
    assert!(store.shelf().holds(attached.file_id));
    // Removing it again, or a file the note never had, changes nothing.
    store.mark_note_pushed(&note.id, 3, 2).await.unwrap();
    assert!(!store.detach_file(&note.id, attached.file_id).await.unwrap());
    assert!(!store.detach_file(&note.id, Uuid::now_v7()).await.unwrap());
    assert_eq!(store.unsent_count().await.unwrap(), 0);
}

#[tokio::test]
async fn a_file_that_cannot_be_attached_leaves_nothing_behind() {
    let desk = desk();
    let store = &desk.store;
    let note = store.create_note("the whiteboard").await.unwrap();
    store.mark_note_pushed(&note.id, 1, 1).await.unwrap();

    // A file larger than a file may be, once encrypted.
    let too_large = desk.folder.path().join("a film.mp4");
    let size = u64::try_from(limits::MAX_FILE_BYTES).unwrap();
    std::fs::File::create(&too_large)
        .unwrap()
        .set_len(size)
        .unwrap();
    let refused = store
        .attach_file(&note.id, &too_large, "a film.mp4", "video/mp4")
        .await;
    assert!(
        matches!(refused, Err(Error::InvalidInput(_))),
        "{refused:?}"
    );

    // A file that is not there.
    let missing = desk.folder.path().join("no such file");
    let refused = store
        .attach_file(&note.id, &missing, "x", "text/plain")
        .await;
    assert!(matches!(refused, Err(Error::Io(_))), "{refused:?}");

    // A note that does not exist, and one that is deleted.
    let source = desk.a_file("photo.jpg", b"bytes");
    let refused = store
        .attach_file(&NoteId::new(), &source, "photo.jpg", "image/jpeg")
        .await;
    assert!(matches!(refused, Err(Error::NotFound(_))), "{refused:?}");
    let deleted = store.create_note("deleted").await.unwrap();
    store.delete_note(&deleted.id).await.unwrap();
    let refused = store
        .attach_file(&deleted.id, &source, "photo.jpg", "image/jpeg")
        .await;
    assert!(matches!(refused, Err(Error::NotFound(_))), "{refused:?}");

    // None of that left a copy on the shelf, or a change to send.
    assert_eq!(files_on_the_shelf(store), 0);
    assert!(store.note_attachments(&note.id).await.unwrap().is_empty());
    assert_eq!(
        store.unsent_notes(0, 10, 1_000_000).await.unwrap().len(),
        1,
        "only the deleted note, which was never sent"
    );
}

#[tokio::test]
async fn a_note_has_only_so_many_files() {
    let desk = desk();
    let store = &desk.store;
    let note = store.create_note("many files").await.unwrap();
    let source = desk.a_file("small.txt", b"x");
    for _ in 0..limits::MAX_FILES_PER_NOTE {
        store
            .attach_file(&note.id, &source, "small.txt", "text/plain")
            .await
            .unwrap();
    }

    let refused = store
        .attach_file(&note.id, &source, "small.txt", "text/plain")
        .await;

    assert!(
        matches!(refused, Err(Error::InvalidInput(_))),
        "{refused:?}"
    );
    let files = usize::try_from(limits::MAX_FILES_PER_NOTE).unwrap();
    assert_eq!(store.note_attachments(&note.id).await.unwrap().len(), files);
    assert_eq!(files_on_the_shelf(store), files);
    // With one removed there is room again.
    let first = store.note_attachments(&note.id).await.unwrap()[0].file_id;
    store.detach_file(&note.id, first).await.unwrap();
    store
        .attach_file(&note.id, &source, "small.txt", "text/plain")
        .await
        .unwrap();
}

#[tokio::test]
async fn a_deleted_note_still_names_its_files() {
    let desk = desk();
    let store = &desk.store;
    let note = store.create_note("the whiteboard").await.unwrap();
    let source = desk.a_file("photo.jpg", b"bytes");
    let attached = store
        .attach_file(&note.id, &source, "photo.jpg", "image/jpeg")
        .await
        .unwrap();

    store.delete_note(&note.id).await.unwrap();

    // The note can come back, and its file with it.
    assert_eq!(
        store.note_attachments(&note.id).await.unwrap(),
        std::slice::from_ref(&attached)
    );
    assert!(store.shelf().holds(attached.file_id));
    store.restore_note(&note.id).await.unwrap();
    assert_eq!(store.note_attachments(&note.id).await.unwrap(), [attached]);
    // A note that is not there has no files to list.
    let missing = store.note_attachments(&NoteId::new()).await;
    assert!(matches!(missing, Err(Error::NotFound(_))));
}

/// A note that was purged while this device held an unsent edit to it is
/// kept as a new note (step 8e). The new note names the same files, under
/// the same IDs and keys: they are the same files, and need not be sent
/// again.
#[tokio::test]
async fn a_note_kept_as_new_after_a_purge_keeps_its_files() {
    let desk = desk();
    let store = &desk.store;
    let note = store.create_note("the whiteboard").await.unwrap();
    let source = desk.a_file("photo.jpg", b"bytes");
    let attached = store
        .attach_file(&note.id, &source, "photo.jpg", "image/jpeg")
        .await
        .unwrap();
    store.mark_note_pushed(&note.id, 2, 1).await.unwrap();
    store
        .update_note(&note.id, "the whiteboard, with a caption")
        .await
        .unwrap();

    let PurgeApplied::KeptAsNew(kept) = store.apply_note_purge(&note.id).await.unwrap() else {
        panic!("the unsent edit should have been kept");
    };

    assert_eq!(store.note_attachments(&kept).await.unwrap(), [attached]);
    assert_eq!(
        store.get_note(&kept).await.unwrap().unwrap().content,
        "the whiteboard, with a caption"
    );
}

/// The store knows which files its notes name without opening every
/// document, as it knows their tags. That is what says a file on the shelf
/// is still wanted.
#[tokio::test]
async fn the_store_knows_which_files_its_notes_name() {
    let desk = desk();
    let store = &desk.store;
    let source = desk.a_file("photo.jpg", b"bytes");
    let first = store.create_note("first").await.unwrap();
    let second = store.create_note("second").await.unwrap();
    let in_first = store
        .attach_file(&first.id, &source, "a.jpg", "image/jpeg")
        .await
        .unwrap()
        .file_id;
    let removed = store
        .attach_file(&first.id, &source, "b.jpg", "image/jpeg")
        .await
        .unwrap()
        .file_id;
    let in_second = store
        .attach_file(&second.id, &source, "c.jpg", "image/jpeg")
        .await
        .unwrap()
        .file_id;
    let mut expected = vec![in_first, removed, in_second];
    expected.sort();
    assert_eq!(store.files_in_use().await.unwrap(), expected);

    // A file removed from its note is named by nothing.
    store.detach_file(&first.id, removed).await.unwrap();
    // A deleted note still names its files.
    store.delete_note(&second.id).await.unwrap();
    let mut expected = vec![in_first, in_second];
    expected.sort();
    assert_eq!(store.files_in_use().await.unwrap(), expected);

    // A note that arrives from another device names its files too.
    let elsewhere = dirt_core::notes::NoteDoc::new(7_100_000_000_000_009);
    elsewhere.set_text("written elsewhere", 1_000);
    let arrived = dirt_core::files::Attachment {
        file_id: Uuid::now_v7(),
        name: "from elsewhere.pdf".to_owned(),
        media_type: "application/pdf".to_owned(),
        size: 10,
        key: dirt_core::files::FileKey::generate(),
        added_at_ms: 2_000,
        thumbnail: None,
    };
    elsewhere.attach(&arrived, 2_000);
    store
        .apply_remote_note(&NoteId::new(), elsewhere.encode(), 5)
        .await
        .unwrap();
    assert!(store
        .files_in_use()
        .await
        .unwrap()
        .contains(&arrived.file_id));

    // A note that is purged, with nothing unsent, names nothing any more.
    store.mark_note_pushed(&second.id, 3, 6).await.unwrap();
    assert_eq!(
        store.apply_note_purge(&second.id).await.unwrap(),
        PurgeApplied::Dropped
    );
    assert!(!store.files_in_use().await.unwrap().contains(&in_second));
    assert!(store.files_in_use().await.unwrap().contains(&in_first));
}

/// Where the files are: beside the store's database.
#[tokio::test]
async fn each_store_has_a_shelf_of_its_own() {
    let desk = desk();
    let other = Store::open(desk.folder.path().join("other").join("notes.db")).unwrap();
    let file = Uuid::now_v7();
    assert_eq!(
        desk.store.shelf().path(file).parent().unwrap(),
        Path::new(desk.folder.path()).join("store").join("files")
    );
    assert_ne!(desk.store.shelf().path(file), other.shelf().path(file));
}
