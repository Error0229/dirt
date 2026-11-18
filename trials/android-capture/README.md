# Trial 2: capture on Android without the app

Kept for reference and not merged. The result is the section "Trial 2
result" of `docs/plans/2026-11-16-interface-redesign-implementation-plan.md`.

## What the trial built

- `crates/dirt-mobile/android/native/`: a Gradle library module that `dx`
  copies into the project it generates (as `:plugins:dirtnative`) because
  `src/main.rs` declares it with `#[manganis::ffi("/android/native")]`. It
  holds the capture sheet (`CaptureActivity`, and `CaptureActivityOwn` in a
  `:capture` process to compare), the job that makes the thought a note and
  syncs once (`CaptureJobService`), the Quick Settings tile, a bar widget
  with a layout of its own, an app shortcut, a receiver for
  `USER_PRESENT`, and their resources.
- `crates/dirt-mobile/src/capture.rs`: `take_in`, which turns every waiting
  thought into a note under the ID it was given at capture, and the JNI
  function `Java_dev_dirt_capture_Core_takeIn`, which the job calls in a
  process where `MainActivity` may never have started.
- `Store::create_note_with_id` in `dirt-core`, and `EncryptedPrefs::open_with`
  in `dirt-mobile`, which opens the secrets with a `Context` handed over by
  Kotlin instead of `ndk_context`.
- The app takes waiting thoughts in when it starts and when the person
  returns to it, and `MainActivity.onResume` tells the page to look again
  (the sheet is translucent, so the page is never hidden under it).

## Corners cut

- Two copies of the sheet and of the job exist only to compare processes.
- The sheet's look is a sketch: no motion, no 🎙, a plain button for ✓.
- The extras `thread`, `pause_at`, `pause_ms`, `expedited` and
  `sync_while_locked` exist for the trial's scripts.
- The JNI function builds a fresh tokio runtime and opens a full `Session`
  each call, and returns a JSON report meant for the log.
- No tests for the Kotlin; one Rust test for `take_in` (`capture.rs`).
- `MainActivity.kt`'s old widget now opens the sheet; the old widget was
  not removed.

## The scripts

All run in Git Bash on the Windows machine of the trial; `env.sh` names
the paths, ports and the emulator. Paths in it are that machine's.

| Script | What it does |
|---|---|
| `build-android.sh` | `dx build --platform android` with JDK 21 and `-j 4`, then Gradle |
| `kotlin-only.sh` | Copies the module, manifest and `MainActivity.kt` into the generated project and packages with Gradle alone |
| `install.sh` | Pushes the APK and installs it with `pm` |
| `cli.sh` | The CLI as the second device, with its own data folder and server |
| `signin-cli.sh`, `signin-phone.sh` | Sign both in to `cargo run -p dirt-sim --example serve`, and let the phone in from the CLI by comparing codes |
| `capture.sh` | Opens the sheet with `am start`, types, presses ✓ |
| `timing.py` | Times `am start -W` and the log lines of the sheet and of the app, cold or warm |
| `ui.py` | Lists, taps and photographs the emulator's screen through uiautomator |

The race of 61 thoughts was run by hand: 60 files written into the
waiting folder with `run-as`, then the app brought to the front and the
`:capture` job asked for at the same moment.

## Results kept here

`results/` holds the timing runs of the second session (2026-11-17,
16:00 on the emulator's clock): `timing-sheet-cold.txt`,
`timing-sheet-warm.txt`, `timing-app-cold.txt`, `timing-app-warm.txt`, and
the two `-speed` files taken after `cmd package compile -m speed`.
`timing-sheetown-cold.txt` was taken while the machine was at full load
and is not used. The first session's figures (10:47) are in the plan.
