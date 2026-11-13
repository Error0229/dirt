//! Measurements behind the limits review (`docs/LIMITS.md`).
//!
//! These are not checks that pass or fail on a number. They print what a
//! note and an account of a given size cost, so that the limits in
//! `dirt_proto::api::limits` can be set against something measured. They
//! take minutes, so they are ignored by default:
//!
//! ```text
//! cargo test -p dirt-sim --test limits -- --ignored --nocapture --test-threads 1
//! ```
//!
//! Start the database and the file storage with `docker compose up -d` at
//! the repository root and set `DATABASE_URL` as `compose.yaml` shows.

use std::time::Instant;

use dirt_core::export::{Export, ExportFormat};
use dirt_core::files::seal::sealed_size;
use dirt_core::files::Attachment;
use dirt_core::notes::NoteDoc;
use dirt_core::session::{Approval, ExchangeStep, Join, SessionState};
use dirt_proto::api::limits;
use dirt_sim::{file_bucket, Client, Server};
use sqlx::PgPool;

/// What encryption adds to a document: a layout byte, the nonce and the tag.
const BLOB_OVERHEAD: usize = 1 + 24 + 16;

fn document_size(text: &str, edits: usize) -> usize {
    let document = NoteDoc::new(1);
    document.set_text(text, 1_000);
    // Each edit appends a word, as typing a little more would.
    let mut grown = text.to_owned();
    for edit in 0..edits {
        grown.push_str(" more");
        document.set_text(&grown, 1_000 + i64::try_from(edit).unwrap_or(0));
    }
    document.encode().len()
}

#[test]
#[ignore = "a measurement, not a check"]
fn the_size_of_a_note_on_the_server() {
    println!("\ncharacters of text | edits | document bytes | blob bytes");
    for (length, edits) in [
        (0, 0),
        (80, 0),
        (280, 0),
        (280, 20),
        (2_000, 0),
        (2_000, 200),
        (20_000, 0),
        (200_000, 0),
        (1_000_000, 0),
    ] {
        let text = "x".repeat(length);
        let size = document_size(&text, edits);
        println!(
            "{length:>18} | {edits:>5} | {size:>14} | {:>10}",
            size + BLOB_OVERHEAD
        );
    }
    let per_account = usize::try_from(limits::MAX_ACCOUNT_BYTES).unwrap_or(usize::MAX);
    let ordinary = document_size(&"x".repeat(280), 20) + BLOB_OVERHEAD;
    println!(
        "\nan account's {} bytes hold {} notes of 280 characters edited 20 times",
        per_account,
        per_account / ordinary
    );
    println!(
        "the note limit of {} is reached first by notes under {} bytes",
        limits::MAX_NOTES_PER_ACCOUNT,
        per_account / usize::try_from(limits::MAX_NOTES_PER_ACCOUNT).unwrap_or(1)
    );
}

/// What deleting a note adds to its document. A full account still takes
/// a push that grows a note by `FULL_ACCOUNT_GROWTH_BYTES`, so that a
/// delete goes through; this is what a delete needs of that.
#[test]
#[ignore = "a measurement, not a check"]
fn what_a_delete_adds_to_a_note() {
    println!("\ndevices that edited the note | bytes before | bytes after the delete | added");
    for devices in [1_u64, 2, 5, 20] {
        let document = NoteDoc::new(1);
        document.set_text("a line of text, about as long as a thought", 1_000);
        let mut text = document.text();
        for device in 2..=devices {
            let other = NoteDoc::from_bytes(device, &document.encode()).unwrap();
            text.push_str(" and more");
            other.set_text(&text, 1_000 + i64::try_from(device).unwrap_or(0));
            document.merge(&other.encode()).unwrap();
        }
        let before = document.encode().len();
        document.delete();
        let after = document.encode().len();
        println!(
            "{devices:>28} | {before:>12} | {after:>22} | {:>5}",
            after - before
        );
    }
    println!(
        "a full account takes growth of up to {} bytes",
        limits::FULL_ACCOUNT_GROWTH_BYTES
    );
}

