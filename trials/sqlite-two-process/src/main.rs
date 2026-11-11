//! Trial 2, desktop half: can two processes safely share one SQLite file?
//!
//! The desktop app and the CLI will both open the same local database. This
//! program stands in for both.
//!
//! ```text
//! sqlite-two-process-trial run     [--rule immediate|deferred] [--open retry|plain]
//!                                  [--writes 1000] [--processes 2]
//!                                  [--busy-timeout-ms 5000]
//!                                  [--think-us 200] [--pause-us 300]
//! sqlite-two-process-trial startup [--open retry|plain] [--rounds 30]
//!                                  [--processes 2]
//! ```
//!
//! `run` creates a fresh database, starts the worker processes at the same
//! moment, and checks the file afterwards. Each worker opens the file, sets
//! it up if nobody has yet, and then does its write transactions on one
//! thread while a second thread in the same process reads.
//!
//! `startup` repeats only the first part, on a fresh file each round, and
//! counts the workers that failed to open the database.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, ErrorCode, TransactionBehavior};

type AnyError = Box<dyn std::error::Error + Send + Sync>;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let outcome = match args.first().map(String::as_str) {
        Some("run") => run(&Options::parse(&args[1..])),
        Some("startup") => startup(&Options::parse(&args[1..])),
        Some("worker") => worker(&Options::parse(&args[1..])),
        _ => Err("usage: sqlite-two-process-trial run|startup|worker [--option value]...".into()),
    };
    match outcome {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(2)
        }
    }
}

#[derive(Clone)]
struct Options {
    /// How a write transaction begins.
    rule: Rule,
    /// Whether opening retries the switch to write-ahead-log mode.
    open_retry: bool,
    writes: u32,
    busy_timeout_ms: u64,
    processes: u32,
    /// Time spent inside each write transaction between its read and its
    /// write, standing in for the work a real caller does there.
    think_us: u64,
    /// Time between two write transactions of one worker. Without it the
    /// first worker to get the lock keeps it almost continuously, and the
    /// workers run one after the other instead of interleaving.
    pause_us: u64,
    /// `startup` only: how many fresh files to race on.
    rounds: u32,
    /// Worker only: the database file and this worker's number.
    path: PathBuf,
    id: u32,
    /// Worker only: the Unix time in milliseconds at which to start, so all
    /// workers open the file together.
    start_at_ms: u128,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Rule {
    /// `BEGIN IMMEDIATE`: take the write lock before reading anything.
    Immediate,
    /// `BEGIN`: read first, and ask for the write lock at the first write.
    /// This is SQLite's default and the control case.
    Deferred,
}

impl Options {
    fn parse(args: &[String]) -> Self {
        let mut options = Self {
            rule: Rule::Immediate,
            open_retry: true,
            writes: 1000,
            busy_timeout_ms: 5000,
            processes: 2,
            think_us: 200,
            pause_us: 300,
            rounds: 30,
            path: PathBuf::new(),
            id: 0,
            start_at_ms: 0,
        };
        for pair in args.chunks(2) {
            let [key, value] = pair else { continue };
            match key.as_str() {
                "--rule" => {
                    options.rule = if value == "deferred" {
                        Rule::Deferred
                    } else {
                        Rule::Immediate
                    };
                }
                "--open" => options.open_retry = value != "plain",
                "--writes" => options.writes = value.parse().unwrap_or(options.writes),
                "--busy-timeout-ms" => {
                    options.busy_timeout_ms = value.parse().unwrap_or(options.busy_timeout_ms);
                }
                "--processes" => options.processes = value.parse().unwrap_or(options.processes),
                "--think-us" => options.think_us = value.parse().unwrap_or(options.think_us),
                "--pause-us" => options.pause_us = value.parse().unwrap_or(options.pause_us),
                "--rounds" => options.rounds = value.parse().unwrap_or(options.rounds),
                "--path" => options.path = PathBuf::from(value),
                "--id" => options.id = value.parse().unwrap_or(0),
                "--start-at-ms" => options.start_at_ms = value.parse().unwrap_or(0),
                _ => {}
            }
        }
        options
    }

    const fn rule_name(&self) -> &'static str {
        match self.rule {
            Rule::Immediate => "immediate",
            Rule::Deferred => "deferred",
        }
    }

