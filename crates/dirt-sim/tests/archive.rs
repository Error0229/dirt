//! Archive, bringing a deleted note back, and a note emptied again, with
//! whole devices against the real server.
//!
//! Archiving is a change to a note that is not an edit. These tests are
//! about what follows from that: it syncs like any change, it moves no edit
//! time, it never brings a deleted note back, and two devices that archive,
//! bring back, edit and delete one note in any order end the same.
//!
//! Start the database with `docker compose up -d` at the repository root and
//! set `DATABASE_URL` as `compose.yaml` shows. Each test runs in its own
//! throwaway database.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dirt_core::api::BoxFuture;
use dirt_core::notes::NoteDoc;
use dirt_core::session::SessionState;
use dirt_core::store::LetGo;
use dirt_core::Store;
use dirt_proto::NoteId;
use dirt_sim::{Client, Device, Server};
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

/// What a device holds of a note, read from the note's document.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Standing {
    text: String,
    archived: bool,
    deleted: bool,
    edited_at: Option<i64>,
}

/// Every note a store holds, deleted ones included, by ID.
async fn standings(store: &Store) -> Vec<(NoteId, Standing)> {
    let mut held: Vec<(NoteId, Standing)> = store
        .note_documents()
        .await
        .unwrap()
        .into_iter()
        .map(|(id, bytes)| {
            // Reading needs no client ID of its own: nothing is written.
            let doc = NoteDoc::from_bytes(0, &bytes).unwrap();
            let standing = Standing {
                text: doc.text(),
                archived: doc.is_archived(),
                deleted: doc.is_deleted(),
                edited_at: doc.edited_at_ms(),
            };
            (id, standing)
        })
        .collect();
    held.sort_by_key(|(id, _)| id.to_string());
    held
}

async fn standing(store: &Store, id: &NoteId) -> Standing {
    standings(store)
        .await
        .into_iter()
        .find(|(held, _)| held == id)
        .map(|(_, standing)| standing)
        .expect("the device holds the note")
}

/// The IDs a list holds, in the list's order.
fn ids(notes: &[dirt_core::Note]) -> Vec<NoteId> {
    notes.iter().map(|note| note.id).collect()
}

/// That the rows a store shows agree with its documents: a note is in the
/// list, in the archive or in neither, as its document says.
async fn rows_agree_with_documents(store: &Store) {
    let listed = ids(&store.list_notes(100_000, 0).await.unwrap());
    let archived = ids(&store.list_archived_notes(100_000, 0).await.unwrap());
    for (id, standing) in standings(store).await {
        let live = !standing.deleted;
        assert_eq!(listed.contains(&id), live && !standing.archived, "{id}");
        assert_eq!(archived.contains(&id), live && standing.archived, "{id}");
        let row = store.get_note(&id).await.unwrap();
        assert_eq!(row.is_some(), live, "{id}");
        if let Some(row) = row {
            assert_eq!(row.content, standing.text);
            assert_eq!(row.archived, standing.archived);
            assert_eq!(Some(row.updated_at), standing.edited_at);
        }
    }
}

