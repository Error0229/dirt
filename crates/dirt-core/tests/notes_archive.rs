//! Archive in the note's document: a mark that is not an edit.
//!
//! Archiving takes a note out of the list and changes nothing else about
//! it. So it must not move the note's edit time, and must not count as an
//! edit to the deletion rule: a note that one device deleted stays deleted
//! when another archives it.

use dirt_core::notes::NoteDoc;
use yrs::updates::decoder::Decode;
use yrs::{ClientID, Doc, Map, Options, ReadTxn, StateVector, Text, Transact, Update};

const A: u64 = 7_100_000_000_000_001;
const B: u64 = 8_200_000_000_000_002;
const C: u64 = 8_900_000_000_000_003;

fn pair(text: &str) -> (NoteDoc, NoteDoc) {
    let a = NoteDoc::new(A);
    a.set_text(text, 1_000);
    let b = NoteDoc::from_bytes(B, &a.encode()).expect("load");
    (a, b)
}

fn sync(a: &NoteDoc, b: &NoteDoc) {
    let (from_a, from_b) = (a.encode(), b.encode());
    a.merge(&from_b).expect("merge into a");
    b.merge(&from_a).expect("merge into b");
}

/// Everything about a note that a person can see.
fn standing(doc: &NoteDoc) -> (String, bool, bool, Option<i64>) {
    (
        doc.text(),
        doc.is_archived(),
        doc.is_deleted(),
        doc.edited_at_ms(),
    )
}

#[test]
fn a_new_note_is_not_archived() {
    let (a, b) = pair("note");
    assert!(!a.is_archived());
    assert!(!b.is_archived());
}

#[test]
fn archiving_and_bringing_back_reach_the_other_device() {
    let (a, b) = pair("bread was bought");

    assert!(a.set_archived(true));
    sync(&a, &b);
    assert!(a.is_archived());
    assert!(b.is_archived());

    assert!(b.set_archived(false));
    sync(&a, &b);
    assert!(!a.is_archived());
    assert!(!b.is_archived());
}

#[test]
fn archiving_is_not_an_edit() {
    let (a, _b) = pair("bread was bought");
    let (counts, edited_at) = (a.version_vector(), a.edited_at_ms());

    a.set_archived(true);
    assert_eq!(a.version_vector(), counts, "no edit count moved");
    assert_eq!(a.edited_at_ms(), edited_at, "the edit time stayed");
    assert_eq!(a.text(), "bread was bought");

    a.set_archived(false);
    assert_eq!(a.version_vector(), counts);
    assert_eq!(a.edited_at_ms(), edited_at);
}

#[test]
fn asking_for_what_the_note_already_is_writes_nothing() {
    let (a, _b) = pair("note");
    let before = a.encode();
    assert!(!a.set_archived(false), "it is not archived");
    assert_eq!(a.encode(), before);

    assert!(a.set_archived(true));
    let archived = a.encode();
    assert!(!a.set_archived(true), "it is archived already");
    assert_eq!(a.encode(), archived);
}

#[test]
fn an_archived_note_that_is_edited_stays_archived() {
    let (a, b) = pair("buy milk");
    a.set_archived(true);
    sync(&a, &b);

    b.set_text("buy milk and eggs", 2_000);
    sync(&a, &b);

    assert!(a.is_archived());
    assert!(b.is_archived());
    assert_eq!(a.text(), "buy milk and eggs");
}

#[test]
fn an_edit_and_an_archive_made_apart_both_hold() {
    let (a, b) = pair("buy milk");
    a.set_archived(true);
    b.set_text("buy milk and eggs", 2_000);

    sync(&a, &b);

    assert_eq!(standing(&a), standing(&b));
    assert_eq!(
        standing(&a),
        ("buy milk and eggs".to_owned(), true, false, Some(2_000))
    );
}

// ---- Two devices choose differently ----

