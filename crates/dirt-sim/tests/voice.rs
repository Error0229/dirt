//! Thoughts said aloud (interface design 6.18), with whole devices against
//! the real server and a real file storage.
//!
//! A recording is kept as a note with the recording as its file; it
//! reaches the other devices encrypted, as any file does. Its words are
//! made by the device that recorded it, and by no other, and reach the
//! others as an edit.
//!
//! Start the database and the file storage with `docker compose up -d` at
//! the repository root and set `DATABASE_URL` as `compose.yaml` shows.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use dirt_core::models::{VoiceRecording, Words};
use dirt_core::session::SessionState;
use dirt_core::voice::{make_words, Transcriber};
use dirt_proto::NoteId;
use dirt_sim::{file_bucket, file_store, Client, Server};
use sqlx::PgPool;
use uuid::Uuid;

async fn two_devices(server: &Server) -> (Client, Client) {
    let laptop = Client::new(server, "Laptop", false);
    laptop.sign_in("alice").await.unwrap();
    let phone = Client::new(server, "Phone", false);
    let state = phone.sign_in("alice").await.unwrap();
    assert!(matches!(state, SessionState::WaitingForApproval { .. }));
    phone.approve_with_key(server, &laptop.account_key()).await;
    (laptop, phone)
}

/// Stands in for a recording: the store and sync do not read what they
/// keep. Not all one byte, so that a stretch of it can be looked for.
fn recording() -> Vec<u8> {
    (0..20_000u32)
        .map(|i| u8::try_from((i * 7) % 253).unwrap())
        .collect()
}

/// Says the same words every time, and counts how often it was asked.
struct Saying {
    words: &'static str,
    asked: AtomicUsize,
}

impl Transcriber for Saying {
    fn words(&self, _recording: &[u8]) -> Result<String, String> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        Ok(self.words.to_owned())
    }
}

async fn stored(pool: &PgPool, file: Uuid) -> Vec<u8> {
    let account: Uuid = sqlx::query_scalar("SELECT id FROM accounts")
        .fetch_one(pool)
        .await
        .unwrap();
    let link = file_store().download_link(&dirt_api::FileStore::object_key(account, file));
    reqwest::get(link)
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap()
        .to_vec()
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn a_thought_said_on_one_device_gets_its_words_there_and_reaches_the_other(pool: PgPool) {
    file_bucket().await;
    let server = Server::with_sign_in(pool.clone()).await;
    let (laptop, phone) = two_devices(&server).await;
    let id = NoteId::new();
    let said = recording();
    laptop
        .session
        .store()
        .keep_voice_note(id, said.clone(), 4_200)
        .await
        .unwrap();

    let report = laptop.session.sync_now().await.unwrap();
    assert_eq!((report.files_sent, report.files_failed), (1, false));
    let file = laptop.session.store().note_attachments(&id).await.unwrap()[0].file_id;
    // The storage holds the recording encrypted: no stretch of it is there.
    let object = stored(&pool, file).await;
    assert!(object.len() > said.len());
    assert!(!object.windows(64).any(|window| window == &said[..64]));

    // The phone has the note, named by its length, and the recording, and
    // does not make the words itself.
    phone.session.sync_now().await.unwrap();
    let there = phone.session.store().get_note(&id).await.unwrap().unwrap();
    assert_eq!(there.content, "");
    assert_eq!(there.voice.unwrap().words, Words::Waiting);
    assert_eq!(there.voice.unwrap().length(), "0:04");
    let path = phone.session.fetch_file(&id, file).await.unwrap();
    assert_eq!(std::fs::read(path).unwrap(), said);
    let phones_ear = Arc::new(Saying {
        words: "not the phone's to say",
        asked: AtomicUsize::new(0),
    });
    let made = make_words(
        &phone.session.store(),
        phones_ear.clone(),
        VoiceRecording::Keep,
    )
    .await
    .unwrap();
    assert!(!made.changed());
    assert_eq!(phones_ear.asked.load(Ordering::SeqCst), 0);

    // The laptop makes them, and they reach the phone as an edit.
    let laptops_ear = Arc::new(Saying {
        words: "Ask Jordan about the offsite",
        asked: AtomicUsize::new(0),
    });
    let made = make_words(
        &laptop.session.store(),
        laptops_ear.clone(),
        VoiceRecording::Keep,
    )
    .await
    .unwrap();
    assert!(made.changed());
    laptop.session.sync_now().await.unwrap();
    phone.session.sync_now().await.unwrap();
    let there = phone.session.store().get_note(&id).await.unwrap().unwrap();
    assert_eq!(there.content, "Ask Jordan about the offsite");
    assert_eq!(there.voice.unwrap().words, Words::Made);
    assert_eq!(laptops_ear.asked.load(Ordering::SeqCst), 1);
}

#[sqlx::test(migrator = "dirt_api::MIGRATOR")]
async fn text_only_takes_the_recording_off_every_device(pool: PgPool) {
    file_bucket().await;
    let server = Server::with_sign_in(pool.clone()).await;
    let (laptop, phone) = two_devices(&server).await;
    let id = NoteId::new();
    laptop
        .session
        .store()
        .keep_voice_note(id, recording(), 2_000)
        .await
        .unwrap();
    laptop.session.sync_now().await.unwrap();
    phone.session.sync_now().await.unwrap();
    assert_eq!(
        phone
            .session
            .store()
            .note_attachments(&id)
            .await
            .unwrap()
            .len(),
        1
    );

    let ear = Arc::new(Saying {
        words: "Water the plants",
        asked: AtomicUsize::new(0),
    });
    make_words(&laptop.session.store(), ear, VoiceRecording::TextOnly)
        .await
        .unwrap();
    laptop.session.sync_now().await.unwrap();
    phone.session.sync_now().await.unwrap();
    let there = phone.session.store().get_note(&id).await.unwrap().unwrap();
    assert_eq!(there.content, "Water the plants");
    assert_eq!(there.voice, None);
    assert!(phone
        .session
        .store()
        .note_attachments(&id)
        .await
        .unwrap()
        .is_empty());
}
