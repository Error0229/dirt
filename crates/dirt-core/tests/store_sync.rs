//! What the store keeps for sync, through its public interface.
//!
//! The simulation tests in `dirt-sim` exercise these through whole sync
//! cycles against the real server. These tests pin each rule on its own.

use dirt_core::notes::NoteDoc;
use dirt_core::store::{ProblemKind, SyncProgress};
use dirt_core::{Error, NoteId, Store};

fn temp_store() -> (tempfile::TempDir, Store) {
    let folder = tempfile::tempdir().unwrap();
    let store = Store::open(folder.path().join("notes.db")).unwrap();
    (folder, store)
}

/// A document as another device would have written it.
fn remote_document(text: &str) -> Vec<u8> {
    let doc = NoteDoc::new(7_100_000_000_000_001);
    doc.set_text(text, 1_700_000_000_000);
    doc.encode()
}

#[tokio::test]
async fn a_new_or_edited_note_is_unsent_until_its_revision_is_pushed() {
    let (_folder, store) = temp_store();
    let note = store.create_note("draft").await.unwrap();

    let unsent = store.unsent_notes(0, 10, 1_000_000).await.unwrap();
    assert_eq!(unsent.len(), 1);
    assert_eq!(unsent[0].id, note.id);
    assert_eq!((unsent[0].revision, unsent[0].base_change), (1, 0));
    assert_eq!(
        NoteDoc::from_bytes(42, &unsent[0].document).unwrap().text(),
        "draft"
    );

    store.mark_note_pushed(&note.id, 1, 12).await.unwrap();
    assert_eq!(store.unsent_count().await.unwrap(), 0);
    assert_eq!(store.note_server_change(&note.id).await.unwrap(), Some(12));

    // An edit makes it unsent again, now based on the server's version.
    store.update_note(&note.id, "draft two").await.unwrap();
    let unsent = store.unsent_notes(0, 10, 1_000_000).await.unwrap();
    assert_eq!((unsent[0].revision, unsent[0].base_change), (2, 12));
}

#[tokio::test]
async fn an_edit_made_after_the_push_was_read_keeps_the_note_unsent() {
    let (_folder, store) = temp_store();
    let note = store.create_note("sent").await.unwrap();
    let sent = store
        .unsent_notes(0, 10, 1_000_000)
        .await
        .unwrap()
        .remove(0);

    // The push is in flight when the note changes again.
    store
        .update_note(&note.id, "sent, then edited")
        .await
        .unwrap();
    store
        .mark_note_pushed(&note.id, sent.revision, 5)
        .await
        .unwrap();

    let unsent = store.unsent_notes(0, 10, 1_000_000).await.unwrap();
    assert_eq!(unsent.len(), 1, "the later edit is still to be sent");
    assert_eq!((unsent[0].revision, unsent[0].base_change), (2, 5));
}

#[tokio::test]
async fn a_push_recorded_late_does_not_undo_a_newer_one() {
    let (_folder, store) = temp_store();
    let note = store.create_note("one").await.unwrap();
    store.update_note(&note.id, "one two").await.unwrap();

    // Two processes share the store. The server took revision 1 from one of
    // them as change 4, then revision 2 from the other as change 6, and the
    // first is the last to write down what happened.
    store.mark_note_pushed(&note.id, 2, 6).await.unwrap();
    store.mark_note_pushed(&note.id, 1, 4).await.unwrap();

    assert_eq!(store.unsent_count().await.unwrap(), 0);
    assert_eq!(store.note_server_change(&note.id).await.unwrap(), Some(6));
}

#[tokio::test]
async fn deleting_a_note_makes_it_unsent_so_the_delete_can_travel() {
    let (_folder, store) = temp_store();
    let note = store.create_note("to delete").await.unwrap();
    store.mark_note_pushed(&note.id, 1, 3).await.unwrap();

    store.delete_note(&note.id).await.unwrap();

    let unsent = store.unsent_notes(0, 10, 1_000_000).await.unwrap();
    assert_eq!(unsent.len(), 1);
    assert!(NoteDoc::from_bytes(42, &unsent[0].document)
        .unwrap()
        .is_deleted());
}

