//! Two processes sharing one store file, as the desktop app and the CLI do.
//!
//! Each test starts copies of this test program as worker processes. A
//! worker is the `worker` function below: when the environment variable
//! `DIRT_STORE_TEST_WORKER` is set it does the work described there, and
//! otherwise it does nothing, which is what happens when the test suite runs
//! it as an ordinary test.
//!
//! What must hold with any number of processes on one file:
//!
//! - no call fails because the file was busy;
//! - no write is lost;
//! - a fresh file opened by several processes at the same moment is set up
//!   exactly once and opens in all of them.
//!
//! The store gets this from two rules, `BEGIN IMMEDIATE` for every write and
//! a retry around the switch to write-ahead-log mode. Trial 2 of the redesign
//! showed that each rule is needed, with a control run for each
//! (`trials/sqlite-two-process/` on the `trials/step-1` branch). The store
//! has no switch to turn a rule off, so the controls are not repeated here.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use dirt_core::notes::NoteDoc;
use dirt_core::{NoteId, Store};

const WORKER_ENV: &str = "DIRT_STORE_TEST_WORKER";

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis()
}

fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(future)
}

/// What one worker process is asked to do.
struct Job {
    path: PathBuf,
    worker: u32,
    /// Notes to create, each read back at once.
    creates: u32,
    /// Rewrites of the shared note, if there is one.
    shared_note: Option<NoteId>,
    rewrites: u32,
    /// Unix time in milliseconds at which to open the file, so that all
    /// workers open it together.
    start_at_ms: u128,
}

impl Job {
    fn encode(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}|{}",
            self.path.display(),
            self.worker,
            self.creates,
            self.shared_note.map(|id| id.as_str()).unwrap_or_default(),
            self.rewrites,
            self.start_at_ms
        )
    }

    fn decode(text: &str) -> Self {
        let parts: Vec<&str> = text.split('|').collect();
        Self {
            path: PathBuf::from(parts[0]),
            worker: parts[1].parse().unwrap(),
            creates: parts[2].parse().unwrap(),
            shared_note: (!parts[3].is_empty()).then(|| parts[3].parse().unwrap()),
            rewrites: parts[4].parse().unwrap(),
            start_at_ms: parts[5].parse().unwrap(),
        }
    }

    fn spawn(&self) -> Child {
        Command::new(std::env::current_exe().unwrap())
            .args(["worker", "--exact", "--nocapture", "--test-threads=1"])
            .env(WORKER_ENV, self.encode())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    }
}

/// The body of a worker process. Any failure panics, which the parent sees
/// as a failed exit status.
#[test]
fn worker() {
    let Ok(spec) = std::env::var(WORKER_ENV) else {
        return;
    };
    let job = Job::decode(&spec);
    let wait = job.start_at_ms.saturating_sub(now_ms());
    std::thread::sleep(Duration::from_millis(u64::try_from(wait).unwrap()));

    let store = Store::open(&job.path).unwrap();
    block_on(async {
        for index in 0..job.creates.max(job.rewrites) {
            if index < job.creates {
                let text = format!("worker {} note {index}", job.worker);
                let note = store.create_note(&text).await.unwrap();
                // Read it back through a read connection straight away.
                let read = store.get_note(&note.id).await.unwrap().unwrap();
                assert_eq!(read.content, text);
            }
            if let (Some(shared), true) = (job.shared_note, index < job.rewrites) {
                let text = format!("worker {} rewrite {index}", job.worker);
                store.update_note(&shared, &text).await.unwrap();
            }
            // Reads from this process run while the other process writes.
            store.list_notes(5, 0).await.unwrap();
        }
    });
}

/// Waits for every worker and panics with its output if one failed.
fn join(children: Vec<Child>) {
    for child in children {
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "a worker process failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

fn run_workers(path: &Path, processes: u32, creates: u32, shared: Option<NoteId>, rewrites: u32) {
    let start_at_ms = now_ms() + 400;
    let children = (1..=processes)
        .map(|worker| {
            Job {
                path: path.to_owned(),
                worker,
                creates,
                shared_note: shared,
                rewrites,
                start_at_ms,
            }
            .spawn()
        })
        .collect();
    join(children);
}

#[test]
fn two_processes_lose_no_write_and_see_no_busy_error() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("notes.db");
    // The shared note exists before the workers start; the file does too.
    let shared = {
        let store = Store::open(&path).unwrap();
        block_on(store.create_note("shared")).unwrap().id
    };

    run_workers(&path, 2, 300, Some(shared), 300);

    let store = Store::open(&path).unwrap();
    block_on(async {
        let notes = store.list_notes(10_000, 0).await.unwrap();
        // Every created note is there exactly once, plus the shared one.
        assert_eq!(notes.len(), 601);
        for worker in 1..=2 {
            for index in 0..300 {
                let text = format!("worker {worker} note {index}");
                assert_eq!(
                    notes.iter().filter(|note| note.content == text).count(),
                    1,
                    "{text:?}"
                );
            }
        }
        // Each rewrite raised the shared note's revision inside its own
        // transaction, so the count shows that none was lost.
        assert_eq!(store.note_revision(&shared).await.unwrap(), Some(601));
        let text = store.get_note(&shared).await.unwrap().unwrap().content;
        assert!(
            text == "worker 1 rewrite 299" || text == "worker 2 rewrite 299",
            "{text:?}"
        );
        // The documents written by two processes all still load.
        for (_, document) in store.note_documents().await.unwrap() {
            NoteDoc::from_bytes(42, &document).unwrap();
        }
        assert_eq!(
            store.search_notes("worker", 10_000).await.unwrap().len(),
            601
        );
    });
}

#[test]
fn four_processes_also_lose_nothing() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("notes.db");

    run_workers(&path, 4, 100, None, 0);

    let store = Store::open(&path).unwrap();
    assert_eq!(block_on(store.list_notes(10_000, 0)).unwrap().len(), 400);
}

#[test]
fn processes_opening_a_new_file_at_the_same_moment_all_succeed() {
    // Each round is a fresh file, opened by three processes at one instant.
    // Every process creates one note, so three notes mean all three opened
    // the file and the tables were set up once.
    let folder = tempfile::tempdir().unwrap();
    for round in 0..20 {
        let path = folder.path().join(format!("round-{round}.db"));
        run_workers(&path, 3, 1, None, 0);

        let store = Store::open(&path).unwrap();
        assert_eq!(
            block_on(store.list_notes(10, 0)).unwrap().len(),
            3,
            "round {round}"
        );
    }
}
