//! What the interface redesign asks of the store: archive, bringing a
//! deleted note back, and letting go of a note that was emptied again.
//!
//! The simulation tests in `dirt-sim` take these through whole sync cycles
//! against the real server. These pin each rule on its own.

use std::time::Duration;

use dirt_core::notes::NoteDoc;
use dirt_core::store::{LetGo, NoteCounts, PurgeApplied};
use dirt_core::{Error, Note, NoteId, Store};

fn temp_store() -> (tempfile::TempDir, Store) {
    let folder = tempfile::tempdir().unwrap();
    let store = Store::open(folder.path().join("notes.db")).unwrap();
    (folder, store)
}

/// Long enough for two notes to have different edit times.
async fn a_moment() {
    tokio::time::sleep(Duration::from_millis(3)).await;
}

fn texts(notes: &[Note]) -> Vec<&str> {
    notes.iter().map(|note| note.content.as_str()).collect()
}

/// The note's document as the store holds it, deleted or not.
async fn document(store: &Store, id: &NoteId) -> Option<NoteDoc> {
    store
        .note_documents()
        .await
        .unwrap()
        .into_iter()
        .find(|(held, _)| held == id)
        .map(|(_, bytes)| NoteDoc::from_bytes(42, &bytes).unwrap())
}

/// The note's copy on another device, which has everything this one has.
async fn elsewhere(store: &Store, id: &NoteId) -> NoteDoc {
    let bytes = store
        .note_documents()
        .await
        .unwrap()
        .into_iter()
        .find(|(held, _)| held == id)
        .unwrap()
        .1;
    NoteDoc::from_bytes(7_100_000_000_000_001, &bytes).unwrap()
}

/// Records that the server took the note as it is now, as change `change`.
async fn pushed(store: &Store, id: &NoteId, change: i64) {
    let revision = store.note_revision(id).await.unwrap().unwrap();
    store.mark_note_pushed(id, revision, change).await.unwrap();
}

// ---- Archive ----

#[tokio::test]
async fn an_archived_note_leaves_the_list_and_keeps_its_edit_time() {
    let (_folder, store) = temp_store();
    let done = store.create_note("bread was bought").await.unwrap();
    a_moment().await;
    let open = store.create_note("call the plumber").await.unwrap();
    a_moment().await;

    let archived = store.archive_note(&done.id).await.unwrap();

    assert!(archived.archived);
    assert_eq!(archived.updated_at, done.updated_at, "not an edit");
    assert_eq!(archived.content, done.content);
    let listed = store.list_notes(10, 0).await.unwrap();
    assert_eq!(listed, std::slice::from_ref(&open));
    assert_eq!(
        store.list_archived_notes(10, 0).await.unwrap(),
        std::slice::from_ref(&archived)
    );
    // It opens as any note does.
    assert_eq!(
        store.get_note(&done.id).await.unwrap(),
        Some(archived.clone())
    );
    assert_eq!(
        store.note_counts().await.unwrap(),
        NoteCounts {
            listed: 1,
            archived: 1
        }
    );
    assert_eq!(store.note_count().await.unwrap(), 2);
    // Everything there is: the list, then the archive.
    assert_eq!(
        store.list_every_note(10, 0).await.unwrap(),
        [open.clone(), archived]
    );
    assert_eq!(store.list_every_note(10, 1).await.unwrap().len(), 1);

    // And back, as it was.
    let back = store.unarchive_note(&done.id).await.unwrap();
    assert_eq!(back, done);
    assert_eq!(store.list_notes(10, 0).await.unwrap(), [open, done]);
    assert!(store.list_archived_notes(10, 0).await.unwrap().is_empty());
}

#[tokio::test]
async fn archived_notes_list_newest_edit_first_and_page() {
    let (_folder, store) = temp_store();
    let mut ids = Vec::new();
    for text in ["one", "two", "three"] {
        let note = store.create_note(text).await.unwrap();
        store.archive_note(&note.id).await.unwrap();
        ids.push(note.id);
        a_moment().await;
    }
    // Archiving in another order changes nothing: the order is of edits.
    store.unarchive_note(&ids[1]).await.unwrap();
    store.archive_note(&ids[1]).await.unwrap();
    assert_eq!(
        texts(&store.list_archived_notes(10, 0).await.unwrap()),
        ["three", "two", "one"]
    );

    // Editing an archived note leaves it archived and moves it up.
    let edited = store.update_note(&ids[0], "one, edited").await.unwrap();
    assert!(edited.archived);
    let archived = store.list_archived_notes(10, 0).await.unwrap();
    assert_eq!(texts(&archived), ["one, edited", "three", "two"]);
    assert_eq!(
        texts(&store.list_archived_notes(2, 1).await.unwrap()),
        ["three", "two"]
    );
    assert!(store.list_notes(10, 0).await.unwrap().is_empty());
}

