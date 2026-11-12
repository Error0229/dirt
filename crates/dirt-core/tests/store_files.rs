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

/// Notes move between stores: into the account's store when the person
/// signs in, and back out of it when the account is deleted, after which
/// the account's folder is removed. A note's files have to make the same
/// journey, or the note arrives naming files that are gone.
#[tokio::test]
async fn notes_that_move_to_another_store_take_their_files_with_them() {
    let desk = desk();
    let local = &desk.store;
    let account = Store::open(desk.folder.path().join("account").join("notes.db")).unwrap();
    let source = desk.a_file("photo.jpg", b"the photograph");
    let note = local
        .create_note("written before signing in")
        .await
        .unwrap();
    let attached = local
        .attach_file(&note.id, &source, "photo.jpg", "image/jpeg")
        .await
        .unwrap();
    // A note that names a file this device has not fetched. It moves too.
    let elsewhere = dirt_core::notes::NoteDoc::new(7_100_000_000_000_009);
    elsewhere.set_text("written elsewhere", 1_000);
    let not_held = dirt_core::files::Attachment {
        file_id: Uuid::now_v7(),
        name: "not fetched yet.pdf".to_owned(),
        media_type: "application/pdf".to_owned(),
        size: 10,
        key: dirt_core::files::FileKey::generate(),
        added_at_ms: 2_000,
        thumbnail: None,
    };
    elsewhere.attach(&not_held, 2_000);
    let other = NoteId::new();
    local
        .apply_remote_note(&other, elsewhere.encode(), 3)
        .await
        .unwrap();

    assert_eq!(local.move_notes_into(&account).await.unwrap(), 2);

    assert_eq!(
        account.note_attachments(&note.id).await.unwrap(),
        std::slice::from_ref(&attached)
    );
    assert_eq!(
        on_the_shelf(&account, attached.file_id).as_deref(),
        Some(&b"the photograph"[..])
    );
    assert_eq!(
        account.note_attachments(&other).await.unwrap(),
        std::slice::from_ref(&not_held)
    );
    assert!(!account.shelf().holds(not_held.file_id));

    // The store the notes left can now go, folder and all, as an account's
    // does when the account is deleted.
    drop(desk.store);
    std::fs::remove_dir_all(desk.folder.path().join("store")).unwrap();
    assert_eq!(
        on_the_shelf(&account, attached.file_id).as_deref(),
        Some(&b"the photograph"[..])
    );

    // And back: the same move in the other direction.
    let again = Store::open(desk.folder.path().join("store").join("notes.db")).unwrap();
    assert_eq!(account.move_notes_into(&again).await.unwrap(), 2);
    assert_eq!(
        on_the_shelf(&again, attached.file_id).as_deref(),
        Some(&b"the photograph"[..])
    );
    assert_eq!(
        again.note_attachments(&note.id).await.unwrap(),
        std::slice::from_ref(&attached)
    );
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

// ---- What the store keeps for telling the server about files ----

use std::time::Duration;

use dirt_core::files::FilePart;
use dirt_core::store::FileLink;

const ACCOUNT: Uuid = Uuid::from_u128(0x0a11_ce00_0000_0000_0000_0000_0000_0001);

/// A note that the server holds, with a file attached after that.
async fn synced_note_with_file(desk: &Desk, text: &str) -> (NoteId, Uuid) {
    let store = &desk.store;
    let note = store.create_note(text).await.unwrap().id;
    let source = desk.a_file(&format!("{}.jpg", Uuid::now_v7()), b"a photograph");
    let file = store
        .attach_file(&note, &source, "photo.jpg", "image/jpeg")
        .await
        .unwrap()
        .file_id;
    (note, file)
}

/// Records that the server holds the note as this device does.
async fn sent(store: &Store, note: &NoteId, change: i64) {
    let revision = store.note_revision(note).await.unwrap().unwrap();
    store
        .mark_note_pushed(note, revision, change)
        .await
        .unwrap();
}

async fn to_confirm(store: &Store) -> Vec<(Uuid, NoteId)> {
    pairs(store.files_to_confirm(0, 100).await.unwrap())
}

async fn to_release(store: &Store, unnamed_before_ms: i64) -> Vec<(Uuid, NoteId)> {
    pairs(
        store
            .files_to_release(unnamed_before_ms, 100)
            .await
            .unwrap(),
    )
}

fn pairs(links: Vec<FileLink>) -> Vec<(Uuid, NoteId)> {
    links
        .into_iter()
        .map(|link| (link.file_id, link.note_id))
        .collect()
}

async fn link(store: &Store, file: Uuid) -> FileLink {
    store
        .files_to_confirm(0, 100)
        .await
        .unwrap()
        .into_iter()
        .find(|link| link.file_id == file)
        .unwrap()
}

const FAR_FUTURE: i64 = i64::MAX;

/// A reference is offered to the server once the server holds the note,
/// and not again once it has been taken.
#[tokio::test]
async fn a_file_is_confirmed_once_its_note_is_on_the_server() {
    let desk = desk();
    let store = &desk.store;
    let (note, file) = synced_note_with_file(&desk, "the whiteboard").await;

    // The server has never had the note: a reference to it would be
    // refused.
    assert!(to_confirm(store).await.is_empty());
    assert_eq!(store.unsent_file_count().await.unwrap(), 1);

    sent(store, &note, 4).await;
    assert_eq!(to_confirm(store).await, [(file, note)]);

    store
        .mark_file_confirmed(&link(store, file).await)
        .await
        .unwrap();
    assert!(to_confirm(store).await.is_empty());
    assert_eq!(store.unsent_file_count().await.unwrap(), 0);
    // An edit to the note's text changes nothing about its file.
    store
        .update_note(&note, "the whiteboard, later")
        .await
        .unwrap();
    assert!(to_confirm(store).await.is_empty());
    assert!(to_release(store, FAR_FUTURE).await.is_empty());
}

/// References are listed a page at a time, in an order that does not
/// change, so that those at the front which cannot be settled do not keep
/// the rest from being asked about.
#[tokio::test]
async fn references_to_confirm_are_listed_in_pages() {
    let desk = desk();
    let store = &desk.store;
    let mut all = Vec::new();
    for text in ["one", "two", "three"] {
        let (note, file) = synced_note_with_file(&desk, text).await;
        sent(store, &note, 1).await;
        all.push((file, note));
    }

    let first = store.files_to_confirm(0, 2).await.unwrap();
    assert_eq!(first.len(), 2);
    let rest = store.files_to_confirm(first[1].position, 2).await.unwrap();
    assert_eq!(rest.len(), 1);
    let mut listed = pairs(first);
    listed.extend(pairs(rest));
    assert_eq!(listed, all);
}

/// A file removed from a note is released only after this device has seen
/// the note without it for the wait, and only once the removal is on the
/// server. Until then the device keeps the file.
#[tokio::test]
async fn a_removed_file_is_released_after_the_wait_and_then_leaves_the_shelf() {
    let desk = desk();
    let store = &desk.store;
    let (note, file) = synced_note_with_file(&desk, "the whiteboard").await;
    sent(store, &note, 1).await;
    store
        .mark_file_confirmed(&link(store, file).await)
        .await
        .unwrap();
    let before = chrono::Utc::now().timestamp_millis() - 1;

    assert!(store.detach_file(&note, file).await.unwrap());

    // The removal has not been sent. Nothing may be told to the server.
    assert!(to_release(store, FAR_FUTURE).await.is_empty());
    sent(store, &note, 2).await;
    // Sent, and the wait is not over.
    assert!(to_release(store, before).await.is_empty());
    assert_eq!(store.clear_spare_files(Duration::ZERO).await.unwrap(), 0);
    assert!(
        store.shelf().holds(file),
        "kept while there is something to tell"
    );

    // The wait is over.
    let due = store.files_to_release(FAR_FUTURE, 100).await.unwrap();
    assert_eq!(pairs(due.clone()), [(file, note)]);
    // A note with a problem is not spoken for: what the server holds of
    // it is not what this device holds.
    store
        .flag_note_problem(
            &note,
            "held".to_owned(),
            dirt_core::store::ProblemKind::Held,
        )
        .await
        .unwrap();
    assert!(to_release(store, FAR_FUTURE).await.is_empty());
    store.clear_note_problem(&note).await.unwrap();

    store.forget_file_link(&due[0]).await.unwrap();
    assert!(to_release(store, FAR_FUTURE).await.is_empty());
    assert_eq!(store.clear_spare_files(Duration::ZERO).await.unwrap(), 1);
    assert!(!store.shelf().holds(file));
    assert!(store.get_note(&note).await.unwrap().is_some());
}

/// A file that was never confirmed is released all the same when it is
/// removed: the answer to a confirm may have been lost, and the server may
/// hold the reference.
#[tokio::test]
async fn a_file_removed_before_it_was_confirmed_is_still_released() {
    let desk = desk();
    let store = &desk.store;
    let (note, file) = synced_note_with_file(&desk, "the whiteboard").await;
    sent(store, &note, 1).await;

    store.detach_file(&note, file).await.unwrap();
    sent(store, &note, 2).await;

    // No longer to confirm, and not counted as waiting to be sent.
    assert!(to_confirm(store).await.is_empty());
    assert_eq!(store.unsent_file_count().await.unwrap(), 0);
    assert_eq!(to_release(store, FAR_FUTURE).await, [(file, note)]);
}

/// A file nothing needs is removed from the device only when it has
/// stayed unneeded for the wait. A file is put on the shelf a moment
/// before the note that names it is written, by this program or by another
/// with the same store open, and must not be taken for spare then.
#[tokio::test]
async fn a_spare_file_is_removed_only_after_it_has_stayed_spare() {
    let desk = desk();
    let store = &desk.store;
    let shelf = store.shelf();
    let arriving = Uuid::now_v7();
    shelf
        .add(
            arriving,
            &desk.a_file("arriving.jpg", b"about to be attached"),
        )
        .unwrap();
    let hour = Duration::from_secs(3_600);

    // Seen spare for the first time: noted, and left.
    assert_eq!(store.clear_spare_files(hour).await.unwrap(), 0);
    assert_eq!(store.clear_spare_files(hour).await.unwrap(), 0);
    assert!(shelf.holds(arriving));

    // A file that came to be needed is forgotten as spare, however long
    // ago it was first seen.
    let (note, file) = synced_note_with_file(&desk, "the whiteboard").await;
    assert_eq!(store.clear_spare_files(Duration::ZERO).await.unwrap(), 1);
    assert!(!shelf.holds(arriving));
    assert!(shelf.holds(file));

    // A deleted note still names its file.
    store.delete_note(&note).await.unwrap();
    assert_eq!(store.clear_spare_files(Duration::ZERO).await.unwrap(), 0);
    assert!(shelf.holds(file));

    // When the note goes, the file is spare, and the wait starts then.
    sent(store, &note, 1).await;
    let revision = store.note_revision(&note).await.unwrap().unwrap();
    assert!(store.drop_purged_note(&note, revision).await.unwrap());
    assert_eq!(store.clear_spare_files(hour).await.unwrap(), 0);
    assert!(shelf.holds(file));
    assert_eq!(store.clear_spare_files(Duration::ZERO).await.unwrap(), 1);
    assert!(!shelf.holds(file));
}

/// Something that stopped half-way through a file leaves a part of one.
/// It goes in time, and one being written now is left alone.
#[tokio::test]
async fn what_an_interrupted_fetch_left_is_cleared_in_time() {
    let desk = desk();
    let shelf = desk.store.shelf();
    let file = Uuid::now_v7();
    let unfinished = shelf.unfinished_of_my_own(file).unwrap();
    std::fs::write(&unfinished, b"half of").unwrap();
    assert_ne!(unfinished, shelf.unfinished_of_my_own(file).unwrap());

    let now = std::time::SystemTime::now();
    assert_eq!(
        shelf
            .clear_unfinished(now - Duration::from_secs(3_600))
            .unwrap(),
        0
    );
    assert!(unfinished.exists());
    assert_eq!(
        shelf
            .clear_unfinished(now + Duration::from_secs(3_600))
            .unwrap(),
        1
    );
    assert!(!unfinished.exists());
    assert!(shelf.list().unwrap().is_empty());
}

/// A new epoch: the server was reset or restored, and may know nothing of
/// the files it was told about. Every reference is offered again.
#[tokio::test]
async fn a_new_epoch_offers_every_reference_again() {
    let desk = desk();
    let store = &desk.store;
    let (note, file) = synced_note_with_file(&desk, "the whiteboard").await;
    sent(store, &note, 1).await;
    let reference = link(store, file).await;
    store
        .flag_file_problem(
            &reference,
            "the account's storage for files is full".to_owned(),
        )
        .await
        .unwrap();
    assert_eq!(store.file_problems().await.unwrap().len(), 1);
    // Refused: it is listed with its reason and not counted as waiting.
    // It is still a file that exists only here.
    assert_eq!(store.unsent_file_count().await.unwrap(), 0);
    assert_eq!(store.unsynced_file_count().await.unwrap(), 1);
    store.mark_file_confirmed(&reference).await.unwrap();
    assert!(store.file_problems().await.unwrap().is_empty());
    assert_eq!(store.unsynced_file_count().await.unwrap(), 0);

    store.begin_epoch(7).await.unwrap();

    // Not until the note is on the server again.
    assert!(to_confirm(store).await.is_empty());
    sent(store, &note, 1).await;
    assert_eq!(to_confirm(store).await, [(file, note)]);
    assert_eq!(store.unsent_file_count().await.unwrap(), 1);
}

/// A file is sealed for the account from the shelf, and what comes back
/// from the storage is kept only if it opens to the file the note names.
#[tokio::test]
async fn a_file_is_sealed_from_the_shelf_and_kept_only_if_it_opens() {
    let desk = desk();
    let store = &desk.store;
    let note = store.create_note("the whiteboard").await.unwrap().id;
    let contents: Vec<u8> = (0..200_000u32).map(|i| (i % 249) as u8).collect();
    let attached = store
        .attach_file(
            &note,
            &desk.a_file("photo.jpg", &contents),
            "photo.jpg",
            "image/jpeg",
        )
        .await
        .unwrap();
    let part = store
        .file_part(&note, attached.file_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!((part.size, &part.key), (attached.size, &attached.key));
    assert_eq!(store.file_part(&note, Uuid::now_v7()).await.unwrap(), None);
    assert_eq!(
        store
            .file_part(&NoteId::new(), attached.file_id)
            .await
            .unwrap(),
        None
    );

    let sealed = store.sealed_file(ACCOUNT, part.clone()).await.unwrap();
    assert_eq!(sealed.len() as u64, attached.stored_size());

    // Another device: it has the note and not the file.
    let other = Store::open(desk.folder.path().join("other").join("notes.db")).unwrap();
    // Sealed for another account, cut short, or changed: not kept.
    let mut changed = sealed.clone();
    changed[sealed.len() / 2] ^= 1;
    let elsewhere = Uuid::from_u128(2);
    for (account, bytes) in [
        (elsewhere, sealed.clone()),
        (ACCOUNT, sealed[..sealed.len() - 1].to_vec()),
        (ACCOUNT, changed),
        (ACCOUNT, Vec::new()),
    ] {
        let kept = other
            .keep_fetched_file(account, part.clone(), bytes)
            .await
            .unwrap();
        assert_eq!(kept, None);
        assert!(!other.shelf().holds(part.file_id));
        assert_eq!(
            files_on_the_shelf(&other),
            0,
            "nothing half-written is left"
        );
    }
    // The right file under a key that opens it, at another size than the
    // note says: not the file the note names.
    let shorter = FilePart {
        size: part.size - 1,
        ..part.clone()
    };
    assert_eq!(
        other
            .keep_fetched_file(ACCOUNT, shorter, sealed.clone())
            .await
            .unwrap(),
        None
    );

    let kept = other
        .keep_fetched_file(ACCOUNT, part.clone(), sealed.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(kept, other.shelf().path(part.file_id));
    assert_eq!(std::fs::read(kept).unwrap(), contents);
    // Fetched a second time, by another program say: the copy stays.
    assert!(other
        .keep_fetched_file(ACCOUNT, part.clone(), sealed)
        .await
        .unwrap()
        .is_some());
    assert_eq!(files_on_the_shelf(&other), 1);

    // A file that is not what the note says is not sent.
    std::fs::write(store.shelf().path(part.file_id), b"something else").unwrap();
    let error = store.sealed_file(ACCOUNT, part.clone()).await.unwrap_err();
    assert!(matches!(error, Error::InvalidInput(_)), "{error}");
    std::fs::remove_file(store.shelf().path(part.file_id)).unwrap();
    assert!(store.sealed_file(ACCOUNT, part).await.is_err());
}

/// A note that arrives from the server naming a file gives this device a
/// reference to confirm too: it may be the device that keeps the note as
/// new after a purge.
#[tokio::test]
async fn a_note_from_the_server_brings_its_references() {
    let desk = desk();
    let store = &desk.store;
    let elsewhere = dirt_core::notes::NoteDoc::new(7_100_000_000_000_009);
    elsewhere.set_text("written elsewhere", 1_000);
    let thumbnail = dirt_core::files::Thumbnail {
        file_id: Uuid::now_v7(),
        key: dirt_core::files::FileKey::generate(),
        size: 40,
    };
    let photo = dirt_core::files::Attachment {
        file_id: Uuid::now_v7(),
        name: "photo.jpg".to_owned(),
        media_type: "image/jpeg".to_owned(),
        size: 10,
        key: dirt_core::files::FileKey::generate(),
        added_at_ms: 2_000,
        thumbnail: Some(thumbnail.clone()),
    };
    elsewhere.attach(&photo, 2_000);
    let note = NoteId::new();
    store
        .apply_remote_note(&note, elsewhere.encode(), 3)
        .await
        .unwrap();

    let mut wanted = to_confirm(store).await;
    wanted.sort();
    let mut expected = vec![(photo.file_id, note), (thumbnail.file_id, note)];
    expected.sort();
    assert_eq!(wanted, expected);
    // This device holds neither file, so it has nothing of its own to
    // send, and nothing that deleting its store would lose.
    assert_eq!(store.unsent_file_count().await.unwrap(), 0);
    assert_eq!(store.unsynced_file_count().await.unwrap(), 0);
    // The thumbnail is a file with a key of its own.
    let part = store
        .file_part(&note, thumbnail.file_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!((part.size, part.key), (40, thumbnail.key));

    // The file is removed elsewhere. Both references are dated here.
    elsewhere.detach(photo.file_id, 4_000);
    store
        .apply_remote_note(&note, elsewhere.encode(), 5)
        .await
        .unwrap();
    assert!(to_confirm(store).await.is_empty());
    let mut due = to_release(store, FAR_FUTURE).await;
    due.sort();
    assert_eq!(due, expected);

    // The server is told, and before its answer is recorded the note
    // comes to name the file again. The reference is not forgotten: it is
    // offered again, because the server has just dropped it.
    let listed = store.files_to_release(FAR_FUTURE, 100).await.unwrap();
    let plain = dirt_core::files::Attachment {
        thumbnail: None,
        ..photo.clone()
    };
    elsewhere.attach(&plain, 6_000);
    store
        .apply_remote_note(&note, elsewhere.encode(), 7)
        .await
        .unwrap();
    for link in &listed {
        store.mark_file_confirmed(link).await.unwrap();
        store.forget_file_link(link).await.unwrap();
    }
    assert_eq!(to_confirm(store).await, [(photo.file_id, note)]);
    assert!(to_release(store, FAR_FUTURE).await.is_empty());
}
