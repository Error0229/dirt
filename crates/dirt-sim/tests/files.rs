//! Files attached to notes, with whole devices against the real server and
//! a real file storage.
//!
//! A file's bytes go from a device to the storage and back, never through
//! the server. What the server keeps is which notes refer to which files,
//! and it learns that from the devices. These tests are about a file
//! reaching the other devices, and about it never being thrown away while
//! a note still names it.
//!
//! Start the database and the file storage with `docker compose up -d` at
//! the repository root and set `DATABASE_URL` as `compose.yaml` shows.

use std::path::PathBuf;
use std::time::Duration;

use dirt_core::files::seal::sealed_size;
use dirt_core::files::Attachment;
use dirt_core::session::SessionState;
use dirt_core::sync::engine::FileError;
use dirt_core::sync::worker::Trigger;
use dirt_proto::api::limits;
use dirt_proto::NoteId;
use dirt_sim::{eventually, file_bucket, file_store, Client, Server};
use sqlx::PgPool;
use uuid::Uuid;

/// Alice's laptop and phone, both in the account.
async fn two_devices(server: &Server) -> (Client, Client) {
    let laptop = Client::new(server, "Laptop", false);
    laptop.sign_in("alice").await.unwrap();
    let phone = another_device(server, &laptop, "Phone").await;
    (laptop, phone)
}

async fn another_device(server: &Server, first: &Client, name: &str) -> Client {
    let device = Client::new(server, name, false);
    let state = device.sign_in("alice").await.unwrap();
    assert!(matches!(state, SessionState::WaitingForApproval { .. }));
    device.approve_with_key(server, &first.account_key()).await;
    device
}

/// The same app, started again as one that does not wait: it purges a
/// deleted note at once, tells the server at once that a note has stopped
/// naming a file, and lets go at once of a file nothing needs. The apps
/// wait thirty days, thirty days and an hour.
fn impatient(client: Client) -> Client {
    client.restarted(|config| {
        config.timing.purge_after = Duration::ZERO;
        config.timing.spare_file_wait = Duration::ZERO;
    })
}

/// A photograph: more than one piece when it is encrypted, and not all
/// the same byte.
fn photograph() -> Vec<u8> {
    (0..150_000u32)
        .map(|i| u8::try_from(i % 251).unwrap())
        .collect()
}

/// Writes a note and attaches a file with these bytes to it.
async fn note_with_file(client: &Client, text: &str, bytes: &[u8]) -> (NoteId, Attachment) {
    let store = client.session.store();
    let note = store.create_note(text).await.unwrap().id;
    let source = client.folder().join(format!("{}.jpg", Uuid::now_v7()));
    std::fs::write(&source, bytes).unwrap();
    let attachment = store
        .attach_file(&note, &source, "whiteboard.jpg", "image/jpeg")
        .await
        .unwrap();
    std::fs::remove_file(&source).unwrap();
    (note, attachment)
}

fn holds(client: &Client, file: Uuid) -> bool {
    client.session.store().shelf().holds(file)
}

async fn attached(client: &Client, note: &NoteId) -> Vec<Attachment> {
    client.session.store().note_attachments(note).await.unwrap()
}

/// Fetches the file as an app does, and reads it.
async fn read(client: &Client, note: &NoteId, file: Uuid) -> Vec<u8> {
    let path = client.session.fetch_file(note, file).await.unwrap();
    std::fs::read(path).unwrap()
}

async fn sync(client: &Client) -> dirt_core::sync::engine::SyncReport {
    client.session.sync_now().await.unwrap()
}

