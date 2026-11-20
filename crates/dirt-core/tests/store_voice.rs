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
    // Found deleted, it was not written to, and it still waits: brought
    // back, it gets its words (step 10's review).
    assert_eq!(store.voice_waiting().await.unwrap().len(), 1);
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

/// A reading the process does not survive (a phone short of memory ends
/// the app as the model loads) still counts as a try: the phone is not
/// asked to load the model at every start for ever, and the row says why.
#[tokio::test]
async fn a_reading_that_never_came_back_counts_as_a_try() {
    let (_folder, store) = temp_store();
    let id = NoteId::new();
    store.keep_voice_note(id, recording(), 1_000).await.unwrap();
    // Each try begins and nothing comes back from it.
    let waiting = store.voice_waiting().await.unwrap().remove(0);
    for _ in 0..dirt_core::voice::MOST_TRIES {
        store.voice_reading(&waiting).await.unwrap();
    }
    let never = Arc::new(Saying {
        words: Ok("never read".to_owned()),
        asked: AtomicUsize::new(0),
    });
    make_words(&store, never.clone(), VoiceRecording::Keep, |_| {})
        .await
        .unwrap();
    assert_eq!(never.asked.load(Ordering::SeqCst), 0);
    let problems = store.voice_problems(vec![id]).await.unwrap();
    assert_eq!(
        problems.get(&id).map(String::as_str),
        Some(dirt_core::store::READING_STOPPED)
    );
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

/// A recording that waited outside the store (Android's capture sheet) is
/// one note, dated when it was said, however often it is taken in.
#[tokio::test]
async fn a_recording_taken_in_is_dated_when_it_was_said_and_made_once() {
    let (_folder, store) = temp_store();
    let said_at = dirt_core::util::now_ms() - 3_600_000;
    let id: NoteId = uuid::Uuid::new_v7(uuid::Timestamp::from_unix(
        uuid::NoContext,
        u64::try_from(said_at / 1000).unwrap(),
        u32::try_from(said_at % 1000).unwrap() * 1_000_000,
    ))
    .to_string()
    .parse()
    .unwrap();
    let note = store
        .take_in_voice_note(id, recording(), 5_000)
        .await
        .unwrap()
        .expect("made");
    assert_eq!(note.updated_at, said_at);
    assert_eq!(note.voice.unwrap().words, Words::Waiting);
    assert_eq!(store.voice_waiting().await.unwrap().len(), 1);
    // Again, after a crash before its file was removed: no second note.
    assert_eq!(
        store
            .take_in_voice_note(id, recording(), 5_000)
            .await
            .unwrap(),
        None
    );
    assert_eq!(store.list_notes(10, 0).await.unwrap().len(), 1);
    // And nothing left on the shelf for the second.
    assert_eq!(store.files_in_use().await.unwrap().len(), 1);
}

// ---- A recording added to a note being written (🎙 in Android's page) ----

/// What is said into a note being written goes among its files at once,
/// unmarked, and its words come at the note's end, after what was typed,
/// meanwhile too.
#[tokio::test]
async fn a_recording_added_to_a_note_puts_its_words_at_the_end() {
    let (_folder, store) = temp_store();
    let note = store.create_note("Groceries").await.unwrap();
    let added = store
        .add_voice_to_note(&note.id, recording())
        .await
        .unwrap();
    // Not a note said: no mark, no name from the recording.
    assert_eq!(added.voice, None);
    assert_eq!(added.content, "Groceries");
    let files = store.note_attachments(&note.id).await.unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].name, VOICE_FILE_NAME);

    let waiting = store.voice_waiting().await.unwrap();
    assert_eq!(waiting.len(), 1);
    assert!(waiting[0].added);
    assert_eq!(waiting[0].file_id, files[0].file_id);

    // Typed on while its words were made.
    store
        .update_note(&note.id, "Groceries\n- bread")
        .await
        .unwrap();
    let ear = Arc::new(Saying {
        words: Ok(" and oat milk ".to_owned()),
        asked: AtomicUsize::new(0),
    });
    let made = make_words(&store, ear, VoiceRecording::Keep, |_| {})
        .await
        .unwrap();
    assert!(made.changed());
    let note = store.get_note(&note.id).await.unwrap().unwrap();
    assert_eq!(note.content, "Groceries\n- bread\n\nand oat milk");
    assert_eq!(note.voice, None);
    assert_eq!(store.note_attachments(&note.id).await.unwrap().len(), 1);
    assert!(store.voice_waiting().await.unwrap().is_empty());
}

