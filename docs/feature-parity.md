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
| A long note | As long as it is written or pasted; a note too large for the server is kept here and is named in Settings → Sync as too large to sync | The same | The same, from `dirt add` or `dirt edit` |
| Write a note | On the page, always open beside the list: the first character makes the note, and it saves itself. Esc or Ctrl Enter is done and gives a blank page at once; there is no ✓; what is typed straight after is the next thought. The words gather into the note's grain, which is tossed onto its row; an old note's words fade where they were. + at the foot of the list, in reach however far it is scrolled, does the same as Ctrl N, and so does + at the window's right edge while the list is folded away. Narrower than 720 px, ‹ at the edge goes back to the list as Esc does. With the list narrowed to tags, a new page gets them at its end | The bar at the bottom, "What's on your mind?", grows into a page with the keyboard up: the first character makes the note, and it saves itself. ⌄ or Back is done: the page closes to the note's grain, which is tossed onto its card. With the list narrowed to tags, a new page gets them at its end | `dirt add`, or `dirt "a thought"` |
| Focus while writing | The first key fades the controls; at the first pause or new line the list goes, by the style chosen in Settings → Appearance: Slide (the default), Rail or Zen. In Rail the list is a column of the notes' grains, each in its note's shade of sand. The note's icons, at the window's right edge, go at the first key, stay away through a pause, and come back once the hand is on the mouse (40 px of movement since the last key), with + while the list is folded away; the window's controls and the list's handle come back as the pointer nears them | Not applicable: a note fills the screen | Not applicable |
| Edit a note | Yes, saved as you type | Tap its card: the note grows out of it, then the keyboard rises with the cursor at the end of its text, and it saves as you type. ‹ or Back closes it into its card, and the keyboard goes once it has | `dirt edit <ID>`, in `$VISUAL` or `$EDITOR` |
| Delete a note | Shift Del in the list, Ctrl Shift Del in a note, or the bin on an archived note. No question first: a toast offers **Undo** | On an archived note: swipe its card left, or the bin in its bar. No question first: a toast offers **Undo** | `dirt delete <ID>` |
| Bring a deleted note back | **Undo** on the toast, or Ctrl Z outside the text, for the last archive or delete, after the toast has gone too. It comes back into its place in the list | **Undo** on the toast, for six seconds; with TalkBack on, until the toast is dismissed | `dirt restore <ID>`, by the ID that `dirt delete` printed or the start of it |
| Archive a note, and bring it back to the list | The box on a row under the pointer, Del in the list, the box at the open note's edge or Ctrl E in it; **Undo** or Ctrl Z, or "Back to the list" beside an archived note | Swipe its card left, or the box in the open note's bar; **Undo**, or "Back to the list" in an archived note's bar | `dirt archive <ID>`, `dirt unarchive <ID>` |
| List the archived notes | "3 archived" under the pile, newest first; Esc or ‹ goes back | "3 archived" under the pile, newest first; ‹ or Back goes back | `dirt list --archived`, which `--tag` narrows as it does the list |
| List notes | Most recently edited first, in layers by when each was edited (today, This week, Earlier this month, then by month). 100 at first, and 100 more as scrolling nears the end. Archived notes are left out. A hollow dot marks a note not on the server yet. Under the last note, the pile: a heap of sand with a grain for every note, archived ones among them, and "1,254 thoughts since Nov 2025" | As cards, in the same layers, 100 at first and 100 more as scrolling nears the end. Archived notes are left out. A hollow dot marks a note not on the server yet, and a picture's thumbnail shows on its card when the device has it. The pile under the last card, as on the desktop | `dirt list`, 10 unless `--limit` says otherwise. Archived notes are left out |
| Use it before signing in | Yes | Yes | Yes |

A deleted note keeps its document, so it can be brought back, with the text
it had. `dirt restore` does it, and the apps' **Undo**. A note that was
archived when it was deleted comes back archived. It keeps the edit time it
had, on every device, so it comes back into its place in the list; bringing
it back still syncs as a change.

In an account, a deleted note is purged thirty days after a device saw it
deleted: its document goes from the server and from every device, and it
can no longer be brought back. Notes kept on a device alone, with nobody
signed in, are not purged: there is no limit for them to count against.

