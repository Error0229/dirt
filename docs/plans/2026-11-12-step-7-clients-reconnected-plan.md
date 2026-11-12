---
title: "Step 7: clients reconnected"
type: plan
date: 2026-11-12
status: in progress
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

- [ ] The redirect handed over by a JNI call.
- [ ] The Android build's rewrite of `127.0.0.1` and `localhost` to the
      emulator's name for the computer changed to match a whole host name.
- [ ] The custom tab, tried.
- [ ] The status view lists the notes.
- [ ] Checked on the emulator: sign-in, the return, and a sign-in finished
      after the app was stopped behind the browser.

### 7c: the documents and the checklist

- [ ] `docs/feature-parity.md`, fresh: what each client does, feature by
      feature.
- [ ] `docs/MANUAL_CHECKLIST.md`: the hotkey, the tray, the Android glue,
      and the account screens that only a real device shows.
- [ ] The checklist run on the three clients against `dirt-trial`,
      including what step 6 carried forward: the recovery phrase typed on a
      waiting device, a reset, a deletion, and the desktop "Sign in"
      button.
- [ ] #217 closed with what was seen.

## Exit

- [ ] The manual checklist passes on all three clients, with the two items
      under "Needs a person" reported as not done.
- [ ] #217 is closed.
- [ ] No client holds code the design lists for deletion.
