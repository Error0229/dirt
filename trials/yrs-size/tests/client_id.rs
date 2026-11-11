//! Client IDs: one per install, shared by the desktop app and the CLI.

use yrs_size_trial::NoteDoc;

const DEVICE: u64 = 7_100_000_000_000_001;
const OTHER: u64 = 8_200_000_000_000_002;

#[test]
fn two_processes_sharing_an_id_are_safe_when_each_edit_reloads_the_note() {
    // The desktop app and the CLI share one client ID and one local database.
    // Each edit loads the stored document, changes it and stores it again, in
    // one write transaction, so the two never write from the same starting
    // state. This models fifty alternating edits.
    let first = NoteDoc::new(DEVICE);
    first.set_text("log:", 0);
    let mut stored = first.encode();

    for round in 0..50 {
        let who = if round % 2 == 0 { "desktop" } else { "cli" };
        let process = NoteDoc::from_bytes(DEVICE, &stored).expect("load");
        let text = format!("{} {who}{round}", process.text());
        process.set_text(&text, round + 1);
        stored = process.encode();
    }

    let reader = NoteDoc::from_bytes(OTHER, &stored).expect("load");
    let text = reader.text();
    for round in 0..50 {
        let who = if round % 2 == 0 { "desktop" } else { "cli" };
        assert!(text.contains(&format!("{who}{round}")), "missing round {round}: {text}");
    }
    assert_eq!(reader.version_vector().get(&DEVICE), Some(&51));
}

#[test]
fn a_healthy_device_is_not_told_it_lost_state() {
    let device = NoteDoc::new(DEVICE);
    device.set_text("one", 1);
    let other = NoteDoc::from_bytes(OTHER, &device.encode()).expect("load");
    other.set_text("one two", 2);

    assert!(!device
        .incoming_knows_more_of_me(&other.encode())
        .expect("check"));
}

#[test]
fn a_device_restored_from_an_old_backup_detects_it() {
    let device = NoteDoc::new(DEVICE);
    device.set_text("one", 1);
    let backup = device.encode();
    device.set_text("one two", 2);
    device.set_text("one two three", 3);
    let on_server = device.encode();

    // The device loses its newer state and comes back from the backup.
    let restored = NoteDoc::from_bytes(DEVICE, &backup).expect("load");

    assert!(restored
        .incoming_knows_more_of_me(&on_server)
        .expect("check"));
}

#[test]
fn writing_under_the_old_id_after_losing_state_loses_the_new_edit() {
    // This is the hazard the check above guards against. The restored device
    // writes again under its old ID, so its new edit reuses sequence numbers
    // the server already holds. The server then treats the new edit as
    // something it has seen and drops it.
    let device = NoteDoc::new(DEVICE);
    device.set_text("one", 1);
    let backup = device.encode();
    device.set_text("one two", 2);
    let server = NoteDoc::from_bytes(OTHER, &device.encode()).expect("load");

    let restored = NoteDoc::from_bytes(DEVICE, &backup).expect("load");
    restored.set_text("one THREE", 3);

    server.merge(&restored.encode()).expect("merge");
    restored.merge(&server.encode()).expect("merge");

    assert!(
        !server.text().contains("THREE"),
        "expected the reused-ID edit to be lost on the server, got {:?}",
        server.text()
    );
}

#[test]
fn switching_to_a_new_id_after_losing_state_keeps_the_new_edit() {
    let device = NoteDoc::new(DEVICE);
    device.set_text("one", 1);
    let backup = device.encode();
    device.set_text("one two", 2);
    let server = NoteDoc::from_bytes(OTHER, &device.encode()).expect("load");

    // The restored device notices the problem and takes a new client ID before
    // it writes anything.
    let restored = NoteDoc::from_bytes(DEVICE, &backup).expect("load");
    assert!(restored
        .incoming_knows_more_of_me(&server.encode())
        .expect("check"));
    let fresh_id = 8_800_000_000_000_008;
    let recovered = NoteDoc::from_bytes(fresh_id, &server.encode()).expect("load");
    recovered.set_text(&format!("{} THREE", recovered.text()), 3);

    server.merge(&recovered.encode()).expect("merge");

    assert_eq!(server.text(), "one two THREE");
    assert_eq!(recovered.text(), "one two THREE");
}