An archived note is out of the list and is still a note: it syncs, is found
by a search, opens and can be edited, and is exported. Archiving is not an
edit. The note keeps its edit time, and a note that another device deleted
stays deleted when this one archives it. A note archived from the CLI, or
on a device with a later version, leaves its list all the same.

## Finding a note

| | Desktop | Android | CLI |
|---|---|---|---|
| Search | Find, at the top of the list (Ctrl F, Ctrl K, `/`): every note, by words, narrowing on every key | Find, behind 🔍 at the top, which slides down under the title: every note, by words, narrowing on every key. Its cross, or Back, closes it | `dirt search`, every note, by words |
| Narrow to a tag | A tag on any row, or the `#` in Find, which opens a list of the six that would narrow most and stays open for more; several at once, with "all of them" or "any of them" | A tag on any card, or the `#` in Find, which opens a strip of pills under it: the six that would narrow most, and the picked ones filled at the front. Several at once, with a last pill for "all of them" or "any of them" | `dirt list --tag`, without the archived notes; `dirt list --archived --tag` for those |
| Tag counts | How many notes there would be with that tag too, archived ones among them | No numbers. A tag that would leave none is not offered, as on the desktop | No |
| Archived notes among what is found | After the rest, under a line "Archived", their titles a shade lighter | The same | After the rest, with `(archived)` at the end of the line, and `"archived"` in `--json` |

"By words" is the store's full-text search: a note matches when it holds
every word typed, in any order, and each word also matches as the start of a
longer one. The apps show up to 100 notes found, and 100 more as scrolling
nears the end.

Finding keeps archived notes, by words or by a tag: they come after every
note that is not archived, however well they match. So the number beside a
tag counts them, and is the number of notes that narrowing to the tag shows.

All three ask the store, which looks through every note. The apps hold only
the notes their list shows, so the list starts at the 100 most recently
edited and grows when asked. `docs/LIMITS.md` has what the store takes to
answer with 20,000 notes in it.

## Capture from outside the app

| | Desktop | Android | CLI |
|---|---|---|---|
| Quick capture | Global hotkey, Ctrl+Alt+N, and "New note" in the tray menu: a small window of its own over any app, kept ready, which saves as it is typed and is put away by Esc, Ctrl Enter or a click outside | The capture sheet, a small screen of its own over any app, from the home-screen widget (a bar, or at one or two cells only a pencil and the mic), a long press on the icon (New note), the Quick Settings tile, or the phone's note-taking role; also over the lock screen, where it adds and shows nothing. ✓ keeps the thought, which becomes a note and syncs without the app being opened | The command itself |
| Take text from another app | No | "Share" to Dirt saves the text as a note at once, with a toast and Undo | A pipe: `some-command \| dirt add` |
| Take a picture from another app | No | "Share" a picture, or several, to Dirt: a note at once, with the pictures attached, called after them if no text came along | No |
| Run in the background | The tray icon, the logo in the chosen accent: a click opens Dirt on Windows (on a Mac and Linux a click shows the menu); its menu has Open Dirt, New note with the hotkey, and Quit Dirt | After the capture sheet's ✓, a job on the phone that needs no network makes the thought a note (and the words of what was said), then one that syncs once; never on a timer | No |
| The pile on the home screen | No | The pile widget, two cells by two: the heap of every thought as sand, as under the list, and "1,254 thoughts". A thought kept from the capture sheet is one more grain at once. Tapping it opens Dirt | No |

The hotkey is shown in Settings and cannot be changed there.

## Voice

