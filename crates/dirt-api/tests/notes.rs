//! Push, pull, the version check and the quotas, against Postgres.
//!
//! Start the database with `docker compose up -d` at the repository root and
//! set `DATABASE_URL` as `compose.yaml` shows. Each test runs in its own
//! throwaway database.

mod common;

use std::collections::BTreeSet;

use axum::http::StatusCode;
use common::{app, get, item, note_id, post, pull, push, push_one, register, try_push, usage};
use dirt_proto::api::{limits, ErrorCode, NoteBlob, PushOutcome, EPOCH_HEADER};
use serde_json::json;
use sqlx::PgPool;

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

// ---- Push and pull ----

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_pushed_note_comes_back_from_a_pull(pool: PgPool) {
    let app = app(pool);
    let device = register(&app, "alice", "Laptop").await;
    let note = note_id();

    let outcome = push_one(&app, &device.token, note, 0, b"ciphertext one").await;
    assert_eq!(outcome, PushOutcome::Accepted { change: 1 });

    let answer = get(&app, "/v1/notes/pull?after=0", Some(&device.token)).await;
    assert_eq!(answer.headers[EPOCH_HEADER], "1");
    let page = pull(&app, &device.token, 0).await;
    assert_eq!(page.epoch, 1);
    assert_eq!(
        page.items,
        vec![NoteBlob {
            note_id: note,
            change: 1,
            key_version: 1,
            format_version: 1,
            ciphertext: Some(b"ciphertext one".to_vec()),
        }]
    );
    assert_eq!(page.next_after, 1);
    assert!(!page.more);

    // Nothing new after the cursor.
    let empty = pull(&app, &device.token, 1).await;
    assert!(empty.items.is_empty());
    assert_eq!(empty.next_after, 1);
    assert!(!empty.more);
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_push_on_the_current_version_replaces_the_note(pool: PgPool) {
    let app = app(pool.clone());
    let device = register(&app, "alice", "Laptop").await;
    let note = note_id();
    push_one(&app, &device.token, note, 0, b"v1").await;

    let outcome = push_one(&app, &device.token, note, 1, b"version two").await;
    assert_eq!(outcome, PushOutcome::Accepted { change: 2 });

    let page = pull(&app, &device.token, 0).await;
    assert_eq!(
        page.items.len(),
        1,
        "a note is one row however often it changes"
    );
    assert_eq!(page.items[0].change, 2);
    assert_eq!(
        page.items[0].ciphertext.as_deref(),
        Some(&b"version two"[..])
    );
    assert_eq!(usage(&pool, device.account_id).await, (2, 1, 11));
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_push_on_an_old_version_is_stale_and_returns_what_the_server_has(pool: PgPool) {
    let app = app(pool.clone());
    let device = register(&app, "alice", "Laptop").await;
    let note = note_id();
    push_one(&app, &device.token, note, 0, b"v1").await;
    push_one(&app, &device.token, note, 1, b"v2").await;
    let current = NoteBlob {
        note_id: note,
        change: 2,
        key_version: 1,
        format_version: 1,
        ciphertext: Some(b"v2".to_vec()),
    };

    // Based on version 1, which has been replaced.
    let late = push_one(&app, &device.token, note, 1, b"late").await;
    assert_eq!(
        late,
        PushOutcome::Stale {
            current: Some(current.clone())
        }
    );
    // Claiming the note is new when it exists.
    let again = push_one(&app, &device.token, note, 0, b"again").await;
    assert_eq!(
        again,
        PushOutcome::Stale {
            current: Some(current)
        }
    );
    // Naming a version of a note the server has never had.
    let orphan = push_one(&app, &device.token, note_id(), 7, b"orphan").await;
    assert_eq!(orphan, PushOutcome::Stale { current: None });

    // None of the refused pushes changed anything or used a change number.
    assert_eq!(usage(&pool, device.account_id).await, (2, 1, 2));
    assert_eq!(
        pull(&app, &device.token, 0).await.items[0]
            .ciphertext
            .as_deref(),
        Some(&b"v2"[..])
    );
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_batch_reports_each_note_separately_and_in_order(pool: PgPool) {
    let app = app(pool);
    let device = register(&app, "alice", "Laptop").await;
    let (first, second) = (note_id(), note_id());
    push_one(&app, &device.token, first, 0, b"exists").await;

    let outcomes = push(
        &app,
        &device.token,
        vec![
            item(second, 0, b"new".to_vec()),
            item(first, 0, b"stale".to_vec()),
            item(first, 1, b"good".to_vec()),
            item(note_id(), 0, vec![0; limits::MAX_NOTE_BYTES + 1]),
        ],
    )
    .await;

    assert_eq!(outcomes[0], PushOutcome::Accepted { change: 2 });
    assert!(matches!(outcomes[1], PushOutcome::Stale { .. }));
    assert_eq!(outcomes[2], PushOutcome::Accepted { change: 3 });
    assert_eq!(rejection(&outcomes[3]), Some(ErrorCode::NoteTooLarge));
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn pull_pages_by_size_and_always_makes_progress(pool: PgPool) {
    let app = app(pool);
    let device = register(&app, "alice", "Laptop").await;
    // Five notes of the largest size. Three fit under the page limit: the
    // fourth starts past it.
    for _ in 0..5 {
        let outcome = push_one(
            &app,
            &device.token,
            note_id(),
            0,
            &vec![9; limits::MAX_NOTE_BYTES],
        )
        .await;
        assert!(accepted(&outcome).is_some());
    }

    let first = pull(&app, &device.token, 0).await;
    let changes: Vec<i64> = first.items.iter().map(|note| note.change).collect();
    assert_eq!(changes, vec![1, 2, 3]);
    assert_eq!(first.next_after, 3);
    assert!(first.more);

    let second = pull(&app, &device.token, first.next_after).await;
    let changes: Vec<i64> = second.items.iter().map(|note| note.change).collect();
    assert_eq!(changes, vec![4, 5]);
    assert_eq!(second.next_after, 5);
    assert!(!second.more);
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_purged_note_is_pulled_with_no_blob(pool: PgPool) {
    let app = app(pool.clone());
    let device = register(&app, "alice", "Laptop").await;
    push_one(&app, &device.token, note_id(), 0, b"soon purged").await;
    sqlx::query("UPDATE notes SET ciphertext = NULL")
        .execute(&pool)
        .await
        .unwrap();

    let page = pull(&app, &device.token, 0).await;
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].ciphertext, None);
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn malformed_requests_are_refused_in_the_protocol_format(pool: PgPool) {
    let app = app(pool);
    let device = register(&app, "alice", "Laptop").await;
    let token = Some(device.token.as_str());

    let not_a_push = post(&app, "/v1/notes/push", token, json!({"items": "nope"})).await;
    assert_eq!(not_a_push.status, StatusCode::BAD_REQUEST);
    assert_eq!(not_a_push.code(), "invalid_request");

    let negative_base = try_push(&app, &device.token, vec![item(note_id(), -1, vec![1])]).await;
    assert_eq!(negative_base.code(), "invalid_request");

    for path in ["/v1/notes/pull?after=-1", "/v1/notes/pull?after=abc"] {
        let answer = get(&app, path, token).await;
        assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{path}");
        assert_eq!(answer.code(), "invalid_request");
    }
}

// ---- The version check under concurrency ----

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn of_fifty_pushes_on_one_version_exactly_one_is_accepted(pool: PgPool) {
    let app = app(pool.clone());
    let device = register(&app, "alice", "Laptop").await;
    let note = note_id();
    push_one(&app, &device.token, note, 0, b"v1").await;

    let mut tasks = Vec::new();
    for writer in 0..50 {
        let (app, token) = (app.clone(), device.token.clone());
        tasks.push(tokio::spawn(async move {
            push_one(&app, &token, note, 1, format!("writer {writer}").as_bytes()).await
        }));
    }
    let mut outcomes = Vec::new();
    for task in tasks {
        outcomes.push(task.await.unwrap());
    }

    let winners: Vec<i64> = outcomes.iter().filter_map(accepted).collect();
    assert_eq!(winners, vec![2]);
    let stale = outcomes
        .iter()
        .filter(|outcome| matches!(outcome, PushOutcome::Stale { .. }))
        .count();
    assert_eq!(stale, 49);
    assert_eq!(usage(&pool, device.account_id).await.0, 2);
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn fifty_creations_of_one_note_accept_exactly_one(pool: PgPool) {
    let app = app(pool);
    let device = register(&app, "alice", "Laptop").await;
    let note = note_id();

    let mut tasks = Vec::new();
    for writer in 0..50 {
        let (app, token) = (app.clone(), device.token.clone());
        tasks.push(tokio::spawn(async move {
            push_one(&app, &token, note, 0, format!("writer {writer}").as_bytes()).await
        }));
    }
    let mut winners = 0;
    for task in tasks {
        winners += usize::from(accepted(&task.await.unwrap()).is_some());
    }
    assert_eq!(winners, 1);
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn pushes_to_different_notes_take_consecutive_change_numbers(pool: PgPool) {
    let app = app(pool.clone());
    let device = register(&app, "alice", "Laptop").await;

    let mut tasks = Vec::new();
    for index in 0..200 {
        let (app, token) = (app.clone(), device.token.clone());
        tasks.push(tokio::spawn(async move {
            push_one(
                &app,
                &token,
                note_id(),
                0,
                format!("note {index}").as_bytes(),
            )
            .await
        }));
    }
    let mut changes = BTreeSet::new();
    for task in tasks {
        changes.insert(accepted(&task.await.unwrap()).expect("every push is accepted"));
    }
    assert_eq!(changes, (1..=200).collect::<BTreeSet<i64>>());

    // Pulled in pages, they come back in order with none missing.
    let page = pull(&app, &device.token, 0).await;
    let pulled: Vec<i64> = page.items.iter().map(|note| note.change).collect();
    assert_eq!(pulled, (1..=200).collect::<Vec<i64>>());
    assert_eq!(usage(&pool, device.account_id).await.1, 200);
}

// ---- Account isolation ----

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn one_account_never_sees_or_touches_anothers_notes(pool: PgPool) {
    let app = app(pool.clone());
    let alice = register(&app, "alice", "Laptop").await;
    let bob = register(&app, "bob", "Laptop").await;
    let note = note_id();
    push_one(&app, &alice.token, note, 0, b"alice's secret").await;

    assert!(pull(&app, &bob.token, 0).await.items.is_empty());

    // Bob uses the same note ID. He gets a note of his own: for him the ID
    // is new, so a base of 1 is stale and a base of 0 creates it.
    assert_eq!(
        push_one(&app, &bob.token, note, 1, b"overwrite?").await,
        PushOutcome::Stale { current: None }
    );
    assert_eq!(
        push_one(&app, &bob.token, note, 0, b"bob's own").await,
        PushOutcome::Accepted { change: 1 }
    );

    let alices = pull(&app, &alice.token, 0).await;
    assert_eq!(alices.items.len(), 1);
    assert_eq!(
        alices.items[0].ciphertext.as_deref(),
        Some(&b"alice's secret"[..])
    );
    let bobs = pull(&app, &bob.token, 0).await;
    assert_eq!(bobs.items[0].ciphertext.as_deref(), Some(&b"bob's own"[..]));

    // Each account counts its own changes and its own usage.
    assert_eq!(usage(&pool, alice.account_id).await, (1, 1, 14));
    assert_eq!(usage(&pool, bob.account_id).await, (1, 1, 9));
}

// ---- Quotas ----

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_note_over_the_size_limit_is_refused_alone(pool: PgPool) {
    let app = app(pool.clone());
    let device = register(&app, "alice", "Laptop").await;

    let at_limit = push_one(
        &app,
        &device.token,
        note_id(),
        0,
        &vec![1; limits::MAX_NOTE_BYTES],
    )
    .await;
    assert_eq!(at_limit, PushOutcome::Accepted { change: 1 });

    let over = push_one(
        &app,
        &device.token,
        note_id(),
        0,
        &vec![1; limits::MAX_NOTE_BYTES + 1],
    )
    .await;
    assert_eq!(rejection(&over), Some(ErrorCode::NoteTooLarge));
    assert_eq!(
        usage(&pool, device.account_id).await.0,
        1,
        "no change number used"
    );
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn an_account_at_its_note_limit_can_change_notes_but_not_add_them(pool: PgPool) {
    let app = app(pool.clone());
    let device = register(&app, "alice", "Laptop").await;
    let existing = note_id();
    push_one(&app, &device.token, existing, 0, b"v1").await;
    sqlx::query("UPDATE accounts SET note_count = $1")
        .bind(limits::MAX_NOTES_PER_ACCOUNT)
        .execute(&pool)
        .await
        .unwrap();

    let new = push_one(&app, &device.token, note_id(), 0, b"one more").await;
    assert_eq!(rejection(&new), Some(ErrorCode::QuotaExceeded));
    let update = push_one(&app, &device.token, existing, 1, b"v2").await;
    assert_eq!(update, PushOutcome::Accepted { change: 2 });
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_full_account_takes_small_growth_so_that_deletes_get_through(pool: PgPool) {
    let app = app(pool.clone());
    let device = register(&app, "alice", "Laptop").await;
    let note = note_id();
    push_one(&app, &device.token, note, 0, &[1; 5000]).await;
    let set_usage = |bytes: i64| {
        let pool = pool.clone();
        async move {
            sqlx::query("UPDATE accounts SET bytes_used = $1")
                .bind(bytes)
                .execute(&pool)
                .await
                .unwrap();
        }
    };
    set_usage(limits::MAX_ACCOUNT_BYTES).await;

    // Growing a note by more than the allowance: refused.
    let big = push_one(&app, &device.token, note, 1, &[1; 5000 + 1025]).await;
    assert_eq!(rejection(&big), Some(ErrorCode::QuotaExceeded));
    // A new note of any real size: refused.
    let new = push_one(&app, &device.token, note_id(), 0, &[1; 2000]).await;
    assert_eq!(rejection(&new), Some(ErrorCode::QuotaExceeded));
    // Growing by the allowance, as a delete marker does: accepted.
    let small = push_one(&app, &device.token, note, 1, &[1; 5000 + 1024]).await;
    assert_eq!(small, PushOutcome::Accepted { change: 2 });
    // Shrinking: accepted.
    let smaller = push_one(&app, &device.token, note, 2, &[1; 100]).await;
    assert_eq!(smaller, PushOutcome::Accepted { change: 3 });

    // At the hard ceiling even small growth is refused.
    let ceiling = limits::MAX_ACCOUNT_BYTES / 100 * limits::ACCOUNT_CEILING_PERCENT;
    set_usage(ceiling).await;
    let at_ceiling = push_one(&app, &device.token, note, 3, &[1; 101]).await;
    assert_eq!(rejection(&at_ceiling), Some(ErrorCode::QuotaExceeded));
    let shrink = push_one(&app, &device.token, note, 3, &[1; 50]).await;
    assert_eq!(shrink, PushOutcome::Accepted { change: 4 });
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn usage_counters_match_what_is_stored(pool: PgPool) {
    let app = app(pool.clone());
    let device = register(&app, "alice", "Laptop").await;
    let (a, b) = (note_id(), note_id());
    push_one(&app, &device.token, a, 0, &[0; 300]).await;
    push_one(&app, &device.token, b, 0, &[0; 700]).await;
    push_one(&app, &device.token, a, 1, &[0; 50]).await;
    push_one(&app, &device.token, b, 1, &[0; 9]).await; // stale: b is at 2

    let (stored_notes, stored_bytes): (i64, i64) = sqlx::query_as(
        "SELECT count(*), coalesce(sum(octet_length(ciphertext)), 0)::bigint FROM notes",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((stored_notes, stored_bytes), (2, 750));
    assert_eq!(usage(&pool, device.account_id).await, (3, 2, 750));
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_batch_over_the_limits_is_refused_whole(pool: PgPool) {
    let app = app(pool.clone());
    let device = register(&app, "alice", "Laptop").await;

    // Four notes of 800 KB: each is allowed, together they pass 3 MB.
    let heavy: Vec<_> = (0..4)
        .map(|_| item(note_id(), 0, vec![3; 800_000]))
        .collect();
    let answer = try_push(&app, &device.token, heavy).await;
    assert_eq!(answer.status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(answer.code(), "payload_too_large");

    let many: Vec<_> = (0..=limits::MAX_BATCH_ITEMS)
        .map(|_| item(note_id(), 0, vec![1]))
        .collect();
    let answer = try_push(&app, &device.token, many).await;
    assert_eq!(answer.code(), "payload_too_large");

    assert_eq!(
        usage(&pool, device.account_id).await,
        (0, 0, 0),
        "nothing was stored"
    );
}
