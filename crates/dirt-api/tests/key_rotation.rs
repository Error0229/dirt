//! Changing the account's key, against Postgres.
//!
//! The server stores the new key wrapped for each device and opens none of
//! the copies, so these tests use any bytes for them. What a device does
//! with its copy is tested in `dirt-sim`, with real keys.
//!
//! Start the database with `docker compose up -d` at the repository root and
//! set `DATABASE_URL` as `compose.yaml` shows.

mod common;

use axum::http::{Method, StatusCode};
use axum::Router;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use common::{app, call, get, item, note_id, post, pull, push, push_one, register, Answer};
use dirt_proto::api::{
    limits, DeviceKeys, ErrorCode, KeyChanged, MyDevice, OldKeyNotes, PushItem, PushOutcome,
    RegisterDeviceResponse,
};
use dirt_proto::NoteId;
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

fn wrapped_for(device: Uuid, bytes: &[u8]) -> Value {
    json!({ "device_id": device, "wrapped": STANDARD.encode(bytes) })
}

async fn change_key(app: &Router, token: &str, from_version: i32, keys: Vec<Value>) -> Answer {
    post(
        app,
        "/v1/account/key",
        Some(token),
        json!({ "from_version": from_version, "keys": keys }),
    )
    .await
}

async fn approve(pool: &PgPool, device: Uuid) {
    sqlx::query("UPDATE devices SET status = 'approved' WHERE id = $1")
        .bind(device)
        .execute(pool)
        .await
        .unwrap();
}

/// Alice's laptop and phone, both approved.
async fn two_devices(
    app: &Router,
    pool: &PgPool,
) -> (RegisterDeviceResponse, RegisterDeviceResponse) {
    let laptop = register(app, "alice", "Laptop").await;
    let phone = register(app, "alice", "Phone").await;
    approve(pool, phone.device_id).await;
    (laptop, phone)
}

fn under(key_version: i32, note: NoteId, base_change: i64, ciphertext: &[u8]) -> PushItem {
    PushItem {
        key_version,
        ..item(note, base_change, ciphertext.to_vec())
    }
}

const fn accepted(outcome: &PushOutcome) -> Option<i64> {
    match outcome {
        PushOutcome::Accepted { change } => Some(*change),
        _ => None,
    }
}

const fn rejection(outcome: &PushOutcome) -> Option<ErrorCode> {
    match outcome {
        PushOutcome::Rejected { code, .. } => Some(*code),
        _ => None,
    }
}

async fn my_keys(app: &Router, token: &str, after: i32) -> Vec<(i32, Vec<u8>)> {
    get(app, &format!("/v1/account/keys?after={after}"), Some(token))
        .await
        .json::<DeviceKeys>()
        .keys
        .into_iter()
        .map(|key| (key.key_version, key.wrapped))
        .collect()
}

