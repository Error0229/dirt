---
title: "Step 7: clients reconnected"
type: plan
date: 2026-11-12
status: done, except for the checklist items that need a person
parent: docs/plans/2026-11-10-refactor-system-redesign-implementation-plan.md
tracking: "#243"
---

# Step 7: clients reconnected

## Goal

Finish the three interfaces against the new core, so that each one shows the
same states, the same sync status and the same account actions, and so that
nothing from the old system is left in a client. Design sections 8 and 9.4
are the specification.

## State before this step

Steps 5 and 6 reconnected more than the parent plan expected of them,
because sync and approval could not be checked on real devices without an
interface. Already on `main`:

- The four session states, sign-in, sign-out and the approval prompt in the
  desktop app, the Android app and the CLI.
- Sync on start, on return to the app, after a change and on the sync
  button, in the desktop and Android apps. The CLI syncs after each command
  that changes a note, and on `dirt sync`.
- The Android return from the browser: a `singleTask` activity, an intent
  filter for `dev.dirt.app://callback`, and the attempt saved before the
  browser opens.
- `dirt auth` and `dirt devices`.

What is missing:

| Deliverable in the parent plan | What is there | What is missing |
|---|---|---|
| Sync status view in every client | Last sync time, notes waiting and the last failure. The desktop app lists reasons with no note named; the Android app gives a count; the CLI prints note IDs | Each note that could not be synced, named, with its reason and what releases it |
| CLI verbose sync | `dirt sync` prints counts | `dirt sync --verbose` |
| Deletion of `config_profiles.rs` | Still there. `dirt config init` and `--profile` are the only way to give the CLI a server address, short of the environment | The deletion, and what gives the CLI its server address in its place |
| The old `auth_cmd.rs` | Already gone | Nothing |
| JNI call in place of the file hand-off | `MainActivity` writes the redirect address to a file that the Rust side looks for | The JNI call |
| A custom tab | A full browser tab | The decision |
| One sign-in on a physical Android device | Emulator only | See "Needs a person" |
| A fresh feature-parity document | An archived one for the old system | The document |
| The manual checklist | None for the new system | The checklist, and a run of it |
| #217 | The signals it names are gone; the count now comes from the store | A check on real clients, then closing it |

Carried forward from step 6: the recovery phrase, reset and deletion screens
have not been pressed on real devices, nor has the desktop app's own "Sign
in" button.

## Decisions

The parent plan's "Settle first" items:

| Question | Decision |
|---|---|
| Android process death during sign-in | Settled in step 5: the attempt is saved before the browser opens, and a device that registers again with the same key resumes its registration. This step's checklist stops the app while the browser is in front and finishes the sign-in |
| The Clerk production instance and its domain | Not settled here. A production instance needs a domain the project owns, which is a purchase, and the standing rule is free plans only. The clients go on using the development instance. The decision stays with step 8, which already holds the other production Clerk decisions |

Decisions this plan adds:

| Question | Decision |
|---|---|
| Where the CLI gets its server address once profiles are gone | The same place as the desktop app, because the two are one device and must name one server: `DIRT_API_BASE_URL` in the environment when the program runs, or else the address built in from `.env.client`. The rule lives in `dirt-core` and both use it. `dirt auth status` says which server is in use and where the address came from |
| An old profiles file | Ignored. Nobody but the author has one, and the CLI says what to set when no server is configured |
| What the sync status view holds | Built once in `dirt-core` so that three clients cannot drift: when the last sync succeeded, how many notes wait to be sent, the last failure by class, and for each note that could not be synced its first line, the reason, and what releases it |
| What releases a note | A held note: an edit on a device that can still read it, which sends a copy this device can read. A refused note: an edit on this device, which gives it another try. The words are the core's |
| `dirt sync --verbose` | Prints the same view after the cycle. Without the flag `dirt sync` prints what it does today |
| The Android hand-off | The Rust side asks the activity for the redirect through JNI, as it already asks Android for the encrypted preferences. The activity holds the address in memory and gives it up once. The address carries the authorization code, which is then never written to a file |
| A custom tab | Tried in 7b. Adopted if sign-in, the return and a stopped app all pass on the emulator; otherwise the full browser tab stays and the reason is recorded |

