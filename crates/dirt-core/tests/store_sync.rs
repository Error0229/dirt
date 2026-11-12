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
