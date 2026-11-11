//! The push and pull queries against a local Postgres 18.
//!
//! Start the database with `docker compose up -d` in this folder, then run
//! `DATABASE_URL=postgres://postgres:trial@127.0.0.1:54329/postgres cargo test`.
//! `#[sqlx::test]` gives every test its own freshly migrated database.

use std::collections::BTreeSet;

use sqlx::PgPool;
use uuid::Uuid;
use vercel_neon_trial::{create_account, pull, push, PushOutcome};

async fn new_account(pool: &PgPool) -> Uuid {
    let account = Uuid::new_v4();
    create_account(pool, account).await.unwrap();
    account
}

async fn counter(pool: &PgPool, account: Uuid) -> i64 {
    sqlx::query_scalar("SELECT change_counter FROM accounts WHERE id = $1")
        .bind(account)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test]
async fn first_push_of_a_new_note_takes_change_number_one(pool: PgPool) {
    let account = new_account(&pool).await;
    let outcome = push(&pool, account, Uuid::new_v4(), 0, b"v1")
        .await
        .unwrap();
    assert_eq!(outcome, PushOutcome::Accepted(1));
    assert_eq!(counter(&pool, account).await, 1);
}

#[sqlx::test]
async fn push_on_the_current_base_replaces_the_note(pool: PgPool) {
    let account = new_account(&pool).await;
    let note = Uuid::new_v4();
    assert_eq!(
        push(&pool, account, note, 0, b"v1").await.unwrap(),
        PushOutcome::Accepted(1)
    );
    assert_eq!(
        push(&pool, account, note, 1, b"version two").await.unwrap(),
        PushOutcome::Accepted(2)
    );

    let stored: Vec<u8> = sqlx::query_scalar("SELECT ciphertext FROM notes WHERE note_id = $1")
        .bind(note)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(stored, b"version two");
}

#[sqlx::test]
async fn push_on_an_old_base_is_stale_and_uses_no_change_number(pool: PgPool) {
    let account = new_account(&pool).await;
    let note = Uuid::new_v4();
    push(&pool, account, note, 0, b"v1").await.unwrap();
    push(&pool, account, note, 1, b"v2").await.unwrap();

    let outcome = push(&pool, account, note, 1, b"late").await.unwrap();
    assert_eq!(outcome, PushOutcome::Stale);
    assert_eq!(counter(&pool, account).await, 2, "rejected push left a gap");

    let stored: Vec<u8> = sqlx::query_scalar("SELECT ciphertext FROM notes WHERE note_id = $1")
        .bind(note)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(stored, b"v2");
}

#[sqlx::test]
async fn creating_a_note_that_already_exists_is_stale(pool: PgPool) {
    let account = new_account(&pool).await;
    let note = Uuid::new_v4();
    push(&pool, account, note, 0, b"v1").await.unwrap();
    assert_eq!(
        push(&pool, account, note, 0, b"again").await.unwrap(),
        PushOutcome::Stale
    );
}

#[sqlx::test]
async fn a_base_for_a_note_the_server_lacks_is_stale(pool: PgPool) {
    let account = new_account(&pool).await;
    let outcome = push(&pool, account, Uuid::new_v4(), 7, b"orphan")
        .await
        .unwrap();
    assert_eq!(outcome, PushOutcome::Stale);
    assert_eq!(counter(&pool, account).await, 0);
}

#[sqlx::test]
async fn push_to_an_unknown_account_writes_nothing(pool: PgPool) {
    let outcome = push(&pool, Uuid::new_v4(), Uuid::new_v4(), 0, b"v1")
        .await
        .unwrap();
    assert_eq!(outcome, PushOutcome::NoAccount);
    let notes: i64 = sqlx::query_scalar("SELECT count(*) FROM notes")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(notes, 0);
}

#[sqlx::test]
async fn accounts_count_separately(pool: PgPool) {
    let first = new_account(&pool).await;
    let second = new_account(&pool).await;
    push(&pool, first, Uuid::new_v4(), 0, b"a").await.unwrap();
    push(&pool, first, Uuid::new_v4(), 0, b"b").await.unwrap();
    assert_eq!(
        push(&pool, second, Uuid::new_v4(), 0, b"c").await.unwrap(),
        PushOutcome::Accepted(1)
    );
}

