//! Runs the trial binary as tests, so `cargo test` covers it.

use std::process::Command;

fn trial(mode: &str, args: &[&str]) -> (bool, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_sqlite-two-process-trial"))
        .arg(mode)
        .args(args)
        .output()
        .expect("the trial binary should start");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
    )
}

#[test]
fn two_processes_with_begin_immediate_lose_nothing_and_see_no_busy_error() {
    let (pass, report) = trial("run", &["--rule", "immediate", "--writes", "1000"]);
    assert!(pass, "{report}");
}

#[test]
fn four_processes_with_begin_immediate_also_pass() {
    let (pass, report) = trial(
        "run",
        &["--rule", "immediate", "--writes", "500", "--processes", "4"],
    );
    assert!(pass, "{report}");
}

/// The control for the two tests above. With SQLite's default `BEGIN`, a
/// transaction that reads and then writes fails with a busy error that no
/// timeout prevents, whenever another process wrote between its read and its
/// write. If this ever passes, those tests prove nothing about
/// `BEGIN IMMEDIATE`.
#[test]
fn two_processes_with_plain_begin_do_see_busy_errors() {
    let (pass, report) = trial("run", &["--rule", "deferred", "--writes", "1000"]);
    assert!(!pass, "the control unexpectedly passed:\n{report}");
    assert!(
        report.contains("lost writes: none"),
        "busy errors are expected, lost writes are not:\n{report}"
    );
}

#[test]
fn two_processes_can_open_a_fresh_file_at_the_same_moment() {
    let (pass, report) = trial("startup", &["--open", "retry", "--rounds", "30"]);
    assert!(pass, "{report}");
}

/// The control for the test above: without the retry, the switch to
/// write-ahead-log mode fails in one of the two processes in most rounds.
#[test]
fn without_the_retry_a_simultaneous_first_open_fails() {
    let (pass, report) = trial("startup", &["--open", "plain", "--rounds", "30"]);
    assert!(!pass, "the control unexpectedly passed:\n{report}");
    assert!(report.contains("open: database is locked"), "{report}");
}
