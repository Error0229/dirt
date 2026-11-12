//! Registration, device rights, and the request limit, against Postgres.
//!
//! Start the database with `docker compose up -d` at the repository root and
//! set `DATABASE_URL` as `compose.yaml` shows. Each test runs in its own
//! throwaway database.

mod common;

use axum::http::{Method, StatusCode};
use common::{
    app, call, claims, get, nonce, note_id, post, pull, push_one, register, registration, sign,
    sign_in_again, sign_with_another_key, try_register,
};
use dirt_proto::api::{
    limits, DeviceInfo, DeviceList, DeviceStatus, NonceResponse, PushOutcome,
    RegisterDeviceResponse, EPOCH_HEADER, PROTOCOL_HEADER,
};
use serde_json::json;
use sqlx::PgPool;

// ---- Health and protocol ----

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn health_answers_and_unknown_paths_get_a_protocol_error(pool: PgPool) {
    let app = app(pool);
    let health = get(&app, "/healthz", None).await;
    assert_eq!(health.status, StatusCode::OK);
    assert_eq!(health.body["ok"], true);

    let missing = get(&app, "/v1/nothing-here", None).await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    assert_eq!(missing.code(), "not_found");
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_client_on_a_newer_protocol_is_told_to_update(pool: PgPool) {
    let app = app(pool);
    for (version, status, code) in [
        ("1", StatusCode::OK, ""),
        ("2", StatusCode::UPGRADE_REQUIRED, "unsupported_protocol"),
        ("one", StatusCode::BAD_REQUEST, "invalid_request"),
    ] {
        let request = axum::http::Request::builder()
            .uri("/healthz")
            .header(PROTOCOL_HEADER, version)
            .body(axum::body::Body::empty())
            .unwrap();
        let response = tower::ServiceExt::oneshot(app.clone(), request)
            .await
            .unwrap();
        assert_eq!(response.status(), status, "version {version}");
        if !code.is_empty() {
            let bytes = http_body_util::BodyExt::collect(response.into_body())
                .await
                .unwrap()
                .to_bytes();
            let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(body["code"], code);
        }
    }
}

// ---- Registration ----

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn the_first_device_is_approved_and_later_ones_wait(pool: PgPool) {
    let app = app(pool);
    let first = register(&app, "alice", "Laptop").await;
    assert_eq!(first.status, DeviceStatus::Approved);
    assert!(first.first_device);
    assert_eq!(first.epoch, 1);

    let second = register(&app, "alice", "Phone").await;
    assert_eq!(second.status, DeviceStatus::Pending);
    assert!(!second.first_device);
    assert_eq!(second.account_id, first.account_id);
    assert_ne!(second.device_id, first.device_id);
    assert_ne!(second.token, first.token);

    let other = register(&app, "bob", "Laptop").await;
    assert_ne!(other.account_id, first.account_id);
    assert!(other.first_device);
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_nonce_comes_with_where_to_sign_in(pool: PgPool) {
    let app = app(pool);
    let answer: NonceResponse = call(&app, Method::POST, "/v1/nonces", None, None)
        .await
        .json();
    assert_eq!(answer.issuer, common::ISSUER);
    assert_eq!(answer.client_id, common::CLIENT_ID);
    assert_eq!(answer.expires_in_secs, limits::NONCE_LIFETIME_SECS);
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_nonce_works_once(pool: PgPool) {
    let app = app(pool);
    let nonce = nonce(&app).await;
    let id_token = sign(&claims("alice", &nonce));
    let body = registration(&id_token, &nonce, "Laptop");

    assert_eq!(
        post(&app, "/v1/devices", None, body.clone()).await.status,
        StatusCode::OK
    );

    // The same token and nonce again: the token is still valid for minutes,
    // but the nonce is spent.
    let replay = post(&app, "/v1/devices", None, body).await;
    assert_eq!(replay.status, StatusCode::UNAUTHORIZED);
    assert_eq!(replay.code(), "sign_in_rejected");
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_nonce_the_server_never_issued_or_that_expired_is_refused(pool: PgPool) {
    let app = app(pool.clone());
    let invented = "a-nonce-the-server-never-issued";
    let id_token = sign(&claims("alice", invented));
    let answer = post(
        &app,
        "/v1/devices",
        None,
        registration(&id_token, invented, "Laptop"),
    )
    .await;
    assert_eq!(answer.code(), "sign_in_rejected");

    let nonce = nonce(&app).await;
    sqlx::query("UPDATE nonces SET expires_at = now() - interval '1 second'")
        .execute(&pool)
        .await
        .unwrap();
    let id_token = sign(&claims("alice", &nonce));
    let answer = post(
        &app,
        "/v1/devices",
        None,
        registration(&id_token, &nonce, "Laptop"),
    )
    .await;
    assert_eq!(answer.code(), "sign_in_rejected");
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_bad_id_token_is_refused_and_does_not_spend_the_nonce(pool: PgPool) {
    let app = app(pool);
    let nonce = nonce(&app).await;
    let good = claims("alice", &nonce);

    let mut wrong_client = good.clone();
    wrong_client["aud"] = json!(["some-other-client"]);
    let mut expired = good.clone();
    expired["exp"] = json!(1);
    let mut other_nonce = good.clone();
    other_nonce["nonce"] = json!("a-different-nonce");

    for (what, id_token) in [
        ("issued for another client", sign(&wrong_client)),
        ("expired", sign(&expired)),
        ("carrying another nonce", sign(&other_nonce)),
        ("signed by another key", sign_with_another_key(&good)),
        ("not a token", "garbage".to_owned()),
    ] {
        let answer = post(
            &app,
            "/v1/devices",
            None,
            registration(&id_token, &nonce, "Laptop"),
        )
        .await;
        assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{what}");
        assert_eq!(answer.code(), "sign_in_rejected", "{what}");
    }

    // None of those attempts used the nonce up.
    let answer = post(
        &app,
        "/v1/devices",
        None,
        registration(&sign(&good), &nonce, "Laptop"),
    )
    .await;
    assert_eq!(answer.status, StatusCode::OK);
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_bad_name_or_public_key_is_refused(pool: PgPool) {
    let app = app(pool);
    let nonce = nonce(&app).await;
    let id_token = sign(&claims("alice", &nonce));

    let mut empty_name = registration(&id_token, &nonce, "   ");
    let long_name = registration(&id_token, &nonce, &"x".repeat(101));
    let mut short_key = registration(&id_token, &nonce, "Laptop");
    short_key["public_key"] = json!("AAAA");
    empty_name["name"] = json!("   ");

    for body in [
        empty_name,
        long_name,
        short_key,
        json!({"not": "a registration"}),
    ] {
        let answer = post(&app, "/v1/devices", None, body).await;
        assert_eq!(answer.status, StatusCode::BAD_REQUEST);
        assert_eq!(answer.code(), "invalid_request");
    }
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn devices_registering_at_the_same_moment_yield_one_approved_device(pool: PgPool) {
    let app = app(pool.clone());
    let mut tasks = Vec::new();
    for index in 0..8 {
        let app = app.clone();
        tasks.push(tokio::spawn(async move {
            try_register(&app, "alice", &format!("Device {index}")).await
        }));
    }
    let mut approved = 0;
    let mut pending = 0;
    let mut refused = 0;
    for task in tasks {
        let answer = task.await.unwrap();
        if answer.status == StatusCode::OK {
            let device: RegisterDeviceResponse = answer.json();
            match device.status {
                DeviceStatus::Approved => {
                    assert!(device.first_device);
                    approved += 1;
                }
                DeviceStatus::Pending => {
                    assert!(!device.first_device);
                    pending += 1;
                }
                DeviceStatus::Revoked => panic!("a new device cannot be revoked"),
            }
        } else {
            assert_eq!(answer.code(), "too_many_pending_devices");
            refused += 1;
        }
    }
    assert_eq!(approved, 1, "exactly one device creates the account key");
    assert_eq!(approved + pending + refused, 8);
    assert!(pending >= 1);

    let (accounts,): (i64,) = sqlx::query_as("SELECT count(*) FROM accounts")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(accounts, 1, "one person, one account");
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn an_account_may_have_only_so_many_devices_waiting(pool: PgPool) {
    let app = app(pool);
    register(&app, "alice", "First").await;
    for index in 0..limits::MAX_PENDING_DEVICES {
        let device = register(&app, "alice", &format!("Waiting {index}")).await;
        assert_eq!(device.status, DeviceStatus::Pending);
    }
    let answer = try_register(&app, "alice", "One too many").await;
    assert_eq!(answer.status, StatusCode::CONFLICT);
    assert_eq!(answer.code(), "too_many_pending_devices");
}

async fn approve(pool: &PgPool, device: uuid::Uuid) {
    sqlx::query("UPDATE devices SET status = 'approved' WHERE id = $1")
        .bind(device)
        .execute(pool)
        .await
        .unwrap();
}

async fn remove(app: &axum::Router, token: &str, device: uuid::Uuid) {
    let path = format!("/v1/devices/{device}");
    let answer = call(app, Method::DELETE, &path, Some(token), None).await;
    assert_eq!(answer.status, StatusCode::OK);
}

async fn device_rows(pool: &PgPool, status: &str) -> i64 {
    let (rows,): (i64,) = sqlx::query_as("SELECT count(*) FROM devices WHERE status = $1")
        .bind(status)
        .fetch_one(pool)
        .await
        .unwrap();
    rows
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn an_account_may_have_only_so_many_devices(pool: PgPool) {
    let app = app(pool.clone());
    let first = register(&app, "alice", "First").await;
    let mut last = first.device_id;
    for index in 1..limits::MAX_DEVICES_PER_ACCOUNT {
        let device = register(&app, "alice", &format!("Device {index}")).await;
        approve(&pool, device.device_id).await;
        last = device.device_id;
    }

    let answer = try_register(&app, "alice", "One too many").await;
    assert_eq!(answer.status, StatusCode::CONFLICT);
    assert_eq!(answer.code(), "too_many_devices");
    // The refusal is the account's alone.
    register(&app, "bob", "Bob's first").await;

    // A device that is removed gives up its place.
    remove(&app, &first.token, last).await;
    let device = register(&app, "alice", "In its place").await;
    assert_eq!(device.status, DeviceStatus::Pending);
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_waiting_device_holds_a_place_too(pool: PgPool) {
    let app = app(pool.clone());
    register(&app, "alice", "First").await;
    let waiting = limits::MAX_PENDING_DEVICES;
    for index in 1..limits::MAX_DEVICES_PER_ACCOUNT - waiting {
        let device = register(&app, "alice", &format!("Device {index}")).await;
        approve(&pool, device.device_id).await;
    }
    for index in 0..waiting {
        register(&app, "alice", &format!("Waiting {index}")).await;
    }
    // The requests lapse without being answered. They no longer count as
    // waiting, but each can still be renewed, so each keeps its place.
    sqlx::query(
        "UPDATE devices SET created_at = now() - interval '8 days' WHERE status = 'pending'",
    )
    .execute(&pool)
    .await
    .unwrap();

    let answer = try_register(&app, "alice", "One too many").await;
    assert_eq!(answer.code(), "too_many_devices");
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_removed_device_is_forgotten_after_a_while(pool: PgPool) {
    let app = app(pool.clone());
    let laptop = register(&app, "alice", "Laptop").await;
    let long_ago = register(&app, "alice", "Removed long ago").await;
    let lately = register(&app, "alice", "Removed lately").await;
    for device in [&long_ago, &lately] {
        approve(&pool, device.device_id).await;
        remove(&app, &laptop.token, device.device_id).await;
    }
    // Another account removed a device as long ago.
    let bob = register(&app, "bob", "Bob's laptop").await;
    let bobs = register(&app, "bob", "Bob's old phone").await;
    remove(&app, &bob.token, bobs.device_id).await;
    // The time of removal is moved back from where the server set it, so
    // that this fails if the server did not record it.
    sqlx::query(
        "UPDATE devices SET revoked_at = revoked_at - make_interval(days => $3) \
          WHERE id IN ($1, $2)",
    )
    .bind(long_ago.device_id)
    .bind(bobs.device_id)
    .bind(i32::try_from(limits::REMOVED_DEVICE_DAYS).unwrap() + 1)
    .execute(&pool)
    .await
    .unwrap();
    // Nothing is forgotten until a device registers.
    assert_eq!(device_rows(&pool, "revoked").await, 3);

    register(&app, "alice", "Phone").await;

    // An account tidies up after itself only: Bob's is still remembered.
    assert_eq!(device_rows(&pool, "revoked").await, 2);
    let answer = get(&app, "/v1/devices/me", Some(&bobs.token)).await;
    assert_eq!(answer.code(), "device_revoked");
    // The one removed lately is still told why it is refused.
    let answer = get(&app, "/v1/devices/me", Some(&lately.token)).await;
    assert_eq!(answer.code(), "device_revoked");
    // The other's token is unknown now. Either way it has to sign in again.
    let answer = get(&app, "/v1/devices/me", Some(&long_ago.token)).await;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED);
    assert_eq!(answer.code(), "unauthenticated");
    // And it comes back as a new device, not as the one it was.
    let again = sign_in_again(&app, "alice", "Back", vec![7; 32], &long_ago.token).await;
    assert_ne!(again.device_id, long_ago.device_id);
    assert_eq!(again.status, DeviceStatus::Pending);
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_request_to_join_that_lapsed_long_ago_is_forgotten(pool: PgPool) {
    let app = app(pool.clone());
    register(&app, "alice", "Laptop").await;
    let lapsed = register(&app, "alice", "Lapsed").await;
    let asked_for_a_reset = register(&app, "alice", "Asked for a reset").await;
    let waiting = register(&app, "alice", "Still waiting").await;
    sqlx::query(
        "UPDATE devices SET created_at = now() - make_interval(days => $3) WHERE id IN ($1, $2)",
    )
    .bind(lapsed.device_id)
    .bind(asked_for_a_reset.device_id)
    .bind(i32::try_from(limits::REMOVED_DEVICE_DAYS).unwrap() + 1)
    .execute(&pool)
    .await
    .unwrap();
    // A device that asked for the account to be reset has to be able to
    // come back when the wait is over, however long ago it asked to join.
    sqlx::query("UPDATE accounts SET reset_requested_at = now(), reset_requested_by = $1")
        .bind(asked_for_a_reset.device_id)
        .execute(&pool)
        .await
        .unwrap();

    register(&app, "alice", "Phone").await;

    let left: Vec<(uuid::Uuid,)> =
        sqlx::query_as("SELECT id FROM devices WHERE status = 'pending' ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
    let left: Vec<uuid::Uuid> = left.into_iter().map(|(id,)| id).collect();
    assert!(!left.contains(&lapsed.device_id));
    assert!(left.contains(&asked_for_a_reset.device_id));
    assert!(left.contains(&waiting.device_id));
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn only_so_many_removed_devices_are_remembered(pool: PgPool) {
    let app = app(pool.clone());
    register(&app, "alice", "Laptop").await;
    // A device signs in and out, over and over. Signing out of a waiting
    // device removes it.
    let mut removed = Vec::new();
    for index in 0..limits::MAX_REMOVED_DEVICES + 3 {
        let device = register(&app, "alice", &format!("Visit {index}")).await;
        remove(&app, &device.token, device.device_id).await;
        removed.push(device);
    }

    register(&app, "alice", "Phone").await;

    assert_eq!(
        device_rows(&pool, "revoked").await,
        limits::MAX_REMOVED_DEVICES
    );
    // The oldest went first.
    let oldest = get(&app, "/v1/devices/me", Some(&removed[0].token)).await;
    assert_eq!(oldest.code(), "unauthenticated");
    let newest = get(&app, "/v1/devices/me", Some(&removed.last().unwrap().token)).await;
    assert_eq!(newest.code(), "device_revoked");
}

// ---- Device rights ----

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn without_a_good_token_nothing_is_served(pool: PgPool) {
    let app = app(pool);
    register(&app, "alice", "Laptop").await;
    for token in [None, Some("not-a-token"), Some("")] {
        for path in ["/v1/devices", "/v1/devices/me", "/v1/notes/pull"] {
            let answer = get(&app, path, token).await;
            assert_eq!(
                answer.status,
                StatusCode::UNAUTHORIZED,
                "{path} with {token:?}"
            );
            assert_eq!(answer.code(), "unauthenticated");
        }
    }
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_pending_device_can_only_ask_about_itself(pool: PgPool) {
    let app = app(pool);
    let first = register(&app, "alice", "Laptop").await;
    let pending = register(&app, "alice", "Phone").await;
    let note = note_id();
    assert!(matches!(
        push_one(&app, &first.token, note, 0, b"secret").await,
        PushOutcome::Accepted { .. }
    ));

    let me = get(&app, "/v1/devices/me", Some(&pending.token)).await;
    let info: DeviceInfo = me.json();
    assert_eq!(info.status, DeviceStatus::Pending);
    assert!(info.current);
    assert_eq!(me.headers[EPOCH_HEADER], "1");

    for (method, path) in [
        (Method::GET, "/v1/devices".to_owned()),
        (Method::GET, "/v1/notes/pull".to_owned()),
        (Method::DELETE, format!("/v1/devices/{}", first.device_id)),
    ] {
        let answer = call(&app, method, &path, Some(&pending.token), None).await;
        assert_eq!(answer.status, StatusCode::FORBIDDEN, "{path}");
        assert_eq!(answer.code(), "device_pending");
    }
    let push = common::try_push(
        &app,
        &pending.token,
        vec![common::item(
            note_id(),
            0,
            b"from a pending device".to_vec(),
        )],
    )
    .await;
    assert_eq!(push.code(), "device_pending");

    // Nothing the pending device tried had any effect.
    assert_eq!(pull(&app, &first.token, 0).await.items.len(), 1);

    // It may remove itself, which is what signing out of it does.
    let path = format!("/v1/devices/{}", pending.device_id);
    let answer = call(&app, Method::DELETE, &path, Some(&pending.token), None).await;
    assert_eq!(answer.status, StatusCode::OK);
    let after = get(&app, "/v1/devices/me", Some(&pending.token)).await;
    assert_eq!(after.code(), "device_revoked");
    // That frees its place among the devices waiting.
    assert_eq!(
        register(&app, "alice", "Tablet").await.status,
        DeviceStatus::Pending
    );
    // The removed device stays in the list, marked as revoked.
    let devices: DeviceList = get(&app, "/v1/devices", Some(&first.token)).await.json();
    assert_eq!(devices.devices.len(), 3);
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn revoking_a_device_stops_its_token_at_once(pool: PgPool) {
    let app = app(pool);
    let first = register(&app, "alice", "Laptop").await;
    let second = register(&app, "alice", "Phone").await;
    assert_eq!(
        get(&app, "/v1/devices/me", Some(&second.token))
            .await
            .status,
        StatusCode::OK
    );

    let path = format!("/v1/devices/{}", second.device_id);
    let answer = call(&app, Method::DELETE, &path, Some(&first.token), None).await;
    assert_eq!(answer.status, StatusCode::OK);

    let after = get(&app, "/v1/devices/me", Some(&second.token)).await;
    assert_eq!(after.status, StatusCode::UNAUTHORIZED);
    assert_eq!(after.code(), "device_revoked");

    // The device stays in the list, marked as revoked.
    let devices: DeviceList = get(&app, "/v1/devices", Some(&first.token)).await.json();
    let revoked = devices
        .devices
        .iter()
        .find(|d| d.id == second.device_id)
        .unwrap();
    assert_eq!(revoked.status, DeviceStatus::Revoked);
    let current = devices
        .devices
        .iter()
        .find(|d| d.id == first.device_id)
        .unwrap();
    assert!(current.current && !revoked.current);
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn one_account_cannot_see_or_revoke_anothers_devices(pool: PgPool) {
    let app = app(pool);
    let alice = register(&app, "alice", "Laptop").await;
    let bob = register(&app, "bob", "Laptop").await;

    let devices: DeviceList = get(&app, "/v1/devices", Some(&alice.token)).await.json();
    assert_eq!(devices.devices.len(), 1);
    assert_eq!(devices.devices[0].id, alice.device_id);

    let path = format!("/v1/devices/{}", bob.device_id);
    let answer = call(&app, Method::DELETE, &path, Some(&alice.token), None).await;
    assert_eq!(answer.status, StatusCode::NOT_FOUND);
    assert_eq!(
        get(&app, "/v1/devices/me", Some(&bob.token)).await.status,
        StatusCode::OK,
        "bob's device still works"
    );

    for path in [
        "/v1/devices/not-a-uuid",
        "/v1/devices/01932aaa-0000-7000-8000-000000000123",
    ] {
        let answer = call(&app, Method::DELETE, path, Some(&alice.token), None).await;
        assert_eq!(answer.code(), "not_found", "{path}");
    }
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_token_unused_for_too_long_stops_working(pool: PgPool) {
    let app = app(pool.clone());
    let device = register(&app, "alice", "Laptop").await;

    // Just inside the limit: still good, and using it refreshes "last seen".
    sqlx::query("UPDATE devices SET last_seen = now() - make_interval(days => 89)")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        get(&app, "/v1/devices/me", Some(&device.token))
            .await
            .status,
        StatusCode::OK
    );
    let (recent,): (bool,) =
        sqlx::query_as("SELECT last_seen > now() - interval '1 minute' FROM devices")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(recent, "a request refreshes last_seen");

    sqlx::query("UPDATE devices SET last_seen = now() - make_interval(days => 91)")
        .execute(&pool)
        .await
        .unwrap();
    let answer = get(&app, "/v1/devices/me", Some(&device.token)).await;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED);
    assert_eq!(answer.code(), "device_expired");
}

async fn expire_every_token(pool: &PgPool) {
    sqlx::query("UPDATE devices SET last_seen = now() - make_interval(days => 91)")
        .execute(pool)
        .await
        .unwrap();
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_device_whose_token_expired_signs_in_again_as_itself(pool: PgPool) {
    let app = app(pool.clone());
    let laptop = register(&app, "alice", "Laptop").await;
    let phone = register(&app, "alice", "Phone").await;
    assert_eq!(phone.status, DeviceStatus::Pending);
    expire_every_token(&pool).await;

    let again = sign_in_again(&app, "alice", "Laptop", vec![7; 32], &laptop.token).await;

    assert_eq!(again.device_id, laptop.device_id);
    assert_eq!(again.account_id, laptop.account_id);
    assert_eq!(again.status, DeviceStatus::Approved);
    assert!(!again.first_device, "the account already has its key");
    assert_ne!(again.token, laptop.token);
    // The new token works and the old one is gone for good.
    assert_eq!(
        get(&app, "/v1/devices", Some(&again.token)).await.status,
        StatusCode::OK
    );
    let old = get(&app, "/v1/devices/me", Some(&laptop.token)).await;
    assert_eq!(old.code(), "unauthenticated");

    // A device that was still waiting keeps waiting: signing in again is
    // not a way round approval.
    let again = sign_in_again(&app, "alice", "Phone", vec![7; 32], &phone.token).await;
    assert_eq!(again.device_id, phone.device_id);
    assert_eq!(again.status, DeviceStatus::Pending);

    let (devices,): (i64,) = sqlx::query_as("SELECT count(*) FROM devices")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(devices, 2, "no device was added");
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn signing_in_again_needs_the_old_token_and_the_same_key(pool: PgPool) {
    let app = app(pool.clone());
    let laptop = register(&app, "alice", "Laptop").await;
    let mallory = register(&app, "mallory", "Laptop").await;
    expire_every_token(&pool).await;

    // The public key is no secret, so it is not enough: someone who can
    // sign in to the account but holds no device gets a new device that has
    // to be approved, as on any first sign-in.
    let with_key_only = register(&app, "alice", "Laptop").await;
    assert_ne!(with_key_only.device_id, laptop.device_id);
    assert_eq!(with_key_only.status, DeviceStatus::Pending);

    // A made-up token, the right token with another key, and another
    // account's token all get the same.
    let cases = [
        ("alice", vec![7; 32], "not-a-token".to_owned()),
        ("alice", vec![8; 32], laptop.token.clone()),
        ("alice", vec![7; 32], mallory.token.clone()),
    ];
    for (user, public_key, previous) in cases {
        // Keep under the limit on waiting devices.
        sqlx::query("DELETE FROM devices WHERE status = 'pending'")
            .execute(&pool)
            .await
            .unwrap();
        let answer = sign_in_again(&app, user, "Laptop", public_key, &previous).await;
        assert_ne!(answer.device_id, laptop.device_id);
        assert_ne!(answer.device_id, mallory.device_id);
        assert_eq!(answer.status, DeviceStatus::Pending);
    }

    // Mallory's own token cannot move Alice's laptop either way round:
    // signing in as Mallory with Alice's token resumes nothing of Alice's.
    let answer = sign_in_again(&app, "mallory", "Laptop", vec![7; 32], &laptop.token).await;
    assert_eq!(answer.account_id, mallory.account_id);
    assert_ne!(answer.device_id, laptop.device_id);
    assert_eq!(answer.status, DeviceStatus::Pending);

    // And the laptop's old token still resumes the laptop afterwards.
    let again = sign_in_again(&app, "alice", "Laptop", vec![7; 32], &laptop.token).await;
    assert_eq!(again.device_id, laptop.device_id);
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_revoked_device_signs_in_again_as_a_new_device(pool: PgPool) {
    let app = app(pool.clone());
    let laptop = register(&app, "alice", "Laptop").await;
    let phone = register(&app, "alice", "Phone").await;
    sqlx::query("UPDATE devices SET status = 'approved' WHERE id = $1")
        .bind(phone.device_id)
        .execute(&pool)
        .await
        .unwrap();
    let path = format!("/v1/devices/{}", phone.device_id);
    let revoked = call(&app, Method::DELETE, &path, Some(&laptop.token), None).await;
    assert_eq!(revoked.status, StatusCode::OK);

    let again = sign_in_again(&app, "alice", "Phone", vec![7; 32], &phone.token).await;

    assert_ne!(again.device_id, phone.device_id);
    assert_eq!(again.status, DeviceStatus::Pending);
    let old = get(&app, "/v1/devices/me", Some(&phone.token)).await;
    assert_eq!(old.code(), "device_revoked");
}

// ---- The request limit ----

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn an_account_over_its_request_limit_is_told_to_wait(pool: PgPool) {
    let app = app(pool.clone());
    let alice = register(&app, "alice", "Laptop").await;
    let bob = register(&app, "bob", "Laptop").await;

    // One request opens the window; then the count is moved to the limit.
    assert_eq!(
        get(&app, "/v1/devices/me", Some(&alice.token)).await.status,
        StatusCode::OK
    );
    sqlx::query("UPDATE request_counts SET requests = $1 WHERE account_id = $2")
        .bind(limits::REQUESTS_PER_WINDOW)
        .bind(alice.account_id)
        .execute(&pool)
        .await
        .unwrap();

    let answer = get(&app, "/v1/devices/me", Some(&alice.token)).await;
    assert_eq!(answer.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(answer.code(), "rate_limited");
    let wait = answer.body["retry_after_secs"].as_i64().unwrap();
    assert!((1..=limits::REQUEST_WINDOW_SECS).contains(&wait), "{wait}");

    // The limit is per account.
    assert_eq!(
        get(&app, "/v1/devices/me", Some(&bob.token)).await.status,
        StatusCode::OK
    );
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn requests_are_counted_per_account(pool: PgPool) {
    let app = app(pool.clone());
    let alice = register(&app, "alice", "Laptop").await;
    for _ in 0..5 {
        get(&app, "/v1/devices/me", Some(&alice.token)).await;
    }
    let (requests,): (i64,) =
        sqlx::query_as("SELECT sum(requests)::bigint FROM request_counts WHERE account_id = $1")
            .bind(alice.account_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(requests, 5);
}