/// A note may be added to more than once; each recording's words come in
/// the order they were said. With Text only each recording goes once its
/// words are in, and a recording with no words stays.
#[tokio::test]
async fn several_recordings_added_each_put_their_words_in_turn() {
    let (_folder, store) = temp_store();
    let note = store.create_note("Plan\n").await.unwrap();
    store
        .add_voice_to_note(&note.id, recording())
        .await
        .unwrap();
    store
        .add_voice_to_note(&note.id, recording())
        .await
        .unwrap();
    let waiting = store.voice_waiting().await.unwrap();
    assert_eq!(waiting.len(), 2);
    let put = store
        .put_words_of(&waiting[0], "first", true)
        .await
        .unwrap();
    let WordsPut::Made(after) = put else {
        panic!("{put:?}")
    };
    assert_eq!(after.content, "Plan\n\nfirst");
    // Text only: that recording is off the note.
    assert_eq!(store.note_attachments(&note.id).await.unwrap().len(), 1);
    // The second had no words: the note is as it was, recording and all.
    let put = store.put_words_of(&waiting[1], "  ", true).await.unwrap();
    assert!(matches!(put, WordsPut::NoneFound(_)));
    assert_eq!(store.note_attachments(&note.id).await.unwrap().len(), 1);
    let note = store.get_note(&note.id).await.unwrap().unwrap();
    assert_eq!(note.content, "Plan\n\nfirst");
    assert!(store.voice_waiting().await.unwrap().is_empty());
}

/// A recording taken off the note before its words were made is not read,
/// and its words are never put in.
#[tokio::test]
async fn a_recording_taken_off_the_note_is_not_read() {
    let (_folder, store) = temp_store();
    let note = store.create_note("Call the bank").await.unwrap();
    store
        .add_voice_to_note(&note.id, recording())
        .await
        .unwrap();
    let file = store.note_attachments(&note.id).await.unwrap()[0].file_id;
    let waiting = store.voice_waiting().await.unwrap().remove(0);
    store.detach_file(&note.id, file).await.unwrap();
    assert!(store.voice_waiting().await.unwrap().is_empty());
    // Words that were being made as it was taken off are not put in.
    assert_eq!(
        store
            .put_words_of(&waiting, "too late", false)
            .await
            .unwrap(),
        WordsPut::Nothing
    );
    let note = store.get_note(&note.id).await.unwrap().unwrap();
    assert_eq!(note.content, "Call the bank");
}

/// Only this device makes the words of what was said into a note here; the
/// note reaches another as a note with one more file.
#[tokio::test]
async fn only_the_device_that_heard_it_makes_the_words_of_a_recording_added() {
    let (_here, heard) = temp_store();
    let (_there, other) = temp_store();
    let note = heard.create_note("Offsite").await.unwrap();
    heard
        .add_voice_to_note(&note.id, recording())
        .await
        .unwrap();
    let arrived = other
        .merge_note_document(&note.id, document(&heard, &note.id).await)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(arrived.content, "Offsite");
    assert!(other.voice_waiting().await.unwrap().is_empty());
}

/// Moved into the account as the device signs in, a recording added to a
/// note still gets its words there, at the note's end.
#[tokio::test]
async fn a_recording_added_to_a_note_moved_into_the_account_still_gets_its_words() {
    let (_before, signed_out) = temp_store();
    let (_after, account) = temp_store();
    let note = signed_out.create_note("Ideas").await.unwrap();
    signed_out
        .add_voice_to_note(&note.id, recording())
        .await
        .unwrap();
    assert_eq!(signed_out.move_notes_into(&account).await.unwrap(), 1);
    let waiting = account.voice_waiting().await.unwrap();
    assert_eq!(waiting.len(), 1);
    assert!(waiting[0].added);
    let ear = Arc::new(Saying {
        words: Ok("a garden".to_owned()),
        asked: AtomicUsize::new(0),
    });
    make_words(&account, ear, VoiceRecording::Keep, |_| {})
        .await
        .unwrap();
    let note = account.get_note(&note.id).await.unwrap().unwrap();
    assert_eq!(note.content, "Ideas\n\na garden");
}

/// Three failed tries, and the note's row says why, as for a note said.
#[tokio::test]
async fn a_recording_added_whose_words_cannot_be_made_is_given_up_and_said() {
    let (_folder, store) = temp_store();
    let note = store.create_note("Notes").await.unwrap();
    store
        .add_voice_to_note(&note.id, recording())
        .await
        .unwrap();
    let broken = Arc::new(Saying {
        words: Err("the model would not load".to_owned()),
        asked: AtomicUsize::new(0),
    });
    for _ in 0..=dirt_core::voice::MOST_TRIES {
        make_words(&store, broken.clone(), VoiceRecording::Keep, |_| {})
            .await
            .unwrap();
    }
    assert_eq!(
        broken.asked.load(Ordering::SeqCst),
        dirt_core::voice::MOST_TRIES as usize
    );
    let problems = store.voice_problems(vec![note.id]).await.unwrap();
    assert_eq!(
        problems.get(&note.id).map(String::as_str),
        Some("the model would not load")
    );
    // The recording stays on the note: nothing said is lost.
    assert_eq!(store.note_attachments(&note.id).await.unwrap().len(), 1);
}

/// A note that is not there, or is deleted, takes no recording.
#[tokio::test]
async fn a_recording_is_not_added_to_a_note_that_is_gone() {
    let (_folder, store) = temp_store();
    let note = store.create_note("Gone soon").await.unwrap();
    store.delete_note(&note.id).await.unwrap();
    assert!(matches!(
        store.add_voice_to_note(&note.id, recording()).await,
        Err(dirt_core::Error::NotFound(_))
    ));
    assert!(matches!(
        store.add_voice_to_note(&NoteId::new(), recording()).await,
        Err(dirt_core::Error::NotFound(_))
    ));
}