    const fn open_name(&self) -> &'static str {
        if self.open_retry {
            "retry"
        } else {
            "plain"
        }
    }

    /// The command that starts one worker with these options.
    fn worker_command(&self, path: &Path, id: u32, start_at_ms: u128) -> std::io::Result<Command> {
        let mut command = Command::new(std::env::current_exe()?);
        command
            .args(["worker", "--rule", self.rule_name()])
            .args(["--open", self.open_name()])
            .args(["--writes", &self.writes.to_string()])
            .args(["--busy-timeout-ms", &self.busy_timeout_ms.to_string()])
            .args(["--think-us", &self.think_us.to_string()])
            .args(["--pause-us", &self.pause_us.to_string()])
            .args(["--id", &id.to_string()])
            .args(["--start-at-ms", &start_at_ms.to_string()])
            .arg("--path")
            .arg(path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        Ok(command)
    }

    /// Starts every worker before waiting for any, so they run together.
    fn spawn_workers(
        &self,
        path: &Path,
        lead_ms: u128,
    ) -> std::io::Result<Vec<std::process::Child>> {
        let start_at_ms = now_ms() + lead_ms;
        (1..=self.processes)
            .map(|id| self.worker_command(path, id, start_at_ms)?.spawn())
            .collect()
    }
}

/// Spins for `micros`. `thread::sleep` on Windows cannot wait for much less
/// than a millisecond.
fn spin(micros: u64) {
    let until = Instant::now() + Duration::from_micros(micros);
    while Instant::now() < until {
        std::hint::spin_loop();
    }
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis())
}

fn is_busy(error: &rusqlite::Error) -> bool {
    matches!(
        error.sqlite_error_code(),
        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked)
    )
}

/// Opens the file the way every Dirt process will.
///
/// The busy timeout is set first, but it does not cover the switch to
/// write-ahead-log mode: when two processes switch a fresh file at the same
/// moment, SQLite fails one of them at once with "database is locked"
/// instead of waiting. With `retry`, that failure is retried until the busy
/// timeout has passed. The mode is stored in the file, so the retry finds the
/// switch already made.
fn open(path: &Path, busy_timeout_ms: u64, retry: bool) -> rusqlite::Result<Connection> {
    let connection = Connection::open(path)?;
    let timeout = Duration::from_millis(busy_timeout_ms);
    connection.busy_timeout(timeout)?;

    let started = Instant::now();
    loop {
        match connection.pragma_update(None, "journal_mode", "WAL") {
            Ok(()) => break,
            Err(error) if retry && is_busy(&error) && started.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(error),
        }
    }
    // NORMAL by default. Set TRIAL_SYNCHRONOUS=FULL to make every commit wait
    // for the disk, which is the setting step 3 proposes; workers inherit it.
    let synchronous = std::env::var("TRIAL_SYNCHRONOUS").unwrap_or_else(|_| "NORMAL".to_owned());
    connection.pragma_update(None, "synchronous", synchronous)?;
    Ok(connection)
}

/// Creates the tables unless another process already has. The version check
/// happens inside the write transaction, so two processes starting together
/// cannot both run the set-up.
fn migrate(connection: &mut Connection) -> rusqlite::Result<bool> {
    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let version: i64 = tx.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let migrated = version == 0;
    if migrated {
        tx.execute_batch(
            "CREATE TABLE log (
                 writer INTEGER NOT NULL,
                 seq    INTEGER NOT NULL,
                 total  INTEGER NOT NULL,
                 PRIMARY KEY (writer, seq)
             );
             CREATE TABLE counter (id INTEGER PRIMARY KEY CHECK (id = 1), n INTEGER NOT NULL);
             INSERT INTO counter (id, n) VALUES (1, 0);",
        )?;
        tx.pragma_update(None, "user_version", 1)?;
    }
    tx.commit()?;
    Ok(migrated)
}

/// One write transaction: read the counter, add one, and log the new total.
/// A lost update would show up as two log rows with the same total.
fn write_once(
    connection: &mut Connection,
    rule: Rule,
    think_us: u64,
    id: u32,
    seq: u32,
) -> rusqlite::Result<()> {
    let behavior = match rule {
        Rule::Immediate => TransactionBehavior::Immediate,
        Rule::Deferred => TransactionBehavior::Deferred,
    };
    let tx = connection.transaction_with_behavior(behavior)?;
    let current: i64 = tx.query_row("SELECT n FROM counter WHERE id = 1", [], |row| row.get(0))?;
    spin(think_us);
    tx.execute("UPDATE counter SET n = ?1 WHERE id = 1", [current + 1])?;
    tx.execute(
        "INSERT INTO log (writer, seq, total) VALUES (?1, ?2, ?3)",
        (id, seq, current + 1),
    )?;
    tx.commit()
}

