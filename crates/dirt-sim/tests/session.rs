//! Whole client cores signing in, syncing and signing out against the real
//! server, through a stand-in identity provider.
//!
//! Start the database with `docker compose up -d` at the repository root and
//! set `DATABASE_URL` as `compose.yaml` shows. Each test runs in its own
//! throwaway database.

use dirt_core::api::BoxFuture;
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

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn one_sign_in_at_a_time_until_it_ends_fails_or_is_cancelled(pool: PgPool) {
    const REDIRECT: &str = "dev.dirt.app://callback";
    let server = Server::with_sign_in(pool).await;
    let phone = Client::new(&server, "Phone", false);
    let session = &phone.session;

    // Two presses at once: the second is refused before it asks the server
    // anything, and so is the desktop's way of signing in meanwhile.
    let (first, second) = tokio::join!(
        session.begin_sign_in(REDIRECT),
        session.begin_sign_in(REDIRECT)
    );
    let address = first.unwrap();
    assert!(matches!(second, Err(SessionError::AlreadySigningIn)));
    assert!(matches!(
        phone.sign_in("alice").await,
        Err(SessionError::AlreadySigningIn)
    ));
    assert_eq!(session.state(), SessionState::SigningIn);

    // The person leaves the browser and cancels: another may begin, and
    // the first attempt's redirect finishes nothing.
    session.cancel_sign_in();
    assert_eq!(session.state(), SessionState::SignedOut);
    let stale = Client::redirect_for(&address, "alice").await;
    assert!(matches!(
        session.finish_sign_in(&stale).await,
        Err(SessionError::NoAttempt)
    ));

    // Cancelled while the sign-in page's address is being made: that
    // beginning ends as cancelled, and leaves the way free.
    let cancelling = session.clone();
    phone
        .transport
        .before_next("POST /v1/nonces", move || -> BoxFuture<'static, ()> {
            Box::pin(async move { cancelling.cancel_sign_in() })
        });
    assert!(matches!(
        session.begin_sign_in(REDIRECT).await,
        Err(SessionError::Cancelled)
    ));
    assert_eq!(session.state(), SessionState::SignedOut);

    // A sign-in that fails frees it too.
    session.begin_sign_in(REDIRECT).await.unwrap();
    assert!(session
        .finish_sign_in("dev.dirt.app://callback?code=stolen&state=someone-elses")
        .await
        .is_err());
    assert_eq!(session.state(), SessionState::SignedOut);

    // And so does one that finishes.
    let address = session.begin_sign_in(REDIRECT).await.unwrap();
    let redirect = Client::redirect_for(&address, "alice").await;
    assert!(session.finish_sign_in(&redirect).await.unwrap().is_ready());
    assert!(matches!(
        session.finish_sign_in(&redirect).await,
        Err(SessionError::NoAttempt)
    ));
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
    let SignOutError::Unsynced {
        notes,
        files,
        cause,
    } = error
    else {
        panic!("expected the unsynced note to stop the sign-out: {error}");
    };
    assert_eq!((notes, files), (1, 0));
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

