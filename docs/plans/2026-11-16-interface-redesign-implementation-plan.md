---
title: "feat: Build the interface redesign of 2026-11-16"
type: feat
status: active
date: 2026-11-16
design: docs/ui/design.md
---

# feat: Build the interface redesign of 2026-11-16

## Overview

This plan builds the design in [`docs/ui/design.md`](../ui/design.md): a new
interface for the desktop app and the Android app, and the features it adds
(archive, undo, voice, capture from outside the app, the file preview, the
pile). [`docs/ui/mockup.html`](../ui/mockup.html) shows it working.

The owner approved the design on 2026-11-16. A review the same day found
what had to be settled before building; the answers are under "Decided"
below and are written into the design. What the design asks of the rest of
the system is in the system design, section 16.

Two trials come first, because either result can change what is built.
Step 1 and step 2 do not wait for them.

## Rules for every step

- The rules in `CLAUDE.md` hold. The two that matter most here: no SQL,
  HTTP or cryptography in a client crate, and `dirt-ui` counts as one; the
  words about sync come from `dirt_core::sync::overview`.
- Where the mockup and the design document disagree, the mockup holds for
  how long a movement takes and on which curve, and the document for
  everything else. A step that finds a disagreement corrects the document
  in its own pull request.
- The mockup is a reference for look and timing, not code to port. About
  450 of its 3,000 lines of script are superseded by later ones, and it
  keeps its state in the page; the apps keep theirs in Rust.
- Each app works after every pull request. The old interface is replaced
  screen by screen, and the pull request that replaces a screen deletes the
  old one.
- A pull request that changes what a client does changes its rows in
  `docs/feature-parity.md` and the items in `docs/MANUAL_CHECKLIST.md`.
- Before it merges, a step is run on Windows and on the Android emulator,
  and its pull request says what was run and what was not. What needs a
  real phone, macOS or Linux goes into the manual checklist.
- Everything is tried against the trial server, never against production.
- Motion that follows a finger or a pointer runs in the page's script from
  first touch to release and tells Rust only the outcome. A call into Rust
  for every movement is too slow on a phone.
- The script that animates the list watches the page for changes and plays
  the difference from the positions it last saw. It is not called once per
  change from Rust, because Rust cannot measure the page before it changes
  it.

## Decided (2026-11-16)

| Question | Decision |
|---|---|
| A system notification on Android when a device asks to join | Not built. The banner inside the app is enough: comparing the code needs both devices in hand |
| Must a thought captured from the widget reach other devices without opening the app | Yes, if trial 2 finds a way. Otherwise it syncs the next time the app is opened |
| Chinese: search, tags, voice | Not in this work. English first. Recorded as later work in the system design, section 16.3 |
| Who settles what the design leaves open or gets wrong | Whoever builds it, writing the answer into the design document |

## Still the owner's

Each has a default that holds if the step is reached without an answer.

| Decision | Needed by | Default |
|---|---|---|
| PDF in the preview: bundle a renderer (a few MB) or not | Step 6 | Not bundled: a PDF opens in another app on Linux and Android |
| The speech model: fetched on first use, or shipped in the app | Step 9, after trial 1 has the sizes | Fetched on first use |
| Snap Layouts on Windows 11: accept losing the flyout, or win it back | Step 3 | Accept losing it |

## Trials

A trial is code on a branch that answers questions and is thrown away. Its
result is a section added to this document.

### Trial 1: the voice engine

The design needs words on screen as they are spoken, on a phone, with
nothing leaving the device. Whisper works on a finished recording, not on
speech as it arrives.

Measure, for whisper.cpp with a sliding window, for a streaming recogniser
(sherpa-onnx), and on Android for the system's on-device recogniser:

- The time from a word being spoken to its being shown, and how often a
  shown word later changes.
- How fast it runs against real time, and the memory it takes, on this
  laptop and on a phone.
- Mistakes on a fixed set of about twenty short spoken thoughts in English.
- Whether a pause of 1.6 seconds after speech can be told from a pause in
  the middle of a thought.
- Whether it builds for Windows, macOS, Linux and both Android targets, and
  what it adds to a build.
- The model each needs, and its size.
- On Android, whether the system's recogniser can be given sound the app
  records itself, so that the recording can be kept.
- A format for the recording that every platform can write and every web
  view can play, at about ten minutes inside the 10 MiB limit on a file.

The phone's half runs a test program on the owner's phone through `adb` and
needs the owner's word first. The emulator's timings say nothing about a
phone.

**It decides:** the engine and where it lives (a `voice` feature of
`dirt-core`, or a `dirt-voice` crate), the model put to the owner, and the
recording's format.

### Trial 2: capture on Android without the app

The capture sheet is a native screen that opens without the web view
(design 8.5). The owner wants what it captures to reach other devices
without the app being opened.

