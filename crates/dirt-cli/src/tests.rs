use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use dirt_core::db::{Database, LibSqlNoteRepository, NoteRepository};
use dirt_core::export::render_markdown_export;
use dirt_core::Note;
use tokio::time::sleep;

use crate::cli::{CompletionShell, ExportFormat};
use crate::commands::common::{
    default_editor, format_relative_time, list_notes, normalize_content, normalize_note_identifier,
    normalize_search_query, note_preview, open_database, resolve_note_for_edit, search_notes,
    DbScope,
};
use crate::commands::completions::run_completions;
use crate::commands::delete::run_delete;
use crate::commands::export::run_export;
use crate::commands::sync::run_sync;
use crate::error::CliError;

/// Tests open the legacy single-DB layout with `SOLO_USER_ID` —
/// the per-user routing only kicks in when `state.json` is
/// present, which the test harness deliberately doesn't create.
fn scope_for(path: &Path) -> DbScope {
    DbScope {
        path: path.to_path_buf(),
        user_id: dirt_core::SOLO_USER_ID.to_string(),
    }
}

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
    let note =
        dirt_core::Note::new_for_user(
            "This is a very long sentence that should be shortened",
            dirt_core::SOLO_USER_ID,
        )
        .unwrap();
    let preview = note_preview(&note, 20);
    assert_eq!(preview, "This is a very lo...");
}

#[cfg_attr(windows, ignore = "libsql integration is flaky on windows CI")]
#[tokio::test(flavor = "current_thread")]
async fn list_notes_respects_limit_and_tag_filter() {
    let db_path = unique_test_db_path();
    {
        let db = Database::open(&db_path).await.unwrap();
        let repo = LibSqlNoteRepository::new(db.connection());

        repo.create(dirt_core::SOLO_USER_ID, "First #work").await.unwrap();
        sleep(Duration::from_millis(2)).await;
        repo.create(dirt_core::SOLO_USER_ID, "Second #personal").await.unwrap();
        sleep(Duration::from_millis(2)).await;
        repo.create(dirt_core::SOLO_USER_ID, "Third #work").await.unwrap();
    }

    let scope = scope_for(&db_path);
    let recent = list_notes(2, None, &scope).await.unwrap();
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0].content, "Third #work");
    assert_eq!(recent[1].content, "Second #personal");

    let work_only = list_notes(10, Some("work"), &scope).await.unwrap();
    assert_eq!(work_only.len(), 2);
    assert!(work_only.iter().all(|note| note.content.contains("#work")));

    cleanup_db_files(&db_path);
}

