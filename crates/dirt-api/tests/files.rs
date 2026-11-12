//! Files attached to notes, against Postgres and a real S3-compatible store.
//!
//! The server signs links and keeps count. The bytes go straight between a
//! device and the store, so these tests play the device: they upload and
//! download with the links they are given, and look in the store to see
//! what is really there.
//!
//! Start both with `docker compose up -d` at the repository root and set
//! `DATABASE_URL` as `compose.yaml` shows.

mod common;

use axum::http::{Method, StatusCode};
use axum::Router;
use common::{
    app, call, file_bucket, file_store, get, note_id, post, push_one, register, state, Answer,
    CRON_SECRET,
};
use dirt_api::{build_router, FileStore};
use dirt_proto::api::{
    limits, DownloadLink, ErrorCode, FileOutcome, FileResults, RegisterDeviceResponse, UploadLink,
};
use dirt_proto::NoteId;
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

async fn ask_to_upload(app: &Router, token: &str, file: Uuid, size: usize) -> Answer {
    post(
        app,
        "/v1/files",
        Some(token),
        json!({ "file_id": file, "size": size }),
    )
    .await
}

/// Sends `bytes` to the store through an upload link, as a device does.
async fn put(link: &str, bytes: &[u8]) -> reqwest::StatusCode {
    reqwest::Client::new()
        .put(link)
        .body(bytes.to_vec())
        .send()
        .await
        .expect("the store answers")
        .status()
}

/// Asks for a link and uploads `bytes` through it.
async fn upload(app: &Router, token: &str, bytes: &[u8]) -> Uuid {
    let file = Uuid::now_v7();
    let link: UploadLink = ask_to_upload(app, token, file, bytes.len()).await.json();
    assert_eq!(link.expires_in_secs, limits::UPLOAD_LINK_SECS);
    assert!(put(&link.url, bytes).await.is_success());
    file
}

fn naming(file: Uuid, note: NoteId) -> Value {
    json!({ "file_id": file, "note_id": note })
}

async fn tell(app: &Router, what: &str, token: &str, items: Vec<Value>) -> Vec<FileOutcome> {
    post(
        app,
        &format!("/v1/files/{what}"),
        Some(token),
        json!({ "items": items }),
    )
    .await
    .json::<FileResults>()
    .results
    .into_iter()
    .map(|result| result.outcome)
    .collect()
}

async fn confirm(app: &Router, token: &str, file: Uuid, note: NoteId) -> FileOutcome {
    tell(app, "confirm", token, vec![naming(file, note)])
        .await
        .remove(0)
}

async fn release(app: &Router, token: &str, file: Uuid, note: NoteId) -> FileOutcome {
    tell(app, "release", token, vec![naming(file, note)])
        .await
        .remove(0)
}

/// A note of the account's that the server holds.
async fn a_note(app: &Router, token: &str) -> NoteId {
    let note = note_id();
    push_one(app, token, note, 0, b"a note").await;
    note
}

/// Fetches a file's bytes through a download link, or the store's refusal.
async fn fetch(app: &Router, token: &str, file: Uuid) -> Result<Vec<u8>, reqwest::StatusCode> {
    let link: DownloadLink = get(app, &format!("/v1/files/{file}"), Some(token))
        .await
        .json();
    let answer = reqwest::get(&link.url).await.expect("the store answers");
    if answer.status().is_success() {
        Ok(answer.bytes().await.unwrap().to_vec())
    } else {
        Err(answer.status())
    }
}

/// What the store itself holds for a file: its size, or nothing.
async fn stored(device: &RegisterDeviceResponse, file: Uuid) -> Option<i64> {
    file_store()
        .size_of(&FileStore::object_key(device.account_id, file))
        .await
        .unwrap()
}

async fn file_bytes(pool: &PgPool, account: Uuid) -> i64 {
    let (bytes,): (i64,) = sqlx::query_as("SELECT file_bytes FROM accounts WHERE id = $1")
        .bind(account)
        .fetch_one(pool)
        .await
        .unwrap();
    bytes
}

/// A file's row: whether it is confirmed, and in how many days it is due to
/// be deleted, if it is.
async fn row(pool: &PgPool, file: Uuid) -> Option<(bool, Option<f64>)> {
    sqlx::query_as(
        "SELECT confirmed, (extract(epoch FROM delete_after - now()) / 86400)::float8 \
           FROM files WHERE file_id = $1",
    )
    .bind(file)
    .fetch_optional(pool)
    .await
    .unwrap()
}

fn about(days: Option<f64>, expected: i64) -> bool {
    let expected = f64::from(i32::try_from(expected).expect("a small number of days"));
    days.is_some_and(|days| (days - expected).abs() < 0.01)
}

async fn make_due(pool: &PgPool, file: Uuid) {
    sqlx::query("UPDATE files SET delete_after = now() - interval '1 minute' WHERE file_id = $1")
        .bind(file)
        .execute(pool)
        .await
        .unwrap();
}