#[tokio::test]
async fn a_document_from_the_server_does_not_become_unsent() {
    let (_folder, store) = temp_store();
    let id = NoteId::new();

    // A note this device has never held arrives.
    let note = store
        .apply_remote_note(&id, remote_document("from another device"), 4)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(note.content, "from another device");
    assert_eq!(store.unsent_count().await.unwrap(), 0);
    assert_eq!(store.note_server_change(&id).await.unwrap(), Some(4));
    assert_eq!(store.search_notes("another", 10).await.unwrap().len(), 1);

    // A sent note of this device receives a newer version: still not unsent.
    let own = store.create_note("mine").await.unwrap();
    store.mark_note_pushed(&own.id, 1, 5).await.unwrap();
    let synced = store.note_documents().await.unwrap();
    let mine = &synced.iter().find(|(note, _)| *note == own.id).unwrap().1;
    let elsewhere = NoteDoc::from_bytes(7_100_000_000_000_001, mine).unwrap();
    elsewhere.set_text("mine, edited elsewhere", 1_800_000_000_000);
    store
        .apply_remote_note(&own.id, elsewhere.encode(), 6)
        .await
        .unwrap();
    assert_eq!(store.unsent_count().await.unwrap(), 0);
    assert_eq!(
        store.get_note(&own.id).await.unwrap().unwrap().content,
        "mine, edited elsewhere"
    );
}

#[tokio::test]
async fn an_unsent_note_stays_unsent_through_a_merge_and_builds_on_what_was_merged() {
    let (_folder, store) = temp_store();
    let note = store.create_note("buy milk today").await.unwrap();
    store.mark_note_pushed(&note.id, 1, 1).await.unwrap();
    let synced = store.note_documents().await.unwrap().remove(0).1;
    let elsewhere = NoteDoc::from_bytes(7_100_000_000_000_001, &synced).unwrap();
    elsewhere.set_text("buy bread today", 1_800_000_000_000);

    store
        .update_note(&note.id, "buy milk tomorrow")
        .await
        .unwrap();
    store
        .apply_remote_note(&note.id, elsewhere.encode(), 2)
        .await
        .unwrap();

    let unsent = store.unsent_notes(0, 10, 1_000_000).await.unwrap();
    assert_eq!(unsent.len(), 1);
    assert_eq!(unsent[0].base_change, 2);
    assert_eq!(
        NoteDoc::from_bytes(42, &unsent[0].document).unwrap().text(),
        "buy bread tomorrow"
    );
}

#[tokio::test]
async fn an_older_version_from_the_server_is_refused_and_the_same_one_is_harmless() {
    let (_folder, store) = temp_store();
    let id = NoteId::new();
    store
        .apply_remote_note(&id, remote_document("newest"), 9)
        .await
        .unwrap();

    let error = store
        .apply_remote_note(&id, remote_document("older"), 8)
        .await
        .unwrap_err();
    assert!(
        matches!(
            error,
            Error::Replay {
                seen: 9,
                got: 8,
                ..
            }
        ),
        "{error}"
    );
    assert_eq!(
        store.get_note(&id).await.unwrap().unwrap().content,
        "newest"
    );

    // The same change number again, as a repeated pull delivers it.
    store
        .apply_remote_note(&id, remote_document("newest"), 9)
        .await
        .unwrap();
    assert_eq!(store.note_server_change(&id).await.unwrap(), Some(9));
}