Find out:

- How long from a tap on the widget to a field that can be typed in, for a
  native screen with no web view.
- Whether the Rust library can be loaded and used where the app's own
  screen never started: open the store, read the device's secrets from
  Android's encrypted preferences, make a note, sync once.
- What starts that work and lets it finish after the sheet has closed.
- That the sheet writing while the app is open loses nothing. The store is
  already safe for two processes on a desktop.
- How more Kotlin files and a folder of resources get into the Gradle
  project `dx` generates, in a way that survives a clean build.
- On the emulator, whether the sheet and a Quick Settings tile open over
  the lock screen, and that nothing of a note can be seen from there.

**It decides:** whether design 8.5's background work is built or its
fallback, and how Android's native code is laid out for steps 8 and 9.

## Steps

### Step 1: what the interface needs from the core

No screen changes. Each item is a pull request with tests in `dirt-core`,
and in `dirt-sim` where two devices are involved.

| Item | What |
|---|---|
| Archive | An `archived` value in the note's document, derived into its row. Not an edit: it changes neither the edit time nor a deletion (design 6.19). The list leaves archived notes out; search gives them after the rest; an export marks them. `dirt archive`, `dirt unarchive`, `dirt list --archived` |
| Undo of a delete | The store's restore, offered to the clients. `dirt restore <ID>` |
| A note emptied again | With no files: taken off the device altogether if it never reached the server, deleted like any other if it did |
| Tags | For the words and tags chosen, each other tag with how many notes it would leave; "any of them" as well as "all of them" |
| The pile and the layers | Every live note's ID and edit time in one cheap call, since the apps hold only the notes their list shows |
| Sync, in short | A short form of each state in `dirt_core::sync::overview` beside the full sentences |
| A file in memory | A file's decrypted bytes from `dirt-core::files` without writing them to disk |

**Settle first:** how a change that is not an edit is noticed as unsent and
pushed; what a note's document from before archive reads as.

**Settled**, with the pull request that built archive, undo and the note
emptied again:

- *A change that is not an edit* is noticed by the row, not by the
  document. Each note's row has a revision that rises with every write
  made on the device, and the note is unsent while that differs from the
  revision last pushed (`dirt-core::store::sync_state`). A delete is not
  an edit either, and has always been sent this way. Archiving saves
  through the same path, so an archive made offline is unsent and the next
  cycle pushes it, and one that arrives from another device is merged and
  derived into the row without making the note unsent. Nothing in the
  protocol or on the server changes.
- *A document from before archive* reads as not archived: the mark is
  `archived` in a map `marks` of the document, and such a document has no
  such map. A build from before archive keeps the mark in a document it
  merges, edits and sends on, because yrs stores and writes out what it
  merged under a name the program never asked for.
  `crates/dirt-core/tests/notes_archive.rs` plays such a build. For the
  same reason a store from before archive reads its documents once when it
  is upgraded, to find the notes that are already archived.

Two things followed from these and are in the system design (6.2, 6.5 and
16.2). The row also keeps the revision of the note's last edit, so that a
copy that is unsent only for having been archived is not kept as a new note
when the server has purged the note. And "never reached the server" is a
mark of its own, set before a push leaves and not when its answer arrives,
because the answer can be lost.

Left open by the design and settled here, in `docs/ui/design.md` 6.1 and
6.19: a note of spaces and line breaks alone is empty; narrowing to a tag
is finding, so it keeps archived notes after the rest, and the count beside
a tag counts them.

**Done when:** two simulated devices that archive, bring back, edit and
delete the same note in every order end the same; a store from the
previous schema opens and keeps its notes; the CLI's new commands are in
`docs/feature-parity.md`.

### Step 2: `dirt-ui`, the foundation

A new crate that both apps use. It holds no screen of either app yet.

- The tokens of design section 4 as one style sheet: light, dark, the six
  accents. A test computes the contrast of every pair section 4.1 lists
  and fails under the ratios it promises.
- Inter and JetBrains Mono, bundled. The icons of 4.4.
- The motion tokens of section 5, taken from the mockup, with the fallback
  for a web view without `linear()`, and one script for the springs, the
  list's animation and reduced motion.
- The small components of section 6: icon button with its tooltip, the
  four kinds of button, toast with **Undo**, banner, segmented control,
  slider, switch, accent dots, panel and sheet, confirmation in place,
  the drawn scrollbar.
- Theme, accent and text size, kept on the device, on both apps.
- The web view's own keys turned off in release builds (design 7.4).
- A gallery, built only for development, that shows every component on the
  desktop and on Android.

**Done when:** the gallery runs on Windows and on the emulator; a list in
it reorders at the display's rate; a card follows a finger with no call
into Rust until release; turning on reduced motion takes effect at once;
the contrast test passes.