// ---- The basic path ----

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn an_archive_reaches_the_other_device_and_changes_nothing_else(pool: PgPool) {
    let server = Server::new(pool);
    let (laptop, phone) = two_devices(&server).await;
    let note = laptop
        .store
        .create_note("bread was bought #shop")
        .await
        .unwrap();
    settle(&[&laptop, &phone]).await;

    laptop.store.archive_note(&note.id).await.unwrap();
    // Not an edit, and to be sent all the same.
    assert_eq!(laptop.store.unsent_count().await.unwrap(), 1);
    let report = laptop.sync().await.unwrap();
    assert_eq!((report.pushed, report.unsent), (1, 0));
    let report = phone.sync().await.unwrap();
    assert_eq!(report.pulled, 1);

    // Out of the phone's list, in its archive, with the edit time it had.
    assert!(phone.store.list_notes(10, 0).await.unwrap().is_empty());
    let archived = phone.store.list_archived_notes(10, 0).await.unwrap();
    assert_eq!(ids(&archived), [note.id]);
    assert_eq!(archived[0].updated_at, note.updated_at);
    assert_eq!(archived[0].content, note.content);
    // Still found, and marked, and not waiting to be sent back.
    let found = phone.store.search_notes("bread", 10).await.unwrap();
    assert!(found[0].archived);
    assert_eq!(phone.store.unsent_count().await.unwrap(), 0);
    // The server holds one note, as before: it has seen nothing new.
    assert_eq!(server.blob_count().await, 1);

    // The phone brings it back, and the laptop's list has it again.
    phone.store.unarchive_note(&note.id).await.unwrap();
    settle(&[&phone, &laptop]).await;
    assert_eq!(laptop.store.list_notes(10, 0).await.unwrap(), [note]);
    assert_eq!(
        standings(&laptop.store).await,
        standings(&phone.store).await
    );
}

/// Alice's laptop and phone as whole apps, both in the account.
async fn two_apps(server: &Server) -> (Client, Client) {
    let laptop = Client::new(server, "Laptop", false);
    laptop.sign_in("alice").await.unwrap();
    let phone = Client::new(server, "Phone", false);
    let state = phone.sign_in("alice").await.unwrap();
    assert!(matches!(state, SessionState::WaitingForApproval { .. }));
    phone.approve_with_key(server, &laptop.account_key()).await;
    (laptop, phone)
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn an_archive_made_offline_is_pushed_when_the_device_is_back(pool: PgPool) {
    let server = Server::with_sign_in(pool).await;
    let (laptop, phone) = two_apps(&server).await;
    let note = laptop
        .session
        .store()
        .create_note("the meeting happened")
        .await
        .unwrap();
    laptop.session.sync_now().await.unwrap();
    phone.session.sync_now().await.unwrap();

    phone.transport.set_offline(true);
    phone.session.store().archive_note(&note.id).await.unwrap();
    assert!(phone.session.sync_now().await.is_err());
    // It waits, as an edit made offline does.
    assert_eq!(phone.session.store().unsent_count().await.unwrap(), 1);
    assert_eq!(laptop.session.sync_now().await.unwrap().pulled, 0);
    assert_eq!(laptop.notes().await.len(), 1);

    phone.transport.set_offline(false);
    let report = phone.session.sync_now().await.unwrap();
    assert_eq!((report.pushed, report.unsent), (1, 0));
    laptop.session.sync_now().await.unwrap();

    assert!(laptop.notes().await.is_empty(), "out of the laptop's list");
    let archived = laptop
        .session
        .store()
        .list_archived_notes(10, 0)
        .await
        .unwrap();
    assert_eq!(ids(&archived), [note.id]);
    assert_eq!(archived[0].updated_at, note.updated_at);
}

// ---- Every order ----

/// One thing a person does to a note.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Act {
    Archive,
    BringBack,
    Edit,
    Delete,
}

/// What one device does to the note while it has not heard from the other.
/// A delete comes last: a deleted note cannot be archived or edited.
const TURNS: &[&[Act]] = &[
    &[],
    &[Act::Archive],
    &[Act::BringBack],
    &[Act::Edit],
    &[Act::Delete],
    &[Act::Archive, Act::Edit],
    &[Act::Edit, Act::Archive],
    &[Act::Archive, Act::BringBack],
    &[Act::Archive, Act::Delete],
    &[Act::Edit, Act::Delete],
];

/// The last choice a turn makes about the archive, if it makes one. Asking
/// for what the note already is writes nothing, and is no choice.
fn choice(turn: &[Act], archived_before: bool) -> Option<bool> {
    let mut archived = archived_before;
    let mut chosen = None;
    for act in turn {
        let wanted = match act {
            Act::Archive => true,
            Act::BringBack => false,
            Act::Edit | Act::Delete => continue,
        };
        if archived != wanted {
            archived = wanted;
            chosen = Some(wanted);
        }
    }
    chosen
}