/// What the server holds about files: files it counts as there, references
/// from notes, files with a date to be deleted on, and the bytes counted
/// against the account.
async fn on_the_server(pool: &PgPool) -> (i64, i64, i64, i64) {
    sqlx::query_as(
        "SELECT (SELECT count(*) FROM files WHERE confirmed), \
                (SELECT count(*) FROM file_notes), \
                (SELECT count(*) FROM files WHERE delete_after IS NOT NULL), \
                (SELECT COALESCE(sum(file_bytes), 0)::bigint FROM accounts)",
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

/// The notes the server knows to refer to a file.
async fn referring_notes(pool: &PgPool, file: Uuid) -> Vec<Uuid> {
    sqlx::query_scalar("SELECT note_id FROM file_notes WHERE file_id = $1 ORDER BY note_id")
        .bind(file)
        .fetch_all(pool)
        .await
        .unwrap()
}

async fn object_key(pool: &PgPool, file: Uuid) -> String {
    let account: Uuid = sqlx::query_scalar("SELECT id FROM accounts")
        .fetch_one(pool)
        .await
        .unwrap();
    dirt_api::FileStore::object_key(account, file)
}

/// What the storage holds for a file, as anyone with a link would see it.
async fn stored(pool: &PgPool, file: Uuid) -> Option<Vec<u8>> {
    let link = file_store().download_link(&object_key(pool, file).await);
    let answer = reqwest::get(link).await.unwrap();
    if answer.status().is_success() {
        Some(answer.bytes().await.unwrap().to_vec())
    } else {
        None
    }
}

fn sealed(plain: &[u8]) -> i64 {
    i64::try_from(sealed_size(plain.len() as u64)).unwrap()
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_file_attached_on_one_device_is_read_on_the_other(pool: PgPool) {
    file_bucket().await;
    let server = Server::with_sign_in(pool.clone()).await;
    let (laptop, phone) = two_devices(&server).await;
    let photo = photograph();
    let (note, attachment) = note_with_file(&laptop, "the whiteboard", &photo).await;

    let report = sync(&laptop).await;

    assert_eq!(
        (report.files_sent, report.files_unsent, report.files_failed),
        (1, 0, false)
    );
    assert_eq!(on_the_server(&pool).await, (1, 1, 0, sealed(&photo)));
    assert_eq!(
        referring_notes(&pool, attachment.file_id).await,
        [note.as_uuid()]
    );
    // What the storage holds is not the photograph: it is as long as the
    // sealed file, and no stretch of the photograph is in it.
    let object = stored(&pool, attachment.file_id).await.unwrap();
    assert_eq!(object.len(), usize::try_from(sealed(&photo)).unwrap());
    assert!(!object.windows(64).any(|window| window == &photo[..64]));

    // The phone learns of the file with the note. It does not fetch it
    // until it is wanted.
    let before = phone.transport.storage_requests();
    sync(&phone).await;
    assert_eq!(
        attached(&phone, &note).await,
        std::slice::from_ref(&attachment)
    );
    assert!(!holds(&phone, attachment.file_id));
    assert_eq!(phone.transport.storage_requests(), before);

    assert_eq!(read(&phone, &note, attachment.file_id).await, photo);
    assert!(holds(&phone, attachment.file_id));
    assert_eq!(phone.transport.storage_requests(), before + 1);
    // Wanted again, it is read from the device.
    assert_eq!(read(&phone, &note, attachment.file_id).await, photo);
    assert_eq!(phone.transport.storage_requests(), before + 1);

    // Nothing more is sent or asked for, on either device.
    let sent = laptop.transport.storage_requests();
    for device in [&laptop, &phone] {
        let report = sync(device).await;
        assert_eq!((report.files_sent, report.files_unsent), (0, 0));
    }
    assert_eq!(laptop.transport.storage_requests(), sent);
    assert_eq!(on_the_server(&pool).await, (1, 1, 0, sealed(&photo)));

    // A file the note does not name is not fetched, whoever asks.
    let error = phone
        .session
        .fetch_file(&note, Uuid::now_v7())
        .await
        .unwrap_err();
    assert!(matches!(error, FileError::NotAttached), "{error}");
}

/// The phone is offline when the photograph is taken. The note and the
/// file wait on it, and both go when it is back.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_file_attached_offline_is_sent_when_the_device_is_back(pool: PgPool) {
    file_bucket().await;
    let server = Server::with_sign_in(pool.clone()).await;
    let (laptop, phone) = two_devices(&server).await;
    let photo = photograph();

    phone.transport.set_offline(true);
    let (note, attachment) = note_with_file(&phone, "taken on the train", &photo).await;
    assert!(phone.session.sync_now().await.is_err());
    assert_eq!(on_the_server(&pool).await, (0, 0, 0, 0));
    // The file is here, and is read here.
    assert_eq!(read(&phone, &note, attachment.file_id).await, photo);

    phone.transport.set_offline(false);
    let report = sync(&phone).await;
    assert_eq!((report.pushed, report.files_sent), (1, 1));

    sync(&laptop).await;
    assert_eq!(read(&laptop, &note, attachment.file_id).await, photo);
}

/// The server is in reach and the storage is not. The note syncs; the file
/// waits and goes with the next cycle. An upload that did arrive, whose
/// answer was lost, is not sent a second time.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_file_that_did_not_get_through_goes_with_the_next_cycle(pool: PgPool) {
    file_bucket().await;
    let server = Server::with_sign_in(pool.clone()).await;
    let (laptop, phone) = two_devices(&server).await;
    let photo = photograph();
    let (note, attachment) = note_with_file(&laptop, "the whiteboard", &photo).await;

    laptop.transport.fail_storage(1);
    let report = sync(&laptop).await;

    // The cycle did not fail: the note is on the server.
    assert_eq!(
        (
            report.pushed,
            report.files_sent,
            report.files_unsent,
            report.files_failed
        ),
        (1, 0, 1, true)
    );
    // The phone has the note and is told plainly that the file is not
    // there yet.
    sync(&phone).await;
    assert_eq!(attached(&phone, &note).await.len(), 1);
    let error = phone
        .session
        .fetch_file(&note, attachment.file_id)
        .await
        .unwrap_err();
    assert!(matches!(error, FileError::Missing), "{error}");
    assert!(!holds(&phone, attachment.file_id));

    // The upload arrives and its answer is lost.
    laptop.transport.lose_next_storage_answer();
    let report = sync(&laptop).await;
    assert_eq!((report.files_unsent, report.files_failed), (1, true));
    assert!(stored(&pool, attachment.file_id).await.is_some());

    // The next cycle asks the server, which finds the file in the storage.
    let uploads = laptop.transport.storage_requests();
    let report = sync(&laptop).await;
    assert_eq!((report.files_unsent, report.files_failed), (0, false));
    assert_eq!(laptop.transport.storage_requests(), uploads);
    assert_eq!(on_the_server(&pool).await, (1, 1, 0, sealed(&photo)));
    assert_eq!(read(&phone, &note, attachment.file_id).await, photo);
}

/// An app that is left open sends the file by itself after a failure, as
/// it does a note.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn an_open_app_tries_a_file_again_without_being_asked(pool: PgPool) {
    file_bucket().await;
    let server = Server::with_sign_in(pool.clone()).await;
    let laptop = Client::new(&server, "Laptop", true);
    laptop.sign_in("alice").await.unwrap();
    laptop.transport.fail_storage(2);

    note_with_file(&laptop, "the whiteboard", &photograph()).await;
    laptop.session.notify(Trigger::Changed);

    eventually("the file reaches the server", || async {
        on_the_server(&pool).await.0 == 1
    })
    .await;
    eventually("the app shows nothing waiting", || async {
        laptop.session.sync_status().files_unsent == 0
    })
    .await;
}

