//! More than one device on an account: approving a device by comparing a
//! code, the recovery phrase, and resetting and deleting an account. Whole
//! client cores against the real server.
//!
//! Start the database with `docker compose up -d` at the repository root and
//! set `DATABASE_URL` as `compose.yaml` shows.

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use dirt_core::enrolment::{CancelReason, EnrolmentError, Message};
use dirt_core::recovery::RecoveryError;
use dirt_core::session::{
    Approval, ExchangeStep, Join, ResetOutcome, SessionError, SessionState, SignOutError, Unsynced,
};
use dirt_proto::api::ErrorClass;
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

/// Alice's first device, signed in and holding a synced note.
async fn laptop_with_a_note(server: &Server) -> Client {
    let laptop = Client::new(server, "Laptop", false);
    laptop.sign_in("alice").await.unwrap();
    laptop
        .session
        .store()
        .create_note("written on the laptop")
        .await
        .unwrap();
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

/// Starts the exchange on both devices and runs it until both show a code.
async fn until_codes(laptop: &Client, phone: &Client) -> (Approval, Join, String, String) {
    let mut join = phone.session.begin_join().await.unwrap();
    let waiting = laptop.session.waiting_devices().await.unwrap();
    let target = waiting
        .into_iter()
        .find(|device| device.id == device_id(phone))
        .expect("the phone is waiting");
    let mut approval = laptop.session.begin_approval(target).unwrap();

    // The laptop answers the commitment; the phone reveals; the laptop
    // checks the reveal.
    assert_eq!(approval.poll().await.unwrap(), ExchangeStep::Waiting);
    let ExchangeStep::Compare { code: on_phone } = join.poll().await.unwrap() else {
        panic!("the phone should show a code");
    };
    let ExchangeStep::Compare { code: on_laptop } = approval.poll().await.unwrap() else {
        panic!("the laptop should show a code");
    };
    (approval, join, on_laptop, on_phone)
}

/// A second device of Alice's that has been approved the real way.
async fn approved_device(server: &Server, laptop: &Client, name: &str) -> Client {
    let device = waiting_device(server, name).await;
    let (mut approval, mut join, on_laptop, on_device) = until_codes(laptop, &device).await;
    assert_eq!(on_laptop, on_device);
    approval.confirm().await.unwrap();
    assert_eq!(join.confirm().await.unwrap(), ExchangeStep::Done);
    device
}

/// Changes the messages in the server's answer to a device's next read of
/// an exchange, as a server in the middle would.
fn tamper(client: &Client, change: impl Fn(&mut Message) + Send + 'static) {
    client
        .transport
        .rewrite_next_answer("/v1/enrolments/", move |answer| {
            let Some(messages) = answer["messages"].as_array_mut() else {
                return;
            };
            for relayed in messages {
                let body = STANDARD
                    .decode(relayed["body"].as_str().unwrap_or_default())
                    .unwrap_or_default();
                if let Some(mut message) = Message::decode(&body) {
                    change(&mut message);
                    relayed["body"] = STANDARD.encode(message.encode()).into();
                }
            }
        });
}

/// A key a server could make up.
fn made_up_key() -> String {
    STANDARD
        .encode(dirt_core::crypto::DeviceKeyPair::generate().public_bytes())
        .trim_end_matches('=')
        .to_owned()
}

async fn enrolment_rows(server: &Server) -> (i64, i64) {
    sqlx::query_as(
        "SELECT (SELECT count(*) FROM enrolment_messages), (SELECT count(*) FROM enrolment_keys)",
    )
    .fetch_one(&server.pool)
    .await
    .unwrap()
}

// ---- Approving a device ----

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_second_device_is_approved_by_comparing_a_code_and_reads_the_notes(pool: PgPool) {
    let server = Server::with_sign_in(pool).await;
    let laptop = laptop_with_a_note(&server).await;
    let phone = waiting_device(&server, "Phone").await;
    // The laptop learns at its next sync that a device is waiting.
    assert_eq!(laptop.session.sync_now().await.unwrap().pending_devices, 1);

    let (mut approval, mut join, on_laptop, on_phone) = until_codes(&laptop, &phone).await;
    assert_eq!(on_laptop, on_phone, "both devices show the same code");

    // The laptop's person confirms first: the key is left, and the phone
    // is still waiting until it has fetched it.
    assert_eq!(approval.confirm().await.unwrap(), ExchangeStep::Finishing);
    assert_eq!(server.device_status(device_id(&phone)).await, "pending");
    // The server holds the key only in a form it cannot open.
    let (wrapped,): (Vec<u8>,) = sqlx::query_as("SELECT wrapped FROM enrolment_keys")
        .fetch_one(&server.pool)
        .await
        .unwrap();
    let account_key = laptop.account_key();
    assert!(!wrapped
        .windows(32)
        .any(|window| window == account_key.as_bytes()));

    assert_eq!(join.confirm().await.unwrap(), ExchangeStep::Done);

    assert!(phone.session.state().is_ready());
    assert_eq!(phone.account_key().as_bytes(), account_key.as_bytes());
    assert_eq!(server.device_status(device_id(&phone)).await, "approved");
    assert_eq!(approval.poll().await.unwrap(), ExchangeStep::Done);
    // Nothing of the exchange is left on the server.
    assert_eq!(enrolment_rows(&server).await, (0, 0));

    // The phone reads the account's notes, and writes one the laptop gets.
    phone.session.sync_now().await.unwrap();
    assert_eq!(phone.notes().await, laptop.notes().await);
    phone
        .session
        .store()
        .create_note("written on the phone")
        .await
        .unwrap();
    phone.session.sync_now().await.unwrap();
    let report = laptop.session.sync_now().await.unwrap();
    assert_eq!((report.pulled, report.pending_devices), (1, 0));
    assert_eq!(laptop.notes().await.len(), 2);
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn the_new_device_may_confirm_first(pool: PgPool) {
    let server = Server::with_sign_in(pool).await;
    let laptop = laptop_with_a_note(&server).await;
    let phone = waiting_device(&server, "Phone").await;
    let (mut approval, mut join, _, _) = until_codes(&laptop, &phone).await;

    // No key has been left yet, so the phone waits.
    assert_eq!(join.confirm().await.unwrap(), ExchangeStep::Finishing);
    assert!(!phone.session.state().is_ready());
    assert_eq!(approval.confirm().await.unwrap(), ExchangeStep::Finishing);
    assert_eq!(join.poll().await.unwrap(), ExchangeStep::Done);
    assert_eq!(approval.poll().await.unwrap(), ExchangeStep::Done);
    assert!(phone.session.state().is_ready());
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_server_that_swaps_the_approving_devices_key_makes_the_codes_differ(pool: PgPool) {
    let server = Server::with_sign_in(pool).await;
    let laptop = laptop_with_a_note(&server).await;
    let phone = waiting_device(&server, "Phone").await;

    let mut join = phone.session.begin_join().await.unwrap();
    let target = laptop.session.waiting_devices().await.unwrap().remove(0);
    let mut approval = laptop.session.begin_approval(target).unwrap();
    approval.poll().await.unwrap();
    // The server gives the phone a key of its own in place of the laptop's.
    let servers_key = made_up_key();
    tamper(&phone, move |message| {
        if let Message::Key { key, .. } = message {
            key.clone_from(&servers_key);
        }
    });
    let ExchangeStep::Compare { code: on_phone } = join.poll().await.unwrap() else {
        panic!("the phone should show a code");
    };
    let ExchangeStep::Compare { code: on_laptop } = approval.poll().await.unwrap() else {
        panic!("the laptop should show a code");
    };

    assert_ne!(on_phone, on_laptop, "the person sees two different codes");

    // The person says so on the phone. It signs out, and the laptop is
    // told why and removes it from the account.
    join.reject(CancelReason::Mismatch).await.unwrap();
    assert_eq!(phone.session.state(), SessionState::SignedOut);
    assert!(phone.secrets_are_empty());
    assert_eq!(
        approval.poll().await.unwrap(),
        ExchangeStep::Cancelled(CancelReason::Mismatch)
    );
    assert_eq!(server.device_status(target_id(&approval)).await, "revoked");
    assert_eq!(enrolment_rows(&server).await, (0, 0));
}

const fn target_id(approval: &Approval) -> Uuid {
    approval.device().id
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_key_left_by_a_server_in_the_middle_is_not_accepted(pool: PgPool) {
    let server = Server::with_sign_in(pool).await;
    let laptop = laptop_with_a_note(&server).await;
    let phone = waiting_device(&server, "Phone").await;

    let mut join = phone.session.begin_join().await.unwrap();
    let target = laptop.session.waiting_devices().await.unwrap().remove(0);
    let mut approval = laptop.session.begin_approval(target).unwrap();
    approval.poll().await.unwrap();
    let servers_key = made_up_key();
    tamper(&phone, move |message| {
        if let Message::Key { key, .. } = message {
            key.clone_from(&servers_key);
        }
    });
    join.poll().await.unwrap();
    approval.poll().await.unwrap();

    // Suppose both people press "they match" without looking. The laptop
    // seals the key for the device it really talked to. The phone, which
    // talked to the server, cannot open it and accepts nothing.
    approval.confirm().await.unwrap();
    let error = join.confirm().await.unwrap_err();
    assert!(
        matches!(error, SessionError::Enrolment(EnrolmentError::KeyRejected)),
        "{error}"
    );
    assert_eq!(phone.session.state(), SessionState::SignedOut);
    assert!(phone.secrets_are_empty());
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_server_that_swaps_the_new_devices_key_is_caught_before_any_code(pool: PgPool) {
    let server = Server::with_sign_in(pool).await;
    let laptop = laptop_with_a_note(&server).await;
    let phone = waiting_device(&server, "Phone").await;

    let mut join = phone.session.begin_join().await.unwrap();
    let target = laptop.session.waiting_devices().await.unwrap().remove(0);
    let mut approval = laptop.session.begin_approval(target).unwrap();
    approval.poll().await.unwrap();
    join.poll().await.unwrap();
    // The phone has revealed its key. The server shows the laptop another.
    let servers_key = made_up_key();
    tamper(&laptop, move |message| {
        if let Message::Reveal { key, .. } = message {
            key.clone_from(&servers_key);
        }
    });

    let error = approval.poll().await.unwrap_err();

    assert!(
        matches!(
            error,
            SessionError::Enrolment(EnrolmentError::CommitmentBroken)
        ),
        "{error}"
    );
    // The laptop removed the device; no code was ever shown on it.
    assert_eq!(server.device_status(device_id(&phone)).await, "revoked");
    assert_eq!(
        approval.poll().await.unwrap(),
        ExchangeStep::Cancelled(CancelReason::Mismatch)
    );
    // The phone finds itself removed, and signs out.
    assert!(matches!(
        join.poll().await.unwrap(),
        ExchangeStep::Cancelled(_)
    ));
    assert_eq!(phone.session.state(), SessionState::SignedOut);
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_device_that_stops_before_it_has_the_key_can_begin_again(pool: PgPool) {
    let server = Server::with_sign_in(pool).await;
    let laptop = laptop_with_a_note(&server).await;
    let phone = waiting_device(&server, "Phone").await;
    let (mut approval, join, _, _) = until_codes(&laptop, &phone).await;
    approval.confirm().await.unwrap();

    // The key is left, and the phone's app stops before fetching it. The
    // secret it needed to open the key is gone with it.
    drop(join);
    assert_eq!(server.device_status(device_id(&phone)).await, "pending");
    let phone = phone.restarted(|_| {});
    assert!(matches!(
        phone.session.state(),
        SessionState::WaitingForApproval { .. }
    ));

    // It begins again, with a new code, and this time finishes.
    let (mut approval, mut join, on_laptop, on_phone) = until_codes(&laptop, &phone).await;
    assert_eq!(on_laptop, on_phone);
    approval.confirm().await.unwrap();
    assert_eq!(join.confirm().await.unwrap(), ExchangeStep::Done);
    phone.session.sync_now().await.unwrap();
    assert_eq!(phone.notes().await, laptop.notes().await);
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn refusing_a_device_removes_it_and_signs_it_out(pool: PgPool) {
    let server = Server::with_sign_in(pool).await;
    let laptop = laptop_with_a_note(&server).await;
    let phone = waiting_device(&server, "Phone").await;
    phone.session.store().create_note("mine").await.unwrap();
    let (mut approval, mut join, _, _) = until_codes(&laptop, &phone).await;

    approval.reject(CancelReason::Declined).await;

    assert_eq!(server.device_status(device_id(&phone)).await, "revoked");
    assert_eq!(
        approval.poll().await.unwrap(),
        ExchangeStep::Cancelled(CancelReason::Declined)
    );
    // The phone finds itself removed and signs out. Its own notes, which
    // were never the account's, stay.
    assert_eq!(
        join.poll().await.unwrap(),
        ExchangeStep::Cancelled(CancelReason::Declined)
    );
    assert_eq!(phone.session.state(), SessionState::SignedOut);
    assert_eq!(phone.notes().await[0].1, "mine");
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn when_two_devices_answer_only_the_first_goes_on(pool: PgPool) {
    let server = Server::with_sign_in(pool).await;
    let laptop = laptop_with_a_note(&server).await;
    let tablet = approved_device(&server, &laptop, "Tablet").await;
    let phone = waiting_device(&server, "Phone").await;

    let mut join = phone.session.begin_join().await.unwrap();
    let target = laptop.session.waiting_devices().await.unwrap().remove(0);
    let mut on_laptop = laptop.session.begin_approval(target.clone()).unwrap();
    let mut on_tablet = tablet.session.begin_approval(target).unwrap();
    on_laptop.poll().await.unwrap();
    on_tablet.poll().await.unwrap();

    // The phone goes on with the laptop, which answered first.
    let ExchangeStep::Compare { code: on_phone } = join.poll().await.unwrap() else {
        panic!("the phone should show a code");
    };
    assert_eq!(
        on_laptop.poll().await.unwrap(),
        ExchangeStep::Compare { code: on_phone }
    );
    // The tablet steps aside, and does not remove the device.
    assert_eq!(
        on_tablet.poll().await.unwrap(),
        ExchangeStep::HandledElsewhere
    );
    assert_eq!(server.device_status(device_id(&phone)).await, "pending");

    on_laptop.confirm().await.unwrap();
    assert_eq!(join.confirm().await.unwrap(), ExchangeStep::Done);
}

// ---- The recovery phrase ----

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_new_device_gets_in_with_the_recovery_phrase_when_no_device_is_left(pool: PgPool) {
    let server = Server::with_sign_in(pool).await;
    let laptop = laptop_with_a_note(&server).await;

    let phrase = laptop.session.create_recovery_phrase().await.unwrap();
    let words = phrase.words().to_string();
    assert_eq!(words.split(' ').count(), 24);
    // The server holds a copy it cannot open: the account key is not in it.
    let (stored,): (Vec<u8>,) = sqlx::query_as("SELECT wrapped || claim_hash FROM recovery_keys")
        .fetch_one(&server.pool)
        .await
        .unwrap();
    let account_key = laptop.account_key();
    assert!(!stored
        .windows(32)
        .any(|window| window == account_key.as_bytes()));

    // The laptop is lost. A new phone signs in and waits; nobody can
    // approve it, but the account has a phrase.
    drop(laptop);
    let phone = waiting_device(&server, "Phone").await;
    let me = phone.session.my_device().await.unwrap();
    assert!(me.recovery_available);

    // A phrase that is not a phrase, and a phrase that is not this one.
    let error = phone
        .session
        .recover_with_phrase("these are not the words")
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        SessionError::Recovery(RecoveryError::NotAPhrase)
    ));
    let other = dirt_core::recovery::RecoveryPhrase::generate();
    let error = phone
        .session
        .recover_with_phrase(&other.words())
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        SessionError::Recovery(RecoveryError::WrongPhrase)
    ));
    assert!(!phone.session.state().is_ready());
    assert_eq!(server.device_status(device_id(&phone)).await, "pending");

    let state = phone.session.recover_with_phrase(&words).await.unwrap();

    assert!(state.is_ready());
    assert_eq!(phone.account_key().as_bytes(), account_key.as_bytes());
    phone.session.sync_now().await.unwrap();
    assert_eq!(phone.notes().await[0].1, "written on the laptop");
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_phrase_that_was_removed_or_replaced_opens_nothing(pool: PgPool) {
    let server = Server::with_sign_in(pool).await;
    let laptop = laptop_with_a_note(&server).await;
    let phone = waiting_device(&server, "Phone").await;

    // No phrase was ever made.
    let never = dirt_core::recovery::RecoveryPhrase::generate();
    assert!(matches!(
        phone.session.recover_with_phrase(&never.words()).await,
        Err(SessionError::NoRecoveryCopy)
    ));
    // A waiting device cannot make one for itself.
    assert!(matches!(
        phone.session.create_recovery_phrase().await,
        Err(SessionError::NotReady)
    ));

    let first = laptop.session.create_recovery_phrase().await.unwrap();
    let second = laptop.session.create_recovery_phrase().await.unwrap();
    // Making a new phrase ends the old one.
    assert!(matches!(
        phone.session.recover_with_phrase(&first.words()).await,
        Err(SessionError::Recovery(RecoveryError::WrongPhrase))
    ));

    laptop.session.remove_recovery_phrase().await.unwrap();
    assert!(matches!(
        phone.session.recover_with_phrase(&second.words()).await,
        Err(SessionError::NoRecoveryCopy)
    ));
    assert!(!phone.session.my_device().await.unwrap().recovery_available);
}

// ---- Signing out, now that it can be safe ----

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn the_only_device_may_sign_out_once_the_account_has_a_phrase(pool: PgPool) {
    let server = Server::with_sign_in(pool).await;
    let laptop = laptop_with_a_note(&server).await;
    assert!(matches!(
        laptop.session.sign_out(Unsynced::Refuse).await,
        Err(SignOutError::OnlyApprovedDevice)
    ));

    let phrase = laptop.session.create_recovery_phrase().await.unwrap();
    laptop.session.sign_out(Unsynced::Refuse).await.unwrap();
    assert_eq!(laptop.session.state(), SessionState::SignedOut);
    assert!(laptop.notes().await.is_empty());

    // The same person signs in again on the same computer. No device of
    // the account is left, so it waits, and the phrase lets it in.
    let state = laptop.sign_in("alice").await.unwrap();
    assert!(matches!(state, SessionState::WaitingForApproval { .. }));
    assert_eq!(laptop.session.my_device().await.unwrap().approvers, 0);
    laptop
        .session
        .recover_with_phrase(&phrase.words())
        .await
        .unwrap();
    laptop.session.sync_now().await.unwrap();
    assert_eq!(laptop.notes().await[0].1, "written on the laptop");
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_device_may_sign_out_once_another_holds_the_key(pool: PgPool) {
    let server = Server::with_sign_in(pool).await;
    let laptop = laptop_with_a_note(&server).await;
    let phone = approved_device(&server, &laptop, "Phone").await;

    laptop.session.sign_out(Unsynced::Refuse).await.unwrap();

    assert_eq!(laptop.session.state(), SessionState::SignedOut);
    // Now the phone is the only one, and may not.
    assert!(matches!(
        phone.session.sign_out(Unsynced::Refuse).await,
        Err(SignOutError::OnlyApprovedDevice)
    ));
}

// ---- Removing a device ----

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_removed_device_can_no_longer_reach_the_server(pool: PgPool) {
    let server = Server::with_sign_in(pool).await;
    let laptop = laptop_with_a_note(&server).await;
    let phone = approved_device(&server, &laptop, "Phone").await;
    phone.session.sync_now().await.unwrap();

    let devices = laptop.session.devices().await.unwrap();
    assert_eq!(devices.len(), 2);
    assert!(devices.iter().any(|device| device.current));
    laptop
        .session
        .remove_device(device_id(&phone))
        .await
        .unwrap();

    let Err(SessionError::Sync(error)) = phone.session.sync_now().await else {
        panic!("a removed device must not sync");
    };
    assert_eq!(error.class(), ErrorClass::StopUntilSignedIn);
    // What it already held it still holds: removal does not reach into the
    // device. Changing the account key, in a later step, is what ends that.
    assert_eq!(phone.notes().await[0].1, "written on the laptop");
}

// ---- Resetting an account ----

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_reset_leaves_one_device_a_new_key_and_its_own_notes(pool: PgPool) {
    let server = Server::with_sign_in(pool).await;
    let laptop = laptop_with_a_note(&server).await;
    let phone = approved_device(&server, &laptop, "Phone").await;
    laptop.session.create_recovery_phrase().await.unwrap();
    let old_key = laptop.account_key();
    let (old_blob,): (Vec<u8>,) = sqlx::query_as("SELECT ciphertext FROM notes")
        .fetch_one(&server.pool)
        .await
        .unwrap();

    assert_eq!(
        laptop.session.reset_account().await.unwrap(),
        ResetOutcome::Done
    );

    assert!(laptop.session.state().is_ready());
    assert_ne!(laptop.account_key().as_bytes(), old_key.as_bytes());
    assert_eq!(server.blob_count().await, 0);
    // The laptop sends its own notes again, under the new key.
    let report = laptop.session.sync_now().await.unwrap();
    assert_eq!((report.pushed, report.unsent), (1, 0));
    let (new_blob,): (Vec<u8>,) = sqlx::query_as("SELECT ciphertext FROM notes")
        .fetch_one(&server.pool)
        .await
        .unwrap();
    assert_ne!(new_blob, old_blob);
    assert_eq!(laptop.notes().await[0].1, "written on the laptop");
    let me = laptop.session.my_device().await.unwrap();
    assert_eq!((me.approvers, me.recovery_available), (0, false));

    // The other device is no longer part of the account.
    let Err(SessionError::Sync(error)) = phone.session.sync_now().await else {
        panic!("the other device left with the reset");
    };
    assert_eq!(error.class(), ErrorClass::StopUntilSignedIn);
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_waiting_device_resets_an_account_whose_first_device_lost_its_key(pool: PgPool) {
    let server = Server::with_sign_in(pool).await;
    // The first device registered, and its app stopped before it saved the
    // key it had just made. Nothing was ever sent.
    let lost = Client::new(&server, "Laptop", false);
    lost.sign_in("alice").await.unwrap();
    drop(lost);

    let phone = waiting_device(&server, "Phone").await;
    phone
        .session
        .store()
        .create_note("written while waiting")
        .await
        .unwrap();

    assert_eq!(
        phone.session.reset_account().await.unwrap(),
        ResetOutcome::Done
    );

    assert!(phone.session.state().is_ready());
    // What it wrote while waiting is still in its local store, to be moved.
    assert_eq!(phone.session.local_notes_waiting().await.unwrap(), 1);
    phone.session.move_local_notes().await.unwrap();
    assert_eq!(phone.session.sync_now().await.unwrap().pushed, 1);
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_waiting_device_can_only_ask_to_reset_an_account_that_holds_notes(pool: PgPool) {
    let server = Server::with_sign_in(pool).await;
    let laptop = laptop_with_a_note(&server).await;
    let phone = waiting_device(&server, "Phone").await;

    let ResetOutcome::Scheduled { effective_at_ms } = phone.session.reset_account().await.unwrap()
    else {
        panic!("a waiting device must not reset an account with notes at once");
    };

    // Nothing has happened, and the laptop is told at its next sync.
    assert!(!phone.session.state().is_ready());
    let report = laptop.session.sync_now().await.unwrap();
    assert_eq!(report.reset_effective_at_ms, Some(effective_at_ms));
    assert_eq!(server.blob_count().await, 1);

    laptop.session.cancel_reset().await.unwrap();

    assert!(laptop
        .session
        .sync_now()
        .await
        .unwrap()
        .reset_effective_at_ms
        .is_none());
    assert!(matches!(
        phone.session.cancel_reset().await,
        Err(SessionError::NotReady)
    ));
    assert_eq!(laptop.notes().await[0].1, "written on the laptop");
}

// ---- Deleting an account ----

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn deleting_the_account_empties_the_server_and_keeps_the_notes_on_the_device(pool: PgPool) {
    let server = Server::with_sign_in(pool.clone()).await;
    let laptop = laptop_with_a_note(&server).await;
    let phone = approved_device(&server, &laptop, "Phone").await;
    phone.session.sync_now().await.unwrap();

    laptop.session.delete_account().await.unwrap();

    let (accounts, devices, notes): (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM accounts), (SELECT count(*) FROM devices), \
                (SELECT count(*) FROM notes)",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((accounts, devices, notes), (0, 0, 0));
    // The laptop is signed out, and its notes are in its local store.
    assert_eq!(laptop.session.state(), SessionState::SignedOut);
    assert!(laptop.secrets_are_empty());
    assert_eq!(laptop.notes().await[0].1, "written on the laptop");

    // The phone finds out at its next sync, and keeps what it holds.
    let Err(SessionError::Sync(error)) = phone.session.sync_now().await else {
        panic!("the account is gone");
    };
    assert_eq!(error.class(), ErrorClass::StopUntilSignedIn);
    assert_eq!(phone.notes().await[0].1, "written on the laptop");

    // A waiting device cannot delete an account.
    let again = Client::new(&server, "Laptop", false);
    again.sign_in("alice").await.unwrap();
    let late = waiting_device(&server, "Late").await;
    assert!(matches!(
        late.session.delete_account().await,
        Err(SessionError::NotReady)
    ));
}

// ---- Requests that fail, and people who change their mind ----

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn an_exchange_survives_a_request_whose_answer_is_lost(pool: PgPool) {
    let server = Server::with_sign_in(pool).await;
    let laptop = laptop_with_a_note(&server).await;
    let phone = waiting_device(&server, "Phone").await;
    let mut join = phone.session.begin_join().await.unwrap();
    let target = laptop.session.waiting_devices().await.unwrap().remove(0);
    let mut approval = laptop.session.begin_approval(target).unwrap();

    // The laptop's answer reaches the server, but the laptop never hears
    // so. It sends the same answer again and goes on.
    laptop.transport.lose_next_answer("POST /v1/enrolments/");
    assert!(approval.poll().await.is_err());
    assert_eq!(approval.poll().await.unwrap(), ExchangeStep::Waiting);

    // The same happens to the phone's reveal.
    phone.transport.lose_next_answer("POST /v1/enrolments/");
    assert!(join.poll().await.is_err());
    let ExchangeStep::Compare { code: on_phone } = join.poll().await.unwrap() else {
        panic!("the phone should show a code after trying again");
    };
    assert_eq!(
        approval.poll().await.unwrap(),
        ExchangeStep::Compare { code: on_phone },
        "sending twice changed nothing"
    );

    approval.confirm().await.unwrap();
    assert_eq!(join.confirm().await.unwrap(), ExchangeStep::Done);
    assert_eq!(approval.poll().await.unwrap(), ExchangeStep::Done);
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn the_new_device_can_still_call_it_off_after_the_key_was_left(pool: PgPool) {
    let server = Server::with_sign_in(pool).await;
    let laptop = laptop_with_a_note(&server).await;
    let phone = waiting_device(&server, "Phone").await;
    let (mut approval, mut join, _, _) = until_codes(&laptop, &phone).await;

    // The laptop's person confirms. Then the phone's person looks properly
    // and says the codes differ.
    assert_eq!(approval.confirm().await.unwrap(), ExchangeStep::Finishing);
    join.reject(CancelReason::Mismatch).await.unwrap();

    assert_eq!(
        approval.poll().await.unwrap(),
        ExchangeStep::Cancelled(CancelReason::Mismatch)
    );
    assert_eq!(server.device_status(target_id(&approval)).await, "revoked");
    // The key that was left for it is gone from the server.
    assert_eq!(enrolment_rows(&server).await, (0, 0));
}

// ---- Two programs on one device ----

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_program_stops_syncing_when_the_other_one_replaces_the_account_key(pool: PgPool) {
    let server = Server::with_sign_in(pool).await;
    // The desktop app and the CLI on one computer share the keys.
    let desktop = laptop_with_a_note(&server).await;
    let cli = desktop.second_program(&server);
    assert!(cli.session.state().is_ready());
    let old_key = desktop.account_key();

    assert_eq!(
        cli.session.reset_account().await.unwrap(),
        ResetOutcome::Done
    );

    // The desktop app still holds the old key. It must not send anything
    // encrypted with it: no device that joins later could read that.
    let Err(SessionError::Sync(error)) = desktop.session.sync_now().await else {
        panic!("a program holding a replaced key must not sync");
    };
    assert_eq!(error.class(), ErrorClass::TryAgainLater);
    assert_eq!(server.blob_count().await, 0);

    // When the person comes back to it, it reads the keys again.
    assert!(desktop.session.reload().unwrap().is_ready());
    assert_ne!(desktop.account_key().as_bytes(), old_key.as_bytes());
    desktop.session.sync_now().await.unwrap();
    assert_eq!(server.blob_count().await, 1);

    // A device that joins now, with the new key, reads what was sent.
    let phone = approved_device(&server, &desktop, "Phone").await;
    phone.session.sync_now().await.unwrap();
    assert!(phone
        .session
        .store()
        .note_problems()
        .await
        .unwrap()
        .is_empty());
    assert_eq!(phone.notes().await[0].1, "written on the laptop");
}

/// The laptop removes the phone while the phone is in the middle of a look:
/// it has read the laptop's answer and is about to send its own. Whichever
/// request brings the news, the phone stops and signs out.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_device_removed_as_it_answers_stops_and_signs_out(pool: PgPool) {
    let server = Server::with_sign_in(pool.clone()).await;
    let laptop = laptop_with_a_note(&server).await;
    let phone = waiting_device(&server, "Phone").await;
    let phone_id = device_id(&phone);
    let mut join = phone.session.begin_join().await.unwrap();
    let target = laptop
        .session
        .waiting_devices()
        .await
        .unwrap()
        .into_iter()
        .find(|device| device.id == phone_id)
        .expect("the phone is waiting");
    let mut approval = laptop.session.begin_approval(target).unwrap();
    assert_eq!(approval.poll().await.unwrap(), ExchangeStep::Waiting);

    phone.transport.before_next(
        "POST /v1/enrolments/",
        move || -> dirt_core::api::BoxFuture<'static, ()> {
            Box::pin(async move {
                sqlx::query("UPDATE devices SET status = 'revoked' WHERE id = $1")
                    .bind(phone_id)
                    .execute(&pool)
                    .await
                    .unwrap();
            })
        },
    );

    assert_eq!(
        join.poll().await.unwrap(),
        ExchangeStep::Cancelled(CancelReason::Declined)
    );
    assert_eq!(phone.session.state(), SessionState::SignedOut);
    assert!(phone.secrets_are_empty());
}

// ---- The exchange as a screen runs it ----

mod on_screen {
    use std::time::Duration;

    use dirt_core::session::{Answer, Exchange, ExchangeView, Pace};
    use dirt_sim::eventually;

    use super::*;

    const fn quick() -> Pace {
        Pace {
            every: Duration::from_millis(30),
            give_up_after: Duration::from_secs(20),
        }
    }

    async fn both_screens(laptop: &Client, phone: &Client, pace: Pace) -> (Exchange, Exchange) {
        let joining = Exchange::join(phone.session.begin_join().await.unwrap(), pace);
        let target = laptop.session.waiting_devices().await.unwrap().remove(0);
        let approving = Exchange::approve(laptop.session.begin_approval(target).unwrap(), pace);
        (approving, joining)
    }

    fn code_on(screen: &Exchange) -> Option<String> {
        match screen.view() {
            ExchangeView::Step(ExchangeStep::Compare { code }) => Some(code),
            _ => None,
        }
    }

    #[sqlx::test(migrator = "dirt_api::MIGRATOR")]
    async fn two_open_screens_reach_the_code_and_finish_by_themselves(pool: PgPool) {
        let server = Server::with_sign_in(pool).await;
        let laptop = laptop_with_a_note(&server).await;
        let phone = waiting_device(&server, "Phone").await;
        let (approving, joining) = both_screens(&laptop, &phone, quick()).await;

        eventually("both screens show a code", || async {
            code_on(&approving).is_some() && code_on(&joining).is_some()
        })
        .await;
        assert_eq!(code_on(&approving), code_on(&joining));

        approving.answer(Answer::Match);
        joining.answer(Answer::Match);

        eventually("both screens say done", || async {
            approving.view() == ExchangeView::Step(ExchangeStep::Done)
                && joining.view() == ExchangeView::Step(ExchangeStep::Done)
        })
        .await;
        assert!(phone.session.state().is_ready());
    }

    #[sqlx::test(migrator = "dirt_api::MIGRATOR")]
    async fn saying_the_codes_differ_ends_it_on_both_screens(pool: PgPool) {
        let server = Server::with_sign_in(pool).await;
        let laptop = laptop_with_a_note(&server).await;
        let phone = waiting_device(&server, "Phone").await;
        let (approving, joining) = both_screens(&laptop, &phone, quick()).await;
        eventually("both screens show a code", || async {
            code_on(&approving).is_some() && code_on(&joining).is_some()
        })
        .await;

        approving.answer(Answer::Mismatch);

        eventually("both screens say it is off", || async {
            matches!(
                approving.view(),
                ExchangeView::Step(ExchangeStep::Cancelled(CancelReason::Mismatch))
            ) && matches!(
                joining.view(),
                ExchangeView::Step(ExchangeStep::Cancelled(_))
            )
        })
        .await;
        assert_eq!(phone.session.state(), SessionState::SignedOut);
        assert_eq!(
            server
                .device_status(target_id_of(&laptop, &server).await)
                .await,
            "revoked"
        );
    }

    /// The one device of the account that is not the laptop's own.
    async fn target_id_of(laptop: &Client, server: &Server) -> Uuid {
        let own = device_id(laptop);
        sqlx::query_scalar("SELECT id FROM devices WHERE id <> $1")
            .bind(own)
            .fetch_one(&server.pool)
            .await
            .unwrap()
    }

    #[sqlx::test(migrator = "dirt_api::MIGRATOR")]
    async fn a_screen_stops_asking_when_it_is_closed_or_nothing_happens(pool: PgPool) {
        let server = Server::with_sign_in(pool).await;
        let laptop = laptop_with_a_note(&server).await;
        let phone = waiting_device(&server, "Phone").await;

        // Nobody opens the approving screen. The new device's screen gives
        // up after its limit.
        let impatient = Pace {
            every: Duration::from_millis(30),
            give_up_after: Duration::from_millis(250),
        };
        let joining = Exchange::join(phone.session.begin_join().await.unwrap(), impatient);
        eventually("the screen gives up", || async {
            joining.view() == ExchangeView::TimedOut
        })
        .await;
        let after_giving_up = phone.transport.requests();
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(phone.transport.requests(), after_giving_up);
        // The device is still waiting, and can try again.
        assert!(matches!(
            phone.session.state(),
            SessionState::WaitingForApproval { .. }
        ));

        // A screen that is closed stops at once.
        let joining = Exchange::join(phone.session.begin_join().await.unwrap(), quick());
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(phone.transport.requests() > after_giving_up);
        drop(joining);
        tokio::time::sleep(Duration::from_millis(100)).await;
        let after_closing = phone.transport.requests();
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(phone.transport.requests(), after_closing);
        drop(laptop);
    }

    #[sqlx::test(migrator = "dirt_api::MIGRATOR")]
    async fn an_answer_given_while_the_network_is_down_is_acted_on_when_it_returns(pool: PgPool) {
        let server = Server::with_sign_in(pool).await;
        let laptop = laptop_with_a_note(&server).await;
        let phone = waiting_device(&server, "Phone").await;
        let (approving, joining) = both_screens(&laptop, &phone, quick()).await;
        eventually("both screens show a code", || async {
            code_on(&approving).is_some() && code_on(&joining).is_some()
        })
        .await;

        // The person presses "They match" on the laptop just as its network
        // drops. The key cannot be left with the server.
        laptop.transport.set_offline(true);
        approving.answer(Answer::Match);
        joining.answer(Answer::Match);
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(!phone.session.state().is_ready());

        // The network returns. Nobody presses anything again.
        laptop.transport.set_offline(false);

        eventually("both screens say done", || async {
            approving.view() == ExchangeView::Step(ExchangeStep::Done)
                && joining.view() == ExchangeView::Step(ExchangeStep::Done)
        })
        .await;
        assert!(phone.session.state().is_ready());
    }

    #[sqlx::test(migrator = "dirt_api::MIGRATOR")]
    async fn a_screen_rides_out_a_network_that_comes_and_goes(pool: PgPool) {
        let server = Server::with_sign_in(pool).await;
        let laptop = laptop_with_a_note(&server).await;
        let phone = waiting_device(&server, "Phone").await;
        let (approving, joining) = both_screens(&laptop, &phone, quick()).await;

        phone.transport.set_offline(true);
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(joining.view(), ExchangeView::Step(ExchangeStep::Waiting));
        phone.transport.set_offline(false);

        eventually("both screens show a code", || async {
            code_on(&approving).is_some() && code_on(&joining).is_some()
        })
        .await;
        assert_eq!(code_on(&approving), code_on(&joining));
    }
}