async fn take_turn(device: &Device, id: &NoteId, turn: &[Act], word: &str) {
    for act in turn {
        match act {
            Act::Archive => {
                device.store.archive_note(id).await.unwrap();
            }
            Act::BringBack => {
                device.store.unarchive_note(id).await.unwrap();
            }
            Act::Edit => {
                let text = device.store.get_note(id).await.unwrap().unwrap().content;
                device
                    .store
                    .update_note(id, &format!("{text} {word}"))
                    .await
                    .unwrap();
            }
            Act::Delete => device.store.delete_note(id).await.unwrap(),
        }
    }
}

/// One note for every pair of turns, from each of its two beginnings: in
/// the list, and archived.
struct Case {
    id: NoteId,
    began_archived: bool,
    edited_at: i64,
    laptop: &'static [Act],
    phone: &'static [Act],
}

/// Both devices take their turn at every note without hearing from each
/// other, then sync, one before the other. Whatever each did, and whoever
/// synced first, both end the same, and as the rules say.
///
/// Two hundred notes, and the store waits for the disk at every write, so
/// this takes a while: half a minute, and a few times that on a busy
/// machine.
async fn every_pair_of_turns_ends_the_same(server: &Server, phone_syncs_first: bool) {
    let (laptop, phone) = two_devices(server).await;
    let mut cases = Vec::new();
    for began_archived in [false, true] {
        for laptop_turn in TURNS {
            for phone_turn in TURNS {
                let note = laptop.store.create_note("thought").await.unwrap();
                if began_archived {
                    laptop.store.archive_note(&note.id).await.unwrap();
                }
                cases.push(Case {
                    id: note.id,
                    began_archived,
                    edited_at: note.updated_at,
                    laptop: laptop_turn,
                    phone: phone_turn,
                });
            }
        }
    }
    settle(&[&laptop, &phone]).await;
    assert_eq!(
        standings(&laptop.store).await,
        standings(&phone.store).await
    );
    // So that an edit's time differs from the time of the note's making.
    tokio::time::sleep(Duration::from_millis(5)).await;

    for case in &cases {
        take_turn(&laptop, &case.id, case.laptop, "laptop").await;
        take_turn(&phone, &case.id, case.phone, "phone").await;
    }
    if phone_syncs_first {
        settle(&[&phone, &laptop]).await;
    } else {
        settle(&[&laptop, &phone]).await;
    }

    // Read once: every note's document is read to answer.
    let held =
        |all: Vec<(NoteId, Standing)>| -> HashMap<NoteId, Standing> { all.into_iter().collect() };
    let on_the_laptop = held(standings(&laptop.store).await);
    let on_the_phone = held(standings(&phone.store).await);
    for case in &cases {
        let on_laptop = on_the_laptop[&case.id].clone();
        let on_phone = on_the_phone[&case.id].clone();
        let told = format!(
            "began archived: {}, laptop {:?}, phone {:?}",
            case.began_archived, case.laptop, case.phone
        );
        assert_eq!(on_laptop, on_phone, "{told}");

        let edited = |turn: &[Act]| turn.contains(&Act::Edit);
        let deleted = |turn: &[Act]| turn.contains(&Act::Delete);
        // A delete covers what the deleting device had seen. Only an edit
        // made elsewhere brings the note back: archiving, and bringing back
        // from the archive, never do.
        let stays_deleted = (deleted(case.laptop) && !edited(case.phone))
            || (deleted(case.phone) && !edited(case.laptop));
        assert_eq!(on_laptop.deleted, stays_deleted, "{told}");

        // One device's choice holds if only one chose. If both chose the
        // same, that. If they chose differently, one of the two, and the
        // devices already agree which.
        match (
            choice(case.laptop, case.began_archived),
            choice(case.phone, case.began_archived),
        ) {
            (None, None) => assert_eq!(on_laptop.archived, case.began_archived, "{told}"),
            (Some(chosen), None) | (None, Some(chosen)) => {
                assert_eq!(on_laptop.archived, chosen, "{told}");
            }
            (Some(one), Some(other)) if one == other => {
                assert_eq!(on_laptop.archived, one, "{told}");
            }
            (Some(_), Some(_)) => {}
        }

        // Every edit is in the text, and nothing else moved the edit time.
        for (turn, word) in [(case.laptop, "laptop"), (case.phone, "phone")] {
            assert_eq!(on_laptop.text.contains(word), edited(turn), "{told}");
        }
        if edited(case.laptop) || edited(case.phone) {
            assert!(on_laptop.edited_at > Some(case.edited_at), "{told}");
        } else {
            assert_eq!(on_laptop.text, "thought", "{told}");
            assert_eq!(on_laptop.edited_at, Some(case.edited_at), "{told}");
        }
    }

    for device in [&laptop, &phone] {
        rows_agree_with_documents(&device.store).await;
        assert_eq!(device.store.unsent_count().await.unwrap(), 0);
        assert!(device.store.note_problems().await.unwrap().is_empty());
    }
    // No act made a second note of one.
    assert_eq!(
        server.blob_count().await,
        i64::try_from(cases.len()).unwrap()
    );
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn archiving_editing_and_deleting_in_every_order_end_the_same_laptop_first(pool: PgPool) {
    every_pair_of_turns_ends_the_same(&Server::new(pool), false).await;
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn archiving_editing_and_deleting_in_every_order_end_the_same_phone_first(pool: PgPool) {
    every_pair_of_turns_ends_the_same(&Server::new(pool), true).await;
}

/// Three rounds on one note, each device answering the other's last word.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_note_archived_back_and_forth_between_two_devices_ends_the_same(pool: PgPool) {
    let server = Server::new(pool);
    let (laptop, phone) = two_devices(&server).await;
    let note = laptop.store.create_note("back and forth").await.unwrap();
    settle(&[&laptop, &phone]).await;

    // Round one: both archive, and the laptop changes its mind.
    laptop.store.archive_note(&note.id).await.unwrap();
    laptop.store.unarchive_note(&note.id).await.unwrap();
    phone.store.archive_note(&note.id).await.unwrap();
    settle(&[&laptop, &phone]).await;
    let after_one = standing(&laptop.store, &note.id).await;
    assert_eq!(after_one, standing(&phone.store, &note.id).await);

    // Round two: each sets the note to the opposite of what round one left,
    // and to the opposite again. Only the phone's last word differs.
    let held = after_one.archived;
    for device in [&laptop, &phone] {
        if held {
            device.store.unarchive_note(&note.id).await.unwrap();
        } else {
            device.store.archive_note(&note.id).await.unwrap();
        }
    }
    if held {
        phone.store.archive_note(&note.id).await.unwrap();
    } else {
        phone.store.unarchive_note(&note.id).await.unwrap();
    }
    settle(&[&phone, &laptop]).await;
    assert_eq!(
        standing(&laptop.store, &note.id).await,
        standing(&phone.store, &note.id).await
    );

    // Round three: with both agreed, one device's choice is the answer.
    let now = standing(&laptop.store, &note.id).await.archived;
    if now {
        laptop.store.unarchive_note(&note.id).await.unwrap();
    } else {
        laptop.store.archive_note(&note.id).await.unwrap();
    }
    settle(&[&laptop, &phone]).await;
    let last = standing(&phone.store, &note.id).await;
    assert_eq!(last.archived, !now);
    assert_eq!(last.edited_at, Some(note.updated_at), "never an edit");
    assert_eq!(last, standing(&laptop.store, &note.id).await);
}

// ---- Bringing a deleted note back ----

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_delete_is_undone_from_either_device_and_an_archived_note_comes_back_archived(
    pool: PgPool,
) {
    let server = Server::new(pool);
    let (laptop, phone) = two_devices(&server).await;
    let listed = laptop
        .store
        .create_note("deleted by mistake")
        .await
        .unwrap();
    let archived = laptop.store.create_note("archived, deleted").await.unwrap();
    laptop.store.archive_note(&archived.id).await.unwrap();
    settle(&[&laptop, &phone]).await;

    laptop.store.delete_note(&listed.id).await.unwrap();
    laptop.store.delete_note(&archived.id).await.unwrap();
    settle(&[&laptop, &phone]).await;
    assert_eq!(phone.store.note_count().await.unwrap(), 0);

    // Undone where it was done.
    laptop.store.restore_note(&listed.id).await.unwrap();
    // Undone on the other device, which holds the deleted note too.
    phone.store.restore_note(&archived.id).await.unwrap();
    settle(&[&laptop, &phone]).await;

    for device in [&laptop, &phone] {
        let list = device.store.list_notes(10, 0).await.unwrap();
        assert_eq!(ids(&list), [listed.id]);
        assert_eq!(list[0].content, "deleted by mistake");
        // Back with the edit time it had, on both devices: into its place
        // in the list, not at the top.
        assert_eq!(list[0].updated_at, listed.updated_at);
        let archive = device.store.list_archived_notes(10, 0).await.unwrap();
        assert_eq!(ids(&archive), [archived.id]);
        assert_eq!(archive[0].updated_at, archived.updated_at);
        rows_agree_with_documents(&device.store).await;
    }
    assert_eq!(
        standings(&laptop.store).await,
        standings(&phone.store).await
    );
}

// ---- Archive and purge ----

/// The same app, started again as one that purges at once. The apps wait
/// thirty days; nothing else about them differs.
fn impatient(client: Client) -> Client {
    client.restarted(|config| config.timing.purge_after = Duration::ZERO)
}

/// A phone that was away while a note was deleted and, in time, purged.
/// It archives the note without having heard, which is not an edit: there
/// is nothing in its copy that the delete did not cover. The note must not
/// come back, as it would if the phone had written in it.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn archiving_a_note_that_was_purged_meanwhile_does_not_bring_it_back(pool: PgPool) {
    let server = Server::with_sign_in(pool).await;
    let (laptop, phone) = two_apps(&server).await;
    let note = laptop
        .session
        .store()
        .create_note("should not exist")
        .await
        .unwrap();
    laptop.session.sync_now().await.unwrap();
    phone.session.sync_now().await.unwrap();

    laptop.session.store().delete_note(&note.id).await.unwrap();
    laptop.session.sync_now().await.unwrap();
    let laptop = impatient(laptop);
    assert_eq!(laptop.session.sync_now().await.unwrap().purged, 1);

    phone.session.store().archive_note(&note.id).await.unwrap();
    assert_eq!(phone.session.store().unsent_count().await.unwrap(), 1);
    phone.session.sync_now().await.unwrap();
    laptop.session.sync_now().await.unwrap();

    for device in [&laptop, &phone] {
        let store = device.session.store();
        assert!(
            store.note_documents().await.unwrap().is_empty(),
            "the note is gone from every device, and no copy was made of it"
        );
        assert_eq!(store.unsent_count().await.unwrap(), 0);
    }
    assert_eq!(server.blob_count().await, 0);
}

