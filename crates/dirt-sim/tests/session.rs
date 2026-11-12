//! Whole client cores signing in, syncing and signing out against the real
//! server, through a stand-in identity provider.
//!
//! Start the database with `docker compose up -d` at the repository root and
//! set `DATABASE_URL` as `compose.yaml` shows. Each test runs in its own
//! throwaway database.

use dirt_core::identity::IdentityError;
use dirt_core::secrets::{DeviceSecrets, SecretStore};
use dirt_core::session::{SessionError, SessionState, SignOutError, Unsynced};
use dirt_core::store::{store_path, StoreName};
use dirt_proto::api::ErrorClass;
use dirt_sim::{Client, Server};
use sqlx::PgPool;
use uuid::Uuid;

fn ids(state: SessionState) -> (Uuid, Uuid) {
    match state {
        SessionState::Ready {
            account_id,
            device_id,
        }
        | SessionState::WaitingForApproval {
            account_id,
            device_id,
        } => (account_id, device_id),
        other => panic!("not signed in: {other:?}"),
    }
}

/// A second device of `first`'s account, approved and holding the key.
async fn second_device(server: &Server, first: &Client, name: &str) -> Client {
    let second = Client::new(server, name, false);
    let state = second.sign_in("alice").await.unwrap();
    assert!(matches!(state, SessionState::WaitingForApproval { .. }));
    second.approve_with_key(server, &first.account_key()).await;
    assert!(second.session.state().is_ready());
    second
}