/// One device archives the note. The other archives it and brings it back,
/// so that its last word is "not archived". Neither has seen the other.
fn two_choices() -> (NoteDoc, NoteDoc) {
    let (a, b) = pair("note");
    a.set_archived(true);
    b.set_archived(true);
    b.set_archived(false);
    assert_ne!(a.is_archived(), b.is_archived());
    (a, b)
}

#[test]
fn when_two_devices_choose_differently_one_choice_holds_on_both() {
    let (a, b) = two_choices();
    sync(&a, &b);
    assert_eq!(a.is_archived(), b.is_archived());
    assert_eq!(standing(&a), standing(&b));
}

#[test]
fn the_choice_that_holds_does_not_depend_on_the_order_of_merging() {
    // A third device hears from the two in one order, a fourth in the
    // other, and a fifth from a device that has already merged them.
    let (a, b) = two_choices();
    let (from_a, from_b) = (a.encode(), b.encode());

    let a_first = NoteDoc::new(C);
    a_first.merge(&from_a).unwrap();
    a_first.merge(&from_b).unwrap();

    let b_first = NoteDoc::new(C + 1);
    b_first.merge(&from_b).unwrap();
    b_first.merge(&from_a).unwrap();

    sync(&a, &b);
    let second_hand = NoteDoc::from_bytes(C + 2, &a.encode()).unwrap();

    let held = a.is_archived();
    for doc in [&b, &a_first, &b_first, &second_hand] {
        assert_eq!(doc.is_archived(), held);
    }
    // Merging the same documents again changes nothing.
    a_first.merge(&from_a).unwrap();
    a_first.merge(&from_b).unwrap();
    assert_eq!(a_first.is_archived(), held);
}

#[test]
fn the_same_choice_made_on_two_devices_is_that_choice() {
    let (a, b) = pair("note");
    a.set_archived(true);
    b.set_archived(true);
    sync(&a, &b);
    assert!(a.is_archived());
    assert!(b.is_archived());
}

// ---- Archive and delete ----

#[test]
fn a_note_deleted_on_one_device_and_archived_on_another_ends_deleted_on_both() {
    let (a, b) = pair("a password typed in the wrong place");
    a.delete();
    // B has not heard of the delete, and clears the note out of its list.
    b.set_archived(true);

    sync(&a, &b);

    assert!(a.is_deleted());
    assert!(b.is_deleted());
}

#[test]
fn bringing_a_note_back_from_the_archive_does_not_bring_it_back_from_a_delete() {
    let (a, b) = pair("note");
    a.set_archived(true);
    sync(&a, &b);

    a.delete();
    b.set_archived(false);
    sync(&a, &b);

    assert!(a.is_deleted());
    assert!(b.is_deleted());
}

#[test]
fn an_edit_brings_a_deleted_note_back_where_an_archive_did_not() {
    let (a, b) = pair("buy milk");
    a.delete();
    b.set_archived(true);
    sync(&a, &b);
    assert!(b.is_deleted());

    // The edit that the delete had not seen is what brings a note back.
    let (a, b) = pair("buy milk");
    a.delete();
    b.set_archived(true);
    b.set_text("buy milk and eggs", 2_000);
    sync(&a, &b);
    assert!(!a.is_deleted());
    assert!(!b.is_deleted());
    assert!(
        a.is_archived(),
        "and it comes back as it was left: archived"
    );
}

#[test]
fn a_deleted_note_that_was_archived_is_restored_archived() {
    let (a, b) = pair("note");
    a.set_archived(true);
    a.delete();
    sync(&a, &b);
    assert!(b.is_deleted());

    b.restore(3_000);
    sync(&a, &b);

    assert_eq!(standing(&a), standing(&b));
    assert!(!a.is_deleted());
    assert!(a.is_archived());
}

// ---- Documents and builds from before archive ----

/// A note as a build from before archive holds it: the four parts that
/// build knew, and the same way of recording an edit.
struct OldBuild {
    doc: Doc,
    client_id: u64,
    edits_made: u64,
}

