//! Purging deleted notes, with whole devices against the real server.
//!
//! A deleted note keeps its document so that the delete can sync, and so
//! that an edit made on another device without knowing of the delete can
//! bring the note back. After a wait, a device that has seen the note
//! deleted all that time asks the server to purge it. These tests are about
//! what must never happen then: a thought being lost.
//!
//! Start the database with `docker compose up -d` at the repository root and
//! set `DATABASE_URL` as `compose.yaml` shows.

use std::time::Duration;

use dirt_core::api::BoxFuture;
use dirt_core::session::SessionState;
use dirt_proto::NoteId;
use dirt_sim::{Client, Server};
use sqlx::PgPool;

/// Alice's laptop and phone, both in the account.
async fn two_devices(server: &Server) -> (Client, Client) {
    let laptop = Client::new(server, "Laptop", false);
    laptop.sign_in("alice").await.unwrap();
    let phone = Client::new(server, "Phone", false);
    let state = phone.sign_in("alice").await.unwrap();
    assert!(matches!(state, SessionState::WaitingForApproval { .. }));
    phone.approve_with_key(server, &laptop.account_key()).await;
    (laptop, phone)
}

/// The same app, started again as one that purges at once. The apps wait
/// thirty days; nothing else about them differs.
fn impatient(client: Client) -> Client {
    client.restarted(|config| config.timing.purge_after = Duration::ZERO)
}

async fn write(client: &Client, text: &str) -> NoteId {
    client.session.store().create_note(text).await.unwrap().id
}

/// The live notes on a device: `(id, text)`, by text.
async fn live(client: &Client) -> Vec<(String, String)> {
    let mut notes = client.notes().await;
    notes.sort_by(|a, b| a.1.cmp(&b.1));
    notes
}

async fn texts(client: &Client) -> Vec<String> {
    live(client)
        .await
        .into_iter()
        .map(|(_, text)| text)
        .collect()
}

/// Every note a device holds, deleted ones included.
async fn rows(client: &Client) -> usize {
    client.session.store().note_documents().await.unwrap().len()
}

