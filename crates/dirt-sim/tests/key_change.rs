//! Changing the account's key: whole client cores against the real server.
//!
//! Removing a device stops it reaching the server, but it keeps the key it
//! had. These tests check what changing the key is for, that it survives
//! devices being away and a change being cut short, and that neither a
//! removed device nor the server can turn it to their use.
//!
//! Start the database with `docker compose up -d` at the repository root and
//! set `DATABASE_URL` as `compose.yaml` shows.

use dirt_core::api::BoxFuture;
use dirt_core::crypto::{decrypt_note, AccountKey, BlobContext};
use dirt_core::key_change::{unwrap_account_key, wrap_account_key, WrapContext};
use dirt_core::secrets::DeviceSecrets;
use dirt_core::session::{Approval, ExchangeStep, Join, KeyChange, SessionError, SessionState};
use dirt_core::store::ProblemKind;
use dirt_core::sync::engine::SyncError;
use dirt_proto::api::ErrorClass;
use dirt_proto::NoteId;
use dirt_sim::{Client, Server};
use sqlx::PgPool;
use uuid::Uuid;

fn device_id(client: &Client) -> Uuid {
    match client.session.state() {
        SessionState::Ready { device_id, .. }
        | SessionState::WaitingForApproval { device_id, .. } => device_id,
        other => panic!("not signed in: {other:?}"),
    }
}

fn secrets_of(client: &Client) -> DeviceSecrets {
    DeviceSecrets::read(&*client.secrets)
        .expect("read secrets")
        .expect("signed in")
}

/// Alice's first device, signed in, holding one synced note.
async fn laptop_with_a_note(server: &Server) -> Client {
    let laptop = Client::new(server, "Laptop", false);
    laptop.sign_in("alice").await.unwrap();
    write(&laptop, "written on the laptop").await;
    laptop.session.sync_now().await.unwrap();
    laptop
}

/// A second device of Alice's, signed in and waiting.
async fn waiting_device(server: &Server, name: &str) -> Client {
    let device = Client::new(server, name, false);
    let state = device.sign_in("alice").await.unwrap();
    assert!(matches!(state, SessionState::WaitingForApproval { .. }));
    device
}

/// Lets `device` in from `approver` by the real exchange, with the codes
/// compared.
async fn let_in(approver: &Client, device: &Client) {
    let mut join: Join = device.session.begin_join().await.unwrap();
    let target = approver
        .session
        .waiting_devices()
        .await
        .unwrap()
        .into_iter()
        .find(|waiting| waiting.id == device_id(device))
        .expect("the device is waiting");
    let mut approval: Approval = approver.session.begin_approval(target).unwrap();
    assert_eq!(approval.poll().await.unwrap(), ExchangeStep::Waiting);
    let ExchangeStep::Compare { code: on_device } = join.poll().await.unwrap() else {
        panic!("the new device should show a code");
    };
    let ExchangeStep::Compare { code: on_approver } = approval.poll().await.unwrap() else {
        panic!("the approving device should show a code");
    };
    assert_eq!(on_device, on_approver);
    approval.confirm().await.unwrap();
    assert_eq!(join.confirm().await.unwrap(), ExchangeStep::Done);
}

/// A further device of Alice's, let in by `approver`.
async fn approved_device(server: &Server, approver: &Client, name: &str) -> Client {
    let device = waiting_device(server, name).await;
    let_in(approver, &device).await;
    device.session.sync_now().await.unwrap();
    device
}

async fn write(client: &Client, text: &str) -> NoteId {
    client.session.store().create_note(text).await.unwrap().id
}

async fn texts(client: &Client) -> Vec<String> {
    let mut texts: Vec<String> = client
        .notes()
        .await
        .into_iter()
        .map(|(_, text)| text)
        .collect();
    texts.sort();
    texts
}