**Left by this step for later ones**, each in the step that has what it
needs:

| What | Step |
|---|---|
| The text size's default becomes 15 px (design 4.2). The old desktop editor reads the same setting, at 14, until it is replaced | 3 |
| <kbd>Ctrl Z</kbd> after the toast has gone. The script presses **Undo** on a toast that is showing; the key on its own is one of 7.4's | 3 |
| The app's own context menu (Copy, Paste, Select all). Release builds show no menu until then, as they did before | 3 |
| Back closes a sheet. The script closes it on <kbd>Esc</kbd> and on a drag; Android's Back is the app's to hand on | 4 |
| Pull to sync, and the note growing out of its card: the two gestures of 5.5 the script does not have yet | 4 |
| Auto on Android. The WebView takes light or dark from the activity's theme, which is a light one, so Auto is light there. The activity needs a theme that follows the system and `uiMode` among the changes it handles itself: without that, Android restarts the activity when the system's theme changes, and the app ends (seen on the emulator) | 4 |
| Reduced motion on Android while the app is open. The WebView reads "Remove animations" only when it starts. Read `Settings.Global.ANIMATOR_DURATION_SCALE` each time the person returns, and give the answer to `UiRoot`'s `reduced_motion` | 4 |
| Wallpaper, the seventh accent on Android 12 and later (design 4.1.1), with its contrast check. A stored accent a build does not know reads as Clay | 5 |
| A toast that stays while a screen reader is on (design 12). `Toast::hold` does the staying; knowing that TalkBack is on needs native code | 10 |
| Trying the fallbacks where they are used: a web view without `linear()` or without View Transitions (WebKitGTK), and the web view's keys in a release build on each system | The manual checklist, G1 to G9 |

### Step 3: the desktop's main screen

The page, the list in its layers, Find and tags, archive, delete and undo,
the sync status, focus while writing (Slide, then Rail and Zen), the window
without a title bar, and every key in design 7.4. The old settings stay
reachable until step 5. The app's root becomes `dirt_ui::UiRoot`, and the
text size's default becomes 15 px.

**Fallback written in advance:** a platform where the window without a
title bar cannot be moved or resized keeps the system's title bar.

**Done when:** every desktop row of design section 2 that this step covers
takes the number of actions the table gives, counted by hand.

### Step 4: Android's main screen

The dock, cards, Find, the note that grows out of its card, swipe to
archive, pull to sync, saving as you type, load more on scroll. Runs
alongside step 3; the two share components through `dirt-ui`, and a
component both need is added there by whichever step reaches it first.

**Done when:** the same count as step 3, for the Android column.

### Step 5: Settings and the flows

One page in six sections on both apps: account, devices with the code
comparison and the recovery phrase, sync with the notes that cannot sync,
appearance, voice (empty until step 9), export. The banner for a waiting
device. The old settings screens are deleted.

**Done when:** every row of `docs/feature-parity.md` under "Account and
devices", "Sync" and "Export" has been done once on each app against the
trial server.

### Step 6: files

The file tile, the preview from memory, dropping files to make a note,
pasting a picture. `docs/SECURITY.md` says what the preview keeps in
memory and what "Open in another app" writes.

### Step 7: grains and the pile

The settling of a new thought, the grain of each note, the pile under the
list, the Rail's dots. The pile is worked out in Rust, once per change,
from step 1's call.

### Step 8: capture from outside the app

- Desktop: quick capture as its own small window, kept ready so that it
  shows at once. Today it is the main window made small.
- Android: the capture sheet, the widget, the app shortcut, the Quick
  Settings tile, the note-taking role, the toast after a share, and the
  background work or its fallback as trial 2 found.
- `docs/SECURITY.md` gains capture over the lock screen.

**Fallback written in advance:** where the tray icon's place on screen
cannot be known, the grain falls and fades where the window was instead of
flying to the tray.

### Step 9: voice

The engine trial 1 chose, listening on the page and in quick capture, the
held hotkey, Android's dock and capture sheet, the Talk entry,
Settings → Voice. `docs/SECURITY.md` gains voice. The model is put in
Dirt's own storage, which is a change to production's storage and waits
for the owner's word.

### Step 10: finishing

The pile widget. A pass over design section 12, item by item, on both
apps. `docs/MANUAL_CHECKLIST.md` rewritten for the new interface. What is
left of the old interface and its dependencies removed. `README.md` and
`CLAUDE.md` brought up to date.

## What runs alongside what

```
trial 1 ─────────────────────────────────────────────┐
trial 2 ──────────────────────────────┐              │
step 1 ──┐                            │              │
step 2 ──┴─ step 3 ─┐                 │              │
            step 4 ─┴─ step 5 ─ step 6 ┬─ step 8 ─ step 9 ─ step 10
                                step 7 ┘
```

At most three run at once on the one machine that builds them.