#[tokio::test]
async fn archiving_a_note_that_is_deleted_or_not_here_is_not_found() {
    let (_folder, store) = temp_store();
    let note = store.create_note("gone").await.unwrap();
    store.delete_note(&note.id).await.unwrap();

    for id in [note.id, NoteId::new()] {
        let error = store.archive_note(&id).await.unwrap_err();
        assert!(matches!(error, Error::NotFound(_)), "{error}");
        let error = store.unarchive_note(&id).await.unwrap_err();
        assert!(matches!(error, Error::NotFound(_)), "{error}");
    }
    // The deleted note's document was not touched.
    assert!(!document(&store, &note.id).await.unwrap().is_archived());
}

#[tokio::test]
async fn a_search_finds_archived_notes_after_the_rest_and_marks_them() {
    let (_folder, store) = temp_store();
    // On its words alone the archived note is the better match.
    let archived = store.create_note("milk milk milk").await.unwrap();
    store.archive_note(&archived.id).await.unwrap();
    a_moment().await;
    let listed = store
        .create_note("a long list of things to buy on the way home, milk among them")
        .await
        .unwrap();

    let found = store.search_notes("milk", 10).await.unwrap();

    assert_eq!(
        found.iter().map(|note| note.id).collect::<Vec<_>>(),
        [listed.id, archived.id]
    );
    assert_eq!(
        found.iter().map(|note| note.archived).collect::<Vec<_>>(),
        [false, true]
    );
    // When there is room for one, it is the one that is not archived.
    let one = store.search_notes("milk", 1).await.unwrap();
    assert_eq!(one[0].id, listed.id);
}

#[tokio::test]
async fn finding_keeps_archived_notes_and_the_list_leaves_them_out() {
    let (_folder, store) = temp_store();
    let tap = store.create_note("fix the tap #home").await.unwrap();
    a_moment().await;
    let paint = store.create_note("paint the fence #home").await.unwrap();
    store.archive_note(&paint.id).await.unwrap();
    a_moment().await;
    let other = store.create_note("renew the passport").await.unwrap();

    // Nothing being found: the list.
    let list = store.find_notes("", None, 10).await.unwrap();
    assert_eq!(texts(&list), [other.content.as_str(), tap.content.as_str()]);

    // A tag, words, or both: archived matches after the rest.
    let by_tag = store.find_notes("", Some("#home"), 10).await.unwrap();
    assert_eq!(
        texts(&by_tag),
        [tap.content.as_str(), paint.content.as_str()]
    );
    assert!(by_tag[1].archived && !by_tag[0].archived);
    let by_word = store.find_notes("paint", None, 10).await.unwrap();
    assert_eq!(texts(&by_word), [paint.content.as_str()]);
    let by_both = store.find_notes("fence", Some("home"), 10).await.unwrap();
    assert_eq!(texts(&by_both), [paint.content.as_str()]);
    assert!(by_both[0].archived);

    // The number beside a tag is the number of notes that choosing it
    // shows.
    assert_eq!(
        store.list_tags().await.unwrap(),
        vec![("home".to_owned(), by_tag.len())]
    );

    // A list by tag is a list, and has its archive.
    assert_eq!(store.list_notes_by_tag("home", 10, 0).await.unwrap(), [tap]);
    let archived = store
        .list_archived_notes_by_tag("#Home", 10, 0)
        .await
        .unwrap();
    assert_eq!(texts(&archived), [paint.content.as_str()]);
}

#[tokio::test]
async fn an_archived_note_is_found_by_the_start_of_its_id() {
    let (_folder, store) = temp_store();
    let note = store.create_note("archived").await.unwrap();
    store.archive_note(&note.id).await.unwrap();

    assert_eq!(
        store
            .list_note_ids_by_prefix(&note.id.as_str()[..13], 10)
            .await
            .unwrap(),
        [note.id.to_string()]
    );
}

