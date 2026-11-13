//! An export with files, made by a whole device against the real server
//! and a real file storage.
//!
//! An export holds every file of every note, so a device fetches the ones
//! it does not hold. What it cannot fetch it leaves out and names. These
//! tests are about that: what is fetched, what a failure costs, and what
//! the export then says. What an export looks like on disk is tested in
//! `dirt-core`, beside the code.
//!
//! Start the database and the file storage with `docker compose up -d` at
//! the repository root and set `DATABASE_URL` as `compose.yaml` shows.

use dirt_core::export::{Export, ExportFormat, ExportNote};
use dirt_core::files::seal::sealed_size;
use dirt_core::session::SessionState;
use dirt_proto::NoteId;
use dirt_sim::{file_bucket, file_store, Client, Server};
use sqlx::PgPool;
use uuid::Uuid;

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

/// A file's bytes: more than one piece when it is encrypted, and different
/// for each `seed`.
fn bytes(seed: u8) -> Vec<u8> {
    (0..150_000u32)
        .map(|i| u8::try_from(i % 251).unwrap() ^ seed)
        .collect()
}

/// Writes a note on `client` and attaches a file of these bytes to it.
async fn note_with_file(client: &Client, text: &str, name: &str, bytes: &[u8]) -> (NoteId, Uuid) {
    let store = client.session.store();
    let note = store.create_note(text).await.unwrap().id;
    let source = client.folder().join(format!("{}.bin", Uuid::now_v7()));
    std::fs::write(&source, bytes).unwrap();
    let attachment = store
        .attach_file(&note, &source, name, "image/jpeg")
        .await
        .unwrap();
    std::fs::remove_file(&source).unwrap();
    (note, attachment.file_id)
}

async fn export(client: &Client) -> Export {
    Export::with_files(
        &client.session.store(),
        Some(&client.session),
        ExportFormat::Json,
    )
    .await
    .unwrap()
}

/// Three notes on the laptop, each with a file, synced to the server and
/// known to the phone, which holds none of the files. Newest first, as an
/// export goes through them.
async fn three_files_the_phone_does_not_hold(
    laptop: &Client,
    phone: &Client,
) -> Vec<(NoteId, Uuid, Vec<u8>)> {
    let mut files = Vec::new();
    for (seed, name) in [(1, "first.jpg"), (2, "second.jpg"), (3, "third.jpg")] {
        let bytes = bytes(seed);
        let (note, file) = note_with_file(laptop, name, name, &bytes).await;
        files.push((note, file, bytes));
        // Each note a moment after the one before, so that their order
        // is not in doubt.
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    let report = laptop.session.sync_now().await.unwrap();
    assert_eq!((report.files_sent, report.files_unsent), (3, 0));
    phone.session.sync_now().await.unwrap();
    for (_, file, _) in &files {
        assert!(!phone.session.store().shelf().holds(*file));
    }
    files.reverse();
    files
}

/// A phone that has never opened the files exports them all: each is
/// fetched once, and stays on the phone.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn an_export_fetches_the_files_the_device_does_not_hold(pool: PgPool) {
    file_bucket().await;
    let server = Server::with_sign_in(pool).await;
    let (laptop, phone) = two_devices(&server).await;
    let files = three_files_the_phone_does_not_hold(&laptop, &phone).await;

    let before = phone.transport.storage_requests();
    let made = export(&phone).await;

    assert_eq!(made.counts(), "3 notes and 3 files");
    assert!(made.left_out().is_empty());
    assert_eq!(phone.transport.storage_requests(), before + 3);

    let folder = phone.folder().join("out");
    made.write_folder(&folder).unwrap();
    let notes: Vec<ExportNote> =
        serde_json::from_str(&std::fs::read_to_string(folder.join("notes.json")).unwrap()).unwrap();
    assert_eq!(notes.len(), 3);
    for (note, file, bytes) in &files {
        let named = notes
            .iter()
            .find(|exported| exported.id == note.to_string())
            .unwrap();
        assert_eq!(named.files.len(), 1);
        assert_eq!(named.files[0].id, file.to_string());
        let path = named.files[0].path.as_deref().unwrap();
        assert_eq!(&std::fs::read(folder.join(path)).unwrap(), bytes);
        assert!(phone.session.store().shelf().holds(*file));
    }

    // A second export has nothing to fetch.
    let again = export(&phone).await;
    assert_eq!(again.counts(), "3 notes and 3 files");
    assert_eq!(phone.transport.storage_requests(), before + 3);
    // And so can be made with no network at all.
    phone.transport.set_offline(true);
    let offline = export(&phone).await;
    assert_eq!(offline.counts(), "3 notes and 3 files");
    assert!(offline.left_out().is_empty());
}