/// Runs `count` pushes at once, each on its own task, and returns the
/// outcomes.
async fn push_concurrently(
    pool: &PgPool,
    count: usize,
    make: impl Fn(usize) -> (Uuid, Uuid, i64),
) -> Vec<PushOutcome> {
    let mut tasks = Vec::with_capacity(count);
    for index in 0..count {
        let pool = pool.clone();
        let (account, note, base) = make(index);
        tasks.push(tokio::spawn(async move {
            push(
                &pool,
                account,
                note,
                base,
                format!("writer {index}").as_bytes(),
            )
            .await
            .unwrap()
        }));
    }
    let mut outcomes = Vec::with_capacity(count);
    for task in tasks {
        outcomes.push(task.await.unwrap());
    }
    outcomes
}

fn accepted(outcomes: &[PushOutcome]) -> Vec<i64> {
    outcomes
        .iter()
        .filter_map(|outcome| match outcome {
            PushOutcome::Accepted(n) => Some(*n),
            _ => None,
        })
        .collect()
}

#[sqlx::test]
async fn fifty_pushes_on_one_base_accept_exactly_one(pool: PgPool) {
    let account = new_account(&pool).await;
    let note = Uuid::new_v4();
    push(&pool, account, note, 0, b"v1").await.unwrap();

    let outcomes = push_concurrently(&pool, 50, |_| (account, note, 1)).await;

    assert_eq!(accepted(&outcomes), vec![2]);
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| **outcome == PushOutcome::Stale)
            .count(),
        49
    );
    assert_eq!(counter(&pool, account).await, 2);
}

#[sqlx::test]
async fn fifty_creates_of_one_note_accept_exactly_one(pool: PgPool) {
    let account = new_account(&pool).await;
    let note = Uuid::new_v4();

    let outcomes = push_concurrently(&pool, 50, |_| (account, note, 0)).await;

    assert_eq!(accepted(&outcomes), vec![1]);
    assert_eq!(counter(&pool, account).await, 1);
}

#[sqlx::test]
async fn five_hundred_pushes_to_different_notes_number_without_gaps(pool: PgPool) {
    let account = new_account(&pool).await;
    let notes: Vec<Uuid> = (0..500).map(|_| Uuid::new_v4()).collect();

    let outcomes = push_concurrently(&pool, 500, |index| (account, notes[index], 0)).await;

    let numbers = accepted(&outcomes);
    assert_eq!(numbers.len(), 500, "every push should be accepted");
    let distinct: BTreeSet<i64> = numbers.iter().copied().collect();
    assert_eq!(distinct, (1..=500).collect::<BTreeSet<i64>>());
    assert_eq!(counter(&pool, account).await, 500);
}

#[sqlx::test]
async fn pull_pages_through_changes_in_order(pool: PgPool) {
    let account = new_account(&pool).await;
    let mut notes = Vec::new();
    for index in 0..5 {
        let note = Uuid::new_v4();
        push(&pool, account, note, 0, &vec![0u8; index + 1])
            .await
            .unwrap();
        notes.push(note);
    }
    // Replacing the first note moves it to the end of the change order.
    push(&pool, account, notes[0], 1, b"replaced")
        .await
        .unwrap();

    let first = pull(&pool, account, 0, 3).await.unwrap();
    assert_eq!(first.change_counter, Some(6));
    let numbers: Vec<i64> = first.notes.iter().map(|n| n.change_number).collect();
    assert_eq!(numbers, vec![2, 3, 4]);

    let second = pull(&pool, account, 4, 3).await.unwrap();
    let numbers: Vec<i64> = second.notes.iter().map(|n| n.change_number).collect();
    assert_eq!(numbers, vec![5, 6]);
    assert_eq!(second.notes[1].note_id, notes[0]);
    assert_eq!(second.notes[1].bytes, 8);

    let done = pull(&pool, account, 6, 3).await.unwrap();
    assert!(done.notes.is_empty());
}

#[sqlx::test]
async fn pull_for_an_unknown_account_reports_no_counter(pool: PgPool) {
    let page = pull(&pool, Uuid::new_v4(), 0, 10).await.unwrap();
    assert_eq!(page.change_counter, None);
    assert!(page.notes.is_empty());
}