/// What letting a device in, a recovery phrase and a change of the key
/// leave on the server, against the sizes the server allows for them.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
#[ignore = "a measurement, not a check"]
async fn what_an_approval_stores(pool: PgPool) {
    let server = Server::with_sign_in(pool.clone()).await;
    let laptop = Client::new(&server, "Laptop", false);
    laptop.sign_in("alice").await.unwrap();
    let phone = Client::new(&server, "Phone", false);
    let SessionState::WaitingForApproval { device_id, .. } = phone.sign_in("alice").await.unwrap()
    else {
        panic!("the second device should wait");
    };

    let mut join: Join = phone.session.begin_join().await.unwrap();
    let target = laptop
        .session
        .waiting_devices()
        .await
        .unwrap()
        .into_iter()
        .find(|waiting| waiting.id == device_id)
        .unwrap();
    let mut approval: Approval = laptop.session.begin_approval(target).unwrap();
    approval.poll().await.unwrap();
    join.poll().await.unwrap();
    approval.poll().await.unwrap();
    approval.confirm().await.unwrap();
    let (messages, longest): (i64, i32) =
        sqlx::query_as("SELECT count(*), COALESCE(max(length(body)), 0) FROM enrolment_messages")
            .fetch_one(&pool)
            .await
            .unwrap();
    let (for_device,): (i32,) = sqlx::query_as("SELECT length(wrapped) FROM enrolment_keys")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(join.confirm().await.unwrap(), ExchangeStep::Done);
    println!(
        "\nan approval exchange: {messages} messages on the server at its end, the longest {longest} bytes \
         (allowed: {} of {} bytes)",
        limits::MAX_ENROLMENT_MESSAGES,
        limits::MAX_ENROLMENT_MESSAGE_BYTES
    );

    laptop.session.create_recovery_phrase().await.unwrap();
    let (under_phrase,): (i32,) = sqlx::query_as("SELECT length(wrapped) FROM recovery_keys")
        .fetch_one(&pool)
        .await
        .unwrap();
    laptop.session.change_account_key().await.unwrap();
    let (after_change,): (i32,) = sqlx::query_as("SELECT max(length(wrapped)) FROM device_keys")
        .fetch_one(&pool)
        .await
        .unwrap();
    println!(
        "copies of the account key: {for_device} bytes for a waiting device, {under_phrase} under \
         a recovery phrase, {after_change} after a change of the key (allowed: {})",
        limits::MAX_WRAPPED_KEY_BYTES
    );
}

async fn second_device(server: &Server, first: &Client, name: &str) -> Client {
    let device = Client::new(server, name, false);
    let state = device.sign_in("alice").await.unwrap();
    assert!(matches!(state, SessionState::WaitingForApproval { .. }));
    device.approve_with_key(server, &first.account_key()).await;
    device
}