| | Desktop | Android | CLI |
|---|---|---|---|
| Say a thought | The mic at the blank page's edge: hold it, speak, let go; or tap it, and ✓ or Enter keeps what was said, ✕ or Esc throws it away. Held, a slide up listens on hands-free and a slide to the left throws it away. While it listens the page shows a row of grains that move with the voice and a timer, and nothing else. Nothing heard: it shakes and nothing is made. Stops at ten minutes, keeping what was said | The mic beside the bar in the dock, with the same gesture: a sheet rises over the lower half of the screen with the grains, the timer, ✕ and ✓. Leaving the app, opening a note or Settings, or Back keeps what was said. Nothing heard: it shakes and nothing is made. The capture sheet's 🎙 listens in the sheet, the same way, without opening the app | No |
| Say something into a note being written | No: type, or use the system's dictation | 🎙 in the note's bar, with the same gesture: it listens under the bar, the keyboard down. The recording is among the note's files at once, and its words come at the end of the note, after a blank line; on a page with no note yet it makes a note said, as the dock does. Back keeps what was said and the page stays; leaving keeps it too | No |
| Say a thought from any app | Hold Ctrl Alt N: quick capture opens listening, and letting go keeps it; it flies to the tray as a typed thought does. A tap opens it for typing, as before | The widget's mic opens the capture sheet listening, hands-free. **Talk** ("Dirt Talk" among the apps, and on a long press of the icon) opens it listening at once and keeps the thought at a pause: a hardware key can be set to open it, over the lock screen too where the phone allows | No |
| What is kept | At once, a note with the recording (Ogg Opus) as its file, named "Voice · 0:12" with a mic on its row until its words come. The words are made afterwards on this computer, in the background, and put at the start of the note; if Dirt is closed first, they are made when it next starts. "No words found" when the model found none | As on the desktop, made on this phone: by the app, or by a job of its own when the thought was said in the capture sheet and the app is not running. A note said on another device shows its name and its mic, and its words when that device has made them | `dirt list` names such a note "Voice · 0:12" until its words come, "(no words found)" when there were none |
| Where speech becomes text | On this computer, with the model shipped in the app (Parakeet TDT-CTC 110M, NVIDIA, CC BY 4.0, credited in Settings → Voice). Nothing is sent anywhere but the encrypted note and file | The same, on this phone: the model is in the app and copied out of it once, on first use | Not applicable |
| Keep the recording, or the text only | Settings → Voice → The recording: Keep, or Text only, which takes the recording off once its words are in | The same, and a word about Talk and the side key | No |
| The microphone's permission | Windows' own setting; the mic shows a slash and says where | Asked at the first press, in Android's dialog. Refused, the mic shows a slash and a press says why; refused for good, it offers Android's page for the app | Not applicable |
| Choose the microphone | Settings → Voice → Microphone: the system's default or any input connected, kept in this device's settings. One not connected when talking, or one that cannot record, gives way to the default, and the row says so. On Linux only the inputs that can record are listed | No: the system's microphone | Not applicable |
| Play a recording | The file preview, as any sound | The file preview, as any sound | `dirt save`, then play it |

## Account and devices

| | Desktop | Android | CLI |
|---|---|---|---|
| Sign in | Settings → Account → Sign in, in the system browser; the button waits with a spinner, and a press while it waits stops waiting: one sign-in at a time | Settings → Account → Sign in, in a browser tab over the app; the button waits with a spinner, and a press while it waits stops waiting: one sign-in at a time | `dirt auth login`; `--no-browser` prints the address |
| Sign out | Yes. Signed out by the CLI meanwhile, it notices when it next has the focus or a write is refused, and saves nothing more to the old notes | Yes | `dirt auth logout`; says when the desktop app still has the notes open, which then go when it lets go, and when the server could not be told. While the desktop app holds the last sign-in's notes, a new sign-in waits and each command says so, working on this computer's own notes |
| A device waiting to be let in | Settings → Devices, and the sync status says so | Settings → Devices, and the sync status says so | Each note command says so first, on standard error, and how to be let in; `dirt sync` fails saying it |
| Move notes written before signing in into the account | A box checked by default under Sign in; afterwards "Bring them in" in Settings → Account | As on the desktop: a box checked by default under Sign in; afterwards "Bring them in" | `dirt auth login --move-local-notes` |
| Ask to be let in, and compare the code | Settings → Devices, which begins by itself on a device that waits: seven pictures with their names | Settings → Devices, which begins by itself on a device that waits: seven pictures with their names, and the digits | `dirt auth join` |
| Let a waiting device in | The banner's Review, Settings → Devices → Add a device, or a waiting device's Let it in | The banner's Review, Settings → Devices → Add a device, or a waiting device's Let it in | `dirt devices approve` |
| List devices, remove one | Settings → Devices, ✕ on a device and a confirmation in its row | Settings → Devices, ✕ on a device and a confirmation in its row | `dirt devices`, `dirt devices remove <ID>` |
| Make or remove the recovery phrase | Settings → Devices; Copy, or Save as a file (`dirt-recovery-phrase.txt`, where the person chooses) | Settings → Devices; Copy, or Save as a file through Android's screen for choosing a place | `dirt auth phrase new`, `dirt auth phrase remove` |
| Get in with the recovery phrase | Settings → Devices on a device that waits: "No other device? Use your recovery phrase." | Settings → Devices on a device that waits: "No other device? Use your recovery phrase." | `dirt auth recover` |
| Start the account again | Settings → Account → Account actions | Settings → Account → Account actions | `dirt auth reset` |
| Refuse a reset somebody asked for | Settings → Account, where the request is shown | Settings → Account, where the request is shown | `dirt auth cancel-reset` |
| Delete the account | Settings → Account → Account actions, with DELETE typed | Settings → Account → Account actions, with DELETE typed | `dirt auth delete-account` |
| Change the account's key | Settings → Account → Account actions | Settings → Account → Account actions | `dirt auth change-key`, or `dirt devices remove <ID> --change-key` |
| How far a key change has got | Settings → Sync, and the key's row under Account actions | Settings → Sync, and the key's row under Account actions | `dirt sync --verbose` |

