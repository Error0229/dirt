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

D2, D3 and D4 need a person at the keyboard: a global hotkey, the tray, and
the system browser cannot be driven from a script without acting on whatever
else is on the screen. So do D10, D18, D20, D21, D24 and D25. A file dialog opens in the
person's own folders with one of their files selected, and "Open" starts
another program on their screen; neither is for a script on a desktop that
somebody is using.

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
| D13 | Settings → Account → Sign out, with a recovery phrase made | "Signed out", and the account's folder under `stores` in the data folder is gone while the app is still open |
| D14 | Settings → Devices → Start the account again; Delete the account | Each asks for a second press. Afterwards the app says what happened and the server holds what the CLI's C8 and C9 expect |
| D15 | Settings → Devices → remove a device, then Change the account's key | Removing says the device still has the key and points to the change. The change asks for a second press, says the key was changed and that a new recovery phrase is needed; the Recovery phrase row says the account has none |
| D9 | Settings → Devices → make a recovery phrase | 24 words, shown once |
| D10 | Settings → Data → Export JSON, then Markdown | A dialog for choosing a folder each time. In the folder chosen is a new one, `dirt-export-<time>`, holding `notes.json` or `notes.md` and, under `files`, a folder for each note with files. The app says how many notes and files were exported, and where. What was in the chosen folder is untouched |
| D11 | Search for a word; press a tag | The list narrows; "All" widens it again. A note older than the 100 most recent is found |
| D16 | With more than 100 notes, scroll to the end of the list and press "Show older notes" | 100 more notes appear below, and the button stays for as long as there are older ones. An open note stays open |
| D12 | Settings → Theme: light, dark, system; font and size | The window follows at once |
| D17 | Open a note and press "Attach a file"; choose a picture and a text file together | Both appear under the note, the picture with its thumbnail. After a moment neither says it is waiting to be sent |
| D18 | Drop a file from the file manager on the note's text | It is attached. The window does not leave the app to show the file |
| D19 | On another device attach a picture to the same note; sync here | The file appears with its thumbnail and says it is not on this device |
| D20 | Press "Open" on that file | It is fetched and opens in the system's viewer under its own name. The row no longer says it is not on this device |
| D21 | Press "Save a copy" | The dialog proposes the file's name. The saved copy is the same as the original |
| D22 | Press "Remove"; sync the other device | The file is gone from the note on both |
| D23 | Attach a file named `something.exe` (any file will do) | It is attached. Its row offers "Save a copy" and "Remove", and no "Open" |
| D24 | With a note whose file this device has never opened, export | The file is in the export, the same as the original: it was fetched. Its row in the note no longer says it is not on this device |
| D25 | The same with the network off | The export is written without the file. The app names the file and the note, and says why |
| D26 | On Linux or macOS: start the app, then `ls -ld` the data folder (`~/.local/share/dirt` or `~/Library/Application Support/dirt`) | `drwx------`: nobody but the owner. One that was `drwxr-xr-x` before the app started is closed too |

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
| A19 | With more than 100 notes, scroll to the end of the list and press "Show older notes" | 100 more notes appear below. With exactly as many notes as are shown, one more press removes the button |
| A10 | Settings → Export JSON, then Markdown | "Exporting...", and then Android's screen for choosing a place, with `dirt-export-<time>.zip` proposed. After "Save" the app says how many notes and files were saved. The zip file holds `notes.json` or `notes.md` and, under `files`, each note's files, the same as the originals. Backing out says nothing was saved |
| A11 | Change a note on another device. Leave Dirt and come back | The change appears |
| A12 | Settings with a note that cannot be synced | The note by name, the reason, and what releases it. Pressing it opens the note if the device holds it |
| A13 | Settings → Devices on a device that is waiting: ask another device | The same code on both. With "They match" on both, the notes arrive |
| A14 | Settings → Devices on a device that is waiting: type the recovery phrase | The device is let in and the notes arrive. A phrase with a word wrong is refused, with the reason |
| A16 | Remove the device from another one, then sync | "This device can no longer sync", and a button to sign in again |
| A17 | With the device offline, change the account's key on another device and write a note there; write a note here; go online and sync | Both notes are on both devices. Nothing is asked of the person |
| A18 | Settings → Devices → Change the account's key | As D15 |
| A15 | A sign-in on a physical device | As A2 |
| A20 | Open a note and press "Attach a file"; choose a photograph | Android's screen for choosing files. The photograph appears under the note with its thumbnail, and after a moment does not say it is waiting to be sent |
| A21 | On another device attach a picture and a text file to a note; open the note here | Both are listed as not on this device, the picture with its thumbnail |
| A22 | Press "Open" on the text file | Android offers the apps that show such a file. The one chosen shows it, under its own name |
| A23 | Press "Save a copy" | Android's screen for choosing a place, with the file's name. The saved copy is the same as the original |
| A24 | Press "Remove"; sync the other device | The file is gone from the note on both |
| A25 | In another app, share a picture to Dirt | Dirt is among the apps offered. A note appears at the top of the list, called after the picture, with the picture attached |
| A26 | Start a new note and press "Attach a file" before saving | The note is saved with the file attached, and stays open |
| A27 | Attach a file larger than 10 MiB | It is not attached, and the app says how large a file may be |
| A28 | Export with a note whose file this device has never opened | The file is in the zip file: it was fetched first |
| A29 | The same in aeroplane mode | The zip file is saved without the file. The app names the file and the note, and says why, in red |

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
| C8 | `dirt auth reset --yes` on an approved device | The account starts again with this device as its only one. The notes on this device are sent again under a new key; the other devices and the recovery phrase are gone |
| C9 | `dirt auth delete-account --yes` | The account is gone from the server; the notes are still on this computer |
| C10 | `dirt auth logout` on the only device of an account with no phrase | Refused, with the reason |
| C11 | `dirt devices remove <ID> --change-key`, then write a note and `dirt sync --verbose` | The device is removed and the key changed. The removed device cannot sync. The other devices get the note |
| C12 | `dirt auth change-key` on an account with a recovery phrase | The key is changed, and the command says the phrase no longer works and how to make a new one. `dirt auth recover` with the old phrase on a waiting device is refused |