/// An account holding as many notes as it may: what it costs to send it,
/// to fetch it on a new device, and to change its key.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
#[ignore = "a measurement, not a check"]
async fn an_account_at_the_note_limit(pool: PgPool) {
    let notes = usize::try_from(limits::MAX_NOTES_PER_ACCOUNT).unwrap();
    let server = Server::with_sign_in(pool.clone()).await;
    let laptop = Client::new(&server, "Laptop", false);
    laptop.sign_in("alice").await.unwrap();

    let began = Instant::now();
    let store = laptop.session.store();
    for number in 0..notes {
        store
            .create_note(&format!(
                "note {number}: something worth remembering, about a line long #tag{}",
                number % 50
            ))
            .await
            .unwrap();
    }
    println!("\nwriting {notes} notes locally: {:.1?}", began.elapsed());

    let (began, before) = (Instant::now(), laptop.transport.requests());
    let report = laptop.session.sync_now().await.unwrap();
    println!(
        "first send: {} notes in {} requests, {:.1?}",
        report.pushed,
        laptop.transport.requests() - before,
        began.elapsed()
    );
    let (bytes_used,): (i64,) = sqlx::query_as("SELECT bytes_used FROM accounts")
        .fetch_one(&pool)
        .await
        .unwrap();
    let tenths_of_a_percent = bytes_used * 1000 / limits::MAX_ACCOUNT_BYTES;
    println!(
        "the server holds {bytes_used} bytes for it, {}.{}% of the byte limit",
        tenths_of_a_percent / 10,
        tenths_of_a_percent % 10
    );

    // What the database itself uses for those rows, with its indexes and
    // its own overhead: this is what a storage allowance is spent on.
    let (on_disk,): (i64,) = sqlx::query_as("SELECT pg_total_relation_size('notes')")
        .fetch_one(&pool)
        .await
        .unwrap();
    println!("the database uses {on_disk} bytes for the notes table and its indexes");

    let phone = second_device(&server, &laptop, "Phone").await;
    let (began, before) = (Instant::now(), phone.transport.requests());
    let report = phone.session.sync_now().await.unwrap();
    println!(
        "a new device fetches it: {} notes in {} requests, {:.1?}",
        report.pulled,
        phone.transport.requests() - before,
        began.elapsed()
    );

    let (began, before) = (Instant::now(), laptop.transport.requests());
    laptop.session.change_account_key().await.unwrap();
    println!(
        "changing the key and sending every note again: {} requests, {:.1?}",
        laptop.transport.requests() - before,
        began.elapsed()
    );
    let (began, before) = (Instant::now(), phone.transport.requests());
    let report = phone.session.sync_now().await.unwrap();
    println!(
        "the other device follows the change: {} notes in {} requests, {:.1?}",
        report.pulled,
        phone.transport.requests() - before,
        began.elapsed()
    );
    println!(
        "requests allowed per account: {} in {} seconds",
        limits::REQUESTS_PER_WINDOW,
        limits::REQUEST_WINDOW_SECS
    );

    // What the apps ask the store for. Each is asked twice and the second
    // answer timed, so that reading the file into memory is not counted.
    println!("\nthe store, holding {notes} notes:");
    for size in [100, 1_000, 5_000, notes] {
        store.list_notes(size, 0).await.unwrap();
        let began = Instant::now();
        let listed = store.list_notes(size, 0).await.unwrap();
        println!(
            "listing the newest {}: {:.1?}",
            listed.len(),
            began.elapsed()
        );
    }
    let offset = notes - 100;
    store.list_notes(100, offset).await.unwrap();
    let began = Instant::now();
    let listed = store.list_notes(100, offset).await.unwrap();
    println!(
        "listing the oldest {}, after skipping {offset}: {:.1?}",
        listed.len(),
        began.elapsed()
    );
    for (what, query, tag) in [
        ("a word every note holds", "remembering", None),
        ("a word and a tag", "remembering", Some("tag7")),
        ("a tag", "", Some("tag7")),
        ("a word one note holds", "19999", None),
        ("the start of a word", "rem", None),
    ] {
        store.find_notes(query, tag, 100).await.unwrap();
        let began = Instant::now();
        let found = store.find_notes(query, tag, 100).await.unwrap();
        println!(
            "finding by {what}: {} found in {:.1?}",
            found.len(),
            began.elapsed()
        );
    }
    let began = Instant::now();
    let tags = store.list_tags().await.unwrap();
    println!(
        "listing the {} tags in use: {:.1?}",
        tags.len(),
        began.elapsed()
    );
    let began = Instant::now();
    let count = store.note_count().await.unwrap();
    println!("counting the {count} notes: {:.1?}", began.elapsed());
}

/// The largest file that may be attached: the limit is on the file as it
/// is stored, which is a little longer than the file.
fn largest_file_bytes() -> u64 {
    let most = u64::try_from(limits::MAX_FILE_BYTES).unwrap();
    let (mut fits, mut too_large) = (0, most);
    while too_large - fits > 1 {
        let middle = fits + (too_large - fits) / 2;
        if sealed_size(middle) <= most {
            fits = middle;
        } else {
            too_large = middle;
        }
    }
    fits
}

/// Bytes that do not repeat and do not compress, as a photograph's do
/// not.
fn file_bytes(size: u64, seed: u64) -> Vec<u8> {
    let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    (0..size)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state.to_le_bytes()[0]
        })
        .collect()
}

