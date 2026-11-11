//! The deletion rule: a delete covers only what the deleting device had seen.

use dirt_core::notes::NoteDoc;
use yrs::updates::decoder::Decode;
use yrs::{ClientID, Doc, Options, ReadTxn, StateVector, Text, Transact, Update};

const A: u64 = 7_100_000_000_000_001;
const B: u64 = 8_200_000_000_000_002;
const C: u64 = 8_900_000_000_000_003;

fn pair(text: &str) -> (NoteDoc, NoteDoc) {
    let a = NoteDoc::new(A);
    a.set_text(text, 1);
    let b = NoteDoc::from_bytes(B, &a.encode()).expect("load");
    (a, b)
}

fn sync(a: &NoteDoc, b: &NoteDoc) {
    let (from_a, from_b) = (a.encode(), b.encode());
    a.merge(&from_b).expect("merge into a");
    b.merge(&from_a).expect("merge into b");
}

#[test]
fn a_new_note_is_not_deleted() {
    let (a, b) = pair("note");
    assert!(!a.is_deleted());
    assert!(!b.is_deleted());
}

#[test]
fn a_delete_reaches_the_other_device() {
    let (a, b) = pair("note");
    a.delete();
    sync(&a, &b);
    assert!(a.is_deleted());
    assert!(b.is_deleted());
}

#[test]
fn an_edit_the_deleter_had_not_seen_brings_the_note_back() {
    let (a, b) = pair("buy milk");
    a.delete();
    b.set_text("buy milk and eggs", 5);

    sync(&a, &b);

    assert!(!a.is_deleted());
    assert!(!b.is_deleted());
    assert_eq!(a.text(), "buy milk and eggs");
    assert_eq!(b.text(), "buy milk and eggs");
}

#[test]
fn an_unseen_edit_that_only_removes_text_also_brings_the_note_back() {
    let (a, b) = pair("buy milk and eggs");
    a.delete();
    // B only removes characters. No text is inserted.
    b.set_text("buy milk", 5);

    sync(&a, &b);

    assert!(!a.is_deleted());
    assert!(!b.is_deleted());
    assert_eq!(a.text(), "buy milk");
}

#[test]
fn an_edit_the_deleter_had_seen_stays_deleted() {
    let (a, b) = pair("buy milk");
    b.set_text("buy milk and eggs", 5);
    sync(&a, &b);
    // A deletes after seeing B's edit.
    a.delete();
    sync(&a, &b);

    assert!(a.is_deleted());
    assert!(b.is_deleted());
}

#[test]
fn deleting_on_both_devices_stays_deleted() {
    let (a, b) = pair("note");
    a.delete();
    b.delete();
    sync(&a, &b);
    assert!(a.is_deleted());
    assert!(b.is_deleted());
}

#[test]
fn edit_then_delete_on_one_device_and_delete_on_the_other_stays_deleted() {
    // A edits and then deletes. B, offline, deletes the older state. With a
    // single shared marker, B's older marker could win and the note would come
    // back although both devices deleted it. With one marker per device, A's
    // marker still covers everything.
    let (a, b) = pair("note");
    a.set_text("note, longer", 5);
    a.delete();
    b.delete();

    sync(&a, &b);

    assert!(a.is_deleted());
    assert!(b.is_deleted());
}

#[test]
fn restoring_brings_the_note_back_everywhere() {
    let (a, b) = pair("note");
    a.delete();
    sync(&a, &b);
    assert!(b.is_deleted());

    b.restore(9);
    sync(&a, &b);

    assert!(!a.is_deleted());
    assert!(!b.is_deleted());
    assert_eq!(a.text(), "note");
}

#[test]
fn a_restored_note_can_be_deleted_again() {
    let (a, b) = pair("note");
    a.delete();
    sync(&a, &b);
    b.restore(9);
    sync(&a, &b);

    a.delete();
    sync(&a, &b);

    assert!(a.is_deleted());
    assert!(b.is_deleted());
}

#[test]
fn a_third_device_can_delete_a_note_that_came_back() {
    let (a, b) = pair("buy milk");
    a.delete();
    b.set_text("buy milk and eggs", 5);
    sync(&a, &b);
    assert!(!a.is_deleted(), "the unseen edit brought it back");

    // C sees everything, then deletes.
    let c = NoteDoc::from_bytes(C, &a.encode()).expect("load");
    c.delete();
    sync(&a, &c);
    sync(&b, &c);

    assert!(a.is_deleted());
    assert!(b.is_deleted());
    assert!(c.is_deleted());
}

#[test]
fn deleting_does_not_change_the_text_or_count_as_an_edit() {
    let (a, _) = pair("note");
    let before = a.version_vector();
    a.delete();
    assert_eq!(a.version_vector(), before);
    assert_eq!(a.text(), "note");
    assert_eq!(a.edited_at_ms(), Some(1));
}

/// Why the rule keeps its own version vector instead of comparing yrs state
/// vectors: an edit that only removes characters leaves the yrs state vector
/// unchanged, so a marker based on it would treat that edit as already seen.
#[test]
fn the_yrs_state_vector_does_not_move_on_a_delete_only_edit() {
    let doc = Doc::with_options(Options::with_client_id(ClientID::new(A)));
    let body = doc.get_or_insert_text("body");
    body.insert(&mut doc.transact_mut(), 0, "buy milk and eggs");
    let full = doc
        .transact()
        .encode_state_as_update_v2(&StateVector::default());

    let other = Doc::with_options(Options::with_client_id(ClientID::new(B)));
    let other_body = other.get_or_insert_text("body");
    other
        .transact_mut()
        .apply_update(Update::decode_v2(&full).expect("decode"))
        .expect("apply");

    let before = other.transact().state_vector();
    other_body.remove_range(&mut other.transact_mut(), 8, 9);
    let after = other.transact().state_vector();

    assert_eq!(
        before, after,
        "removing text did not advance the state vector"
    );
}
