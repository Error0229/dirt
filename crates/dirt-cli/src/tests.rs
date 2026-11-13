use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use dirt_core::export::render_markdown_export;
use dirt_core::notes::NoteDoc;
use dirt_core::{Note, NoteId, Store};
use tempfile::TempDir;
use tokio::time::sleep;

use crate::cli::{CompletionShell, ExportFormat};
use crate::commands::common::{
    default_editor, format_relative_time, list_notes, normalize_content, normalize_note_identifier,
    normalize_search_query, note_preview, resolve_note_for_edit, search_notes,
};
use crate::commands::completions::run_completions;
use crate::commands::delete::run_delete;
use crate::commands::export::run_export;
use crate::commands::sync::run_sync;
use crate::context::Context;
use crate::error::CliError;

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

#[tokio::test(flavor = "current_thread")]
async fn list_notes_respects_limit_and_tag_filter() {
    let (_folder, db_path) = temp_db();
    let store = Store::open(&db_path).unwrap();
    store.create_note("First #work").await.unwrap();
    sleep(Duration::from_millis(2)).await;
    store.create_note("Second #personal").await.unwrap();
    sleep(Duration::from_millis(2)).await;
    store.create_note("Third #work").await.unwrap();

    let recent = list_notes(2, None, &store).await.unwrap();
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0].content, "Third #work");
    assert_eq!(recent[1].content, "Second #personal");

    let work_only = list_notes(10, Some("work"), &store).await.unwrap();
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

#[test]
fn normalize_note_identifier_rejects_empty() {
    assert!(matches!(
        normalize_note_identifier(" \n "),
        Err(CliError::EmptyNoteId)
    ));
    assert_eq!(
        normalize_note_identifier("  abc123  ").unwrap(),
        "abc123".to_string()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn resolve_note_for_edit_supports_exact_and_prefix_id() {
    let (_folder, db_path) = temp_db();
    let store = Store::open(&db_path).unwrap();
    insert_note(&store, "11111111-1111-7111-8111-111111111111", "Note A").await;
    insert_note(&store, "11111111-1111-7111-8111-222222222222", "Note B").await;

    let by_exact = resolve_note_for_edit("11111111-1111-7111-8111-111111111111", &store)
        .await
        .unwrap();
    assert_eq!(by_exact.content, "Note A");

    let by_prefix = resolve_note_for_edit("11111111-1111-7111-8111-2", &store)
        .await
        .unwrap();
    assert_eq!(by_prefix.content, "Note B");
}

#[tokio::test(flavor = "current_thread")]
async fn resolve_note_for_edit_rejects_ambiguous_prefix() {
    let (_folder, db_path) = temp_db();
    let store = Store::open(&db_path).unwrap();
    insert_note(&store, "aaaaaaaa-aaaa-7aaa-8aaa-aaaaaaaaaaaa", "Left").await;
    insert_note(&store, "aaaaaaaa-aaaa-7aaa-8aaa-bbbbbbbbbbbb", "Right").await;

    let error = resolve_note_for_edit("aaaaaaaa-aaaa-7aaa-8aaa", &store)
        .await
        .unwrap_err();
    assert!(matches!(error, CliError::AmbiguousNoteId(_)));
}

#[tokio::test(flavor = "current_thread")]
async fn resolve_note_for_edit_rejects_missing_note() {
    let (_folder, db_path) = temp_db();
    let store = Store::open(&db_path).unwrap();

    let error = resolve_note_for_edit("does-not-exist", &store)
        .await
        .unwrap_err();
    assert!(matches!(error, CliError::NoteNotFound(_)));
}

#[tokio::test(flavor = "current_thread")]
async fn run_delete_soft_deletes_note_by_exact_and_prefix_id() {
    let (_folder, db_path) = temp_db();
    let store = Store::open(&db_path).unwrap();
    let note_a = insert_note(&store, "bbbbbbbb-bbbb-7bbb-8bbb-111111111111", "Keep me").await;
    let note_b = insert_note(&store, "bbbbbbbb-bbbb-7bbb-8bbb-222222222222", "Delete me").await;

    let context = without_a_server(&store);
    run_delete("bbbbbbbb-bbbb-7bbb-8bbb-2", &context)
        .await
        .unwrap();

    assert!(store.get_note(&note_b).await.unwrap().is_none());
    assert!(store.get_note(&note_a).await.unwrap().is_some());

    run_delete("bbbbbbbb-bbbb-7bbb-8bbb-111111111111", &context)
        .await
        .unwrap();

    assert!(store.get_note(&note_a).await.unwrap().is_none());
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

#[test]
fn note_to_export_item_sorts_tags() {
    let note = Note::new("#zeta test #alpha #beta");
    let export = dirt_core::export::note_to_export_item(&note);

    assert_eq!(export.tags, vec!["alpha", "beta", "zeta"]);
}

#[test]
fn render_markdown_export_includes_frontmatter_and_content() {
    let note = Note {
        id: "cccccccc-cccc-7ccc-8ccc-111111111111".parse().unwrap(),
        content: "Hello export #tag".to_string(),
        created_at: 123,
        updated_at: 456,
    };

    let rendered = render_markdown_export(&[note]);
    assert!(rendered.contains("id: cccccccc-cccc-7ccc-8ccc-111111111111"));
    assert!(rendered.contains("created_at: 123"));
    assert!(rendered.contains("updated_at: 456"));
    assert!(rendered.contains("tags:\n  - tag"));
    assert!(rendered.contains("Hello export #tag"));
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

    run_export(ExportFormat::Json, Some(&output_path), &store)
        .await
        .unwrap();

    let exported = std::fs::read_to_string(&output_path).unwrap();
    assert!(exported.contains("\"content\": \"Export me #one\""));
    assert!(exported.contains("\"tags\": [\n      \"one\"\n    ]"));

    let _ = std::fs::remove_file(output_path);
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
    }
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
    let (folder, store, context, note) = a_note("the meeting").await;
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
        files_lines(&note, &store).await.unwrap(),
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
        files_lines(&note, &store).await.unwrap(),
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
        files_lines(&note, &store).await.unwrap()[0]
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