/// `dirt-api` as it is until its owner opens a storage account. Notes
/// sync. A file waits on the device that holds it, and the app does not
/// keep asking.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_server_with_no_storage_syncs_notes_and_lets_files_wait(pool: PgPool) {
    let server = Server::without_file_storage(pool.clone()).await;
    let laptop = Client::new(&server, "Laptop", true);
    laptop.sign_in("alice").await.unwrap();
    let phone = another_device(&server, &laptop, "Phone").await;
    let photo = photograph();

    let (note, attachment) = note_with_file(&laptop, "the whiteboard", &photo).await;
    let report = sync(&laptop).await;

    // The note is on the server, sent by this cycle or by the app's own.
    assert_eq!(server.blob_count().await, 1);
    assert_eq!((report.files_unsent, report.files_failed), (1, false));
    sync(&phone).await;
    assert_eq!(phone.notes().await.len(), 1);
    assert!(phone
        .session
        .fetch_file(&note, attachment.file_id)
        .await
        .is_err());
    // The laptop still reads its own copy.
    assert_eq!(read(&laptop, &note, attachment.file_id).await, photo);

    // Left alone, the open app asks the server for nothing.
    tokio::time::sleep(Duration::from_millis(400)).await;
    let asked = laptop.transport.requests();
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(laptop.transport.requests(), asked);
}

