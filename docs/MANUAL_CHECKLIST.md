# Manual checklist

What the automated tests cannot see: the parts of each client that only
exist on a real screen, and the glue between the app and the operating
system. Everything else is covered by tests of `dirt-core` and by
`dirt-sim`, which runs real client cores against the real server.

Run it before a release, and after changing anything it names. Record the
run at the end.

## Before starting

- A server to test against that is not a live one, with its migrations
  applied. `docs/DEPLOY.md` says how to run one.
- A sign-in that the server's Clerk instance accepts. A development
  instance accepts `<anything>+clerk_test@example.com` with the code
  `424242`.
- For the desktop app and the CLI, set `DIRT_DATA_DIR` to an empty folder,
  so that the run does not touch real notes. They share that folder and one
  keychain entry, named after the server: they are one device.
- The desktop app built with `dx build --platform desktop`. A plain `cargo`
  build runs without its style sheets.
- The Android app on an emulator, and on a physical device when one is to
  hand. Record which.

Afterwards, delete the test account, the keychain entry and the app's data.

## Desktop app

| # | Do | Expect |
|---|---|---|
| D1 | Start the app with no account | The list, and "Not signed in" in Settings → Account |
| D2 | Press Ctrl+Alt+N with another window in front | The quick-capture box. Type, press Enter: the note is in the list |
| D3 | Close the window | The app stays in the tray. "Open Dirt" brings the window back, "New Note" opens quick capture, "Quit" ends it |
| D4 | Settings → Account → Sign in | The system browser opens on the sign-in page. After it, the app says it is signed in |
| D5 | Write a note. Settings → Sync | "Synced just now", nothing waiting |
| D6 | Change a note on another device, then click on the app's window | The change appears without pressing anything |
| D7 | Settings → Sync with a note that cannot be synced (see "Making a note that cannot be synced") | The note by name, the reason, and what releases it. Pressing the name opens the note |
| D8 | Settings → Devices with another device waiting | "Let it in" shows a code. With "They match" on both, the app says the device was let in |
| D9 | Settings → Devices → make a recovery phrase | 24 words, shown once |
| D10 | Settings → Data → Export JSON, then Markdown | A file dialog each time, and a file holding every note |
| D11 | Search for a word; press a tag | The list narrows; "All" widens it again |
| D12 | Settings → Theme: light, dark, system; font and size | The window follows at once |

## Android app

| # | Do | Expect |
|---|---|---|
| A1 | Start the app with no account | The list, with "Not signed in" above it |
| A2 | Settings → Sign in | A browser tab over the app. After "Allow" it closes by itself and Settings says signed in |
| A3 | Sign in again from a fresh install, and stop the app's process while the browser tab is in front (`adb shell am kill <package>`), then press "Allow" | The app starts again and is signed in |
| A4 | In another app, share a piece of text to Dirt | Dirt comes forward with the text as the newest note |
| A5 | The same with Dirt not running | Dirt starts with the text as the newest note |
| A6 | Share a web page from the browser | A note with the page's title on the first line and its address below |
| A7 | Add the "Quick capture" widget to the home screen and press it | A blank note to write in |
| A8 | Type in the search box; press a tag | The list narrows. A note older than the 100 most recent is found. Pressing the tag again, or "All", widens the list |
| A9 | Open a note found by searching, change it, save | The change is kept; the list is as before |
| A10 | Settings → Export JSON, then Markdown | Android's screen for choosing a place. After "Save" the app says how many notes were saved, and the file holds them. Backing out says nothing was saved |
| A11 | Change a note on another device. Leave Dirt and come back | The change appears |
| A12 | Settings with a note that cannot be synced | The note by name, the reason, and what releases it. Pressing it opens the note if the device holds it |
| A13 | Settings → Devices on a device that is waiting: ask another device | The same code on both. With "They match" on both, the notes arrive |
| A14 | Settings → Devices on a device that is waiting: type the recovery phrase | The device is let in and the notes arrive |
| A15 | A sign-in on a physical device | As A2 |

## CLI

| # | Do | Expect |
|---|---|---|
| C1 | `dirt auth status` with no server set | "No server is configured", and what to set |
| C2 | `dirt auth login` | The browser opens; afterwards "Signed in" |
| C3 | `dirt "a thought"`, then `dirt auth status` | The note's ID at once; the status names the server and says it synced |
| C4 | `dirt sync --verbose` with a note that cannot be synced | The note by name and ID, the reason, and what releases it |
| C5 | `dirt auth join` on a waiting device, `dirt devices approve` on another | The same code on both. With `y` on both, the new device is let in |
| C6 | `dirt auth phrase new`, then `dirt auth recover` on a waiting device | 24 words; typing them lets the device in |
| C7 | `dirt devices`, `dirt devices remove <ID>` | The list; the removed device can no longer sync |
| C8 | `dirt auth reset --yes` on an approved device | The account starts again, empty, with this device in it |
| C9 | `dirt auth delete-account --yes` | The account is gone from the server; the notes are still on this computer |
| C10 | `dirt auth logout` on the only device of an account with no phrase | Refused, with the reason |

## Making a note that cannot be synced

D7, A12 and C4 need a note the device cannot read. With two devices in one
account, A and B:

1. Write a note on A and sync both, so that B holds it.
2. Change the note on A and sync A.
3. Before B syncs, change one byte of that note's row on the server:
   `UPDATE notes SET ciphertext = set_byte(ciphertext, 40, get_byte(ciphertext, 40) # 1) WHERE note_id = '<ID>';`
4. Sync B. It lists the note as one it cannot read, and keeps its own copy.
5. Change the note on A again and sync both. B's list is empty again.

## Runs

| Date | Server | Desktop | Android | CLI | Not run |
|---|---|---|---|---|---|