/// Reads until told to stop. Each read checks, inside one read transaction,
/// that the counter equals the number of log rows: a reader must never see
/// half a write. Returns the reads done, the torn reads and the busy errors.
fn read_until(
    path: &Path,
    busy_timeout_ms: u64,
    stop: &AtomicBool,
) -> Result<(u64, u64, u64), String> {
    let mut connection = open(path, busy_timeout_ms, true).map_err(|e| e.to_string())?;
    let (mut reads, mut torn, mut busy) = (0u64, 0u64, 0u64);
    while !stop.load(Ordering::Relaxed) {
        let result = (|| -> rusqlite::Result<(i64, i64)> {
            let tx = connection.transaction()?;
            let n = tx.query_row("SELECT n FROM counter WHERE id = 1", [], |r| r.get(0))?;
            let rows = tx.query_row("SELECT count(*) FROM log", [], |r| r.get(0))?;
            tx.commit()?;
            Ok((n, rows))
        })();
        match result {
            Ok((n, rows)) => {
                reads += 1;
                torn += u64::from(n != rows);
            }
            Err(error) if is_busy(&error) => busy += 1,
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok((reads, torn, busy))
}

fn worker(options: &Options) -> Result<bool, AnyError> {
    // Wait for the shared start time so the workers race from the first open.
    let now = now_ms();
    if options.start_at_ms > now {
        let wait = u64::try_from(options.start_at_ms - now)?;
        std::thread::sleep(Duration::from_millis(wait));
    }

    let mut connection = open(&options.path, options.busy_timeout_ms, options.open_retry)
        .map_err(|error| format!("worker {}: open: {error}", options.id))?;
    let migrated = migrate(&mut connection)
        .map_err(|error| format!("worker {}: set-up: {error}", options.id))?;

    let stop = Arc::new(AtomicBool::new(false));
    let reader = {
        let stop = Arc::clone(&stop);
        let path = options.path.clone();
        let busy_timeout_ms = options.busy_timeout_ms;
        std::thread::spawn(move || read_until(&path, busy_timeout_ms, &stop))
    };

    let started = Instant::now();
    let first_write_ms = now_ms();
    let mut busy_errors = 0u32;
    let mut other_errors = 0u32;
    let mut slowest = Duration::ZERO;
    for seq in 1..=options.writes {
        let write_started = Instant::now();
        // A real caller would surface a busy error. The trial counts it and
        // tries again, so that the lost-write check still means something.
        loop {
            match write_once(
                &mut connection,
                options.rule,
                options.think_us,
                options.id,
                seq,
            ) {
                Ok(()) => break,
                Err(error) if is_busy(&error) => busy_errors += 1,
                Err(error) => {
                    other_errors += 1;
                    eprintln!("worker {}: write {seq}: {error}", options.id);
                    break;
                }
            }
        }
        slowest = slowest.max(write_started.elapsed());
        spin(options.pause_us);
    }
    let last_write_ms = now_ms();
    let elapsed = started.elapsed();
    stop.store(true, Ordering::Relaxed);
    let (reads, torn, reader_busy) = reader.join().map_err(|_| "reader panicked")??;

    // One line, parsed by `run`.
    println!(
        "worker={} migrated={} writes={} busy_errors={busy_errors} other_errors={other_errors} \
         reads={reads} torn_reads={torn} reader_busy={reader_busy} elapsed_ms={} \
         slowest_write_ms={} first_write_ms={first_write_ms} last_write_ms={last_write_ms}",
        options.id,
        u8::from(migrated),
        options.writes,
        elapsed.as_millis(),
        slowest.as_millis(),
    );
    Ok(other_errors == 0)
}

fn field(line: &str, name: &str) -> u64 {
    line.split_whitespace()
        .find_map(|part| part.strip_prefix(&format!("{name}=")))
        .and_then(|value| value.parse().ok())
        .unwrap_or(0)
}

fn run(options: &Options) -> Result<bool, AnyError> {
    let folder = std::env::temp_dir().join(format!(
        "dirt-two-process-{}-{}",
        options.rule_name(),
        std::process::id()
    ));
    std::fs::create_dir_all(&folder)?;
    let path = folder.join("trial.db");

    let spawned = options.spawn_workers(&path, 500)?;
    println!(
        "== {} processes, {} writes each, rule={}, open={}, busy timeout {} ms, \
         {} us inside and {} us between transactions",
        options.processes,
        options.writes,
        options.rule_name(),
        options.open_name(),
        options.busy_timeout_ms,
        options.think_us,
        options.pause_us
    );
    let mut busy_errors = 0;
    let mut torn_reads = 0;
    let mut migrations = 0;
    let mut workers_ok = true;
    let mut latest_first = 0;
    let mut earliest_last = u64::MAX;
    for child in spawned {
        let output = child.wait_with_output()?;
        let line = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        println!("{line}");
        let complaint = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        if !complaint.is_empty() {
            println!("{complaint}");
        }
        busy_errors += field(&line, "busy_errors") + field(&line, "reader_busy");
        torn_reads += field(&line, "torn_reads");
        migrations += field(&line, "migrated");
        latest_first = latest_first.max(field(&line, "first_write_ms"));
        earliest_last = earliest_last.min(field(&line, "last_write_ms"));
        workers_ok &= output.status.success();
    }
    // How long every worker was writing at the same time. A run in which the
    // workers did not overlap proves nothing.
    let overlap_ms = earliest_last.saturating_sub(latest_first);

    // Check the file from a new connection, after the workers have exited.
    let connection = open(&path, options.busy_timeout_ms, true)?;
    let expected = i64::from(options.processes) * i64::from(options.writes);
    let counter: i64 = connection.query_row("SELECT n FROM counter", [], |row| row.get(0))?;
    let rows: i64 = connection.query_row("SELECT count(*) FROM log", [], |row| row.get(0))?;
    let distinct_totals: i64 =
        connection.query_row("SELECT count(DISTINCT total) FROM log", [], |row| {
            row.get(0)
        })?;
    let complete_writers: i64 = connection.query_row(
        "SELECT count(*) FROM (SELECT writer FROM log GROUP BY writer
           HAVING count(*) = ?1 AND min(seq) = 1 AND max(seq) = ?1)",
        [options.writes],
        |row| row.get(0),
    )?;
    let integrity: String = connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    let journal: String = connection.query_row("PRAGMA journal_mode", [], |row| row.get(0))?;
    println!(
        "expected={expected} counter={counter} log_rows={rows} distinct_totals={distinct_totals} \
         complete_writers={complete_writers} integrity={integrity} journal_mode={journal}"
    );
    println!(
        "set-up ran in {migrations} process(es); all workers were writing together for \
         {overlap_ms} ms"
    );

    let no_lost_write = counter == expected
        && rows == expected
        && distinct_totals == expected
        && complete_writers == i64::from(options.processes);
    let pass = workers_ok
        && no_lost_write
        && busy_errors == 0
        && torn_reads == 0
        && migrations == 1
        && overlap_ms > 0
        && integrity == "ok";
    println!(
        "busy errors reaching a caller: {busy_errors}; torn reads: {torn_reads}; \
         lost writes: {} -> {}",
        if no_lost_write { "none" } else { "SOME" },
        if pass { "PASS" } else { "FAIL" }
    );
    drop(connection);
    let _ = std::fs::remove_dir_all(&folder);
    Ok(pass)
}

/// Races the workers on a fresh file, `rounds` times, and counts the workers
/// that could not open it.
fn startup(options: &Options) -> Result<bool, AnyError> {
    let mut options = options.clone();
    options.writes = 0;
    let folder = std::env::temp_dir().join(format!(
        "dirt-two-process-startup-{}-{}",
        options.open_name(),
        std::process::id()
    ));
    std::fs::create_dir_all(&folder)?;

    let mut failed_workers = 0u32;
    let mut failed_rounds = 0u32;
    let mut reasons: BTreeMap<String, u32> = BTreeMap::new();
    for round in 0..options.rounds {
        let path = folder.join(format!("round-{round}.db"));
        let mut failed = 0;
        for child in options.spawn_workers(&path, 150)? {
            let output = child.wait_with_output()?;
            if !output.status.success() {
                failed += 1;
                let complaint = String::from_utf8_lossy(&output.stderr);
                // "error: worker 1: open: database is locked" -> the last two parts.
                let reason: Vec<&str> = complaint.trim().split(": ").skip(2).collect();
                *reasons.entry(reason.join(": ")).or_insert(0) += 1;
            }
        }
        failed_workers += failed;
        failed_rounds += u32::from(failed > 0);
    }
    println!(
        "== start-up race: {} rounds, {} processes, open={}",
        options.rounds,
        options.processes,
        options.open_name()
    );
    println!("rounds with a failed start: {failed_rounds}; failed workers: {failed_workers}");
    for (reason, count) in reasons {
        println!("x{count}: {reason}");
    }
    let _ = std::fs::remove_dir_all(&folder);
    Ok(failed_workers == 0)
}
