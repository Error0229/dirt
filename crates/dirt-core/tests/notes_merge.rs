//! Merging concurrent edits from two devices.

use dirt_core::notes::NoteDoc;

// Client IDs must be below 2^53. These are arbitrary large values, so that the
// encoded size reflects real IDs and not small test numbers.
const A: u64 = 7_100_000_000_000_001;
const B: u64 = 8_200_000_000_000_002;

/// Two devices that start from the same synced note.
fn pair(text: &str) -> (NoteDoc, NoteDoc) {
    let a = NoteDoc::new(A);
    a.set_text(text, 1);
    let b = NoteDoc::from_bytes(B, &a.encode()).expect("load");
    (a, b)
}

/// Exchanges full state in both directions.
fn sync(a: &NoteDoc, b: &NoteDoc) {
    let (from_a, from_b) = (a.encode(), b.encode());
    a.merge(&from_b).expect("merge into a");
    b.merge(&from_a).expect("merge into b");
}

#[test]
fn edits_to_different_words_both_survive_exactly_once() {
    let (a, b) = pair("buy milk today");
    a.set_text("buy bread today", 2);
    b.set_text("buy milk tomorrow", 3);

    sync(&a, &b);

    assert_eq!(a.text(), "buy bread tomorrow");
    assert_eq!(b.text(), "buy bread tomorrow");
}

#[test]
fn edits_to_different_words_merge_in_non_ascii_text() {
    let (a, b) = pair("買牛奶 🥛 今天");
    a.set_text("買麵包 🥛 今天", 2);
    b.set_text("買牛奶 🥛 明天", 3);

    sync(&a, &b);

    assert_eq!(a.text(), "買麵包 🥛 明天");
    assert_eq!(b.text(), "買麵包 🥛 明天");
}

#[test]
fn one_device_changing_two_places_merges_with_an_edit_in_between() {
    let (a, b) = pair("alpha beta gamma delta");
    // A changes the first and last words in one save.
    a.set_text("ALPHA beta gamma DELTA", 2);
    // B changes a word in the middle.
    b.set_text("alpha beta GAMMA delta", 3);

    sync(&a, &b);

    assert_eq!(a.text(), "ALPHA beta GAMMA DELTA");
    assert_eq!(b.text(), "ALPHA beta GAMMA DELTA");
}

#[test]
fn edits_to_the_same_word_converge_to_one_result() {
    let (a, b) = pair("call Sam");
    a.set_text("call Samantha", 2);
    b.set_text("call Samuel", 3);

    sync(&a, &b);

    // The merged word is some interleaving of both. What matters is that both
    // devices end up with the same text and that nothing either typed is lost.
    assert_eq!(a.text(), b.text());
    let merged = a.text();
    assert!(merged.starts_with("call Sam"), "got {merged:?}");
    assert!(merged.contains("antha"), "got {merged:?}");
    assert!(merged.contains("uel"), "got {merged:?}");
}

#[test]
fn merging_is_idempotent_and_order_independent() {
    let (a, b) = pair("one two three");
    a.set_text("one 2 three", 2);
    b.set_text("one two 3", 3);
    let (from_a, from_b) = (a.encode(), b.encode());

    // A fresh observer merges the two states in both orders, with repeats.
    let ab = NoteDoc::new(9_000_000_000_000_009);
    ab.merge(&from_a).expect("merge");
    ab.merge(&from_b).expect("merge");
    ab.merge(&from_a).expect("repeat merge");

    let ba = NoteDoc::new(9_000_000_000_000_009);
    ba.merge(&from_b).expect("merge");
    ba.merge(&from_b).expect("repeat merge");
    ba.merge(&from_a).expect("merge");

    assert_eq!(ab.text(), "one 2 3");
    assert_eq!(ba.text(), "one 2 3");
    assert_eq!(ab.encode(), ba.encode(), "same bytes whatever the order");
}

#[test]
fn an_unchanged_save_writes_nothing() {
    let (a, _) = pair("same text");
    let before = a.encode();
    assert!(!a.set_text("same text", 99));
    assert_eq!(a.encode(), before);
}
