use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use dirt_core::notes::NoteDoc;
use dirt_core::{NoteId, Store};
use tempfile::TempDir;
use tokio::time::sleep;

use clap::Parser;

use crate::cli::{Cli, Commands, CompletionShell, ExportFormat};
use crate::commands::archive::{run_archive, run_unarchive};
use crate::commands::common::{
    default_editor, format_relative_time, list_notes, listing_lines, normalize_content,
    normalize_search_query, note_preview, note_to_list_item, search_notes,
};
use crate::commands::completions::run_completions;
use crate::commands::delete::run_delete;
use crate::commands::export::{files_not_here, run_export};
use crate::commands::restore::run_restore;
use crate::commands::sync::run_sync;
use crate::context::Context;
use crate::error::CliError;
use crate::naming::{deleted_note_named, find, read_note_ref};

#[test]
fn normalize_content_trims_and_rejects_empty() {
    assert_eq!(normalize_content("  hello  "), Some("hello".to_string()));
    assert_eq!(normalize_content(" \n\t "), None);
}

#[test]
fn normalize_content_keeps_multiline_text() {
    assert_eq!(
        normalize_content("line 1\nline 2\n"),
        Some("line 1\nline 2".to_string())
    );
}

#[test]
fn default_editor_is_defined() {
    assert!(!default_editor().is_empty());
}

#[test]
fn format_relative_time_units() {
    let now = 10_000_000;
    assert_eq!(format_relative_time(now - 30_000, now), "just now");
    assert_eq!(format_relative_time(now - 120_000, now), "2m ago");
    assert_eq!(format_relative_time(now - 2 * 60 * 60_000, now), "2h ago");
}

#[test]
fn note_preview_truncates_with_ellipsis() {
    let note = dirt_core::Note::new("This is a very long sentence that should be shortened");
    let preview = note_preview(&note, 20);
    assert_eq!(preview, "This is a very lo...");
}

#[test]
fn a_note_that_was_said_is_listed_by_its_length_until_it_has_words() {
    use dirt_core::models::{Voice, Words};
    let mut note = dirt_core::Note::new("");
    note.voice = Some(Voice {
        duration_ms: 12_300,
        words: Words::Waiting,
    });
    assert_eq!(note_preview(&note, 40), "Voice \u{b7} 0:12");
    note.voice = Some(Voice {
        duration_ms: 12_300,
        words: Words::NoneFound,
    });
    assert_eq!(
        note_preview(&note, 40),
        "Voice \u{b7} 0:12 (no words found)"
    );
    note.content = "Buy stamps".to_owned();
    assert_eq!(note_preview(&note, 40), "Buy stamps");
}

#[tokio::test(flavor = "current_thread")]
async fn list_notes_respects_limit_and_tag_filter() {
    let (_folder, db_path) = temp_db();
    let store = Store::open(&db_path).unwrap();
    store.create_note("First #work").await.unwrap();
    sleep(Duration::from_millis(2)).await;
    store.create_note("Second #personal").await.unwrap();
    sleep(Duration::from_millis(2)).await;
    store.create_note("Third #work").await.unwrap();

    let recent = list_notes(2, None, false, &store).await.unwrap();
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0].content, "Third #work");
    assert_eq!(recent[1].content, "Second #personal");

    let work_only = list_notes(10, Some("work"), false, &store).await.unwrap();
    assert_eq!(work_only.len(), 2);
    assert!(work_only.iter().all(|note| note.content.contains("#work")));
}

#[tokio::test(flavor = "current_thread")]
async fn search_notes_finds_matches_with_limit() {
    let (_folder, db_path) = temp_db();
    let store = Store::open(&db_path).unwrap();
    store.create_note("Milk and eggs").await.unwrap();
    sleep(Duration::from_millis(2)).await;
    store.create_note("Milkshake recipe").await.unwrap();
    sleep(Duration::from_millis(2)).await;
    store.create_note("Unrelated note").await.unwrap();

    let matches = search_notes("milk", 1, &store).await.unwrap();
    assert_eq!(matches.len(), 1);
    assert!(matches[0].content.to_lowercase().contains("milk"));
}

