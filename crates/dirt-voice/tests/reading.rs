//! The engine itself: the model reading speech, kept as Dirt keeps it.
//!
//! The speech is the two recordings that come with the model's archive
//! (`test_wavs/`). The tests that need the model are ignored by default,
//! because most of CI builds without it (`DIRT_VOICE_MODEL=skip`); CI's
//! voice job runs them with `--include-ignored`, as does
//! `cargo test -p dirt-voice -- --include-ignored` on a computer that built
//! with the model.

#![cfg(voice_engine)]

use std::path::PathBuf;

use dirt_voice::reading::words_of;
use dirt_voice::{recording, stand_in, Listening, VoiceFolder, RATE};

fn folder() -> VoiceFolder {
    VoiceFolder::built().expect("the voice folder this crate was built with")
}

fn sample(name: &str) -> PathBuf {
    let model = env!("DIRT_VOICE_BUILT_MODEL");
    assert!(!model.is_empty(), "built without the model");
    PathBuf::from(model).join("test_wavs").join(name)
}

/// The words, in lower case and without punctuation, for comparing.
fn plain(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
#[ignore = "needs the speech model: cargo test -p dirt-voice -- --include-ignored"]
fn a_recording_kept_as_a_note_keeps_its_words() {
    let folder = folder();
    let sound = stand_in::read_wav(&folder, &sample("0.wav")).unwrap();
    let file = recording::encode(&sound).unwrap();
    let words = words_of(&folder, &file).unwrap();
    println!("{words}");
    let plain = plain(&words);
    // "Well, I don't wish to see it anymore, observed Phoebe, turning
    // away her eyes. It is certainly very like the old portrait."
    for word in ["observed", "phoebe", "turning", "eyes", "portrait"] {
        assert!(plain.contains(word), "{word:?} is not in {words:?}");
    }
    // Capitals and punctuation, as a person would type them.
    assert!(words.starts_with(char::is_uppercase), "{words:?}");
}

#[test]
#[ignore = "needs the speech model: cargo test -p dirt-voice -- --include-ignored"]
fn a_long_recording_is_read_in_pieces_and_no_word_is_lost_at_a_join() {
    let folder = folder();
    let one = stand_in::read_wav(&folder, &sample("0.wav")).unwrap();
    let alone = plain(&words_of(&folder, &recording::encode(&one).unwrap()).unwrap());
    // The same speech four times, with 0.4 s of quiet between, as trial 1's
    // long recording had: longer than one piece.
    let mut long = Vec::new();
    for _ in 0..4 {
        long.extend_from_slice(&one);
        long.extend(std::iter::repeat_n(0.0, RATE as usize * 2 / 5));
    }
    assert!(long.len() > 25 * RATE as usize);
    let file = recording::encode(&long).unwrap();
    let words = plain(&words_of(&folder, &file).unwrap());
    println!("{words}");
    let count = |text: &str| text.split_whitespace().count();
    assert!(
        count(&words) + 2 >= 4 * count(&alone),
        "{} words for four times {}",
        count(&words),
        count(&alone)
    );
    assert_eq!(words.matches("observed phoebe").count(), 4, "{words}");
    assert_eq!(words.matches("old portrait").count(), 4, "{words}");
}

#[test]
fn listening_hears_speech_and_keeps_the_recording() {
    let folder = folder();
    let with_model = !env!("DIRT_VOICE_BUILT_MODEL").is_empty();
    // Built without the model's archive there is no speech to play: a
    // second of quiet stands in, and only the recording is checked.
    let sound = if with_model {
        stand_in::read_wav(&folder, &sample("en-english.wav")).unwrap()
    } else {
        vec![0.0; RATE as usize]
    };
    let mut listening = Listening::start(&folder).unwrap();
    let mut heard = None;
    for piece in sound.chunks(RATE as usize / 10) {
        heard = Some(listening.take(piece).unwrap());
    }
    let quiet = vec![0.0001_f32; RATE as usize * 2];
    for piece in quiet.chunks(RATE as usize / 10) {
        heard = Some(listening.take(piece).unwrap());
    }
    let heard = heard.unwrap();
    let recording = listening.finish().unwrap();
    let expected_ms = ((sound.len() + quiet.len()) * 1000 / RATE as usize) as u64;
    assert_eq!(recording.duration_ms, expected_ms);
    assert_eq!(
        recording::decode(&recording.file).unwrap().len(),
        sound.len() + quiet.len()
    );
    if with_model {
        assert!(heard.speech && recording.speech);
        // Two seconds of quiet after the speech, as the detector counts it.
        let quiet_ms = heard.quiet_ms.expect("quiet after speech");
        assert!((1_200..=2_100).contains(&quiet_ms), "{quiet_ms} ms");
    }
}

#[test]
fn quiet_alone_is_nothing_heard() {
    let folder = folder();
    let mut listening = Listening::start(&folder).unwrap();
    let room = vec![0.0002_f32; RATE as usize * 3];
    for piece in room.chunks(1_600) {
        let heard = listening.take(piece).unwrap();
        assert!(!heard.speech);
        assert!(heard.level < 0.2);
    }
    let recording = listening.finish().unwrap();
    assert!(!recording.speech);
    assert_eq!(recording.duration_ms, 3_000);
}

/// A user folder named in letters beyond ASCII (`C:\Users\José`) holds the
/// app, and so the voice folder: the engine must open the model there.
#[test]
#[ignore = "needs the speech model: cargo test -p dirt-voice -- --include-ignored"]
fn the_model_is_read_from_a_folder_whose_name_is_not_ascii() {
    let built = folder();
    let parent = tempfile::tempdir().unwrap();
    let there = parent.path().join("Jos\u{e9} \u{65e5}\u{672c}");
    std::fs::create_dir(&there).unwrap();
    for entry in std::fs::read_dir(built.path()).unwrap() {
        let source = entry.unwrap().path();
        let target = there.join(source.file_name().unwrap());
        if std::fs::hard_link(&source, &target).is_err() {
            std::fs::copy(&source, &target).unwrap();
        }
    }
    let moved = VoiceFolder::at(&there).unwrap();
    let sound = stand_in::read_wav(&moved, &sample("0.wav")).unwrap();
    let words = words_of(&moved, &recording::encode(&sound).unwrap()).unwrap();
    assert!(plain(&words).contains("phoebe"), "{words}");
}