#[cfg_attr(windows, ignore = "libsql integration is flaky on windows CI")]
#[tokio::test(flavor = "current_thread")]
async fn search_notes_finds_matches_with_limit() {
    let db_path = unique_test_db_path();
    {
        let db = Database::open(&db_path).await.unwrap();
        let repo = LibSqlNoteRepository::new(db.connection());

        repo.create(dirt_core::SOLO_USER_ID, "Milk and eggs").await.unwrap();
        sleep(Duration::from_millis(2)).await;
        repo.create(dirt_core::SOLO_USER_ID, "Milkshake recipe").await.unwrap();
        sleep(Duration::from_millis(2)).await;
        repo.create(dirt_core::SOLO_USER_ID, "Unrelated note").await.unwrap();
    }

    let matches = search_notes("milk", 1, &scope_for(&db_path)).await.unwrap();
    assert_eq!(matches.len(), 1);
    assert!(matches[0].content.to_lowercase().contains("milk"));

    cleanup_db_files(&db_path);
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

#[cfg_attr(windows, ignore = "libsql integration is flaky on windows CI")]
#[tokio::test(flavor = "current_thread")]
async fn resolve_note_for_edit_supports_exact_and_prefix_id() {
    let db_path = unique_test_db_path();
    let db = Database::open(&db_path).await.unwrap();
    let repo = LibSqlNoteRepository::new(db.connection());

    let note_a = Note {
        id: "11111111-1111-7111-8111-111111111111".parse().unwrap(),
        content: "Note A".to_string(),
        created_at: 1000,
        updated_at: 1000,
        user_id: dirt_core::SOLO_USER_ID.to_string(),
        server_updated_at: None,
        deleted_at: None,
    };
    let note_b = Note {
        id: "11111111-1111-7111-8111-222222222222".parse().unwrap(),
        content: "Note B".to_string(),
        created_at: 1001,
        updated_at: 1001,
        user_id: dirt_core::SOLO_USER_ID.to_string(),
        server_updated_at: None,
        deleted_at: None,
    };
    repo.create_with_note(&note_a).await.unwrap();
    repo.create_with_note(&note_b).await.unwrap();
    drop(db);

    let service = open_database(&scope_for(&db_path)).await.unwrap();

    let by_exact = resolve_note_for_edit("11111111-1111-7111-8111-111111111111", &service)
        .await
        .unwrap();
    assert_eq!(by_exact.content, "Note A");

    let by_prefix = resolve_note_for_edit("11111111-1111-7111-8111-2", &service)
        .await
        .unwrap();
    assert_eq!(by_prefix.content, "Note B");

    cleanup_db_files(&db_path);
}

#[cfg_attr(windows, ignore = "libsql integration is flaky on windows CI")]
#[tokio::test(flavor = "current_thread")]
async fn resolve_note_for_edit_rejects_ambiguous_prefix() {
    let db_path = unique_test_db_path();
    let db = Database::open(&db_path).await.unwrap();
    let repo = LibSqlNoteRepository::new(db.connection());

    let note_a = Note {
        id: "aaaaaaaa-aaaa-7aaa-8aaa-aaaaaaaaaaaa".parse().unwrap(),
        content: "Left".to_string(),
        created_at: 1000,
        updated_at: 1000,
        user_id: dirt_core::SOLO_USER_ID.to_string(),
        server_updated_at: None,
        deleted_at: None,
    };
    let note_b = Note {
        id: "aaaaaaaa-aaaa-7aaa-8aaa-bbbbbbbbbbbb".parse().unwrap(),
        content: "Right".to_string(),
        created_at: 1001,
        updated_at: 1001,
        user_id: dirt_core::SOLO_USER_ID.to_string(),
        server_updated_at: None,
        deleted_at: None,
    };
    repo.create_with_note(&note_a).await.unwrap();
    repo.create_with_note(&note_b).await.unwrap();
    drop(db);

    let service = open_database(&scope_for(&db_path)).await.unwrap();

    let error = resolve_note_for_edit("aaaaaaaa-aaaa-7aaa-8aaa", &service)
        .await
        .unwrap_err();
    assert!(matches!(error, CliError::AmbiguousNoteId(_)));

    cleanup_db_files(&db_path);
}

#[cfg_attr(windows, ignore = "libsql integration is flaky on windows CI")]
#[tokio::test(flavor = "current_thread")]
async fn resolve_note_for_edit_rejects_missing_note() {
    let db_path = unique_test_db_path();
    let service = open_database(&scope_for(&db_path)).await.unwrap();

    let error = resolve_note_for_edit("does-not-exist", &service)
        .await
        .unwrap_err();
    assert!(matches!(error, CliError::NoteNotFound(_)));

    cleanup_db_files(&db_path);
}

#[cfg_attr(windows, ignore = "libsql integration is flaky on windows CI")]
#[tokio::test(flavor = "current_thread")]
async fn run_delete_soft_deletes_note_by_exact_and_prefix_id() {
    let db_path = unique_test_db_path();
    let db = Database::open(&db_path).await.unwrap();
    let repo = LibSqlNoteRepository::new(db.connection());

    let note_a = Note {
        id: "bbbbbbbb-bbbb-7bbb-8bbb-111111111111".parse().unwrap(),
        content: "Keep me".to_string(),
        created_at: 1000,
        updated_at: 1000,
        user_id: dirt_core::SOLO_USER_ID.to_string(),
        server_updated_at: None,
        deleted_at: None,
    };
    let note_b = Note {
        id: "bbbbbbbb-bbbb-7bbb-8bbb-222222222222".parse().unwrap(),
        content: "Delete me".to_string(),
        created_at: 1001,
        updated_at: 1001,
        user_id: dirt_core::SOLO_USER_ID.to_string(),
        server_updated_at: None,
        deleted_at: None,
    };
    repo.create_with_note(&note_a).await.unwrap();
    repo.create_with_note(&note_b).await.unwrap();
    drop(db);

    run_delete("bbbbbbbb-bbbb-7bbb-8bbb-2", &scope_for(&db_path))
        .await
        .unwrap();

    let db = Database::open(&db_path).await.unwrap();
    let repo = LibSqlNoteRepository::new(db.connection());
    assert!(repo.get(&note_b.id).await.unwrap().is_none());
    assert!(repo.get(&note_a.id).await.unwrap().is_some());
    drop(db);

    run_delete("bbbbbbbb-bbbb-7bbb-8bbb-111111111111", &scope_for(&db_path))
        .await
        .unwrap();

    let db = Database::open(&db_path).await.unwrap();
    let repo = LibSqlNoteRepository::new(db.connection());
    assert!(repo.get(&note_a.id).await.unwrap().is_none());

    cleanup_db_files(&db_path);
}

#[cfg_attr(windows, ignore = "libsql integration is flaky on windows CI")]
#[tokio::test(flavor = "current_thread")]
#[allow(unsafe_code)]
async fn run_sync_requires_sync_configuration() {
    let db_path = unique_test_db_path();

    // Force the unconfigured path so the test doesn't accidentally pick
    // up a developer's real DIRT_API_BASE_URL. Post-2.8 there is no
    // client-side bearer env var to clear — auth flows through the
    // keyring, which the CLI guards against silently in
    // `commands::sync::build_session` (Auth error if empty).
    // SAFETY: tests in this module run on a current_thread runtime, so
    // there's no concurrent reader of these vars within the process.
    unsafe {
        std::env::remove_var("DIRT_API_BASE_URL");
        std::env::set_var("DIRT_PROFILE", "this-profile-does-not-exist");
    }

    let error = run_sync(&scope_for(&db_path)).await.unwrap_err();
    // No api_base_url is `SyncNotConfigured`. The contract is that
    // `dirt sync` refuses to run rather than silently no-oping.
    assert!(
        matches!(error, CliError::SyncNotConfigured | CliError::Config(_)),
        "unexpected error variant: {error:?}",
    );

    unsafe {
        std::env::remove_var("DIRT_PROFILE");
    }
    cleanup_db_files(&db_path);
}

#[test]
fn note_to_export_item_sorts_tags() {
    let note = Note::new_for_user("#zeta test #alpha #beta", dirt_core::SOLO_USER_ID).unwrap();
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
        user_id: dirt_core::SOLO_USER_ID.to_string(),
        server_updated_at: None,
        deleted_at: None,
    };

    let rendered = render_markdown_export(&[note]);
    assert!(rendered.contains("id: cccccccc-cccc-7ccc-8ccc-111111111111"));
    assert!(rendered.contains("created_at: 123"));
    assert!(rendered.contains("updated_at: 456"));
    assert!(rendered.contains("tags:\n  - tag"));
    assert!(rendered.contains("Hello export #tag"));
}

