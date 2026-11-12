//! The local store, through its public interface, on a real file.

use dirt_core::models::{NoteId, Settings, ThemeMode};
use dirt_core::notes::NoteDoc;
use dirt_core::store::{store_path, StoreName};
use dirt_core::{Error, Store};

/// A store in a temporary folder. The folder is removed when the first value
/// is dropped, so keep it alive for the length of the test.
fn temp_store() -> (tempfile::TempDir, Store) {
    let folder = tempfile::tempdir().unwrap();
    let store = Store::open(folder.path().join("notes.db")).unwrap();
    (folder, store)
}

// ---- Create, read, update ----

#[tokio::test]
async fn a_created_note_can_be_read_back() {
    let (_folder, store) = temp_store();
    let note = store.create_note("buy milk").await.unwrap();

    assert_eq!(note.content, "buy milk");
    assert_eq!(note.created_at, note.id.created_at_ms().unwrap());
    assert!(note.updated_at >= note.created_at);
    assert_eq!(store.get_note(&note.id).await.unwrap(), Some(note));
}

#[tokio::test]
async fn a_note_that_does_not_exist_reads_as_none() {
    let (_folder, store) = temp_store();
    assert_eq!(store.get_note(&NoteId::new()).await.unwrap(), None);
}

#[tokio::test]
async fn an_update_replaces_the_text_and_moves_the_edit_time() {
    let (_folder, store) = temp_store();
    let note = store.create_note("first").await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(3)).await;

    let updated = store.update_note(&note.id, "second").await.unwrap();

    assert_eq!(updated.content, "second");
    assert_eq!(updated.created_at, note.created_at);
    assert!(updated.updated_at > note.updated_at);
    assert_eq!(store.get_note(&note.id).await.unwrap(), Some(updated));
}

#[tokio::test]
async fn updating_a_missing_note_is_not_found() {
    let (_folder, store) = temp_store();
    let error = store.update_note(&NoteId::new(), "text").await.unwrap_err();
    assert!(matches!(error, Error::NotFound(_)), "{error}");
}

#[tokio::test]
async fn notes_list_most_recently_edited_first_and_page() {
    let (_folder, store) = temp_store();
    let mut ids = Vec::new();
    for text in ["one", "two", "three"] {
        ids.push(store.create_note(text).await.unwrap().id);
        tokio::time::sleep(std::time::Duration::from_millis(3)).await;
    }
    // Editing the oldest note moves it to the front.
    store.update_note(&ids[0], "one, edited").await.unwrap();

    let all = store.list_notes(10, 0).await.unwrap();
    let texts: Vec<&str> = all.iter().map(|n| n.content.as_str()).collect();
    assert_eq!(texts, ["one, edited", "three", "two"]);

    let second_page = store.list_notes(2, 2).await.unwrap();
    assert_eq!(second_page.len(), 1);
    assert_eq!(second_page[0].content, "two");
}

// ---- Delete and restore ----