/// What the page typed at the end while the words were being made stays
/// where it was typed, and the words come after it: the page's save, made
/// from the text it saved before, is merged into the note that has the
/// words.
#[tokio::test]
async fn typing_at_the_end_while_the_words_are_made_stays_before_them() {
    let (_folder, store) = temp_store();
    let note = store.create_note("Buy milk, eg").await.unwrap();
    store
        .add_voice_to_note(&note.id, recording())
        .await
        .unwrap();
    let waiting = store.voice_waiting().await.unwrap().remove(0);
    store
        .put_words_of(&waiting, "call mum", false)
        .await
        .unwrap();
    // The page saved "Buy milk, eg" and has typed "gs" since.
    let saved = store
        .update_note_from(&note.id, "Buy milk, eg", "Buy milk, eggs")
        .await
        .unwrap();
    assert_eq!(saved.content, "Buy milk, eggs\n\ncall mum");
}

/// Two makers of words reading the same recording at once (the app's and
/// the sheet's job, in one process) put its words in once.
#[tokio::test]
async fn the_words_of_a_recording_added_are_put_in_once() {
    let (_folder, store) = temp_store();
    let note = store.create_note("Plan").await.unwrap();
    store
        .add_voice_to_note(&note.id, recording())
        .await
        .unwrap();
    let waiting = store.voice_waiting().await.unwrap().remove(0);
    let first = store.put_words_of(&waiting, "once", false).await.unwrap();
    assert!(matches!(first, WordsPut::Made(_)));
    let second = store.put_words_of(&waiting, "once", false).await.unwrap();
    assert_eq!(second, WordsPut::Nothing);
    let note = store.get_note(&note.id).await.unwrap().unwrap();
    assert_eq!(note.content, "Plan\n\nonce");
}

/// A note deleted while its recording is read keeps the recording waiting,
/// and brought back it gets its words: for a note said and for a recording
/// added alike.
#[tokio::test]
async fn words_made_while_the_note_is_deleted_wait_for_it_to_come_back() {
    let (_folder, store) = temp_store();
    let note = store.create_note("Errands").await.unwrap();
    store
        .add_voice_to_note(&note.id, recording())
        .await
        .unwrap();
    let said = NoteId::new();
    store
        .keep_voice_note(said, recording(), 2_000)
        .await
        .unwrap();
    let waiting = store.voice_waiting().await.unwrap();
    assert_eq!(waiting.len(), 2);
    store.delete_note(&note.id).await.unwrap();
    store.delete_note(&said).await.unwrap();
    for one in &waiting {
        assert_eq!(
            store.put_words_of(one, "late words", false).await.unwrap(),
            WordsPut::Nothing
        );
    }
    store.restore_note(&note.id).await.unwrap();
    store.restore_note(&said).await.unwrap();
    assert_eq!(store.voice_waiting().await.unwrap().len(), 2);
    let ear = Arc::new(Saying {
        words: Ok("back again".to_owned()),
        asked: AtomicUsize::new(0),
    });
    make_words(&store, ear, VoiceRecording::Keep, |_| {})
        .await
        .unwrap();
    let note = store.get_note(&note.id).await.unwrap().unwrap();
    assert_eq!(note.content, "Errands\n\nback again");
    let said = store.get_note(&said).await.unwrap().unwrap();
    assert_eq!(said.content, "back again");
}

/// Moved into the account as the device signs in, a recording given up on
/// stays given up: its tries go with it.
#[tokio::test]
async fn a_recording_given_up_on_stays_given_up_when_moved() {
    let (_before, signed_out) = temp_store();
    let (_after, account) = temp_store();
    let note = signed_out.create_note("Ideas").await.unwrap();
    signed_out
        .add_voice_to_note(&note.id, recording())
        .await
        .unwrap();
    let said = NoteId::new();
    signed_out
        .keep_voice_note(said, recording(), 2_000)
        .await
        .unwrap();
    let broken = Arc::new(Saying {
        words: Err("the model would not load".to_owned()),
        asked: AtomicUsize::new(0),
    });
    for _ in 0..dirt_core::voice::MOST_TRIES {
        make_words(&signed_out, broken.clone(), VoiceRecording::Keep, |_| {})
            .await
            .unwrap();
    }
    assert_eq!(signed_out.move_notes_into(&account).await.unwrap(), 2);
    let ear = Arc::new(Saying {
        words: Ok("not again".to_owned()),
        asked: AtomicUsize::new(0),
    });
    make_words(&account, ear.clone(), VoiceRecording::Keep, |_| {})
        .await
        .unwrap();
    assert_eq!(ear.asked.load(Ordering::SeqCst), 0);
    let problems = account.voice_problems(vec![note.id, said]).await.unwrap();
    assert_eq!(problems.len(), 2);
}
