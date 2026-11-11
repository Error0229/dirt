//! Trial 2, Android half: the SQLite work the app does on start.
//!
//! The same function runs inside the Android app, in a plain binary pushed
//! to the emulator, and on the host.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Instant;

use rusqlite::Connection;

/// One line of the report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub label: String,
    pub value: String,
    /// `None` for information, `Some` for a check that passed or failed.
    pub ok: Option<bool>,
}

#[derive(Debug, Clone, Default)]
pub struct Report {
    pub lines: Vec<Line>,
}

impl Report {
    fn info(&mut self, label: &str, value: impl Into<String>) {
        self.lines.push(Line {
            label: label.to_owned(),
            value: value.into(),
            ok: None,
        });
    }

    fn check(&mut self, label: &str, ok: bool, value: impl Into<String>) {
        self.lines.push(Line {
            label: label.to_owned(),
            value: value.into(),
            ok: Some(ok),
        });
    }

    /// True when every check passed and there was at least one.
    #[must_use]
    pub fn passed(&self) -> bool {
        let mut checks = self.lines.iter().filter_map(|line| line.ok).peekable();
        checks.peek().is_some() && checks.all(|ok| ok)
    }

    #[must_use]
    pub fn to_text(&self) -> String {
        let mut text = String::new();
        for line in &self.lines {
            let mark = match line.ok {
                Some(true) => "PASS",
                Some(false) => "FAIL",
                None => "    ",
            };
            let _ = writeln!(text, "{mark} {}: {}", line.label, line.value);
        }
        let _ = writeln!(
            text,
            "RESULT: {}",
            if self.passed() { "PASS" } else { "FAIL" }
        );
        text
    }
}

/// The app's private files folder.
///
/// On Android it is derived from the process name, which is the package
/// name, the way `dirt-mobile` already does. Elsewhere it is a temporary
/// folder.
#[must_use]
pub fn data_folder() -> PathBuf {
    if cfg!(target_os = "android") {
        if let Some(package) = std::fs::read("/proc/self/cmdline")
            .ok()
            .and_then(|bytes| {
                let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
                String::from_utf8(bytes[..end].to_vec()).ok()
            })
            .filter(|name| !name.is_empty() && !name.contains('/'))
        {
            return PathBuf::from("/data/data").join(package).join("files");
        }
    }
    std::env::temp_dir().join("dirt-rusqlite-android-trial")
}

/// Runs every step and never panics: a failed step becomes a failed line.
#[must_use]
pub fn run_trial(folder: &Path) -> Report {
    let mut report = Report::default();
    report.info(
        "target",
        format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
    );
    report.info("data folder", folder.display().to_string());
    if let Err(error) = steps(folder, &mut report) {
        report.check("stopped by an error", false, error.to_string());
    }
    report
}

