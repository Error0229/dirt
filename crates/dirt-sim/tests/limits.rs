//! Measurements behind the limits review (`docs/LIMITS.md`).
//!
//! These are not checks that pass or fail on a number. They print what a
//! note and an account of a given size cost, so that the limits in
//! `dirt_proto::api::limits` can be set against something measured. They
//! take minutes, so they are ignored by default:
//!
//! ```text
//! cargo test -p dirt-sim --test limits -- --ignored --nocapture --test-threads 1
//! ```
//!
//! Start the database with `docker compose up -d` at the repository root and
//! set `DATABASE_URL` as `compose.yaml` shows.

use std::time::Instant;

use dirt_core::notes::NoteDoc;
use dirt_core::session::SessionState;
use dirt_proto::api::limits;
use dirt_sim::{Client, Server};
use sqlx::PgPool;

/// What encryption adds to a document: a layout byte, the nonce and the tag.
const BLOB_OVERHEAD: usize = 1 + 24 + 16;

fn document_size(text: &str, edits: usize) -> usize {
    let document = NoteDoc::new(1);
    document.set_text(text, 1_000);
    // Each edit appends a word, as typing a little more would.
    let mut grown = text.to_owned();
    for edit in 0..edits {
        grown.push_str(" more");
        document.set_text(&grown, 1_000 + i64::try_from(edit).unwrap_or(0));
    }
    document.encode().len()
}

#[test]
#[ignore = "a measurement, not a check"]
fn the_size_of_a_note_on_the_server() {
    println!("\ncharacters of text | edits | document bytes | blob bytes");
    for (length, edits) in [
        (0, 0),
        (80, 0),
        (280, 0),
        (280, 20),
        (2_000, 0),
        (2_000, 200),
        (20_000, 0),
        (200_000, 0),
        (1_000_000, 0),
    ] {
        let text = "x".repeat(length);
        let size = document_size(&text, edits);
        println!(
            "{length:>18} | {edits:>5} | {size:>14} | {:>10}",
            size + BLOB_OVERHEAD
        );
    }
    let per_account = usize::try_from(limits::MAX_ACCOUNT_BYTES).unwrap_or(usize::MAX);
    let ordinary = document_size(&"x".repeat(280), 20) + BLOB_OVERHEAD;
    println!(
        "\nan account's {} bytes hold {} notes of 280 characters edited 20 times",
        per_account,
        per_account / ordinary
    );
    println!(
        "the note limit of {} is reached first by notes under {} bytes",
        limits::MAX_NOTES_PER_ACCOUNT,
        per_account / usize::try_from(limits::MAX_NOTES_PER_ACCOUNT).unwrap_or(1)
    );
}

async fn second_device(server: &Server, first: &Client, name: &str) -> Client {
    let device = Client::new(server, name, false);
    let state = device.sign_in("alice").await.unwrap();
    assert!(matches!(state, SessionState::WaitingForApproval { .. }));
    device.approve_with_key(server, &first.account_key()).await;
    device
}

/// An account holding as many notes as it may: what it costs to send it,
/// to fetch it on a new device, and to change its key.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
#[ignore = "a measurement, not a check"]
async fn an_account_at_the_note_limit(pool: PgPool) {
    let notes = usize::try_from(limits::MAX_NOTES_PER_ACCOUNT).unwrap();
    let server = Server::with_sign_in(pool.clone()).await;
    let laptop = Client::new(&server, "Laptop", false);
    laptop.sign_in("alice").await.unwrap();

    let began = Instant::now();
    let store = laptop.session.store();
    for number in 0..notes {
        store
            .create_note(&format!(
                "note {number}: something worth remembering, about a line long #tag{}",
                number % 50
            ))
            .await
            .unwrap();
    }
    println!("\nwriting {notes} notes locally: {:.1?}", began.elapsed());

    let (began, before) = (Instant::now(), laptop.transport.requests());
    let report = laptop.session.sync_now().await.unwrap();
    println!(
        "first send: {} notes in {} requests, {:.1?}",
        report.pushed,
        laptop.transport.requests() - before,
        began.elapsed()
    );
    let (bytes_used,): (i64,) = sqlx::query_as("SELECT bytes_used FROM accounts")
        .fetch_one(&pool)
        .await
        .unwrap();
    let tenths_of_a_percent = bytes_used * 1000 / limits::MAX_ACCOUNT_BYTES;
    println!(
        "the server holds {bytes_used} bytes for it, {}.{}% of the byte limit",
        tenths_of_a_percent / 10,
        tenths_of_a_percent % 10
    );

    // What the database itself uses for those rows, with its indexes and
    // its own overhead: this is what a storage allowance is spent on.
    let (on_disk,): (i64,) = sqlx::query_as("SELECT pg_total_relation_size('notes')")
        .fetch_one(&pool)
        .await
        .unwrap();
    println!("the database uses {on_disk} bytes for the notes table and its indexes");

    let phone = second_device(&server, &laptop, "Phone").await;
    let (began, before) = (Instant::now(), phone.transport.requests());
    let report = phone.session.sync_now().await.unwrap();
    println!(
        "a new device fetches it: {} notes in {} requests, {:.1?}",
        report.pulled,
        phone.transport.requests() - before,
        began.elapsed()
    );

    let (began, before) = (Instant::now(), laptop.transport.requests());
    laptop.session.change_account_key().await.unwrap();
    println!(
        "changing the key and sending every note again: {} requests, {:.1?}",
        laptop.transport.requests() - before,
        began.elapsed()
    );
    let (began, before) = (Instant::now(), phone.transport.requests());
    let report = phone.session.sync_now().await.unwrap();
    println!(
        "the other device follows the change: {} notes in {} requests, {:.1?}",
        report.pulled,
        phone.transport.requests() - before,
        began.elapsed()
    );
    println!(
        "requests allowed per account: {} in {} seconds",
        limits::REQUESTS_PER_WINDOW,
        limits::REQUEST_WINDOW_SECS
    );

    // What the apps ask the store for. Each is asked twice and the second
    // answer timed, so that reading the file into memory is not counted.
    println!("\nthe store, holding {notes} notes:");
    for size in [100, 1_000, 5_000, notes] {
        store.list_notes(size, 0).await.unwrap();
        let began = Instant::now();
        let listed = store.list_notes(size, 0).await.unwrap();
        println!(
            "listing the newest {}: {:.1?}",
            listed.len(),
            began.elapsed()
        );
    }
    let offset = notes - 100;
    store.list_notes(100, offset).await.unwrap();
    let began = Instant::now();
    let listed = store.list_notes(100, offset).await.unwrap();
    println!(
        "listing the oldest {}, after skipping {offset}: {:.1?}",
        listed.len(),
        began.elapsed()
    );
    for (what, query, tag) in [
        ("a word every note holds", "remembering", None),
        ("a word and a tag", "remembering", Some("tag7")),
        ("a tag", "", Some("tag7")),
        ("a word one note holds", "19999", None),
        ("the start of a word", "rem", None),
    ] {
        store.find_notes(query, tag, 100).await.unwrap();
        let began = Instant::now();
        let found = store.find_notes(query, tag, 100).await.unwrap();
        println!(
            "finding by {what}: {} found in {:.1?}",
            found.len(),
            began.elapsed()
        );
    }
    let began = Instant::now();
    let tags = store.list_tags().await.unwrap();
    println!(
        "listing the {} tags in use: {:.1?}",
        tags.len(),
        began.elapsed()
    );
    let began = Instant::now();
    let count = store.note_count().await.unwrap();
    println!("counting the {count} notes: {:.1?}", began.elapsed());
}