/// A file is removed from a note that lives on. It stays everywhere for
/// the wait, and then goes: the server is told, and each device lets its
/// copy go.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_file_removed_from_a_note_goes_after_the_wait(pool: PgPool) {
    file_bucket().await;
    let server = Server::with_sign_in(pool.clone()).await;
    let (laptop, phone) = two_devices(&server).await;
    let photo = photograph();
    let (note, attachment) = note_with_file(&laptop, "the whiteboard", &photo).await;
    let file = attachment.file_id;
    sync(&laptop).await;
    sync(&phone).await;
    read(&phone, &note, file).await;

    assert!(laptop
        .session
        .store()
        .detach_file(&note, file)
        .await
        .unwrap());
    sync(&laptop).await;
    sync(&phone).await;

    // The note has no file on either device, and the note is still there.
    for device in [&laptop, &phone] {
        assert!(attached(device, &note).await.is_empty());
        assert_eq!(device.notes().await.len(), 1);
    }
    // Until the wait is over nothing else changes, however often they
    // sync: the server still has the reference, and both devices the file.
    for device in [&laptop, &phone] {
        sync(device).await;
        assert!(holds(device, file));
    }
    assert_eq!(on_the_server(&pool).await, (1, 1, 0, sealed(&photo)));

    // The wait is over on the laptop.
    let laptop = impatient(laptop);
    sync(&laptop).await;

    assert!(referring_notes(&pool, file).await.is_empty());
    // The file's own last thirty days have begun on the server.
    assert_eq!(on_the_server(&pool).await, (1, 0, 1, sealed(&photo)));
    assert!(!holds(&laptop, file));

    // The phone's wait ends later. Telling the server again changes
    // nothing there, and the phone lets its copy go.
    let phone = impatient(phone);
    sync(&phone).await;
    assert!(!holds(&phone, file));
    assert_eq!(on_the_server(&pool).await, (1, 0, 1, sealed(&photo)));
    for device in [&laptop, &phone] {
        assert_eq!(device.notes().await.len(), 1);
        assert_eq!(sync(device).await.files_unsent, 0);
    }
}

/// A note with a file is deleted and, in time, purged. Its file leaves the
/// devices with it, and the server starts the file's last thirty days.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_purged_note_takes_its_file_from_the_devices(pool: PgPool) {
    file_bucket().await;
    let server = Server::with_sign_in(pool.clone()).await;
    let (laptop, phone) = two_devices(&server).await;
    let photo = photograph();
    let (note, attachment) = note_with_file(&laptop, "the whiteboard", &photo).await;
    let file = attachment.file_id;
    sync(&laptop).await;
    sync(&phone).await;
    read(&phone, &note, file).await;

    laptop.session.store().delete_note(&note).await.unwrap();
    sync(&laptop).await;
    sync(&phone).await;
    // A deleted note can come back, and its file with it: both are kept.
    for device in [&laptop, &phone] {
        assert!(holds(device, file));
    }
    assert_eq!(on_the_server(&pool).await, (1, 1, 0, sealed(&photo)));

    let laptop = impatient(laptop);
    assert_eq!(sync(&laptop).await.purged, 1);

    assert!(!holds(&laptop, file));
    assert!(referring_notes(&pool, file).await.is_empty());
    assert_eq!(on_the_server(&pool).await, (1, 0, 1, sealed(&photo)));

    let phone = impatient(phone);
    sync(&phone).await;
    assert!(!holds(&phone, file));
    assert!(phone
        .session
        .store()
        .note_documents()
        .await
        .unwrap()
        .is_empty());
}

/// Step 8e keeps an unsent edit to a purged note as a new note. The new
/// note names the old one's file, and the phone that made it never fetched
/// the file. The phone tells the server that the new note refers to it,
/// which stops the file's last thirty days, and every device can read it.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn an_edit_kept_as_a_new_note_after_a_purge_keeps_the_file(pool: PgPool) {
    file_bucket().await;
    let server = Server::with_sign_in(pool.clone()).await;
    let (laptop, phone) = two_devices(&server).await;
    let photo = photograph();
    let (note, attachment) = note_with_file(&laptop, "the whiteboard", &photo).await;
    let file = attachment.file_id;
    sync(&laptop).await;
    sync(&phone).await;

    // The phone goes away and writes a caption.
    phone
        .session
        .store()
        .update_note(&note, "the whiteboard, with a caption")
        .await
        .unwrap();
    // Meanwhile the laptop deletes the note and, in time, purges it.
    laptop.session.store().delete_note(&note).await.unwrap();
    let laptop = impatient(laptop);
    sync(&laptop).await;
    assert_eq!(on_the_server(&pool).await, (1, 0, 1, sealed(&photo)));
    assert!(!holds(&laptop, file));

    sync(&phone).await;
    sync(&phone).await;

    let kept = phone.notes().await;
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].1, "the whiteboard, with a caption");
    let kept: NoteId = kept[0].0.parse().unwrap();
    assert_ne!(kept, note);
    assert_eq!(
        attached(&phone, &kept).await,
        std::slice::from_ref(&attachment)
    );
    // The server knows the new note refers to the file, and no longer
    // means to delete it.
    assert_eq!(referring_notes(&pool, file).await, [kept.as_uuid()]);
    assert_eq!(on_the_server(&pool).await, (1, 1, 0, sealed(&photo)));
    assert_eq!(server.daily_sweep().await, 0);

    // Both devices read it: the phone for the first time, and the laptop
    // again, having let its copy go with the old note.
    assert_eq!(read(&phone, &kept, file).await, photo);
    sync(&laptop).await;
    assert_eq!(read(&laptop, &kept, file).await, photo);
}