/// A picture as large as a phone's camera makes, saved as a JPEG: twelve
/// million pixels of something that is not one colour.
fn photograph(path: &std::path::Path) {
    let picture = image::RgbImage::from_fn(4_000, 3_000, |x, y| {
        let wave = ((x / 7 + y / 5) % 256).to_le_bytes()[0];
        let grain = (x.wrapping_mul(2_654_435_761) ^ y.wrapping_mul(40_503)).to_le_bytes()[1] / 8;
        image::Rgb([
            wave.wrapping_add(grain),
            (x % 256).to_le_bytes()[0],
            (y % 256).to_le_bytes()[0].wrapping_add(grain),
        ])
    });
    picture
        .save_with_format(path, image::ImageFormat::Jpeg)
        .unwrap();
}

async fn attach(
    client: &Client,
    note: &dirt_proto::NoteId,
    name: &str,
    bytes: &[u8],
) -> Attachment {
    let source = client.folder().join("to-attach");
    std::fs::write(&source, bytes).unwrap();
    let attachment = client
        .session
        .store()
        .attach_file(note, &source, name, dirt_core::files::media_type_for(name))
        .await
        .unwrap();
    std::fs::remove_file(&source).unwrap();
    attachment
}

/// What each file adds to its note's document, which is sent whole each
/// time the note is edited.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "a measurement, not a check"]
async fn what_a_file_adds_to_a_note() {
    let folder = tempfile::tempdir().unwrap();
    let store = dirt_core::Store::open(folder.path().join("notes.db")).unwrap();
    let document = |note: dirt_proto::NoteId| {
        let store = store.clone();
        async move {
            store
                .note_documents()
                .await
                .unwrap()
                .into_iter()
                .find(|(id, _)| *id == note)
                .unwrap()
                .1
                .len()
        }
    };
    let most = usize::try_from(limits::MAX_FILES_PER_NOTE).unwrap();
    let text = "x".repeat(280);

    println!("\nfiles on a note of 280 characters | document bytes | blob bytes");
    let note = store.create_note(&text).await.unwrap().id;
    let empty = document(note).await;
    println!("{:>33} | {empty:>14} | {:>10}", 0, empty + BLOB_OVERHEAD);
    let source = folder.path().join("source");
    std::fs::write(&source, b"a file").unwrap();
    for number in 1..=most {
        // A name as long as a phone gives a photograph.
        let name = format!("IMG_20261112_1015{number:02}.pdf");
        store
            .attach_file(&note, &source, &name, "application/pdf")
            .await
            .unwrap();
        if [1, 5, 10, most].contains(&number) {
            let size = document(note).await;
            println!("{number:>33} | {size:>14} | {:>10}", size + BLOB_OVERHEAD);
        }
    }
    let full = document(note).await;
    println!(
        "each file adds about {} bytes; a note with {most} costs {} bytes more to send at each edit",
        (full - empty) / most,
        full - empty
    );
    let refused = store
        .attach_file(&note, &source, "one-more.pdf", "application/pdf")
        .await
        .unwrap_err();
    println!("one more than {most} is refused on the device: {refused}");

    // A picture is two files: itself and its thumbnail.
    let note = store.create_note(&text).await.unwrap().id;
    let empty = document(note).await;
    let picture = folder.path().join("picture.png");
    image::RgbImage::from_pixel(1_600, 1_200, image::Rgb([30, 90, 200]))
        .save_with_format(&picture, image::ImageFormat::Png)
        .unwrap();
    for number in 1..=most / 2 {
        let attachment = store
            .attach_file(
                &note,
                &picture,
                &format!("IMG_20261112_1015{number:02}.png"),
                "image/png",
            )
            .await
            .unwrap();
        assert!(attachment.thumbnail.is_some());
    }
    let full = document(note).await;
    println!(
        "a picture with its thumbnail adds about {} bytes; {} pictures fill a note",
        (full - empty) / (most / 2),
        most / 2
    );
    let without = store
        .attach_file(&note, &picture, "one-more.png", "image/png")
        .await;
    println!(
        "one more picture: {}",
        without.map_or_else(
            |error| format!("refused: {error}"),
            |attachment| format!("attached, thumbnail: {}", attachment.thumbnail.is_some())
        )
    );
}

