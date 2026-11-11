# Trial 2: rusqlite on Android, and two processes on one file

- **Date:** 2026-11-11
- **Verdict:** pass, with one gap: no physical arm64 device was tested
- **Code:** `trials/rusqlite-android/` and `trials/sqlite-two-process/` on the
  `trials/step-1` branch
- **Plan:** `docs/plans/2026-11-10-refactor-system-redesign-implementation-plan.md`, Step 1
- **Tracking issue:** #237

## Question

The redesign replaces `libsql` with `rusqlite` for the local database
(decision 12). Two things had to be shown first:

1. `rusqlite` with its bundled SQLite builds, links and runs on Android.
2. The desktop app and the CLI can safely share one database file.

## Verdict

Both hold. `rusqlite` replaces `libsql` for the local file.

| Criterion | Measured |
|---|---|
| App builds, links and runs with `dx` on x86_64 | Yes, on the Android 16 emulator |
| App builds, links and runs with `dx` on arm64 | Builds and links. Runs on the emulator through its ARM translation. Not run on a physical arm64 device |
| Search returns the inserted row | Yes |
| Sort of 10,000 rows completes | Yes, in 129 ms on the emulator |
| `compile_options` lists `ENABLE_FTS5` and `TEMP_STORE=3` | Both, on Android |
| Two processes: no busy error reaches the caller | 0 in 2 × 1,000 writes, given the two rules below |
| Two processes: no lost write | None |
| Plain `cargo build` links for Android (to record) | Yes, for both architectures |

The pass depends on two rules that step 3 must build in. Without either one,
the two-process run fails:

1. **Every write transaction starts with `BEGIN IMMEDIATE`.**
2. **Opening the file retries the switch to write-ahead-log mode.** The busy
   timeout does not cover that switch.

## Method

- **Versions:** rusqlite 0.42.0 (the plan named 0.41; 0.42 is current) with
  the `bundled` feature, which is SQLite 3.54.1. Dioxus 0.7.10, `dx` 0.7.9,
  Rust 1.99.1, NDK 26.3.11579264, JDK 21.
- **Android device:** the `dirt-trial` emulator, Android 16, x86_64 image with
  Google APIs. The image also lists `arm64-v8a` and runs arm64 code through
  `libndk_translation`.
- **The app:** one screen. On start it opens a database in its private files
  folder, switches to write-ahead-log mode, creates a full-text table, inserts
  and searches, inserts and sorts 10,000 rows, builds and sorts a 200,000-row
  temporary table, lists the compile options, then closes and reopens the
  file. It shows each step as PASS or FAIL and writes the same report to a
  file.
- **A plain binary:** the same steps with no app around them, built with
  `cargo build` alone and run on the emulator through `adb shell`. This is the
  check for whether CI can link without `dx`.
- **Two processes:** one program started twice at the same moment on a fresh
  file. Each process opens the file, creates the tables if the other has not,
  and performs 1,000 write transactions while a second thread in the same
  process reads. Each write reads a counter, adds one and logs the new total,
  so a lost update would leave two log rows with the same total. Each read
  checks that the counter equals the number of log rows.
- **JDK:** `dx` ran with `JAVA_HOME` set to JDK 21 for that command only. The
  global `JAVA_HOME` was not changed.

## Android results

All four runs passed every step. Times are from one run each, in milliseconds.

| Build | Runs as | Search | Insert 10,000 | Sort 10,000 | Temp table of 200,000, sorted |
|---|---|---|---|---|---|
| `dx` app, release | x86_64 | 66 | 173 | 129 | 2,713 |
| `dx` app, release | arm64, translated | 1,237 | 387 | 177 | 4,494 |
| Plain `cargo build`, debug | x86_64 | 20 | 95 | 56 | 2,675 |
| Plain `cargo build`, debug | arm64, translated | 1,026 | 678 | 496 | 7,623 |

The arm64 times measure the emulator's translation, not arm64 hardware.

Other numbers:

- **Build time with `dx`:** 12 minutes for the first x86_64 build, of which 6
  were Rust and 6 were the first Gradle run. The arm64 build after it took 5
  minutes.
- **Package size:** 16 MB for a debug-signed package with one architecture.
  The native library is 26.5 MB before compression. Most of it is Dioxus, not
  SQLite.

Screenshots and reports are in `trials/rusqlite-android/results/`.

## Two-process results

Output of the runs recorded in `trials/sqlite-two-process/results/runs.txt`:

| Run | Busy errors reaching a caller | Lost writes | Torn reads | Slowest single write |
|---|---|---|---|---|
| 2 processes × 1,000 writes, `BEGIN IMMEDIATE` | 0 | 0 | 0 | 123 ms |
| 4 processes × 1,000 writes, `BEGIN IMMEDIATE` | 0 | 0 | 0 | 1,306 ms |
| 2 processes × 10,000 writes, no pauses, `BEGIN IMMEDIATE` | 0 | 0 | 0 | 642 ms |
| 2 processes × 1,000 writes, plain `BEGIN` (control) | 850 | 0 | 0 | 7 ms |