## Needs a person

Two things in this step cannot be done by the agent building it:

- **A sign-in on a physical Android device.** Only an emulator is attached.
- **The Clerk production instance,** which needs a domain bought.

Both are reported at the end of the step. Neither blocks the rest.

## Build order

### 7a: the core, the CLI and the desktop app

- [x] `dirt-core`: the sync status view as one value, with each problem
      note's first line, reason and what releases it.
- [x] `dirt-core`: the rule for the server address.
- [x] CLI: `dirt sync --verbose`; `config_profiles.rs`, `dirt config` and
      `--profile` deleted; `dirt auth status` names the server.
- [x] Desktop: the status view lists the notes, and choosing one opens it.
- [x] Tests: the view for a held note and for a refused one, against the
      real server in `dirt-sim`; the server address rule.

**Result (2026-11-12): done.**

- `dirt_core::sync::overview::SyncOverview` is the view, read through
  `Session::sync_overview`. It holds the words as well as the figures. The
  CLI prints it for `dirt sync --verbose` and inside `dirt auth status`;
  the desktop app renders it in Settings → Sync, where a listed note that
  is on this device opens when its name is pressed.
- `dirt_core::config::ServerAddress` is the rule for the server address.
  An address that is not one is an error, in the CLI and as the desktop
  app's start-up message, and never a reason to fall back to the other
  source: the notes would go to a server the person did not mean.
- The CLI has a build script that reads what the desktop app's reads, so a
  CLI and a desktop app built from one checkout name one server.
- The test of the view against the real server has a phone with three notes
  it cannot sync: one it holds whose newer copy is damaged on the server,
  one it never held, and one too large for the server. It checks how each
  is named, and that doing what the view says releases each.

What building it showed:

- **Replacing the text of a very large note stalled the app.** The test
  needed a note over the server's limit of 1 MiB and then replaced its
  text. The second step did not finish in ten minutes: the search for the
  smallest edit between two large texts with little in common has no bound.
  Saving a note must never wait, so the search now has 100 ms. Past that
  the rest of the text is replaced whole, which is a longer edit that still
  gives the right text. A note of ordinary size is done in far less and is
  not affected. A test replaces 1 MiB of text three ways and fails if any
  takes ten seconds or gives the wrong text.
- **The rewrite of this computer's names matched the start of a host.**
  `10.0.2.20` became `127.0.0.10` and `localhost.example` became
  `127.0.0.1.example`: another machine was taken for this one. The desktop
  app had done this since before the redesign, in two places; step 7a
  carried it into the core and so to the CLI's environment. Only a whole
  host name is rewritten now, the core's is the one copy for the desktop
  app and the CLI, and the Android build's copy, which rewrites the other
  way with the same fault, is on 7b's list. Found in review.
- **A stub was found in the CLI.** `dirt tui` prints "Opening TUI..." and
  does nothing. Neither the product design nor the redesign mentions a
  terminal interface, and no step builds one. It is outside what this plan
  may delete, so it is left and reported for a decision.

### 7b: the Android app

- [x] The redirect handed over by a JNI call.
- [x] The Android build's rewrite of `127.0.0.1` and `localhost` to the
      emulator's name for the computer changed to match a whole host name.
- [x] The custom tab, tried.
- [x] The status view lists the notes.
- [x] Checked on the emulator: sign-in, the return, and a sign-in finished
      after the app was stopped behind the browser.

**Result (2026-11-12): done, with one screen not yet seen on a device.**

- **The hand-off.** `MainActivity` keeps the redirect address in memory and
  gives it up once, to `services::sign_in::take_redirect`, which asks
  through JNI. Nothing is written to the app's files: after a sign-in the
  folder holds no redirect. The activity's part is static, so it does not
  depend on which activity object Android has made, and its two methods
  are marked to be kept through code shrinking, because they are found by
  name at run time.