async fn old_notes(app: &Router, token: &str) -> OldKeyNotes {
    get(app, "/v1/account/key/notes", Some(token)).await.json()
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_key_change_gives_each_device_its_copy_and_moves_the_account_on(pool: PgPool) {
    let app = app(pool.clone());
    let (laptop, phone) = two_devices(&app, &pool).await;
    let note = note_id();
    assert!(
        accepted(&push_one(&app, &laptop.token, note, 0, b"under the first key").await).is_some()
    );
    assert_eq!(pull(&app, &phone.token, 0).await.key_version, 1);

    let changed: KeyChanged = change_key(
        &app,
        &laptop.token,
        1,
        vec![
            wrapped_for(laptop.device_id, b"for the laptop"),
            wrapped_for(phone.device_id, b"for the phone"),
        ],
    )
    .await
    .json();
    assert_eq!((changed.key_version, changed.old_key_notes), (2, 1));

    // Each device finds its own copy, and only its own.
    assert_eq!(
        my_keys(&app, &phone.token, 1).await,
        [(2, b"for the phone".to_vec())]
    );
    assert_eq!(
        my_keys(&app, &laptop.token, 1).await,
        [(2, b"for the laptop".to_vec())]
    );
    assert!(my_keys(&app, &phone.token, 2).await.is_empty());

    // Every device learns of the change at its next pull, and from its own
    // record.
    let pulled = pull(&app, &phone.token, 0).await;
    assert_eq!((pulled.key_version, pulled.old_key_notes), (2, 1));
    let me: MyDevice = get(&app, "/v1/devices/me", Some(&phone.token)).await.json();
    assert_eq!(me.key_version, 2);
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_key_change_must_name_every_approved_device_and_no_other(pool: PgPool) {
    let app = app(pool.clone());
    let (laptop, phone) = two_devices(&app, &pool).await;
    let tablet = register(&app, "alice", "Tablet").await;
    let removed = register(&app, "alice", "Old phone").await;
    approve(&pool, removed.device_id).await;
    let path = format!("/v1/devices/{}", removed.device_id);
    call(&app, Method::DELETE, &path, Some(&laptop.token), None).await;

    let for_laptop = || wrapped_for(laptop.device_id, b"l");
    let for_phone = || wrapped_for(phone.device_id, b"p");
    let refused = |keys: Vec<Value>| {
        let app = app.clone();
        let token = laptop.token.clone();
        async move { change_key(&app, &token, 1, keys).await }
    };

    // One approved device left out: it would be locked out of the account.
    let answer = refused(vec![for_laptop()]).await;
    assert_eq!(answer.status, StatusCode::CONFLICT);
    assert_eq!(answer.code(), "devices_changed");
    // A waiting device named: it would be let in without being approved.
    let answer = refused(vec![
        for_laptop(),
        for_phone(),
        wrapped_for(tablet.device_id, b"t"),
    ])
    .await;
    assert_eq!(answer.code(), "devices_changed");
    // A removed device named: it would be let back in.
    let answer = refused(vec![
        for_laptop(),
        for_phone(),
        wrapped_for(removed.device_id, b"r"),
    ])
    .await;
    assert_eq!(answer.code(), "devices_changed");
    // A device named twice, and a copy of no size or too large.
    let answer = refused(vec![for_laptop(), for_phone(), for_phone()]).await;
    assert_eq!(answer.code(), "invalid_request");
    let answer = refused(vec![for_laptop(), wrapped_for(phone.device_id, b"")]).await;
    assert_eq!(answer.code(), "invalid_request");
    let too_large = vec![0u8; limits::MAX_WRAPPED_KEY_BYTES + 1];
    let answer = refused(vec![for_laptop(), wrapped_for(phone.device_id, &too_large)]).await;
    assert_eq!(answer.code(), "invalid_request");

    // Nothing changed, and nothing was stored.
    assert_eq!(pull(&app, &laptop.token, 0).await.key_version, 1);
    assert!(my_keys(&app, &phone.token, 0).await.is_empty());

    // A device that is waiting, or was removed, cannot change the key.
    let keys = vec![for_laptop(), for_phone()];
    assert_eq!(
        change_key(&app, &tablet.token, 1, keys.clone())
            .await
            .code(),
        "device_pending"
    );
    assert_eq!(
        change_key(&app, &removed.token, 1, keys).await.code(),
        "device_revoked"
    );
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn two_devices_changing_the_key_at_once_change_it_once(pool: PgPool) {
    let app = app(pool.clone());
    let (laptop, phone) = two_devices(&app, &pool).await;
    let keys = |tag: &[u8]| {
        vec![
            wrapped_for(laptop.device_id, tag),
            wrapped_for(phone.device_id, tag),
        ]
    };

    let (first, second) = tokio::join!(
        change_key(&app, &laptop.token, 1, keys(b"laptop's")),
        change_key(&app, &phone.token, 1, keys(b"phone's")),
    );
    let mut codes = [first.status, second.status];
    codes.sort();
    assert_eq!(codes, [StatusCode::OK, StatusCode::CONFLICT]);
    let lost = if first.status == StatusCode::OK {
        &second
    } else {
        &first
    };
    assert_eq!(lost.code(), "key_changed");

    // Both devices hold copies of one key: the winner's.
    let on_laptop = my_keys(&app, &laptop.token, 1).await;
    let on_phone = my_keys(&app, &phone.token, 1).await;
    assert_eq!(on_laptop.len(), 1);
    assert_eq!(on_laptop, on_phone);
    assert_eq!(pull(&app, &laptop.token, 0).await.key_version, 2);
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_note_under_the_old_key_is_refused_and_the_change_ends_with_the_last_old_note(
    pool: PgPool,
) {
    let app = app(pool.clone());
    let (laptop, phone) = two_devices(&app, &pool).await;
    let (first, second) = (note_id(), note_id());
    let first_change = accepted(&push_one(&app, &laptop.token, first, 0, b"one").await).unwrap();
    let second_change = accepted(&push_one(&app, &laptop.token, second, 0, b"two").await).unwrap();
    change_key(
        &app,
        &laptop.token,
        1,
        vec![
            wrapped_for(laptop.device_id, b"l"),
            wrapped_for(phone.device_id, b"p"),
        ],
    )
    .await;

    let listed = old_notes(&app, &phone.token).await;
    assert_eq!((listed.key_version, listed.total), (2, 2));
    assert_eq!(listed.notes, [first, second]);

    // The phone has not caught up. Its note is refused, and so is one under
    // a version that does not exist yet. Neither uses a change number.
    let late = note_id();
    let outcomes = push(
        &app,
        &phone.token,
        vec![under(1, late, 0, b"late"), under(3, note_id(), 0, b"early")],
    )
    .await;
    assert_eq!(rejection(&outcomes[0]), Some(ErrorCode::KeyChanged));
    assert_eq!(rejection(&outcomes[1]), Some(ErrorCode::KeyChanged));
    assert_eq!(pull(&app, &phone.token, 0).await.items.len(), 2);

    // Under the new key it is accepted. A new note is not one of those to
    // send again, so the count stands.
    let outcomes = push(&app, &phone.token, vec![under(2, late, 0, b"late")]).await;
    assert!(accepted(&outcomes[0]).is_some());
    assert_eq!(old_notes(&app, &phone.token).await.total, 2);

    // Sending the old notes again under the new key counts them down.
    let outcomes = push(
        &app,
        &laptop.token,
        vec![under(2, first, first_change, b"one, again")],
    )
    .await;
    assert!(accepted(&outcomes[0]).is_some());
    let listed = old_notes(&app, &laptop.token).await;
    assert_eq!((listed.total, listed.notes.as_slice()), (1, &[second][..]));
    assert_eq!(pull(&app, &phone.token, 0).await.old_key_notes, 1);

    // A stale attempt changes nothing.
    let outcomes = push(&app, &phone.token, vec![under(2, second, 0, b"stale")]).await;
    assert!(matches!(outcomes[0], PushOutcome::Stale { .. }));
    assert_eq!(old_notes(&app, &laptop.token).await.total, 1);

    let outcomes = push(
        &app,
        &phone.token,
        vec![under(2, second, second_change, b"two, again")],
    )
    .await;
    assert!(accepted(&outcomes[0]).is_some());
    let listed = old_notes(&app, &laptop.token).await;
    assert_eq!((listed.total, listed.notes.len()), (0, 0));
    assert_eq!(pull(&app, &laptop.token, 0).await.old_key_notes, 0);

    // Writing a note again after that does not take the count below zero.
    let pulled = pull(&app, &laptop.token, 0).await;
    let change = pulled
        .items
        .iter()
        .find(|blob| blob.note_id == first)
        .unwrap()
        .change;
    push(
        &app,
        &laptop.token,
        vec![under(2, first, change, b"once more")],
    )
    .await;
    assert_eq!(old_notes(&app, &laptop.token).await.total, 0);
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_key_change_cancels_the_recovery_phrase_and_an_exchange_under_way(pool: PgPool) {
    let app = app(pool.clone());
    let (laptop, phone) = two_devices(&app, &pool).await;
    let tablet = register(&app, "alice", "Tablet").await;
    let recovery = json!({
        "key_version": 1,
        "wrapped": STANDARD.encode(b"sealed"),
        "claim_hash": STANDARD.encode([7u8; limits::CLAIM_HASH_BYTES]),
    });
    let stored = call(
        &app,
        Method::PUT,
        "/v1/recovery",
        Some(&laptop.token),
        Some(recovery),
    )
    .await;
    assert_eq!(stored.status, StatusCode::OK);
    post(
        &app,
        &format!("/v1/enrolments/{}/messages", tablet.device_id),
        Some(&tablet.token),
        json!({ "body": STANDARD.encode(b"commit"), "restart": true }),
    )
    .await;
    call(
        &app,
        Method::PUT,
        &format!("/v1/enrolments/{}/key", tablet.device_id),
        Some(&laptop.token),
        Some(json!({ "key_version": 1, "wrapped": STANDARD.encode(b"old key") })),
    )
    .await;

    change_key(
        &app,
        &laptop.token,
        1,
        vec![
            wrapped_for(laptop.device_id, b"l"),
            wrapped_for(phone.device_id, b"p"),
        ],
    )
    .await;

    let me: MyDevice = get(&app, "/v1/devices/me", Some(&laptop.token))
        .await
        .json();
    assert!(!me.recovery_available);
    let (messages, keys, recovery_copies): (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM enrolment_messages), (SELECT count(*) FROM enrolment_keys), \
                (SELECT count(*) FROM recovery_keys)",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((messages, keys, recovery_copies), (0, 0, 0));
    // The waiting device is still waiting. It begins its exchange again.
    let me: MyDevice = get(&app, "/v1/devices/me", Some(&tablet.token))
        .await
        .json();
    assert_eq!(me.device.status, dirt_proto::api::DeviceStatus::Pending);
}

/// A copy of the account key left for a waiting device, or under the
/// recovery phrase, has to hold the key the account has now. A device that
/// has not caught up with a change would otherwise hand on the key that was
/// replaced.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_copy_of_a_replaced_key_is_not_accepted_for_a_new_device_or_a_phrase(pool: PgPool) {
    let app = app(pool.clone());
    let (laptop, phone) = two_devices(&app, &pool).await;
    let tablet = register(&app, "alice", "Tablet").await;
    change_key(
        &app,
        &laptop.token,
        1,
        vec![
            wrapped_for(laptop.device_id, b"l"),
            wrapped_for(phone.device_id, b"p"),
        ],
    )
    .await;
    post(
        &app,
        &format!("/v1/enrolments/{}/messages", tablet.device_id),
        Some(&tablet.token),
        json!({ "body": STANDARD.encode(b"commit"), "restart": true }),
    )
    .await;

    let key_path = format!("/v1/enrolments/{}/key", tablet.device_id);
    let leave = |version: i32| {
        let app = app.clone();
        let token = phone.token.clone();
        let path = key_path.clone();
        async move {
            let body = json!({ "key_version": version, "wrapped": STANDARD.encode(b"sealed") });
            call(&app, Method::PUT, &path, Some(&token), Some(body)).await
        }
    };
    // The phone has not caught up: it offers the first key.
    let refused = leave(1).await;
    assert_eq!(refused.status, StatusCode::CONFLICT);
    assert_eq!(refused.code(), "key_changed");
    let (left,): (i64,) = sqlx::query_as("SELECT count(*) FROM enrolment_keys")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(left, 0);
    assert_eq!(leave(2).await.status, StatusCode::OK);

    let recovery = |version: i32| {
        let app = app.clone();
        let token = phone.token.clone();
        async move {
            let body = json!({
                "key_version": version,
                "wrapped": STANDARD.encode(b"sealed"),
                "claim_hash": STANDARD.encode([7u8; limits::CLAIM_HASH_BYTES]),
            });
            call(&app, Method::PUT, "/v1/recovery", Some(&token), Some(body)).await
        }
    };
    assert_eq!(recovery(1).await.code(), "key_changed");
    let me: MyDevice = get(&app, "/v1/devices/me", Some(&laptop.token))
        .await
        .json();
    assert!(!me.recovery_available);
    assert_eq!(recovery(2).await.status, StatusCode::OK);
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_removed_device_loses_its_copies_and_a_reset_starts_the_versions_again(pool: PgPool) {
    let app = app(pool.clone());
    let (laptop, phone) = two_devices(&app, &pool).await;
    push_one(&app, &laptop.token, note_id(), 0, b"a note").await;
    change_key(
        &app,
        &laptop.token,
        1,
        vec![
            wrapped_for(laptop.device_id, b"l"),
            wrapped_for(phone.device_id, b"p"),
        ],
    )
    .await;

    let path = format!("/v1/devices/{}", phone.device_id);
    call(&app, Method::DELETE, &path, Some(&laptop.token), None).await;
    let (copies,): (i64,) = sqlx::query_as("SELECT count(*) FROM device_keys WHERE device_id = $1")
        .bind(phone.device_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(copies, 0);
    assert_eq!(
        get(&app, "/v1/account/keys?after=0", Some(&phone.token))
            .await
            .code(),
        "device_revoked"
    );

    // A second change names only the laptop now.
    let changed: KeyChanged = change_key(
        &app,
        &laptop.token,
        2,
        vec![wrapped_for(laptop.device_id, b"l3")],
    )
    .await
    .json();
    assert_eq!(changed.key_version, 3);
    assert_eq!(
        my_keys(&app, &laptop.token, 1).await,
        [(2, b"l".to_vec()), (3, b"l3".to_vec())]
    );

    // A reset makes a first key again.
    let reset = post(&app, "/v1/account/reset", Some(&laptop.token), json!({})).await;
    assert_eq!(reset.status, StatusCode::OK);
    let pulled = pull(&app, &laptop.token, 0).await;
    assert_eq!((pulled.key_version, pulled.old_key_notes), (1, 0));
    assert!(my_keys(&app, &laptop.token, 0).await.is_empty());
    let (copies,): (i64,) = sqlx::query_as("SELECT count(*) FROM device_keys")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(copies, 0);
    assert!(
        accepted(&push_one(&app, &laptop.token, note_id(), 0, b"after the reset").await).is_some()
    );
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn one_account_cannot_change_or_read_anothers_keys(pool: PgPool) {
    let app = app(pool.clone());
    let (laptop, phone) = two_devices(&app, &pool).await;
    let stranger = register(&app, "bob", "Bob's laptop").await;

    // Bob names Alice's devices: they are not his account's.
    let answer = change_key(
        &app,
        &stranger.token,
        1,
        vec![
            wrapped_for(laptop.device_id, b"x"),
            wrapped_for(phone.device_id, b"x"),
        ],
    )
    .await;
    assert_eq!(answer.code(), "devices_changed");
    // Bob adds Alice's phone to a change of his own key.
    let answer = change_key(
        &app,
        &stranger.token,
        1,
        vec![
            wrapped_for(stranger.device_id, b"b"),
            wrapped_for(phone.device_id, b"x"),
        ],
    )
    .await;
    assert_eq!(answer.code(), "devices_changed");
    assert!(my_keys(&app, &phone.token, 0).await.is_empty());

    change_key(
        &app,
        &laptop.token,
        1,
        vec![
            wrapped_for(laptop.device_id, b"l"),
            wrapped_for(phone.device_id, b"p"),
        ],
    )
    .await;
    assert!(my_keys(&app, &stranger.token, 0).await.is_empty());
    assert_eq!(pull(&app, &stranger.token, 0).await.key_version, 1);
    assert_eq!(old_notes(&app, &stranger.token).await.total, 0);
}