/// One file at its largest, and one photograph: what attaching, sending
/// and fetching each costs.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
#[ignore = "a measurement, not a check"]
async fn one_file_at_its_largest(pool: PgPool) {
    file_bucket().await;
    let server = Server::with_sign_in(pool.clone()).await;
    let laptop = Client::new(&server, "Laptop", false);
    laptop.sign_in("alice").await.unwrap();
    let phone = second_device(&server, &laptop, "Phone").await;
    let store = laptop.session.store();

    let largest = largest_file_bytes();
    println!(
        "\nthe largest file that may be attached: {largest} bytes, stored as {} (limit {})",
        sealed_size(largest),
        limits::MAX_FILE_BYTES
    );
    let source = laptop.folder().join("too-large");
    std::fs::write(&source, file_bytes(largest + 1, 1)).unwrap();
    let note = store.create_note("a large file").await.unwrap().id;
    let refused = store
        .attach_file(&note, &source, "too-large.bin", "application/octet-stream")
        .await
        .unwrap_err();
    println!("one byte more is refused on the device: {refused}");

    let bytes = file_bytes(largest, 2);
    let began = Instant::now();
    let attachment = attach(&laptop, &note, "largest.bin", &bytes).await;
    println!("attaching it: {:.1?}", began.elapsed());

    let (began, requests, storage) = (
        Instant::now(),
        laptop.transport.requests(),
        laptop.transport.storage_requests(),
    );
    let report = laptop.session.sync_now().await.unwrap();
    println!(
        "sending it with its note: {:.1?}, {} requests to the server and {} to the storage \
         (files sent: {})",
        began.elapsed(),
        laptop.transport.requests() - requests,
        laptop.transport.storage_requests() - storage,
        report.files_sent
    );
    let (counted,): (i64,) = sqlx::query_as("SELECT file_bytes FROM accounts")
        .fetch_one(&pool)
        .await
        .unwrap();
    println!("the server counts {counted} bytes against the account for it");

    phone.session.sync_now().await.unwrap();
    let (began, requests, storage) = (
        Instant::now(),
        phone.transport.requests(),
        phone.transport.storage_requests(),
    );
    let path = phone
        .session
        .fetch_file(&note, attachment.file_id)
        .await
        .unwrap();
    println!(
        "another device fetching and opening it: {:.1?}, {} requests to the server and {} to \
         the storage",
        began.elapsed(),
        phone.transport.requests() - requests,
        phone.transport.storage_requests() - storage
    );
    assert_eq!(std::fs::metadata(path).unwrap().len(), largest);

    // A photograph, which gets a thumbnail.
    let picture = laptop.folder().join("photograph.jpg");
    photograph(&picture);
    let size = std::fs::metadata(&picture).unwrap().len();
    let note = store.create_note("a photograph").await.unwrap().id;
    let began = Instant::now();
    let attachment = store
        .attach_file(&note, &picture, "photograph.jpg", "image/jpeg")
        .await
        .unwrap();
    let thumbnail = attachment.thumbnail.expect("a thumbnail");
    println!(
        "a photograph of twelve million pixels ({size} bytes as a JPEG): attached in {:.1?}, \
         its thumbnail is {} bytes",
        began.elapsed(),
        thumbnail.size
    );
    let (began, storage) = (Instant::now(), laptop.transport.storage_requests());
    laptop.session.sync_now().await.unwrap();
    println!(
        "sending it and its thumbnail: {:.1?}, {} requests to the storage",
        began.elapsed(),
        laptop.transport.storage_requests() - storage
    );
    phone.session.sync_now().await.unwrap();
    let began = Instant::now();
    phone
        .session
        .fetch_file(&note, thumbnail.file_id)
        .await
        .unwrap();
    println!(
        "another device fetching the thumbnail alone: {:.1?}",
        began.elapsed()
    );
}