/// With no way to the server, an export finds that out once. It holds
/// what the device has, and names the rest with one reason.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn an_export_with_no_network_asks_once_and_names_what_it_lacks(pool: PgPool) {
    file_bucket().await;
    let server = Server::with_sign_in(pool).await;
    let (laptop, phone) = two_devices(&server).await;
    let files = three_files_the_phone_does_not_hold(&laptop, &phone).await;
    // The phone has opened the second file, and holds it.
    let (held_note, held_file, _) = &files[1];
    phone
        .session
        .fetch_file(held_note, *held_file)
        .await
        .unwrap();

    phone.transport.set_offline(true);
    let before = phone.transport.requests();
    let made = export(&phone).await;

    assert_eq!(phone.transport.requests(), before + 1);
    // The file the phone holds is in the export all the same, though it
    // comes after the one that could not be fetched.
    assert_eq!(made.counts(), "3 notes and 1 file");
    let left_out = made.left_out();
    assert_eq!(
        left_out
            .iter()
            .map(|file| file.name.as_str())
            .collect::<Vec<_>>(),
        ["third.jpg", "first.jpg"]
    );
    assert_eq!(left_out[0].why, left_out[1].why);
    // The reason is about reaching the server, not about the files.
    assert!(
        !left_out[0].why.contains("does not have it"),
        "{}",
        left_out[0].why
    );
    assert_eq!(
        made.left_out_lines(usize::MAX),
        [
            "2 files are not in the export.".to_owned(),
            left_out[0].why.clone(),
            "  third.jpg, attached to: third.jpg".to_owned(),
            "  first.jpg, attached to: first.jpg".to_owned(),
        ]
    );

    // Back in reach, the same export is whole.
    phone.transport.set_offline(false);
    let whole = export(&phone).await;
    assert_eq!(whole.counts(), "3 notes and 3 files");
    assert!(whole.left_out().is_empty());
}

/// The storage is another place than the server, and can be out of reach
/// by itself. That too is found out once.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn an_export_stops_fetching_when_the_storage_cannot_be_reached(pool: PgPool) {
    file_bucket().await;
    let server = Server::with_sign_in(pool).await;
    let (laptop, phone) = two_devices(&server).await;
    three_files_the_phone_does_not_hold(&laptop, &phone).await;

    let before = phone.transport.storage_requests();
    phone.transport.fail_storage(1);
    let made = export(&phone).await;

    assert_eq!(phone.transport.storage_requests(), before + 1);
    assert_eq!(made.counts(), "3 notes and no files");
    assert_eq!(made.left_out().len(), 3);
    // Nothing was asked of the storage after the failure, so the one
    // failure that was arranged is the only one there was.
    let whole = export(&phone).await;
    assert_eq!(whole.counts(), "3 notes and 3 files");
}

/// One file whose copy in the storage is not what it should be is that
/// file's own trouble. The export says so of it, and has the others.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_file_that_cannot_be_opened_is_left_out_and_the_rest_are_exported(pool: PgPool) {
    file_bucket().await;
    let server = Server::with_sign_in(pool.clone()).await;
    let (laptop, phone) = two_devices(&server).await;
    let files = three_files_the_phone_does_not_hold(&laptop, &phone).await;

    // One byte of the stored object of the first file an export comes to
    // is changed.
    let (_, damaged, plain) = &files[0];
    let account: Uuid = sqlx::query_scalar("SELECT id FROM accounts")
        .fetch_one(&pool)
        .await
        .unwrap();
    let key = dirt_api::FileStore::object_key(account, *damaged);
    let mut object = reqwest::get(file_store().download_link(&key))
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap()
        .to_vec();
    object[100] ^= 1;
    let size = i64::try_from(sealed_size(plain.len() as u64)).unwrap();
    let answer = reqwest::Client::new()
        .put(file_store().upload_link(&key, size))
        .body(object)
        .send()
        .await
        .unwrap();
    assert!(answer.status().is_success(), "{}", answer.status());

    let made = export(&phone).await;

    assert_eq!(made.counts(), "3 notes and 2 files");
    assert_eq!(
        made.left_out_lines(usize::MAX),
        [
            "1 file is not in the export.",
            "The server's copy of the file could not be decrypted, and nothing was kept. \
             A device that holds the file still has it.",
            "  third.jpg, attached to: third.jpg",
        ]
    );
    // The laptop, which holds every file, exports them all.
    let whole = export(&laptop).await;
    assert_eq!(whole.counts(), "3 notes and 3 files");
}

/// A device that is signed out exports the notes of the store it is given
/// with the files it holds, and asks the server for nothing.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_device_that_is_not_signed_in_exports_what_it_holds(pool: PgPool) {
    file_bucket().await;
    let server = Server::with_sign_in(pool).await;
    let visitor = Client::new(&server, "Visitor", false);
    let (_, file) = note_with_file(&visitor, "before signing in", "local.jpg", &bytes(7)).await;
    let (_, gone) = note_with_file(&visitor, "its file is lost", "lost.jpg", &bytes(8)).await;
    let store = visitor.session.store();
    std::fs::remove_file(store.shelf().path(gone)).unwrap();

    let before = visitor.transport.requests();
    let made = export(&visitor).await;

    assert_eq!(visitor.transport.requests(), before);
    assert_eq!(made.counts(), "2 notes and 1 file");
    assert!(store.shelf().holds(file));
    assert_eq!(
        made.left_out()[0].why,
        "The file is not on this device, and this device is not signed in to fetch it."
    );
}