// ---- Archive and sync ----

#[tokio::test]
async fn archiving_makes_a_sent_note_unsent_though_it_is_not_an_edit() {
    let (_folder, store) = temp_store();
    let note = store.create_note("bread was bought").await.unwrap();
    pushed(&store, &note.id, 7).await;
    assert_eq!(store.unsent_count().await.unwrap(), 0);

    store.archive_note(&note.id).await.unwrap();

    let unsent = store.unsent_notes(0, 10, 1_000_000).await.unwrap();
    assert_eq!(unsent.len(), 1);
    assert_eq!((unsent[0].revision, unsent[0].base_change), (2, 7));
    let sent = NoteDoc::from_bytes(42, &unsent[0].document).unwrap();
    assert!(sent.is_archived());
    assert_eq!(sent.edited_at_ms(), Some(note.updated_at));

    // Asking again for what it already is changes nothing to send.
    store.archive_note(&note.id).await.unwrap();
    assert_eq!(store.note_revision(&note.id).await.unwrap(), Some(2));
    pushed(&store, &note.id, 8).await;
    assert_eq!(store.unsent_count().await.unwrap(), 0);

    // Bringing it back is to be sent as well.
    store.unarchive_note(&note.id).await.unwrap();
    let unsent = store.unsent_notes(0, 10, 1_000_000).await.unwrap();
    assert_eq!((unsent[0].revision, unsent[0].base_change), (3, 8));
}

#[tokio::test]
async fn an_archive_from_another_device_updates_the_row_and_is_not_unsent() {
    let (_folder, store) = temp_store();
    let note = store.create_note("bread was bought #shop").await.unwrap();
    pushed(&store, &note.id, 3).await;
    let other = elsewhere(&store, &note.id).await;
    other.set_archived(true);

    let merged = store
        .apply_remote_note(&note.id, other.encode(), 4)
        .await
        .unwrap()
        .expect("the note is not deleted");

    assert!(merged.archived);
    assert_eq!(merged.updated_at, note.updated_at);
    assert!(store.list_notes(10, 0).await.unwrap().is_empty());
    assert_eq!(store.list_archived_notes(10, 0).await.unwrap(), [merged]);
    assert_eq!(store.unsent_count().await.unwrap(), 0);

    // And the other device brings it back.
    other.set_archived(false);
    store
        .apply_remote_note(&note.id, other.encode(), 5)
        .await
        .unwrap();
    assert_eq!(store.list_notes(10, 0).await.unwrap(), [note]);
    assert_eq!(store.unsent_count().await.unwrap(), 0);
}

#[tokio::test]
async fn a_note_deleted_elsewhere_stays_deleted_when_it_is_archived_here() {
    let (_folder, store) = temp_store();
    let note = store.create_note("typed in the wrong place").await.unwrap();
    pushed(&store, &note.id, 3).await;
    let other = elsewhere(&store, &note.id).await;
    other.delete();

    // This device has not heard of the delete.
    store.archive_note(&note.id).await.unwrap();
    let merged = store
        .apply_remote_note(&note.id, other.encode(), 4)
        .await
        .unwrap();

    assert_eq!(merged, None, "the note is deleted");
    assert_eq!(store.get_note(&note.id).await.unwrap(), None);
    assert!(store.list_archived_notes(10, 0).await.unwrap().is_empty());
    assert!(store.search_notes("wrong", 10).await.unwrap().is_empty());
    // What this device still has to send is a deleted note too.
    let unsent = store.unsent_notes(0, 10, 1_000_000).await.unwrap();
    assert!(NoteDoc::from_bytes(42, &unsent[0].document)
        .unwrap()
        .is_deleted());
}

/// The server purged a note that had been deleted elsewhere for a long
/// time, and this device, which never heard of the delete, had archived
/// it. Archiving is not an edit, so there is nothing here that the delete
/// did not cover.
#[tokio::test]
async fn a_purged_note_that_was_only_archived_here_goes() {
    let (_folder, store) = temp_store();
    let note = store
        .create_note("deleted elsewhere long ago")
        .await
        .unwrap();
    pushed(&store, &note.id, 3).await;
    store.archive_note(&note.id).await.unwrap();
    assert_eq!(store.unsent_count().await.unwrap(), 1);

    assert_eq!(
        store.apply_note_purge(&note.id).await.unwrap(),
        PurgeApplied::Dropped
    );

    assert!(store.note_documents().await.unwrap().is_empty());
    assert_eq!(store.note_counts().await.unwrap(), NoteCounts::default());
}