The desktop app and the CLI on one computer are one device. They share the
data folder and the keychain entry, so signing in with either signs in both.

## Files

| | Desktop | Android | CLI |
|---|---|---|---|
| Attach a file to a note | The paperclip at the note's edge (its tooltip, while the note has no files, says a file can be dropped or pasted too), or Ctrl O, in a file dialog that takes several; or drop files anywhere on the window; or paste a picture into the page. On the blank page any of them makes a note of the files | The paperclip in the page's bar, on Android's screen for choosing files, which takes several (20 at most at once). On a new page with no words it makes a note of the files | `dirt attach <note> <file>`; `--name` and `--type` say what the path does not |
| List a note's files | On the shelf at the foot of the open note's column, held there while the text scrolls: one line, "3 files", that folds and unfolds them. Unfolded, a tile for each: its thumbnail or kind, name, size and kind, and where it is if it is not both here and on the server ("uploading", "on this device only", "not on this device; fetched when it is wanted"), at most three rows, the rest scrolling inside. Folded, a small square for each on the line. Every note opens folded, whatever its count; a file attached, here or by a sync, pops in as a square on the folded line and does not unfold it; only the person unfolds it, and the choice is not kept | The same, at the foot of the page, above the keyboard when it is up | `dirt files <note>`, the same; `--json` |
| Look at a file | A click on its tile or, folded, its square, or Space: the preview, inside the app, from memory. Pictures, sounds, videos, text and PDFs (drawn by the bundled PDF.js); a picture whole, double-click to zoom it, then drag or scroll it; ← → for the note's other files; Esc, Space, a click on the backdrop or ✕ to close. A file not on the device is fetched first, with a ring on its tile. On a file of another kind a click shows its icons | The same, with a tap on its tile or its square: the same kinds, PDFs by the bundled PDF.js; swipe sideways for the note's other files, two fingers to zoom a picture, one to move it zoomed; swipe down, Back or ✕ to close | No: `dirt save`, then open it |
| Open a file | "Open in another app", an icon on the file under the pointer and in the preview, hands a copy to whatever the system uses for files of its kind, fetching the file first if need be; on Windows the copy is marked as downloaded. Not offered for a file the system would run | "Open in another app", an icon on the file and in the preview, lends a copy by a `content:` address that grants reading that one file, to the app Android lets the person choose. Not offered for a file the system would run or install | No: `dirt save`, then open it |
| Save a copy of a file | "Save a copy", an icon on the file and in the preview, in a file dialog that proposes the file's name | "Save a copy", an icon on the file and in the preview, on Android's screen for choosing a place | `dirt save <note> <file> [path]`. Does not replace a file that is there unless `--force` |
| Remove a file from a note | "Remove from the note", an icon on the file and in the preview, with the toast "File removed" | "Remove from the note", an icon on the file and in the preview, with the toast "File removed" | `dirt detach <note> <file>` |
| See a picture's thumbnail | In the file's tile. Fetched by itself, without the picture | The same | Not applicable |