/// An account holding as many bytes of files as it may: what it costs to
/// send them, and to export them on a device that holds none.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
#[ignore = "a measurement, not a check"]
async fn an_account_at_the_file_limit(pool: PgPool) {
    file_bucket().await;
    let server = Server::with_sign_in(pool.clone()).await;
    let laptop = Client::new(&server, "Laptop", false);
    laptop.sign_in("alice").await.unwrap();
    let phone = second_device(&server, &laptop, "Phone").await;
    let store = laptop.session.store();

    let largest = largest_file_bytes();
    let room = u64::try_from(limits::MAX_ACCOUNT_FILE_BYTES).unwrap();
    let fit = room / sealed_size(largest);
    println!(
        "\nan account's {room} bytes for files hold {fit} files of the largest size, \
         or {} photographs of 3 MB",
        room / sealed_size(3_000_000)
    );

    let began = Instant::now();
    for number in 0..fit {
        let note = store
            .create_note(&format!("note {number} with a large file"))
            .await
            .unwrap()
            .id;
        attach(
            &laptop,
            &note,
            &format!("large-{number}.bin"),
            &file_bytes(largest, number + 10),
        )
        .await;
    }
    println!("attaching {fit} of them: {:.1?}", began.elapsed());

    let (began, storage) = (Instant::now(), laptop.transport.storage_requests());
    let report = laptop.session.sync_now().await.unwrap();
    println!(
        "sending them: {} files in {:.1?}, {} requests to the storage",
        report.files_sent,
        began.elapsed(),
        laptop.transport.storage_requests() - storage
    );
    let (counted,): (i64,) = sqlx::query_as("SELECT file_bytes FROM accounts")
        .fetch_one(&pool)
        .await
        .unwrap();
    println!("the server counts {counted} bytes of {room}");

    // One more, of any size worth the name.
    let note = store.create_note("one file too many").await.unwrap().id;
    attach(&laptop, &note, "one-more.bin", &file_bytes(1_000_000, 5)).await;
    let report = laptop.session.sync_now().await.unwrap();
    let refused = store.file_problems().await.unwrap();
    println!(
        "one more megabyte: sent {}, waiting {}, and the device says: {}",
        report.files_sent,
        report.files_unsent,
        refused
            .first()
            .map_or("nothing", |problem| problem.reason.as_str())
    );

    // The other device has the notes and none of the files, and exports.
    phone.session.sync_now().await.unwrap();
    let (began, storage) = (Instant::now(), phone.transport.storage_requests());
    let export = Export::with_files(
        &phone.session.store(),
        Some(&phone.session),
        ExportFormat::Json,
    )
    .await
    .unwrap();
    println!(
        "a device that holds none of them gathers an export: {} in {:.1?}, {} requests to \
         the storage, {} left out",
        export.counts(),
        began.elapsed(),
        phone.transport.storage_requests() - storage,
        export.left_out().len()
    );
    let began = Instant::now();
    let again = Export::with_files(
        &phone.session.store(),
        Some(&phone.session),
        ExportFormat::Json,
    )
    .await
    .unwrap();
    println!(
        "gathering it again, with every file now on the device: {} in {:.1?}",
        again.counts(),
        began.elapsed()
    );
    let folder = phone.folder().join("export");
    let began = Instant::now();
    export.write_folder(&folder).unwrap();
    println!("writing it as a folder: {:.1?}", began.elapsed());
    let archive = phone.folder().join("export.zip");
    let began = Instant::now();
    export.write_zip(&archive, 1_800_000_000_000).unwrap();
    println!(
        "writing it as one zip file: {:.1?}, {} bytes",
        began.elapsed(),
        std::fs::metadata(&archive).unwrap().len()
    );
}

/// The notes of an account at its note limit, exported: the text is made
/// in memory, so its size is what an export of the notes costs.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "a measurement, not a check"]
async fn exporting_an_account_at_the_note_limit() {
    let notes = usize::try_from(limits::MAX_NOTES_PER_ACCOUNT).unwrap();
    let folder = tempfile::tempdir().unwrap();
    let store = dirt_core::Store::open(folder.path().join("notes.db")).unwrap();
    for number in 0..notes {
        store
            .create_note(&format!(
                "note {number}: something worth remembering, about a line long #tag{}",
                number % 50
            ))
            .await
            .unwrap();
    }
    println!("\nexporting {notes} notes of a line each:");
    for format in [ExportFormat::Json, ExportFormat::Markdown] {
        let began = Instant::now();
        let export = Export::with_files(&store, None, format).await.unwrap();
        let gathered = began.elapsed();
        let began = Instant::now();
        let text = export.notes_text().unwrap();
        let rendered = began.elapsed();
        let archive = folder.path().join(format!("{}.zip", format.extension()));
        let began = Instant::now();
        export.write_zip(&archive, 1_800_000_000_000).unwrap();
        println!(
            "{format:?}: gathered in {gathered:.1?}, {} bytes of text made in {rendered:.1?}, \
             written as a zip file in {:.1?}",
            text.len(),
            began.elapsed()
        );
    }
}