/// Two devices were away with edits when a third purged the note. Each
/// keeps its own new note, and both name the file. Purging one of the new
/// notes later leaves the other's file readable.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn two_notes_kept_after_a_purge_both_keep_the_file(pool: PgPool) {
    file_bucket().await;
    let server = Server::with_sign_in(pool.clone()).await;
    let (laptop, phone) = two_devices(&server).await;
    let tablet = another_device(&server, &laptop, "Tablet").await;
    let photo = photograph();
    let (note, attachment) = note_with_file(&laptop, "the whiteboard", &photo).await;
    let file = attachment.file_id;
    sync(&laptop).await;
    for device in [&phone, &tablet] {
        sync(device).await;
    }

    for (device, text) in [(&phone, "from the phone"), (&tablet, "from the tablet")] {
        device
            .session
            .store()
            .update_note(&note, text)
            .await
            .unwrap();
    }
    laptop.session.store().delete_note(&note).await.unwrap();
    let laptop = impatient(laptop);
    assert_eq!(sync(&laptop).await.purged, 1);

    for device in [&phone, &tablet] {
        sync(device).await;
        sync(device).await;
    }
    for device in [&phone, &tablet, &laptop] {
        sync(device).await;
    }

    // Every device has both new notes, and each names the file.
    let mut kept = Vec::new();
    for (id, text) in laptop.notes().await {
        let id: NoteId = id.parse().unwrap();
        assert_eq!(
            attached(&laptop, &id).await,
            std::slice::from_ref(&attachment),
            "{text}"
        );
        kept.push((id, text));
    }
    kept.sort_by(|a, b| a.1.cmp(&b.1));
    assert_eq!(
        kept.iter()
            .map(|(_, text)| text.as_str())
            .collect::<Vec<_>>(),
        ["from the phone", "from the tablet"]
    );
    assert_eq!(referring_notes(&pool, file).await.len(), 2);
    assert_eq!(on_the_server(&pool).await, (1, 2, 0, sealed(&photo)));

    // The phone's note is deleted and purged.
    let (from_phone, from_tablet) = (kept[0].0, kept[1].0);
    phone
        .session
        .store()
        .delete_note(&from_phone)
        .await
        .unwrap();
    let phone = impatient(phone);
    sync(&phone).await;
    assert_eq!(sync(&phone).await.unsent, 0);

    // The tablet's note still refers to the file, so the server keeps it,
    // and the tablet reads it for the first time.
    assert_eq!(referring_notes(&pool, file).await, [from_tablet.as_uuid()]);
    assert_eq!(on_the_server(&pool).await, (1, 1, 0, sealed(&photo)));
    assert_eq!(server.daily_sweep().await, 0);
    sync(&tablet).await;
    assert_eq!(read(&tablet, &from_tablet, file).await, photo);
}