/// What the server holds for the account: notes with a blob, markers of
/// purged notes, and what it counts.
async fn on_the_server(pool: &PgPool) -> (i64, i64, i64, i64) {
    sqlx::query_as(
        "SELECT (SELECT count(*) FROM notes WHERE ciphertext IS NOT NULL), \
                (SELECT count(*) FROM notes WHERE ciphertext IS NULL), \
                note_count, purged_count FROM accounts",
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_deleted_note_is_purged_after_the_wait_and_every_device_lets_it_go(pool: PgPool) {
    let server = Server::with_sign_in(pool.clone()).await;
    let (laptop, phone) = two_devices(&server).await;
    write(&laptop, "kept").await;
    let thrown_away = write(&laptop, "thrown away").await;
    laptop.session.sync_now().await.unwrap();
    phone.session.sync_now().await.unwrap();

    laptop
        .session
        .store()
        .delete_note(&thrown_away)
        .await
        .unwrap();
    laptop.session.sync_now().await.unwrap();
    phone.session.sync_now().await.unwrap();
    assert_eq!(texts(&phone).await, ["kept"]);

    // Until the wait is over nothing is purged, however often they sync.
    // Both devices still hold the deleted note's document.
    for device in [&laptop, &phone] {
        assert_eq!(device.session.sync_now().await.unwrap().purged, 0);
        assert_eq!(rows(device).await, 2);
    }
    assert_eq!(on_the_server(&pool).await, (2, 0, 2, 0));

    // The wait is over on the laptop.
    let laptop = impatient(laptop);
    let report = laptop.session.sync_now().await.unwrap();

    assert_eq!(report.purged, 1);
    assert_eq!(on_the_server(&pool).await, (1, 1, 1, 1));
    assert_eq!(rows(&laptop).await, 1);
    assert_eq!(texts(&laptop).await, ["kept"]);

    // The phone learns of it by syncing, and lets its copy go too.
    phone.session.sync_now().await.unwrap();
    assert_eq!(rows(&phone).await, 1);
    assert_eq!(texts(&phone).await, ["kept"]);

    // Nothing more happens, on either, and nothing is left to send.
    for device in [&laptop, &phone] {
        let report = device.session.sync_now().await.unwrap();
        assert_eq!((report.purged, report.pushed, report.unsent), (0, 0, 0));
        assert!(device
            .session
            .store()
            .note_problems()
            .await
            .unwrap()
            .is_empty());
    }
    assert_eq!(on_the_server(&pool).await, (1, 1, 1, 1));
}

/// The phone edits a note while it is away. Meanwhile the laptop deletes
/// the note and, in time, purges it. The delete never covered the phone's
/// edit, and with the note purged there is nothing left to merge it into.
/// The edit is kept, as a new note, on every device.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn an_edit_made_without_knowing_of_the_delete_is_kept_as_a_new_note(pool: PgPool) {
    let server = Server::with_sign_in(pool.clone()).await;
    let (laptop, phone) = two_devices(&server).await;
    let thought = write(&laptop, "a thought").await;
    laptop.session.sync_now().await.unwrap();
    phone.session.sync_now().await.unwrap();

    phone.transport.set_offline(true);
    phone
        .session
        .store()
        .update_note(&thought, "a thought, and what came of it")
        .await
        .unwrap();

    laptop.session.store().delete_note(&thought).await.unwrap();
    laptop.session.sync_now().await.unwrap();
    let laptop = impatient(laptop);
    assert_eq!(laptop.session.sync_now().await.unwrap().purged, 1);
    assert_eq!(on_the_server(&pool).await, (0, 1, 0, 1));

    phone.transport.set_offline(false);
    phone.session.sync_now().await.unwrap();

    let on_phone = live(&phone).await;
    assert_eq!(on_phone.len(), 1);
    assert_eq!(on_phone[0].1, "a thought, and what came of it");
    assert_ne!(on_phone[0].0, thought.to_string(), "it is a new note");
    assert_eq!(rows(&phone).await, 1);
    assert_eq!(phone.session.sync_now().await.unwrap().unsent, 0);

    laptop.session.sync_now().await.unwrap();
    assert_eq!(live(&laptop).await, on_phone);
    assert_eq!(on_the_server(&pool).await, (1, 1, 1, 1));
}

/// The phone was away through the delete and the purge, holding the note
/// as it was and with nothing of its own to add. What it holds was all in
/// the version that was deleted, so its copy goes and no note appears.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_device_that_never_saw_the_delete_and_changed_nothing_lets_the_note_go(pool: PgPool) {
    let server = Server::with_sign_in(pool.clone()).await;
    let (laptop, phone) = two_devices(&server).await;
    let thought = write(&laptop, "a thought").await;
    write(&laptop, "another").await;
    laptop.session.sync_now().await.unwrap();
    phone.session.sync_now().await.unwrap();
    phone.transport.set_offline(true);

    laptop.session.store().delete_note(&thought).await.unwrap();
    laptop.session.sync_now().await.unwrap();
    let laptop = impatient(laptop);
    assert_eq!(laptop.session.sync_now().await.unwrap().purged, 1);

    phone.transport.set_offline(false);
    assert_eq!(texts(&phone).await, ["a thought", "another"]);
    phone.session.sync_now().await.unwrap();

    assert_eq!(texts(&phone).await, ["another"]);
    assert_eq!(rows(&phone).await, 1);
    assert_eq!(phone.session.sync_now().await.unwrap().unsent, 0);
    assert_eq!(on_the_server(&pool).await, (1, 1, 1, 1));
}

/// Both devices delete the note, the phone while it is away. Its unsent
/// change is the delete itself, so there is nothing to keep.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_note_deleted_on_both_devices_comes_back_on_neither(pool: PgPool) {
    let server = Server::with_sign_in(pool.clone()).await;
    let (laptop, phone) = two_devices(&server).await;
    let thought = write(&laptop, "a thought").await;
    laptop.session.sync_now().await.unwrap();
    phone.session.sync_now().await.unwrap();

    phone.transport.set_offline(true);
    phone.session.store().delete_note(&thought).await.unwrap();
    laptop.session.store().delete_note(&thought).await.unwrap();
    laptop.session.sync_now().await.unwrap();
    let laptop = impatient(laptop);
    assert_eq!(laptop.session.sync_now().await.unwrap().purged, 1);

    phone.transport.set_offline(false);
    let report = phone.session.sync_now().await.unwrap();

    assert_eq!(report.unsent, 0);
    assert_eq!(rows(&phone).await, 0);
    laptop.session.sync_now().await.unwrap();
    assert_eq!(rows(&laptop).await, 0);
    assert_eq!(on_the_server(&pool).await, (0, 1, 0, 1));
}

