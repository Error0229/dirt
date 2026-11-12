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

    let error = run_sync(&without_a_server(&store)).await.unwrap_err();

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
        ["No server is configured. Notes are kept on this device only."]
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