// ---- Signing in ----

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn the_first_device_signs_in_makes_the_key_and_syncs(pool: PgPool) {
    let server = Server::with_sign_in(pool).await;
    let laptop = Client::new(&server, "Laptop", false);
    assert_eq!(laptop.session.state(), SessionState::SignedOut);
    let local = laptop.session.store().path().to_owned();

    let state = laptop.sign_in("alice").await.unwrap();

    let (account, device) = ids(state);
    assert!(state.is_ready());
    assert_eq!(server.device_status(device).await, "approved");
    // Notes now live in the account's own store.
    let store = laptop.session.store();
    assert_ne!(store.path(), local);
    assert_eq!(
        store.path(),
        store_path(
            laptop.folder(),
            &StoreName::account(&account.to_string()).unwrap()
        )
    );

    store.create_note("the first synced note").await.unwrap();
    let report = laptop.session.sync_now().await.unwrap();
    assert_eq!((report.pushed, report.unsent), (1, 0));
    assert_eq!(server.blob_count().await, 1);
    assert!(store.last_synced_ms().await.unwrap().is_some());

    // The app is closed and opened again: still signed in, same notes.
    let laptop = laptop.restarted(|_| {});
    assert_eq!(ids(laptop.session.state()), (account, device));
    assert_eq!(laptop.notes().await[0].1, "the first synced note");
    assert_eq!(laptop.session.sync_now().await.unwrap().unsent, 0);
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_second_device_waits_and_reads_nothing_until_it_has_the_key(pool: PgPool) {
    let server = Server::with_sign_in(pool).await;
    let laptop = Client::new(&server, "Laptop", false);
    laptop.sign_in("alice").await.unwrap();
    laptop.session.store().create_note("secret").await.unwrap();
    laptop.session.sync_now().await.unwrap();

    let phone = Client::new(&server, "Phone", false);
    let state = phone.sign_in("alice").await.unwrap();

    let (account, device) = ids(state);
    assert!(matches!(state, SessionState::WaitingForApproval { .. }));
    assert_eq!(account, ids(laptop.session.state()).0);
    assert_eq!(server.device_status(device).await, "pending");
    // It keeps using the local store and cannot sync.
    assert!(phone.notes().await.is_empty());
    assert!(matches!(
        phone.session.sync_now().await,
        Err(SessionError::NotReady)
    ));

    // Approved by the server but without the key, it still waits: there is
    // nothing it could decrypt with. Handing over the key is step 6.
    server.approve(device).await;
    let state = phone.session.check_approval().await.unwrap();
    assert!(matches!(state, SessionState::WaitingForApproval { .. }));

    // With the key it is ready and reads the account's notes.
    phone.approve_with_key(&server, &laptop.account_key()).await;
    phone.session.sync_now().await.unwrap();
    assert_eq!(phone.notes().await, laptop.notes().await);
    assert_eq!(phone.notes().await[0].1, "secret");
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn notes_written_before_signing_in_are_moved_only_when_asked(pool: PgPool) {
    let server = Server::with_sign_in(pool).await;
    let laptop = Client::new(&server, "Laptop", false);
    let local = laptop.session.store();
    local.create_note("written signed out").await.unwrap();
    local.create_note("another").await.unwrap();
    assert_eq!(laptop.session.local_notes_waiting().await.unwrap(), 0);

    laptop.sign_in("alice").await.unwrap();

    // Signing in moves nothing by itself.
    assert!(laptop.notes().await.is_empty());
    assert_eq!(laptop.session.local_notes_waiting().await.unwrap(), 2);

    assert_eq!(laptop.session.move_local_notes().await.unwrap(), 2);

    assert_eq!(laptop.session.local_notes_waiting().await.unwrap(), 0);
    assert!(local.note_documents().await.unwrap().is_empty());
    assert_eq!(laptop.notes().await.len(), 2);
    // They are part of the account now, and sync.
    assert_eq!(laptop.session.sync_now().await.unwrap().pushed, 2);
    assert_eq!(server.blob_count().await, 2);
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn the_app_can_be_stopped_while_the_browser_is_open(pool: PgPool) {
    let server = Server::with_sign_in(pool).await;
    let phone = Client::new(&server, "Phone", false).restarted(|config| {
        config.persist_attempts = true;
    });

    // As on Android: the app starts the sign-in, the browser comes to the
    // front, and the system stops the app.
    let address = phone
        .session
        .begin_sign_in("dev.dirt.app://callback")
        .await
        .unwrap();
    assert_eq!(phone.session.state(), SessionState::SigningIn);
    let redirect = Client::redirect_for(&address, "alice").await;
    assert!(redirect.starts_with("dev.dirt.app://callback?"));
    let phone = phone.restarted(|_| {});
    assert_eq!(phone.session.state(), SessionState::SignedOut);

    // The restarted app is handed the redirect and finishes the attempt.
    let state = phone.session.finish_sign_in(&redirect).await.unwrap();
    assert!(state.is_ready());
    assert!(!phone.folder().join("sign-in-attempt.json").exists());

    // The same redirect delivered again finds no attempt to finish.
    assert!(matches!(
        phone.session.finish_sign_in(&redirect).await,
        Err(SessionError::NoAttempt)
    ));
    assert!(phone.session.state().is_ready());
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_sign_in_that_fails_or_is_cancelled_leaves_the_device_as_it_was(pool: PgPool) {
    let server = Server::with_sign_in(pool).await;
    let laptop = Client::new(&server, "Laptop", false);

    // The browser never comes back; the person cancels.
    let session = laptop.session.clone();
    let waiting =
        tokio::spawn(async move { session.sign_in_with_browser(|_address| Ok(())).await });
    dirt_sim::eventually("the sign-in starts", || async {
        laptop.session.state() == SessionState::SigningIn
    })
    .await;
    laptop.session.cancel_sign_in();
    assert!(matches!(
        waiting.await.unwrap(),
        Err(SessionError::Cancelled)
    ));
    assert_eq!(laptop.session.state(), SessionState::SignedOut);

    // The browser cannot be opened at all.
    let error = laptop
        .session
        .sign_in_with_browser(|_address| Err("no browser".to_owned()))
        .await
        .unwrap_err();
    assert!(matches!(error, SessionError::Browser(_)));
    assert_eq!(laptop.session.state(), SessionState::SignedOut);

    // A redirect that belongs to another attempt.
    laptop
        .session
        .begin_sign_in("dev.dirt.app://callback")
        .await
        .unwrap();
    let error = laptop
        .session
        .finish_sign_in("dev.dirt.app://callback?code=stolen&state=someone-elses")
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        SessionError::Identity(IdentityError::Rejected(_))
    ));
    assert_eq!(laptop.session.state(), SessionState::SignedOut);
    assert!(laptop.secrets.load().unwrap().is_none());

    // None of that stops a later sign-in.
    assert!(laptop.sign_in("alice").await.unwrap().is_ready());
}

// ---- A token that stopped working ----

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_device_whose_token_expired_signs_in_again_as_itself(pool: PgPool) {
    let server = Server::with_sign_in(pool).await;
    let laptop = Client::new(&server, "Laptop", false);
    let (account, device) = ids(laptop.sign_in("alice").await.unwrap());
    let store = laptop.session.store();
    store.create_note("before the drawer").await.unwrap();
    laptop.session.sync_now().await.unwrap();

    server.expire(device).await;
    store.create_note("written while expired").await.unwrap();
    let Err(SessionError::Sync(error)) = laptop.session.sync_now().await else {
        panic!("an expired device must not sync");
    };
    assert_eq!(error.class(), ErrorClass::StopUntilSignedIn);
    // It is still signed in as far as the device is concerned.
    assert!(laptop.session.state().is_ready());

    let state = laptop.sign_in("alice").await.unwrap();

    assert_eq!(ids(state), (account, device));
    assert!(state.is_ready());
    assert_eq!(laptop.notes().await.len(), 2);
    assert_eq!(laptop.session.sync_now().await.unwrap().pushed, 1);
    assert_eq!(server.blob_count().await, 2);
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn signing_in_again_as_someone_else_is_refused_and_changes_nothing(pool: PgPool) {
    let server = Server::with_sign_in(pool.clone()).await;
    let laptop = Client::new(&server, "Laptop", false);
    let before = ids(laptop.sign_in("alice").await.unwrap());
    let key = laptop.account_key();
    // Bob already has an account, so the device registered for him below
    // is one that would wait for approval.
    let bobs = Client::new(&server, "Bob's", false);
    bobs.sign_in("bob").await.unwrap();

    let error = laptop.sign_in("bob").await.unwrap_err();

    assert!(matches!(error, SessionError::DifferentAccount));
    assert_eq!(ids(laptop.session.state()), before);
    assert_eq!(laptop.account_key().as_bytes(), key.as_bytes());
    // Nothing is left waiting in Bob's account.
    let (pending,): (i64,) =
        sqlx::query_as("SELECT count(*) FROM devices WHERE status = 'pending'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(pending, 0);
}

// ---- Signing out ----

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn the_only_approved_device_cannot_sign_out(pool: PgPool) {
    let server = Server::with_sign_in(pool).await;
    let laptop = Client::new(&server, "Laptop", false);
    let before = ids(laptop.sign_in("alice").await.unwrap());
    laptop.session.store().create_note("kept").await.unwrap();
    // A device that is only waiting does not make it safe.
    let waiting = Client::new(&server, "Phone", false);
    waiting.sign_in("alice").await.unwrap();

    for choice in [Unsynced::Refuse, Unsynced::Discard] {
        let error = laptop.session.sign_out(choice).await.unwrap_err();
        assert!(matches!(error, SignOutError::OnlyApprovedDevice), "{error}");
    }

    assert_eq!(ids(laptop.session.state()), before);
    assert_eq!(laptop.notes().await[0].1, "kept");
    assert!(DeviceSecrets::read(&*laptop.secrets).unwrap().is_some());
    assert_eq!(server.device_status(before.1).await, "approved");
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_waiting_device_signs_out_and_leaves_the_account(pool: PgPool) {
    let server = Server::with_sign_in(pool).await;
    let laptop = Client::new(&server, "Laptop", false);
    laptop.sign_in("alice").await.unwrap();
    let phone = Client::new(&server, "Phone", false);
    let (_, device) = ids(phone.sign_in("alice").await.unwrap());
    phone
        .session
        .store()
        .create_note("still local")
        .await
        .unwrap();

    phone.session.sign_out(Unsynced::Refuse).await.unwrap();

    assert_eq!(phone.session.state(), SessionState::SignedOut);
    assert!(phone.secrets.load().unwrap().is_none());
    assert_eq!(server.device_status(device).await, "revoked");
    // Its local notes were never the account's, and stay.
    assert_eq!(phone.notes().await[0].1, "still local");
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn signing_out_syncs_first_and_then_removes_the_notes_and_the_keys(pool: PgPool) {
    let server = Server::with_sign_in(pool).await;
    let laptop = Client::new(&server, "Laptop", false);
    laptop.sign_in("alice").await.unwrap();
    let phone = second_device(&server, &laptop, "Phone").await;
    let (account, device) = ids(phone.session.state());
    let folder = store_path(
        phone.folder(),
        &StoreName::account(&account.to_string()).unwrap(),
    )
    .parent()
    .unwrap()
    .to_owned();
    phone
        .session
        .store()
        .create_note("written just before signing out")
        .await
        .unwrap();

    phone.session.sign_out(Unsynced::Refuse).await.unwrap();

    assert_eq!(phone.session.state(), SessionState::SignedOut);
    assert!(phone.secrets.load().unwrap().is_none());
    assert_eq!(server.device_status(device).await, "revoked");
    assert!(
        !folder.exists(),
        "the account's notes are gone from the device"
    );
    assert!(
        phone.notes().await.is_empty(),
        "back on the empty local store"
    );
    // The note reached the account before the device left.
    laptop.session.sync_now().await.unwrap();
    assert_eq!(laptop.notes().await[0].1, "written just before signing out");
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn signing_out_with_notes_that_cannot_be_sent_asks_first(pool: PgPool) {
    let server = Server::with_sign_in(pool).await;
    let laptop = Client::new(&server, "Laptop", false);
    laptop.sign_in("alice").await.unwrap();
    let phone = second_device(&server, &laptop, "Phone").await;
    let (_, device) = ids(phone.session.state());
    phone
        .session
        .store()
        .create_note("not sent yet")
        .await
        .unwrap();

    // The cycle is cut off, so the note cannot reach the server.
    phone.transport.lose_next_answer("/v1/notes/pull");
    let error = phone.session.sign_out(Unsynced::Refuse).await.unwrap_err();
    let SignOutError::Unsynced { notes, cause } = error else {
        panic!("expected the unsynced note to stop the sign-out: {error}");
    };
    assert_eq!(notes, 1);
    assert!(cause.is_some());
    assert!(phone.session.state().is_ready());
    assert_eq!(phone.notes().await[0].1, "not sent yet");
    assert_eq!(server.device_status(device).await, "approved");

    // Told to discard them, it signs out.
    phone.transport.lose_next_answer("/v1/notes/pull");
    phone.session.sign_out(Unsynced::Discard).await.unwrap();
    assert_eq!(phone.session.state(), SessionState::SignedOut);
    assert_eq!(server.device_status(device).await, "revoked");
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_folder_that_could_not_be_removed_at_sign_out_goes_at_the_next_start(pool: PgPool) {
    let server = Server::with_sign_in(pool).await;
    let laptop = Client::new(&server, "Laptop", false);
    laptop.sign_in("alice").await.unwrap();
    let phone = second_device(&server, &laptop, "Phone").await;
    let (account, _) = ids(phone.session.state());
    let database = store_path(
        phone.folder(),
        &StoreName::account(&account.to_string()).unwrap(),
    );
    let folder = database.parent().unwrap().to_owned();

    // The interface still holds the store while the session signs out.
    let held_by_the_interface = phone.session.store();
    phone.session.sign_out(Unsynced::Refuse).await.unwrap();
    drop(held_by_the_interface);

    // Whether the folder could be removed depends on the platform. Either
    // way it is gone once the app has started again.
    let phone = phone.restarted(|_| {});
    assert!(!folder.exists());
    assert_eq!(phone.session.state(), SessionState::SignedOut);
}

// ---- Two programs, one device ----

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_running_app_notices_a_sign_in_or_sign_out_made_by_the_other_program(pool: PgPool) {
    let server = Server::with_sign_in(pool).await;
    let laptop = Client::new(&server, "Laptop", false);
    assert_eq!(laptop.session.reload().unwrap(), SessionState::SignedOut);

    // The other program on this machine signs in: the record appears in the
    // shared secure storage.
    let other = Client::new(&server, "Laptop", false);
    other.sign_in("alice").await.unwrap();
    laptop
        .secrets
        .save(&other.secrets.load().unwrap().unwrap())
        .unwrap();

    let state = laptop.session.reload().unwrap();
    assert_eq!(ids(state), ids(other.session.state()));
    assert!(state.is_ready());

    // And signs out again.
    laptop.secrets.clear().unwrap();
    assert_eq!(laptop.session.reload().unwrap(), SessionState::SignedOut);
    assert!(matches!(
        laptop.session.sync_now().await,
        Err(SessionError::NotReady)
    ));
}