- **Local references.** The redirect is asked for about three times a
  second while a sign-in is under way. Each call runs in a JNI local
  frame, so a thread that stays attached does not fill its table of
  references while the person is at the sign-in page.
- **The custom tab: adopted.** The sign-in page opens in a tab of the
  browser laid over the app, in the app's own task. When the redirect
  brings the app forward the tab closes by itself; with a full browser tab
  the person was left to close it. If the activity cannot open one, the
  browser is opened as before.
- **The server address.** The Android build writes the address down as it
  was given, and the app applies `ServerAddress::on_android` when it
  starts. That uses the same whole-host rule as the desktop app and the
  CLI, in the other direction.
- **The status view** lists each note that could not be synced, with the
  core's words. A note that is on the device opens when it is pressed.

Checked on the emulator against the server on `dirt-trial`:

- Sign in from Settings: the custom tab opened on Clerk's page, "Allow"
  closed it, and Settings showed the device signed in and synced.
- The same with the app's process stopped while the tab was in front: the
  redirect started the app again and it finished the sign-in from the
  attempt the session had saved.

Not checked on a device: the list of notes that could not be synced. It
needs a note the phone cannot read, which takes a second device and a
damaged copy on the server; 7c's run of the checklist makes one.

The test account was deleted from the trial database afterwards.

### 7c: the documents, and what writing them found

- [x] `docs/feature-parity.md`, fresh: what each client does, feature by
      feature.
- [x] `docs/MANUAL_CHECKLIST.md`: the hotkey, the tray, the Android glue,
      and the account screens that only a real device shows.
- [x] The Android app's search, tag filter, export, share and widget, which
      listing its features found missing.

**Result (2026-11-12): done.**

**Writing the feature-parity document found five things the Android app
did not do.** It had no search, no tag filter and no export, and it ignored
text shared to it and the home-screen widget, though its manifest offered
both. All five had worked before the app's shell was rewritten in May 2026.
The rewrite kept their helper modules, each marked as unused, and left a
note that they would follow. The design asks for search on all three
clients, and its tests section names a checklist for "the Android glue", so
they belong to this step and were built in it:

- **Search and the tag row** ask the store, through the new
  `Store::find_notes`, which matches every word typed and a tag together.
  The list holds the 100 most recently edited notes; searching reaches the
  rest. A note found that way is added to the list's notes when it is
  opened, because the editor opens a note from them.
- **Text shared from another app** becomes a note at once, with no press
  needed: capture does not wait for the person. A shared page keeps its
  title as the first line. **The widget** opens a blank note. Neither
  takes the person out of a note they are writing.
- **Export** hands the file to Android's own screen for choosing where to
  save it. The app needs no storage permission, and the file is where the
  person put it. The code from before the rewrite wrote into the app's
  private folder, which a person cannot open.
- All three go through `services::activity`, the JNI module step 7b made
  for the sign-in redirect, which replaces passing text through environment
  variables. `filters.rs` and `launch.rs` are deleted: the store's search
  and the activity do their work.

What trying them on the emulator showed:

- **A share that arrived while the app was in front was not noticed.** The
  interface asked the activity when its page became visible again, and a
  page that never stopped being visible gets no such event. That happens
  when the app shares the screen with the one sharing, or sits under a
  chooser that does not cover it. The activity now tells the page when an
  intent arrives.
- **A second share before the first was taken replaced it.** The activity
  kept one. It keeps a queue.

Checked on the emulator: a share with the app not running, in front and in
the background; a shared page with a title; the widget's request; search
by the start of a word and for no match; the tag row; opening and changing
a note found by searching; an export saved to Downloads and read back, and
one backed out of.

Not checked: the widget placed on a home screen and pressed. Its intent
was sent with `adb`, which is what pressing it sends.

**The desktop app's list and search reach only its 100 most recent
notes.** It filters the notes it has loaded. Changing that means changing
how it loads notes, and how many it can show at once has to be measured
first. It is recorded in the parity document and placed in step 8's limits
review.