/// The account's store folder on `client`'s device.
fn account_folder(client: &Client) -> std::path::PathBuf {
    let (account, _) = ids(client.session.state());
    store_path(
        client.folder(),
        &StoreName::account(&account.to_string()).unwrap(),
    )
    .parent()
    .unwrap()
    .to_owned()
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_sign_out_by_the_cli_stops_the_open_app_writing_to_the_notes_it_removes(pool: PgPool) {
    let server = Server::with_sign_in(pool).await;
    let laptop = Client::new(&server, "Laptop", false);
    laptop.sign_in("alice").await.unwrap();
    let desktop = second_device(&server, &laptop, "Desktop").await;
    let cli = desktop.second_program(&server);
    let folder = account_folder(&desktop);
    // What the desktop app's screen holds.
    let screen = desktop.session.store();
    screen.create_note("synced before").await.unwrap();
    desktop.session.sync_now().await.unwrap();

    cli.session.sign_out(Unsynced::Refuse).await.unwrap();

    // On Windows the desktop's hold keeps the folder, whole, until it lets
    // go, and the CLI says so; elsewhere it went at once.
    assert_eq!(cli.session.remove_signed_out_stores(), cfg!(windows));
    // The desktop writes nothing more to it...
    let refused = screen
        .create_note("typed after the sign-out")
        .await
        .unwrap_err();
    assert!(matches!(refused, dirt_core::Error::SignedOut), "{refused}");
    // ...and notices the sign-out, and takes the local store into use.
    assert_eq!(desktop.session.reload().unwrap(), SessionState::SignedOut);
    assert_ne!(desktop.session.store().path(), screen.path());
    drop(screen);
    assert!(!desktop.session.remove_signed_out_stores());
    assert!(!folder.exists());
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn signing_back_in_beside_the_open_app_starts_clean_and_loses_nothing(pool: PgPool) {
    let server = Server::with_sign_in(pool).await;
    let laptop = Client::new(&server, "Laptop", false);
    laptop.sign_in("alice").await.unwrap();
    let desktop = second_device(&server, &laptop, "Desktop").await;
    let cli = desktop.second_program(&server);
    let screen = desktop.session.store();
    // Sent by the sync that signing out begins with.
    screen.create_note("sent at sign-out").await.unwrap();
    cli.session.sign_out(Unsynced::Refuse).await.unwrap();
    laptop
        .session
        .store()
        .create_note("in the account")
        .await
        .unwrap();
    laptop.session.sync_now().await.unwrap();

    // The CLI signs in again to the same account, as a new device, and is
    // let in before the desktop app has noticed anything.
    let state = cli.sign_in("alice").await.unwrap();
    assert!(matches!(state, SessionState::WaitingForApproval { .. }));
    let mut secrets = DeviceSecrets::read(&*cli.secrets).unwrap().unwrap();
    server.approve(secrets.device_id).await;
    secrets.account_key = Some(laptop.account_key());
    secrets.approved = true;
    secrets.write(&*cli.secrets).unwrap();
    let taken = cli.session.reload();

    if cfg!(windows) {
        // The old notes are still held, so they cannot go; the new sign-in
        // does not take them over, and is taken into use once they have.
        assert!(matches!(taken, Err(SessionError::PreviousNotesOpen)));
        assert_eq!(cli.session.state(), SessionState::SignedOut);
        assert!(cli.session.held_back().is_some());
        // The CLI opens a session for each command: that still opens, on
        // the local store, and says why, so a thought can be written.
        let next_command = desktop.second_program(&server);
        assert_eq!(next_command.session.state(), SessionState::SignedOut);
        assert!(next_command.session.held_back().is_some());
        let local = next_command.session.store();
        assert_eq!(local.path(), next_command.session.device_store().path());
        local.create_note("a thought meanwhile").await.unwrap();
        drop(local);
        drop(next_command);
        // The desktop app notices, and cannot either while its screen holds
        // the old store. Once the screen lets go, it can.
        assert!(matches!(
            desktop.session.reload(),
            Err(SessionError::PreviousNotesOpen)
        ));
        assert_eq!(desktop.session.state(), SessionState::SignedOut);
        drop(screen);
        assert!(!desktop.session.remove_signed_out_stores());
        assert!(desktop.session.reload().unwrap().is_ready());
        assert!(cli.session.reload().unwrap().is_ready());
        assert_eq!(desktop.session.held_back(), None);
        // And the next command takes it into use as it opens.
        assert!(desktop.second_program(&server).session.state().is_ready());
    } else {
        // The old folder went at sign-out, under the desktop's hold, and
        // what the desktop writes to it is refused.
        assert!(taken.unwrap().is_ready());
        assert!(matches!(
            screen.create_note("lost").await.unwrap_err(),
            dirt_core::Error::SignedOut
        ));
        drop(screen);
        assert!(desktop.session.reload().unwrap().is_ready());
    }

    // One new store for the new device, which has never synced, and from
    // which nothing is lost: the account's notes come to it.
    assert_eq!(cli.session.store().path(), desktop.session.store().path());
    assert!(cli.notes().await.is_empty());
    assert_eq!(cli.session.store().last_synced_ms().await.unwrap(), None);
    cli.session.sync_now().await.unwrap();
    let texts: Vec<String> = cli
        .notes()
        .await
        .into_iter()
        .map(|(_, text)| text)
        .collect();
    assert_eq!(texts, ["sent at sign-out", "in the account"]);
    cli.session
        .store()
        .create_note("written after")
        .await
        .unwrap();
    assert_eq!(cli.session.sync_now().await.unwrap().pushed, 1);
}

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

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_sign_in_not_yet_in_use_can_sign_out(pool: PgPool) {
    // Only Windows keeps a folder that another program holds open.
    if !cfg!(windows) {
        return;
    }
    let server = Server::with_sign_in(pool).await;
    let laptop = Client::new(&server, "Laptop", false);
    laptop.sign_in("alice").await.unwrap();
    let desktop = second_device(&server, &laptop, "Desktop").await;
    let cli = desktop.second_program(&server);
    let screen = desktop.session.store();
    cli.session.sign_out(Unsynced::Refuse).await.unwrap();
    cli.sign_in("alice").await.unwrap();
    let mut secrets = DeviceSecrets::read(&*cli.secrets).unwrap().unwrap();
    let device = secrets.device_id;
    server.approve(device).await;
    secrets.account_key = Some(laptop.account_key());
    secrets.approved = true;
    secrets.write(&*cli.secrets).unwrap();

    // The next command finds the sign-in held back, and signs it out.
    let next_command = desktop.second_program(&server);
    assert!(next_command.session.held_back().is_some());
    let signed_out = next_command.session.sign_out(Unsynced::Refuse).await;
    assert_eq!(signed_out.unwrap(), dirt_core::session::SignedOut::Complete);
    assert!(next_command.secrets_are_empty());
    assert_eq!(next_command.session.held_back(), None);
    assert_eq!(server.device_status(device).await, "revoked");
    drop(screen);
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_sign_out_the_server_does_not_hear_of_says_so(pool: PgPool) {
    let server = Server::with_sign_in(pool).await;
    let laptop = Client::new(&server, "Laptop", false);
    laptop.sign_in("alice").await.unwrap();
    let phone = second_device(&server, &laptop, "Phone").await;
    // The server's answer never arrives, as when the network goes.
    phone.transport.lose_next_answer("DELETE /v1/devices");

    let signed_out = phone.session.sign_out(Unsynced::Refuse).await.unwrap();

    assert_eq!(signed_out, dirt_core::session::SignedOut::StillListed);
    assert!(phone.secrets_are_empty());
    assert_eq!(phone.session.state(), SessionState::SignedOut);
}