/// An edit that the purge did not cover is kept as a new note, as always,
/// and the note is archived still.
#[tokio::test]
async fn a_purged_note_edited_and_archived_here_is_kept_as_a_new_archived_note() {
    let (_folder, store) = temp_store();
    for edit_first in [true, false] {
        let note = store.create_note("deleted elsewhere").await.unwrap();
        pushed(&store, &note.id, 3).await;
        if edit_first {
            store.update_note(&note.id, "written here").await.unwrap();
            store.archive_note(&note.id).await.unwrap();
        } else {
            store.archive_note(&note.id).await.unwrap();
            store.update_note(&note.id, "written here").await.unwrap();
        }

        let PurgeApplied::KeptAsNew(kept) = store.apply_note_purge(&note.id).await.unwrap() else {
            panic!("the unsent edit was thrown away");
        };

        let kept = store.get_note(&kept).await.unwrap().unwrap();
        assert_eq!(kept.content, "written here");
        assert!(kept.archived);
        assert_eq!(store.get_note(&note.id).await.unwrap(), None);
        store.delete_note(&kept.id).await.unwrap();
    }
}

/// An edit that was sent, and an archive after it that was not: the purge
/// covered the edit.
#[tokio::test]
async fn a_purged_note_whose_edit_was_sent_before_it_was_archived_goes() {
    let (_folder, store) = temp_store();
    let note = store.create_note("first").await.unwrap();
    pushed(&store, &note.id, 3).await;
    store.update_note(&note.id, "second").await.unwrap();
    // The push that took the edit was read before the note was archived,
    // and is recorded after.
    let sent = store.note_revision(&note.id).await.unwrap().unwrap();
    store.archive_note(&note.id).await.unwrap();
    store.mark_note_pushed(&note.id, sent, 4).await.unwrap();
    assert_eq!(store.unsent_count().await.unwrap(), 1);

    assert_eq!(
        store.apply_note_purge(&note.id).await.unwrap(),
        PurgeApplied::Dropped
    );
}

// ---- Bringing a deleted note back ----

#[tokio::test]
async fn a_deleted_note_is_brought_back_as_it_was() {
    let (_folder, store) = temp_store();
    let listed = store.create_note("keep after all #later").await.unwrap();
    let archived = store.create_note("archived, then deleted").await.unwrap();
    store.archive_note(&archived.id).await.unwrap();
    store.delete_note(&listed.id).await.unwrap();
    store.delete_note(&archived.id).await.unwrap();
    assert_eq!(store.note_count().await.unwrap(), 0);

    let back = store.restore_note(&listed.id).await.unwrap();
    assert_eq!(back.content, "keep after all #later");
    assert!(!back.archived);
    assert_eq!(
        store.list_notes(10, 0).await.unwrap(),
        std::slice::from_ref(&back)
    );
    assert_eq!(
        store.list_tags().await.unwrap(),
        vec![("later".to_owned(), 1)]
    );
    // Bringing back a note that is not deleted gives the note.
    assert_eq!(store.restore_note(&listed.id).await.unwrap(), back);

    // An archived note comes back to the archive, not to the list.
    let back = store.restore_note(&archived.id).await.unwrap();
    assert!(back.archived);
    assert_eq!(store.list_archived_notes(10, 0).await.unwrap(), [back]);
    assert_eq!(store.list_notes(10, 0).await.unwrap().len(), 1);
}