fn steps(folder: &Path, report: &mut Report) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(folder)?;
    let path = folder.join("trial.db");
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }
    let mut db = Connection::open(&path)?;

    let version: String = db.query_row("SELECT sqlite_version()", [], |row| row.get(0))?;
    report.info("SQLite version", version);

    // Write-ahead-log mode. The pragma answers with the mode now in force.
    let mode: String = db.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
    report.check("write-ahead-log mode", mode == "wal", mode);

    // Full-text search.
    let started = Instant::now();
    db.execute_batch(
        "CREATE VIRTUAL TABLE notes_fts USING fts5(body);
         INSERT INTO notes_fts (body) VALUES
           ('buy oat milk on the way home'),
           ('a fleeting thought about sync protocols'),
           ('想到一個關於同步的點子');",
    )?;
    let found: Vec<String> = db
        .prepare("SELECT body FROM notes_fts WHERE notes_fts MATCH ?1")?
        .query_map(["thought"], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    report.check(
        "full-text search finds the inserted row",
        found == ["a fleeting thought about sync protocols"],
        format!("{found:?} in {} ms", started.elapsed().as_millis()),
    );

    // Insert 10,000 rows and sort them on a column with no index.
    let started = Instant::now();
    db.execute_batch("CREATE TABLE rows (id INTEGER PRIMARY KEY, word TEXT NOT NULL)")?;
    let tx = db.transaction()?;
    {
        let mut insert = tx.prepare("INSERT INTO rows (id, word) VALUES (?1, ?2)")?;
        for id in 0..10_000u32 {
            // A fixed scramble, so the insertion order is not the sort order.
            let scrambled = id.wrapping_mul(2_654_435_761) % 1_000_003;
            insert.execute((id, format!("word-{scrambled:07}-{}", "x".repeat(64))))?;
        }
    }
    tx.commit()?;
    let inserted_ms = started.elapsed().as_millis();

    let started = Instant::now();
    let sorted: Vec<String> = db
        .prepare("SELECT word FROM rows ORDER BY word")?
        .query_map([], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    let in_order = sorted.windows(2).all(|pair| pair[0] <= pair[1]);
    report.check(
        "sort of 10,000 rows completes in order",
        sorted.len() == 10_000 && in_order,
        format!(
            "{} rows; insert {inserted_ms} ms, sort {} ms",
            sorted.len(),
            started.elapsed().as_millis()
        ),
    );

    // A temporary table and a larger sort both need temporary storage.
    // Android gives an app no shared temporary folder, so this is the step
    // that fails if SQLite tries to use one.
    let started = Instant::now();
    db.execute_batch(
        "CREATE TEMP TABLE scratch AS
           SELECT a.word || b.id AS word FROM rows a, (SELECT id FROM rows LIMIT 20) b;",
    )?;
    let count: i64 = db.query_row("SELECT count(*) FROM scratch", [], |row| row.get(0))?;
    // The offset makes SQLite sort every row to find the last one.
    let last: String = db.query_row(
        "SELECT word FROM scratch ORDER BY word DESC LIMIT 1 OFFSET 199999",
        [],
        |row| row.get(0),
    )?;
    report.check(
        "temporary table of 200,000 rows, sorted",
        count == 200_000 && last.starts_with("word-"),
        format!("{count} rows in {} ms", started.elapsed().as_millis()),
    );

    let options: Vec<String> = db
        .prepare("PRAGMA compile_options")?
        .query_map([], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    let has = |name: &str| options.iter().any(|option| option == name);
    report.check(
        "compiled with ENABLE_FTS5",
        has("ENABLE_FTS5"),
        has("ENABLE_FTS5").to_string(),
    );
    let temp_store = options
        .iter()
        .find(|option| option.starts_with("TEMP_STORE"))
        .cloned()
        .unwrap_or_else(|| "not listed".to_owned());
    // rusqlite's bundled build sets TEMP_STORE=3 (always memory) for Android
    // targets only. Other targets keep SQLite's default and have a real
    // temporary folder, so the value is a check only on Android.
    if cfg!(target_os = "android") {
        report.check(
            "compiled with TEMP_STORE=3",
            temp_store == "TEMP_STORE=3",
            temp_store,
        );
    } else {
        report.info("temporary storage option", temp_store);
    }
    report.info("compile options", options.join(" "));

    // Close and reopen, as the next app start would.
    drop(db);
    let db = Connection::open(&path)?;
    let rows: i64 = db.query_row("SELECT count(*) FROM rows", [], |row| row.get(0))?;
    let mode: String = db.query_row("PRAGMA journal_mode", [], |row| row.get(0))?;
    let integrity: String = db.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    report.check(
        "data is there after reopening",
        rows == 10_000 && mode == "wal" && integrity == "ok",
        format!("{rows} rows, journal mode {mode}, integrity {integrity}"),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_trial_passes_on_the_host() {
        let folder =
            std::env::temp_dir().join(format!("dirt-rusqlite-trial-test-{}", std::process::id()));
        let report = run_trial(&folder);
        let _ = std::fs::remove_dir_all(&folder);
        assert!(report.passed(), "{}", report.to_text());
    }

    #[test]
    fn a_report_with_no_checks_does_not_pass() {
        assert!(!Report::default().passed());
    }
}