/// The server's database is restored from a backup taken before the file
/// was sent. The storage is not part of a backup, and here it has lost the
/// file too. A device that does not hold the file can do nothing but say
/// so. The device that holds it sends it again, without being asked.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_file_the_server_forgot_is_sent_again_by_a_device_that_holds_it(pool: PgPool) {
    file_bucket().await;
    let server = Server::with_sign_in(pool.clone()).await;
    let (laptop, phone) = two_devices(&server).await;
    let photo = photograph();
    let (note, attachment) = note_with_file(&laptop, "the whiteboard", &photo).await;
    let file = attachment.file_id;
    sync(&laptop).await;
    sync(&phone).await;

    let key = object_key(&pool, file).await;
    for statement in [
        "DELETE FROM file_notes",
        "DELETE FROM files",
        "DELETE FROM notes",
        "UPDATE accounts SET epoch = epoch + 1000, change_counter = 0, note_count = 0, \
                bytes_used = 0, file_bytes = 0",
    ] {
        sqlx::query(statement).execute(&pool).await.unwrap();
    }
    file_store().delete(&key).await.unwrap();

    // The phone comes first. It sends the note again and cannot send the
    // file. That is no failure, and it has nothing of its own waiting.
    let report = sync(&phone).await;
    assert_eq!(
        (
            report.pushed,
            report.files_sent,
            report.files_unsent,
            report.files_failed
        ),
        (1, 0, 0, false)
    );
    let error = phone.session.fetch_file(&note, file).await.unwrap_err();
    assert!(matches!(error, FileError::Missing), "{error}");
    // Nor is anything wrong with the file as far as the phone can tell.
    assert!(phone
        .session
        .store()
        .file_problems()
        .await
        .unwrap()
        .is_empty());

    // The laptop holds the file.
    let report = sync(&laptop).await;
    assert_eq!((report.files_sent, report.files_unsent), (1, 0));
    assert_eq!(on_the_server(&pool).await, (1, 1, 0, sealed(&photo)));

    assert_eq!(read(&phone, &note, file).await, photo);
    // The phone's own reference is taken at its next cycle, and after
    // that it asks no more.
    sync(&phone).await;
    let asked = phone.transport.requests();
    sync(&phone).await;
    assert_eq!(
        phone.transport.requests(),
        asked + 1,
        "one pull and no more"
    );
}

/// A note written before signing in has a file. Moving the note into the
/// account takes the file along, and from there it reaches the server.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_file_attached_before_signing_in_reaches_the_account(pool: PgPool) {
    file_bucket().await;
    let server = Server::with_sign_in(pool.clone()).await;
    let laptop = Client::new(&server, "Laptop", false);
    let photo = photograph();
    let (note, attachment) = note_with_file(&laptop, "written signed out", &photo).await;
    // Signed out, the file is read from the device.
    assert_eq!(read(&laptop, &note, attachment.file_id).await, photo);

    laptop.sign_in("alice").await.unwrap();
    assert_eq!(laptop.session.move_local_notes().await.unwrap(), 1);
    let report = sync(&laptop).await;
    assert_eq!((report.pushed, report.files_sent), (1, 1));

    let phone = another_device(&server, &laptop, "Phone").await;
    sync(&phone).await;
    assert_eq!(read(&phone, &note, attachment.file_id).await, photo);
}

/// The storage is not trusted. What it returns in a file's place is not
/// kept, and is not shown as the file.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_file_changed_in_the_storage_is_not_kept(pool: PgPool) {
    file_bucket().await;
    let server = Server::with_sign_in(pool.clone()).await;
    let (laptop, phone) = two_devices(&server).await;
    let photo = photograph();
    let (note, attachment) = note_with_file(&laptop, "the whiteboard", &photo).await;
    let file = attachment.file_id;
    sync(&laptop).await;
    sync(&phone).await;

    // One byte of the stored object is changed, in the middle.
    let mut object = stored(&pool, file).await.unwrap();
    let middle = object.len() / 2;
    object[middle] ^= 1;
    let link = file_store().upload_link(&object_key(&pool, file).await, sealed(&photo));
    let answer = reqwest::Client::new()
        .put(link)
        .body(object)
        .send()
        .await
        .unwrap();
    assert!(answer.status().is_success(), "{}", answer.status());

    let error = phone.session.fetch_file(&note, file).await.unwrap_err();

    assert!(matches!(error, FileError::Damaged), "{error}");
    assert!(!holds(&phone, file));
    // Nothing half-written is left beside the phone's files either.
    let left: Vec<PathBuf> =
        std::fs::read_dir(phone.session.store().shelf().path(file).parent().unwrap())
            .map(|entries| entries.map(|entry| entry.unwrap().path()).collect())
            .unwrap_or_default();
    assert!(left.is_empty(), "{left:?}");
    // The laptop's own copy is untouched.
    assert_eq!(read(&laptop, &note, file).await, photo);
}