// ---- A note emptied again ----

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_note_emptied_before_it_was_sent_never_reaches_the_server(pool: PgPool) {
    let server = Server::new(pool);
    let (laptop, phone) = two_devices(&server).await;
    let note = laptop.store.create_note("h").await.unwrap();
    laptop.store.update_note(&note.id, "").await.unwrap();

    assert_eq!(
        laptop.store.let_go_if_empty(&note.id).await.unwrap(),
        LetGo::Dropped
    );

    let report = laptop.sync().await.unwrap();
    assert_eq!((report.pushed, report.unsent), (0, 0));
    assert_eq!(phone.sync().await.unwrap().pulled, 0);
    assert_eq!(server.blob_count().await, 0);
    assert!(laptop.store.note_documents().await.unwrap().is_empty());
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_note_that_was_pushed_then_emptied_offline_is_deleted_on_every_device(pool: PgPool) {
    let server = Server::new(pool);
    let (laptop, phone) = two_devices(&server).await;
    let kept = laptop.store.create_note("a real thought").await.unwrap();
    let note = laptop.store.create_note("half a thou").await.unwrap();
    settle(&[&laptop, &phone]).await;
    assert_eq!(phone.notes().await.len(), 2);

    // The laptop is offline: nothing syncs between these and the sync
    // below.
    laptop.store.update_note(&note.id, "").await.unwrap();
    assert_eq!(
        laptop.store.let_go_if_empty(&note.id).await.unwrap(),
        LetGo::Deleted
    );
    assert_eq!(phone.notes().await.len(), 2, "the phone has not heard");

    settle(&[&laptop, &phone]).await;

    for device in [&laptop, &phone] {
        assert_eq!(
            ids(&device.store.list_notes(10, 0).await.unwrap()),
            [kept.id]
        );
        // Deleted like any other note: the document is held, marked.
        assert!(standing(&device.store, &note.id).await.deleted);
    }
}

/// The note was made at the first character, a sync began, and the person
/// took the character back and left while the push was on its way. The
/// server has the note, so it is deleted, and it comes back nowhere.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_note_emptied_while_its_first_push_is_under_way_does_not_come_back(pool: PgPool) {
    let server = Server::new(pool);
    let (laptop, phone) = two_devices(&server).await;
    let note = laptop.store.create_note("h").await.unwrap();

    let store = laptop.store.clone();
    let id = note.id;
    let outcome = Arc::new(Mutex::new(None));
    let seen = Arc::clone(&outcome);
    laptop.before_next("/v1/notes/push", move || -> BoxFuture<'static, ()> {
        Box::pin(async move {
            store.update_note(&id, "").await.unwrap();
            let let_go = store.let_go_if_empty(&id).await.unwrap();
            *seen.lock().unwrap() = Some(let_go);
        })
    });
    let report = laptop.sync().await.unwrap();

    assert_eq!(
        *outcome.lock().unwrap(),
        Some(LetGo::Deleted),
        "a note on its way to the server is not simply forgotten"
    );
    assert_eq!(report.pushed, 1);
    assert_eq!(report.unsent, 1, "the delete is still to be sent");

    settle(&[&laptop, &phone]).await;
    for device in [&laptop, &phone] {
        assert!(device.notes().await.is_empty());
        assert!(standing(&device.store, &note.id).await.deleted);
    }
}

/// The delete that lets an emptied note go covers what this device had
/// seen, like any delete. What another device wrote in the note meanwhile
/// is a thought, and stays.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_note_emptied_here_and_written_in_elsewhere_keeps_what_was_written(pool: PgPool) {
    let server = Server::new(pool);
    let (laptop, phone) = two_devices(&server).await;
    let note = laptop.store.create_note("milk").await.unwrap();
    settle(&[&laptop, &phone]).await;

    laptop.store.update_note(&note.id, "").await.unwrap();
    assert_eq!(
        laptop.store.let_go_if_empty(&note.id).await.unwrap(),
        LetGo::Deleted
    );
    phone
        .store
        .update_note(&note.id, "milk and eggs")
        .await
        .unwrap();
    settle(&[&laptop, &phone]).await;

    assert_eq!(laptop.notes().await, phone.notes().await);
    let notes = laptop.notes().await;
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].1.trim(), "and eggs");
}