/// Undo after a delete puts the note back into its place in the list, not
/// at the top: it keeps the edit time it had (interface design, 6.19).
/// Bringing it back is still a change to be sent.
#[tokio::test]
async fn a_note_brought_back_comes_back_into_its_place() {
    let (_folder, store) = temp_store();
    let older = store.create_note("older").await.unwrap();
    a_moment().await;
    let middle = store.create_note("in the middle").await.unwrap();
    a_moment().await;
    store.create_note("newer").await.unwrap();
    pushed(&store, &middle.id, 1).await;
    store.delete_note(&middle.id).await.unwrap();
    a_moment().await;

    let back = store.restore_note(&middle.id).await.unwrap();
    assert_eq!(back.updated_at, middle.updated_at);
    assert_eq!(
        texts(&store.list_notes(10, 0).await.unwrap()),
        ["newer", "in the middle", "older"]
    );
    let unsent = store.unsent_notes(0, 10, usize::MAX).await.unwrap();
    assert!(
        unsent.iter().any(|note| note.id == middle.id),
        "bringing it back is sent"
    );
    assert_eq!(
        document(&store, &middle.id).await.unwrap().edited_at_ms(),
        Some(middle.updated_at)
    );
    // The oldest note is still the oldest.
    assert_eq!(
        store.get_note(&older.id).await.unwrap().unwrap().updated_at,
        older.updated_at
    );
}

#[tokio::test]
async fn a_note_this_device_no_longer_holds_cannot_be_brought_back() {
    let (_folder, store) = temp_store();
    let note = store.create_note("purged").await.unwrap();
    pushed(&store, &note.id, 2).await;
    store.delete_note(&note.id).await.unwrap();
    assert_eq!(
        store.apply_note_purge(&note.id).await.unwrap(),
        PurgeApplied::Dropped
    );

    let error = store.restore_note(&note.id).await.unwrap_err();
    assert!(matches!(error, Error::NotFound(_)), "{error}");
}

#[tokio::test]
async fn deleted_notes_are_found_by_the_start_of_their_id_for_bringing_back() {
    let (_folder, store) = temp_store();
    let live = store.create_note("live").await.unwrap();
    let deleted = store.create_note("deleted").await.unwrap();
    store.delete_note(&deleted.id).await.unwrap();
    // The start that both IDs share is their time; the whole ID tells them
    // apart.
    let (live_id, deleted_id) = (live.id.to_string(), deleted.id.to_string());

    assert_eq!(
        store
            .list_deleted_note_ids_by_prefix(&deleted_id, 10)
            .await
            .unwrap(),
        std::slice::from_ref(&deleted_id)
    );
    assert!(store
        .list_deleted_note_ids_by_prefix(&live_id, 10)
        .await
        .unwrap()
        .is_empty());
    assert_eq!(
        store
            .list_deleted_note_ids_by_prefix(&deleted_id[..4], 10)
            .await
            .unwrap(),
        std::slice::from_ref(&deleted_id)
    );
    assert_eq!(
        store
            .list_note_ids_by_prefix(&live_id[..4], 10)
            .await
            .unwrap(),
        [live_id]
    );
    assert!(store
        .list_deleted_note_ids_by_prefix("%", 10)
        .await
        .unwrap()
        .is_empty());

    store.restore_note(&deleted.id).await.unwrap();
    assert!(store
        .list_deleted_note_ids_by_prefix(&deleted_id, 10)
        .await
        .unwrap()
        .is_empty());
}

// ---- A note emptied again ----

#[tokio::test]
async fn a_note_emptied_before_it_was_ever_sent_is_taken_off_the_device() {
    let (_folder, store) = temp_store();
    let kept = store.create_note("a real thought").await.unwrap();
    // The first character typed makes the note, and it is saved as the
    // person types.
    let note = store.create_note("h").await.unwrap();
    store.update_note(&note.id, "hm #maybe").await.unwrap();
    store.update_note(&note.id, "").await.unwrap();

    assert_eq!(
        store.let_go_if_empty(&note.id).await.unwrap(),
        LetGo::Dropped
    );

    // No row, no document, nothing to sync, nothing to find.
    assert!(document(&store, &note.id).await.is_none());
    assert_eq!(store.note_revision(&note.id).await.unwrap(), None);
    let unsent = store.unsent_notes(0, 10, 1_000_000).await.unwrap();
    assert_eq!(unsent.len(), 1);
    assert_eq!(unsent[0].id, kept.id);
    assert_eq!(store.note_count().await.unwrap(), 1);
    assert!(store.list_tags().await.unwrap().is_empty());
    assert!(store
        .list_deleted_note_ids_by_prefix(&note.id.to_string(), 10)
        .await
        .unwrap()
        .is_empty());
    // Asked again, there is nothing to let go.
    assert_eq!(store.let_go_if_empty(&note.id).await.unwrap(), LetGo::Gone);
}

