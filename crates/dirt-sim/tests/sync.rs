//! Several real client cores syncing through the real server.
//!
//! Start the database with `docker compose up -d` at the repository root and
//! set `DATABASE_URL` as `compose.yaml` shows. Each test runs in its own
//! throwaway database.

use dirt_core::api::BoxFuture;
use dirt_core::store::ProblemKind;
use dirt_proto::api::{DeviceStatus, ErrorClass};
use dirt_sim::{Device, Server};
use sqlx::PgPool;

/// Two devices of one account: the first makes the key, the second is
/// approved and given it.
async fn two_devices(server: &Server) -> (Device, Device) {
    let first = Device::sign_in(server, "alice", "Laptop", None).await;
    let second = Device::sign_in(server, "alice", "Phone", Some(&first.key)).await;
    (first, second)
}

/// Syncs each device in turn until one full pass changes nothing.
async fn settle(devices: &[&Device]) {
    for _ in 0..10 {
        let mut quiet = true;
        for device in devices {
            let report = device.sync_until_settled().await;
            quiet &= report.pulled == 0 && report.pushed == 0;
        }
        if quiet {
            return;
        }
    }
    panic!("the devices did not settle");
}

async fn stored_blobs(pool: &PgPool) -> Vec<Vec<u8>> {
    sqlx::query_scalar("SELECT ciphertext FROM notes ORDER BY change_number")
        .fetch_all(pool)
        .await
        .unwrap()
}

