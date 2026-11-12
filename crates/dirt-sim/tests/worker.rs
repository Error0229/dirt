//! The sync worker: when an app that stays open syncs, and when it does not.
//!
//! These tests run the worker with pauses of tens of milliseconds in place
//! of seconds. The rules are the same.
//!
//! Start the database with `docker compose up -d` at the repository root and
//! set `DATABASE_URL` as `compose.yaml` shows.

use std::time::Duration;

use dirt_core::sync::worker::Trigger;
use dirt_proto::api::ErrorClass;
use dirt_sim::{eventually, Client, Server};
use sqlx::PgPool;

/// An app with the worker running, signed in as the first device of
/// `alice`'s account, after its start-up sync has finished.
async fn running_app(server: &Server) -> Client {
    let app = Client::new(server, "Laptop", true);
    app.sign_in("alice").await.unwrap();
    synced(&app).await;
    app
}

/// Waits until the app has completed a sync and has nothing to send.
async fn synced(app: &Client) {
    eventually("the app syncs", || async {
        let status = app.session.sync_status();
        !status.syncing && status.last_synced_ms.is_some() && status.unsent == 0
    })
    .await;
}

/// Waits long enough for anything the worker had scheduled to have run.
async fn quiet() {
    tokio::time::sleep(Duration::from_millis(700)).await;
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn the_app_syncs_when_it_starts_and_after_a_change(pool: PgPool) {
    let server = Server::with_sign_in(pool).await;
    let app = running_app(&server).await;
    assert!(app.session.sync_status().failure.is_none());

    app.session.store().create_note("one").await.unwrap();
    app.session.notify(Trigger::Changed);

    eventually("the note reaches the server", || async {
        server.blob_count().await == 1
    })
    .await;
    synced(&app).await;
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn an_app_that_is_open_and_untouched_does_not_talk_to_the_server(pool: PgPool) {
    let server = Server::with_sign_in(pool).await;
    let app = running_app(&server).await;
    quiet().await;
    let before = app.transport.requests();

    quiet().await;

    assert_eq!(app.transport.requests(), before, "there is no timer");
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_burst_of_changes_is_sent_together(pool: PgPool) {
    let server = Server::with_sign_in(pool).await;
    let app = running_app(&server).await;
    quiet().await;
    let before = app.transport.requests();

    for index in 0..5 {
        app.session
            .store()
            .create_note(&format!("note {index}"))
            .await
            .unwrap();
        app.session.notify(Trigger::Changed);
    }
    eventually("all five reach the server", || async {
        server.blob_count().await == 5
    })
    .await;
    quiet().await;

    // One cycle: a pull, one push carrying all five, and the pull that
    // brings the device's own changes back. Not one cycle per change.
    let used = app.transport.requests() - before;
    assert!(used <= 4, "{used} requests for five changes");
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn returning_to_the_app_syncs_unless_it_just_did(pool: PgPool) {
    let server = Server::with_sign_in(pool).await;
    // The gap that counts as "just did" is 30 seconds here, as in the apps.
    let app = running_app(&server).await;
    quiet().await;
    let before = app.transport.requests();

    app.session.notify(Trigger::Returned);
    quiet().await;
    assert_eq!(
        app.transport.requests(),
        before,
        "it synced a moment ago, so returning does nothing"
    );

    // The sync button always syncs.
    app.session.notify(Trigger::Manual);
    eventually("the button syncs", || async {
        app.transport.requests() > before
    })
    .await;

    // With no recent sync, returning syncs.
    let app = app.restarted(|config| config.timing.return_gap = Duration::ZERO);
    synced(&app).await;
    quiet().await;
    let before = app.transport.requests();
    app.session.notify(Trigger::Returned);
    eventually("returning syncs", || async {
        app.transport.requests() > before
    })
    .await;
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn after_being_offline_the_app_sends_what_is_waiting_without_being_asked(pool: PgPool) {
    let server = Server::with_sign_in(pool).await;
    let app = running_app(&server).await;

    app.transport.set_offline(true);
    app.session
        .store()
        .create_note("written offline")
        .await
        .unwrap();
    app.session.notify(Trigger::Changed);
    eventually("the failure is shown", || async {
        let status = app.session.sync_status();
        status.unsent == 1
            && status
                .failure
                .as_ref()
                .is_some_and(|failure| failure.class == ErrorClass::TryAgainLater)
    })
    .await;
    assert_eq!(server.blob_count().await, 0);

    // The network returns. Nothing tells the app; it was retrying.
    app.transport.set_offline(false);

    eventually("the note is sent", || async {
        server.blob_count().await == 1
    })
    .await;
    synced(&app).await;
    assert!(app.session.sync_status().failure.is_none());

    // Once nothing is waiting, the retries stop.
    quiet().await;
    let before = app.transport.requests();
    quiet().await;
    assert_eq!(app.transport.requests(), before);
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn with_nothing_to_send_a_failure_is_not_retried(pool: PgPool) {
    let server = Server::with_sign_in(pool).await;
    let app = running_app(&server).await;
    quiet().await;

    app.transport.set_offline(true);
    app.session.notify(Trigger::Manual);
    eventually("the failure is shown", || async {
        app.session.sync_status().failure.is_some()
    })
    .await;
    let before = app.transport.requests();

    quiet().await;

    assert_eq!(
        app.transport.requests(),
        before,
        "nothing is waiting, so the next event is soon enough"
    );
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_device_that_must_sign_in_again_stops_trying_until_it_has(pool: PgPool) {
    let server = Server::with_sign_in(pool).await;
    let app = running_app(&server).await;
    let dirt_core::session::SessionState::Ready { device_id, .. } = app.session.state() else {
        panic!("signed in");
    };

    server.expire(device_id).await;
    app.session.store().create_note("stuck").await.unwrap();
    app.session.notify(Trigger::Changed);
    eventually("the app says to sign in again", || async {
        app.session
            .sync_status()
            .failure
            .is_some_and(|failure| failure.class == ErrorClass::StopUntilSignedIn)
    })
    .await;
    quiet().await;
    let before = app.transport.requests();

    // More changes, and coming back to the app, achieve nothing by
    // themselves, so they are not tried.
    app.session.store().create_note("also stuck").await.unwrap();
    app.session.notify(Trigger::Changed);
    app.session.notify(Trigger::Returned);
    quiet().await;
    assert_eq!(app.transport.requests(), before);
    assert_eq!(app.session.sync_status().unsent, 2);

    // Signing in again sends what was waiting.
    app.sign_in("alice").await.unwrap();
    eventually("both notes are sent", || async {
        server.blob_count().await == 2
    })
    .await;
    synced(&app).await;
    assert!(app.session.sync_status().failure.is_none());
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn the_status_says_when_notes_arrived_from_another_device(pool: PgPool) {
    let server = Server::with_sign_in(pool).await;
    let app = running_app(&server).await;
    let other = Client::new(&server, "Phone", false);
    other.sign_in("alice").await.unwrap();
    other.approve_with_key(&server, &app.account_key()).await;
    other
        .session
        .store()
        .create_note("from the phone")
        .await
        .unwrap();
    other.session.sync_now().await.unwrap();
    let before = app.session.sync_status().pulled;

    app.session.notify(Trigger::Manual);

    eventually("the note arrives", || async {
        app.session.sync_status().pulled > before
    })
    .await;
    assert_eq!(app.notes().await[0].1, "from the phone");
}