**No client can bring a deleted note back,** though a deleted note keeps
its document and the store has the operation. No design document asks for
a way to do it. It is recorded in the parity document and reported for a
decision, as `dirt tui` is.

### 7d: the checklist, run

- [x] The checklist run on the three clients against `dirt-trial`,
      including what step 6 carried forward: the recovery phrase typed on a
      waiting device, a reset and a deletion. The desktop "Sign in" button
      was not pressed: see below.
- [x] #217 closed with what was seen.

**Result (2026-11-12): every item that was run passed. Some were not run.**
The run is recorded in `docs/MANUAL_CHECKLIST.md`.

What was run, on the CLI, the desktop app and the Android emulator against
the server on `dirt-trial`:

- The CLI as an account's first device; a recovery phrase made; the Android
  app signed in as a second device and let in by **typing the phrase**.
- Notes in both directions, and arriving on return to the Android app
  without anything pressed.
- A note the phone could not read, listed in its Settings; one the computer
  could not read, in `dirt sync --verbose`, `dirt auth status` and the
  desktop app's Settings → Sync. An edit on the device that could read the
  note released it.
- The phone removed with `dirt devices remove`, after which it said it could
  no longer sync.
- **A reset** from the CLI: the account began again with that device only,
  its notes sent again under a new key. The phone signed in afresh and was
  let in by comparing a code, approved from the CLI.
- **A deletion** from the CLI: the server held nothing, the keychain entry
  was gone, and the notes were still on the computer. They were moved into
  the next account with `dirt auth login --move-local-notes`.
- The phone as an account's first device, letting the computer in.
- **#217:** a note written without syncing showed as "1 note is waiting to
  be sent" in the desktop app's Settings → Sync, and "Sync now" cleared it.

What was not run:

- **On the desktop app:** the global hotkey, the tray menu and the "Sign
  in" button, which open or act on the person's own keyboard and browser;
  and the export dialog, the search box, the theme settings, and the
  recovery-phrase, reset and deletion screens. The window minimised itself
  each time it was restored for the second half of the run and could not
  be driven. The reset and the deletion were done from the CLI, and the
  phrase on Android; their desktop screens have still not been pressed.
- **On Android:** the widget placed on a home screen, and a physical
  device.
- **On the CLI:** `dirt auth recover`.

What the run found, each fixed in this step:

- **`dirt edit` failed on Windows when `EDITOR` was a command holding a
  path.** The CLI tries the whole of `EDITOR` as a program's name and, if
  that is "not found", as a program with arguments. Windows calls a name
  with a drive's colon after an argument invalid, not missing, so the
  second attempt was never made. It is made now after any failure. A test
  for Windows fails without the change.
- **A note the device never held was said to be kept.** What releases a
  held note read "this device keeps its own copy and does not send it",
  which is true only of a note the device has. For one it never had, the
  view now says the note will arrive when a device that can read it edits
  it.
- **The desktop app kept a signed-out account's files open.** Its note
  list is loaded through a query library that keeps a query for a while
  after its last use, and the query held the store. The files could not be
  removed until the app closed, though signing out promises to remove
  them. The query now names the store by its file and is given the store
  when it runs. Checked on the running app: after "Sign out" the account's
  folder was gone with the app still open.
- **A removed phone said its notes were syncing.** "Signed in. Your notes
  sync with your other devices" stood above "This device has to sign in
  again". It now says the device can no longer sync. The new line was
  written after the run and has not been seen on a device.
- **The emulator's text entry dropped letters from the phrase,** which the
  app refused with "That is not a recovery phrase: check each word and
  their order." Typed a word at a time it was accepted, with the capital
  letter the keyboard gave its first word.

The test accounts were deleted from the trial database afterwards.

## Exit

- [ ] The manual checklist passes on all three clients, with the two items
      under "Needs a person" reported as not done. **Not met in full:**
      every item run passed, and more than two were not run. They are
      listed under 7d and in the checklist's record of the run, and are
      for a person to do. #243 stays open for them.
- [x] #217 is closed.
- [x] No client holds code the design lists for deletion.