#[tokio::test]
async fn a_deleted_note_disappears_everywhere() {
    let (_folder, store) = temp_store();
    let note = store.create_note("secret #plan").await.unwrap();

    store.delete_note(&note.id).await.unwrap();

    assert_eq!(store.get_note(&note.id).await.unwrap(), None);
    assert!(store.list_notes(10, 0).await.unwrap().is_empty());
    assert!(store.search_notes("secret", 10).await.unwrap().is_empty());
    assert!(store
        .list_notes_by_tag("plan", 10, 0)
        .await
        .unwrap()
        .is_empty());
    assert!(store.list_tags().await.unwrap().is_empty());
    assert!(store
        .list_note_ids_by_prefix(&note.id.as_str()[..8], 10)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn deleting_twice_is_fine_and_deleting_a_missing_note_is_not_found() {
    let (_folder, store) = temp_store();
    let note = store.create_note("gone").await.unwrap();
    store.delete_note(&note.id).await.unwrap();
    store.delete_note(&note.id).await.unwrap();

    let error = store.delete_note(&NoteId::new()).await.unwrap_err();
    assert!(matches!(error, Error::NotFound(_)), "{error}");
}

#[tokio::test]
async fn a_deleted_note_cannot_be_edited() {
    let (_folder, store) = temp_store();
    let note = store.create_note("gone").await.unwrap();
    store.delete_note(&note.id).await.unwrap();

    let error = store.update_note(&note.id, "back?").await.unwrap_err();
    assert!(matches!(error, Error::NotFound(_)), "{error}");
}

#[tokio::test]
async fn a_restored_note_comes_back_with_its_text_tags_and_search_entry() {
    let (_folder, store) = temp_store();
    let note = store
        .create_note("call the #plumber tomorrow")
        .await
        .unwrap();
    store.delete_note(&note.id).await.unwrap();

    let restored = store.restore_note(&note.id).await.unwrap();

    assert_eq!(restored.id, note.id);
    assert_eq!(restored.content, "call the #plumber tomorrow");
    assert_eq!(store.search_notes("plumber", 10).await.unwrap().len(), 1);
    assert_eq!(
        store
            .list_notes_by_tag("plumber", 10, 0)
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        store.list_tags().await.unwrap(),
        vec![("plumber".to_owned(), 1)]
    );
}

#[tokio::test]
async fn restoring_a_live_note_changes_nothing() {
    let (_folder, store) = temp_store();
    let note = store.create_note("still here").await.unwrap();
    let before = store.note_revision(&note.id).await.unwrap();

    let restored = store.restore_note(&note.id).await.unwrap();

    assert_eq!(restored, note);
    assert_eq!(store.note_revision(&note.id).await.unwrap(), before);
}

// ---- Tags ----

#[tokio::test]
async fn tags_follow_the_text() {
    let (_folder, store) = temp_store();
    let a = store.create_note("#Work finish the #report").await.unwrap();
    store.create_note("#work call back").await.unwrap();

    assert_eq!(
        store.list_tags().await.unwrap(),
        vec![("report".to_owned(), 1), ("work".to_owned(), 2)]
    );
    assert_eq!(
        store.list_notes_by_tag("work", 10, 0).await.unwrap().len(),
        2
    );
    assert_eq!(
        store.list_notes_by_tag("#WORK", 10, 0).await.unwrap().len(),
        2
    );

    // Removing a tag from the text removes it from the index.
    store.update_note(&a.id, "#work finish it").await.unwrap();
    assert_eq!(
        store.list_tags().await.unwrap(),
        vec![("work".to_owned(), 2)]
    );
    assert!(store
        .list_notes_by_tag("report", 10, 0)
        .await
        .unwrap()
        .is_empty());
}

// ---- Search ----

#[tokio::test]
async fn search_needs_every_word_and_matches_word_starts() {
    let (_folder, store) = temp_store();
    store
        .create_note("buy oat milk on the way home")
        .await
        .unwrap();
    store
        .create_note("milk the idea for all it is worth")
        .await
        .unwrap();
    store.create_note("nothing relevant").await.unwrap();

    assert_eq!(store.search_notes("milk", 10).await.unwrap().len(), 2);
    assert_eq!(store.search_notes("milk home", 10).await.unwrap().len(), 1);
    assert_eq!(store.search_notes("mil", 10).await.unwrap().len(), 2);
    assert_eq!(store.search_notes("MILK", 10).await.unwrap().len(), 2);
    assert!(store.search_notes("cheese", 10).await.unwrap().is_empty());
}

#[tokio::test]
async fn search_follows_edits() {
    let (_folder, store) = temp_store();
    let note = store.create_note("draft about penguins").await.unwrap();
    store
        .update_note(&note.id, "draft about puffins")
        .await
        .unwrap();

    assert!(store.search_notes("penguins", 10).await.unwrap().is_empty());
    assert_eq!(store.search_notes("puffins", 10).await.unwrap().len(), 1);
}

#[tokio::test]
async fn punctuation_in_a_query_is_not_an_error() {
    let (_folder, store) = temp_store();
    store
        .create_note("email bob@example.com about C++")
        .await
        .unwrap();

    for query in [
        "\"",
        "(",
        "a AND",
        "bob@example.com",
        "C++",
        "#",
        "*",
        "NEAR(",
        "-x",
    ] {
        let result = store.search_notes(query, 10).await;
        assert!(result.is_ok(), "query {query:?} failed: {result:?}");
    }
    assert_eq!(
        store
            .search_notes("bob@example.com", 10)
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(store.search_notes("   ", 10).await.unwrap().is_empty());
}

#[tokio::test]
async fn search_finds_text_that_is_not_ascii() {
    let (_folder, store) = temp_store();
    store.create_note("Żółć über café").await.unwrap();
    assert_eq!(store.search_notes("café", 10).await.unwrap().len(), 1);
    assert_eq!(store.search_notes("über", 10).await.unwrap().len(), 1);
}

// ---- IDs by prefix ----

#[tokio::test]
async fn ids_are_found_by_prefix_and_odd_prefixes_match_nothing() {
    let (_folder, store) = temp_store();
    let note = store.create_note("find me").await.unwrap();
    let id = note.id.as_str();

    assert_eq!(
        store.list_note_ids_by_prefix(&id[..8], 10).await.unwrap(),
        vec![id.clone()]
    );
    assert_eq!(
        store
            .list_note_ids_by_prefix(&id.to_uppercase(), 10)
            .await
            .unwrap(),
        vec![id]
    );
    for prefix in ["", "%", "_", "zz", "0193 "] {
        let found = store.list_note_ids_by_prefix(prefix, 10).await.unwrap();
        assert!(
            found.is_empty() || prefix.trim() == "0193",
            "prefix {prefix:?} matched {found:?}"
        );
    }
}

// ---- Settings ----

#[tokio::test]
async fn settings_default_until_saved_and_then_round_trip() {
    let (_folder, store) = temp_store();
    assert_eq!(store.load_settings().await.unwrap(), Settings::default());

    let chosen = Settings {
        font_family: "JetBrains Mono".to_owned(),
        font_size: 18,
        theme: ThemeMode::Dark,
        capture_hotkey: "Ctrl+Alt+N".to_owned(),
    };
    store.save_settings(&chosen).await.unwrap();
    assert_eq!(store.load_settings().await.unwrap(), chosen);
}

// ---- Durability and sharing ----

#[tokio::test]
async fn notes_and_settings_survive_closing_and_reopening() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("notes.db");
    let id = {
        let store = Store::open(&path).unwrap();
        store
            .save_settings(&Settings {
                font_size: 20,
                ..Settings::default()
            })
            .await
            .unwrap();
        store.create_note("remember #this").await.unwrap().id
    };

    let store = Store::open(&path).unwrap();
    let note = store.get_note(&id).await.unwrap().unwrap();
    assert_eq!(note.content, "remember #this");
    assert_eq!(store.search_notes("remember", 10).await.unwrap().len(), 1);
    assert_eq!(
        store.list_tags().await.unwrap(),
        vec![("this".to_owned(), 1)]
    );
    assert_eq!(store.load_settings().await.unwrap().font_size, 20);
}

#[tokio::test]
async fn opening_creates_missing_folders() {
    let folder = tempfile::tempdir().unwrap();
    let path = store_path(folder.path(), &StoreName::Local);
    let store = Store::open(&path).unwrap();
    store.create_note("first").await.unwrap();
    assert!(path.exists());
    assert_eq!(store.path(), path);
}

#[tokio::test(flavor = "multi_thread")]
async fn two_handles_on_one_file_do_not_lose_each_others_writes() {
    // Two separate `Store`s on one file behave like the desktop app and the
    // CLI: each has its own connections. The two-process test covers real
    // processes; this covers the same code path quickly.
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("notes.db");
    let first = Store::open(&path).unwrap();
    let second = Store::open(&path).unwrap();

    let mut tasks = Vec::new();
    for (name, store) in [("first", first.clone()), ("second", second.clone())] {
        tasks.push(tokio::spawn(async move {
            for index in 0..100 {
                store.create_note(&format!("{name} {index}")).await.unwrap();
            }
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }

    assert_eq!(first.list_notes(1000, 0).await.unwrap().len(), 200);
    assert_eq!(second.list_notes(1000, 0).await.unwrap().len(), 200);
}

#[tokio::test(flavor = "multi_thread")]
async fn two_handles_rewriting_one_note_leave_it_whole() {
    // An update replaces the text as it stands at the moment of the write,
    // so of two handles rewriting one note the last write wins. What must
    // hold is that every write lands as a whole: the final text is one that
    // somebody wrote, both handles agree on it, and the document still loads.
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("notes.db");
    let first = Store::open(&path).unwrap();
    let second = Store::open(&path).unwrap();
    let id = first.create_note("start").await.unwrap().id;

    let mut tasks = Vec::new();
    for (name, store) in [("first", first.clone()), ("second", second.clone())] {
        tasks.push(tokio::spawn(async move {
            for index in 0..50 {
                store
                    .update_note(&id, &format!("{name} wrote version {index}"))
                    .await
                    .unwrap();
            }
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }

    let text = first.get_note(&id).await.unwrap().unwrap().content;
    assert!(
        text == "first wrote version 49" || text == "second wrote version 49",
        "unexpected final text: {text:?}"
    );
    assert_eq!(second.get_note(&id).await.unwrap().unwrap().content, text);
    assert_eq!(first.note_revision(&id).await.unwrap(), Some(101));
    let document = first.note_documents().await.unwrap().remove(0).1;
    assert_eq!(NoteDoc::from_bytes(42, &document).unwrap().text(), text);
    assert_eq!(first.search_notes("version", 10).await.unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn reads_keep_answering_while_writes_are_queued() {
    let (_folder, store) = temp_store();
    store.create_note("seed").await.unwrap();

    let writer = {
        let store = store.clone();
        tokio::spawn(async move {
            for index in 0..200 {
                store.create_note(&format!("note {index}")).await.unwrap();
            }
        })
    };
    let mut reads = 0;
    while !writer.is_finished() {
        assert!(!store.list_notes(5, 0).await.unwrap().is_empty());
        reads += 1;
    }
    writer.await.unwrap();
    assert!(reads > 0);
    assert_eq!(store.list_notes(1000, 0).await.unwrap().len(), 201);
}

// ---- Revisions ----

#[tokio::test]
async fn every_change_raises_the_revision_and_a_save_with_no_change_does_not() {
    let (_folder, store) = temp_store();
    let note = store.create_note("v1").await.unwrap();
    assert_eq!(store.note_revision(&note.id).await.unwrap(), Some(1));

    store.update_note(&note.id, "v2").await.unwrap();
    assert_eq!(store.note_revision(&note.id).await.unwrap(), Some(2));

    store.update_note(&note.id, "v2").await.unwrap();
    assert_eq!(store.note_revision(&note.id).await.unwrap(), Some(2));

    store.delete_note(&note.id).await.unwrap();
    assert_eq!(store.note_revision(&note.id).await.unwrap(), Some(3));

    store.restore_note(&note.id).await.unwrap();
    assert_eq!(store.note_revision(&note.id).await.unwrap(), Some(4));

    assert_eq!(store.note_revision(&NoteId::new()).await.unwrap(), None);
}

// ---- Documents and merging ----

#[tokio::test]
async fn the_stored_document_is_a_note_document_with_the_same_text() {
    let (_folder, store) = temp_store();
    let note = store.create_note("as stored").await.unwrap();

    let documents = store.note_documents().await.unwrap();
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].0, note.id);
    let doc = NoteDoc::from_bytes(42, &documents[0].1).unwrap();
    assert_eq!(doc.text(), "as stored");
    assert!(!doc.is_deleted());
}

#[tokio::test]
async fn an_edit_made_elsewhere_merges_with_a_local_edit() {
    let (_folder, store) = temp_store();
    let note = store.create_note("buy milk today").await.unwrap();
    let synced = store.note_documents().await.unwrap().remove(0).1;

    // Another device changes one word of the copy it received...
    let other = NoteDoc::from_bytes(7_100_000_000_000_001, &synced).unwrap();
    other.set_text("buy bread today", note.updated_at + 1);
    // ...while this device changes a different word.
    store
        .update_note(&note.id, "buy milk tomorrow")
        .await
        .unwrap();

    let merged = store
        .merge_note_document(&note.id, other.encode())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(merged.content, "buy bread tomorrow");
    assert_eq!(store.search_notes("bread", 10).await.unwrap().len(), 1);
    assert!(store.search_notes("milk", 10).await.unwrap().is_empty());
}

#[tokio::test]
async fn merging_the_same_document_twice_changes_nothing() {
    let (_folder, store) = temp_store();
    let note = store.create_note("stable").await.unwrap();
    let same = store.note_documents().await.unwrap().remove(0).1;
    let before = store.note_revision(&note.id).await.unwrap();

    let merged = store.merge_note_document(&note.id, same).await.unwrap();

    assert_eq!(merged, Some(note.clone()));
    assert_eq!(store.note_revision(&note.id).await.unwrap(), before);
}

#[tokio::test]
async fn a_delete_made_elsewhere_arrives_through_a_merge() {
    let (_folder, store) = temp_store();
    let note = store
        .create_note("to be deleted elsewhere #tag")
        .await
        .unwrap();
    let synced = store.note_documents().await.unwrap().remove(0).1;
    let other = NoteDoc::from_bytes(7_100_000_000_000_001, &synced).unwrap();
    other.delete();

    let merged = store
        .merge_note_document(&note.id, other.encode())
        .await
        .unwrap();

    assert_eq!(merged, None);
    assert_eq!(store.get_note(&note.id).await.unwrap(), None);
    assert!(store.list_tags().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_document_that_is_not_one_is_refused_and_nothing_is_stored() {
    let (_folder, store) = temp_store();
    let id = NoteId::new();
    let error = store
        .merge_note_document(&id, b"not a document".to_vec())
        .await
        .unwrap_err();
    assert!(matches!(error, Error::Document(_)), "{error}");
    assert!(store.note_documents().await.unwrap().is_empty());
}

// ---- Moving local notes into an account ----

#[tokio::test]
async fn local_notes_move_into_an_account_store() {
    let folder = tempfile::tempdir().unwrap();
    let local = Store::open(store_path(folder.path(), &StoreName::Local)).unwrap();
    let account = Store::open(store_path(
        folder.path(),
        &StoreName::account("user_123").unwrap(),
    ))
    .unwrap();
    let kept = local
        .create_note("written before sign-in #idea")
        .await
        .unwrap();
    let deleted = local.create_note("deleted before sign-in").await.unwrap();
    local.delete_note(&deleted.id).await.unwrap();
    account.create_note("already in the account").await.unwrap();

    let moved = local.move_notes_into(&account).await.unwrap();

    assert_eq!(moved, 2, "deleted notes move too, as deleted");
    assert!(local.note_documents().await.unwrap().is_empty());
    assert!(local.list_notes(10, 0).await.unwrap().is_empty());
    assert!(local.search_notes("written", 10).await.unwrap().is_empty());

    let in_account = account.list_notes(10, 0).await.unwrap();
    assert_eq!(in_account.len(), 2);
    let arrived = account.get_note(&kept.id).await.unwrap().unwrap();
    assert_eq!(arrived.content, "written before sign-in #idea");
    assert_eq!(arrived.created_at, kept.created_at);
    assert_eq!(account.get_note(&deleted.id).await.unwrap(), None);
    assert_eq!(
        account
            .list_notes_by_tag("idea", 10, 0)
            .await
            .unwrap()
            .len(),
        1
    );
    // The deleted note can still be restored in the account.
    assert_eq!(
        account.restore_note(&deleted.id).await.unwrap().content,
        "deleted before sign-in"
    );
}

#[tokio::test]
async fn moving_twice_is_harmless_and_a_store_cannot_move_into_itself() {
    let folder = tempfile::tempdir().unwrap();
    let local = Store::open(folder.path().join("local.db")).unwrap();
    let account = Store::open(folder.path().join("account.db")).unwrap();
    local.create_note("once").await.unwrap();

    assert_eq!(local.move_notes_into(&account).await.unwrap(), 1);
    assert_eq!(local.move_notes_into(&account).await.unwrap(), 0);
    assert_eq!(account.list_notes(10, 0).await.unwrap().len(), 1);

    let error = account.move_notes_into(&account.clone()).await.unwrap_err();
    assert!(matches!(error, Error::InvalidInput(_)), "{error}");
    assert_eq!(account.list_notes(10, 0).await.unwrap().len(), 1);
}

#[tokio::test]
async fn two_stores_never_see_each_others_notes() {
    let folder = tempfile::tempdir().unwrap();
    let alice = Store::open(store_path(
        folder.path(),
        &StoreName::account("alice").unwrap(),
    ))
    .unwrap();
    let bob = Store::open(store_path(
        folder.path(),
        &StoreName::account("bob").unwrap(),
    ))
    .unwrap();
    alice.create_note("alice's note").await.unwrap();

    assert!(bob.list_notes(10, 0).await.unwrap().is_empty());
    assert!(bob.search_notes("alice", 10).await.unwrap().is_empty());
}

// ---- Closing ----

#[tokio::test]
async fn closing_the_store_leaves_one_complete_file() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("notes.db");
    {
        let store = Store::open(&path).unwrap();
        store.create_note("kept in the main file").await.unwrap();
        // While the store is open, recent writes sit in the log.
        assert!(folder.path().join("notes.db-wal").exists());
    }

    // No log is left beside the file, so a copy of the file alone is a
    // complete backup.
    assert!(!folder.path().join("notes.db-wal").exists());
    let copy = folder.path().join("copy.db");
    std::fs::copy(&path, &copy).unwrap();
    let restored = Store::open(&copy).unwrap();
    assert_eq!(
        restored.list_notes(10, 0).await.unwrap()[0].content,
        "kept in the main file"
    );
}

/// A store must open even if another program holds the file for longer than
/// one write is willing to wait. A program that cannot open its notes can do
/// nothing at all, so opening keeps trying.
#[test]
fn opening_outlasts_a_program_that_holds_the_file_for_a_long_time() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("notes.db");
    drop(Store::open(&path).unwrap());

    // Another program takes the write lock and keeps it for longer than the
    // five seconds a single write waits.
    let holder = rusqlite::Connection::open(&path).unwrap();
    holder.execute_batch("BEGIN IMMEDIATE").unwrap();
    let held_for = std::time::Duration::from_millis(6500);
    let release = std::thread::spawn(move || {
        std::thread::sleep(held_for);
        holder.execute_batch("COMMIT").unwrap();
    });

    let started = std::time::Instant::now();
    let store = Store::open(&path);

    assert!(store.is_ok(), "{:?}", store.err());
    assert!(started.elapsed() >= held_for, "it waited for the lock");
    release.join().unwrap();
}

/// What a list with a search box and a tag filter shows. It has to reach
/// notes a client has not loaded: the oldest note here is beyond the first
/// hundred.
#[tokio::test]
async fn finding_notes_by_words_and_tag_looks_through_every_note() {
    let (_folder, store) = temp_store();
    let oldest = store
        .create_note("call the plumber about the boiler #home")
        .await
        .unwrap();
    for number in 0..120 {
        store
            .create_note(&format!("filler {number} #work"))
            .await
            .unwrap();
    }
    let newest = store
        .create_note("plumbing course starts in May #work")
        .await
        .unwrap();
    let first_page = store.list_notes(100, 0).await.unwrap();
    assert!(first_page.iter().all(|note| note.id != oldest.id));

    let ids =
        |notes: Vec<dirt_core::Note>| notes.into_iter().map(|note| note.id).collect::<Vec<_>>();

    // Words alone, as the start of longer words.
    let found = ids(store.find_notes("plumb", None, 50).await.unwrap());
    assert_eq!(found.len(), 2);
    assert!(found.contains(&oldest.id) && found.contains(&newest.id));
    // Every word has to match, in any order and not side by side.
    assert_eq!(
        ids(store.find_notes("boiler call", None, 50).await.unwrap()),
        [oldest.id]
    );

    // A tag alone, with or without its sign, most recently edited first.
    assert_eq!(
        ids(store.find_notes("", Some("#home"), 50).await.unwrap()),
        [oldest.id]
    );
    let work = store.find_notes("  ", Some("work"), 200).await.unwrap();
    assert_eq!(work.len(), 121);
    assert_eq!(work[0].id, newest.id);

    // Both together.
    assert_eq!(
        ids(store.find_notes("plumb", Some("work"), 50).await.unwrap()),
        [newest.id]
    );
    assert!(store
        .find_notes("boiler", Some("work"), 50)
        .await
        .unwrap()
        .is_empty());

    // Neither: the list itself.
    assert_eq!(
        ids(store.find_notes("", None, 100).await.unwrap()),
        ids(first_page)
    );
    // Text with nothing to search for matches nothing, tag or no tag.
    assert!(store.find_notes("!!!", None, 50).await.unwrap().is_empty());
    assert!(store
        .find_notes("!!!", Some("work"), 50)
        .await
        .unwrap()
        .is_empty());

    // A deleted note is not found.
    store.delete_note(&oldest.id).await.unwrap();
    assert!(store
        .find_notes("boiler", None, 50)
        .await
        .unwrap()
        .is_empty());
    assert!(store
        .find_notes("", Some("home"), 50)
        .await
        .unwrap()
        .is_empty());
}