/// What the rows about files cost the database: one for each file, and one
/// for each note that refers to a file. Rows are written straight into the
/// tables, as many as give a steady figure, since only their size is
/// wanted.
#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
#[ignore = "a measurement, not a check"]
async fn what_a_files_rows_cost_the_database(pool: PgPool) {
    const ROWS: i64 = 100_000;
    let server = Server::with_sign_in(pool.clone()).await;
    let laptop = Client::new(&server, "Laptop", false);
    laptop.sign_in("alice").await.unwrap();
    let size_of = |table: &'static str| {
        let pool = pool.clone();
        async move {
            let (bytes,): (i64,) = sqlx::query_as("SELECT pg_total_relation_size($1::regclass)")
                .bind(table)
                .fetch_one(&pool)
                .await
                .unwrap();
            bytes
        }
    };
    let (files_before, notes_before) = (size_of("files").await, size_of("file_notes").await);

    sqlx::query(
        "INSERT INTO files (account_id, file_id, size, confirmed, delete_after) \
         SELECT id, gen_random_uuid(), 100, true, NULL \
           FROM accounts, generate_series(1, $1)",
    )
    .bind(ROWS)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO file_notes (account_id, file_id, note_id) \
         SELECT account_id, file_id, gen_random_uuid() FROM files",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("VACUUM ANALYZE files")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("VACUUM ANALYZE file_notes")
        .execute(&pool)
        .await
        .unwrap();

    let per_file = (size_of("files").await - files_before) / ROWS;
    let per_reference = (size_of("file_notes").await - notes_before) / ROWS;
    println!(
        "\na file costs the database {per_file} bytes, and a note's reference to one \
         {per_reference} bytes, with their indexes"
    );
    // What the limits on bytes, on files to a note and on requests would
    // allow by themselves, with no limit on how many files there are.
    let notes = limits::MAX_NOTES_PER_ACCOUNT;
    let references = notes * limits::MAX_FILES_PER_NOTE;
    let week = limits::UNCONFIRMED_FILE_DAYS * 24 * 3600 / limits::REQUEST_WINDOW_SECS
        * i64::from(limits::REQUESTS_PER_WINDOW);
    println!(
        "with no limit on their number: {notes} notes with {} files each are {references}          references, {} bytes, and {} bytes of files; and links asked for and never used, at          the most an account's requests allow in the {} days one is kept, are {week} rows, {}          bytes",
        limits::MAX_FILES_PER_NOTE,
        references * per_reference,
        references * per_file,
        limits::UNCONFIRMED_FILE_DAYS,
        week * per_file
    );
    println!(
        "with the limits of {} files and {} references: {} bytes at the most",
        limits::MAX_FILES_PER_ACCOUNT,
        limits::MAX_FILE_REFERENCES_PER_ACCOUNT,
        limits::MAX_FILES_PER_ACCOUNT * per_file
            + limits::MAX_FILE_REFERENCES_PER_ACCOUNT * per_reference
    );

    // What asking for one more link costs when the account has that many.
    let began = Instant::now();
    let (count,): (i64,) =
        sqlx::query_as("SELECT count(*) FROM files WHERE account_id = (SELECT id FROM accounts)")
            .fetch_one(&pool)
            .await
            .unwrap();
    println!(
        "counting an account's {count} files: {:.1?}",
        began.elapsed()
    );
    let began = Instant::now();
    let (count,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM file_notes WHERE account_id = (SELECT id FROM accounts)",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    println!("counting its {count} references: {:.1?}", began.elapsed());
}