#[test]
fn normalize_search_query_rejects_empty() {
    assert!(normalize_search_query(" \n\t ").is_err());
    assert_eq!(
        normalize_search_query("  exact phrase  ").unwrap(),
        "exact phrase"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn resolve_note_for_edit_supports_exact_and_prefix_id() {
    let (_folder, db_path) = temp_db();
    let store = Store::open(&db_path).unwrap();
    insert_note(&store, "11111111-1111-7111-8111-111111111111", "Note A").await;
    insert_note(&store, "11111111-1111-7111-8111-222222222222", "Note B").await;

    let by_exact = named("11111111-1111-7111-8111-111111111111", &store)
        .await
        .unwrap();
    assert_eq!(by_exact.content, "Note A");

    let by_prefix = named("11111111-1111-7111-8111-2", &store).await.unwrap();
    assert_eq!(by_prefix.content, "Note B");
}

#[tokio::test(flavor = "current_thread")]
async fn resolve_note_for_edit_rejects_ambiguous_prefix() {
    let (_folder, db_path) = temp_db();
    let store = Store::open(&db_path).unwrap();
    insert_note(&store, "aaaaaaaa-aaaa-7aaa-8aaa-aaaaaaaaaaaa", "Left").await;
    insert_note(&store, "aaaaaaaa-aaaa-7aaa-8aaa-bbbbbbbbbbbb", "Right").await;

    let error = named("aaaaaaaa-aaaa-7aaa-8aaa", &store).await.unwrap_err();
    assert!(matches!(error, CliError::SeveralNotes(_)));
}

#[tokio::test(flavor = "current_thread")]
async fn resolve_note_for_edit_rejects_missing_note() {
    let (_folder, db_path) = temp_db();
    let store = Store::open(&db_path).unwrap();

    let error = named("does-not-exist", &store).await.unwrap_err();
    assert!(matches!(error, CliError::NoteNotFound(_)));
}

#[tokio::test(flavor = "current_thread")]
async fn run_delete_soft_deletes_note_by_exact_and_prefix_id() {
    let (_folder, db_path) = temp_db();
    let store = Store::open(&db_path).unwrap();
    let note_a = insert_note(&store, "bbbbbbbb-bbbb-7bbb-8bbb-111111111111", "Keep me").await;
    let note_b = insert_note(&store, "bbbbbbbb-bbbb-7bbb-8bbb-222222222222", "Delete me").await;

    let context = without_a_server(&store);
    run_delete(&arg("bbbbbbbb-bbbb-7bbb-8bbb-2"), &context)
        .await
        .unwrap();

    assert!(store.get_note(&note_b).await.unwrap().is_none());
    assert!(store.get_note(&note_a).await.unwrap().is_some());

    run_delete(&arg("bbbbbbbb-bbbb-7bbb-8bbb-111111111111"), &context)
        .await
        .unwrap();

    assert!(store.get_note(&note_a).await.unwrap().is_none());
}

// ---- Archive, and bringing a deleted note back ----

#[test]
fn the_new_commands_are_read_from_the_command_line() {
    let parsed = |words: &[&str]| Cli::try_parse_from(words).unwrap().command.unwrap();

    assert!(matches!(
        parsed(&["dirt", "archive", "0192d3a8"]),
        Commands::Archive { note } if note == ["0192d3a8"]
    ));
    assert!(matches!(
        parsed(&["dirt", "unarchive", "0192d3a8"]),
        Commands::Unarchive { note } if note == ["0192d3a8"]
    ));
    assert!(matches!(
        parsed(&["dirt", "restore", "0192d3a8"]),
        Commands::Restore { note } if note == ["0192d3a8"]
    ));
    assert!(matches!(
        parsed(&["dirt", "list", "--archived", "--tag", "home"]),
        Commands::List { archived: true, tag: Some(tag), json: false, limit: 10 } if tag == "home"
    ));
    assert!(matches!(
        parsed(&["dirt", "list"]),
        Commands::List {
            archived: false,
            ..
        }
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn run_archive_takes_a_note_out_of_the_list_and_run_unarchive_brings_it_back() {
    let (_folder, db_path) = temp_db();
    let store = Store::open(&db_path).unwrap();
    let done = insert_note(
        &store,
        "cccccccc-cccc-7ccc-8ccc-111111111111",
        "Bread #shop",
    )
    .await;
    let open = insert_note(&store, "cccccccc-cccc-7ccc-8ccc-222222222222", "Milk #shop").await;
    let context = without_a_server(&store);

    run_archive(&arg("cccccccc-cccc-7ccc-8ccc-1"), &context)
        .await
        .unwrap();

    let ids = |notes: Vec<dirt_core::Note>| notes.iter().map(|note| note.id).collect::<Vec<_>>();
    assert_eq!(
        ids(list_notes(10, None, false, &store).await.unwrap()),
        [open]
    );
    assert_eq!(
        ids(list_notes(10, None, true, &store).await.unwrap()),
        [done]
    );
    // `dirt list --tag` and `dirt list --archived --tag`.
    assert_eq!(
        ids(list_notes(10, Some("shop"), false, &store).await.unwrap()),
        [open]
    );
    assert_eq!(
        ids(list_notes(10, Some("shop"), true, &store).await.unwrap()),
        [done]
    );
    // Not an edit: the note has the edit time it had.
    let archived = store.get_note(&done).await.unwrap().unwrap();
    assert!(archived.archived);
    assert_eq!(archived.updated_at, 1000);
    // Archiving it again is not an error, and changes nothing.
    let revision = store.note_revision(&done).await.unwrap();
    run_archive(&arg("cccccccc-cccc-7ccc-8ccc-111111111111"), &context)
        .await
        .unwrap();
    assert_eq!(store.note_revision(&done).await.unwrap(), revision);

    // An archived note is named by the start of its ID like any other.
    run_unarchive(&arg("cccccccc-cccc-7ccc-8ccc-1"), &context)
        .await
        .unwrap();

    assert_eq!(list_notes(10, None, false, &store).await.unwrap().len(), 2);
    assert!(list_notes(10, None, true, &store).await.unwrap().is_empty());
    assert_eq!(
        store.get_note(&done).await.unwrap().unwrap().updated_at,
        1000
    );
}

#[tokio::test(flavor = "current_thread")]
async fn archiving_a_note_that_is_not_there_is_not_found() {
    let (_folder, db_path) = temp_db();
    let store = Store::open(&db_path).unwrap();
    let deleted = insert_note(&store, "dddddddd-dddd-7ddd-8ddd-111111111111", "Gone").await;
    store.delete_note(&deleted).await.unwrap();
    let context = without_a_server(&store);

    for id in ["dddddddd-dddd-7ddd-8ddd-111111111111", "eeeeeeee"] {
        let error = run_archive(&arg(id), &context).await.unwrap_err();
        assert!(matches!(error, CliError::NoteNotFound(_)), "{error}");
        let error = run_unarchive(&arg(id), &context).await.unwrap_err();
        assert!(matches!(error, CliError::NoteNotFound(_)), "{error}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_search_lists_archived_matches_after_the_rest_and_says_which_they_are() {
    let (_folder, db_path) = temp_db();
    let store = Store::open(&db_path).unwrap();
    let archived = store.create_note("Milk milk milk #shop").await.unwrap();
    store.archive_note(&archived.id).await.unwrap();
    sleep(Duration::from_millis(2)).await;
    let listed = store.create_note("Remember the milk").await.unwrap();

    let matches = search_notes("milk", 10, &store).await.unwrap();
    assert_eq!(
        matches.iter().map(|note| note.id).collect::<Vec<_>>(),
        [listed.id, archived.id]
    );

    let lines = listing_lines(&matches, &store).await.unwrap();
    assert!(!lines[0].contains("archived"), "{}", lines[0]);
    assert!(lines[1].ends_with("#shop  (archived)"), "{}", lines[1]);
    // Without tags the word stands where the tags would.
    let plain = store.create_note("No tags here").await.unwrap();
    let plain = store.archive_note(&plain.id).await.unwrap();
    let line = listing_lines(&[plain], &store).await.unwrap().remove(0);
    assert!(line.ends_with("just now  (archived)"), "{line}");

    // As JSON, every note says whether it is archived.
    let items: Vec<serde_json::Value> = matches
        .iter()
        .map(|note| serde_json::to_value(note_to_list_item(note)).unwrap())
        .collect();
    assert_eq!(items[0]["archived"], false);
    assert_eq!(items[1]["archived"], true);
}

#[tokio::test(flavor = "current_thread")]
async fn run_restore_brings_a_deleted_note_back_by_exact_and_prefix_id() {
    let (_folder, db_path) = temp_db();
    let store = Store::open(&db_path).unwrap();
    let note_a = insert_note(&store, "ffffffff-ffff-7fff-8fff-111111111111", "First").await;
    let note_b = insert_note(&store, "ffffffff-ffff-7fff-8fff-222222222222", "Second").await;
    let context = without_a_server(&store);
    for id in [
        "ffffffff-ffff-7fff-8fff-111111111111",
        "ffffffff-ffff-7fff-8fff-222222222222",
    ] {
        run_delete(&arg(id), &context).await.unwrap();
    }
    assert!(list_notes(10, None, false, &store)
        .await
        .unwrap()
        .is_empty());

    // By the start of the ID, which no live note has any more.
    run_restore(&arg("ffffffff-ffff-7fff-8fff-2"), &context)
        .await
        .unwrap();
    assert_eq!(
        store.get_note(&note_b).await.unwrap().unwrap().content,
        "Second"
    );
    assert!(store.get_note(&note_a).await.unwrap().is_none());

    // By the whole ID, as `dirt delete` printed it.
    run_restore(&arg("ffffffff-ffff-7fff-8fff-111111111111"), &context)
        .await
        .unwrap();
    assert_eq!(
        store.get_note(&note_a).await.unwrap().unwrap().content,
        "First"
    );
    // Bringing back a note that is not deleted leaves it as it is.
    let revision = store.note_revision(&note_a).await.unwrap();
    run_restore(&arg("ffffffff-ffff-7fff-8fff-111111111111"), &context)
        .await
        .unwrap();
    assert_eq!(store.note_revision(&note_a).await.unwrap(), revision);
}

#[tokio::test(flavor = "current_thread")]
async fn run_restore_says_when_no_deleted_note_or_more_than_one_is_meant() {
    let (_folder, db_path) = temp_db();
    let store = Store::open(&db_path).unwrap();
    let live = insert_note(&store, "abababab-abab-7bab-8bab-111111111111", "Live").await;
    for id in [
        "acacacac-acac-7cac-8cac-111111111111",
        "acacacac-acac-7cac-8cac-222222222222",
    ] {
        let id = insert_note(&store, id, "Deleted").await;
        store.delete_note(&id).await.unwrap();
    }
    let context = without_a_server(&store);

    // Two deleted notes share the start.
    let error = run_restore(&arg("acacacac"), &context).await.unwrap_err();
    assert!(matches!(error, CliError::SeveralNotes(_)), "{error}");
    // The start of a live note's ID names no deleted note.
    let error = deleted_note_named(&arg("abababab"), &store)
        .await
        .unwrap_err();
    assert!(matches!(error, CliError::NoteNotFound(_)), "{error}");
    // A whole ID that this device holds no note for.
    let error = run_restore(&arg("adadadad-adad-7dad-8dad-111111111111"), &context)
        .await
        .unwrap_err();
    assert!(matches!(error, CliError::NoteNotFound(_)), "{error}");
    assert!(matches!(
        run_restore(&arg("  "), &context).await.unwrap_err(),
        CliError::EmptyNoteId
    ));

    assert!(store.get_note(&live).await.unwrap().is_some());
    assert_eq!(list_notes(10, None, false, &store).await.unwrap().len(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn an_export_keeps_archived_notes_and_marks_them() {
    let (folder, db_path) = temp_db();
    let store = Store::open(&db_path).unwrap();
    store.create_note("Still to do").await.unwrap();
    let done = store.create_note("Done and archived").await.unwrap();
    store.archive_note(&done.id).await.unwrap();
    let context = without_a_server(&store);

    let json_path = folder.path().join("notes.json");
    run_export(ExportFormat::Json, Some(&json_path), None, &context)
        .await
        .unwrap();
    let exported: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&json_path).unwrap()).unwrap();
    let marks: Vec<(&str, bool)> = exported
        .as_array()
        .unwrap()
        .iter()
        .map(|note| {
            (
                note["content"].as_str().unwrap(),
                note["archived"].as_bool().unwrap(),
            )
        })
        .collect();
    assert_eq!(marks, [("Still to do", false), ("Done and archived", true)]);

    let markdown_path = folder.path().join("notes.md");
    run_export(ExportFormat::Markdown, Some(&markdown_path), None, &context)
        .await
        .unwrap();
    let exported = std::fs::read_to_string(&markdown_path).unwrap();
    assert_eq!(exported.matches("archived: true\n").count(), 1);
    assert!(exported.ends_with("archived: true\n---\n\nDone and archived\n"));
}

/// A sync that cannot happen must fail, so that nobody takes a silent
/// success to mean that their notes reached another device.
#[tokio::test(flavor = "current_thread")]
async fn run_sync_fails_when_there_is_no_server_to_sync_with() {
    let (_folder, db_path) = temp_db();
    let store = Store::open(&db_path).unwrap();

    let error = run_sync(&without_a_server(&store), false)
        .await
        .unwrap_err();

    assert!(matches!(error, CliError::Account(_)));
    assert!(error.to_string().contains("no server is configured"));
}

#[tokio::test(flavor = "current_thread")]
async fn the_account_status_says_when_notes_stay_on_this_device() {
    let (_folder, db_path) = temp_db();
    let store = Store::open(&db_path).unwrap();

    let lines = crate::commands::account::status_lines(&without_a_server(&store))
        .await
        .unwrap();

    assert_eq!(
        lines,
        [
            "No server is configured. Notes are kept on this device only.",
            "Set DIRT_API_BASE_URL to a server's address to sync."
        ]
    );
}

#[test]
fn a_sign_out_says_whether_the_notes_went_or_wait_for_the_other_program() {
    use crate::commands::account::signed_out_words;

    let removed = signed_out_words(false);
    assert!(removed.contains("notes and keys were removed"), "{removed}");
    // Put off while the desktop app has them open: not said to be removed.
    let put_off = signed_out_words(true);
    assert!(
        !put_off.contains("notes and keys were removed"),
        "{put_off}"
    );
    assert!(put_off.contains("desktop app"), "{put_off}");
    assert!(put_off.contains("nothing more is saved"), "{put_off}");
}

#[test]
fn a_device_waiting_to_be_let_in_is_told_how() {
    let words = crate::context::WAITING_TO_BE_LET_IN;
    assert!(words.contains("waiting to be let in"), "{words}");
    assert!(words.contains("`dirt auth join`"), "{words}");
    assert!(words.contains("`dirt devices approve`"), "{words}");
}

/// `EDITOR` may be a program with arguments. The command here does nothing
/// and succeeds, whatever file it is given.
#[test]
fn an_editor_command_with_arguments_is_run() {
    let (_folder, file) = temp_db();
    std::fs::write(&file, "text").unwrap();
    let command = if cfg!(windows) {
        "cmd /C rem"
    } else {
        "sh -c true"
    };
    crate::commands::common::launch_editor(command, &file).unwrap();
}

/// On Windows a command with a drive's colon after an argument cannot be
/// the name of a program, and the failure is not "not found". It still has
/// to be read as a program with arguments.
#[cfg(windows)]
#[test]
fn an_editor_command_holding_a_path_is_run_on_windows() {
    let (_folder, file) = temp_db();
    std::fs::write(&file, "text").unwrap();
    crate::commands::common::launch_editor("cmd /C rem C:\\anywhere", &file).unwrap();
}

#[test]
fn an_editor_that_does_not_exist_is_an_error() {
    let (_folder, file) = temp_db();
    std::fs::write(&file, "text").unwrap();
    assert!(crate::commands::common::launch_editor("no-such-editor-anywhere", &file).is_err());
    assert!(
        crate::commands::common::launch_editor("no-such-editor-anywhere --wait", &file).is_err()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn run_export_writes_json_file() {
    let (_folder, db_path) = temp_db();
    let store = Store::open(&db_path).unwrap();
    store.create_note("Export me #one").await.unwrap();

    let output_path = std::env::temp_dir().join(format!(
        "dirt-export-test-{}.json",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos())
    ));

    run_export(
        ExportFormat::Json,
        Some(&output_path),
        None,
        &without_a_server(&store),
    )
    .await
    .unwrap();

    let exported = std::fs::read_to_string(&output_path).unwrap();
    assert!(exported.contains("\"content\": \"Export me #one\""));
    assert!(exported.contains("\"tags\": [\n      \"one\"\n    ]"));

    let _ = std::fs::remove_file(output_path);
}

/// A note with two files, the second of which this device does not hold.
async fn note_with_a_file_here_and_one_away(folder: &TempDir, store: &Store) -> NoteId {
    let note = store.create_note("Trip #travel").await.unwrap();
    for (name, bytes) in [("ticket.txt", "a ticket"), ("map.txt", "north is up")] {
        let source = folder.path().join(name);
        std::fs::write(&source, bytes).unwrap();
        store
            .attach_file(&note.id, &source, name, "text/plain")
            .await
            .unwrap();
    }
    let away = store.note_attachments(&note.id).await.unwrap()[1].file_id;
    std::fs::remove_file(store.shelf().path(away)).unwrap();
    note.id
}

#[tokio::test(flavor = "multi_thread")]
async fn an_export_into_a_folder_holds_the_files_and_fails_if_one_is_missing() {
    let (folder, db_path) = temp_db();
    let store = Store::open(&db_path).unwrap();
    let note = note_with_a_file_here_and_one_away(&folder, &store).await;
    let context = without_a_server(&store);
    let target = folder.path().join("out");

    // One file cannot be had: the folder is written, and the command
    // fails, saying which.
    let failed = run_export(ExportFormat::Json, None, Some(&target), &context)
        .await
        .unwrap_err();
    assert_eq!(
        failed.to_string(),
        "1 file is not in the export.\n\
         The file is not on this device, and this device is not signed in to fetch it.\n  \
         map.txt, attached to: Trip #travel"
    );
    assert_eq!(
        std::fs::read(
            target
                .join("files")
                .join(note.to_string())
                .join("ticket.txt")
        )
        .unwrap(),
        b"a ticket"
    );
    let notes = std::fs::read_to_string(target.join("notes.json")).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&notes).unwrap();
    assert_eq!(
        parsed[0]["files"][0]["path"],
        format!("files/{note}/ticket.txt")
    );
    assert!(parsed[0]["files"][1]["path"].is_null());

    // A folder that is there is not written into.
    let again = run_export(ExportFormat::Json, None, Some(&target), &context)
        .await
        .unwrap_err();
    assert!(again.to_string().contains("is already there"), "{again}");

    // With every file on the device, the command succeeds.
    let away = store.note_attachments(&note).await.unwrap()[1].file_id;
    std::fs::write(store.shelf().path(away), b"north is up").unwrap();
    let whole = folder.path().join("whole");
    run_export(ExportFormat::Markdown, None, Some(&whole), &context)
        .await
        .unwrap();
    assert!(whole.join("notes.md").is_file());
    assert_eq!(
        std::fs::read(whole.join("files").join(note.to_string()).join("map.txt")).unwrap(),
        b"north is up"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_text_alone_names_the_files_and_says_they_are_not_exported() {
    let (folder, db_path) = temp_db();
    let store = Store::open(&db_path).unwrap();
    note_with_a_file_here_and_one_away(&folder, &store).await;
    let output = folder.path().join("notes.json");
    run_export(
        ExportFormat::Json,
        Some(&output),
        None,
        &without_a_server(&store),
    )
    .await
    .unwrap();
    let parsed: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&output).unwrap()).unwrap();
    assert_eq!(parsed[0]["files"][0]["name"], "ticket.txt");
    assert!(parsed[0]["files"][0]["path"].is_null());
    assert_eq!(parsed[0]["files"].as_array().unwrap().len(), 2);

    assert_eq!(files_not_here(0), None);
    assert_eq!(
        files_not_here(1).unwrap(),
        "1 file attached to these notes is named here and not exported. \
         `dirt export --folder <folder>` writes the notes and their files."
    );
    assert!(files_not_here(2)
        .unwrap()
        .starts_with("2 files attached to these notes are named here"));
}

#[test]
fn run_completions_writes_bash_script_file() {
    let output_path = std::env::temp_dir().join(format!(
        "dirt-completions-test-{}.bash",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos())
    ));

    run_completions(CompletionShell::Bash, Some(&output_path)).unwrap();

    let script = std::fs::read_to_string(&output_path).unwrap();
    assert!(script.contains("_dirt()"));
    assert!(script.contains("complete -F _dirt"));
    assert!(script.contains(" default dirt"));

    let _ = std::fs::remove_file(output_path);
}

/// What a command gets when no server is configured: a store and no session.
fn without_a_server(store: &Store) -> Context {
    Context {
        store: store.clone(),
        session: None,
        server: None,
        can_ask: false,
    }
}

/// One argument, as clap gives it.
fn arg(text: &str) -> Vec<String> {
    vec![text.to_owned()]
}

/// The note `text` names, as any command reads it, with nobody to ask.
async fn named(text: &str, store: &Store) -> Result<dirt_core::Note, CliError> {
    find(&read_note_ref(&arg(text)), store, false).await
}

/// A database path inside a new temporary folder. The folder is removed
/// when the returned guard is dropped.
fn temp_db() -> (TempDir, PathBuf) {
    let folder = tempfile::tempdir().unwrap();
    let db_path = folder.path().join("notes.db");
    (folder, db_path)
}

/// Stores a note under a chosen ID, which `Store::create_note` cannot do.
async fn insert_note(store: &Store, id: &str, content: &str) -> NoteId {
    let id: NoteId = id.parse().unwrap();
    let document = NoteDoc::new(1);
    document.set_text(content, 1000);
    store
        .merge_note_document(&id, document.encode())
        .await
        .unwrap();
    id
}

// ---- Files ----

use crate::commands::files::{files_lines, run_attach, run_detach, run_save};

/// A note in a store with no server, and a folder to keep files in.
async fn a_note(text: &str) -> (TempDir, Store, Context, String) {
    let (folder, db_path) = temp_db();
    let store = Store::open(&db_path).unwrap();
    let note = store.create_note(text).await.unwrap().id.to_string();
    let context = without_a_server(&store);
    (folder, store, context, note)
}

fn a_file(folder: &TempDir, name: &str, contents: &[u8]) -> PathBuf {
    let path = folder.path().join(name);
    std::fs::write(&path, contents).unwrap();
    path
}

#[tokio::test(flavor = "current_thread")]
async fn a_file_is_attached_listed_saved_and_removed() {
    let (folder, _store, context, note) = a_note("the meeting").await;
    let minutes = a_file(&folder, "minutes.txt", b"who said what");

    let attached = run_attach(&note, &minutes, None, None, &context)
        .await
        .unwrap();
    assert_eq!(attached.name, "minutes.txt");
    assert_eq!(attached.media_type, "text/plain");

    // Listed by the start of its ID, with its name, size and kind. With no
    // server it is waiting to be sent, and stays so.
    let short: String = attached.file_id.to_string().chars().take(13).collect();
    assert_eq!(
        files_lines(&note, &context).await.unwrap(),
        [format!(
            "{short}  minutes.txt       13 B  text/plain  (waiting to be sent)"
        )]
    );

    // Saved into a folder, under its own name, by the start of its ID.
    let out = folder.path().join("out");
    std::fs::create_dir(&out).unwrap();
    let saved = run_save(&note, &short, Some(&out), false, &context)
        .await
        .unwrap();
    assert_eq!(saved, out.join("minutes.txt"));
    assert_eq!(std::fs::read(&saved).unwrap(), b"who said what");

    // Something already there is not replaced unless that is asked for.
    std::fs::write(&saved, b"my own notes").unwrap();
    let refused = run_save(&note, "minutes.txt", Some(&out), false, &context)
        .await
        .unwrap_err();
    assert!(refused.to_string().contains("already there"), "{refused}");
    assert_eq!(std::fs::read(&saved).unwrap(), b"my own notes");
    run_save(&note, "minutes.txt", Some(&out), true, &context)
        .await
        .unwrap();
    assert_eq!(std::fs::read(&saved).unwrap(), b"who said what");
    // To a path that names the file itself.
    let renamed = out.join("copy.txt");
    run_save(&note, "minutes.txt", Some(&renamed), false, &context)
        .await
        .unwrap();
    assert_eq!(std::fs::read(&renamed).unwrap(), b"who said what");

    run_detach(&note, "minutes.txt", &context).await.unwrap();
    assert_eq!(
        files_lines(&note, &context).await.unwrap(),
        ["The note has no files."]
    );
    let gone = run_detach(&note, "minutes.txt", &context)
        .await
        .unwrap_err();
    assert!(gone.to_string().contains("no file with the ID or name"));
}

/// A file's name comes out of a note, where another device put it. It is
/// saved as a plain name in the folder the person chose, whatever it says.
#[tokio::test(flavor = "current_thread")]
async fn a_file_is_never_saved_outside_the_folder_asked_for() {
    let (folder, store, context, note) = a_note("the meeting").await;
    let source = a_file(&folder, "source.txt", b"contents");
    let note_id: NoteId = note.parse().unwrap();
    // As another client, or another version, might have named it.
    store
        .attach_file(&note_id, &source, "../../escaped.txt", "text/plain")
        .await
        .unwrap();
    let out = folder.path().join("deep").join("er");
    std::fs::create_dir_all(&out).unwrap();

    let saved = run_save(&note, "../../escaped.txt", Some(&out), false, &context)
        .await
        .unwrap();

    assert_eq!(saved, out.join("escaped.txt"));
    assert!(!folder.path().join("escaped.txt").exists());

    // And this client does not put such a name into a note itself.
    let attached = run_attach(&note, &source, Some("..\\up\\name.txt"), None, &context)
        .await
        .unwrap();
    assert_eq!(attached.name, "name.txt");
}

#[tokio::test(flavor = "current_thread")]
async fn a_file_is_named_by_id_or_by_a_name_no_other_has() {
    let (folder, store, context, note) = a_note("the meeting").await;
    let source = a_file(&folder, "photo.jpg", b"not really a photograph");
    let first = run_attach(&note, &source, None, None, &context)
        .await
        .unwrap();
    let second = run_attach(&note, &source, None, Some("image/x-test"), &context)
        .await
        .unwrap();
    assert_eq!(first.media_type, "image/jpeg");
    assert_eq!(second.media_type, "image/x-test");

    // Two files of one name: the name does not say which.
    let unclear = run_detach(&note, "photo.jpg", &context).await.unwrap_err();
    let said = unclear.to_string();
    assert!(
        said.contains("names 2 files")
            && said.contains(&first.file_id.to_string())
            && said.contains(&second.file_id.to_string()),
        "{said}"
    );
    // The whole ID does.
    run_detach(&note, &second.file_id.to_string(), &context)
        .await
        .unwrap();
    let left = store
        .note_attachments(&note.parse().unwrap())
        .await
        .unwrap();
    assert_eq!(left, [first]);
    // Now the name is enough.
    run_detach(&note, "photo.jpg", &context).await.unwrap();

    let error = run_detach(&note, "  ", &context).await.unwrap_err();
    assert!(error.to_string().contains("say which file"), "{error}");
}

#[tokio::test(flavor = "current_thread")]
async fn what_cannot_be_attached_or_saved_says_why() {
    let (folder, store, context, note) = a_note("the meeting").await;

    // A folder, and nothing at all.
    for path in [folder.path().to_path_buf(), folder.path().join("missing")] {
        let error = run_attach(&note, &path, None, None, &context)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("is not a file"), "{error}");
    }
    let source = a_file(&folder, "photo.jpg", b"bytes");
    let no_note = run_attach("no-such-note", &source, None, None, &context)
        .await
        .unwrap_err();
    assert!(matches!(no_note, CliError::NoteNotFound(_)), "{no_note}");

    // A file the note names and this device does not hold, with nobody
    // signed in to fetch it.
    let attached = run_attach(&note, &source, None, None, &context)
        .await
        .unwrap();
    std::fs::remove_file(store.shelf().path(attached.file_id)).unwrap();
    assert_eq!(
        files_lines(&note, &context).await.unwrap()[0]
            .rsplit("  ")
            .next()
            .unwrap(),
        "(not on this device; fetched when it is wanted)"
    );
    let out = folder.path().join("out.jpg");
    let error = run_save(&note, "photo.jpg", Some(&out), false, &context)
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("not signed in to fetch it"),
        "{error}"
    );
    assert!(!out.exists());
}

#[test]
fn a_sync_says_how_many_files_it_sent() {
    use dirt_core::sync::engine::SyncReport;

    let report = |files_sent| SyncReport {
        pulled: 2,
        pushed: 1,
        files_sent,
        ..SyncReport::default()
    };
    let said = crate::commands::sync::synced_line;
    assert_eq!(said(&report(0)), "Synced: 2 received, 1 sent.");
    assert_eq!(said(&report(1)), "Synced: 2 received, 1 sent, and 1 file.");
    assert_eq!(said(&report(3)), "Synced: 2 received, 1 sent, and 3 files.");
}

/// After a sync the person is told of files that did not go, and why.
#[test]
fn a_sync_says_which_files_did_not_go() {
    use dirt_core::store::FileProblem;
    use dirt_core::sync::engine::SyncReport;

    let said = crate::commands::sync::files_not_sent;
    assert!(said(&SyncReport::default(), &[]).is_empty());

    let waiting = SyncReport {
        files_unsent: 2,
        ..SyncReport::default()
    };
    assert_eq!(said(&waiting, &[]), ["2 files are waiting to be sent."]);
    let failed = SyncReport {
        files_unsent: 1,
        files_failed: true,
        ..SyncReport::default()
    };
    assert_eq!(
        said(&failed, &[]),
        ["1 file is waiting to be sent. The file storage could not be reached; `dirt sync` tries again."]
    );

    let refused = FileProblem {
        file_id: "0192d3a8-0000-7000-8000-000000000002".parse().unwrap(),
        note_id: NoteId::new(),
        reason: "the account's storage for files is full".to_owned(),
        name: Some("film.mp4".to_owned()),
        note_text: "holiday".to_owned(),
    };
    assert_eq!(
        said(&SyncReport::default(), &[refused]),
        ["Not sent: film.mp4, attached to: holiday (The account's storage for files is full)."]
    );
}

/// A file that came to be there after `dirt save` looked, while it was
/// fetching, is not replaced: the copy is refused at the moment of writing.
#[test]
fn a_copy_is_not_written_over_a_file_that_appeared_meanwhile() {
    use crate::commands::files::copy_to;

    let folder = tempfile::tempdir().unwrap();
    let held = folder.path().join("held");
    std::fs::write(&held, b"the file from the note").unwrap();
    let target = folder.path().join("target.txt");

    // Nothing there: written.
    copy_to(&held, &target, false).unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), b"the file from the note");

    // Something there now, as if another program had just put it.
    std::fs::write(&target, b"somebody else's").unwrap();
    let refused = copy_to(&held, &target, false).unwrap_err();
    assert!(refused.to_string().contains("already there"), "{refused}");
    assert_eq!(std::fs::read(&target).unwrap(), b"somebody else's");

    // Asked to replace it: replaced whole, with nothing of the old left.
    std::fs::write(&target, b"somebody else's, and much longer than the file").unwrap();
    copy_to(&held, &target, true).unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), b"the file from the note");

    // A copy that cannot be made leaves nothing under the name.
    let fresh = folder.path().join("fresh.txt");
    let error = copy_to(&folder.path().join("no such file"), &fresh, false).unwrap_err();
    assert!(matches!(error, CliError::Io(_)), "{error}");
    assert!(!fresh.exists());
}

// ---- Naming a note: numbers, IDs, words, the latest ----

use crate::commands::common::{edit_in_editor, editor_from, note_file_name, report_line, Editor};
use crate::commands::edit::{edit_with, EditOutcome};
use crate::commands::list::run_list;
use crate::commands::search::run_search;
use crate::commands::show::{run_show, show_lines};
use crate::naming::{choose, last_list, list_file, remember_list, LastList, NoteRef};

/// Three notes, written in this order, so that the list shows them the
/// other way round.
async fn three_notes() -> (TempDir, Store, Context, [NoteId; 3]) {
    let (folder, db_path) = temp_db();
    let store = Store::open(&db_path).unwrap();
    let mut ids = Vec::new();
    for text in [
        "Buy groceries #shop",
        "Groceries for the party",
        "Call the dentist",
    ] {
        ids.push(store.create_note(text).await.unwrap().id);
        sleep(Duration::from_millis(2)).await;
    }
    let context = without_a_server(&store);
    (folder, store, context, [ids[0], ids[1], ids[2]])
}

fn words(typed: &[&str]) -> NoteRef {
    read_note_ref(
        &typed
            .iter()
            .map(|word| (*word).to_owned())
            .collect::<Vec<_>>(),
    )
}

#[test]
fn what_is_typed_is_read_as_a_number_then_an_id_then_words() {
    assert_eq!(words(&[]), NoteRef::Latest);
    assert_eq!(words(&["  "]), NoteRef::Latest);
    assert_eq!(words(&["2"]), NoteRef::Number(2));
    assert_eq!(words(&["2024"]), NoteRef::Number(2024));
    assert_eq!(words(&["0"]), NoteRef::Number(0));
    assert_eq!(
        words(&["99999999999999999999999"]),
        NoteRef::Number(usize::MAX)
    );
    // Every note ID begins with a zero, so digits that begin with one are
    // the start of an ID.
    assert_eq!(words(&["0193"]), NoteRef::IdOrWords("0193".to_owned()));
    assert_eq!(
        words(&["0193a1b2-c"]),
        NoteRef::IdOrWords("0193a1b2-c".to_owned())
    );
    // Hexadecimal may be a word too.
    assert_eq!(words(&["cafe"]), NoteRef::IdOrWords("cafe".to_owned()));
    let id = NoteId::new();
    assert_eq!(words(&[&id.to_string()]), NoteRef::Id(id));
    assert_eq!(
        words(&["groceries"]),
        NoteRef::Words("groceries".to_owned())
    );
    // Words without quotes come as several arguments.
    assert_eq!(
        words(&["buy", "groceries"]),
        NoteRef::Words("buy groceries".to_owned())
    );
    assert_eq!(words(&["2", "days"]), NoteRef::Words("2 days".to_owned()));
}

#[tokio::test(flavor = "current_thread")]
async fn a_number_names_the_note_the_last_list_showed_under_it() {
    let (_folder, store, _context, [first, second, third]) = three_notes().await;

    // No list yet.
    let error = named("2", &store).await.unwrap_err();
    assert!(error.to_string().contains("no list"), "{error}");

    run_list(10, None, false, false, &store).await.unwrap();
    let list = last_list(&store).unwrap();
    assert_eq!(list.ids, [third, second, first]);
    assert_eq!(list.what, "`dirt list`");
    assert_eq!(named("1", &store).await.unwrap().id, third);
    assert_eq!(named("3", &store).await.unwrap().id, first);

    // Editing the first note moves it to the top of the list, but the
    // number still names what the list showed.
    store
        .update_note(&first, "Buy groceries and milk")
        .await
        .unwrap();
    assert_eq!(named("3", &store).await.unwrap().id, first);

    // A number the list did not have.
    let error = named("7", &store).await.unwrap_err();
    assert!(matches!(error, CliError::NoteNotFound(_)));
    assert!(
        error.to_string().contains("had 3 notes: there is no 7"),
        "{error}"
    );
    let error = named("0", &store).await.unwrap_err();
    assert!(error.to_string().contains("numbered from 1"), "{error}");

    // A list as JSON is not numbered, and leaves the last list alone.
    run_list(1, None, false, true, &store).await.unwrap();
    assert_eq!(last_list(&store).unwrap().ids.len(), 3);

    // A search is a list too.
    run_search("dentist", 10, false, &store).await.unwrap();
    let list = last_list(&store).unwrap();
    assert_eq!(list.ids, [third]);
    assert_eq!(list.what, "`dirt search dentist`");
}

#[tokio::test(flavor = "current_thread")]
async fn a_number_whose_note_was_deleted_says_so_and_restore_takes_it() {
    let (_folder, store, context, [_, second, _]) = three_notes().await;
    run_list(10, None, false, false, &store).await.unwrap();

    run_delete(&arg("2"), &context).await.unwrap();
    assert!(store.get_note(&second).await.unwrap().is_none());

    let error = named("2", &store).await.unwrap_err();
    assert!(
        error
            .to_string()
            .contains("`dirt restore 2` brings it back"),
        "{error}"
    );
    run_restore(&arg("2"), &context).await.unwrap();
    assert_eq!(named("2", &store).await.unwrap().id, second);

    // A note the list had that this device no longer holds at all.
    let gone = LastList {
        what: "`dirt list`".to_owned(),
        at: 0,
        ids: vec![NoteId::new()],
    };
    std::fs::write(list_file(&store), serde_json::to_vec(&gone).unwrap()).unwrap();
    let error = named("1", &store).await.unwrap_err();
    assert!(error.to_string().contains("not on this device"), "{error}");

    // A list that cannot be read is no list.
    std::fs::write(list_file(&store), b"not a list").unwrap();
    let error = named("1", &store).await.unwrap_err();
    assert!(error.to_string().contains("no list"), "{error}");
}

#[tokio::test(flavor = "current_thread")]
async fn the_last_list_is_kept_beside_the_database() {
    let (_folder, db_path) = temp_db();
    let store = Store::open(&db_path).unwrap();
    let mut beside = db_path.into_os_string();
    beside.push("-list");
    assert_eq!(list_file(&store), PathBuf::from(beside));

    // Not in a store that is being signed out of.
    std::fs::write(
        store
            .path()
            .parent()
            .unwrap()
            .join(dirt_core::store::SIGNED_OUT_MARKER),
        b"",
    )
    .unwrap();
    remember_list(&store, "`dirt list`", &[]);
    assert!(!list_file(&store).exists());
}

#[tokio::test(flavor = "current_thread")]
async fn words_name_the_one_note_holding_them_or_list_the_several() {
    let (_folder, store, _context, [first, second, third]) = three_notes().await;

    // One match.
    assert_eq!(named("dentist", &store).await.unwrap().id, third);
    assert_eq!(
        find(&words(&["call", "the"]), &store, false)
            .await
            .unwrap()
            .id,
        third
    );

    // None.
    let error = named("zebra", &store).await.unwrap_err();
    assert!(matches!(error, CliError::NoteNotFound(_)));
    assert!(
        error.to_string().contains("No note holds \"zebra\""),
        "{error}"
    );

    // Several: listed, numbered, and kept as the last list, so that the
    // next command can take one by its number.
    let error = named("groceries", &store).await.unwrap_err();
    let CliError::SeveralNotes(said) = &error else {
        panic!("{error}");
    };
    assert!(
        said.starts_with("Several notes holding \"groceries\":"),
        "{said}"
    );
    assert!(said.contains("\n1  "), "{said}");
    assert!(said.contains("\n2  "), "{said}");
    assert!(said.contains("Name one by its number"), "{said}");
    let list = last_list(&store).unwrap();
    assert_eq!(list.ids.len(), 2);
    assert!(list.ids.contains(&first) && list.ids.contains(&second));
    assert_eq!(named("2", &store).await.unwrap().id, list.ids[1]);
}

#[tokio::test(flavor = "current_thread")]
async fn hexadecimal_that_begins_no_id_is_taken_as_words() {
    let (_folder, db_path) = temp_db();
    let store = Store::open(&db_path).unwrap();
    let cafe = store.create_note("The cafe opens at nine").await.unwrap();
    store.create_note("Something else").await.unwrap();

    assert_eq!(named("cafe", &store).await.unwrap().id, cafe.id);
    // The start of its ID still names it.
    let start: String = cafe.id.to_string().chars().take(30).collect();
    assert_eq!(named(&start, &store).await.unwrap().id, cafe.id);
}

#[tokio::test(flavor = "current_thread")]
async fn the_start_of_an_id_that_several_share_lists_them_numbered() {
    let (_folder, db_path) = temp_db();
    let store = Store::open(&db_path).unwrap();
    let left = insert_note(&store, "0193a1b2-0000-7000-8000-000000000001", "Left").await;
    let right = insert_note(&store, "0193a1b2-c000-7000-8000-000000000002", "Right").await;

    let error = named("0193a1b2", &store).await.unwrap_err();
    let CliError::SeveralNotes(said) = &error else {
        panic!("{error}");
    };
    assert!(
        said.starts_with("Several notes whose IDs begin with 0193a1b2:"),
        "{said}"
    );
    // Each shown by as much of its ID as tells it from the other.
    assert!(said.contains("0193a1b2-0"), "{said}");
    assert!(said.contains("0193a1b2-c"), "{said}");
    let list = last_list(&store).unwrap();
    assert_eq!(list.ids.len(), 2);
    assert!(list.ids.contains(&left) && list.ids.contains(&right));
    assert_eq!(named("0193a1b2-c", &store).await.unwrap().id, right);
}

#[tokio::test(flavor = "current_thread")]
async fn nothing_named_is_the_latest_note_for_show_and_edit_alone() {
    let (_folder, db_path) = temp_db();
    let store = Store::open(&db_path).unwrap();
    let context = without_a_server(&store);
    let error = find(&NoteRef::Latest, &store, false).await.unwrap_err();
    assert!(matches!(error, CliError::NoteNotFound(_)), "{error}");

    let (_folder, store, context_with_notes, [first, _, third]) = three_notes().await;
    assert_eq!(
        find(&NoteRef::Latest, &store, false).await.unwrap().id,
        third
    );
    // The latest is the most recently edited, which the list shows first.
    store
        .update_note(&first, "Buy groceries today")
        .await
        .unwrap();
    assert_eq!(
        find(&NoteRef::Latest, &store, false).await.unwrap().id,
        first
    );
    // `dirt show` takes it.
    run_show(&[], false, &context_with_notes).await.unwrap();

    // Commands that change a note need one named.
    assert!(matches!(
        run_delete(&[], &context).await.unwrap_err(),
        CliError::EmptyNoteId
    ));
    assert!(matches!(
        run_archive(&arg(" "), &context_with_notes)
            .await
            .unwrap_err(),
        CliError::EmptyNoteId
    ));
}

#[test]
fn a_choice_is_asked_until_a_number_in_the_list_is_given() {
    let mut said = Vec::new();
    let mut typed = std::io::Cursor::new("x\n5\n2\n");
    assert_eq!(choose(&mut typed, &mut said, 3).unwrap(), Some(2));
    let said = String::from_utf8(said).unwrap();
    assert_eq!(said.matches("Which one?").count(), 3);
    assert_eq!(said.matches("Type a number from 1 to 3.").count(), 2);

    let mut said = Vec::new();
    assert_eq!(
        choose(&mut std::io::Cursor::new("\n"), &mut said, 3).unwrap(),
        None
    );
    assert_eq!(
        choose(&mut std::io::Cursor::new(""), &mut said, 3).unwrap(),
        None
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_list_numbers_its_notes_and_shows_as_much_of_each_id_as_tells_it_apart() {
    let (_folder, db_path) = temp_db();
    let store = Store::open(&db_path).unwrap();
    insert_note(
        &store,
        "0193a1b2-0000-7000-8000-000000000001",
        "Older #shop",
    )
    .await;
    insert_note(&store, "0193a1b2-c000-7000-8000-000000000002", "Newer").await;
    insert_note(&store, "0194ffff-0000-7000-8000-000000000003", "Far away").await;

    // Sorted by ID here, as all three have the same edit time.
    let notes = list_notes(10, None, false, &store).await.unwrap();
    let lines = listing_lines(&notes, &store).await.unwrap();
    let by_text = |text: &str| {
        lines
            .iter()
            .find(|line| line.contains(text))
            .unwrap()
            .clone()
    };
    assert!(
        by_text("Older").contains("  0193a1b2-0  Older #shop"),
        "{lines:?}"
    );
    assert!(by_text("Older").ends_with("#shop"), "{lines:?}");
    assert!(
        by_text("Newer").contains("  0193a1b2-c  Newer"),
        "{lines:?}"
    );
    // The short IDs line up: the shorter is padded.
    assert!(
        by_text("Far away").contains("  0194ffff    Far away"),
        "{lines:?}"
    );
    for (index, line) in lines.iter().enumerate() {
        assert!(line.starts_with(&format!("{}  ", index + 1)), "{line}");
    }
}

#[test]
fn what_was_done_is_said_in_words_to_a_terminal_and_as_the_id_to_a_script() {
    let note = dirt_core::Note::new("Buy milk\nand bread");
    assert_eq!(
        report_line("Saved", &note, None, false),
        note.id.to_string()
    );
    assert_eq!(report_line("Saved", &note, None, true), "Saved: Buy milk");
    assert_eq!(
        report_line(
            "Deleted",
            &note,
            Some("`dirt restore 0193` brings it back"),
            true
        ),
        "Deleted: Buy milk. `dirt restore 0193` brings it back"
    );
}

#[test]
fn a_note_is_edited_in_a_file_named_after_its_first_words() {
    assert_eq!(note_file_name(None), "new-note.txt");
    let note = dirt_core::Note::new("Buy milk & eggs!\nand more");
    assert_eq!(note_file_name(Some(&note)), "buy-milk-eggs.txt");
    let note = dirt_core::Note::new("Café crème, 2 × ☕");
    assert_eq!(note_file_name(Some(&note)), "café-crème-2.txt");
    let note = dirt_core::Note::new("   ");
    assert_eq!(note_file_name(Some(&note)), "note.txt");
}

#[test]
fn the_editor_is_visual_then_editor_then_the_default() {
    let some = |text: &str| Some(text.to_owned());
    assert_eq!(
        editor_from(some("code --wait"), some("vi")),
        Editor {
            command: "code --wait".to_owned(),
            from: Some("VISUAL")
        }
    );
    assert_eq!(editor_from(some(" "), some("nano")).from, Some("EDITOR"));
    let fallback = editor_from(None, some(""));
    assert_eq!(fallback.command, default_editor());
    assert_eq!(fallback.from, None);
}

#[test]
fn an_editor_that_cannot_start_says_how_to_choose_one_when_none_was_chosen() {
    let missing = Editor {
        command: "no-such-editor-anywhere".to_owned(),
        from: None,
    };
    let error = edit_in_editor(&missing, "text", "note.txt").unwrap_err();
    let said = error.to_string();
    assert!(matches!(error, CliError::EditorFailed(_)), "{said}");
    assert!(
        said.contains("`no-such-editor-anywhere` could not be started"),
        "{said}"
    );
    assert!(said.contains("neither VISUAL nor EDITOR is set"), "{said}");
    assert!(said.contains("Set EDITOR"), "{said}");

    let chosen = Editor {
        from: Some("EDITOR"),
        ..missing
    };
    let said = edit_in_editor(&chosen, "text", "note.txt")
        .unwrap_err()
        .to_string();
    assert!(said.contains("It is the editor EDITOR names"), "{said}");
}

/// An editor that writes `text` over the file it is given.
fn an_editor_that_writes(folder: &TempDir, text: &str) -> Editor {
    let source = a_file(folder, "edited.txt", text.as_bytes());
    let command = if cfg!(windows) {
        format!("cmd /C copy /Y {}", source.display())
    } else {
        format!("cp {}", source.display())
    };
    Editor {
        command,
        from: Some("EDITOR"),
    }
}

/// An editor that changes nothing.
fn an_editor_that_changes_nothing() -> Editor {
    Editor {
        command: if cfg!(windows) {
            "cmd /C rem".to_owned()
        } else {
            "sh -c true".to_owned()
        },
        from: Some("EDITOR"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn an_edit_saves_what_the_editor_left_or_says_it_is_unchanged() {
    let (folder, store, context, [_, second, third]) = three_notes().await;
    run_list(10, None, false, false, &store).await.unwrap();

    let outcome = edit_with(&an_editor_that_changes_nothing(), &arg("2"), &context)
        .await
        .unwrap();
    assert!(matches!(outcome, EditOutcome::Unchanged(note) if note.id == second));
    let revision = store.note_revision(&second).await.unwrap();

    let editor = an_editor_that_writes(&folder, "Groceries for the party: cake\n");
    let outcome = edit_with(&editor, &arg("2"), &context).await.unwrap();
    assert!(matches!(outcome, EditOutcome::Saved(ref note) if note.id == second));
    let saved = store.get_note(&second).await.unwrap().unwrap();
    assert_eq!(saved.content, "Groceries for the party: cake");
    assert!(store.note_revision(&second).await.unwrap() > revision);

    // With nothing named, the latest: the note just edited.
    let editor = an_editor_that_writes(&folder, "Groceries: cake and candles");
    edit_with(&editor, &[], &context).await.unwrap();
    assert_eq!(
        store.get_note(&second).await.unwrap().unwrap().content,
        "Groceries: cake and candles"
    );
    assert_eq!(
        store.get_note(&third).await.unwrap().unwrap().content,
        "Call the dentist"
    );

    // Emptied, nothing is saved.
    let editor = an_editor_that_writes(&folder, "  \n");
    assert!(matches!(
        edit_with(&editor, &arg("dentist"), &context)
            .await
            .unwrap_err(),
        CliError::EmptyEditedContent
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn a_note_is_shown_whole_with_what_there_is_to_know_about_it() {
    let (folder, store, context, note) = a_note("Buy milk #shop\nand bread #home").await;
    let note_id: NoteId = note.parse().unwrap();
    let receipt = a_file(&folder, "receipt.txt", b"two items");
    run_attach(&note, &receipt, None, None, &context)
        .await
        .unwrap();
    store.archive_note(&note_id).await.unwrap();

    let shown = store.get_note(&note_id).await.unwrap().unwrap();
    let files = store.attached_files(&note_id).await.unwrap();
    let lines = show_lines(&shown, &files, shown.updated_at + 120_000);
    assert_eq!(lines[0], "Buy milk #shop");
    assert_eq!(lines[1], "and bread #home");
    assert_eq!(lines[2], "");
    assert_eq!(lines[3], format!("ID        {note}"));
    assert!(lines[4].starts_with("Edited    2m ago, "), "{lines:?}");
    assert!(lines[5].starts_with("Written   "), "{lines:?}");
    assert_eq!(lines[6], "Tags      #home #shop");
    assert!(lines[7].starts_with("Files     receipt.txt  "), "{lines:?}");
    assert!(lines[8].starts_with("Archived  yes"), "{lines:?}");

    run_show(&arg("milk"), true, &context).await.unwrap();
}

#[test]
fn show_is_read_and_tui_is_gone() {
    let parsed = |words: &[&str]| Cli::try_parse_from(words).unwrap();
    assert!(matches!(
        parsed(&["dirt", "show"]).command,
        Some(Commands::Show { note, json: false }) if note.is_empty()
    ));
    assert!(matches!(
        parsed(&["dirt", "show", "buy", "milk", "--json"]).command,
        Some(Commands::Show { note, json: true }) if note == ["buy", "milk"]
    ));
    assert!(matches!(
        parsed(&["dirt", "edit"]).command,
        Some(Commands::Edit { note }) if note.is_empty()
    ));
    assert!(matches!(
        parsed(&["dirt", "edit", "2"]).command,
        Some(Commands::Edit { note }) if note == ["2"]
    ));
    // A command that changes a note needs it named.
    assert!(Cli::try_parse_from(["dirt", "delete"]).is_err());
    // `dirt tui` is a thought now, as any other word is.
    let tui = parsed(&["dirt", "tui"]);
    assert!(tui.command.is_none());
    assert_eq!(tui.note, ["tui"]);
}