/// Every blob the server holds: `(note, key version, ciphertext)`.
async fn blobs(pool: &PgPool) -> Vec<(Uuid, i32, Vec<u8>)> {
    sqlx::query_as(
        "SELECT note_id, key_version, ciphertext FROM notes \
          WHERE ciphertext IS NOT NULL ORDER BY change_number",
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

async fn account_state(pool: &PgPool) -> (i32, i64) {
    sqlx::query_as("SELECT key_version, old_key_notes FROM accounts")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_removed_device_cannot_read_what_is_written_after_the_key_is_changed(pool: PgPool) {
    let server = Server::with_sign_in(pool.clone()).await;
    let laptop = laptop_with_a_note(&server).await;
    let phone = approved_device(&server, &laptop, "Phone").await;
    let lost = approved_device(&server, &laptop, "Lost tablet").await;
    // What the tablet took with it: the account key, and its own keys.
    let taken = secrets_of(&lost);
    let old_key = taken.account_key.clone().unwrap();
    assert_eq!(texts(&lost).await, ["written on the laptop"]);

    laptop
        .session
        .remove_device(device_id(&lost))
        .await
        .unwrap();
    let changed = laptop.session.change_account_key().await.unwrap();
    assert_eq!(
        changed,
        KeyChange::Changed {
            key_version: 2,
            old_key_notes: 0
        }
    );
    let secret = write(&laptop, "written after the tablet was lost").await;
    laptop.session.sync_now().await.unwrap();

    // The devices that stayed read everything, old and new.
    phone.session.sync_now().await.unwrap();
    assert_eq!(
        texts(&phone).await,
        ["written after the tablet was lost", "written on the laptop"]
    );
    assert_eq!(secrets_of(&phone).key_version, 2);
    assert_eq!(account_state(&pool).await, (2, 0));

    // The tablet cannot reach the server.
    let refused = lost.session.sync_now().await.unwrap_err();
    assert!(matches!(
        &refused,
        SessionError::Sync(error) if error.class() == ErrorClass::StopUntilSignedIn
    ));

    // Give it everything the server holds anyway. No blob opens with the
    // key it has, whatever version it claims the blob to be.
    let account_id = taken.account_id;
    let stored = blobs(&pool).await;
    assert_eq!(stored.len(), 2);
    for (note, version, ciphertext) in &stored {
        assert_eq!(*version, 2, "no note is left under the key the tablet has");
        let note_id = NoteId::from_uuid(*note);
        for claimed in [1, 2] {
            let context = BlobContext::under(account_id, note_id, claimed);
            assert!(decrypt_note(&old_key, &context, ciphertext).is_err());
        }
    }
    assert!(stored.iter().any(|(note, _, _)| *note == secret.as_uuid()));

    // Nor does any wrapped copy of the new key open for it, with the old
    // account key and its own device key, as any device or version.
    let copies: Vec<(Uuid, i32, Vec<u8>)> =
        sqlx::query_as("SELECT device_id, key_version, wrapped FROM device_keys")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(
        copies.len(),
        2,
        "one copy each for the laptop and the phone"
    );
    for (device, version, wrapped) in &copies {
        assert_ne!(*device, taken.device_id);
        for claimed_device in [*device, taken.device_id] {
            let context = WrapContext {
                account_id,
                device_id: claimed_device,
                key_version: *version,
            };
            assert!(unwrap_account_key(wrapped, &old_key, &taken.device_key, &context).is_err());
        }
    }
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_device_that_was_away_through_two_key_changes_catches_up(pool: PgPool) {
    let server = Server::with_sign_in(pool.clone()).await;
    let laptop = laptop_with_a_note(&server).await;
    let phone = approved_device(&server, &laptop, "Phone").await;

    // The phone goes away, and a note is written on it meanwhile.
    phone.transport.set_offline(true);
    write(&phone, "written on the phone while it was away").await;

    laptop.session.change_account_key().await.unwrap();
    write(&laptop, "under the second key").await;
    laptop.session.sync_now().await.unwrap();
    let again = laptop.session.change_account_key().await.unwrap();
    assert!(matches!(again, KeyChange::Changed { key_version: 3, .. }));
    write(&laptop, "under the third key").await;
    laptop.session.sync_now().await.unwrap();

    // It comes back holding the first key. It opens the second with the
    // first and the third with the second.
    phone.transport.set_offline(false);
    let report = phone.session.sync_now().await.unwrap();
    assert_eq!((report.key_version, report.old_key_notes), (3, 0));
    let all = [
        "under the second key",
        "under the third key",
        "written on the laptop",
        "written on the phone while it was away",
    ];
    assert_eq!(texts(&phone).await, all);
    laptop.session.sync_now().await.unwrap();
    assert_eq!(texts(&laptop).await, all);

    let on_phone = secrets_of(&phone);
    let on_laptop = secrets_of(&laptop);
    assert_eq!(on_phone.key_version, 3);
    assert_eq!(
        on_phone.account_key.unwrap().as_bytes(),
        on_laptop.account_key.unwrap().as_bytes()
    );
    // With nothing left under them, the keys before are let go.
    assert!(on_phone.older_keys.is_empty());
    assert!(on_laptop.older_keys.is_empty());
    assert!(blobs(&pool)
        .await
        .iter()
        .all(|(_, version, _)| *version == 3));
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_key_change_cut_short_is_finished_by_another_device(pool: PgPool) {
    let server = Server::with_sign_in(pool.clone()).await;
    let laptop = laptop_with_a_note(&server).await;
    write(&laptop, "a second note").await;
    laptop.session.sync_now().await.unwrap();
    let phone = approved_device(&server, &laptop, "Phone").await;

    // The server takes the laptop's request and the laptop never hears. It
    // is switched off before it does anything more.
    laptop.transport.lose_next_answer("POST /v1/account/key");
    let cut_short = laptop.session.change_account_key().await.unwrap_err();
    assert!(matches!(
        &cut_short,
        SessionError::Server(error) if error.class() == ErrorClass::TryAgainLater
    ));
    laptop.transport.set_offline(true);
    assert_eq!(account_state(&pool).await, (2, 2));
    assert_eq!(secrets_of(&laptop).key_version, 1);

    // The phone knows nothing of this. Its next sync finds a new key,
    // opens its copy, and sends the notes again.
    let report = phone.session.sync_now().await.unwrap();
    assert_eq!((report.key_version, report.old_key_notes), (2, 0));
    assert_eq!(account_state(&pool).await, (2, 0));
    assert!(blobs(&pool)
        .await
        .iter()
        .all(|(_, version, _)| *version == 2));

    // The laptop comes back and gets the key it made from its own copy.
    laptop.transport.set_offline(false);
    laptop.session.sync_now().await.unwrap();
    assert_eq!(secrets_of(&laptop).key_version, 2);
    assert_eq!(
        secrets_of(&laptop).account_key.unwrap().as_bytes(),
        secrets_of(&phone).account_key.unwrap().as_bytes()
    );
    write(&laptop, "after all that").await;
    laptop.session.sync_now().await.unwrap();
    phone.session.sync_now().await.unwrap();
    assert_eq!(
        texts(&phone).await,
        ["a second note", "after all that", "written on the laptop"]
    );
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_key_the_server_makes_by_itself_is_refused(pool: PgPool) {
    let server = Server::with_sign_in(pool.clone()).await;
    let laptop = laptop_with_a_note(&server).await;
    let phone = approved_device(&server, &laptop, "Phone").await;
    let before = secrets_of(&phone);

    // The server knows each device's public key, the account and the
    // version. It wraps a key of its own for both devices and says the
    // account's key has changed. It has never had the account's key, so it
    // wraps with a guess.
    let servers_key = AccountKey::generate();
    let guess = AccountKey::generate();
    for device in [&laptop, &phone] {
        let secrets = secrets_of(device);
        let context = WrapContext {
            account_id: secrets.account_id,
            device_id: secrets.device_id,
            key_version: 2,
        };
        let forged = wrap_account_key(
            &servers_key,
            &guess,
            &secrets.device_key.public_bytes(),
            &context,
        )
        .unwrap();
        sqlx::query("INSERT INTO device_keys (device_id, key_version, wrapped) VALUES ($1, 2, $2)")
            .bind(secrets.device_id)
            .bind(forged)
            .execute(&pool)
            .await
            .unwrap();
    }
    sqlx::query("UPDATE accounts SET key_version = 2")
        .execute(&pool)
        .await
        .unwrap();

    // The phone writes something private and syncs.
    write(&phone, "not for the server").await;
    let refused = phone.session.sync_now().await.unwrap_err();
    assert!(matches!(
        refused,
        SessionError::Sync(SyncError::KeyRejected)
    ));

    // It adopted nothing, and sent nothing under the server's key.
    let after = secrets_of(&phone);
    assert_eq!(after.key_version, 1);
    assert_eq!(
        after.account_key.unwrap().as_bytes(),
        before.account_key.unwrap().as_bytes()
    );
    let stored = blobs(&pool).await;
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].1, 1);
    // The note is still on the phone.
    assert!(texts(&phone)
        .await
        .contains(&"not for the server".to_owned()));
    // And it stays refused: the device does not try the forged key again
    // and then accept it.
    assert!(phone.session.sync_now().await.is_err());
    assert_eq!(secrets_of(&phone).key_version, 1);
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn two_devices_changing_the_key_at_once_make_one_change(pool: PgPool) {
    let server = Server::with_sign_in(pool.clone()).await;
    let laptop = laptop_with_a_note(&server).await;
    let phone = approved_device(&server, &laptop, "Phone").await;

    // Both prepare a change from the same key. The laptop's request reaches
    // the server first: it is made while the phone's is on its way.
    let first = laptop.session.clone();
    let (told, laptops_outcome) = tokio::sync::oneshot::channel();
    phone
        .transport
        .before_next("POST /v1/account/key", move || -> BoxFuture<'static, ()> {
            Box::pin(async move {
                let _ = told.send(first.change_account_key().await);
            })
        });
    let on_phone = phone.session.change_account_key().await.unwrap();
    let on_laptop = laptops_outcome.await.unwrap().unwrap();

    assert!(
        matches!(on_laptop, KeyChange::Changed { key_version: 2, .. }),
        "{on_laptop:?}"
    );
    // The phone's request was for a key that had already been replaced. It
    // made no second change: the account has a new key, which is what was
    // asked for. It took the laptop's.
    assert_eq!(
        on_phone,
        KeyChange::ChangedByAnotherDevice { key_version: 2 }
    );

    laptop.session.sync_now().await.unwrap();
    phone.session.sync_now().await.unwrap();
    assert_eq!(account_state(&pool).await, (2, 0));
    assert_eq!(
        secrets_of(&laptop).account_key.unwrap().as_bytes(),
        secrets_of(&phone).account_key.unwrap().as_bytes()
    );
    write(&phone, "after both asked").await;
    phone.session.sync_now().await.unwrap();
    laptop.session.sync_now().await.unwrap();
    assert_eq!(
        texts(&laptop).await,
        ["after both asked", "written on the laptop"]
    );
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_note_sent_under_the_old_key_is_refused_and_arrives_under_the_new_one(pool: PgPool) {
    let server = Server::with_sign_in(pool.clone()).await;
    let laptop = laptop_with_a_note(&server).await;
    let phone = approved_device(&server, &laptop, "Phone").await;
    let written = write(&phone, "written just before the key changed").await;

    // The key changes after the phone has pulled and before it pushes: its
    // pull said nothing of a new key.
    let changing = laptop.session.clone();
    phone
        .transport
        .before_next("POST /v1/notes/push", move || -> BoxFuture<'static, ()> {
            Box::pin(async move {
                changing.change_account_key().await.unwrap();
            })
        });

    let report = phone.session.sync_now().await.unwrap();
    assert_eq!(report.unsent, 0);
    assert_eq!(secrets_of(&phone).key_version, 2);
    let stored = blobs(&pool).await;
    let sent = stored
        .iter()
        .find(|(note, _, _)| *note == written.as_uuid())
        .expect("the note reached the server");
    assert_eq!(sent.1, 2);
    // Refusing it was not a problem with the note.
    assert!(phone
        .session
        .store()
        .note_problems()
        .await
        .unwrap()
        .is_empty());

    laptop.session.sync_now().await.unwrap();
    assert!(texts(&laptop)
        .await
        .contains(&"written just before the key changed".to_owned()));
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn the_recovery_phrase_stops_working_and_a_new_one_works(pool: PgPool) {
    let server = Server::with_sign_in(pool.clone()).await;
    let laptop = laptop_with_a_note(&server).await;
    let old_phrase = laptop.session.create_recovery_phrase().await.unwrap();
    assert!(laptop.session.my_device().await.unwrap().recovery_available);

    laptop.session.change_account_key().await.unwrap();
    assert!(!laptop.session.my_device().await.unwrap().recovery_available);

    // A new device with the old phrase gets nowhere: there is nothing for
    // the phrase to open.
    let tablet = waiting_device(&server, "Tablet").await;
    let refused = tablet
        .session
        .recover_with_phrase(&old_phrase.words())
        .await
        .unwrap_err();
    assert!(matches!(refused, SessionError::NoRecoveryCopy));

    let new_phrase = laptop.session.create_recovery_phrase().await.unwrap();
    // The old phrase does not open the new copy either.
    assert!(tablet
        .session
        .recover_with_phrase(&old_phrase.words())
        .await
        .is_err());
    let state = tablet
        .session
        .recover_with_phrase(&new_phrase.words())
        .await
        .unwrap();
    assert!(state.is_ready());
    tablet.session.sync_now().await.unwrap();
    assert_eq!(texts(&tablet).await, ["written on the laptop"]);
    assert_eq!(secrets_of(&tablet).key_version, 2);
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_device_let_in_during_a_key_change_reads_everything_once_it_is_over(pool: PgPool) {
    let server = Server::with_sign_in(pool.clone()).await;
    let laptop = laptop_with_a_note(&server).await;
    let phone = approved_device(&server, &laptop, "Phone").await;

    // The laptop begins a change and goes away before sending any note
    // again. The phone opens the new key, and its sync is cut off before
    // it sends them either.
    laptop.transport.lose_next_answer("POST /v1/account/key");
    laptop.session.change_account_key().await.unwrap_err();
    laptop.transport.set_offline(true);
    phone
        .transport
        .lose_next_answer("GET /v1/account/key/notes");
    phone.session.sync_now().await.unwrap_err();
    assert_eq!(secrets_of(&phone).key_version, 2);
    assert_eq!(account_state(&pool).await, (2, 1));

    // A tablet is let in now. It is given the new key and no other.
    let tablet = waiting_device(&server, "Tablet").await;
    let_in(&phone, &tablet).await;
    let on_tablet = secrets_of(&tablet);
    assert_eq!(on_tablet.key_version, 2);
    assert!(on_tablet.older_keys.is_empty());

    // The note still under the first key is held on the tablet: it cannot
    // read it, and says so.
    tablet.session.sync_now().await.unwrap();
    assert!(texts(&tablet).await.is_empty());
    let problems = tablet.session.store().note_problems().await.unwrap();
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0].kind, ProblemKind::Held);
    assert_eq!(
        problems[0].reason,
        "encrypted with a key this device does not have"
    );

    // The phone finishes the change, and the tablet reads the note.
    phone.session.sync_now().await.unwrap();
    assert_eq!(account_state(&pool).await, (2, 0));
    tablet.session.sync_now().await.unwrap();
    assert_eq!(texts(&tablet).await, ["written on the laptop"]);
    assert!(tablet
        .session
        .store()
        .note_problems()
        .await
        .unwrap()
        .is_empty());
}

/// The key changes between an approval being opened and the person saying
/// the codes match. The new device must be given the key the account has
/// then, not the one the approving device held when it opened the approval.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn an_approval_that_spans_a_key_change_hands_over_the_new_key(pool: PgPool) {
    let server = Server::with_sign_in(pool.clone()).await;
    let laptop = laptop_with_a_note(&server).await;
    let phone = approved_device(&server, &laptop, "Phone").await;

    let tablet = waiting_device(&server, "Tablet").await;
    let target = phone
        .session
        .waiting_devices()
        .await
        .unwrap()
        .into_iter()
        .find(|waiting| waiting.id == device_id(&tablet))
        .unwrap();
    // The phone opens the approval while it still holds the first key.
    let mut approval: Approval = phone.session.begin_approval(target).unwrap();
    assert_eq!(secrets_of(&phone).key_version, 1);

    laptop.session.change_account_key().await.unwrap();
    phone.session.sync_now().await.unwrap();
    assert_eq!(secrets_of(&phone).key_version, 2);

    // The tablet asks now. The phone answers with the approval it opened
    // before the change.
    let mut join: Join = tablet.session.begin_join().await.unwrap();
    assert_eq!(approval.poll().await.unwrap(), ExchangeStep::Waiting);
    let ExchangeStep::Compare { code: on_tablet } = join.poll().await.unwrap() else {
        panic!("the tablet should show a code");
    };
    let ExchangeStep::Compare { code: on_phone } = approval.poll().await.unwrap() else {
        panic!("the phone should show a code");
    };
    assert_eq!(on_tablet, on_phone);
    approval.confirm().await.unwrap();
    assert_eq!(join.confirm().await.unwrap(), ExchangeStep::Done);

    let on_tablet = secrets_of(&tablet);
    assert_eq!(on_tablet.key_version, 2);
    assert_eq!(
        on_tablet.account_key.unwrap().as_bytes(),
        secrets_of(&laptop).account_key.unwrap().as_bytes()
    );
    tablet.session.sync_now().await.unwrap();
    assert_eq!(texts(&tablet).await, ["written on the laptop"]);
}

/// The desktop app is in the middle of fetching a new key when the CLI, on
/// the same computer, resets the account. The reset stores a fresh key, and
/// that record is its only copy: every other device is gone. When the
/// app's fetch comes back it must not store what it fetched over it.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_key_fetched_while_the_other_program_resets_is_not_stored_over_the_new_key(pool: PgPool) {
    let server = Server::with_sign_in(pool.clone()).await;
    let desktop = laptop_with_a_note(&server).await;
    let phone = approved_device(&server, &desktop, "Phone").await;
    phone.session.change_account_key().await.unwrap();

    // The desktop app's sync finds a new key and asks for its copy. While
    // the answer is on its way, the other program's reset lands: the same
    // account, device and token, and a new first key.
    let after_reset = AccountKey::generate();
    let stored = std::sync::Arc::clone(&desktop.secrets);
    let fresh = after_reset.clone();
    desktop
        .transport
        .rewrite_next_answer("GET /v1/account/keys", move |_answer| {
            let mut secrets = DeviceSecrets::read(&*stored).unwrap().unwrap();
            secrets.set_only_key(1, fresh);
            secrets.write(&*stored).unwrap();
        });

    let refused = desktop.session.sync_now().await.unwrap_err();
    assert!(
        matches!(refused, SessionError::Sync(SyncError::Superseded)),
        "{refused:?}"
    );
    let now = secrets_of(&desktop);
    assert_eq!(now.key_version, 1);
    assert_eq!(
        now.account_key.unwrap().as_bytes(),
        after_reset.as_bytes(),
        "the key the reset stored was overwritten"
    );
    assert!(now.older_keys.is_empty());
}

/// The same with a sign-out in place of the reset: the fetched key must not
/// bring the stored secrets back.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_key_fetched_while_the_other_program_signs_out_does_not_sign_it_back_in(pool: PgPool) {
    let server = Server::with_sign_in(pool.clone()).await;
    let desktop = laptop_with_a_note(&server).await;
    let phone = approved_device(&server, &desktop, "Phone").await;
    phone.session.change_account_key().await.unwrap();

    let stored = std::sync::Arc::clone(&desktop.secrets);
    desktop
        .transport
        .rewrite_next_answer("GET /v1/account/keys", move |_answer| {
            dirt_core::secrets::SecretStore::clear(&*stored).unwrap();
        });

    let refused = desktop.session.sync_now().await.unwrap_err();
    assert!(
        matches!(refused, SessionError::Sync(SyncError::Superseded)),
        "{refused:?}"
    );
    assert!(desktop.secrets_are_empty());
}

/// A device let in after one change of the key never had the notes still
/// under the first key. When the key changes again, the server's list of
/// notes to send again begins with those, a page of them, and the device
/// can send none. It has to look further, where its own note is.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_device_that_can_send_none_of_the_first_page_sends_what_it_holds(pool: PgPool) {
    let server = Server::with_sign_in(pool.clone()).await;
    let page = dirt_proto::api::limits::MAX_BATCH_ITEMS;

    // More notes than one page of the list holds, all under the first key.
    let laptop = Client::new(&server, "Laptop", false);
    laptop.sign_in("alice").await.unwrap();
    for number in 0..page + 5 {
        write(&laptop, &format!("an old note {number}")).await;
    }
    laptop.session.sync_now().await.unwrap();
    let phone = approved_device(&server, &laptop, "Phone").await;

    // The key changes, and nobody sends the notes again: the laptop goes
    // away before it can, and the phone's sync is cut off after it has
    // opened the new key.
    laptop.transport.lose_next_answer("POST /v1/account/key");
    laptop.session.change_account_key().await.unwrap_err();
    laptop.transport.set_offline(true);
    phone
        .transport
        .lose_next_answer("GET /v1/account/key/notes");
    phone.session.sync_now().await.unwrap_err();
    let old_notes = i64::try_from(page + 5).unwrap();
    assert_eq!(account_state(&pool).await, (2, old_notes));

    // A tablet is let in with the second key. It writes a note of its own.
    let tablet = waiting_device(&server, "Tablet").await;
    let_in(&phone, &tablet).await;
    phone.transport.set_offline(true);
    let own = write(&tablet, "written on the tablet").await;
    tablet.session.sync_now().await.unwrap();
    assert_eq!(account_state(&pool).await, (2, old_notes));

    // The tablet changes the key again. Its own note is now under an older
    // key too, behind a full page of notes it cannot read.
    let changed = tablet.session.change_account_key().await.unwrap();
    assert!(matches!(changed, KeyChange::Changed { key_version: 3, .. }));

    let stored = blobs(&pool).await;
    let sent = stored
        .iter()
        .find(|(note, _, _)| *note == own.as_uuid())
        .unwrap();
    assert_eq!(sent.1, 3, "the tablet's own note was not sent again");
    // The notes it never held are still to be sent, by a device that can.
    assert_eq!(account_state(&pool).await, (3, old_notes));
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_reset_during_a_key_change_starts_the_keys_again(pool: PgPool) {
    let server = Server::with_sign_in(pool.clone()).await;
    let laptop = laptop_with_a_note(&server).await;
    let phone = approved_device(&server, &laptop, "Phone").await;

    laptop.transport.lose_next_answer("POST /v1/account/key");
    laptop.session.change_account_key().await.unwrap_err();
    assert_eq!(account_state(&pool).await, (2, 1));

    // The laptop still holds the first key. It resets the account.
    laptop.session.reset_account().await.unwrap();
    assert_eq!(account_state(&pool).await, (1, 0));
    let on_laptop = secrets_of(&laptop);
    assert_eq!(on_laptop.key_version, 1);
    assert!(on_laptop.older_keys.is_empty());

    let report = laptop.session.sync_now().await.unwrap();
    assert_eq!((report.key_version, report.old_key_notes), (1, 0));
    let stored = blobs(&pool).await;
    assert_eq!(stored.len(), 1);
    let (note, version, ciphertext) = &stored[0];
    assert_eq!(*version, 1);
    let context = BlobContext::under(on_laptop.account_id, NoteId::from_uuid(*note), 1);
    assert!(decrypt_note(&on_laptop.account_key.unwrap(), &context, ciphertext).is_ok());
    let (copies,): (i64,) = sqlx::query_as("SELECT count(*) FROM device_keys")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(copies, 0);

    // The phone was removed by the reset.
    assert!(phone.session.sync_now().await.is_err());
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_program_follows_a_key_change_the_other_program_made(pool: PgPool) {
    let server = Server::with_sign_in(pool.clone()).await;
    // The desktop app and the CLI on one computer: one device, one place
    // for its keys.
    let desktop = laptop_with_a_note(&server).await;
    let cli = desktop.second_program(&server);
    let phone = approved_device(&server, &desktop, "Phone").await;

    cli.session.change_account_key().await.unwrap();

    // The desktop app has not been told. Its next sync takes the keys the
    // CLI stored and carries on; it is not a new sign-in, and nothing is
    // refused as changed by another program.
    write(&desktop, "written in the app afterwards").await;
    let report = desktop.session.sync_now().await.unwrap();
    assert_eq!((report.key_version, report.unsent), (2, 0));
    assert!(desktop.session.reload().unwrap().is_ready());
    desktop.session.sync_now().await.unwrap();

    phone.session.sync_now().await.unwrap();
    assert_eq!(
        texts(&phone).await,
        ["written in the app afterwards", "written on the laptop"]
    );
    assert!(blobs(&pool)
        .await
        .iter()
        .all(|(_, version, _)| *version == 2));
}

/// An app that stays open shows how far a key change has got from what its
/// sync worker reports. The change is made past the worker, so the worker
/// has to be told, or the app goes on showing the account as it was.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn an_open_app_reports_the_key_change_it_made(pool: PgPool) {
    let server = Server::with_sign_in(pool.clone()).await;
    let app = Client::new(&server, "Laptop", true);
    app.sign_in("alice").await.unwrap();
    write(&app, "written in the app").await;
    dirt_sim::eventually("the app has synced under the first key", || async {
        app.session.sync_status().key_version == 1 && server.blob_count().await == 1
    })
    .await;

    app.session.change_account_key().await.unwrap();

    dirt_sim::eventually("the app reports the new key", || async {
        app.session.sync_status().key_version == 2
    })
    .await;
    assert_eq!(app.session.sync_status().old_key_notes, 0);
}

/// The server keeps only so many copies of later keys for a device. One
/// that was away through more changes than that finds the copy that follows
/// its own key gone. It says so and stops, with its notes as they were:
/// signing in again makes it a new device, which another device lets in.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_device_too_far_behind_to_follow_stops_and_keeps_its_notes(pool: PgPool) {
    let server = Server::with_sign_in(pool.clone()).await;
    let laptop = laptop_with_a_note(&server).await;
    let phone = approved_device(&server, &laptop, "Phone").await;
    phone.transport.set_offline(true);
    write(&phone, "written on the phone while it was away").await;

    laptop.session.change_account_key().await.unwrap();
    laptop.session.change_account_key().await.unwrap();
    // What the server does to a copy once more changes have been made than
    // it keeps copies for.
    sqlx::query("DELETE FROM device_keys WHERE device_id = $1 AND key_version = 2")
        .bind(device_id(&phone))
        .execute(&pool)
        .await
        .unwrap();

    phone.transport.set_offline(false);
    let error = phone.session.sync_now().await.unwrap_err();
    let SessionError::Sync(error) = error else {
        panic!("expected a sync failure, got {error:?}");
    };
    assert!(
        matches!(error, SyncError::KeyUnavailable),
        "expected the key to be out of reach, got {error:?}"
    );
    assert_eq!(error.class(), ErrorClass::StopUntilSignedIn);
    // It adopted nothing: the third key's copy is there, but it opens only
    // with the second.
    assert_eq!(secrets_of(&phone).key_version, 1);
    assert_eq!(
        texts(&phone).await,
        [
            "written on the laptop",
            "written on the phone while it was away"
        ]
    );
}