/// The phone's edit reaches the server in the moment between the laptop
/// deciding to purge and the server hearing of it. The server holds a
/// newer version than the laptop named, and purges nothing. The note is
/// live again, with the edit, on both.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn an_edit_that_arrives_while_the_purge_is_asked_for_saves_the_note(pool: PgPool) {
    let server = Server::with_sign_in(pool.clone()).await;
    let (laptop, phone) = two_devices(&server).await;
    let thought = write(&laptop, "a thought").await;
    laptop.session.sync_now().await.unwrap();
    phone.session.sync_now().await.unwrap();

    phone.transport.set_offline(true);
    phone
        .session
        .store()
        .update_note(&thought, "a thought, and what came of it")
        .await
        .unwrap();
    laptop.session.store().delete_note(&thought).await.unwrap();
    laptop.session.sync_now().await.unwrap();
    let laptop = impatient(laptop);

    phone.transport.set_offline(false);
    let phones = phone.session.clone();
    laptop
        .transport
        .before_next("POST /v1/notes/purge", move || -> BoxFuture<'static, ()> {
            Box::pin(async move {
                phones.sync_now().await.unwrap();
            })
        });
    let report = laptop.session.sync_now().await.unwrap();

    assert_eq!(report.purged, 0);
    assert_eq!(on_the_server(&pool).await, (1, 0, 1, 0));
    // The laptop still holds the note, deleted as far as it knows, until
    // its next sync brings the edit.
    assert_eq!(rows(&laptop).await, 1);
    let report = laptop.session.sync_now().await.unwrap();
    assert_eq!(report.purged, 0);
    let expected = vec![(
        thought.to_string(),
        "a thought, and what came of it".to_owned(),
    )];
    assert_eq!(live(&laptop).await, expected);
    assert_eq!(live(&phone).await, expected);
    assert_eq!(on_the_server(&pool).await, (1, 0, 1, 0));
}

/// A device that is told of the purge by a push, not a pull: it sends its
/// edit before it has pulled, and the server hands back the marker.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn an_edit_pushed_onto_a_purged_note_is_kept_as_a_new_note(pool: PgPool) {
    let server = Server::with_sign_in(pool.clone()).await;
    let (laptop, phone) = two_devices(&server).await;
    let thought = write(&laptop, "a thought").await;
    laptop.session.sync_now().await.unwrap();
    phone.session.sync_now().await.unwrap();
    laptop.session.store().delete_note(&thought).await.unwrap();
    laptop.session.sync_now().await.unwrap();

    // The phone has pulled, and is about to push an edit, when the laptop
    // purges the note.
    phone
        .session
        .store()
        .update_note(&thought, "a thought, and what came of it")
        .await
        .unwrap();
    let laptop = impatient(laptop);
    let laptops = laptop.session.clone();
    phone
        .transport
        .before_next("POST /v1/notes/push", move || -> BoxFuture<'static, ()> {
            Box::pin(async move {
                assert_eq!(laptops.sync_now().await.unwrap().purged, 1);
            })
        });
    // The phone had not seen the delete either: it pulls it in this very
    // cycle, merges, finds its edit uncovered, and pushes.
    let report = phone.session.sync_now().await.unwrap();

    assert_eq!(report.unsent, 0);
    let on_phone = live(&phone).await;
    assert_eq!(on_phone.len(), 1);
    assert_eq!(on_phone[0].1, "a thought, and what came of it");
    assert_ne!(on_phone[0].0, thought.to_string());
    assert_eq!(rows(&phone).await, 1);
    laptop.session.sync_now().await.unwrap();
    assert_eq!(live(&laptop).await, on_phone);
    assert_eq!(on_the_server(&pool).await, (1, 1, 1, 1));
}
