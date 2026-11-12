//! The approval exchange, the recovery copy, and resetting and deleting an
//! account, against Postgres.
//!
//! Start the database with `docker compose up -d` at the repository root and
//! set `DATABASE_URL` as `compose.yaml` shows. Each test runs in its own
//! throwaway database.

mod common;

use axum::http::{Method, StatusCode};
use axum::Router;
use common::{app, call, get, note_id, post, pull, push_one, register, sign_in_again, Answer};
use dirt_proto::api::{
    limits, DeviceList, DeviceStatus, EnrolmentMessage, EnrolmentMessages, MyDevice, PushOutcome,
    RegisterDeviceResponse, ResetResponse, WrappedKey,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

fn base64(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn messages_path(device: Uuid) -> String {
    format!("/v1/enrolments/{device}/messages")
}

fn key_path(device: Uuid) -> String {
    format!("/v1/enrolments/{device}/key")
}

fn complete_path(device: Uuid) -> String {
    format!("/v1/enrolments/{device}/complete")
}

async fn send(app: &Router, token: &str, device: Uuid, body: &[u8], restart: bool) -> Answer {
    post(
        app,
        &messages_path(device),
        Some(token),
        json!({ "body": base64(body), "restart": restart }),
    )
    .await
}

async fn read(app: &Router, token: &str, device: Uuid, after: i64) -> Vec<EnrolmentMessage> {
    get(
        app,
        &format!("{}?after={after}", messages_path(device)),
        Some(token),
    )
    .await
    .json::<EnrolmentMessages>()
    .messages
}

async fn put(app: &Router, path: &str, token: &str, body: Value) -> Answer {
    call(app, Method::PUT, path, Some(token), Some(body)).await
}

async fn delete(app: &Router, path: &str, token: &str) -> Answer {
    call(app, Method::DELETE, path, Some(token), None).await
}

async fn me(app: &Router, token: &str) -> MyDevice {
    get(app, "/v1/devices/me", Some(token)).await.json()
}

fn wrapped(bytes: &[u8]) -> Value {
    json!({ "key_version": 1, "wrapped": base64(bytes) })
}

/// An account with one approved device and one that is waiting.
async fn laptop_and_waiting_phone(
    app: &Router,
) -> (RegisterDeviceResponse, RegisterDeviceResponse) {
    let laptop = register(app, "alice", "Laptop").await;
    let phone = register(app, "alice", "Phone").await;
    assert_eq!(phone.status, DeviceStatus::Pending);
    (laptop, phone)
}

async fn age_request(pool: &PgPool, device: Uuid, days: i32) {
    sqlx::query("UPDATE devices SET created_at = now() - make_interval(days => $2) WHERE id = $1")
        .bind(device)
        .bind(days)
        .execute(pool)
        .await
        .unwrap();
}

// ---- The mailbox ----

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn the_two_devices_of_an_exchange_relay_messages_through_the_server(pool: PgPool) {
    let app = app(pool);
    let (laptop, phone) = laptop_and_waiting_phone(&app).await;

    let first = send(&app, &phone.token, phone.device_id, b"commit", true).await;
    assert_eq!(first.json::<EnrolmentMessage>().seq, 1);
    let second = send(&app, &laptop.token, phone.device_id, b"key", false).await;
    assert_eq!(second.json::<EnrolmentMessage>().seq, 2);

    // Each side reads what the other sent, in order, with who sent it.
    let all = read(&app, &laptop.token, phone.device_id, 0).await;
    assert_eq!(all.len(), 2);
    assert_eq!(
        (all[0].sender, all[0].body.as_slice()),
        (phone.device_id, &b"commit"[..])
    );
    assert_eq!(
        (all[1].sender, all[1].body.as_slice()),
        (laptop.device_id, &b"key"[..])
    );
    let newer = read(&app, &phone.token, phone.device_id, 1).await;
    assert_eq!(newer.len(), 1);
    assert_eq!(newer[0].seq, 2);
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn beginning_again_drops_the_old_messages_and_numbers_keep_rising(pool: PgPool) {
    let app = app(pool);
    let (laptop, phone) = laptop_and_waiting_phone(&app).await;
    send(&app, &phone.token, phone.device_id, b"first try", true).await;
    send(&app, &laptop.token, phone.device_id, b"answer", false).await;
    put(
        &app,
        &key_path(phone.device_id),
        &laptop.token,
        wrapped(b"old key"),
    )
    .await;

    let again = send(&app, &phone.token, phone.device_id, b"second try", true).await;

    // A reader that had seen up to 2 misses nothing and repeats nothing.
    assert_eq!(again.json::<EnrolmentMessage>().seq, 3);
    let all = read(&app, &laptop.token, phone.device_id, 0).await;
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].body, b"second try");
    // The key left for the first try is gone with it.
    let key = get(&app, &key_path(phone.device_id), Some(&phone.token)).await;
    assert_eq!(key.code(), "not_found");

    // Only the waiting device may begin again.
    let by_approver = send(&app, &laptop.token, phone.device_id, b"x", true).await;
    assert_eq!(by_approver.code(), "invalid_request");
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn an_exchange_is_closed_to_everyone_else(pool: PgPool) {
    let app = app(pool);
    let (_laptop, phone) = laptop_and_waiting_phone(&app).await;
    let tablet = register(&app, "alice", "Tablet").await;
    let mallory = register(&app, "mallory", "Laptop").await;
    send(&app, &phone.token, phone.device_id, b"commit", true).await;

    // Another waiting device of the same account.
    for answer in [
        send(&app, &tablet.token, phone.device_id, b"x", false).await,
        get(&app, &messages_path(phone.device_id), Some(&tablet.token)).await,
        get(&app, &key_path(phone.device_id), Some(&tablet.token)).await,
        post(
            &app,
            &complete_path(phone.device_id),
            Some(&tablet.token),
            json!({}),
        )
        .await,
    ] {
        assert_eq!(answer.status, StatusCode::FORBIDDEN);
        assert_eq!(answer.code(), "device_pending");
    }
    // An approved device of another account sees no such request.
    for answer in [
        send(&app, &mallory.token, phone.device_id, b"x", false).await,
        get(&app, &messages_path(phone.device_id), Some(&mallory.token)).await,
        put(
            &app,
            &key_path(phone.device_id),
            &mallory.token,
            wrapped(b"k"),
        )
        .await,
    ] {
        assert_eq!(answer.code(), "not_found");
    }
    // Nobody at all without a token.
    let anonymous = get(&app, &messages_path(phone.device_id), None).await;
    assert_eq!(anonymous.code(), "unauthenticated");

    assert_eq!(read(&app, &phone.token, phone.device_id, 0).await.len(), 1);
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn an_exchange_is_bounded_in_size_and_number(pool: PgPool) {
    let app = app(pool);
    let (laptop, phone) = laptop_and_waiting_phone(&app).await;

    let empty = send(&app, &phone.token, phone.device_id, b"", true).await;
    assert_eq!(empty.code(), "invalid_request");
    let large = vec![1u8; limits::MAX_ENROLMENT_MESSAGE_BYTES + 1];
    let too_large = send(&app, &phone.token, phone.device_id, &large, true).await;
    assert_eq!(too_large.code(), "invalid_request");

    for _ in 0..limits::MAX_ENROLMENT_MESSAGES {
        let answer = send(&app, &laptop.token, phone.device_id, b"m", false).await;
        assert_eq!(answer.status, StatusCode::OK);
    }
    let one_more = send(&app, &laptop.token, phone.device_id, b"m", false).await;
    assert_eq!(one_more.code(), "invalid_request");
    // Beginning again makes room.
    let again = send(&app, &phone.token, phone.device_id, b"m", true).await;
    assert_eq!(again.status, StatusCode::OK);

    let big_key = vec![1u8; limits::MAX_WRAPPED_KEY_BYTES + 1];
    let refused = put(
        &app,
        &key_path(phone.device_id),
        &laptop.token,
        wrapped(&big_key),
    )
    .await;
    assert_eq!(refused.code(), "invalid_request");
}

// ---- The key, and becoming approved ----

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_device_is_approved_only_when_it_says_it_has_the_key(pool: PgPool) {
    let app = app(pool.clone());
    let (laptop, phone) = laptop_and_waiting_phone(&app).await;
    let note = note_id();
    push_one(&app, &laptop.token, note, 0, b"secret").await;
    send(&app, &phone.token, phone.device_id, b"commit", true).await;

    // It cannot approve itself: no approved device has left a key.
    let early = post(
        &app,
        &complete_path(phone.device_id),
        Some(&phone.token),
        json!({}),
    )
    .await;
    assert_eq!(early.code(), "not_found");
    // Nor leave itself one.
    let own = put(
        &app,
        &key_path(phone.device_id),
        &phone.token,
        wrapped(b"k"),
    )
    .await;
    assert_eq!(own.code(), "device_pending");

    let stored = put(
        &app,
        &key_path(phone.device_id),
        &laptop.token,
        wrapped(b"sealed"),
    )
    .await;
    assert_eq!(stored.status, StatusCode::OK);
    // The key has been left, and the device is still waiting.
    assert_eq!(
        me(&app, &phone.token).await.device.status,
        DeviceStatus::Pending
    );
    let cannot_read = get(&app, "/v1/notes/pull", Some(&phone.token)).await;
    assert_eq!(cannot_read.code(), "device_pending");

    let key: WrappedKey = get(&app, &key_path(phone.device_id), Some(&phone.token))
        .await
        .json();
    assert_eq!(key.wrapped, b"sealed");
    // The approving device cannot say "received" on the other's behalf.
    let wrong_side = post(
        &app,
        &complete_path(phone.device_id),
        Some(&laptop.token),
        json!({}),
    )
    .await;
    assert_eq!(wrong_side.code(), "invalid_request");

    let done = post(
        &app,
        &complete_path(phone.device_id),
        Some(&phone.token),
        json!({}),
    )
    .await;
    assert_eq!(done.status, StatusCode::OK);

    assert_eq!(
        me(&app, &phone.token).await.device.status,
        DeviceStatus::Approved
    );
    assert_eq!(pull(&app, &phone.token, 0).await.items.len(), 1);
    // The server kept neither the key nor the messages.
    let (messages, keys): (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM enrolment_messages), (SELECT count(*) FROM enrolment_keys)",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((messages, keys), (0, 0));
    // And there is no request left to take part in.
    let over = get(&app, &messages_path(phone.device_id), Some(&laptop.token)).await;
    assert_eq!(over.code(), "not_found");
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn refusing_a_device_ends_its_exchange(pool: PgPool) {
    let app = app(pool.clone());
    let (laptop, phone) = laptop_and_waiting_phone(&app).await;
    send(&app, &phone.token, phone.device_id, b"commit", true).await;
    put(
        &app,
        &key_path(phone.device_id),
        &laptop.token,
        wrapped(b"sealed"),
    )
    .await;

    let path = format!("/v1/devices/{}", phone.device_id);
    assert_eq!(
        delete(&app, &path, &laptop.token).await.status,
        StatusCode::OK
    );

    let after = post(
        &app,
        &complete_path(phone.device_id),
        Some(&phone.token),
        json!({}),
    )
    .await;
    assert_eq!(after.code(), "device_revoked");
    let (messages, keys): (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM enrolment_messages), (SELECT count(*) FROM enrolment_keys)",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((messages, keys), (0, 0));
}

// ---- Requests that lapse ----

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_request_to_join_lapses_and_frees_its_place(pool: PgPool) {
    let app = app(pool.clone());
    let laptop = register(&app, "alice", "Laptop").await;
    let mut waiting = Vec::new();
    for name in ["One", "Two", "Three"] {
        waiting.push(register(&app, "alice", name).await);
    }
    assert_eq!(pull(&app, &laptop.token, 0).await.pending_devices, 3);
    let full = common::try_register(&app, "alice", "Four").await;
    assert_eq!(full.code(), "too_many_pending_devices");

    // Just inside the limit a request still stands.
    let days = i32::try_from(limits::PENDING_DEVICE_DAYS).unwrap();
    age_request(&pool, waiting[0].device_id, days - 1).await;
    assert_eq!(
        me(&app, &waiting[0].token).await.device.status,
        DeviceStatus::Pending
    );

    age_request(&pool, waiting[0].device_id, days + 1).await;

    let lapsed = get(&app, "/v1/devices/me", Some(&waiting[0].token)).await;
    assert_eq!(lapsed.code(), "device_expired");
    // Nobody can take part in its exchange any more.
    let closed = send(&app, &laptop.token, waiting[0].device_id, b"key", false).await;
    assert_eq!(closed.code(), "not_found");
    // It no longer counts, so another device may ask.
    assert_eq!(pull(&app, &laptop.token, 0).await.pending_devices, 2);
    assert_eq!(
        register(&app, "alice", "Four").await.status,
        DeviceStatus::Pending
    );

    // Signing in again on the lapsed device renews its request, if there is
    // room. There is none now.
    sqlx::query("DELETE FROM devices WHERE name = 'Four'")
        .execute(&pool)
        .await
        .unwrap();
    let renewed = sign_in_again(&app, "alice", "One", vec![7; 32], &waiting[0].token).await;
    assert_eq!(renewed.device_id, waiting[0].device_id);
    assert_eq!(renewed.status, DeviceStatus::Pending);
    assert_eq!(
        me(&app, &renewed.token).await.device.status,
        DeviceStatus::Pending
    );
}

// ---- What a device is told ----

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_device_learns_who_could_approve_it_and_whether_a_phrase_would(pool: PgPool) {
    let app = app(pool);
    let (laptop, phone) = laptop_and_waiting_phone(&app).await;

    let waiting = me(&app, &phone.token).await;
    assert_eq!((waiting.approvers, waiting.recovery_available), (1, false));
    // The only approved device has nobody else to rely on.
    let alone = me(&app, &laptop.token).await;
    assert_eq!((alone.approvers, alone.recovery_available), (0, false));
    assert!(alone.reset_effective_at_ms.is_none());

    let stored = put(
        &app,
        "/v1/recovery",
        &laptop.token,
        json!({ "key_version": 1, "wrapped": base64(b"sealed"), "claim_hash": base64(&[0; 32]) }),
    )
    .await;
    assert_eq!(stored.status, StatusCode::OK);
    assert!(me(&app, &phone.token).await.recovery_available);
    assert!(me(&app, &laptop.token).await.recovery_available);
}

// ---- The recovery copy ----

fn recovery_body(sealed: &[u8], claim_code: &[u8]) -> Value {
    json!({
        "key_version": 1,
        "wrapped": base64(sealed),
        "claim_hash": base64(&Sha256::digest(claim_code)),
    })
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_waiting_device_with_the_claim_code_is_approved(pool: PgPool) {
    let app = app(pool);
    let (laptop, phone) = laptop_and_waiting_phone(&app).await;
    push_one(&app, &laptop.token, note_id(), 0, b"secret").await;
    put(
        &app,
        "/v1/recovery",
        &laptop.token,
        recovery_body(b"sealed", b"the code"),
    )
    .await;

    // The waiting device may read the copy.
    let copy: WrappedKey = get(&app, "/v1/recovery", Some(&phone.token)).await.json();
    assert_eq!(
        (copy.key_version, copy.wrapped.as_slice()),
        (1, &b"sealed"[..])
    );

    // A wrong code gets nothing and changes nothing.
    let wrong = post(
        &app,
        "/v1/recovery/claim",
        Some(&phone.token),
        json!({ "claim_code": base64(b"a guess") }),
    )
    .await;
    assert_eq!(wrong.status, StatusCode::FORBIDDEN);
    assert_eq!(wrong.code(), "recovery_rejected");
    assert_eq!(
        me(&app, &phone.token).await.device.status,
        DeviceStatus::Pending
    );

    let right = post(
        &app,
        "/v1/recovery/claim",
        Some(&phone.token),
        json!({ "claim_code": base64(b"the code") }),
    )
    .await;
    assert_eq!(right.status, StatusCode::OK);
    assert_eq!(
        me(&app, &phone.token).await.device.status,
        DeviceStatus::Approved
    );
    assert_eq!(pull(&app, &phone.token, 0).await.items.len(), 1);
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_waiting_device_cannot_make_replace_or_remove_the_recovery_copy(pool: PgPool) {
    let app = app(pool);
    let (laptop, phone) = laptop_and_waiting_phone(&app).await;

    // With no copy there is nothing to read or claim.
    assert_eq!(
        get(&app, "/v1/recovery", Some(&phone.token)).await.code(),
        "not_found"
    );
    let no_copy = post(
        &app,
        "/v1/recovery/claim",
        Some(&phone.token),
        json!({ "claim_code": base64(b"x") }),
    )
    .await;
    assert_eq!(no_copy.code(), "not_found");

    // A waiting device that could write the copy would choose the claim
    // code, and so approve itself.
    let own = put(
        &app,
        "/v1/recovery",
        &phone.token,
        recovery_body(b"mine", b"mine"),
    )
    .await;
    assert_eq!(own.code(), "device_pending");

    put(
        &app,
        "/v1/recovery",
        &laptop.token,
        recovery_body(b"sealed", b"the code"),
    )
    .await;
    let replace = put(
        &app,
        "/v1/recovery",
        &phone.token,
        recovery_body(b"mine", b"mine"),
    )
    .await;
    assert_eq!(replace.code(), "device_pending");
    let remove = delete(&app, "/v1/recovery", &phone.token).await;
    assert_eq!(remove.code(), "device_pending");
    let copy: WrappedKey = get(&app, "/v1/recovery", Some(&laptop.token)).await.json();
    assert_eq!(copy.wrapped, b"sealed");

    // The approved device may replace it and remove it.
    put(
        &app,
        "/v1/recovery",
        &laptop.token,
        recovery_body(b"newer", b"new code"),
    )
    .await;
    let old_code = post(
        &app,
        "/v1/recovery/claim",
        Some(&phone.token),
        json!({ "claim_code": base64(b"the code") }),
    )
    .await;
    assert_eq!(old_code.code(), "recovery_rejected");
    assert_eq!(
        delete(&app, "/v1/recovery", &laptop.token).await.status,
        StatusCode::OK
    );
    assert_eq!(
        delete(&app, "/v1/recovery", &laptop.token).await.code(),
        "not_found"
    );

    // A badly formed copy is refused.
    let bad = put(
        &app,
        "/v1/recovery",
        &laptop.token,
        json!({ "key_version": 1, "wrapped": base64(b"sealed"), "claim_hash": base64(b"short") }),
    )
    .await;
    assert_eq!(bad.code(), "invalid_request");
}

// ---- Deleting an account ----

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn deleting_an_account_removes_everything_the_server_holds_for_it(pool: PgPool) {
    let app = app(pool.clone());
    let (laptop, phone) = laptop_and_waiting_phone(&app).await;
    let bob = register(&app, "bob", "Laptop").await;
    push_one(&app, &laptop.token, note_id(), 0, b"secret").await;
    push_one(&app, &bob.token, note_id(), 0, b"bob's").await;
    put(
        &app,
        "/v1/recovery",
        &laptop.token,
        recovery_body(b"sealed", b"code"),
    )
    .await;
    send(&app, &phone.token, phone.device_id, b"commit", true).await;

    // A waiting device may not.
    assert_eq!(
        delete(&app, "/v1/account", &phone.token).await.code(),
        "device_pending"
    );

    assert_eq!(
        delete(&app, "/v1/account", &laptop.token).await.status,
        StatusCode::OK
    );

    let counts: (i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM accounts), (SELECT count(*) FROM devices), \
                (SELECT count(*) FROM notes), (SELECT count(*) FROM recovery_keys), \
                (SELECT count(*) FROM enrolment_messages)",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    // Only Bob's account, device and note are left.
    assert_eq!(counts, (1, 1, 1, 0, 0));
    for token in [&laptop.token, &phone.token] {
        let gone = get(&app, "/v1/devices/me", Some(token)).await;
        assert_eq!(gone.code(), "unauthenticated");
    }
    assert_eq!(pull(&app, &bob.token, 0).await.items.len(), 1);

    // The same person signing in afterwards starts a new account.
    let fresh = register(&app, "alice", "Laptop").await;
    assert!(fresh.first_device);
    assert_ne!(fresh.account_id, laptop.account_id);
}

// ---- Resetting an account ----

async fn reset(app: &Router, token: &str) -> ResetResponse {
    post(app, "/v1/account/reset", Some(token), json!({}))
        .await
        .json()
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn an_approved_device_resets_the_account_at_once(pool: PgPool) {
    let app = app(pool.clone());
    let (laptop, phone) = laptop_and_waiting_phone(&app).await;
    let note = note_id();
    push_one(&app, &laptop.token, note, 0, b"old").await;
    put(
        &app,
        "/v1/recovery",
        &laptop.token,
        recovery_body(b"sealed", b"code"),
    )
    .await;

    let answer = reset(&app, &laptop.token).await;

    assert!(answer.done);
    assert_eq!(answer.epoch, 2);
    // The notes, the recovery copy and the other device are gone.
    let page = pull(&app, &laptop.token, 0).await;
    assert_eq!((page.epoch, page.items.len()), (2, 0));
    assert!(!me(&app, &laptop.token).await.recovery_available);
    assert_eq!(
        get(&app, "/v1/devices/me", Some(&phone.token)).await.code(),
        "unauthenticated"
    );
    let devices: DeviceList = get(&app, "/v1/devices", Some(&laptop.token)).await.json();
    assert_eq!(devices.devices.len(), 1);
    assert_eq!(common::usage(&pool, laptop.account_id).await, (0, 0, 0));

    // The account takes notes again, numbered from the start.
    assert!(matches!(
        push_one(&app, &laptop.token, note, 0, b"new").await,
        PushOutcome::Accepted { change: 1 }
    ));
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_waiting_device_resets_an_account_that_holds_no_notes_at_once(pool: PgPool) {
    let app = app(pool);
    // The first device registered and then lost its key before sending
    // anything: nobody can ever approve the second one.
    let (lost, phone) = laptop_and_waiting_phone(&app).await;

    let answer = reset(&app, &phone.token).await;

    assert!(answer.done);
    assert_eq!(
        me(&app, &phone.token).await.device.status,
        DeviceStatus::Approved
    );
    assert_eq!(
        get(&app, "/v1/devices/me", Some(&lost.token)).await.code(),
        "unauthenticated"
    );
    assert!(matches!(
        push_one(&app, &phone.token, note_id(), 0, b"first").await,
        PushOutcome::Accepted { .. }
    ));
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_waiting_device_can_only_ask_for_a_reset_of_an_account_with_notes(pool: PgPool) {
    let app = app(pool.clone());
    let (laptop, phone) = laptop_and_waiting_phone(&app).await;
    let tablet = register(&app, "alice", "Tablet").await;
    push_one(&app, &laptop.token, note_id(), 0, b"precious").await;

    let asked = reset(&app, &phone.token).await;

    assert!(!asked.done);
    let effective = asked.effective_at_ms.unwrap();
    // Nothing has changed yet, and the working device is told.
    let page = pull(&app, &laptop.token, 0).await;
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.reset_effective_at_ms, Some(effective));
    assert_eq!(
        me(&app, &laptop.token).await.reset_effective_at_ms,
        Some(effective)
    );
    // Asking again does not restart the wait, and another waiting device
    // neither replaces the request nor inherits it.
    assert_eq!(
        reset(&app, &phone.token).await.effective_at_ms,
        Some(effective)
    );
    assert_eq!(
        reset(&app, &tablet.token).await.effective_at_ms,
        Some(effective)
    );

    // When the wait is over, only the device that asked may go ahead.
    sqlx::query("UPDATE accounts SET reset_requested_at = now() - make_interval(days => $1)")
        .bind(i32::try_from(limits::RESET_DELAY_DAYS).unwrap() + 1)
        .execute(&pool)
        .await
        .unwrap();
    assert!(!reset(&app, &tablet.token).await.done);
    assert_eq!(pull(&app, &laptop.token, 0).await.items.len(), 1);
    // The device that asked can still reach the server, though its request
    // to join is older than a request may be.
    age_request(&pool, phone.device_id, 30).await;
    let done = reset(&app, &phone.token).await;
    assert!(done.done);
    assert_eq!(done.epoch, 2);
    assert_eq!(
        me(&app, &phone.token).await.device.status,
        DeviceStatus::Approved
    );
    assert_eq!(
        get(&app, "/v1/devices/me", Some(&laptop.token))
            .await
            .code(),
        "unauthenticated"
    );
    assert!(pull(&app, &phone.token, 0).await.items.is_empty());
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_working_device_refuses_a_reset_somebody_asked_for(pool: PgPool) {
    let app = app(pool.clone());
    let (laptop, phone) = laptop_and_waiting_phone(&app).await;
    push_one(&app, &laptop.token, note_id(), 0, b"precious").await;
    // Nothing to cancel yet.
    assert_eq!(
        delete(&app, "/v1/account/reset", &laptop.token)
            .await
            .code(),
        "not_found"
    );
    assert!(!reset(&app, &phone.token).await.done);

    // The device that asked cannot cancel on the account's behalf, which
    // matters little, and a waiting device cannot cancel another's.
    assert_eq!(
        delete(&app, "/v1/account/reset", &phone.token).await.code(),
        "device_pending"
    );
    assert_eq!(
        delete(&app, "/v1/account/reset", &laptop.token)
            .await
            .status,
        StatusCode::OK
    );
    assert!(pull(&app, &laptop.token, 0)
        .await
        .reset_effective_at_ms
        .is_none());

    // A new request starts a new wait.
    assert!(!reset(&app, &phone.token).await.done);
    // Removing the device that asked ends its request too.
    let path = format!("/v1/devices/{}", phone.device_id);
    delete(&app, &path, &laptop.token).await;
    let page = pull(&app, &laptop.token, 0).await;
    assert!(page.reset_effective_at_ms.is_none());
    assert_eq!(page.items.len(), 1);
}