/// The account's room for files is used up. The note syncs, and the file
/// is listed with the reason. When there is room again it goes.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_file_the_account_has_no_room_for_waits_with_its_reason(pool: PgPool) {
    file_bucket().await;
    let server = Server::with_sign_in(pool.clone()).await;
    let (laptop, _phone) = two_devices(&server).await;
    sqlx::query("UPDATE accounts SET file_bytes = $1")
        .bind(limits::MAX_ACCOUNT_FILE_BYTES)
        .execute(&pool)
        .await
        .unwrap();
    let photo = photograph();
    let (note, first) = note_with_file(&laptop, "the whiteboard", &photo).await;
    let (_, second) = note_with_file(&laptop, "another", &photo).await;

    let report = sync(&laptop).await;

    assert_eq!(
        (
            report.pushed,
            report.files_sent,
            report.files_unsent,
            report.files_failed
        ),
        (2, 0, 1, false),
        "the second file was not tried once the account was known to be full"
    );
    let problems = laptop.session.store().file_problems().await.unwrap();
    assert_eq!(problems.len(), 1);
    assert_eq!(
        (problems[0].file_id, problems[0].note_id),
        (first.file_id, note)
    );
    assert!(
        problems[0].reason.contains("full"),
        "{}",
        problems[0].reason
    );
    // What every client shows about sync says so, by the file's name.
    let shown = laptop.session.sync_overview().await.unwrap().lines(0);
    assert!(
        shown.contains(&"1 file could not be sent:".to_owned())
            && shown
                .iter()
                .any(|line| line.contains("whiteboard.jpg, attached to: the whiteboard")),
        "{shown:?}"
    );

    sqlx::query("UPDATE accounts SET file_bytes = 0")
        .execute(&pool)
        .await
        .unwrap();
    let report = sync(&laptop).await;
    assert_eq!((report.files_sent, report.files_unsent), (2, 0));
    assert!(laptop
        .session
        .store()
        .file_problems()
        .await
        .unwrap()
        .is_empty());
    assert!(stored(&pool, second.file_id).await.is_some());
}

/// Signing out deletes the account's notes and files from the device. A
/// file that has not reached the server would be gone for good, while its
/// note, which did sync, went on naming it. Signing out is refused, as it
/// is for a note that has not been sent.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn signing_out_is_refused_while_a_file_is_only_on_this_device(pool: PgPool) {
    file_bucket().await;
    let server = Server::with_sign_in(pool.clone()).await;
    let (laptop, phone) = two_devices(&server).await;
    let photo = photograph();
    let (note, attachment) = note_with_file(&phone, "taken on the phone", &photo).await;

    // The note gets through and the file does not, now and at sign-out.
    phone.transport.fail_storage(2);
    assert_eq!(sync(&phone).await.pushed, 1);
    let error = phone
        .session
        .sign_out(dirt_core::session::Unsynced::Refuse)
        .await
        .unwrap_err();

    let dirt_core::session::SignOutError::Unsynced { notes, files, .. } = error else {
        panic!("expected the file to stop the sign-out: {error}");
    };
    assert_eq!((notes, files), (0, 1));
    assert!(phone.session.state().is_ready());
    assert_eq!(read(&phone, &note, attachment.file_id).await, photo);

    // With the storage in reach, signing out sends the file first.
    phone
        .session
        .sign_out(dirt_core::session::Unsynced::Refuse)
        .await
        .unwrap();
    assert_eq!(phone.session.state(), SessionState::SignedOut);
    sync(&laptop).await;
    assert_eq!(read(&laptop, &note, attachment.file_id).await, photo);
}

/// A file was removed from a note, and before the wait was over the server
/// lost all knowledge of the file. There is nothing left there to drop, and
/// the device lets the file go as it would have.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_removed_file_the_server_no_longer_knows_still_leaves_the_device(pool: PgPool) {
    file_bucket().await;
    let server = Server::with_sign_in(pool.clone()).await;
    let (laptop, _phone) = two_devices(&server).await;
    let (note, attachment) = note_with_file(&laptop, "the whiteboard", &photograph()).await;
    let file = attachment.file_id;
    sync(&laptop).await;
    laptop
        .session
        .store()
        .detach_file(&note, file)
        .await
        .unwrap();
    sync(&laptop).await;
    for statement in ["DELETE FROM file_notes", "DELETE FROM files"] {
        sqlx::query(statement).execute(&pool).await.unwrap();
    }

    let laptop = impatient(laptop);
    sync(&laptop).await;

    assert!(!holds(&laptop, file));
    // Nothing is asked about the file again.
    let asked = laptop.transport.requests();
    sync(&laptop).await;
    assert_eq!(
        laptop.transport.requests(),
        asked + 1,
        "one pull and no more"
    );
}