#[cfg_attr(windows, ignore = "libsql integration is flaky on windows CI")]
#[tokio::test(flavor = "current_thread")]
async fn run_export_writes_json_file() {
    let db_path = unique_test_db_path();
    {
        let db = Database::open(&db_path).await.unwrap();
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.create(dirt_core::SOLO_USER_ID, "Export me #one").await.unwrap();
    }

    let output_path = std::env::temp_dir().join(format!(
        "dirt-export-test-{}.json",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos())
    ));

    run_export(ExportFormat::Json, Some(&output_path), &scope_for(&db_path))
        .await
        .unwrap();

    let exported = std::fs::read_to_string(&output_path).unwrap();
    assert!(exported.contains("\"content\": \"Export me #one\""));
    assert!(exported.contains("\"tags\": [\n      \"one\"\n    ]"));

    let _ = std::fs::remove_file(output_path);
    cleanup_db_files(&db_path);
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

fn unique_test_db_path() -> PathBuf {
    static NEXT_TEST_DB_ID: AtomicU64 = AtomicU64::new(0);

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let sequence = NEXT_TEST_DB_ID.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("dirt-cli-list-test-{timestamp}-{sequence}.db"))
}

fn cleanup_db_files(path: &PathBuf) {
    // On Windows, libsql can keep file handles alive briefly after drop.
    // Removing test DB files eagerly can trigger intermittent access violations.
    if cfg!(windows) {
        return;
    }

    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(path.with_extension("db-shm"));
    let _ = std::fs::remove_file(path.with_extension("db-wal"));
}