impl OldBuild {
    fn new(client_id: u64) -> Self {
        let doc = Doc::with_options(Options::with_client_id(ClientID::new(client_id)));
        doc.get_or_insert_text("body");
        doc.get_or_insert_map("edits");
        doc.get_or_insert_map("deletions");
        doc.get_or_insert_map("files");
        Self {
            doc,
            client_id,
            edits_made: 0,
        }
    }

    fn merge(&self, bytes: &[u8]) {
        let update = Update::decode_v2(bytes).expect("decode");
        self.doc.transact_mut().apply_update(update).expect("apply");
    }

    /// Adds `text` at the end of the note, as an edit.
    fn append(&mut self, text: &str, now_ms: i64) {
        let body = self.doc.get_or_insert_text("body");
        let edits = self.doc.get_or_insert_map("edits");
        let mut txn = self.doc.transact_mut();
        let end = body.len(&txn);
        body.insert(&mut txn, end, text);
        self.edits_made += 1;
        edits.insert(
            &mut txn,
            self.client_id.to_string(),
            format!("{}:{now_ms}", self.edits_made),
        );
    }

    fn encode(&self) -> Vec<u8> {
        self.doc
            .transact()
            .encode_state_as_update_v2(&StateVector::default())
    }
}

#[test]
fn a_document_from_before_archive_reads_as_not_archived() {
    let mut old = OldBuild::new(A);
    old.append("written before archive", 1_000);

    let doc = NoteDoc::from_bytes(B, &old.encode()).unwrap();

    assert!(!doc.is_archived());
    assert_eq!(doc.text(), "written before archive");
    assert_eq!(doc.edited_at_ms(), Some(1_000));
}

#[test]
fn the_mark_survives_a_merge_with_a_document_that_lacks_it() {
    let mut old = OldBuild::new(A);
    old.append("buy milk", 1_000);
    let new = NoteDoc::from_bytes(B, &old.encode()).unwrap();
    new.set_archived(true);

    // The old build writes on, and its document, which has never held the
    // mark, is merged into the one that has it.
    old.append(" and eggs", 2_000);
    new.merge(&old.encode()).unwrap();

    assert!(new.is_archived());
    assert_eq!(new.text(), "buy milk and eggs");
}

/// What the design promises of a build from before archive: it keeps the
/// mark in a document it merges, edits and sends on, though it never asks
/// yrs for the map the mark is in.
#[test]
fn a_build_from_before_archive_keeps_the_mark_when_it_merges_and_edits() {
    let new = NoteDoc::new(B);
    new.set_text("buy milk", 1_000);
    new.set_archived(true);

    let mut old = OldBuild::new(A);
    old.merge(&new.encode());
    old.append(" and eggs", 2_000);
    // What the old build stores, and pushes for every device to merge.
    let from_old = old.encode();

    // A device that has never seen the note takes it from the old build.
    let fresh = NoteDoc::from_bytes(C, &from_old).unwrap();
    assert!(fresh.is_archived());
    assert_eq!(fresh.text(), "buy milk and eggs");
    assert_eq!(fresh.edited_at_ms(), Some(2_000));

    // And the old build's copy still takes a later choice.
    new.merge(&from_old).unwrap();
    new.set_archived(false);
    old.merge(&new.encode());
    let fresh = NoteDoc::from_bytes(C + 1, &old.encode()).unwrap();
    assert!(!fresh.is_archived());
}

/// A mark this version cannot read is not taken for "archived", and is not
/// lost by being loaded and stored.
#[test]
fn a_mark_with_a_value_this_version_cannot_read_reads_as_not_archived() {
    let later = Doc::with_options(Options::with_client_id(ClientID::new(A)));
    let marks = later.get_or_insert_map("marks");
    marks.insert(&mut later.transact_mut(), "archived", "until Friday");
    let bytes = later
        .transact()
        .encode_state_as_update_v2(&StateVector::default());

    let doc = NoteDoc::from_bytes(B, &bytes).unwrap();
    assert!(!doc.is_archived());

    // This version's own choice replaces it.
    assert!(doc.set_archived(true));
    assert!(doc.is_archived());
}
