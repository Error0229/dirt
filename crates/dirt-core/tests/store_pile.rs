//! The pile (design 6.17): every live note the device holds, in one call,
//! and what is worked out from the answer.

use dirt_core::models::{Grain, Layer, PileNote, PileSummary};
use dirt_core::notes::NoteDoc;
use dirt_core::{NoteId, Store};

fn temp_store() -> (tempfile::TempDir, Store) {
    let folder = tempfile::tempdir().unwrap();
    let store = Store::open(folder.path().join("notes.db")).unwrap();
    (folder, store)
}

#[tokio::test]
async fn the_pile_holds_every_live_note_and_no_deleted_one() {
    let (_folder, store) = temp_store();
    assert!(store.pile().await.unwrap().is_empty());

    let first = store.create_note("the first thought").await.unwrap();
    let put_away = store.create_note("done with this #done").await.unwrap();
    let gone = store.create_note("should not exist").await.unwrap();
    let last = store.create_note("the newest thought").await.unwrap();
    store.archive_note(&put_away.id).await.unwrap();
    store.delete_note(&gone.id).await.unwrap();

    let pile = store.pile().await.unwrap();
    assert_eq!(
        pile,
        [
            PileNote {
                id: first.id,
                edited_at: first.updated_at,
                archived: false
            },
            PileNote {
                id: put_away.id,
                edited_at: put_away.updated_at,
                archived: true
            },
            PileNote {
                id: last.id,
                edited_at: last.updated_at,
                archived: false
            },
        ]
    );
    // What the list and the archive hold between them, and no more.
    let counts = store.note_counts().await.unwrap();
    assert_eq!(pile.len(), counts.listed + counts.archived);
    for grain in &pile {
        let note = store.get_note(&grain.id).await.unwrap().unwrap();
        assert_eq!(grain.created_at(), note.created_at);
        assert_eq!(grain.edited_at, note.updated_at);
        assert_eq!(grain.archived, note.archived);
    }

    // An edit moves the edit time and not the note's place in the pile:
    // a grain lies where it fell.
    tokio::time::sleep(std::time::Duration::from_millis(3)).await;
    let edited = store
        .update_note(&first.id, "the first thought, again")
        .await
        .unwrap();
    let pile = store.pile().await.unwrap();
    assert_eq!(pile[0].id, first.id);
    assert_eq!(pile[0].edited_at, edited.updated_at);
    assert!(pile[0].edited_at > pile[0].created_at());

    // A note brought back from a delete is a grain again.
    store.restore_note(&gone.id).await.unwrap();
    let pile = store.pile().await.unwrap();
    assert_eq!(
        pile.iter().map(|grain| grain.id).collect::<Vec<_>>(),
        [first.id, put_away.id, gone.id, last.id]
    );

    let summary = PileSummary::of(&pile, 0);
    assert_eq!((summary.thoughts, summary.archived), (4, 1));
    assert!(summary.since.is_some());
    // Each is of today, whatever today is, and has its own shade.
    let now = dirt_core::util::now_ms();
    assert!(pile
        .iter()
        .all(|grain| Layer::of(grain.edited_at, now, 0) == Layer::Today));
    assert_eq!(pile[1].grain(), Grain::of(&put_away.id));
}

/// Notes that came from other devices lie in the table in the order they
/// arrived. The pile is in the order they were made all the same.
#[tokio::test]
async fn the_pile_is_oldest_first_however_the_notes_arrived() {
    let (_folder, store) = temp_store();
    let made_at = |ms: u64| {
        let time = uuid::Timestamp::from_unix(uuid::NoContext, ms / 1000, 0);
        NoteId::from_uuid(uuid::Uuid::new_v7(time))
    };
    // March, then January, then February, as a sync might bring them.
    let ids = [
        made_at(1_772_000_000_000),
        made_at(1_767_000_000_000),
        made_at(1_770_000_000_000),
    ];
    for (number, id) in ids.iter().enumerate() {
        let document = NoteDoc::new(7_100_000_000_000_001);
        document.set_text(&format!("from another device {number}"), 1_773_000_000_000);
        store
            .apply_remote_note(id, document.encode(), i64::try_from(number).unwrap() + 1)
            .await
            .unwrap();
    }
    let here = store.create_note("written here, now").await.unwrap();

    let pile = store.pile().await.unwrap();
    assert_eq!(
        pile.iter().map(|grain| grain.id).collect::<Vec<_>>(),
        [ids[1], ids[2], ids[0], here.id]
    );
    assert!(pile
        .windows(2)
        .all(|pair| pair[0].created_at() <= pair[1].created_at()));
    assert_eq!(pile[0].edited_at, 1_773_000_000_000);

    // The oldest was made on 29 December 2025, UTC.
    let summary = PileSummary::of(&pile, 0);
    let since = summary.since.unwrap();
    assert_eq!((since.year, since.month), (2025, 12));
    assert_eq!(since.name(), "December");
}

/// The counts' mark moves when the notes the pile is made of change, and
/// only then: an app reads the pile again when it moves, and not for
/// every edit.
#[tokio::test]
async fn the_counts_mark_moves_when_the_piles_notes_change() {
    let (_folder, store) = temp_store();
    let mark = || async { store.note_counts().await.unwrap().live_mark };
    assert_eq!(mark().await, 0);

    let first = store.create_note("the first thought").await.unwrap();
    let after_first = mark().await;
    assert_ne!(after_first, 0, "a note made");
    let second = store.create_note("the second thought").await.unwrap();
    let both = mark().await;
    assert_ne!(both, after_first, "another note made");

    store
        .update_note(&first.id, "the first thought, edited")
        .await
        .unwrap();
    assert_eq!(mark().await, both, "an edit");
    store.archive_note(&second.id).await.unwrap();
    assert_eq!(mark().await, both, "an archive");
    store.unarchive_note(&second.id).await.unwrap();
    assert_eq!(mark().await, both, "an archive undone");

    store.delete_note(&second.id).await.unwrap();
    assert_eq!(mark().await, after_first, "a delete");
    store.restore_note(&second.id).await.unwrap();
    assert_eq!(mark().await, both, "a delete undone");

    // One note made and another deleted between two readings: the number
    // of notes is the same, the mark is not.
    let counted = store.note_counts().await.unwrap();
    store.create_note("a third thought").await.unwrap();
    store.delete_note(&first.id).await.unwrap();
    let now = store.note_counts().await.unwrap();
    assert_eq!(now.listed + now.archived, counted.listed + counted.archived);
    assert_ne!(now.live_mark, counted.live_mark);
}
