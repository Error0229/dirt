//! Purging deleted notes, against Postgres.
//!
//! The server cannot tell a deleted note from a live one. It purges what an
//! approved device names, if it still holds the version that device saw.
//! What a device does before it asks, and when it is told of a purge, is
//! tested in `dirt-sim`.
//!
//! Start the database with `docker compose up -d` at the repository root and
//! set `DATABASE_URL` as `compose.yaml` shows.

mod common;

use axum::http::StatusCode;
use axum::Router;
use common::{app, get, item, note_id, post, pull, push, push_one, register, usage, Answer};
use dirt_proto::api::{
    limits, ErrorCode, PurgeOutcome, PurgeResponse, PushItem, PushOutcome, EPOCH_HEADER,
};
use dirt_proto::NoteId;
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

fn naming(note: NoteId, base_change: i64) -> Value {
    json!({ "note_id": note, "base_change": base_change })
}

async fn try_purge(app: &Router, token: &str, items: Vec<Value>) -> Answer {
    post(
        app,
        "/v1/notes/purge",
        Some(token),
        json!({ "items": items }),
    )
    .await
}

async fn purge(app: &Router, token: &str, items: Vec<Value>) -> Vec<PurgeOutcome> {
    try_purge(app, token, items)
        .await
        .json::<PurgeResponse>()
        .results
        .into_iter()
        .map(|result| result.outcome)
        .collect()
}

async fn purge_one(app: &Router, token: &str, note: NoteId, base_change: i64) -> PurgeOutcome {
    purge(app, token, vec![naming(note, base_change)])
        .await
        .remove(0)
}