#[tokio::test]
async fn a_note_of_spaces_and_line_breaks_is_empty() {
    let (_folder, store) = temp_store();
    let note = store.create_note("x").await.unwrap();
    store.update_note(&note.id, "  \n\t \n").await.unwrap();

    assert_eq!(
        store.let_go_if_empty(&note.id).await.unwrap(),
        LetGo::Dropped
    );
}

#[tokio::test]
async fn a_note_with_text_or_a_file_is_kept() {
    let (folder, store) = temp_store();
    let note = store.create_note("x").await.unwrap();
    assert_eq!(store.let_go_if_empty(&note.id).await.unwrap(), LetGo::Kept);

    // No text, and a file: a photograph with nothing written under it is a
    // thought.
    let source = folder.path().join("whiteboard.txt");
    std::fs::write(&source, b"what was on the whiteboard").unwrap();
    let attached = store
        .attach_file(&note.id, &source, "whiteboard.txt", "text/plain")
        .await
        .unwrap();
    store.update_note(&note.id, "").await.unwrap();
    assert_eq!(store.let_go_if_empty(&note.id).await.unwrap(), LetGo::Kept);
    assert_eq!(store.get_note(&note.id).await.unwrap().unwrap().content, "");

    // The file taken away again, nothing is left.
    assert!(store.detach_file(&note.id, attached.file_id).await.unwrap());
    assert_eq!(
        store.let_go_if_empty(&note.id).await.unwrap(),
        LetGo::Dropped
    );
    assert!(store.files_in_use().await.unwrap().is_empty());
    assert!(store.notes_with_files().await.unwrap().is_empty());
}

/// A note the server has taken may be on another device by now. Emptied,
/// with this device offline, it is deleted, and the delete waits to be
/// sent.
#[tokio::test]
async fn a_note_that_was_pushed_and_then_emptied_is_deleted_like_any_other() {
    let (_folder, store) = temp_store();
    let note = store.create_note("on second thoughts").await.unwrap();
    pushed(&store, &note.id, 5).await;
    store.update_note(&note.id, "").await.unwrap();

    assert_eq!(
        store.let_go_if_empty(&note.id).await.unwrap(),
        LetGo::Deleted
    );

    assert_eq!(store.get_note(&note.id).await.unwrap(), None);
    let held = document(&store, &note.id).await.expect("the row stays");
    assert!(held.is_deleted());
    let unsent = store.unsent_notes(0, 10, 1_000_000).await.unwrap();
    assert_eq!(unsent.len(), 1);
    assert_eq!((unsent[0].id, unsent[0].base_change), (note.id, 5));
    assert!(NoteDoc::from_bytes(42, &unsent[0].document)
        .unwrap()
        .is_deleted());
    assert_eq!(store.let_go_if_empty(&note.id).await.unwrap(), LetGo::Gone);
}

/// A database put back from a copy made before the note's first push says
/// the note was never offered, though the server has it. The server's copy
/// arriving is proof enough that it was.
#[tokio::test]
async fn a_note_the_server_sends_back_has_been_offered() {
    let (_folder, store) = temp_store();
    let note = store.create_note("h").await.unwrap();
    let batch = store.unsent_notes(0, 10, 1_000_000).await.unwrap();
    assert!(!batch[0].offered, "a new note has not been offered");

    let servers = elsewhere(&store, &note.id).await;
    store
        .apply_remote_note(&note.id, servers.encode(), 7)
        .await
        .unwrap();

    store.update_note(&note.id, "").await.unwrap();
    assert_eq!(
        store.let_go_if_empty(&note.id).await.unwrap(),
        LetGo::Deleted,
        "the server holds it, so it is deleted and not simply forgotten"
    );
    assert!(document(&store, &note.id).await.unwrap().is_deleted());
}

/// The push that held the note went out and its answer never came. The
/// server may have the note, so it is not simply forgotten.
#[tokio::test]
async fn a_note_whose_first_push_was_only_begun_is_deleted_not_dropped() {
    let (_folder, store) = temp_store();
    let note = store.create_note("h").await.unwrap();
    let batch = store.unsent_notes(0, 10, 1_000_000).await.unwrap();
    assert!(!batch[0].offered, "a new note has not been offered");
    assert_eq!(
        store.offer_notes(vec![note.id]).await.unwrap(),
        [note.id],
        "the store holds it, so the push may send it"
    );
    let batch = store.unsent_notes(0, 10, 1_000_000).await.unwrap();
    assert!(batch[0].offered);

    store.update_note(&note.id, "").await.unwrap();
    assert_eq!(
        store.let_go_if_empty(&note.id).await.unwrap(),
        LetGo::Deleted
    );
    assert!(document(&store, &note.id).await.unwrap().is_deleted());
}

