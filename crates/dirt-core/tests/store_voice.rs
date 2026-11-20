//! Notes that were said (interface design 6.18): a recording kept as a
//! note at once, and its words made afterwards by the device that recorded
//! it alone.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use dirt_core::models::{VoiceRecording, Words};
use dirt_core::store::{LetGo, WordsPut, VOICE_FILE_NAME, VOICE_MEDIA_TYPE};
use dirt_core::voice::{make_words, Transcriber};
use dirt_core::{NoteId, Store};

fn temp_store() -> (tempfile::TempDir, Store) {
    let folder = tempfile::tempdir().unwrap();
    let store = Store::open(folder.path().join("notes.db")).unwrap();
    (folder, store)
}

/// Not a real recording: the store does not read what it keeps.
fn recording() -> Vec<u8> {
    b"OggS a recording of twelve seconds".to_vec()
}

async fn document(store: &Store, id: &NoteId) -> Vec<u8> {
    store
        .note_documents()
        .await
        .unwrap()
        .into_iter()
        .find(|(held, _)| held == id)
        .unwrap()
        .1
}

#[tokio::test]
async fn a_recording_is_kept_as_a_note_at_once() {
    let (_folder, store) = temp_store();
    let id = NoteId::new();
    let note = store
        .keep_voice_note(id, recording(), 12_300)
        .await
        .unwrap();

    assert_eq!(note.id, id);
    assert_eq!(note.content, "");
    let voice = note.voice.unwrap();
    assert_eq!(voice.words, Words::Waiting);
    assert_eq!(voice.length(), "0:12");
    // It is in the list, named by its length until it has its words.
    let listed = store.list_notes(10, 0).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].voice, note.voice);

    let files = store.note_attachments(&id).await.unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].name, VOICE_FILE_NAME);
    assert_eq!(files[0].media_type, VOICE_MEDIA_TYPE);
    assert_eq!(
        store.file_bytes(files[0].file_id).await.unwrap(),
        Some(recording())
    );

    let waiting = store.voice_waiting().await.unwrap();
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0].note, id);
    assert_eq!(waiting[0].file_id, files[0].file_id);

    // A note with a file is not let go for having no words.
    assert_eq!(store.let_go_if_empty(&id).await.unwrap(), LetGo::Kept);
}

#[tokio::test]
async fn its_words_are_put_at_the_start_of_the_note() {
    let (_folder, store) = temp_store();
    let id = NoteId::new();
    store.keep_voice_note(id, recording(), 4_000).await.unwrap();
    // Typed on the note while its words were being made.
    store.update_note(&id, "and #milk").await.unwrap();

    let put = store
        .put_voice_words(&id, "  Buy oat milk and stamps. ", false)
        .await
        .unwrap();
    let WordsPut::Made(note) = put else {
        panic!("{put:?}")
    };
    assert_eq!(note.content, "Buy oat milk and stamps.\n\nand #milk");
    assert_eq!(note.voice.unwrap().words, Words::Made);
    assert_eq!(store.note_attachments(&id).await.unwrap().len(), 1);
    assert!(store.voice_waiting().await.unwrap().is_empty());
    // Found by its words.
    assert_eq!(store.search_notes("stamps", 10).await.unwrap().len(), 1);

    // Done once: asked again, nothing is added twice.
    assert_eq!(
        store.put_voice_words(&id, "again", false).await.unwrap(),
        WordsPut::Nothing
    );
    let note = store.get_note(&id).await.unwrap().unwrap();
    assert_eq!(note.content, "Buy oat milk and stamps.\n\nand #milk");
}

#[tokio::test]
async fn text_only_takes_the_recording_off_once_the_words_are_in() {
    let (_folder, store) = temp_store();
    let id = NoteId::new();
    store.keep_voice_note(id, recording(), 4_000).await.unwrap();
    let put = store
        .put_voice_words(&id, "Call the dentist", true)
        .await
        .unwrap();
    let WordsPut::Made(note) = put else {
        panic!("{put:?}")
    };
    assert_eq!(note.content, "Call the dentist");
    // A note like any other: no file, no mic.
    assert_eq!(note.voice, None);
    assert!(store.note_attachments(&id).await.unwrap().is_empty());
}