async fn purged_count(pool: &PgPool, account: Uuid) -> i64 {
    let (count,): (i64,) = sqlx::query_as("SELECT purged_count FROM accounts WHERE id = $1")
        .bind(account)
        .fetch_one(pool)
        .await
        .unwrap();
    count
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_purge_removes_the_blob_and_frees_the_notes_place(pool: PgPool) {
    let app = app(pool.clone());
    let device = register(&app, "alice", "Laptop").await;
    let (kept, gone) = (note_id(), note_id());
    push_one(&app, &device.token, kept, 0, b"kept").await;
    push_one(
        &app,
        &device.token,
        gone,
        0,
        b"deleted, as far as a device can tell",
    )
    .await;
    assert_eq!(usage(&pool, device.account_id).await, (2, 2, 40));

    let outcome = purge_one(&app, &device.token, gone, 2).await;

    // The purge takes the account's next change number, as a push would.
    assert_eq!(outcome, PurgeOutcome::Purged { change: 3 });
    assert_eq!(usage(&pool, device.account_id).await, (3, 1, 4));
    assert_eq!(purged_count(&pool, device.account_id).await, 1);

    // Every device learns of it by pulling: the note's row, with no blob.
    let page = pull(&app, &device.token, 2).await;
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].note_id, gone);
    assert_eq!(page.items[0].change, 3);
    assert_eq!(page.items[0].ciphertext, None);
    // The other note is as it was.
    let all = pull(&app, &device.token, 0).await;
    assert_eq!(all.items[0].ciphertext.as_deref(), Some(&b"kept"[..]));

    // The answer carries the epoch, as every answer to a device does.
    let answer = try_purge(&app, &device.token, vec![naming(kept, 1)]).await;
    assert_eq!(answer.status, StatusCode::OK);
    assert!(answer.headers.contains_key(EPOCH_HEADER));
    assert_eq!(answer.json::<PurgeResponse>().epoch, 1);
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_note_that_changed_since_the_device_looked_is_not_purged(pool: PgPool) {
    let app = app(pool.clone());
    let device = register(&app, "alice", "Laptop").await;
    let note = note_id();
    push_one(&app, &device.token, note, 0, b"deleted").await;
    // Another device, which had not seen the delete, edits the note.
    push_one(&app, &device.token, note, 1, b"edited, and live again").await;

    let outcome = purge_one(&app, &device.token, note, 1).await;

    assert_eq!(
        outcome,
        PurgeOutcome::Stale {
            current_change: Some(2)
        }
    );
    // Nothing changed, and no change number was used.
    assert_eq!(usage(&pool, device.account_id).await, (2, 1, 22));
    assert_eq!(purged_count(&pool, device.account_id).await, 0);
    let page = pull(&app, &device.token, 0).await;
    assert_eq!(
        page.items[0].ciphertext.as_deref(),
        Some(&b"edited, and live again"[..])
    );
    // A version newer than the server's is no better than an older one.
    let ahead = purge_one(&app, &device.token, note, 3).await;
    assert_eq!(
        ahead,
        PurgeOutcome::Stale {
            current_change: Some(2)
        }
    );
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn purging_twice_and_purging_a_note_the_server_never_had(pool: PgPool) {
    let app = app(pool.clone());
    let device = register(&app, "alice", "Laptop").await;
    let note = note_id();
    push_one(&app, &device.token, note, 0, b"deleted").await;
    assert_eq!(
        purge_one(&app, &device.token, note, 1).await,
        PurgeOutcome::Purged { change: 2 }
    );

    // A second device asks for the same purge. It is told the note is
    // purged, by the same marker, and nothing is counted twice.
    assert_eq!(
        purge_one(&app, &device.token, note, 1).await,
        PurgeOutcome::Purged { change: 2 }
    );
    assert_eq!(usage(&pool, device.account_id).await, (2, 0, 0));
    assert_eq!(purged_count(&pool, device.account_id).await, 1);

    assert_eq!(
        purge_one(&app, &device.token, note_id(), 1).await,
        PurgeOutcome::Stale {
            current_change: None
        }
    );
}

/// Nothing is ever stored in the place of a purged note. A device that
/// still holds an edit to it is handed the marker, whatever version it
/// names, and keeps its edit as a new note.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_purged_note_takes_no_more_pushes(pool: PgPool) {
    let app = app(pool.clone());
    let device = register(&app, "alice", "Laptop").await;
    let note = note_id();
    push_one(&app, &device.token, note, 0, b"deleted").await;
    purge_one(&app, &device.token, note, 1).await;

    for base in [0, 1, 2] {
        let outcome = push_one(&app, &device.token, note, base, b"an edit made unawares").await;
        let PushOutcome::Stale {
            current_change,
            current,
        } = outcome
        else {
            panic!("a push on base {base} onto a purged note was {outcome:?}");
        };
        assert_eq!(current_change, Some(2));
        let marker = current.expect("the marker is handed back");
        assert_eq!((marker.note_id, marker.change), (note, 2));
        assert_eq!(marker.ciphertext, None);
    }
    assert_eq!(usage(&pool, device.account_id).await, (2, 0, 0));
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn an_account_at_its_note_limit_has_room_again_after_a_purge(pool: PgPool) {
    let app = app(pool.clone());
    let device = register(&app, "alice", "Laptop").await;
    let deleted = note_id();
    push_one(&app, &device.token, deleted, 0, b"deleted").await;
    sqlx::query("UPDATE accounts SET note_count = $1")
        .bind(limits::MAX_NOTES_PER_ACCOUNT)
        .execute(&pool)
        .await
        .unwrap();
    let refused = push_one(&app, &device.token, note_id(), 0, b"one more").await;
    assert!(matches!(
        refused,
        PushOutcome::Rejected {
            code: ErrorCode::QuotaExceeded,
            ..
        }
    ));

    purge_one(&app, &device.token, deleted, 1).await;

    let taken = push_one(&app, &device.token, note_id(), 0, b"one more").await;
    assert_eq!(taken, PushOutcome::Accepted { change: 3 });
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn an_account_may_purge_only_so_many_notes(pool: PgPool) {
    let app = app(pool.clone());
    let device = register(&app, "alice", "Laptop").await;
    let (first, second) = (note_id(), note_id());
    push_one(&app, &device.token, first, 0, b"one").await;
    push_one(&app, &device.token, second, 0, b"two").await;
    sqlx::query("UPDATE accounts SET purged_count = $1")
        .bind(limits::MAX_PURGED_NOTES_PER_ACCOUNT - 1)
        .execute(&pool)
        .await
        .unwrap();

    let outcomes = purge(
        &app,
        &device.token,
        vec![naming(first, 1), naming(second, 2)],
    )
    .await;

    // The last place is taken by the first, and the second is refused
    // alone. It stays as it was.
    assert_eq!(outcomes[0], PurgeOutcome::Purged { change: 3 });
    assert!(matches!(
        outcomes[1],
        PurgeOutcome::Rejected {
            code: ErrorCode::QuotaExceeded,
            ..
        }
    ));
    assert_eq!(usage(&pool, device.account_id).await, (3, 1, 3));
    // A note already purged is still reported as purged at the limit.
    assert_eq!(
        purge_one(&app, &device.token, first, 1).await,
        PurgeOutcome::Purged { change: 3 }
    );
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn one_account_cannot_purge_anothers_note(pool: PgPool) {
    let app = app(pool.clone());
    let alice = register(&app, "alice", "Laptop").await;
    let bob = register(&app, "bob", "Laptop").await;
    let note = note_id();
    push_one(&app, &alice.token, note, 0, b"alice's").await;

    // To Bob's account there is no such note.
    assert_eq!(
        purge_one(&app, &bob.token, note, 1).await,
        PurgeOutcome::Stale {
            current_change: None
        }
    );
    let page = pull(&app, &alice.token, 0).await;
    assert_eq!(page.items[0].ciphertext.as_deref(), Some(&b"alice's"[..]));
    assert_eq!(usage(&pool, alice.account_id).await, (1, 1, 7));
    assert_eq!(usage(&pool, bob.account_id).await, (0, 0, 0));
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_purge_is_refused_whole_when_it_is_malformed_or_not_allowed(pool: PgPool) {
    let app = app(pool.clone());
    let laptop = register(&app, "alice", "Laptop").await;
    let waiting = register(&app, "alice", "Phone").await;
    let note = note_id();
    push_one(&app, &laptop.token, note, 0, b"deleted").await;

    // A device that is not approved can purge nothing.
    let answer = try_purge(&app, &waiting.token, vec![naming(note, 1)]).await;
    assert_eq!(answer.code(), "device_pending");
    let answer = post(&app, "/v1/notes/purge", None, json!({ "items": [] })).await;
    assert_eq!(answer.code(), "unauthenticated");

    // A note named twice, and a version the server can never have held.
    let twice = try_purge(&app, &laptop.token, vec![naming(note, 1), naming(note, 1)]).await;
    assert_eq!(twice.code(), "invalid_request");
    for base in [0, -1] {
        let never = try_purge(&app, &laptop.token, vec![naming(note, base)]).await;
        assert_eq!(never.code(), "invalid_request", "base {base}");
    }
    let too_many: Vec<Value> = (0..=limits::MAX_BATCH_ITEMS)
        .map(|_| naming(note_id(), 1))
        .collect();
    let answer = try_purge(&app, &laptop.token, too_many).await;
    assert_eq!(answer.code(), "payload_too_large");
    let not_a_purge = post(
        &app,
        "/v1/notes/purge",
        Some(&laptop.token),
        json!({ "items": "nope" }),
    )
    .await;
    assert_eq!(not_a_purge.code(), "invalid_request");

    // None of that purged anything.
    assert_eq!(usage(&pool, laptop.account_id).await, (1, 1, 7));
    assert!(get(&app, "/v1/notes/pull?after=0", Some(&laptop.token))
        .await
        .json::<dirt_proto::api::PullResponse>()
        .items[0]
        .ciphertext
        .is_some());
}

/// A change of the account's key counts the notes still under the old key,
/// so that devices know when the change is complete. A purged note has no
/// blob to send again and must not hold that count up.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn purging_a_note_under_an_old_key_counts_it_off_the_key_change(pool: PgPool) {
    let app = app(pool.clone());
    let device = register(&app, "alice", "Laptop").await;
    let (old, deleted) = (note_id(), note_id());
    push_one(&app, &device.token, old, 0, b"under the first key").await;
    push_one(&app, &device.token, deleted, 0, b"deleted").await;
    let changed = post(
        &app,
        "/v1/account/key",
        Some(&device.token),
        json!({ "from_version": 1, "keys": [{ "device_id": device.device_id, "wrapped": "AAEC" }] }),
    )
    .await;
    assert_eq!(changed.status, StatusCode::OK);
    assert_eq!(pull(&app, &device.token, 0).await.old_key_notes, 2);

    purge_one(&app, &device.token, deleted, 2).await;

    assert_eq!(pull(&app, &device.token, 0).await.old_key_notes, 1);
    // The other is sent again under the second key, and the change is
    // complete. A purged note is not listed among those to send again.
    let again = PushItem {
        key_version: 2,
        ..item(old, 1, b"under the second key".to_vec())
    };
    assert_eq!(
        push(&app, &device.token, vec![again]).await[0],
        PushOutcome::Accepted { change: 4 }
    );
    assert_eq!(pull(&app, &device.token, 0).await.old_key_notes, 0);

    // A later change of the key does not count the purged note either.
    let changed = post(
        &app,
        "/v1/account/key",
        Some(&device.token),
        json!({ "from_version": 2, "keys": [{ "device_id": device.device_id, "wrapped": "AAEC" }] }),
    )
    .await;
    assert_eq!(changed.status, StatusCode::OK);
    assert_eq!(pull(&app, &device.token, 0).await.old_key_notes, 1);
}

/// Resetting an account removes its notes, the markers of purged ones
/// among them, and the count of purges starts again.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_reset_starts_the_count_of_purged_notes_again(pool: PgPool) {
    let app = app(pool.clone());
    let device = register(&app, "alice", "Laptop").await;
    let note = note_id();
    push_one(&app, &device.token, note, 0, b"deleted").await;
    purge_one(&app, &device.token, note, 1).await;
    assert_eq!(purged_count(&pool, device.account_id).await, 1);

    let reset = post(&app, "/v1/account/reset", Some(&device.token), json!({})).await;
    assert_eq!(reset.status, StatusCode::OK);

    assert_eq!(purged_count(&pool, device.account_id).await, 0);
    assert!(pull(&app, &device.token, 0).await.items.is_empty());
}