// ---- The basic path ----

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_note_written_on_one_device_appears_on_the_other(pool: PgPool) {
    let server = Server::new(pool.clone());
    let (laptop, phone) = two_devices(&server).await;
    laptop
        .store
        .create_note("call the plumber #home")
        .await
        .unwrap();

    let report = laptop.sync().await.unwrap();
    assert_eq!((report.pushed, report.unsent), (1, 0));
    let report = phone.sync().await.unwrap();
    assert_eq!(report.pulled, 1);

    assert_eq!(phone.notes().await, laptop.notes().await);
    assert_eq!(phone.notes().await[0].1, "call the plumber #home");
    // The pulled note is searchable and tagged on the second device, and it
    // is not waiting to be sent back.
    assert_eq!(
        phone.store.search_notes("plumber", 10).await.unwrap().len(),
        1
    );
    assert_eq!(
        phone.store.list_tags().await.unwrap(),
        vec![("home".to_owned(), 1)]
    );
    assert_eq!(phone.store.unsent_count().await.unwrap(), 0);
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn the_server_holds_nothing_it_can_read(pool: PgPool) {
    let server = Server::new(pool.clone());
    let laptop = Device::sign_in(&server, "alice", "Laptop", None).await;
    let secret = "the surprise party is on Friday";
    laptop.store.create_note(secret).await.unwrap();
    laptop.sync().await.unwrap();

    let blobs = stored_blobs(&pool).await;
    assert_eq!(blobs.len(), 1);
    for word in ["surprise", "party", "Friday"] {
        assert!(
            !blobs[0]
                .windows(word.len())
                .any(|window| window == word.as_bytes()),
            "the server's blob contains {word:?}"
        );
    }
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_sync_with_nothing_to_do_changes_nothing(pool: PgPool) {
    let server = Server::new(pool.clone());
    let laptop = Device::sign_in(&server, "alice", "Laptop", None).await;
    laptop.store.create_note("once").await.unwrap();
    laptop.sync().await.unwrap();
    // The cycle after a push pulls the device's own change back once,
    // which moves its cursor past it.
    laptop.sync().await.unwrap();

    let report = laptop.sync().await.unwrap();
    assert_eq!(
        (report.pulled, report.pushed, report.stale, report.problems),
        (0, 0, 0, 0)
    );
    assert_eq!(stored_blobs(&pool).await.len(), 1);
}

// ---- Merging ----

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn edits_made_offline_on_two_devices_both_survive(pool: PgPool) {
    let server = Server::new(pool);
    let (laptop, phone) = two_devices(&server).await;
    let note = laptop.store.create_note("buy milk today").await.unwrap();
    settle(&[&laptop, &phone]).await;

    // Both devices edit the same note without syncing in between.
    laptop
        .store
        .update_note(&note.id, "buy bread today")
        .await
        .unwrap();
    phone
        .store
        .update_note(&note.id, "buy milk tomorrow")
        .await
        .unwrap();

    // The laptop pushes first. The phone's push is then stale: it merges
    // what the server returned and pushes the result.
    laptop.sync().await.unwrap();
    let report = phone.sync().await.unwrap();
    assert!(report.pulled >= 1 && report.pushed == 1, "{report:?}");
    settle(&[&laptop, &phone]).await;

    assert_eq!(laptop.notes().await, phone.notes().await);
    assert_eq!(laptop.notes().await[0].1, "buy bread tomorrow");
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_delete_reaches_the_other_device_and_an_unseen_edit_brings_the_note_back(pool: PgPool) {
    let server = Server::new(pool);
    let (laptop, phone) = two_devices(&server).await;
    let kept = laptop.store.create_note("stays deleted").await.unwrap();
    let revived = laptop.store.create_note("comes back").await.unwrap();
    settle(&[&laptop, &phone]).await;

    // The laptop deletes both. The phone, not knowing, edits one of them.
    laptop.store.delete_note(&kept.id).await.unwrap();
    laptop.store.delete_note(&revived.id).await.unwrap();
    phone
        .store
        .update_note(&revived.id, "comes back, edited")
        .await
        .unwrap();
    settle(&[&laptop, &phone]).await;

    let expected = vec![(revived.id.to_string(), "comes back, edited".to_owned())];
    assert_eq!(laptop.notes().await, expected);
    assert_eq!(phone.notes().await, expected);
}

/// A small deterministic generator, so that a failure can be reproduced.
struct Rng(u64);

impl Rng {
    fn next(&mut self, bound: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        usize::try_from(self.0 >> 33).unwrap() % bound
    }
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn three_devices_with_random_edits_and_deletes_converge(pool: PgPool) {
    let server = Server::new(pool);
    let first = Device::sign_in(&server, "alice", "Laptop", None).await;
    let second = Device::sign_in(&server, "alice", "Phone", Some(&first.key)).await;
    let third = Device::sign_in(&server, "alice", "Tablet", Some(&first.key)).await;
    let devices = [&first, &second, &third];
    let words = ["alpha", "beta", "gamma", "delta", "epsilon"];

    for seed in [7, 99] {
        let mut rng = Rng(seed);
        for step in 0..60 {
            let device = devices[rng.next(3)];
            let notes = device.store.list_notes(100, 0).await.unwrap();
            match rng.next(10) {
                // Create.
                0..=2 => {
                    device
                        .store
                        .create_note(&format!("note {seed}-{step} {}", words[rng.next(5)]))
                        .await
                        .unwrap();
                }
                // Edit: change one word, add one, or drop one.
                3..=6 if !notes.is_empty() => {
                    let note = &notes[rng.next(notes.len())];
                    let mut parts: Vec<&str> = note.content.split(' ').collect();
                    match rng.next(3) {
                        0 => {
                            let at = rng.next(parts.len());
                            parts[at] = words[rng.next(5)];
                        }
                        1 => parts.insert(rng.next(parts.len() + 1), words[rng.next(5)]),
                        _ if parts.len() > 1 => {
                            parts.remove(rng.next(parts.len()));
                        }
                        _ => {}
                    }
                    device
                        .store
                        .update_note(&note.id, &parts.join(" "))
                        .await
                        .unwrap();
                }
                // Delete.
                7 if !notes.is_empty() => {
                    let note = &notes[rng.next(notes.len())];
                    device.store.delete_note(&note.id).await.unwrap();
                }
                // Sync one device, at a random moment.
                _ => {
                    device.sync().await.unwrap();
                }
            }
        }
        settle(&devices).await;

        let reference = first.notes().await;
        assert_eq!(second.notes().await, reference, "seed {seed}");
        assert_eq!(third.notes().await, reference, "seed {seed}");
        for device in devices {
            assert_eq!(device.store.unsent_count().await.unwrap(), 0);
            assert!(device.store.note_problems().await.unwrap().is_empty());
        }
    }
    assert!(
        !first.notes().await.is_empty(),
        "the run should leave some notes to compare"
    );
}

// ---- Edits and interruptions during a cycle ----

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn an_edit_made_while_a_push_is_in_flight_is_sent_by_the_next_cycle(pool: PgPool) {
    let server = Server::new(pool);
    let (laptop, phone) = two_devices(&server).await;
    let note = laptop.store.create_note("first draft").await.unwrap();

    // Just before the push reaches the server, the person types some more.
    let store = laptop.store.clone();
    let id = note.id;
    laptop.before_next("/v1/notes/push", move || -> BoxFuture<'static, ()> {
        Box::pin(async move {
            store
                .update_note(&id, "first draft, continued")
                .await
                .unwrap();
        })
    });

    let report = laptop.sync().await.unwrap();
    assert_eq!(report.pushed, 1);
    assert_eq!(
        report.unsent, 1,
        "the later edit must still count as unsent"
    );

    // The server has the version that was sent, not the later one.
    phone.sync().await.unwrap();
    assert_eq!(phone.notes().await[0].1, "first draft");

    settle(&[&laptop, &phone]).await;
    assert_eq!(phone.notes().await[0].1, "first draft, continued");
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_cycle_cut_off_after_any_request_can_simply_run_again(pool: PgPool) {
    let server = Server::new(pool.clone());
    let (laptop, phone) = two_devices(&server).await;
    laptop.store.create_note("one").await.unwrap();
    laptop.store.create_note("two").await.unwrap();

    // The server stores the push, but the answer never arrives.
    laptop.lose_next_answer("/v1/notes/push");
    let error = laptop.sync().await.unwrap_err();
    assert_eq!(error.class(), ErrorClass::TryAgainLater);
    assert_eq!(
        stored_blobs(&pool).await.len(),
        2,
        "the server did store them"
    );
    assert_eq!(laptop.store.unsent_count().await.unwrap(), 2);

    // Running again repairs it: the pull brings back what was stored.
    laptop.sync_until_settled().await;
    assert_eq!(laptop.store.unsent_count().await.unwrap(), 0);

    // The same for a pull whose answer is lost.
    phone.lose_next_answer("/v1/notes/pull");
    assert!(phone.sync().await.is_err());
    phone.sync_until_settled().await;

    assert_eq!(phone.notes().await, laptop.notes().await);
    assert_eq!(laptop.notes().await.len(), 2, "no note was duplicated");
    let (rows,): (i64,) = sqlx::query_as("SELECT count(*) FROM notes")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rows, 2);
}

// ---- A server that cannot be trusted ----

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_blob_that_will_not_decrypt_holds_that_note_and_the_rest_sync(pool: PgPool) {
    let server = Server::new(pool.clone());
    let (laptop, phone) = two_devices(&server).await;
    let damaged = laptop.store.create_note("will be damaged").await.unwrap();
    laptop.store.create_note("stays fine").await.unwrap();
    laptop.sync().await.unwrap();

    // One blob is changed on the server.
    sqlx::query(
        "UPDATE notes SET ciphertext = set_byte(ciphertext, 40, get_byte(ciphertext, 40) # 1) \
          WHERE note_id = $1",
    )
    .bind(damaged.id.as_uuid())
    .execute(&pool)
    .await
    .unwrap();

    let report = phone.sync().await.unwrap();
    assert_eq!((report.pulled, report.problems), (1, 1));
    assert_eq!(phone.notes().await.len(), 1);
    assert_eq!(phone.notes().await[0].1, "stays fine");
    let problems = phone.store.note_problems().await.unwrap();
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0].note_id, damaged.id);
    assert_eq!(problems[0].kind, ProblemKind::Held);

    // The problem does not go away by itself, and the phone never pushes
    // over the copy it could not read.
    phone.sync().await.unwrap();
    assert_eq!(phone.store.note_problems().await.unwrap().len(), 1);

    // A good copy from the laptop ends the problem.
    laptop
        .store
        .update_note(&damaged.id, "will be damaged, then repaired")
        .await
        .unwrap();
    laptop.sync_until_settled().await;
    phone.sync_until_settled().await;
    assert!(phone.store.note_problems().await.unwrap().is_empty());
    assert_eq!(phone.notes().await, laptop.notes().await);
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_server_that_replays_an_older_version_is_refused(pool: PgPool) {
    let server = Server::new(pool.clone());
    let laptop = Device::sign_in(&server, "alice", "Laptop", None).await;
    let note = laptop.store.create_note("version one").await.unwrap();
    laptop.sync().await.unwrap();
    let old_blob = stored_blobs(&pool).await.remove(0);
    laptop
        .store
        .update_note(&note.id, "version two")
        .await
        .unwrap();
    laptop.sync().await.unwrap();
    assert_eq!(
        laptop.store.note_server_change(&note.id).await.unwrap(),
        Some(2)
    );

    // The server now answers a pull with the first version again, under its
    // old change number. The blob is genuine, so it decrypts.
    let replayed = serde_json::json!({
        "note_id": note.id, "change": 1, "key_version": 1, "format_version": 1,
        "ciphertext": base64_text(&old_blob),
    });
    laptop.rewrite_next_answer("/v1/notes/pull", move |answer| {
        answer["items"] = serde_json::json!([replayed]);
    });
    let report = laptop.sync().await.unwrap();

    assert_eq!(report.problems, 1);
    let problems = laptop.store.note_problems().await.unwrap();
    assert!(
        problems[0].reason.contains("after change 2"),
        "{problems:?}"
    );
    assert_eq!(laptop.notes().await[0].1, "version two");
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_push_answer_with_a_result_missing_marks_only_the_notes_it_names(pool: PgPool) {
    let server = Server::new(pool);
    let (laptop, phone) = two_devices(&server).await;
    laptop.store.create_note("first").await.unwrap();
    laptop.store.create_note("second").await.unwrap();

    // The server stores both notes but its answer names only the second.
    laptop.rewrite_next_answer("/v1/notes/push", |answer| {
        answer["results"].as_array_mut().unwrap().remove(0);
    });
    let report = laptop.sync().await.unwrap();
    assert_eq!((report.pushed, report.unsent), (1, 1));

    // The note that got no answer is sent again and settles; nothing is
    // recorded against the wrong note along the way.
    settle(&[&laptop, &phone]).await;
    assert!(laptop.store.note_problems().await.unwrap().is_empty());
    assert_eq!(laptop.store.unsent_count().await.unwrap(), 0);
    assert_eq!(phone.notes().await.len(), 2);
    assert_eq!(phone.notes().await, laptop.notes().await);
}

fn base64_text(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

// ---- Starting over ----

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_new_epoch_makes_devices_merge_everything_again_and_lose_nothing(pool: PgPool) {
    let server = Server::new(pool.clone());
    let (laptop, phone) = two_devices(&server).await;
    laptop
        .store
        .create_note("written on the laptop")
        .await
        .unwrap();
    phone
        .store
        .create_note("written on the phone")
        .await
        .unwrap();
    settle(&[&laptop, &phone]).await;
    let before = laptop.notes().await;
    assert_eq!(before.len(), 2);

    // The server's database is restored from a backup taken before either
    // note existed, and the account's epoch is raised as the deploy guide
    // says. Change numbers start again from zero.
    sqlx::query("DELETE FROM notes")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE accounts SET epoch = epoch + 1, change_counter = 0, note_count = 0, bytes_used = 0",
    )
    .execute(&pool)
    .await
    .unwrap();
    laptop
        .store
        .create_note("written after the restore")
        .await
        .unwrap();

    settle(&[&laptop, &phone]).await;

    assert_eq!(laptop.notes().await.len(), 3);
    assert_eq!(phone.notes().await, laptop.notes().await);
    for device in [&laptop, &phone] {
        assert_eq!(device.store.sync_progress().await.unwrap().epoch, 2);
        assert!(device.store.note_problems().await.unwrap().is_empty());
    }
    assert_eq!(
        stored_blobs(&pool).await.len(),
        3,
        "the server has them again"
    );
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_device_restored_from_a_backup_loses_no_edit(pool: PgPool) {
    let server = Server::new(pool);
    let (laptop, phone) = two_devices(&server).await;
    let note = laptop.store.create_note("one").await.unwrap();
    laptop.sync().await.unwrap();

    // A backup of the laptop's database is taken here.
    let (laptop, backup) = laptop.snapshot();

    // The laptop carries on: two more edits reach the server.
    laptop.store.update_note(&note.id, "one two").await.unwrap();
    laptop.sync().await.unwrap();
    laptop
        .store
        .update_note(&note.id, "one two three")
        .await
        .unwrap();
    laptop.sync().await.unwrap();

    // The laptop is restored from the backup. It has forgotten two of its
    // own edits, which the server still holds.
    let laptop = laptop.restored_from(&backup);
    assert_eq!(laptop.notes().await[0].1, "one");
    laptop.sync().await.unwrap();
    assert_eq!(laptop.notes().await[0].1, "one two three");

    // An edit made after the restore must not collide with the forgotten
    // ones: both devices end with it.
    laptop
        .store
        .update_note(&note.id, "one two three four")
        .await
        .unwrap();
    settle(&[&laptop, &phone]).await;
    assert_eq!(laptop.notes().await[0].1, "one two three four");
    assert_eq!(phone.notes().await, laptop.notes().await);
}

// ---- Accounts ----

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn two_accounts_never_exchange_notes(pool: PgPool) {
    let server = Server::new(pool);
    let alice = Device::sign_in(&server, "alice", "Laptop", None).await;
    let bob = Device::sign_in(&server, "bob", "Laptop", None).await;
    alice.store.create_note("alice's note").await.unwrap();
    bob.store.create_note("bob's note").await.unwrap();

    settle(&[&alice, &bob]).await;

    assert_eq!(alice.notes().await.len(), 1);
    assert_eq!(alice.notes().await[0].1, "alice's note");
    assert_eq!(bob.notes().await[0].1, "bob's note");
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_device_with_the_wrong_key_reads_nothing_and_overwrites_nothing(pool: PgPool) {
    let server = Server::new(pool.clone());
    let laptop = Device::sign_in(&server, "alice", "Laptop", None).await;
    let note = laptop
        .store
        .create_note("only for holders of the key")
        .await
        .unwrap();
    laptop.sync().await.unwrap();

    // A second device is approved but holds a different key, as it would if
    // the key hand-over had gone wrong.
    let wrong_key = dirt_core::crypto::AccountKey::generate();
    let intruder = Device::sign_in(&server, "alice", "Phone", Some(&wrong_key)).await;
    let report = intruder.sync().await.unwrap();

    assert_eq!(report.problems, 1);
    assert!(intruder.notes().await.is_empty());
    assert_eq!(
        intruder.store.note_problems().await.unwrap()[0].note_id,
        note.id
    );
    // The laptop's blob is untouched.
    assert_eq!(stored_blobs(&pool).await.len(), 1);
    laptop.sync().await.unwrap();
    assert_eq!(laptop.notes().await[0].1, "only for holders of the key");
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_device_whose_token_expired_signs_in_again_and_carries_on(pool: PgPool) {
    let server = Server::new(pool);
    let (laptop, phone) = two_devices(&server).await;
    phone.store.create_note("written first").await.unwrap();
    settle(&[&laptop, &phone]).await;

    // The phone sits in a drawer past the limit, then a note is edited on it.
    server.expire(phone.registration.device_id).await;
    let note = phone.store.list_notes(10, 0).await.unwrap().remove(0).id;
    phone
        .store
        .update_note(&note, "written first, edited in the drawer")
        .await
        .unwrap();
    let error = phone.sync().await.unwrap_err();
    assert_eq!(error.class(), ErrorClass::StopUntilSignedIn);
    assert_eq!(phone.store.unsent_count().await.unwrap(), 1);

    let old = phone.registration.clone();
    let phone = phone.sign_in_again("alice").await;

    // The same device, still approved, with its store and what was unsent.
    assert_eq!(phone.registration.device_id, old.device_id);
    assert_eq!(phone.registration.status, DeviceStatus::Approved);
    assert_ne!(phone.registration.token, old.token);
    settle(&[&phone, &laptop]).await;
    assert_eq!(
        laptop.notes().await[0].1,
        "written first, edited in the drawer"
    );
}