| C13 | `dirt attach <note> <a picture>`, then `dirt files <note>` | The file's ID. The list shows the file with its size and kind and nothing more, because it has been sent |
| C14 | On another device: `dirt sync`, `dirt files <note>`, `dirt save <note> <name>` | The file is listed as not on this device; saving fetches it, and the copy is the same as the original byte for byte |
| C15 | `dirt save` again to the same place | Refused: the file is already there. With `--force` it is replaced |
| C16 | `dirt attach` with the file storage out of reach | The file's ID on standard output, and on standard error that 1 file is waiting. `dirt files` says so too. `dirt sync` once the storage is back sends it |
| C17 | `dirt detach <note> <name>`, then `dirt sync` on the other device | The file is gone from the note on both |
| C18 | On a device that has not fetched a note's file: `dirt export --folder out` | `out` on standard output, and on standard error how many notes and files. `out/notes.json` names the file with its path, and the file at that path is the same as the original |
| C19 | `dirt export --folder out` again | Refused: `out` is already there, and nothing in it is changed |
| C20 | With the file storage out of reach, on a device that lacks a file: `dirt export --folder out2` | `out2` is written without the file. Standard error names the file, its note and the reason, and the exit status is 1 |
| C21 | `dirt export`, and `dirt export --format markdown --output notes.md` | The notes' text, naming each note's files with no path. Standard error says how many files are named and not exported |
| C22 | On Linux or macOS: any command, then `ls -ld` the data folder | `drwx------`. A folder that was open to others before is closed. With `--db-path` in another folder, that folder's permissions do not change |

## Making a note that cannot be synced

D7, A12 and C4 need a note the device cannot read. With two devices in one
account, A and B:

1. Write a note on A and sync both, so that B holds it.
2. Change the note on A and sync A.
3. Before B syncs, change one byte of that note's row on the server:
   `UPDATE notes SET ciphertext = set_byte(ciphertext, 40, get_byte(ciphertext, 40) # 1) WHERE note_id = '<ID>';`
4. Sync B. It lists the note as one it cannot read, and keeps its own copy.
5. Change the note on A again and sync both. B's list is empty again.

## Notes for whoever runs it

- On an emulator, `adb shell input text` drops letters from long text. Type
  a recovery phrase one word at a time.
- A phone's keyboard gives the first word of a phrase a capital letter. The
  phrase is accepted with it.
- Returning to an app syncs only if the last sync was more than 30 seconds
  ago. Wait that long before A11 and D6.
- The desktop app shows nothing to Windows UI Automation while its window
  is minimised.
- Both servers have file storage since 2026-11-13. For trying files with
  no account and no network,
  `cargo run -p dirt-sim --example serve` runs the real server on
  this machine with the storage from `compose.yaml` and a sign-in that
  needs no account; the comment at the top of
  `crates/dirt-sim/examples/serve.rs` says how to sign in and how to have
  two devices on one computer.

## Runs