#[tokio::test]
async fn no_words_found_keeps_the_recording_and_its_name() {
    let (_folder, store) = temp_store();
    let id = NoteId::new();
    store.keep_voice_note(id, recording(), 2_000).await.unwrap();
    let before = store.note_revision(&id).await.unwrap();
    let edited = store.get_note(&id).await.unwrap().unwrap().updated_at;

    // Text only was chosen; with no words the recording stays all the same.
    let put = store.put_voice_words(&id, " \n", true).await.unwrap();
    let WordsPut::NoneFound(note) = put else {
        panic!("{put:?}")
    };
    assert_eq!(note.content, "");
    assert_eq!(note.voice.unwrap().words, Words::NoneFound);
    assert_eq!(store.note_attachments(&id).await.unwrap().len(), 1);
    assert!(store.voice_waiting().await.unwrap().is_empty());
    // Not an edit: the note keeps its time. It is sent all the same.
    assert_eq!(note.updated_at, edited);
    assert!(store.note_revision(&id).await.unwrap() > before);
}

#[tokio::test]
async fn a_deleted_note_waits_and_is_not_written_to() {
    let (_folder, store) = temp_store();
    let id = NoteId::new();
    store.keep_voice_note(id, recording(), 2_000).await.unwrap();
    store.delete_note(&id).await.unwrap();
    assert!(store.voice_waiting().await.unwrap().is_empty());
    // Brought back, it waits again.
    store.restore_note(&id).await.unwrap();
    assert_eq!(store.voice_waiting().await.unwrap().len(), 1);

    store.delete_note(&id).await.unwrap();
    assert_eq!(
        store.put_voice_words(&id, "too late", false).await.unwrap(),
        WordsPut::Nothing
    );
    store.restore_note(&id).await.unwrap();
    // It was let go of when it was found deleted.
    assert!(store.voice_waiting().await.unwrap().is_empty());
    assert_eq!(store.get_note(&id).await.unwrap().unwrap().content, "");
}

#[tokio::test]
async fn only_the_device_that_recorded_it_makes_its_words() {
    let (_here, recorded) = temp_store();
    let (_there, other) = temp_store();
    let id = NoteId::new();
    recorded
        .keep_voice_note(id, recording(), 7_000)
        .await
        .unwrap();

    let arrived = other
        .merge_note_document(&id, document(&recorded, &id).await)
        .await
        .unwrap()
        .unwrap();
    // The other device sees a recording waiting for its words, and does
    // not make them.
    assert_eq!(arrived.voice.unwrap().words, Words::Waiting);
    assert!(other.voice_waiting().await.unwrap().is_empty());

    recorded
        .put_voice_words(&id, "Ask Jordan about the offsite", false)
        .await
        .unwrap();
    let arrived = other
        .merge_note_document(&id, document(&recorded, &id).await)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(arrived.content, "Ask Jordan about the offsite");
    assert_eq!(arrived.voice.unwrap().words, Words::Made);
}

#[tokio::test]
async fn a_note_moved_into_the_account_still_gets_its_words() {
    let (_before, signed_out) = temp_store();
    let (_after, account) = temp_store();
    let id = NoteId::new();
    signed_out
        .keep_voice_note(id, recording(), 3_000)
        .await
        .unwrap();
    assert_eq!(signed_out.move_notes_into(&account).await.unwrap(), 1);

    let waiting = account.voice_waiting().await.unwrap();
    assert_eq!(waiting.len(), 1);
    assert_eq!(
        account.file_bytes(waiting[0].file_id).await.unwrap(),
        Some(recording())
    );
}

/// A transcriber that says what it was told to, and counts.
struct Saying {
    words: Result<String, String>,
    asked: AtomicUsize,
}

impl Transcriber for Saying {
    fn words(&self, recording: &[u8]) -> Result<String, String> {
        assert_eq!(recording, recording_bytes());
        self.asked.fetch_add(1, Ordering::SeqCst);
        self.words.clone()
    }
}

fn recording_bytes() -> Vec<u8> {
    recording()
}