| Start-up race, 100 fresh files, 2 processes | Rounds in which one process failed to open |
|---|---|
| Open without the retry (control) | 70 |
| Open with the retry | 0 |

In the first run both processes were writing at the same time for 1,043 ms.
The workers read about 140,000 times during it.

## Findings

### Android

- **`rusqlite` works on Android with no special handling.** No build script,
  no extra linker argument and no feature beyond `bundled` was needed.
- **Plain cargo links.** The open rusqlite issue about a link failure on
  `x86_64-linux-android` did not reproduce with Rust 1.99.1 and NDK 26.3. CI
  does not have to switch to `dx` to link. It can replace `cargo check` with
  `cargo build` for the mobile crate and so catch link failures.
- **`TEMP_STORE=3` is set for Android targets only.** The bundled build adds
  it when the target is Android, where an app has no shared temporary folder.
  On Windows the value is `TEMP_STORE=1`. The 200,000-row temporary table is
  the step that would fail on Android without it, and it passed.
- **Full-text search needs no extra feature.** `ENABLE_FTS5` is on in the
  bundled build on every target.
- **The data folder comes from the process name.** The app reads
  `/proc/self/cmdline` to get its package name and uses
  `/data/data/<package>/files`, as `dirt-mobile` already does. It worked.
  Asking Android for the folder through JNI would be more correct.
- **Keep `dx` and `dioxus` on the same version.** Cargo resolved Dioxus
  0.7.10 while `dx` is 0.7.9. `dx` printed "dx and dioxus versions are
  incompatible" and the build still worked. Step 7 should pin one to the
  other.
- **`dx` keeps libraries from earlier builds.** The package built for arm64
  also contained the x86_64 library from the build before it, and the emulator
  loaded that one. Installing with `adb install --abi arm64-v8a` forced the
  arm64 library. A release build must start from a clean output folder or
  check which architectures are in the package.

### Two processes

- **`BEGIN IMMEDIATE` is required, not optional.** With plain `BEGIN`, a
  transaction that reads and then writes failed 850 times in 2,000 writes,
  despite a five-second busy timeout. SQLite cannot let it wait: another
  process wrote after its read, so its view of the data is out of date. No
  write was lost, but the error reaches the caller every time.
- **The busy timeout does not cover the switch to write-ahead-log mode.**
  When two processes open a fresh file together, one of them fails at once
  with "database is locked" in 70 of 100 attempts. This is the first start of
  the desktop app and the CLI at the same moment. Retrying the switch for up
  to the busy timeout removed every failure, because the mode is stored in the
  file and the retry finds it already set.
- **Set-up must check the version inside the write transaction.** Both
  processes try to create the tables. With the check inside a
  `BEGIN IMMEDIATE` transaction, the set-up ran in exactly one process in
  every run.
- **Readers never wait and never see half a write.** No read failed and no
  read saw a counter that disagreed with the log.
- **A waiting writer can wait much longer than a write takes.** SQLite's busy
  handler sleeps between attempts, for up to 100 ms at a time, and does not
  queue fairly. One write waited 1.3 s when four processes wrote without
  pause. Two interactive programs will not write that densely, but a bulk
  import in one of them could delay the other. Five seconds of busy timeout
  left a wide margin in every run.

## Effect on the design

- **Decision 12 stands:** `rusqlite` replaces `libsql` for the local file.
- **Section 8.4** should state the two rules as requirements of the storage
  module: `BEGIN IMMEDIATE` for every write transaction, and an open routine
  that sets the busy timeout first and retries the journal-mode switch.
- **Section 9.3** can keep cargo for the Android CI job and should change it
  from `cargo check` to `cargo build`.
- **Step 3** gets a two-process test of its own, modelled on
  `trials/sqlite-two-process/tests/two_process.rs`, including the two controls
  that show the rules are what make it pass.

## Not covered

- **A physical arm64 device.** The arm64 build was run only through the
  emulator's translation. A real phone should run the app once before step 7
  is called done.
- **The desktop app and CLI themselves.** The trial used a stand-in program.
  Windows was the only host tested; macOS and Linux use different file
  locking and should run the same test in CI.
- **A process killed in the middle of a write.** SQLite's recovery is well
  established, and the trial did not test it.

## How to rerun

```bash
# Two processes
cd trials/sqlite-two-process
cargo test --release
cargo run --release -- run
cargo run --release -- startup --open plain

# Android (emulator running; ANDROID_HOME and NDK_HOME set)
cd trials/rusqlite-android
JAVA_HOME="$DIRT_ANDROID_JAVA_HOME" dx build --android \
  --target x86_64-linux-android --release --bin rusqlite-android-trial
adb install -r target/dx/rusqlite-android-trial/release/android/app/app/build/outputs/apk/debug/app-debug.apk
adb shell monkey -p dev.dirt.trial.rusqlite -c android.intent.category.LAUNCHER 1
adb shell run-as dev.dirt.trial.rusqlite cat files/report.txt
```