/// The note was let go between being read for a push and being offered.
/// The push is told so, and leaves it out.
#[tokio::test]
async fn a_note_let_go_before_its_push_was_offered_is_not_sent() {
    let (_folder, store) = temp_store();
    let stays = store.create_note("stays").await.unwrap();
    let goes = store.create_note("g").await.unwrap();
    let batch = store.unsent_notes(0, 10, 1_000_000).await.unwrap();
    assert_eq!(batch.len(), 2);

    store.update_note(&goes.id, "").await.unwrap();
    assert_eq!(
        store.let_go_if_empty(&goes.id).await.unwrap(),
        LetGo::Dropped
    );

    let held = store
        .offer_notes(batch.iter().map(|note| note.id).collect())
        .await
        .unwrap();
    assert_eq!(held, [stays.id]);
}

#[tokio::test]
async fn a_note_that_came_from_another_device_and_is_emptied_here_is_deleted() {
    let (_folder, store) = temp_store();
    let id = NoteId::new();
    let other = NoteDoc::new(7_100_000_000_000_001);
    other.set_text("written on the phone", 1_700_000_000_000);
    store
        .apply_remote_note(&id, other.encode(), 9)
        .await
        .unwrap();

    store.update_note(&id, "").await.unwrap();

    assert_eq!(store.let_go_if_empty(&id).await.unwrap(), LetGo::Deleted);
    assert_eq!(store.unsent_count().await.unwrap(), 1);
}

/// When the account starts over, every note is to be sent again and the
/// change numbers are forgotten. The notes are no less on other devices
/// for that.
#[tokio::test]
async fn a_new_epoch_does_not_make_a_sent_note_look_never_sent() {
    let (_folder, store) = temp_store();
    let note = store.create_note("sent before the reset").await.unwrap();
    pushed(&store, &note.id, 5).await;
    store.begin_epoch(2).await.unwrap();
    assert_eq!(store.note_server_change(&note.id).await.unwrap(), Some(0));

    store.update_note(&note.id, "").await.unwrap();

    assert_eq!(
        store.let_go_if_empty(&note.id).await.unwrap(),
        LetGo::Deleted
    );
}

#[tokio::test]
async fn there_is_nothing_to_let_go_of_a_note_that_is_deleted_or_not_here() {
    let (_folder, store) = temp_store();
    let note = store.create_note("").await.unwrap();
    store.delete_note(&note.id).await.unwrap();

    assert_eq!(store.let_go_if_empty(&note.id).await.unwrap(), LetGo::Gone);
    assert_eq!(
        store.let_go_if_empty(&NoteId::new()).await.unwrap(),
        LetGo::Gone
    );
    // The deleted note is still held, for the delete to sync.
    assert!(document(&store, &note.id).await.is_some());
}

/// Notes written before signing in are moved into the account's store when
/// the person signs in. There they are new, and have reached no server.
#[tokio::test]
async fn a_note_moved_into_the_account_has_not_been_offered_and_keeps_its_archive() {
    let folder = tempfile::tempdir().unwrap();
    let local = Store::open(folder.path().join("local.db")).unwrap();
    let account = Store::open(folder.path().join("account.db")).unwrap();
    let archived = local
        .create_note("archived while signed out")
        .await
        .unwrap();
    local.archive_note(&archived.id).await.unwrap();
    let draft = local.create_note("d").await.unwrap();

    assert_eq!(local.move_notes_into(&account).await.unwrap(), 2);

    let moved = account.list_archived_notes(10, 0).await.unwrap();
    assert_eq!(texts(&moved), ["archived while signed out"]);
    assert_eq!(moved[0].updated_at, archived.updated_at);
    account.update_note(&draft.id, "").await.unwrap();
    assert_eq!(
        account.let_go_if_empty(&draft.id).await.unwrap(),
        LetGo::Dropped
    );
}