| Date | Server | Desktop | Android | CLI | Not run |
|---|---|---|---|---|---|
| 2026-11-13, files through R2 | `dirt-trial`, with its R2 bucket | Not run | Emulator, with no window. A2 and A13 on the way: signed in through Clerk and was let in from the CLI, the codes the same. A21 passed, with three files attached on the computer: the picture's thumbnail was there, and "Open" fetched the picture and showed it. A20 with a 684 KiB file that is not a photograph | C13 passed, with three files (1.4 MiB, a text file named in Chinese, a picture). C14 for the phone's file: the copy `dirt save` wrote is the same as the original, byte for byte | The desktop app. A phone that is not an emulator. Removing a file, the limits and the daily sweep on this server |
| 2026-11-13, the production server's first day | Production, `https://api.dirt.catjam.dev` | Not run | Not run | C2 as far as the browser: `dirt auth login --no-browser` printed an address at the production sign-in service, and the page it opened asked for an email address under "Sign in to Dirt". Not finished | Everything else. Nobody has signed in: production has no test user, a real address has to receive the code, and an account's first device should be its owner's |
| 2026-11-13, for step 9f | The real server on this machine (`dirt-sim`'s `serve`), with the local file storage, reached from the emulator through `adb reverse` | Not run | Emulator, with no window. A10 passed, for JSON and for Markdown: the zip file saved to the phone's Downloads held the notes and every file, each compared byte for byte with the original, one of them under a name in Chinese. A28 passed: the phone had opened none of the files. A29 passed with the phone's network switched off: the zip file was saved without the one file, and the app named it in red | C18 to C21 passed, with two devices on one computer: a device that had fetched nothing exported four files, each the same as its original, among them two of one name and one named `../../outside.txt`, which was written inside the export. C22 passed on Linux (Ubuntu in WSL): a new data folder was `700`, one that had been `755` was closed, and the folder of a database given with `--db-path` stayed `755` | D10, D24 and D25: the desktop app chooses the folder in a file dialog, which is not driven on a desktop somebody may be using. What it writes is the core's, as the CLI's is, and its own part has tests. D26 and C22 on macOS, where the same tests run in CI. A physical phone |
| 2026-11-13, for step 9e | The real server on this machine (`dirt-sim`'s `serve`), with the local file storage, reached from the emulator through `adb reverse` | Not run | Emulator, with no window. A2 and A4's joining again on the way, against the stand-in sign-in. A20 to A27 passed: the photograph attached on the phone, the copy saved on the phone and the picture shared from Android's Files app were each compared byte for byte with the original. A twelve-megapixel photograph took about three and a half seconds to attach | The first device, and the check on what the phone sent | A physical phone, and so what its camera saves. A share of several pictures at once |
| 2026-11-13, for step 9d | The real server on this machine (`dirt-sim`'s `serve`), with the local file storage | D17 passed, through the file dialog, with one file. D19 passed: the thumbnail came by itself and the picture stayed away. D22's "Remove" passed, checked against `dirt files` on the same device. D23 passed: a file named `setup.exe` had no "Open". The rows were looked at in a capture of the window | Not run | Used as the second device, and to check the desktop's store | D18: needs a hand to drop a file. D20: "Open" was pressed once, by a person at the machine and not by the script; the file was fetched and a copy made under its own name, and what the viewer showed was not seen. D21: the dialog opened with the right title and was not answered, because it had one of the person's own files selected |
| 2026-11-12, for step 9c | The real server on this machine (`dirt-sim`'s `serve`), with the local file storage | Not run | Not run | C13 to C17 passed, with two devices on one computer. A 4.4 MiB picture attached, with its thumbnail and the sync, in 1.8 s in a debug build. C5 again on the way | Against Cloudflare R2, and against `dirt-trial`: neither has storage for files yet |
| 2026-11-12, for step 8c | None: not signed in | D16 passed, driven through UI Automation, and the same under a tag. D11's tag row | Emulator. A19 passed, and the same under a tag | Used to write the 230 notes | D16's "an open note stays open", which a test covers. Search typed into either app |
| 2026-11-12, for step 8b | `dirt-trial` | Not run: D15 | Emulator. A16, A17 and A18 passed | C11 passed. C12 on an account with no phrase: the key was changed | D15: the desktop window could not be driven in the earlier run and was not tried again. C12's phrase: the account in the run had none |
| 2026-11-12 | `dirt-trial` | D5, D6, D7, D13 passed. D11: the tag row only. D8 passed in step 6, not run again | Emulator. A1 to A6 and A8 to A14 passed. A8 without a note beyond the first 100, which the tests cover. A16: the phone said it had to sign in again; its first line was reworded afterwards and the new words have not been seen on a device | C1 to C10 passed. C2 with `--no-browser`. C6's `dirt auth recover` not run; the phrase was typed on Android instead | D1 to D4, D9, D10, D11's search, D12, D14: they need a person at the keyboard, or the window could not be driven. A7 with a widget placed on a home screen; its intent was sent instead. A15: no physical device |