#[tokio::test]
async fn words_are_made_in_the_background_and_a_failure_waits_for_the_next_try() {
    let (_folder, store) = temp_store();
    let first = NoteId::new();
    let second = NoteId::new();
    store
        .keep_voice_note(first, recording(), 1_000)
        .await
        .unwrap();
    store
        .keep_voice_note(second, recording(), 1_000)
        .await
        .unwrap();

    let failing = Arc::new(Saying {
        words: Err("the engine could not start".to_owned()),
        asked: AtomicUsize::new(0),
    });
    let made = make_words(&store, failing.clone(), VoiceRecording::Keep, |_| {})
        .await
        .unwrap();
    assert_eq!(failing.asked.load(Ordering::SeqCst), 2);
    assert_eq!(made.failed.len(), 2);
    assert!(!made.changed());
    let waiting = store.voice_waiting().await.unwrap();
    assert_eq!(waiting.len(), 2);
    assert!(waiting.iter().all(|w| w.tries == 1));
    // The notes are as they were kept.
    assert_eq!(store.get_note(&first).await.unwrap().unwrap().content, "");

    let working = Arc::new(Saying {
        words: Ok("Water the plants".to_owned()),
        asked: AtomicUsize::new(0),
    });
    let made = make_words(&store, working.clone(), VoiceRecording::TextOnly, |_| {})
        .await
        .unwrap();
    assert!(made.changed());
    assert_eq!(made.put.len(), 2);
    assert!(store.voice_waiting().await.unwrap().is_empty());
    let note = store.get_note(&second).await.unwrap().unwrap();
    assert_eq!(note.content, "Water the plants");
    // Text only was chosen.
    assert!(store.note_attachments(&second).await.unwrap().is_empty());

    // Nothing waits: nothing is read.
    make_words(&store, working.clone(), VoiceRecording::Keep, |_| {})
        .await
        .unwrap();
    assert_eq!(working.asked.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn a_store_written_before_voice_opens_and_has_no_voice_notes() {
    let (_folder, store) = temp_store();
    let typed = store.create_note("typed").await.unwrap();
    assert_eq!(typed.voice, None);
    assert!(store.voice_waiting().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_recording_whose_words_cannot_be_made_is_given_up_and_says_why() {
    let (_folder, store) = temp_store();
    let id = NoteId::new();
    store.keep_voice_note(id, recording(), 1_000).await.unwrap();
    let failing = Arc::new(Saying {
        words: Err("the model could not read it".to_owned()),
        asked: AtomicUsize::new(0),
    });
    for _ in 0..dirt_core::voice::MOST_TRIES + 2 {
        make_words(&store, failing.clone(), VoiceRecording::Keep, |_| {})
            .await
            .unwrap();
    }
    // Tried no more than that, each try loading the model.
    assert_eq!(
        failing.asked.load(Ordering::SeqCst),
        dirt_core::voice::MOST_TRIES as usize
    );
    let problems = store.voice_problems(vec![id]).await.unwrap();
    assert_eq!(
        problems.get(&id).map(String::as_str),
        Some("the model could not read it")
    );
    // The recording stays, waiting, on the note.
    assert_eq!(store.note_attachments(&id).await.unwrap().len(), 1);
    let note = store.get_note(&id).await.unwrap().unwrap();
    assert_eq!(note.voice.unwrap().words, Words::Waiting);
}

/// The words are told of as each note's are put in, not after the round.
#[tokio::test]
async fn each_note_is_told_of_as_its_words_are_put_in() {
    let (_folder, store) = temp_store();
    let first = NoteId::new();
    let second = NoteId::new();
    store
        .keep_voice_note(first, recording(), 1_000)
        .await
        .unwrap();
    store
        .keep_voice_note(second, recording(), 1_000)
        .await
        .unwrap();
    let ear = Arc::new(Saying {
        words: Ok("Said".to_owned()),
        asked: AtomicUsize::new(0),
    });
    let mut told = Vec::new();
    let watched = Arc::clone(&ear);
    make_words(&store, ear, VoiceRecording::Keep, |put| {
        // Told before the next recording is read.
        told.push((
            watched.asked.load(Ordering::SeqCst),
            matches!(put, WordsPut::Made(_)),
        ));
    })
    .await
    .unwrap();
    assert_eq!(told, [(1, true), (2, true)]);
}
