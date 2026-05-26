//! End-to-end coverage for the per-user local-DB partitioning that
//! closes issue #234.
//!
//! These tests exercise the active-user pointer + per-user DB path
//! plumbing the way `dirt-cli` will use it in production: write
//! `state.json` to mark which account "owns" this machine right now,
//! resolve the DB path through `resolve_active_db`, and open a
//! `DatabaseService` for that user. Two accounts on the same machine
//! must see strictly separate stores; switching between them is
//! purely a pointer-rewrite and a re-open of a different file.
//!
//! Lives as an integration test (under `tests/`) because the contract
//! it pins down is the public surface of `dirt_core::services` — the
//! same surface `dirt-cli`'s `commands::common::resolve_db_scope`
//! calls into. A regression here means the security fix has broken.

use dirt_core::services::db_paths::{
    migrate_solo_db_to_user, read_active_user, resolve_active_db, solo_db_path, user_db_path,
    write_active_user, SoloMigrationOutcome,
};
use dirt_core::services::DatabaseService;
use dirt_core::SOLO_USER_ID;
use tempfile::TempDir;

const USER_A: &str = "01932aaa-0000-7000-8000-000000000001";
const USER_B: &str = "01932bbb-0000-7000-8000-000000000002";

/// Headline test for issue #234.
///
/// User A signs in → notes go into `<A>/dirt.db`, stamped A.
/// User B signs in → notes go into `<B>/dirt.db`, stamped B. A's
/// rows must NOT be visible from B's DB, and vice versa. Switching
/// back to A still surfaces A's original rows untouched.
#[tokio::test(flavor = "current_thread")]
async fn two_user_ids_get_separate_dbs() {
    let dir = TempDir::new().expect("tempdir");

    // ---- User A signs in ----
    write_active_user(dir.path(), USER_A)
        .await
        .expect("write active user A");
    let (path_a, uid_a) = resolve_active_db(dir.path())
        .await
        .expect("resolve A's DB");
    assert_eq!(uid_a, USER_A);
    assert_eq!(path_a, user_db_path(dir.path(), USER_A).unwrap());

    let db_a = DatabaseService::open_for_user(path_a.clone(), USER_A)
        .await
        .expect("open A");
    let note_a = db_a
        .create_note("A's private note")
        .await
        .expect("create note for A");
    assert_eq!(
        note_a.user_id, USER_A,
        "new notes must be stamped with the active user's id"
    );
    drop(db_a);

    // ---- User B signs in (rewrites state.json) ----
    write_active_user(dir.path(), USER_B)
        .await
        .expect("write active user B");
    let (path_b, uid_b) = resolve_active_db(dir.path())
        .await
        .expect("resolve B's DB");
    assert_eq!(uid_b, USER_B);
    assert_ne!(
        path_b, path_a,
        "different users must resolve to different DB files"
    );

    let db_b = DatabaseService::open_for_user(path_b.clone(), USER_B)
        .await
        .expect("open B");
    db_b.create_note("B's private note")
        .await
        .expect("create note for B");

    // B's DB sees only B's notes — A's row is in a different file on
    // disk, not visible from this `DatabaseService`.
    let listed_by_b = db_b.list_notes(10, 0).await.expect("list B");
    assert_eq!(listed_by_b.len(), 1, "B must not see A's notes");
    assert_eq!(listed_by_b[0].content, "B's private note");
    assert_eq!(listed_by_b[0].user_id, USER_B);
    drop(db_b);

    // ---- Switch back to A ----
    write_active_user(dir.path(), USER_A)
        .await
        .expect("re-write active user A");
    let (path_a2, uid_a2) = resolve_active_db(dir.path())
        .await
        .expect("resolve A's DB again");
    assert_eq!(uid_a2, USER_A);
    assert_eq!(path_a2, path_a);
    let db_a2 = DatabaseService::open_for_user(path_a2, USER_A)
        .await
        .expect("re-open A");

    // A's row is still there, untouched by B's intervening writes.
    let listed_by_a = db_a2.list_notes(10, 0).await.expect("list A");
    assert_eq!(listed_by_a.len(), 1, "A's data must survive B's session");
    assert_eq!(listed_by_a[0].content, "A's private note");
    assert_eq!(listed_by_a[0].user_id, USER_A);
}