/// Lets the objects waiting to be deleted have waited long enough: an hour,
/// so that no upload link for one of them can still be in use.
async fn the_wait_is_over(pool: &PgPool) {
    sqlx::query("UPDATE orphan_objects SET created_at = created_at - interval '2 hours'")
        .execute(pool)
        .await
        .unwrap();
}

async fn sweep_all(app: &Router) -> Answer {
    call(
        app,
        Method::GET,
        "/v1/maintenance/sweep",
        Some(CRON_SECRET),
        None,
    )
    .await
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_file_goes_up_through_one_link_and_comes_down_through_another(pool: PgPool) {
    file_bucket().await;
    let app = app(pool.clone());
    let device = register(&app, "alice", "Laptop").await;
    let note = a_note(&app, &device.token).await;
    let bytes = b"ciphertext, as far as the server is concerned";

    let file = upload(&app, &device.token, bytes).await;

    // Counted from the moment the link was issued, and due to be discarded
    // in a week if no note ever refers to it.
    assert_eq!(file_bytes(&pool, device.account_id).await, 45);
    let (confirmed, due) = row(&pool, file).await.unwrap();
    assert!(!confirmed && about(due, limits::UNCONFIRMED_FILE_DAYS));

    assert_eq!(
        confirm(&app, &device.token, file, note).await,
        FileOutcome::Done
    );
    assert_eq!(row(&pool, file).await, Some((true, None)));

    let link: DownloadLink = get(&app, &format!("/v1/files/{file}"), Some(&device.token))
        .await
        .json();
    assert_eq!(
        (link.size, link.expires_in_secs),
        (45, limits::DOWNLOAD_LINK_SECS)
    );
    assert_eq!(fetch(&app, &device.token, file).await.unwrap(), bytes);

    // Confirming again changes nothing.
    assert_eq!(
        confirm(&app, &device.token, file, note).await,
        FileOutcome::Done
    );
    assert_eq!(file_bytes(&pool, device.account_id).await, 45);
}

/// A device cannot store more than it asked for and was counted for. The
/// store refuses a body of another length, because the length is part of
/// what the link's signature covers. And whatever the store holds, the
/// server looks before it counts a file as there.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_link_takes_only_the_size_it_was_signed_for(pool: PgPool) {
    file_bucket().await;
    let app = app(pool.clone());
    let device = register(&app, "alice", "Laptop").await;
    let note = a_note(&app, &device.token).await;
    let file = Uuid::now_v7();
    let link: UploadLink = ask_to_upload(&app, &device.token, file, 10).await.json();

    for wrong in [&b"too long for the link"[..], &b"short"[..]] {
        let status = put(&link.url, wrong).await;
        assert!(
            status.is_client_error(),
            "a body of {} bytes on a link for 10 was answered {status}",
            wrong.len()
        );
    }
    assert_eq!(stored(&device, file).await, None);
    assert_eq!(
        confirm(&app, &device.token, file, note).await,
        FileOutcome::NotUploaded
    );

    // The right length goes in, and the file can then be confirmed.
    assert!(put(&link.url, b"ten bytes!").await.is_success());
    assert_eq!(
        confirm(&app, &device.token, file, note).await,
        FileOutcome::Done
    );
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_file_is_confirmed_only_if_it_and_its_note_are_there(pool: PgPool) {
    file_bucket().await;
    let app = app(pool.clone());
    let device = register(&app, "alice", "Laptop").await;
    let note = a_note(&app, &device.token).await;

    // A file the server was never asked about.
    assert_eq!(
        confirm(&app, &device.token, Uuid::now_v7(), note).await,
        FileOutcome::NoSuchFile
    );
    // A link was issued and nothing was sent.
    let never_sent = Uuid::now_v7();
    ask_to_upload(&app, &device.token, never_sent, 5).await;
    assert_eq!(
        confirm(&app, &device.token, never_sent, note).await,
        FileOutcome::NotUploaded
    );

    let file = upload(&app, &device.token, b"bytes").await;
    // A note the server does not hold: it has not synced yet.
    assert_eq!(
        confirm(&app, &device.token, file, note_id()).await,
        FileOutcome::NoSuchNote
    );
    // A note that was purged refers to nothing.
    let purged = a_note(&app, &device.token).await;
    let answer = post(
        &app,
        "/v1/notes/purge",
        Some(&device.token),
        json!({ "items": [{ "note_id": purged, "base_change": 2 }] }),
    )
    .await;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(
        confirm(&app, &device.token, file, purged).await,
        FileOutcome::NoSuchNote
    );
    // None of that made the file any note's.
    let (confirmed, due) = row(&pool, file).await.unwrap();
    assert!(!confirmed && about(due, limits::UNCONFIRMED_FILE_DAYS));

    assert_eq!(
        confirm(&app, &device.token, file, note).await,
        FileOutcome::Done
    );
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn an_account_holds_only_so_much_and_a_file_is_only_so_large(pool: PgPool) {
    file_bucket().await;
    let app = app(pool.clone());
    let device = register(&app, "alice", "Laptop").await;
    let token = &device.token;

    let too_large = usize::try_from(limits::MAX_FILE_BYTES).unwrap() + 1;
    let answer = ask_to_upload(&app, token, Uuid::now_v7(), too_large).await;
    assert_eq!(answer.status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(answer.code(), "file_too_large");
    for size in [0, -5] {
        let answer = post(
            &app,
            "/v1/files",
            Some(token),
            json!({ "file_id": Uuid::now_v7(), "size": size }),
        )
        .await;
        assert_eq!(answer.code(), "invalid_request", "size {size}");
    }

    // The account has ten bytes left.
    sqlx::query("UPDATE accounts SET file_bytes = $1")
        .bind(limits::MAX_ACCOUNT_FILE_BYTES - 10)
        .execute(&pool)
        .await
        .unwrap();
    let answer = ask_to_upload(&app, token, Uuid::now_v7(), 11).await;
    assert_eq!(answer.status, StatusCode::INSUFFICIENT_STORAGE);
    assert_eq!(answer.code(), "quota_exceeded");
    let fits = Uuid::now_v7();
    assert_eq!(
        ask_to_upload(&app, token, fits, 10).await.status,
        StatusCode::OK
    );
    assert_eq!(
        file_bytes(&pool, device.account_id).await,
        limits::MAX_ACCOUNT_FILE_BYTES
    );

    // Asking again for the same file is how an upload is tried again. It
    // is not counted twice, though the account is full.
    assert_eq!(
        ask_to_upload(&app, token, fits, 10).await.status,
        StatusCode::OK
    );
    assert_eq!(
        file_bytes(&pool, device.account_id).await,
        limits::MAX_ACCOUNT_FILE_BYTES
    );
    // A file never changes, so neither does its size.
    let answer = ask_to_upload(&app, token, fits, 9).await;
    assert_eq!(answer.code(), "invalid_request");
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn one_account_never_reaches_anothers_file(pool: PgPool) {
    file_bucket().await;
    let app = app(pool.clone());
    let alice = register(&app, "alice", "Laptop").await;
    let bob = register(&app, "bob", "Laptop").await;
    let alices_note = a_note(&app, &alice.token).await;
    let bobs_note = a_note(&app, &bob.token).await;
    let file = upload(&app, &alice.token, b"alice's file").await;
    confirm(&app, &alice.token, file, alices_note).await;

    // To Bob's account there is no such file, to fetch, confirm or release.
    let answer = get(&app, &format!("/v1/files/{file}"), Some(&bob.token)).await;
    assert_eq!(answer.status, StatusCode::NOT_FOUND);
    assert_eq!(
        confirm(&app, &bob.token, file, bobs_note).await,
        FileOutcome::NoSuchFile
    );
    assert_eq!(
        release(&app, &bob.token, file, alices_note).await,
        FileOutcome::NoSuchFile
    );
    // Nor can Alice's file be confirmed for Bob's note.
    assert_eq!(
        confirm(&app, &alice.token, file, bobs_note).await,
        FileOutcome::NoSuchNote
    );

    // Bob uploads a file under the same ID. It is another object, and
    // Alice's is as it was.
    let link: UploadLink = ask_to_upload(&app, &bob.token, file, 5).await.json();
    assert!(put(&link.url, b"bob's").await.is_success());
    assert_eq!(
        fetch(&app, &alice.token, file).await.unwrap(),
        b"alice's file"
    );
    assert_eq!(fetch(&app, &bob.token, file).await.unwrap(), b"bob's");
    assert_eq!(file_bytes(&pool, alice.account_id).await, 12);
    assert_eq!(file_bytes(&pool, bob.account_id).await, 5);
}

/// A file is kept for as long as any note refers to it, and for thirty days
/// after the last one stops. Two notes can refer to one file: two devices
/// that each kept a purged note's text as a new note of their own.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_file_starts_its_last_thirty_days_when_its_last_note_lets_go(pool: PgPool) {
    file_bucket().await;
    let app = app(pool.clone());
    let device = register(&app, "alice", "Laptop").await;
    let token = &device.token;
    let (first, second) = (a_note(&app, token).await, a_note(&app, token).await);
    let file = upload(&app, token, b"shared").await;
    assert_eq!(confirm(&app, token, file, first).await, FileOutcome::Done);
    assert_eq!(confirm(&app, token, file, second).await, FileOutcome::Done);
    // Counted once, however many notes refer to it.
    assert_eq!(file_bytes(&pool, device.account_id).await, 6);

    // The first note is purged. The second still refers to the file.
    let answer = post(
        &app,
        "/v1/notes/purge",
        Some(token),
        json!({ "items": [{ "note_id": first, "base_change": 1 }] }),
    )
    .await;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(row(&pool, file).await, Some((true, None)));

    // The file is removed from the second note.
    assert_eq!(release(&app, token, file, second).await, FileOutcome::Done);
    let (_, due) = row(&pool, file).await.unwrap();
    assert!(about(due, limits::UNREFERENCED_FILE_DAYS), "{due:?}");
    // Released again, the days are not started again.
    sqlx::query("UPDATE files SET delete_after = delete_after - interval '10 days'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(release(&app, token, file, second).await, FileOutcome::Done);
    let (_, due) = row(&pool, file).await.unwrap();
    assert!(about(due, limits::UNREFERENCED_FILE_DAYS - 10), "{due:?}");
    // The file is still there to fetch meanwhile.
    assert_eq!(fetch(&app, token, file).await.unwrap(), b"shared");

    // A device that was away comes back with a note that needs the file.
    assert_eq!(confirm(&app, token, file, second).await, FileOutcome::Done);
    assert_eq!(row(&pool, file).await, Some((true, None)));

    // The last note is purged: thirty days from now.
    let answer = post(
        &app,
        "/v1/notes/purge",
        Some(token),
        json!({ "items": [{ "note_id": second, "base_change": 2 }] }),
    )
    .await;
    assert_eq!(answer.status, StatusCode::OK);
    let (_, due) = row(&pool, file).await.unwrap();
    assert!(about(due, limits::UNREFERENCED_FILE_DAYS), "{due:?}");
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_note_refers_to_only_so_many_files(pool: PgPool) {
    file_bucket().await;
    let app = app(pool.clone());
    let device = register(&app, "alice", "Laptop").await;
    let token = &device.token;
    let (note, other) = (a_note(&app, token).await, a_note(&app, token).await);
    let mut files = Vec::new();
    for _ in 0..=limits::MAX_FILES_PER_NOTE {
        files.push(upload(&app, token, b"x").await);
    }
    let one_too_many = files.pop().unwrap();
    let items = files.iter().map(|file| naming(*file, note)).collect();
    let outcomes = tell(&app, "confirm", token, items).await;
    assert!(outcomes.iter().all(|outcome| *outcome == FileOutcome::Done));

    assert!(matches!(
        confirm(&app, token, one_too_many, note).await,
        FileOutcome::Rejected {
            code: ErrorCode::QuotaExceeded,
            ..
        }
    ));
    // Another note has room, and a file already confirmed stays so.
    assert_eq!(
        confirm(&app, token, one_too_many, other).await,
        FileOutcome::Done
    );
    assert_eq!(
        confirm(&app, token, files[0], note).await,
        FileOutcome::Done
    );
}

/// What is due goes when the account next asks for a link, and once a day
/// for every account. The row goes and the space comes back at once; the
/// object goes from the store an hour later.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn what_is_due_is_deleted_from_the_store(pool: PgPool) {
    file_bucket().await;
    let app = app(pool.clone());
    let alice = register(&app, "alice", "Laptop").await;
    let bob = register(&app, "bob", "Laptop").await;
    let note = a_note(&app, &alice.token).await;
    let kept = upload(&app, &alice.token, b"kept").await;
    confirm(&app, &alice.token, kept, note).await;
    let abandoned = upload(&app, &alice.token, b"abandoned").await;
    let bobs = upload(&app, &bob.token, b"bob's, abandoned").await;
    assert_eq!(file_bytes(&pool, alice.account_id).await, 13);
    make_due(&pool, abandoned).await;
    make_due(&pool, bobs).await;

    // Alice asks for another link. Her due file is no longer hers; Bob's
    // is not hers to sweep, and her kept file is not due.
    ask_to_upload(&app, &alice.token, Uuid::now_v7(), 1).await;

    assert_eq!(row(&pool, abandoned).await, None);
    assert_eq!(file_bytes(&pool, alice.account_id).await, 4 + 1);
    assert!(row(&pool, bobs).await.is_some());
    // A link to a file that has gone is no longer given.
    let answer = get(&app, &format!("/v1/files/{abandoned}"), Some(&alice.token)).await;
    assert_eq!(answer.status, StatusCode::NOT_FOUND);
    // Its object waits, and asking again does not hurry it.
    assert_eq!(stored(&alice, abandoned).await, Some(9));
    ask_to_upload(&app, &alice.token, Uuid::now_v7(), 1).await;
    assert_eq!(stored(&alice, abandoned).await, Some(9));

    // An hour on, her next request for a link deletes it.
    the_wait_is_over(&pool).await;
    ask_to_upload(&app, &alice.token, Uuid::now_v7(), 1).await;
    assert_eq!(stored(&alice, abandoned).await, None);
    assert_eq!(stored(&alice, kept).await, Some(4));
    assert_eq!(queued(&pool).await, 0);

    // The daily request does it for everyone: Bob's row today, and his
    // object the day after.
    let answer = sweep_all(&app).await;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(answer.body["deleted"], 0);
    assert_eq!(row(&pool, bobs).await, None);
    assert_eq!(file_bytes(&pool, bob.account_id).await, 0);
    assert_eq!(stored(&bob, bobs).await, Some(16));
    the_wait_is_over(&pool).await;
    assert_eq!(sweep_all(&app).await.body["deleted"], 1);
    assert_eq!(stored(&bob, bobs).await, None);
    assert_eq!(stored(&alice, kept).await, Some(4));
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn the_daily_request_is_for_the_one_who_holds_its_secret(pool: PgPool) {
    file_bucket().await;
    let app = app(pool.clone());
    let device = register(&app, "alice", "Laptop").await;
    let file = upload(&app, &device.token, b"abandoned").await;
    make_due(&pool, file).await;

    // To anyone else there is no such address: no secret, a wrong one, or
    // a device's own token.
    for token in [None, Some("a guess"), Some(device.token.as_str())] {
        let answer = call(&app, Method::GET, "/v1/maintenance/sweep", token, None).await;
        assert_eq!(answer.status, StatusCode::NOT_FOUND, "{token:?}");
    }
    assert_eq!(stored(&device, file).await, Some(9));

    // A server that was given no secret has no such address at all.
    let mut without = state(pool.clone());
    let mut config = (*without.config).clone();
    config.cron_secret = None;
    without.config = std::sync::Arc::new(config);
    let answer = call(
        &build_router(without),
        Method::GET,
        "/v1/maintenance/sweep",
        Some(CRON_SECRET),
        None,
    )
    .await;
    assert_eq!(answer.status, StatusCode::NOT_FOUND);
    assert_eq!(stored(&device, file).await, Some(9));
}

/// Deleting or resetting an account takes its files with it. The rows go
/// at once; the objects are queued, and the daily request deletes them.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn an_account_that_is_deleted_or_reset_takes_its_files_with_it(pool: PgPool) {
    file_bucket().await;
    let app = app(pool.clone());
    let alice = register(&app, "alice", "Laptop").await;
    let bob = register(&app, "bob", "Laptop").await;
    let alices_note = a_note(&app, &alice.token).await;
    let alices = upload(&app, &alice.token, b"alice's").await;
    confirm(&app, &alice.token, alices, alices_note).await;
    let bobs_note = a_note(&app, &bob.token).await;
    let bobs = upload(&app, &bob.token, b"bob's").await;
    confirm(&app, &bob.token, bobs, bobs_note).await;

    let reset = post(&app, "/v1/account/reset", Some(&alice.token), json!({})).await;
    assert_eq!(reset.status, StatusCode::OK);
    assert_eq!(row(&pool, alices).await, None);
    assert_eq!(file_bytes(&pool, alice.account_id).await, 0);
    let deleted = call(&app, Method::DELETE, "/v1/account", Some(&bob.token), None).await;
    assert_eq!(deleted.status, StatusCode::OK);

    // The objects are still in the store, and remembered.
    assert_eq!(stored(&alice, alices).await, Some(7));
    assert_eq!(stored(&bob, bobs).await, Some(5));
    let carol = register(&app, "carol", "Laptop").await;
    let (queued,): (i64,) = sqlx::query_as("SELECT count(*) FROM orphan_objects")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(queued, 2);
    // Not at once: see the test of a late upload.
    assert_eq!(sweep_all(&app).await.body["deleted"], 0);

    the_wait_is_over(&pool).await;
    // Another account asking for a link deletes what it has waiting, which
    // is nothing. These are not its objects.
    ask_to_upload(&app, &carol.token, Uuid::now_v7(), 1).await;
    assert_eq!(stored(&alice, alices).await, Some(7));
    assert_eq!(stored(&bob, bobs).await, Some(5));
    let answer = sweep_all(&app).await;
    assert_eq!(answer.body["deleted"], 2);
    assert_eq!(stored(&alice, alices).await, None);
    assert_eq!(stored(&bob, bobs).await, None);
    let (queued,): (i64,) = sqlx::query_as("SELECT count(*) FROM orphan_objects")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(queued, 0);
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_server_with_no_storage_refuses_files_and_nothing_else(pool: PgPool) {
    let mut bare = state(pool.clone());
    bare.files = None;
    let app = build_router(bare);
    let device = register(&app, "alice", "Laptop").await;
    let note = a_note(&app, &device.token).await;
    let token = Some(device.token.as_str());

    let answers = [
        post(
            &app,
            "/v1/files",
            token,
            json!({ "file_id": Uuid::now_v7(), "size": 1 }),
        )
        .await,
        get(&app, &format!("/v1/files/{}", Uuid::now_v7()), token).await,
        post(&app, "/v1/files/confirm", token, json!({ "items": [] })).await,
        post(&app, "/v1/files/release", token, json!({ "items": [] })).await,
    ];
    for answer in answers {
        assert_eq!(answer.status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(answer.code(), "files_unavailable");
    }
    // Notes are as they were, and a purge works.
    let purge = post(
        &app,
        "/v1/notes/purge",
        token,
        json!({ "items": [{ "note_id": note, "base_change": 1 }] }),
    )
    .await;
    assert_eq!(purge.status, StatusCode::OK);
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn file_requests_are_refused_whole_when_malformed_or_not_allowed(pool: PgPool) {
    file_bucket().await;
    let app = app(pool.clone());
    let laptop = register(&app, "alice", "Laptop").await;
    let waiting = register(&app, "alice", "Phone").await;
    let note = a_note(&app, &laptop.token).await;
    let file = upload(&app, &laptop.token, b"bytes").await;

    // A device that is not approved has nothing to do with files.
    let pending = Some(waiting.token.as_str());
    let item = json!({ "items": [naming(file, note)] });
    for answer in [
        post(
            &app,
            "/v1/files",
            pending,
            json!({ "file_id": Uuid::now_v7(), "size": 1 }),
        )
        .await,
        get(&app, &format!("/v1/files/{file}"), pending).await,
        post(&app, "/v1/files/confirm", pending, item.clone()).await,
        post(&app, "/v1/files/release", pending, item.clone()).await,
    ] {
        assert_eq!(answer.code(), "device_pending");
    }
    let answer = post(&app, "/v1/files/confirm", None, item).await;
    assert_eq!(answer.code(), "unauthenticated");

    let token = Some(laptop.token.as_str());
    let answer = get(&app, "/v1/files/not-an-id", token).await;
    assert_eq!(answer.status, StatusCode::NOT_FOUND);
    for what in ["confirm", "release"] {
        let twice = json!({ "items": [naming(file, note), naming(file, note)] });
        let answer = post(&app, &format!("/v1/files/{what}"), token, twice).await;
        assert_eq!(answer.code(), "invalid_request", "{what}");
        let too_many: Vec<Value> = (0..=limits::MAX_BATCH_ITEMS)
            .map(|_| naming(Uuid::now_v7(), note))
            .collect();
        let answer = post(
            &app,
            &format!("/v1/files/{what}"),
            token,
            json!({ "items": too_many }),
        )
        .await;
        assert_eq!(answer.code(), "payload_too_large", "{what}");
    }
    assert_eq!(
        row(&pool, file).await.map(|(confirmed, _)| confirmed),
        Some(false)
    );
}

/// The server, with a file store that nothing listens at.
fn app_with_no_store_listening(pool: PgPool) -> Router {
    let config = dirt_api::FilesConfig {
        endpoint: "http://127.0.0.1:9".to_owned(),
        ..dirt_api::FilesConfig::local()
    };
    let mut cut_off = state(pool);
    cut_off.files = Some(std::sync::Arc::new(FileStore::new(&config).unwrap()));
    build_router(cut_off)
}

async fn queued(pool: &PgPool) -> i64 {
    let (queued,): (i64,) = sqlx::query_as("SELECT count(*) FROM orphan_objects")
        .fetch_one(pool)
        .await
        .unwrap();
    queued
}

/// A store that cannot be reached delays a deletion and loses none. The
/// file's row goes when its time comes, whatever the store is doing, and
/// its object waits in a queue until the store takes the deletion.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_store_that_cannot_be_reached_delays_a_deletion_and_loses_none(pool: PgPool) {
    file_bucket().await;
    let app = app(pool.clone());
    let device = register(&app, "alice", "Laptop").await;
    let note = a_note(&app, &device.token).await;
    let abandoned = upload(&app, &device.token, b"abandoned").await;
    let waiting = upload(&app, &device.token, b"not yet confirmed").await;
    make_due(&pool, abandoned).await;
    let cut_off = app_with_no_store_listening(pool.clone());

    // Asking for a link sweeps first. The link itself needs no store: it
    // is signed, not fetched.
    let answer = ask_to_upload(&cut_off, &device.token, Uuid::now_v7(), 1).await;
    assert_eq!(answer.status, StatusCode::OK);
    // The server no longer knows the file, and its space has come back.
    assert_eq!(row(&pool, abandoned).await, None);
    assert_eq!(file_bytes(&pool, device.account_id).await, 17 + 1);
    // Its object is still in the store, and remembered.
    assert_eq!(stored(&device, abandoned).await, Some(9));
    assert_eq!(queued(&pool).await, 1);
    the_wait_is_over(&pool).await;
    let answer = sweep_all(&cut_off).await;
    assert_eq!(answer.body["deleted"], 0);
    assert_eq!(queued(&pool).await, 1);

    // A file cannot be confirmed while the store cannot be asked about it.
    let answer = post(
        &cut_off,
        "/v1/files/confirm",
        Some(&device.token),
        json!({ "items": [naming(waiting, note)] }),
    )
    .await;
    assert_eq!(answer.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(answer.code(), "files_unavailable");
    assert_eq!(
        row(&pool, waiting).await.map(|(confirmed, _)| confirmed),
        Some(false)
    );

    // With the store back, and the hour over, the object goes.
    assert_eq!(sweep_all(&app).await.body["deleted"], 1);
    assert_eq!(stored(&device, abandoned).await, None);
    assert_eq!(queued(&pool).await, 0);
    assert_eq!(stored(&device, waiting).await, Some(17));
}

/// A file's ID is its place in the store. After a reset, the device that
/// remains sends its files again under the IDs its notes name, to the very
/// places whose old objects are waiting to be deleted. That deletion must
/// not take the file. Asking for the file again calls it off.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_file_asked_for_again_calls_off_the_deletion_of_its_old_object(pool: PgPool) {
    file_bucket().await;
    let app = app(pool.clone());
    let device = register(&app, "alice", "Laptop").await;
    let token = &device.token;
    let note = a_note(&app, token).await;
    let file = upload(&app, token, b"wanted after all").await;
    confirm(&app, token, file, note).await;
    let other = upload(&app, token, b"not wanted again").await;
    confirm(&app, token, other, note).await;

    let reset = post(&app, "/v1/account/reset", Some(token), json!({})).await;
    assert_eq!(reset.status, StatusCode::OK);
    assert_eq!(queued(&pool).await, 2);
    let note = a_note(&app, token).await;

    // The device sends its note again and asks to send the file again.
    assert_eq!(
        ask_to_upload(&app, token, file, 16).await.status,
        StatusCode::OK
    );
    // The deletion of this file's object is called off. The other's is not.
    assert_eq!(queued(&pool).await, 1);
    // The object that is there is the file, byte for byte, since a file
    // never changes. It can be confirmed as it stands.
    assert_eq!(confirm(&app, token, file, note).await, FileOutcome::Done);

    the_wait_is_over(&pool).await;
    assert_eq!(sweep_all(&app).await.body["deleted"], 1);

    assert_eq!(fetch(&app, token, file).await.unwrap(), b"wanted after all");
    assert_eq!(row(&pool, file).await, Some((true, None)));
    assert_eq!(stored(&device, other).await, None);
    assert_eq!(queued(&pool).await, 0);
}

/// A link works for a quarter of an hour, and the store knows nothing of
/// the server's rows. A device is given a link; the account is reset before
/// it uses it; the sweep runs; and then the upload arrives. If the object's
/// place in the queue had gone with that sweep, the upload would leave an
/// object that nothing remembers. It waits an hour, by which time no link
/// for it can work.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn an_upload_that_arrives_after_its_file_was_swept_is_still_deleted(pool: PgPool) {
    file_bucket().await;
    let app = app(pool.clone());
    let device = register(&app, "alice", "Laptop").await;
    let token = &device.token;
    let file = Uuid::now_v7();
    let link: UploadLink = ask_to_upload(&app, token, file, 4).await.json();
    let never_confirmed = Uuid::now_v7();
    let late: UploadLink = ask_to_upload(&app, token, never_confirmed, 9).await.json();

    let reset = post(&app, "/v1/account/reset", Some(token), json!({})).await;
    assert_eq!(reset.status, StatusCode::OK);
    // The same for a file that simply fell due.
    let second = register(&app, "bob", "Laptop").await;
    let due = Uuid::now_v7();
    let due_link: UploadLink = ask_to_upload(&app, &second.token, due, 3).await.json();
    make_due(&pool, due).await;

    // The sweep runs before any of the uploads.
    assert_eq!(sweep_all(&app).await.body["deleted"], 0);
    assert_eq!(queued(&pool).await, 3);

    // The uploads arrive. The store takes them: the links are good.
    assert!(put(&link.url, b"late").await.is_success());
    assert!(put(&late.url, b"very late").await.is_success());
    assert!(put(&due_link.url, b"due").await.is_success());
    assert_eq!(stored(&device, file).await, Some(4));
    // Sweeping again, inside the hour, still leaves them queued.
    assert_eq!(sweep_all(&app).await.body["deleted"], 0);
    assert_eq!(queued(&pool).await, 3);

    the_wait_is_over(&pool).await;
    assert_eq!(sweep_all(&app).await.body["deleted"], 3);
    assert_eq!(stored(&device, file).await, None);
    assert_eq!(stored(&device, never_confirmed).await, None);
    assert_eq!(stored(&second, due).await, None);
    assert_eq!(queued(&pool).await, 0);
}

/// A sweep lists the objects that have waited long enough, and then takes
/// them one at a time. Between the two, a file can be asked for again, which
/// takes its key out of the queue and issues a new link, and the account
/// reset, which queues the key afresh. The sweep then holds a key from its
/// list whose entry is new, and must leave it: its hour has only begun.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_key_queued_afresh_while_a_sweep_runs_is_not_deleted_by_it(pool: PgPool) {
    file_bucket().await;
    let app = app(pool.clone());
    let device = register(&app, "alice", "Laptop").await;
    let token = &device.token;
    let file = upload(&app, token, b"asked for twice").await;
    let key = FileStore::object_key(device.account_id, file);

    // Queued, and its hour over: a sweep lists it now.
    post(&app, "/v1/account/reset", Some(token), json!({})).await;
    the_wait_is_over(&pool).await;
    // The device asks for the file again and is given a link. Asking
    // deletes what the account has waiting, the old object among it. Then
    // the account is reset again, before the device has used the link.
    let link: UploadLink = ask_to_upload(&app, token, file, 15).await.json();
    post(&app, "/v1/account/reset", Some(token), json!({})).await;
    assert_eq!(queued(&pool).await, 1);
    assert_eq!(stored(&device, file).await, None);

    // The sweep reaches the key it listed. The entry under it is new.
    let store = file_store();
    let deleted = dirt_api::files::delete_waiting_object(&pool, &store, &key)
        .await
        .unwrap();
    assert!(!deleted);
    assert_eq!(queued(&pool).await, 1, "the key is still remembered");

    // The device uses its link. What it leaves is deleted in its time,
    // because the key is still in the queue to say it is there.
    assert!(put(&link.url, b"asked for twice").await.is_success());
    assert_eq!(stored(&device, file).await, Some(15));
    the_wait_is_over(&pool).await;
    assert!(dirt_api::files::delete_waiting_object(&pool, &store, &key)
        .await
        .unwrap());
    assert_eq!(stored(&device, file).await, None);
    assert_eq!(queued(&pool).await, 0);
}

/// A file is confirmed for a note in the moment it falls due. Whichever
/// request the server takes first, it never ends up saying a file is there
/// when its object is gone, and neither request fails.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn confirming_a_file_while_it_is_swept_never_leaves_a_row_without_an_object(pool: PgPool) {
    file_bucket().await;
    let app = app(pool.clone());
    let device = register(&app, "alice", "Laptop").await;
    let token = device.token.clone();
    let note = a_note(&app, &token).await;

    let mut kept = 0;
    for round in 0..12 {
        let file = upload(&app, &token, format!("round {round}").as_bytes()).await;
        make_due(&pool, file).await;

        let confirming = post(
            &app,
            "/v1/files/confirm",
            Some(&token),
            json!({ "items": [naming(file, note)] }),
        );
        // Each is given the first move in turn.
        let (confirmed, swept) = if round % 2 == 0 {
            tokio::join!(confirming, sweep_all(&app))
        } else {
            let (swept, confirmed) = tokio::join!(sweep_all(&app), confirming);
            (confirmed, swept)
        };
        assert_eq!(
            confirmed.status,
            StatusCode::OK,
            "round {round}: {:?}",
            confirmed.body
        );
        assert_eq!(
            swept.status,
            StatusCode::OK,
            "round {round}: {:?}",
            swept.body
        );

        // Whatever is left to delete is deleted before looking.
        the_wait_is_over(&pool).await;
        sweep_all(&app).await;
        let outcome = confirmed.json::<FileResults>().results.remove(0).outcome;
        match (outcome, row(&pool, file).await) {
            (FileOutcome::Done, Some((true, None))) => {
                assert!(stored(&device, file).await.is_some(), "round {round}");
                kept += 1;
                // Make room in the note for the next round.
                assert_eq!(release(&app, &token, file, note).await, FileOutcome::Done);
            }
            (FileOutcome::NoSuchFile, None) => {
                assert_eq!(stored(&device, file).await, None, "round {round}");
            }
            other => panic!("round {round}: the file was left as {other:?}"),
        }
    }
    println!("confirmed first in {kept} of 12 rounds");
}

/// The server does not rely on the storage refusing a body of the wrong
/// length. Before a file is counted as there, it asks the storage how large
/// the object is. Here an object of another size is put in the file's
/// place by something that holds the storage's own key.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_file_is_not_confirmed_at_a_size_other_than_the_one_asked_for(pool: PgPool) {
    file_bucket().await;
    let app = app(pool.clone());
    let device = register(&app, "alice", "Laptop").await;
    let note = a_note(&app, &device.token).await;
    let file = Uuid::now_v7();
    ask_to_upload(&app, &device.token, file, 10).await;

    let key = FileStore::object_key(device.account_id, file);
    let larger = file_store().upload_link(&key, 21);
    assert!(put(&larger, b"larger than was asked").await.is_success());
    assert_eq!(stored(&device, file).await, Some(21));

    assert_eq!(
        confirm(&app, &device.token, file, note).await,
        FileOutcome::NotUploaded
    );
    assert_eq!(
        row(&pool, file).await.map(|(confirmed, _)| confirmed),
        Some(false)
    );
}