#[tokio::test]
async fn the_cursor_and_epoch_are_kept_and_a_new_epoch_starts_over() {
    let (_folder, store) = temp_store();
    assert_eq!(
        store.sync_progress().await.unwrap(),
        SyncProgress::default()
    );
    let note = store.create_note("survives an epoch").await.unwrap();
    store.begin_epoch(1).await.unwrap();
    store.mark_note_pushed(&note.id, 1, 40).await.unwrap();
    store.set_sync_cursor(40).await.unwrap();
    store
        .flag_note_problem(&NoteId::new(), "stuck".to_owned(), ProblemKind::Held)
        .await
        .unwrap();
    assert_eq!(
        store.sync_progress().await.unwrap(),
        SyncProgress {
            cursor: 40,
            epoch: 1
        }
    );

    store.begin_epoch(2).await.unwrap();

    assert_eq!(
        store.sync_progress().await.unwrap(),
        SyncProgress {
            cursor: 0,
            epoch: 2
        }
    );
    // Every note is offered again, as new, and old problems are forgotten.
    let unsent = store.unsent_notes(0, 10, 1_000_000).await.unwrap();
    assert_eq!(unsent.len(), 1);
    assert_eq!(unsent[0].base_change, 0);
    assert!(store.note_problems().await.unwrap().is_empty());
    // A change number lower than the old one is no longer a replay.
    let synced = store.note_documents().await.unwrap().remove(0).1;
    store.apply_remote_note(&note.id, synced, 1).await.unwrap();
}

#[tokio::test]
async fn a_flagged_note_is_not_pushed_and_only_the_right_event_clears_the_flag() {
    let (_folder, store) = temp_store();
    let too_large = store.create_note("refused by the server").await.unwrap();
    let unreadable = store.create_note("server copy unreadable").await.unwrap();
    let fine = store.create_note("fine").await.unwrap();
    store
        .flag_note_problem(
            &too_large.id,
            "too large to sync".to_owned(),
            ProblemKind::RetryAfterEdit,
        )
        .await
        .unwrap();
    store
        .flag_note_problem(
            &unreadable.id,
            "could not be decrypted".to_owned(),
            ProblemKind::Held,
        )
        .await
        .unwrap();

    let unsent = store.unsent_notes(0, 10, 1_000_000).await.unwrap();
    assert_eq!(unsent.len(), 1);
    assert_eq!(unsent[0].id, fine.id);
    assert_eq!(store.note_problems().await.unwrap().len(), 2);

    // Editing gives the refused note another try, but does not release the
    // note whose server copy could not be read.
    store.update_note(&too_large.id, "shorter").await.unwrap();
    store
        .update_note(&unreadable.id, "edited locally")
        .await
        .unwrap();
    let problems = store.note_problems().await.unwrap();
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0].note_id, unreadable.id);
    assert_eq!(problems[0].kind, ProblemKind::Held);
    assert_eq!(store.unsent_count().await.unwrap(), 2);

    // A readable copy from the server releases it.
    let synced = store.note_documents().await.unwrap();
    let document = synced
        .iter()
        .find(|(id, _)| *id == unreadable.id)
        .unwrap()
        .1
        .clone();
    store
        .apply_remote_note(&unreadable.id, document, 3)
        .await
        .unwrap();
    assert!(store.note_problems().await.unwrap().is_empty());
}

#[tokio::test]
async fn unsent_notes_come_in_pages_by_size_and_position() {
    let (_folder, store) = temp_store();
    for index in 0..5 {
        store
            .create_note(&format!("note {index} {}", "x".repeat(2000)))
            .await
            .unwrap();
    }

    // Each document is a little over 2,000 bytes, so two fit in 5,000.
    let first = store.unsent_notes(0, 10, 5000).await.unwrap();
    assert_eq!(first.len(), 2);
    let second = store
        .unsent_notes(first[1].position, 10, 5000)
        .await
        .unwrap();
    assert_eq!(second.len(), 2);
    let third = store
        .unsent_notes(second[1].position, 10, 5000)
        .await
        .unwrap();
    assert_eq!(third.len(), 1);
    assert!(store
        .unsent_notes(third[0].position, 10, 5000)
        .await
        .unwrap()
        .is_empty());

    // A note larger than the page still comes, alone, so nothing is stuck.
    assert_eq!(store.unsent_notes(0, 10, 100).await.unwrap().len(), 1);
    // The count limit applies too.
    assert_eq!(store.unsent_notes(0, 3, 1_000_000).await.unwrap().len(), 3);
}