/// On a fresh machine that has never signed in, the resolver returns
/// the legacy `dirt.db` location and SOLO_USER_ID. This is the only
/// state where the legacy location is ever the answer — the moment
/// the user signs in for the first time, the migration moves it.
#[tokio::test(flavor = "current_thread")]
async fn pre_signin_resolves_to_legacy_solo_layout() {
    let dir = TempDir::new().expect("tempdir");
    let (path, user_id) = resolve_active_db(dir.path()).await.unwrap();
    assert_eq!(path, solo_db_path(dir.path()));
    assert_eq!(user_id, SOLO_USER_ID);
    assert!(read_active_user(dir.path()).await.unwrap().is_none());
}

/// First sign-in on a machine that has Phase-1 capture history:
/// the legacy `dirt.db` is moved into `<user_id>/dirt.db` and every
/// row's `user_id` column is rewritten from SOLO_USER_ID. The user
/// sees their pre-upgrade notes in their per-user DB; the legacy
/// path no longer exists.
#[tokio::test(flavor = "current_thread")]
async fn first_signin_migrates_legacy_data_into_user_dir() {
    let dir = TempDir::new().expect("tempdir");

    // Seed a legacy solo DB the way Phase 1 left it.
    let legacy_path = solo_db_path(dir.path());
    let solo = DatabaseService::open_local_path(legacy_path.clone())
        .await
        .unwrap();
    solo.create_note("legacy phase 1 capture").await.unwrap();
    drop(solo);

    // First sign-in as user A runs the migration.
    let outcome = migrate_solo_db_to_user(dir.path(), USER_A).await.unwrap();
    assert_eq!(outcome, SoloMigrationOutcome::Migrated);
    write_active_user(dir.path(), USER_A).await.unwrap();

    // Legacy file gone, per-user file present.
    assert!(!tokio::fs::try_exists(&legacy_path).await.unwrap());
    let user_path = user_db_path(dir.path(), USER_A).unwrap();
    assert!(tokio::fs::try_exists(&user_path).await.unwrap());

    // The pre-upgrade note is now A's note.
    let (path, uid) = resolve_active_db(dir.path()).await.unwrap();
    assert_eq!(uid, USER_A);
    let db = DatabaseService::open_for_user(path, USER_A).await.unwrap();
    let notes = db.list_notes(10, 0).await.unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].content, "legacy phase 1 capture");
    assert_eq!(
        notes[0].user_id, USER_A,
        "migration must rewrite SOLO_USER_ID rows to the new owner"
    );
}

/// Sign-out preserves the active-user pointer per the design: local
/// writes done after sign-out still target the same user's DB, and
/// the next sign-in for that user picks up exactly where they left
/// off. (Sign-out itself doesn't run in this test — we just verify
/// that leaving `state.json` in place behaves as the design demands.)
#[tokio::test(flavor = "current_thread")]
async fn signout_preserves_db_ownership() {
    let dir = TempDir::new().expect("tempdir");
    write_active_user(dir.path(), USER_A).await.unwrap();

    let (path, _) = resolve_active_db(dir.path()).await.unwrap();
    let db = DatabaseService::open_for_user(path.clone(), USER_A)
        .await
        .unwrap();
    db.create_note("captured while signed in").await.unwrap();
    drop(db);

    // ... user signs out (keyring cleared, state.json untouched) ...
    // Subsequent offline capture still targets A's DB:
    let (path_again, uid_again) = resolve_active_db(dir.path()).await.unwrap();
    assert_eq!(uid_again, USER_A);
    let db2 = DatabaseService::open_for_user(path_again, USER_A)
        .await
        .unwrap();
    db2.create_note("captured while signed out")
        .await
        .unwrap();

    let notes = db2.list_notes(10, 0).await.unwrap();
    assert_eq!(notes.len(), 2);
    assert!(notes.iter().all(|n| n.user_id == USER_A));
}