A photograph gets a thumbnail if it is a JPEG, a PNG, a GIF or a WebP. One
saved as HEIC is attached without: the core does not read HEIC. The
emulator's camera saves JPEG. What a given phone's camera saves has not
been looked at; many offer both.

A file is named by the start of its ID or by its name. A picture gets a
thumbnail when it is attached, whichever client attaches it. A file's name
comes out of a note and is never used as a path: `dirt save` writes it as a
plain name in the folder asked for.

## Sync

| | Desktop | Android | CLI |
|---|---|---|---|
| Sync without being asked | On start, on return to the window, after a change | On start, on return to the app, after a change | After each command that changes a note |
| Sync now | The sync status at the foot of the list, or Ctrl R | Pull the list down, or tap the sync icon at the top | `dirt sync` |
| When it last synced, and how many notes and files wait | The sync status at the foot of the list, its tooltip, and Settings → Sync | The sync icon at the top, with a grey dot when notes wait offline and an amber one when something needs the person; a long press gives its words. And Settings → Sync | `dirt auth status`, `dirt sync --verbose`. `dirt sync` and any command that changes a note say when a file did not go |
| Each note that could not be synced, with the reason and what releases it | Settings → Sync, which the sync status opens when there is a problem | Settings → Sync, which the sync icon opens when there is a problem | `dirt auth status`, `dirt sync --verbose` |
| Each file the server refused, with the reason | Settings → Sync | Settings → Sync | `dirt auth status`, `dirt sync`, `dirt files <note>` |
| Open such a note from the list of them | Yes | Yes | By its ID, which is printed |

Nothing syncs on a timer. An app left open and untouched shows what it had
when it last synced. The three show the same sync status in the same words,
which come from `dirt_core::sync::overview`.

## Export

| | Desktop | Android | CLI |
|---|---|---|---|
| Every note and its files, the notes as JSON | Settings → Export | Settings → Export | `dirt export --folder <folder>` |
| The same, the notes as Markdown | Settings → Export | Settings → Export | `dirt export --folder <folder> --format markdown` |
| What is written | A new folder, `dirt-export-<time>`, inside the folder chosen in a dialog | One zip file holding what the folder would, saved through Android's screen for choosing a place | The folder named, which must not be there yet |
| A file this device does not hold | Fetched first | Fetched first | Fetched first |
| A file that cannot be had | Left out. Listed under Export, with the reason | Left out. Listed under Export, with the reason | Left out and named with the reason, and the command fails |
| The notes' text alone, naming their files | No | No | `dirt export`, to standard output or `--output <file>` |

An export is `notes.json` or `notes.md` and, beside it, `files/<note
id>/<file name>` for each file. The text lists each note's files with the
path of each, or no path for a file the export does not hold. It is made
by `dirt_core::export`, the same for the three.

Archived notes are exported, after the notes of the list, and marked:
`"archived": true` in the JSON, and a line `archived: true` in the block at
the head of the note in the Markdown.

## Appearance

| | Desktop | Android | CLI |
|---|---|---|---|
| Light and dark themes | Settings → Appearance: Auto, which follows the system, Light or Dark; and the six accents | Settings → Appearance: Auto, which follows the system as it changes, Light or Dark; and the six accents | Not applicable |
| The window | Windows: no title bar. Dirt's own minimise, maximise and close at the top right; the top row moves the window and a double-click on it maximises; every edge resizes it. Linux and macOS: the system's title bar, until they are tried (manual checklist). In a window wider than 1112 px, maximised among them, the open note's column is in the middle of the room beside the list, its icons beside its first line | Not applicable | Not applicable |
| Text size | Settings → Appearance, Ctrl + and Ctrl − (13 to 20 px, 15 at first). Inter and JetBrains Mono, bundled; the choice of font is gone | Settings → Appearance, the same sizes; a note's text is drawn a pixel larger than the size chosen, as design 4.2 has it. The same two fonts | Not applicable |
| Shell completions | Not applicable | Not applicable | `dirt completions <shell>` |

## Not built

| What | Where it stands |
|---|---|
| `dirt tui` | A stub that prints "Opening TUI..." and does nothing. No design document mentions a terminal interface. Needs a decision: remove it or plan it |
| iOS and web clients | Outside the design (section 3.3) |
