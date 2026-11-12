# Feature parity

What each of Dirt's three clients does, feature by feature. Written fresh in
step 7 of the redesign, when the clients were reconnected to the new core.
The matrix for the system before the redesign is in
[archive/feature-parity.md](archive/feature-parity.md).

The clients are the desktop app (`dirt-desktop`), the Android app
(`dirt-mobile`) and the command line (`dirt`). Everything they share is in
`dirt-core`, so a difference between them is a difference in interface, not
in what happens to a note.

When a client gains or loses something, change its row here in the same
pull request.

## Notes

| | Desktop | Android | CLI |
|---|---|---|---|
| Write a note | Yes | Yes | `dirt add`, or `dirt "a thought"` |
| Edit a note | Yes, saved as you type | Yes, with a Save button | `dirt edit <ID>`, in `$VISUAL` or `$EDITOR` |
| Delete a note | Yes | Yes | `dirt delete <ID>` |
| Bring a deleted note back | No | No | No |
| List notes | The 100 most recently edited | The 100 most recently edited | `dirt list`, 10 unless `--limit` says otherwise |
| Use it before signing in | Yes | Yes | Yes |

A deleted note keeps its document, so it can be brought back and the store
has the operation for it. No client offers it. See "Not built" below.

## Finding a note

| | Desktop | Android | CLI |
|---|---|---|---|
| Search | Within the 100 notes in the list, for the text as typed | Every note, by words | `dirt search`, every note, by words |
| Narrow to a tag | Tag row above the list, within the 100 notes in the list | Tag row above the list, every note | `dirt list --tag` |
| Tag counts | Of the 100 notes in the list | Of every note | No |

"By words" is the store's full-text search: a note matches when it holds
every word typed, in any order, and each word also matches as the start of a
longer one. Android shows up to 100 notes found.

**The desktop app cannot reach a note older than its 100 most recent.** Its
list and its search work on the notes it has loaded. The Android app and the
CLI ask the store, which looks through all of them. Changing the desktop app
means changing how it loads notes, and how many it can show at once without
slowing down has to be measured first. That is in step 8's limits review.

## Capture from outside the app

| | Desktop | Android | CLI |
|---|---|---|---|
| Quick capture | Global hotkey, Ctrl+Alt+N, and "New Note" in the tray menu | Home-screen widget, which opens a blank note | The command itself |
| Take text from another app | No | "Share" to Dirt saves the text as a note at once | A pipe: `some-command \| dirt add` |
| Run in the background | Tray icon, with Open and Quit | No | No |

The hotkey is shown in Settings and cannot be changed there.

## Account and devices

| | Desktop | Android | CLI |
|---|---|---|---|
| Sign in | Settings → Account, in the system browser | Settings, in a browser tab over the app | `dirt auth login`; `--no-browser` prints the address |
| Sign out | Yes | Yes | `dirt auth logout` |
| Move notes written before signing in into the account | Yes | Yes | `dirt auth login --move-local-notes` |
| Ask to be let in, and compare the code | Settings → Devices | Settings → Devices | `dirt auth join` |
| Let a waiting device in | Settings → Devices | Settings → Devices | `dirt devices approve` |
| List devices, remove one | Settings → Devices | Settings → Devices | `dirt devices`, `dirt devices remove <ID>` |
| Make or remove the recovery phrase | Settings → Devices | Settings → Devices | `dirt auth phrase new`, `dirt auth phrase remove` |
| Get in with the recovery phrase | Settings → Devices | Settings → Devices | `dirt auth recover` |
| Start the account again | Settings → Devices | Settings → Devices | `dirt auth reset` |
| Refuse a reset somebody asked for | Settings → Devices | Settings → Devices | `dirt auth cancel-reset` |
| Delete the account | Settings → Devices | Settings → Devices | `dirt auth delete-account` |
| Change the account's key | Settings → Devices | Settings → Devices | `dirt auth change-key`, or `dirt devices remove <ID> --change-key` |
| How far a key change has got | Settings → Sync, Settings → Devices | Settings, Settings → Devices | `dirt sync --verbose` |

The desktop app and the CLI on one computer are one device. They share the
data folder and the keychain entry, so signing in with either signs in both.

## Sync

| | Desktop | Android | CLI |
|---|---|---|---|
| Sync without being asked | On start, on return to the window, after a change | On start, on return to the app, after a change | After each command that changes a note |
| Sync now | Settings → Sync | Settings | `dirt sync` |
| When it last synced, and how many notes wait | Settings → Sync | Settings, and a line above the list | `dirt auth status`, `dirt sync --verbose` |
| Each note that could not be synced, with the reason and what releases it | Settings → Sync | Settings | `dirt auth status`, `dirt sync --verbose` |
| Open such a note from the list of them | Yes | Yes | By its ID, which is printed |

Nothing syncs on a timer. An app left open and untouched shows what it had
when it last synced. The three show the same sync status in the same words,
which come from `dirt_core::sync::overview`.

## Export

| | Desktop | Android | CLI |
|---|---|---|---|
| Every note as JSON | Settings → Data | Settings → Export | `dirt export` |
| Every note as Markdown | Settings → Data | Settings → Export | `dirt export --format markdown` |
| Where the file goes | A file dialog | Android's screen for choosing a place | `--output`, or standard output |

## Appearance

| | Desktop | Android | CLI |
|---|---|---|---|
| Light and dark themes | Yes, or follow the system | Light only | Not applicable |
| Font and size | Yes | No | Not applicable |
| Shell completions | Not applicable | Not applicable | `dirt completions <shell>` |

## Not built

| What | Where it stands |
|---|---|
| Attachments | Specified in design section 10. Step 9 of the build order |
| The desktop app reaching notes beyond its 100 most recent | Step 8's limits review |
| Bringing a deleted note back | The store can; no design document asks for a way to do it from a client. Needs a decision |
| `dirt tui` | A stub that prints "Opening TUI..." and does nothing. No design document mentions a terminal interface. Needs a decision: remove it or plan it |
| Themes and fonts on Android | No design document asks for them |
| iOS and web clients | Outside the design (section 3.3) |