#[tokio::test]
async fn forgetting_the_servers_version_offers_the_note_as_new() {
    let (_folder, store) = temp_store();
    let note = store.create_note("the server lost this").await.unwrap();
    store.mark_note_pushed(&note.id, 1, 7).await.unwrap();

    store.forget_note_server_version(&note.id).await.unwrap();

    let unsent = store.unsent_notes(0, 10, 1_000_000).await.unwrap();
    assert_eq!(unsent[0].base_change, 0);
}

// ---- Purging ----

/// A time comfortably after now, so that a note deleted in the test counts
/// as having been deleted long enough.
fn later() -> i64 {
    dirt_core::util::now_ms() + 60_000
}

async fn rows(store: &Store) -> usize {
    store.note_documents().await.unwrap().len()
}

#[tokio::test]
async fn a_note_may_be_purged_only_once_it_has_been_deleted_and_sent_for_long_enough() {
    let (_folder, store) = temp_store();
    let live = store.create_note("live").await.unwrap();
    let deleted = store.create_note("deleted").await.unwrap();
    store.mark_note_pushed(&live.id, 1, 1).await.unwrap();
    store.mark_note_pushed(&deleted.id, 1, 2).await.unwrap();
    assert!(store
        .purge_candidates(later(), 10)
        .await
        .unwrap()
        .is_empty());

    store.delete_note(&deleted.id).await.unwrap();
    // The delete has not been sent: the server would be asked to purge a
    // version it does not hold.
    assert!(store
        .purge_candidates(later(), 10)
        .await
        .unwrap()
        .is_empty());

    store.mark_note_pushed(&deleted.id, 2, 3).await.unwrap();
    let candidates = store.purge_candidates(later(), 10).await.unwrap();
    assert_eq!(candidates.len(), 1);
    assert_eq!(
        (
            candidates[0].id,
            candidates[0].revision,
            candidates[0].server_change
        ),
        (deleted.id, 2, 3)
    );
    // Not yet, by a clock that says it was deleted only just now.
    let a_minute_ago = dirt_core::util::now_ms() - 60_000;
    assert!(store
        .purge_candidates(a_minute_ago, 10)
        .await
        .unwrap()
        .is_empty());
    // No more than asked for.
    assert!(store.purge_candidates(later(), 0).await.unwrap().is_empty());

    // A note with a sync problem is left alone.
    store
        .flag_note_problem(&deleted.id, "held".to_owned(), ProblemKind::Held)
        .await
        .unwrap();
    assert!(store
        .purge_candidates(later(), 10)
        .await
        .unwrap()
        .is_empty());
    store.clear_note_problem(&deleted.id).await.unwrap();

    // A note that comes back is no longer one to purge, and if it is
    // deleted again the wait starts again.
    store.restore_note(&deleted.id).await.unwrap();
    store.mark_note_pushed(&deleted.id, 3, 4).await.unwrap();
    assert!(store
        .purge_candidates(later(), 10)
        .await
        .unwrap()
        .is_empty());
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let before_the_second_delete = dirt_core::util::now_ms() - 1;
    store.delete_note(&deleted.id).await.unwrap();
    store.mark_note_pushed(&deleted.id, 4, 5).await.unwrap();
    assert!(store
        .purge_candidates(before_the_second_delete, 10)
        .await
        .unwrap()
        .is_empty());
    assert_eq!(store.purge_candidates(later(), 10).await.unwrap().len(), 1);
}

#[tokio::test]
async fn a_delete_that_arrives_from_another_device_starts_the_wait_here() {
    let (_folder, store) = temp_store();
    let id = NoteId::new();
    let doc = NoteDoc::new(7_100_000_000_000_001);
    doc.set_text("written elsewhere", 1_700_000_000_000);
    store.apply_remote_note(&id, doc.encode(), 1).await.unwrap();
    assert!(store
        .purge_candidates(later(), 10)
        .await
        .unwrap()
        .is_empty());

    // The other device deleted it long ago by its own clock. This device
    // counts from when it saw the delete.
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let just_before = dirt_core::util::now_ms() - 1;
    doc.delete();
    store.apply_remote_note(&id, doc.encode(), 2).await.unwrap();

    assert!(store
        .purge_candidates(just_before, 10)
        .await
        .unwrap()
        .is_empty());
    let candidates = store.purge_candidates(later(), 10).await.unwrap();
    assert_eq!(candidates.len(), 1);
    assert_eq!((candidates[0].id, candidates[0].server_change), (id, 2));

    // The note is written again while it stays deleted: a third device
    // deleted it too. The wait is not started again by that.
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let after_the_delete_was_seen = dirt_core::util::now_ms() - 1;
    let third = NoteDoc::from_bytes(7_100_000_000_000_002, &doc.encode()).unwrap();
    third.delete();
    store
        .apply_remote_note(&id, third.encode(), 3)
        .await
        .unwrap();
    let candidates = store
        .purge_candidates(after_the_delete_was_seen, 10)
        .await
        .unwrap();
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].server_change, 3);
}

#[tokio::test]
async fn a_purged_note_is_dropped_unless_it_changed_after_it_was_listed() {
    let (_folder, store) = temp_store();
    let note = store.create_note("deleted #tag").await.unwrap();
    store.delete_note(&note.id).await.unwrap();
    store.mark_note_pushed(&note.id, 2, 1).await.unwrap();
    let listed = store.purge_candidates(later(), 10).await.unwrap()[0];

    // The person brings the note back while the purge is being asked for.
    store.restore_note(&note.id).await.unwrap();
    assert!(!store
        .drop_purged_note(&listed.id, listed.revision)
        .await
        .unwrap());
    assert_eq!(rows(&store).await, 1);
    assert_eq!(store.list_notes(10, 0).await.unwrap().len(), 1);

    // Deleted again and sent, it is listed afresh and can go.
    store.delete_note(&note.id).await.unwrap();
    store.mark_note_pushed(&note.id, 4, 2).await.unwrap();
    let listed = store.purge_candidates(later(), 10).await.unwrap()[0];
    assert!(store
        .drop_purged_note(&listed.id, listed.revision)
        .await
        .unwrap());
    assert_eq!(rows(&store).await, 0);
    assert_eq!(store.note_server_change(&note.id).await.unwrap(), None);
    // Asked again, there is nothing to drop.
    assert!(!store
        .drop_purged_note(&listed.id, listed.revision)
        .await
        .unwrap());
}

#[tokio::test]
async fn news_of_a_purge_removes_a_copy_that_holds_nothing_unsent() {
    use dirt_core::store::PurgeApplied;
    let (_folder, store) = temp_store();

    // A note this device does not hold.
    assert_eq!(
        store.apply_note_purge(&NoteId::new()).await.unwrap(),
        PurgeApplied::NotHeld
    );
    // A note it does not hold because it could never read it: a blob under
    // a key from before this device joined. It is listed as a problem, and
    // the purge is the last that will ever be heard of it.
    let unreadable = NoteId::new();
    store
        .flag_note_problem(
            &unreadable,
            "encrypted with a key this device does not have".to_owned(),
            ProblemKind::Held,
        )
        .await
        .unwrap();
    assert_eq!(store.note_problems().await.unwrap().len(), 1);
    assert_eq!(
        store.apply_note_purge(&unreadable).await.unwrap(),
        PurgeApplied::NotHeld
    );
    assert!(store.note_problems().await.unwrap().is_empty());

    // Live here, and sent: this device never saw the delete. What it holds
    // was in the version that was purged.
    let never_saw = store.create_note("deleted elsewhere #gone").await.unwrap();
    store.mark_note_pushed(&never_saw.id, 1, 1).await.unwrap();
    // Deleted here too, with the delete not yet sent.
    let also_deleted = store.create_note("deleted on both").await.unwrap();
    store
        .mark_note_pushed(&also_deleted.id, 1, 2)
        .await
        .unwrap();
    store.delete_note(&also_deleted.id).await.unwrap();
    // Held back by a sync problem: the problem goes with the note.
    let troubled = store.create_note("could not be read").await.unwrap();
    store.mark_note_pushed(&troubled.id, 1, 3).await.unwrap();
    store
        .flag_note_problem(&troubled.id, "held".to_owned(), ProblemKind::Held)
        .await
        .unwrap();
    let kept = store
        .create_note("nothing to do with it #kept")
        .await
        .unwrap();

    for id in [never_saw.id, also_deleted.id, troubled.id] {
        assert_eq!(
            store.apply_note_purge(&id).await.unwrap(),
            PurgeApplied::Dropped
        );
        assert_eq!(store.get_note(&id).await.unwrap(), None);
        assert_eq!(store.note_server_change(&id).await.unwrap(), None);
    }
    assert_eq!(rows(&store).await, 1);
    assert_eq!(store.list_notes(10, 0).await.unwrap()[0].id, kept.id);
    // Search, the tags and the problems no longer know the notes.
    assert!(store
        .find_notes("deleted", None, 10)
        .await
        .unwrap()
        .is_empty());
    assert_eq!(store.list_tags().await.unwrap(), [("kept".to_owned(), 1)]);
    assert!(store.note_problems().await.unwrap().is_empty());
    // Nothing is left unsent but the note that was never sent.
    assert_eq!(store.unsent_count().await.unwrap(), 1);
}

/// The one thing a purge cannot have covered is an edit this device has
/// not sent. If the note is not deleted here, the person wrote something
/// no other device has seen, and it is never thrown away.
#[tokio::test]
async fn news_of_a_purge_keeps_an_unsent_edit_as_a_new_note() {
    use dirt_core::store::PurgeApplied;
    let (_folder, store) = temp_store();
    let note = store.create_note("a thought").await.unwrap();
    store.mark_note_pushed(&note.id, 1, 1).await.unwrap();
    let edited = store
        .update_note(&note.id, "a thought, and more of it #more")
        .await
        .unwrap();

    let PurgeApplied::KeptAsNew(kept) = store.apply_note_purge(&note.id).await.unwrap() else {
        panic!("the edit should have been kept");
    };

    assert_ne!(kept, note.id);
    assert_eq!(store.get_note(&note.id).await.unwrap(), None);
    let new = store.get_note(&kept).await.unwrap().unwrap();
    assert_eq!(new.content, "a thought, and more of it #more");
    // It stays where it was in the list.
    assert_eq!(new.updated_at, edited.updated_at);
    assert_eq!(rows(&store).await, 1);
    // It is a new note to the server, found by search and by its tag.
    let unsent = store.unsent_notes(0, 10, 1_000_000).await.unwrap();
    assert_eq!(unsent.len(), 1);
    assert_eq!((unsent[0].id, unsent[0].base_change), (kept, 0));
    assert_eq!(
        store
            .find_notes("thought", Some("more"), 10)
            .await
            .unwrap()
            .len(),
        1
    );

    // A note that was never sent at all is kept the same way.
    let draft = store.create_note("never sent").await.unwrap();
    assert!(matches!(
        store.apply_note_purge(&draft.id).await.unwrap(),
        PurgeApplied::KeptAsNew(_)
    ));
    assert_eq!(rows(&store).await, 2);
}

/// A device that joins an account pulls notes that were deleted before it
/// came. It has never seen them live, and its wait starts when they arrive.
#[tokio::test]
async fn a_note_that_arrives_already_deleted_starts_its_wait_on_arrival() {
    let (_folder, store) = temp_store();
    let id = NoteId::new();
    let doc = NoteDoc::new(7_100_000_000_000_001);
    doc.set_text("deleted before this device joined", 1_700_000_000_000);
    doc.delete();
    let just_before = dirt_core::util::now_ms() - 1;

    store.apply_remote_note(&id, doc.encode(), 4).await.unwrap();

    assert!(store.list_notes(10, 0).await.unwrap().is_empty());
    assert!(store
        .purge_candidates(just_before, 10)
        .await
        .unwrap()
        .is_empty());
    let candidates = store.purge_candidates(later(), 10).await.unwrap();
    assert_eq!(candidates.len(), 1);
    assert_eq!((candidates[0].id, candidates[0].server_change), (id, 4));
}
