# Dirt design system

The look, feel and behaviour of Dirt's desktop app and Android app. It
replaces the current interface. Nothing from the current interface is kept
just because it already exists.

- **Interactive mockup:** [mockup.html](mockup.html). Open it in a browser.
  It shows both apps side by side, working: write on a blank page, say a
  thought, find, tag, edit, archive, delete and undo, attach (drop real files on the
  desktop window), sync, quick capture, letting a device in, settings and
  export. **Simulate** in the top bar plays the events a person can't cause
  alone (a new device asking to join, going offline, a note the server
  refuses, talking from another app, the phone's side key, a year of
  thoughts). Voice is simulated unless **Use my microphone** is chosen there
  and the browser can turn speech into text. **Tasks** lists every task
  with its action count and ticks each one off as you do it.
- **Product rules this follows:** [../DESIGN.md](../DESIGN.md). Capture
  velocity, under two seconds from thought to saved note, is still the
  metric that matters most.
- **What each client must be able to do:**
  [../feature-parity.md](../feature-parity.md). Section 3 maps every row of
  it to a place in this design.
- **Where the mockup and this document disagree:** the mockup holds for
  how long a movement takes and on which curve, because that is what was
  approved by eye; section 5's numbers are corrected to it when the tokens
  are built. This document holds for everything else: what a key or a
  gesture does, sizes, words, and what is where.
- **How it is built, and in what order:**
  [../plans/2026-11-16-interface-redesign-implementation-plan.md](../plans/2026-11-16-interface-redesign-implementation-plan.md).

---

## The idea: grains of sand

積沙成塔: grains of sand, gathered, become a tower. Dirt is for the
thoughts that pass in a second, and one alone is worth little: a name, half
an idea, a thing to buy. Kept, they pile up into something worth a great
deal. So every note is a **grain**, and everything a person has written is
their **pile**. The design carries the idea in what things do, quietly,
never as a game:

- **A new thought settles onto the pile.** Done with it, its words gather
  into a grain that drops onto the list (5.5). Nothing says "Saved".
- **The list is the pile seen from the side.** The newest grains are on the
  surface at the top; older layers lie below, each layer's edge a faint
  line of grains with its name ("This week", "October"). Scrolling down is
  digging (6.17).
- **At the bottom is the pile itself:** a heap of real sand, one tiny
  grain for every thought, piled the way sand piles, with how many there
  are and since when. Ten thoughts are a pinch; a year's are a heap. On
  Android the same pile can sit on the home screen and gains a grain each
  time (8.5).
- **Each note has its grain,** a shade of sand taken from its ID. It is the
  note's dot on the Rail (7.7) and its place in the pile.
- **No streaks, no goals, no numbers pushed at the person.** A pile is not
  a target. It is there when looked for and says nothing otherwise
  (principle 5). Archiving a note takes it out of the list but leaves its
  grain in the pile; only deleting takes the grain away (6.19).

---

## 1. Principles

Six rules. When two of them pull in different directions, the earlier one
wins.

1. **Capture comes first.** A blank page is always ready (on the desktop
   with the cursor already in it), and a thought can be said instead of
   typed. Nothing is in the way: no welcome screen, no "new note" step, no
   title field, no Save, no sign-in wall.
2. **A thought gets a page, not a box.** Writing happens on a page that
   grows with the thought. Enter is a new line; no key saves, because the
   page saves itself from the first word. Done with it (Esc, or ✓), it
   settles onto the pile and a blank page is ready for the next. Finding
   has a field of its own, one key away.
3. **Act on the thing.** You archive a note by swiping it or hovering it,
   not by opening a menu. You attach a file by dropping it on the window.
   You sync by pulling the list down. A button exists only where there is
   no gesture for it, and every button is an icon with a tooltip.
4. **Only the note you're in.** When a note is open, what is on screen is
   that note. The list and the window's controls go while you write. A
   control comes back only when the pointer goes to where it lives (the
   note's icons beside its column, the window's in the top corner, the list
   at the left edge), never just because the mouse moved. Nothing on screen
   talks about anything else.
5. **As few words as possible.** Words appear only when something needs
   the person: a note that can't sync, a device asking to join, a note just
   deleted. Everything that always works says nothing (saving, syncing),
   and nothing says what is already shown elsewhere (a note's edit time is
   on its row, not over its text). Lists that can grow, such as tags, are
   never laid out in full; they are asked for, and show a few at a time.
6. **Nothing teleports.** In the real world nothing moves instantly. Every
   change of state moves, briefly and with purpose, so the eye can follow
   where a thing came from and where it went (section 5).

### What this rules out

- Confirmation dialogs for anything that can be undone. Archiving or
  deleting a note shows **Undo** instead. Dialogs are kept for what can't be undone:
  removing a device, changing the key, starting the account again, deleting
  the account.
- Save buttons. Every note saves itself as it is typed, on both platforms.
- Labels on icon buttons, and explanations on screen. A tooltip (desktop)
  or a long press (Android) says what an icon does, with its shortcut.
- Separate screens for search, tags, sync status or the note list. They are
  all one screen.
- A row of every tag. Tags are asked for, from Find (6.3).
- A one-line box that makes a note on Enter. Thoughts are longer than a
  line, and a key that saves at once makes the person hurry.
- Controls that come back whenever the mouse moves, or that sit in the
  window's corner, away from the note they act on.
- "Saved", "Synced", "Edited 4 minutes ago", key hints on screen, and
  messages under a thought being written such as "No notes match".

---

## 2. Action budget

An action is one click, tap, key chord, gesture, or a run of typing. Fewer is
better; each of these is the fewest the design can manage without guessing
for the person.

| Task | Desktop | Android |
|---|---|---|
| Write a thought (app open) | **1**: type. The blank page is already open, and saves itself. <kbd>Esc</kbd> puts the thought on the pile and gives the next page | **2**: tap the bar, type. It saves itself |
| Say a thought (app open) | **1**: hold the mic on the blank page, speak, let go | **1**: hold the mic, speak, let go |
| Write a thought (app closed or in another app) | **3**: <kbd>Ctrl Alt N</kbd>, type, <kbd>Esc</kbd> | **3**: widget, type, ✓ |
| Say a thought (app closed or in another app) | **1**: hold <kbd>Ctrl Alt N</kbd>, speak, let go | **2**: the widget's mic, speak, ✓ |
| Write a thought on a locked phone | n/a | **4**: pull down, the tile, type, ✓. Or **1** where the phone lets a key open Dirt: double-press the side key and speak; a pause keeps it (8.6) |
| Share text or a picture from another app | n/a | **2**: Share, Dirt. Saved at once |
| Find a note | **2**: <kbd>Ctrl F</kbd> (or click Find), type | **2**: 🔍, type |
| Narrow to a tag | **1**: click the tag on any note. Or the `#` on Find, the tag | **1**: tap the tag on any card. Or 🔍, `#`, the tag |
| Narrow to two tags at once | **3**: the `#`, one tag, the other. The list stays open while you pick | **4**: 🔍, `#`, one tag, the other |
| Write a note with a tag already chosen | **1**: with the list narrowed to it, a new page gets the tag | **2** |
| Open and edit a note | **2**: click it, type. It saves itself | **2**: tap it, type |
| Archive a note | **1**: the box on hover, or <kbd>Del</kbd> | **1**: swipe left |
| Delete a note for good | **1**: <kbd>Shift Del</kbd>. On an archived note, the bin or <kbd>Del</kbd> | **2**: on an archived note (Find, or "archived" under the pile), swipe left or the bin |
| Undo an archive or a delete | **1**: **Undo** or <kbd>Ctrl Z</kbd> | **1**: **Undo** |
| Attach a file | **1**: drop it on the window | **2**: paperclip, choose |
| Look at a file | **1**: click it, or <kbd>Space</kbd>. <kbd>←</kbd> <kbd>→</kbd> for the next | **1**: tap it. Swipe for the next |
| Open, save or remove a file | **1**: the icon on hover, or in the preview | **1** |
| Sync now | **1**: click the sync status, or <kbd>Ctrl R</kbd> | **1**: pull the list down |
| See why a note won't sync | **1**: click the warning | **1**: tap the warning |
| Let a new device in | **2**: **Review**, **They match** | **2** |
| Sign in | **2**: Settings, **Sign in** (then the browser) | **2** |
| Change the theme or the accent | **2**: Settings, the theme or a colour | **2** |
| Export everything | **2**: Settings, **Export** (then the folder) | **2** (then where to save) |

How the counts were brought down, compared with the current apps:

- The list, Find and tags are on the one screen, so finding needs no
  navigation.
- The desktop opens with the cursor on a blank page, so the first action
  is the thought itself. Voice skips the keyboard altogether.
- Finding costs one action more than it did when one field wrote and found
  (<kbd>Ctrl F</kbd>, or 🔍). That trade is made on purpose: a thought is
  written far more often than one is looked for, and a page is better to
  write on than a one-line box (principle 2).
- Picking several tags used to close the list after each, so two tags
  cost four actions. The list now stays open, and a tag on any note
  narrows to it in one.
- The everyday act of clearing a note out of the way is archiving, which
  loses nothing, so it keeps the one-action gesture. Deleting for good is
  rarer and final, so it sits one step deeper (6.19).
- A device asking to join shows a banner on every device already in. The
  person does not have to go to Settings → Devices to find it.
- Android's Save button and its separate "sync line above the list" are
  gone. The editor saves as you type, and pulling down syncs.
- Settings is one scrolling page. Nothing in it is more than one level
  deep, so every setting is two actions away.

---

## 3. Information architecture

### Desktop: one window

```
┌─────────────────────────────────────────────────────────────────┐
│ ┌─ 🔍 Find                          # ─┐                │ — ▢ ✕ │
│ └──────────────────────────────────────┘                          │
│   Ask Jordan about the Q3 offsite…  4m    What's on your mind?  🎙 │
│   This week · · · · · · · · · · · · ·                          📎 │
│   Sourdough: 78% hydration…        50m                            │
│   …                                                               │
│               ⋰⋱  (the pile)                                      │
│  ☁                                 ⚙                              │
└─────────────────────────────────────────────────────────────────┘
       the list (344 px)                  the page
```

- **Left:** Find (with its `#` for tags), the list in its layers with the
  pile at the end, and a footer with the sync icon and Settings. While a
  note is being written the list goes (7.7).
- **Right:** **the page.** With no note open it is blank, with the cursor
  in it, its placeholder and two icons in its gutter (🎙, 📎). An open note
  is the same page, with ✓, 📎 and the archive box in the gutter (6.1,
  6.4). An archived note has "back to the list" and 🗑 instead (6.19).
- **Narrower than 720 px:** the list takes the whole window and a note
  opens over it, as on Android.
- **Over everything:** Settings (a panel), quick capture (its own small
  window, over any app), toasts and banners.

### Android: one screen and two layers

```
┌──────────────────────┐   ┌──────────────────────┐   ┌──────────────────────┐
│ Notes       🔍  ☁  ⚙ │   │ ⌄          🎙  📎  🗑 │   │ Notes       🔍  ☁  ⚙ │
│ ┌──────────────────┐ │   │                      │   │ ┌──────────────────┐ │
│ │ Ask Jordan…      │ │   │ What's on your mind? │   │ │ Ask Jordan…      │ │
│ │ 4m         #work │ │   │                      │   ╭──────────────────────╮
│ └──────────────────┘ │   │                      │   │                      │
│ This week · · · · ·  │   │                      │   │                      │
│ ┌──────────────────┐ │   │                      │   │ ·˙·.˙·˙.·˙·.     🔒  │
│ │ Sourdough…       │ │   ├──────────────────────┤   │                      │
│ └──────────────────┘ │   │       keyboard       │   │ ‹ Cancel  0:03  (🎙) │
│ ( What's on your… )🎙 │   │                      │   │                      │
└──────────────────────┘   └──────────────────────┘   └──────────────────────┘
      the list                 a new page (grows        listening (hold the
                               out of the bar)          mic: a sheet rises)
```

- **The dock** is at the bottom, where the thumb is: the bar "What's on
  your mind?", which grows into a full-screen page, and the clay mic, which
  listens while it is held.
- **Find** is behind 🔍 at the top. It slides down under the title, with
  `#` for tags, and the list narrows under it.
- A note opens by growing out of its card and closes by shrinking back into
  it. A new page grows out of the bar and, done, drops onto the list as a
  grain (5.5).
- Settings is a sheet from the bottom. Drag it down, tap outside it, or use
  Back to close.

### Where every feature lives

| Feature (from feature-parity.md) | Desktop | Android |
|---|---|---|
| Write a note | The page, always open; it saves itself. <kbd>Esc</kbd> or ✓ when done | The bar at the bottom opens a page; it saves itself |
| Say a note | Hold 🎙 on the blank page; hold <kbd>Ctrl Alt N</kbd> from any app | Hold the mic in the dock; the widget's mic; a hardware key where the phone allows it (6.18) |
| Edit a note | The open note, saved as you type | The same. No Save button |
| Archive a note | The box on hover, <kbd>Del</kbd> in the list, the box in the note | Swipe left; the box in the note |
| Archived notes | Found by Find, under their own line after the rest; all of them from "archived" under the pile | The same |
| Delete a note for good | <kbd>Shift Del</kbd>; on an archived note, the bin or <kbd>Del</kbd> | On an archived note, swipe left or the bin |
| Bring a note back | **Undo** on the toast, <kbd>Ctrl Z</kbd>; on an archived note, "back to the list" | The same, without the key |
| List notes, show older | The list loads more as it nears the end. No button | The same |
| Search | Find, at the top of the list: <kbd>Ctrl F</kbd>, <kbd>Ctrl K</kbd> or <kbd>/</kbd> | 🔍 at the top |
| Narrow to a tag, tag counts | `#` in Find: the six most used tags, with counts, narrowing as you type. Chosen tags are tokens in Find | `#` in Find: a strip of tags under it |
| Quick capture | <kbd>Ctrl Alt N</kbd>; New Note in the tray menu | The capture sheet, from the home-screen widget, long-press the icon → New note, the Quick Settings tile, or the lock screen where the phone allows it (8.5, 8.6) |
| Take text or pictures from another app | Drop on the window | Share → Dirt, saved at once with a toast |
| Run in the background | Tray icon | n/a |
| Sign in, sign out, move local notes in | Settings → Account | The same |
| Let a device in, compare the code | Banner on every device → Review; or Settings → Devices → Add a device | The same |
| List and remove devices | Settings → Devices | The same |
| Recovery phrase: make, remove, get in with it | Settings → Devices | The same |
| Change the key, start again, refuse a reset, delete the account | Settings → Account → Account actions | The same |
| Sync without being asked | On start, on return, after a change. Never on a timer | The same |
| Sync now | The sync status, <kbd>Ctrl R</kbd> | Pull the list down; the sync icon |
| Sync status, waiting notes and files, problems | The sync status (icon + word), tooltip, Settings → Sync | The sync icon (with a dot), Settings → Sync |
| Open a note that can't sync | Settings → Sync → the note | The same |
| Attach a file | Drop it; paperclip; <kbd>Ctrl Shift A</kbd>; paste a picture | Paperclip on a page or in a note; Share |
| Look at a file | The preview, inside the app (6.16) | The same |
| Open in another app, save a copy, remove a file | Icons on the file, on hover, and in the preview | Icons on the file, and in the preview |
| Thumbnails | In the file, and on the note's card | The same |
| Export JSON or Markdown | Settings → Export | The same (a zip) |
| Theme, accent, text size | Settings → Appearance; <kbd>Ctrl +</kbd> / <kbd>Ctrl −</kbd> for size | Settings → Appearance (new on Android), with "Wallpaper" as one more accent on Android 12 and later |

---

## 4. Foundations

All values are CSS custom properties on `:root`. Both apps render in a
webview, so one style sheet serves both (section 10). The names below are
the ones the mockup uses.

### 4.1 Colour

A warm, paper-like neutral with one accent, **clay**: the colour of soil, for
an app called Dirt. Clay is used sparingly: the caret's field, the selected
row's bar, the mic, tags, and the one primary button in a view.
Everything else is neutral.

| Token | Light | Dark | Used for |
|---|---|---|---|
| `--bg` | `#FAF8F5` | `#161412` | Window and screen background, the list |
| `--surface` | `#FFFFFF` | `#1E1B18` | The open note, cards, panels, fields |
| `--surface-2` | `#F3F0EB` | `#27231F` | Hover, pressed, quiet buttons |
| `--surface-3` | `#EAE5DE` | `#302B26` | Pressed on `--surface-2` |
| `--border` | `#E7E2DA` | `#332E29` | Hairlines, card edges |
| `--border-strong` | `#D3CBC0` | `#463F38` | Sheet grabber |
| `--border-control` | `#8F877D` | `#776B5F` | The outline of an unchecked checkbox, switch or radio |
| `--text` | `#1F1B16` | `#EDE8E1` | Body text, titles |
| `--text-2` | `#665F56` | `#A9A096` | Note previews, icons |
| `--text-3` | `#736B62` | `#938B81` | Times, layer names, hints, placeholders |
| `--accent` | `#AC4A26` | `#E2774F` | Clay. Tags, primary button, focus |
| `--accent-hover` | `#933F20` | `#EC8B66` | Primary button hover |
| `--accent-soft` | `#F8E6DD` | `#3A2419` | Focus halo, a new note's flash |
| `--on-accent` | `#FFFFFF` | `#1A0F0A` | Text and icons on clay |
| `--success` | `#2F7D4F` | `#5DBB84` | Synced tick, a device let in |
| `--warning` | `#9A5D10` | `#E0A045` | A note that can't sync |
| `--danger` | `#BE3A2B` | `#E86A5A` | Delete, remove, swipe background |
| `--danger-soft` | `#F9E3E0` | `#3A1E1A` | Delete hover, confirmations |
| `--on-danger` | `#FFFFFF` | `#1A0F0A` | Text on the red fill of a final confirm |
| `--mark` | `#F7DDB0` | `#5A3F1C` | Search matches |
| `--inverse` | `#2A2520` | `#EDE8E1` | Toasts and tooltips |
| `--on-inverse` | `#F5F1EB` | `#1E1B18` | Text on them |
| `--accent-inverse` | `#F29A76` | `#B2471F` | **Undo** on a toast |

Contrast: `--text` on `--bg` is over 15:1 in both themes. Every colour
that carries text is checked on every background it is drawn on, not only
on white: `--bg` (the list), `--surface`, `--surface-2` (a hovered or
selected row) and, for tags, `--accent-soft` (a tag token). Each pair
passes WCAG AA for normal text, 4.5:1, in both themes:

| Text | Lowest pair, light | Lowest pair, dark |
|---|---|---|
| `--text-3`: times, layer names, placeholders | 4.6 on `--surface-2` | 4.6 on `--surface-2` |
| `--accent`: tags, links | 4.6 on `--accent-soft` | 4.8 on `--accent-soft` |
| `--on-accent` on `--accent` | 5.6 | 6.3 |
| `--warning`: "1 note can't sync" | 4.7 on `--surface-2` | 6.9 on `--surface-2` |
| `--danger`: **Remove**, **Delete the account** | 4.8 on `--surface-2` | 4.9 on `--surface-2` |
| `--on-danger` on `--danger` | 5.5 | 5.9 |
| `--text` on `--mark`: a search match | 13.0 | 8.0 |

Controls that are not text need 3:1: an unchecked checkbox or switch is
outlined in `--border-control` (3.1 on `--surface-2`, light), not in a
hairline colour. `--border` and `--border-strong` only separate things that
are already told apart another way. An icon is a control too, and needs
the same 3:1: the bin under the pointer is `--danger` on `--danger-soft`,
which is over 4.4 in both themes. That pair is not used for words.

Two pairs were found when the tokens were built. The red fill of a final
confirm had white text in both themes, and white on the dark theme's
lighter red is 3.2; `--on-danger` is dark there, as `--on-accent` is. A
search match kept the colour of the text around it, and the preview's
`--text-2` on the dark theme's `--mark` is 3.8; a match is drawn in
`--text` whatever is around it.

A test in `dirt-ui` computes every pair named here and in 4.1.1 from the
values the style sheet is made from, and fails under these ratios. It also
holds the table above to its own numbers, so a colour cannot change
without the table changing with it.

A first version checked clay on white alone and passed; on the list's own
background it was 4.3, on a tag token 3.8, and `--text-3` was 2.9. Those
colours are a shade darker now. `--text-3` is close to `--text-2` as a
result, so the two are told apart by size as much as by colour: times and
layer names are 11–12 px.

**Theme:** Auto (follows the system), Light or Dark, on both platforms.
Switching cross-fades every colour over 320 ms instead of snapping.

Auto is the style sheet's own: with no theme chosen, the dark colours are
under `prefers-color-scheme: dark`. WebView2 answers that from the
system's setting. Android's WebView answers it from the theme of the
activity it is in, so the activity's theme is AppCompat's DayNight, which
follows the system, and `uiMode` is among the changes the activity takes
itself: without that, Android restarts the activity when the system's
theme changes, and the app ends. The bars above and below the page are
the activity's; the page tells it whether it is dark and its `--bg`
whenever its look changes, and they take that colour (8.7).

#### 4.1.1 Accent

The person picks the accent in Settings → Appearance. **Clay is the
default**, and the values in the table above are Clay's. Every other accent
replaces the same five tokens and nothing else: the neutrals, `--mark`,
`--success`, `--warning` and `--danger` stay as they are, so a warning
still looks like a warning and delete still looks like delete whatever the
accent.

| Accent | Light: `--accent` · hover · soft | Dark: `--accent` · hover · soft | Feel |
|---|---|---|---|
| **Clay** (default) | `#AC4A26` · `#933F20` · `#F8E6DD` | `#E2774F` · `#EC8B66` · `#3A2419` | Soil, warm |
| Moss | `#3C754B` · `#336340` · `#E3EFE5` | `#6FBF85` · `#85CC98` · `#1E3324` | Calm, green |
| Lake | `#2E6AB0` · `#285B98` · `#E2ECF7` | `#6FA8E8` · `#88B9EE` · `#1A2A3D` | Cool, blue |
| Plum | `#8A4A9E` · `#743C86` · `#F1E5F4` | `#C792D8` · `#D3A6E1` · `#33233A` | Soft, violet |
| Honey | `#8D610D` · `#78530B` · `#F6EBD3` | `#E0B14F` · `#E8C06A` · `#3A2E14` | Bright, amber |
| Graphite | `#3D3832` · `#2A2622` · `#ECE8E2` | `#D9D2C8` · `#E8E2D9` · `#2E2A26` | No colour at all |

| Accent | `--on-accent` light / dark | `--accent-inverse` light / dark |
|---|---|---|
| Clay | `#FFFFFF` / `#1A0F0A` | `#F29A76` / `#B2471F` |
| Moss | `#FFFFFF` / `#0E1A12` | `#8FD0A0` / `#2F6B40` |
| Lake | `#FFFFFF` / `#0B1626` | `#8DBDF0` / `#285E9E` |
| Plum | `#FFFFFF` / `#1C1020` | `#D6A8E4` / `#7A3F8C` |
| Honey | `#FFFFFF` / `#1C1505` | `#EAC47A` / `#8A5E0C` |
| Graphite | `#FFFFFF` / `#1A1714` | `#E2DCD3` / `#3D3832` |

`--focus` is the accent at 45% (light) or 55% (dark).

Every pair was checked against WCAG AA for normal text (4.5:1), in both
themes: the accent on `--bg`, `--surface`, `--surface-2` and its own
`--accent-soft` (a tag token, the lowest), `--on-accent` on the accent, and
`--accent-inverse` (Undo) on the toast. The lowest are 4.5–4.6:1; most are
between 5 and 9. A new accent is added only with the same twelve checks
passing.

- **What follows the accent:** the logo's square, Find's focus ring,
  the selected row's bar, tags, tokens, the primary button, the mic
  button, the slider, links, the widget's mic and the capture sheet's
  tick. The Android widget and capture sheet use the same accent as the
  app.
- **Android 12 and later** add a seventh choice, **Wallpaper**, which takes
  the accent from the system's own colours (Material You). It goes through
  the same contrast check and falls back to Clay if a pair fails.
- **Per device.** The accent is kept on the device, like the theme and the
  text size. It is not synced: a phone and a laptop may want different
  ones, and nothing about the account depends on it.
- **The control** is a row of colour dots (6.11). Picking one cross-fades
  every accent colour over 320 ms.

### 4.2 Type

| Role | Font | Size / line | Weight |
|---|---|---|---|
| Screen title (Android "Notes", "Settings") | Inter | 27 / 32 | 700, −0.025em |
| Note text in the editor | Inter | 15 / 1.65 (desktop), 16 / 1.6 (Android). Text size setting: 13–20 | 400 |
| Find, and the bar "What's on your mind?" | Inter | 15 / 22 (desktop), 16 / 22 (Android) | 400 |
| Quick capture | Inter | 17 / 24 | 400 |
| Note title in the list (its first line) | Inter | 14 desktop, 15 Android | 560 |
| Note preview (the rest) | Inter | 13 / 13.5, two or three lines | 400 |
| Interface text, buttons | Inter | 13.5 | 500–550 |
| Hints, times, captions | Inter | 12–12.5 | 400–500 |
| Section labels | Inter | 11.5, uppercase, +0.07em | 600 |
| Shortcuts, recovery words | JetBrains Mono | 10.5–12 | 500 |

Notes are plain text. The first line of a note is its title in the list, and
nowhere is it styled differently while being written. Tags (`#word`) show in
clay in the list.

Inter and JetBrains Mono are bundled with the apps, so nothing is fetched at
run time. Fall back to the system UI font.

### 4.3 Space, size, shape

- **Spacing** steps of 4: 4, 8, 12, 16, 20, 24, 32, 44. The editor has 44 px
  of side margin and a 680 px line length cap; Android uses 20.
- **Hit targets:** at least 32 × 32 px on desktop and 40 × 40 dp on Android
  (icon buttons are 40; the dock's bar and mic are 52 tall, and Find's
  buttons 36 inside a 46 tall field). Swipe and pull areas are whole cards and the whole list.
- **Radius:** `--r-s` 6 (small controls), 8 (buttons), `--r-m` 10 (rows,
  cards on desktop, fields), `--r-l` 14 (panels, banners), 16 (Android
  cards), 24 (sheets), 26 (the dock's bar), 999 (tags, the mic).
- **Elevation:** three shadows. `--shadow-1` for cards and the selected row,
  `--shadow-2` for the dock and toasts, `--shadow-3` for panels,
  sheets, banners and quick capture. Dark mode uses deeper, blacker shadows.

### 4.4 Icons

- Line icons on a 24 px grid, 1.75 px stroke, round caps and joins
  (Lucide's style). Drawn at 18 px; 15 px in small buttons; 22 px for
  empty states. They are Lucide's drawings, some simplified, under its ISC
  licence, drawn inline so that they take the colour of the text around
  them; the licence is in `crates/dirt-ui/assets`, with the fonts'.
- One icon, one meaning, everywhere:

| Icon | Meaning |
|---|---|
| magnifier | Find |
| mic | Talk: hold to speak and let go, or tap for hands-free (6.18). On a card, "said, not typed" |
| check ✓ | Done: put this thought on the pile; while listening, keep what was said |
| chevron down ⌄ | Done with a new page on Android |
| hash `#` | Tags: shows the few most used, to narrow to or to add |
| box | Archive: out of the list, still in the pile and in Find |
| box with an arrow up | Back to the list, on an archived note |
| bin | Delete for good |
| paperclip | Attach; on a card, "has files" |
| copy | Copy the note's text |
| cloud with tick | Synced |
| turning arrows | Syncing; "Sync now" |
| cloud struck through | Offline |
| triangle | Something needs you |
| laptop / phone | A device; "this device only" when signed out |
| key | Recovery phrase, account key |
| lock | Encrypted |
| gear | Settings |
| hollow dot (7 px) | This note has not reached the server yet |

### 4.5 Tooltips and long presses

Tooltips replace on-screen labels and explanations.

- Every icon button has one: what it does, in two or three words, and its
  shortcut in `kbd` chips. Example: **Delete** <kbd>Del</kbd>.
- Times, the sync status, file icons and the not-synced dot have one that
  says more: "Edited Nov 13, 09:12", "Last synced 2 minutes ago", "Not on
  the server yet".
- Appear after 480 ms of hover, at once if another tooltip closed less than
  500 ms ago (so you can sweep along a toolbar). Fade in over 120 ms while
  rising 3 px. Hide on press, scroll or leave.
- Above the element; below it when there's no room. Never wider than 280 px.
- On Android, a long press on an icon shows the same text, as Android's own
  tooltips do: when Android itself takes the press for a long one, and for
  1.5 s. The press that showed it does not also press the button.
- Keyboard focus shows it as a hover does, since a shortcut is written
  nowhere else.

---

## 5. Motion

### 5.1 Why

Motion explains change. In the real world nothing moves instantly, and
nothing appears from nowhere: a thing comes from somewhere and goes
somewhere. Dirt's motion says where. A new thought gathers into a grain
and drops onto the pile; Settings grows out of the gear; quick capture flies to
the tray; a deleted note tears away and the list closes over it. Motion
never delays the person: input is accepted during every animation, and
every animation can be interrupted and reversed from where it is.

### 5.2 Choreography: seven rules

1. **From the cause, to the destination.** An element enters from what
   caused it (the button pressed, the field typed in, the point a file was
   dropped) and leaves towards where it goes (the tray, the list, the bin).
   Nothing rises from "the bottom" unless it lives there, as a sheet or a
   toast does.
2. **Direction means something.** Down the list is down on screen: opening
   a note further down brings its text up from below, one further up brings
   it down from above. Leaving is left; going deeper grows outward.
3. **Springs for movement, curves for everything else.** Position, size and
   scale move on springs (5.3), which feel physical and can be redirected
   mid-way. Opacity, colour and blur use plain curves (5.4).
4. **A bounce budget.** Only two kinds of thing bounce: something the
   person lets go of (a card released, a token dropped in) and a
   confirmation (send, a tick, undo). Large surfaces, text and anything
   that leaves never bounce.
5. **One leads, the rest follow.** The element the person acted on moves
   first; what it disturbs follows 40–150 ms later (follow-through). Groups
   arrive in a stagger of 18–45 ms per item, six items at most before the
   rest arrive together.
6. **Exits are shorter and plainer.** About 60% of the entrance, on the
   exit curve, with no spring: what leaves no longer matters.
7. **Interruptible.** Every animation is a transition from where things
   are, never a canned sequence from the start, so a second press reverses
   the first from its current position.

### 5.3 Springs

Three springs, each given by its **response** (how quickly it moves, the
period of the undamped spring) and its **damping** (1 is no bounce; lower
bounces more). They were simulated and written as CSS `linear()` easings,
so CSS transitions and the Web Animations API use the same curves. The
mockup's **Springs** panel plots each one and races a ball along it.

| Spring | Response | Damping | Reaches 90% | Settles | Overshoot | For |
|---|---|---|---|---|---|---|
| **snap** | 0.3 s | 0.78 | 142 ms | 380 ms | 2.0% | Small controls moving: segment thumbs, switches, selection, tokens, popovers |
| **pop** | 0.38 s | 0.58 | 150 ms | 600 ms | 10.7% | Confirmations and things let go of: send, ticks, a card released, undo |
| **glide** | 0.55 s | 1.0 | 344 ms | 750 ms | 0.0% | Large surfaces: sheets, panels, the list folding, the note column |

Snap and pop arrive almost equally fast; pop then swings 11% past and back,
which is the "bounce". Glide takes twice as long to get there and never
overshoots, which is what makes a large surface feel heavy rather than
springy.

Tokens: `--sp-snap`, `--sp-pop`, `--sp-glide` (the easing) and `--d-snap`,
`--d-pop`, `--d-glide` (the duration it takes to settle). `--e-spring`, used
by the smaller components in section 6, is the pop spring.

"Reaches 90%" is the first of a spring's 48 steps at or past nine tenths of
the way.

A spring's curve is sometimes run over a shorter time than it takes to
settle, where the thing moving is small: the pop curve over 280 ms
(`--d-3`) for a tag token and the ring of an accent dot, over 400 ms
(`--d-4`) for the seven pictures and a tick; the glide curve over 560 ms
for a new page on Android. The catalogue says where.

### 5.4 Curves

| Curve | Value | Duration | For |
|---|---|---|---|
| **standard** | `cubic-bezier(.2,0,0,1)` | 120–280 ms | Fades and colour changes in place: hover, the dim behind a panel, the theme's colours |
| **enter** | `cubic-bezier(.05,.7,.1,1)` | 200–400 ms | Fades that travel a little: a toast's first rise, a tooltip |
| **exit** | `cubic-bezier(.3,0,.8,.15)` | 140–300 ms | Everything that leaves: slow to start, quick to go |
| **toss** | `cubic-bezier(.25,.7,.5,1)` | the first 42% of a flight | A grain thrown up: quick off the hand, slowing to the top of its arc |
| **gravity** | `cubic-bezier(.55,0,.9,.45)` | the rest of the flight | Falling: slow at the top, fastest as it lands |

The curves are tokens: `--e-std` (standard), `--e-out` (enter: an
ease-out, so it arrives softly), `--e-in` (exit: an ease-in, so it leaves
quickly), with toss and gravity in the one script that throws grains.
Their durations are tokens too: `--d-1` 120 ms, `--d-2` 200 ms, `--d-3`
280 ms, `--d-4` 400 ms.

Two things that gather use curves of their own, close to gravity: the
words of a settled thought shrink on `cubic-bezier(.5,0,.75,.3)`, and an
Android page closes to its grain on `cubic-bezier(.5,0,.75,.2)`.

Hover and press feedback stays at 120 ms: the press itself (a button
shrinking a little) on the standard curve, the change of colour under the
pointer on the web view's own `ease`. Durations mentioned elsewhere in
this document are approximate; the tokens here are the real ones.

### 5.5 Catalogue

**Writing and the list**

| Moment | Motion |
|---|---|
| The first word on the blank page | Nothing jumps: the page simply becomes a note, and the list starts to leave (focus, 7.7) |
| A new thought is let go (<kbd>Esc</kbd>, ✓) | **Settling**, in four beats. The words shrink toward where they began and fade (280 ms, on their own curve, 5.4), and the note's grain, in its own shade, pops out there (150 ms). The grain is tossed: up 46 px on the toss curve, then down onto the top of the list on gravity, moving across on a curve all the while (480–640 ms by distance). Where it lands the row appears (snap, from 96%), the rows below give 3 px and come back one after another, 24 ms apart, and seven specks of dust puff out and fall. The blank page is ready from the first beat; only its placeholder fades back in. A sync arriving mid-flight does not show the row early |
| A new page is let go on Android (⌄, Back) | The page closes to a 7 px circle where its words began (300 ms, on its own curve, 5.4); the grain is tossed from there onto the top card, as above |
| An old note is let go | No grain: the list comes back and the note is in its place |
| A note arrives from another device | It settles into place from 8 px above (snap) and its row flashes clay-soft for 1.6 s |
| The list narrows or reorders | Rows that stay slide to their new places (snap); new ones settle in from 8 px above (snap), staggered 24 ms, the seventh and later together |
| Selecting a note | One highlight, with its clay bar, slides from the old row to the new one, stretching to the new row's height (snap). Nothing fades |
| A note opens in the editor | Its text and then its files come from the direction of the list: from below if the note is further down, from above if further up, 18 px (snap, the files 50 ms behind) |
| A note is archived | It sinks into the pile: its grain appears at the row's left edge and falls on gravity's curve, fading (720 ms), while the row folds shut and the rows below rise into its place (snap) |
| A note is deleted | The row tears away to the left: 28 px, shrinking to 96%, fading (170 ms, exit). The rows below rise into the gap on snap, starting 110 ms after it began to tear |
| Undo | The row drops back into its place from 90% with a bounce (pop) |
| A tag token appears | It pops into the field from 70% (the pop curve over 280 ms) |
| A tag token is removed | It shrinks to nothing sideways while fading (200 ms, exit), and the field closes up |
| The tag list opens | It opens in place under Find (glide), pushing the notes down rather than covering them; its rows arrive top to bottom, 26 ms apart. On Android the pills pop in left to right (pop) |
| A tag is picked | Its tick pops in (pop) and the row stays where it is; the notes below narrow (snap) |
| The pile grows | As a new thought's grain lands on its row, and if the heap is in sight, the newest grain drops 46 px onto the top of the heap (640 ms, `cubic-bezier(.55,0,1,.45)`). Nothing else moves |

**Panels, sheets, toasts**

| Moment | Motion |
|---|---|
| Settings opens (desktop) | It grows out of the gear: from the gear's centre and 6% of its size, corners from round to 14 px (480 ms, snap). The rail's entries slide in from the left, 30 ms apart; the sections rise 14 px, 40 ms apart |
| Settings closes | It shrinks back into the gear (240 ms, exit) |
| Settings opens (Android) | The sheet rises (glide) while the screen behind steps back: 93%, rounded corners, a little darker. The sections rise 28 px in turn, 45 ms apart |
| A toast | 560 ms in all: a 44 px pill rises 16 px (enter, the first third), then opens sideways into the message (snap); the words fade in after it, from 230 ms, 50 ms apart (220 ms each, enter). Going, it closes back to a pill and drops away (300 ms, exit). A new toast opens while the old one closes, so one seems to turn into the other |
| A device asks to join | The banner drops from the top edge with a bounce (pop), fading in over 180 ms. Dismissed, it flicks back up the same way |
| Menus and tooltips | Menus grow from their button (snap from 96%); tooltips fade in over 120 ms (standard) while moving 3 px from their element (snap) |
| A segmented control | The thumb travels on snap and stretches 16% wide a third of the way into its travel, settling back as it arrives |

**Theme and accent**

| Moment | Motion |
|---|---|
| Light, dark or an accent chosen | The new look spreads in a circle from the point clicked until it covers the window (620 ms, glide). Where the platform cannot (5.7), every colour cross-fades instead (320 ms) |

**Quick capture and files**

| Moment | Motion |
|---|---|
| Quick capture opens | The box comes into focus: from 97% and 10 px up, blurred 10 px, to sharp (snap; the blur over 260 ms on enter), fading in over 280 ms (standard) |
| Quick capture is put away (<kbd>Esc</kbd>, <kbd>Ctrl Enter</kbd>, a click outside) | The page shrinks to its grain and flies to the tray on an arc: across quickly (`cubic-bezier(.25,.65,.45,1)`), down slowly then fast (`cubic-bezier(.6,0,.9,.45)`), so the path curves (600 ms). The tray icon pulses once as it lands: a ring in the accent grows from 40% to 260% and fades (560 ms, enter). With nothing written it only fades (280 ms, standard) |
| A file is attached | Its tile grows out of the paperclip, or out of the point it was dropped on, and bounces into place (pop). Several files follow one another, 70 ms apart |
| A preview opens and closes | The file grows out of its tile (420 ms, enter) and shrinks back into it (320 ms, standard), the backdrop fading with it (280 ms, standard). With reduced motion it fades in where it is (6.16) |
| The next file in the preview | It comes in 48 px from the side it was asked for from (280 ms, enter). A file read after its thumbnail was shown fades in where the thumbnail was |
| Files dragged over the window | A clay dashed edge and the words of what a drop would do fade in over the window (200 ms, standard) |

**Focus while writing (7.7)**

| Moment | Motion |
|---|---|
| The first key | Nothing moves. Only the controls fade: the note's icons and the window's to nothing, the list to half (200 ms, standard). The words being typed stay exactly where they were |
| The first pause | 600 ms without a key, or the first new line, and focus comes in by the chosen style, below. Each key before that waits it out again |
| Slide in | The list's content fades out (140 ms, exit); 90 ms later the list folds to nothing (glide); 60 ms after that the note's column glides to the middle (glide). Three moves, overlapping, each led by the one before |
| Rail in | The rows fold to the left one after another, 18 ms apart; the list narrows to 56 px (glide); then the dots pop into the rail top to bottom, 24 ms apart (pop) |
| Zen in | As Slide, then the paper warms (600 ms, standard, from 260 ms), the type grows by 4 px (glide, from 300 ms), and the top and bottom of the text fade in last (500 ms, from 420 ms). It deepens in stages |
| Out, any style | Reversed and quicker: the width returns first (glide) while the list's content fades in (200 ms, standard), the rows in sight arriving in a cascade from the left, 14 px, from 230 ms, 22 ms apart (snap) |
| The pointer during focus | A control fades in (200 ms) only when the pointer comes within 64 px of it: the note's icons in the gutter, the window's controls in the top corner, the list's handle at the left edge. Moving anywhere else shows nothing, and the layout never moves |

**Voice (6.18)**

| Moment | Motion |
|---|---|
| Pressing the mic | It grows to 132% (pop) as a sheet rises behind it (glide), with the lock above the mic |
| Listening | A row of grains moves with the voice, like sand on a drum: still in silence, most in the middle, higher as it gets louder. A soft ring around the mic breathes with it. No words are shown |
| Sliding up | The lock follows the finger halfway; past 60 px it drops away as the mic turns into ✓ (pop) |
| Sliding left | "Cancel" follows the finger and fades. Past 90 px the grains blow off to the left, blurring (260 ms, exit), and the sheet sinks |
| Letting go, or ✓ | The row of grains gathers into one grain, which settles onto the pile as a typed thought's does; the sheet sinks (glide) |
| A voice note gets its words | They fade in where its name was (280 ms, standard). Nothing else moves |
| Nothing heard | The sheet shakes once (320 ms) and sinks; nothing is made |

**Android gestures**

| Moment | Motion |
|---|---|
| Swiping a card | The card follows the finger exactly. Let go past 96 px: it flies off (180 ms, exit) and the list closes up (snap). Let go before: it springs back (pop) |
| Pulling the list down | It follows with resistance; let go past 62 px and it holds at 58 px while the arrow spins, then settles back (400 ms, enter) |
| Opening and closing a note | The note grows out of its card (440 ms, enter; its text rises 14 px behind it, 380 ms from 60 ms) and shrinks back into it, wherever the card now is (340 ms, standard) |
| A new page | It grows out of the bar at the bottom (the glide curve over 560 ms); nothing written, it goes back into the bar (320 ms, standard) |
| Find | Slides down under the title (glide); closing it slides it back |
| Keyboard | Rises on glide, and the field rides up with it |

**Devices**

| Moment | Motion |
|---|---|
| The code appears | The seven pictures pop in, 45 ms apart (the pop curve over 400 ms) |
| They match | The seven gather to the middle, from the outside in, shrinking (300 ms, exit), and where they meet the tick pops in (the pop curve over 400 ms) and draws itself (450 ms, enter, from 120 ms) |

**Static on purpose:** characters as they are typed, the caret, selection,
native scrolling, and anything following a finger while it is still down.

### 5.6 Reduced motion

When the system asks for reduced motion, every spring becomes a 90 ms
`ease` and every travel, growth, arc and stagger is dropped. The curves'
durations shorten with them: `--d-1` to 1 ms, the other three to 90 ms.
What remains is fading: a toast fades in, Settings fades in where it is,
the theme cross-fades (still over 320 ms, with no circle), a settled
thought's row fades in with no grain. What follows a finger still follows
it. While
listening the grains stay in a still row, and only their strength changes.
Spinners still turn, because they carry information.

### 5.7 Building it

- The springs are CSS `linear()` easings, written out from the simulation
  (48 steps each). WebView2 and Android's WebView (Chromium) support them;
  WebKitGTK does from its 2.44 releases. A web view without `linear()`
  gets `cubic-bezier` approximations: snap
  `cubic-bezier(.2,.9,.3,1.02)`, pop `cubic-bezier(.34,1.4,.64,1)`, glide
  `cubic-bezier(.2,0,0,1)`. Which it gets is found by asking the web view
  (`@supports` in the style sheet, `CSS.supports` in the script), never by
  its name.
- Movement that depends on where things are (the settling grain,
  Settings from the gear, a tile from the drop point, the gathering
  pictures) uses the Web Animations API from one small shared script, as
  the list's FLIP does.
- The circle reveal uses the View Transitions API. Where it is missing
  (WebKitGTK today), the theme cross-fades. That too is found by asking:
  the script looks for `document.startViewTransition`.
- Every animation reads the reduced-motion setting when it starts, so
  changing the setting takes effect at once. That holds wherever the web
  view passes the system's setting on as it changes. Android's WebView
  reads "Remove animations" once, when it starts, so the app reads
  `Settings.Global.ANIMATOR_DURATION_SCALE` itself when it starts and each
  time the person returns to it, and gives the answer to the root
  component's `reduced_motion`. Changed while Dirt is in front, it is
  honoured from the next return.

---

## 6. Components

### 6.1 The page, and Find

The most important component is the **page**: where a thought is written.

| | Desktop | Android |
|---|---|---|
| Where | The right side of the window, always there. With no note open it is blank, with the cursor in it | The bar at the bottom, "What's on your mind?", grows into a full-screen page (glide) |
| Blank | Only the placeholder "What's on your mind?", and 🎙 and 📎 in its gutter | The placeholder; the top bar has ⌄, 🎙 and 📎 |
| The first word | Makes the note. From then on it saves itself, as any note does, and the list leaves (7.7) | The same |
| <kbd>Enter</kbd> | A new line. No key ever saves | The same |
| Done | <kbd>Esc</kbd>, <kbd>Ctrl Enter</kbd> or ✓: the thought settles onto the pile (5.5) and the page is blank again, the cursor still in it | ⌄ or Back: the page closes into a grain that drops onto the list |
| Nothing written | Done does nothing. A note emptied again is let go without a word. Spaces and line breaks alone are nothing written; a file with no words under it is a note | The page goes back into the bar |
| Tags | With the list narrowed to `#work`, a new page gets `#work` at its end, where it can be seen and removed | The same |
| Files | Drop one on the page, or 📎: the page becomes a note with the file | 📎 |

Before, one field wrote and found, and a note was made the moment Enter was
pressed. That made writing a one-line act: anything longer meant writing
it, saving it, and opening it again. A page lets a thought be as long as it
is, and a page that saves itself takes away the decision of when it is
finished.

**Find** is a field of its own. On the desktop it is at the top of the list
(placeholder "Find", a magnifier, `#` at its end), reached with
<kbd>Ctrl F</kbd>, <kbd>Ctrl K</kbd>, <kbd>/</kbd> or a click. On Android it
is behind 🔍 at the top and slides down under the title.

| | |
|---|---|
| Typing | Narrows the list on every key. Same rule as the store's search: every word typed must start a word in the note; `#tag` matches tags |
| One line | Find never grows. Long text scrolls inside it; a pasted line break becomes a space. So the list under it never moves while you type |
| Tags | Chosen tags are tokens at the start of Find, `[#work ×]`. When they would leave too little room for words they fold into one, `[#work +2]`, which opens the tag list. They narrow the list and a new page gets them (6.3) |
| <kbd>Enter</kbd> | Opens the first note found |
| <kbd>↓</kbd> | Moves into the list (desktop) |
| <kbd>Backspace</kbd> when empty | Takes back the last tag |
| <kbd>Esc</kbd> | Closes the tag list; then empties the text; then clears the tags; then back to the page |
| Focus | Clay border and a 3 px `--accent-soft` halo |
| Nothing matches | The list is simply empty. No message |

### 6.2 Note row (desktop) and card (Android)

- First line as the title, the rest as a preview (two lines on desktop, three
  on Android), matches highlighted.
- Tags at the very end of a note are labels, not prose: they are left out
  of the preview and shown apart in small grey type, at most two and "+1"
  for the rest. Tags in the middle of a sentence stay where they are, in
  clay.
- On the right, or along the bottom on Android: the edit time ("now", "4m",
  "3h", "Thu", "Oct 3"), a paperclip if it has files (a mic instead if one
  of them is its voice recording, 6.18), the hollow dot if it hasn't reached the server.
- A picture attached to it shows as a 44 px thumbnail on the card.
- Its tags are buttons: clicking one narrows the list to it.
- Desktop hover: the time gives way to the archive box (a bin on an
  archived note). Selected: white surface,
  hairline, clay bar on the left.
- Android: tap opens it, swipe left archives it (deletes it, if it is
  already archived), long press shows its time in full.

### 6.3 Tags

Tags stay what they are in the note: `#words` in its text, which the CLI
and an export keep. What changes is that the apps never lay them all out.
However many there are, the screen shows none until they are asked for,
and then at most six.

| | Desktop | Android |
|---|---|---|
| Asked for by | The `#` at the end of Find, or typing `#` there | The `#` at the start of Find, which also spares the trip to the keyboard's symbols |
| Or | Click a tag on any note: the list narrows to it at once | Tap a tag on a card |
| Shown as | A list that opens in place under Find, pushing the notes down so they stay in sight: `#name` and a grey count, at most six besides those picked. The first unpicked has <kbd>Tab</kbd> beside it | A row of pills under Find |
| Narrowing | Typing after `#` narrows the list: `#co` shows `#cooking`, `#coffee` | The same |
| Picking | Click: the tag is ticked, becomes a token in Find, and **the list stays open** for the next. Click it again to take it back. <kbd>Tab</kbd> takes the first | Tap: the pill fills with clay and moves to the front. Tap again to take it back |
| The counts | Each count is how many notes there would be with that tag too. A tag that would leave none is not offered, so no pick can empty the list | The same, without the counts |
| Several | Notes with **all of them**, the default. Once two are picked, a small switch at the foot of the list offers **any of them** | A last pill, "all of them ⌄", switches to "any of them" |
| Closing | <kbd>Esc</kbd>, a click outside, or the `#` again. The tokens stay | The picked pills stay under Find while any are picked; the rest go |
| A token | Clay text on `--accent-soft`, with a small × | The picked pill is the token |
| A new tag | Type it in the note's text, as now. It is offered from then on | The same |

Before, the list closed after each pick, so a second tag meant opening it
again, and nothing showed which tags could still narrow the list without
emptying it.

The same tokens do two jobs: while `#work` is in Find, the list shows
`#work` notes, and a page started then is written with `#work`.

The tags and their counts come from the store in one call
(`Store::tags_to_narrow`), given the words in Find, the tags picked, the
rule, and what was typed after `#`. Settled when it was built:

- **A count is what picking the tag then shows.** It is the number of
  notes Find gives for the same words and the picked tags with that tag
  added, archived notes among them (6.19). The store's tests hold the two
  to each other.
- **The order** is by count, the most first, and by name among equals.
  The six shown are the first six.
- **Under "any of them"** another tag takes nothing away, so its count is
  the notes there are already and those it would add. A tag is offered
  when a note that matches the words carries it. One whose notes are all
  in the list already is offered with the number there is now; one that no
  matching note carries is not offered, since it would add nothing.
- **Typing after `#`** narrows by the start of the name, in lower case. A
  character a tag cannot hold matches no tag.

### 6.4 Editor

An open note is the same page (6.1): its text, its files under it, and
its icons in a **gutter beside its first line**, just outside the text
column: ✓ (done), 📎 and the archive box. On an archived note the box
gives way to "back to the list" and 🗑 (6.19). On the blank page the gutter
holds 🎙 and 📎.
Copy is in the right-click menu and <kbd>Ctrl Shift C</kbd>, not an icon.

The icons belong to the note, so they sit by the note and travel with it:
when the column glides to the middle in focus, its gutter goes with it.
They never share the top row with the window's controls. That put two
clusters of grey icons side by side, one for the note and one for the
window, a few pixels apart, and far from the text when the column was in
the middle.

At rest the icons show at 60%, full on hover. While writing they go, and
come back only when the pointer comes within 64 px of the gutter (200 ms).
The window's controls do the same at their corner, and the list's handle
at the left edge. A mouse moving anywhere else brings nothing back. A
control that has the keyboard's focus always shows, whatever the pointer
does, and so does one touched on a touch screen.

No title field, no formatting, no Save, no "Saved", no "Edited 4 minutes
ago" (the row in the list has the time, and its tooltip the full date).
On Android the note already fills the screen, and its top bar fades while
the keyboard is up.

### 6.5 File tile

Thumbnail or kind icon, name, "1.8 MiB · JPEG", and where it is if that
matters: "uploading" (with a clay line running along its foot), "on this
device only" (offline or signed out), "not on this device; fetched when it
is wanted".
Desktop hover shows **Open in another app**, **Save a copy** and
**Remove** as icons; Android always shows them. A file the system would run
is shown without **Open**. A click or tap anywhere else on the tile, or
<kbd>Space</kbd> when it has focus, previews it (6.16).

What this left open is settled under 6.16. The size is in the core's words,
which count in binary units ("1.8 MiB", "214 KiB"); the first version of
this section said "MB".

### 6.6 Sync status

| State | Desktop footer | Android top bar | Tooltip |
|---|---|---|---|
| Synced | cloud-tick alone, quiet grey. No word | cloud-tick | "Last synced 2 minutes ago" <kbd>Ctrl R</kbd> |
| Syncing | turning arrows, clay. No word | turning arrows | |
| Just synced | green tick for 1.4 s | green tick | |
| Offline | cloud struck through, "Offline · 2 waiting" | the same, with a grey dot when notes wait | "Notes wait here and go when you're back" |
| A problem | amber triangle, "1 note can't sync" | triangle with an amber dot | "See why" |
| Signed out | laptop, "This device only" | laptop | "Not syncing. Sign in to sync" |

Clicking it syncs now. When there is a problem it opens Settings → Sync
instead, and when signed out it opens Settings → Account. Sync never
interrupts with a dialog.

The short words here and the full sentences in Settings → Sync must come
from `dirt_core::sync::overview`, so all three clients say the same thing.
The overview has the full sentences, and since step 1 of the plan the short
form: `SyncIndicator` says which of the six states holds, with its words
and its tooltip, and `Session::sync_indicator` gives it for a session. An
app chooses the icon by the state and writes no words of its own.

What the table leaves open, settled when that was built:

- **Which state, when two hold:** signed out, then syncing, then just
  synced, then offline, then a problem, then synced.
- **Offline** is any failure that sync tries again by itself: no network,
  a server that cannot be reached, or one that asks to be left alone for a
  while. With nothing waiting it says "Offline" alone.
- **"2 waiting"** counts notes. A note with a new picture is one thing
  that waits, though its picture and its thumbnail wait too, so files are
  counted only when no note waits and files do.
- **A problem** is a note that could not be synced ("1 note can't sync",
  "3 notes can't sync"); or, with no such note, a file the server refused
  ("1 file can't sync"); or a sync that failed for a reason waiting does
  not cure, which today is that this device has to sign in again or that
  the app is too old ("Can't sync"). Settings → Sync says which.
- **Signed out** is also a device that has signed in and is waiting to be
  let in: its notes are on it alone until then.
- **Just synced** holds for 1.4 s after a sync completes, and only if it
  completed: a sync that failed has not just synced.
- **The tooltip of Synced** counts as the sentence in Settings does: "Last
  synced just now" within the first minute, then whole minutes, hours and
  days ("Last synced 1 minute ago", "Last synced 3 hours ago"), and "Not
  synced yet" before the first sync. Syncing and Just synced have no
  tooltip. Each state has its short word for a screen reader, shown or not.

### 6.7 Toast

Dark on light (and light on dark), bottom centre, above the Android field.
One at a time: a new one replaces the old. Carries at most one action.
Lasts 2.6 s, or 6 s when it has **Undo**, with a draining bar along its
bottom so the time left is visible. Hovering it pauses the timer, and so
does keyboard focus inside it. The bar is the timer: it stops while the
toast is held and goes on from where it was, so what it shows is what is
left.

On the desktop <kbd>Ctrl Z</kbd> is written beside **Undo**. A toast can
be told to stay until it is dismissed, with a close icon in place of the
bar; that is for Android while a screen reader is on (section 12).

### 6.8 Banner

For something another device is waiting on, which can't wait for the person
to open Settings: today only "Work laptop wants to join". Appears on every
device already in, at the top, with **Review** and (desktop) a close icon.
It stays until answered or dismissed.

A device learns that another is waiting when it syncs: on start, on return
to the app, after a change. There is no system notification (decided
2026-11-16). Comparing the code needs both devices in hand, and opening
Dirt on the one already in shows the banner.

### 6.9 Panel (desktop Settings) and sheet (Android Settings)

One scrolling page in six sections: Account, Devices, Sync, Appearance,
Voice (6.18), Export. Voice comes with voice itself (the plan's step 9);
until then the page has the other five, since a section with nothing in it
would be a heading over nothing. On the desktop a rail on the left jumps to each section and follows
the scroll. On Android it is a sheet with a grabber. Each section is a card
of rows: an icon square, a title, one line of grey detail, and at most one
control on the right.

### 6.10 Buttons

| Kind | Look | Used for |
|---|---|---|
| Icon button | 32 px (40 dp), no fill until hover | Every action in the main screens |
| Primary | Clay fill, 34 px | The one main action of a flow: **Sign in**, **They match**, **Review** |
| Quiet | `--surface-2` fill | Secondary actions in Settings: **Make one**, **Export** |
| Ghost | Hairline outline | The other choice beside a primary: **They don't**, **Sign out** |
| Danger | Red text; red fill, with `--on-danger` text, only for the final confirm | **Delete the account**, **Remove** |

A flow has at most one primary button on screen.

On Android a button with words is 40 dp high (36 dp for the small one in
a row), and every icon button is 40 dp whatever size the desktop gives it.

### 6.11 Segmented control, slider, switch

Segmented control for two or three exclusive options (theme, export format),
with a sliding thumb. A slider with small and large "A" for text size.
Colour dots for the accent: 24 px circles, 9 px apart, each in the colour
it would be in the current theme; the chosen one has a 2 px ring of its own
colour 2 px outside it. No names on screen: each dot's tooltip (long press
on Android) gives its name, and Clay's says "(default)".
Checkboxes only inside a flow ("Bring the 12 notes on this device into the
account", checked by default).

A switch is 40 × 24 px. Off, it is an outline in `--border-control` with a
thumb of the same colour; on, it is filled with the accent and its thumb,
in `--on-accent`, has moved across (snap).

The segmented control and the accent dots are groups of exclusive choices:
one stop for <kbd>Tab</kbd>, and the arrow keys move the choice. On
Android a segment is 40 dp high and a dot, still 24 px, is 16 px from the
next so that each has a 40 dp target. While the text-size slider's thumb
is held, the text on the page follows it; the size is kept when the thumb
is let go.

### 6.12 Confirmation in place

For the few things that can't be undone, the row itself turns pale red and
asks: "Remove Pixel 8? It stops getting notes." with **Cancel** and
**Remove**. No modal dialog. Deleting the account also asks for the word
DELETE to be typed in the real app.

Focus goes to **Cancel** as the row asks, <kbd>Tab</kbd> stays between its
two buttons, <kbd>Esc</kbd> is **Cancel**, and focus goes back to the row
afterwards.

### 6.13 Code comparison

Seven emoji with their names, in a row, the same on both devices (the
short-code verification from `vodozemac`). "Does Work laptop show these?
Same pictures, same order: then it's really yours." **They match** (primary)
and **They don't** (ghost). Then a drawn tick ("Work laptop is in. It can
read your notes now.") or a red cross ("Nothing was shared. Start again from
the new device.").

### 6.14 Empty states

Almost nothing. A list with no notes at all shows a faint feather; a list
that Find has emptied shows nothing, because Find above already says why.
With no note open the page is blank, with its placeholder: that is the
empty state, and it is also where to start. No sentences, no
illustrations, no onboarding cards.

### 6.15 Scrollbars

Scrollbars are drawn by the app, the same on Windows, macOS, Linux and
Android, and lie over the content instead of taking a strip of its width.

| | Desktop | Android |
|---|---|---|
| At rest | Not shown | Not shown |
| Appears | While scrolling, and when the pointer comes within 28 px of the edge | While scrolling |
| Thumb | 5 px, fully rounded, `--text` at 24%, 2 px in from the edge, 4 px from the ends; never shorter than 28 px | 3 px, 1 px from the edge; never shorter than 36 dp |
| Under the pointer | Widens to 8 px, `--text` at 42% | n/a |
| Track | Invisible. A click on it scrolls 90% of a page towards the click | n/a |
| Dragging | The thumb follows the pointer | n/a: the content follows the finger |
| Goes | Fades out 0.9 s after the last scroll, unless the pointer is on it or dragging it | Fades out 0.9 s after the last scroll |

It is used on everything that scrolls: the list, the open note, Settings,
and a text file in the preview. The colour follows the theme because it is
made from `--text`, and the accent is not used: a scrollbar is never the
thing to look at. Keyboard and screen readers are untouched, because the
content still scrolls natively and only the drawing of the bar is the
app's own.

### 6.16 File preview

A file is looked at inside Dirt. Its bytes are read into memory to be
shown and no copy of it is written to be shown from, so no other app is
handed it. **Open in another app**, which does write a copy under the
file's own name and hand it over, stays for when the person wants that.

This paragraph said first that the decrypted bytes are never written to
disk. That was more than is true: a file the device holds lies beside the
notes' database as it was attached, not encrypted, as the notes themselves
lie there (`docs/SECURITY.md`), and a file fetched to be previewed is kept
there like any other, so it is fetched once. What the preview spares is
the second copy, in the folder another app can be lent from. The core
gives the bytes with `Session::file_bytes`; a file is 10 MiB at most.

| Kind | Shown as |
|---|---|
| Pictures: JPEG, PNG, GIF, WebP | The picture, as large as fits. Double-click or pinch to zoom to 2× |
| Audio | A card: the clay play button, a waveform that fills with clay as it plays and can be clicked to seek, and the time in mono |
| Video | The video with the system's own controls |
| Text: `.txt`, `.md`, `.csv`, `.json`, code | The text in mono on a card, with its own scrollbar |
| PDF | Pages, on every platform: a renderer is bundled with the apps (decided 2026-11-18, section 11) |
| Anything else, HEIC among them | Not previewed: a click shows its icons, and **Open in another app** is one press of its own. Nothing says it couldn't be previewed |

On screen there is only the file, on a near-black backdrop. Around it, and
quiet:

- **Top right:** Open in another app, Save a copy, Remove from note, Close.
  On Android a tap on the backdrop hides and shows these.
- **Top left:** the file's name, on hover only (desktop).
- **Sides:** ‹ and ›, on hover, when the note has more than one file
  (desktop). Android swipes.
- **Bottom:** a dot for each file, the current one wider. None for one
  file.

| Does | Desktop | Android |
|---|---|---|
| Open | Click the tile, or <kbd>Space</kbd> on it | Tap the tile |
| Next, previous | <kbd>→</kbd> <kbd>←</kbd>, or ‹ › | Swipe sideways |
| Close | <kbd>Esc</kbd>, <kbd>Space</kbd>, a click on the backdrop, or × | Swipe down, Back, or × |

A file that is not on the device yet is fetched first: a clay ring turns on
its tile, and the preview opens when it has come. The picture's thumbnail,
already on the device, is shown at once while the whole file is decrypted.

The ring was first to fill. A file is fetched in one request, whose
progress the core does not see, and a file is 10 MiB at most, so the ring
turns until the file has come, as a spinner does; so does the line of a
file being sent, which goes in one request at the end of a sync.

**Settled when files were built** (implementation plan, step 6). The tiles
and the preview are `dirt_ui::files`, the same for both apps; each app
gives them what only it can do (another app, a save dialog).

| | Settled |
|---|---|
| Which files are previewed | Pictures: JPEG, PNG, GIF, WebP. Sounds: MP3, M4A, AAC, WAV, Ogg, Opus, FLAC. Videos: MP4, M4V, WebM; not QuickTime's `.mov`, which Chromium does not promise to play. Text: any `text/` type, and the endings of plain text, data and code (`.txt`, `.md`, `.csv`, `.json`, `.log`, `.rs`, `.py`, `.sh` and the like): a script is shown, never run, which is how to look at one safely. PDF. Not SVG, which this table did not list. Told by the type the note gives and by the name's ending (`dirt_ui::files::kind`), never by reading the file |
| The kind on a tile | The name's ending in capitals, "JPEG" for `.jpg`, "File" for none. A sound with no thumbnail shows a note's icon of music, anything else a page's |
| Where the file is | The core's words (`AttachedFile::whereabouts`): "uploading" while the device can send (signed in, let in, not offline), "on this device only" while it cannot, "not on this device; fetched when it is wanted", "could not be sent:" and the server's reason |
| A file the preview does not show | A click gives the keyboard to the tile's first icon, which shows the icons; its tooltip says "Not shown here: open or save it from its icons". Opening a file in another app is always a press of its own, never a click on the file (a review found that a click on the file was one press away from running a document's macros). Section 2 is unchanged: open is one action, the icon |
| A copy handed out | On Windows, **Open in another app** and **Save a copy** mark their copy as downloaded (the Mark of the Web), so that Office opens it in Protected View and SmartScreen asks before running what is in it. The kinds never opened are `runs_as_a_program`'s, which counts Office's older kinds, Excel's add-ins and disk images among them (`docs/SECURITY.md`) |
| Opening | A file on the device opens at once, its thumbnail standing in, blurred, while it is read. One that is not has the ring on its tile and opens when it has come; if it cannot be had, an error that stays says so ("receipt.pdf could not be fetched"), the reason in its tooltip. Stepping in the preview to a file not on the device shows the ring, white, where the file will be |
| The tile's parts | The part that previews and the three icons are side by side, not one inside the other, so that each is a control of its own for the keyboard and a screen reader. <kbd>Space</kbd> and <kbd>Enter</kbd> on it preview |
| Focus in the preview | The preview itself takes the keyboard as it opens, not its first button, so that <kbd>Space</kbd> closes it and presses nothing. <kbd>Space</kbd> presses a button that has been given the keyboard (<kbd>Tab</kbd>), as it does anywhere, plays a sound from its play button, and scrolls text or a PDF that has the keyboard. <kbd>←</kbd> <kbd>→</kbd> step through the files except on a video or a sound's bars, where they seek |
| Remove | One action, as section 2 promises, with the toast "File removed" and no **Undo**: the device lets go of a file that no note names at its next sync, so there would be nothing to bring back. In the preview the next file takes its place, or the preview closes |
| A sound | The card's bars are the sound's own shape, read from it in the page; a sound the web view cannot read is drawn flat and still plays. The time is "0:12 / 0:48" |
| A PDF | Its pages one under another, as wide as the column allows up to 900 px, each drawn as it comes within 600 px of sight. Drawn by PDF.js (section 11); a PDF it cannot draw says so where its pages would be, and **Open in another app** is beside it |
| Text | In mono on a card, with the drawn scrollbar. Read as UTF-8, a byte-order mark left off and anything that is not UTF-8 shown as the replacement character |
| Pinch | Two fingers spreading by a fifth zoom a picture to 2×, closing by a seventh bring it back. A double tap does the same |
| The bytes | Held in memory by the app while the file is shown, and given to the web view at an address only the app answers (`dirt_ui::files::served`), which tells it to keep nothing and guess nothing, and lets a video seek. The type it is told is the app's, from a short list for the kind it decided, never the note's claim. Let go of as the preview moves on or closes; a file still being read then is dropped when it comes (`docs/SECURITY.md`) |
| A video | The web view's controls without Download, picture-in-picture or casting. Nothing in the preview has the web view's menu, or can be dragged out of the window |
| Dropping (desktop) | Anywhere on the window, as 7.3 says; while files are dragged over it, a dashed clay edge and "Drop to attach to this note" or "Drop to make a note of it" show. Not while the preview, Settings or the panel of keys is open: the note they would go to cannot be seen, so the hint does not show and the drop does nothing |
| Pasting (desktop) | A paste into the page with text in it pastes the text, as always; one with no text and a file (a picture copied from another program) attaches the file instead, and on a blank page makes a note of it, as a drop does. A picture the web view calls "image.png" is named "Pasted picture.png". A file past 10 MiB is not sent over from the page and is refused as too large |
| How many at once | One drop or paste takes as many files as a note may have, 20; an error names the rest. On Android one choosing or one share is copied into the app up to 20 files, and the rest are named and not copied |
| Android: the tile | Its icons always show, beside its words (they are 40 dp targets). A tap anywhere else on it previews it; on a kind the preview does not show, it gives the keyboard to the first icon, as on the desktop |
| Android: opening in another app | A copy in the app's own files, lent by the `FileProvider`'s `content:` address with a grant to read that one file, through Android's chooser. When no app on the phone opens the kind, the chooser says so itself ("No apps can perform this action") and Dirt adds nothing. There is no Mark of the Web on Android; `docs/SECURITY.md` says what protects the copy instead |
| Android: saving a copy | Android's screen for choosing a place, as an export is saved, with the file's name proposed; the toast "Saved a copy", and backing out says nothing |
| Android: the preview's gestures | A swipe sideways steps, a swipe down closes, a tap on the backdrop hides and shows the controls, two fingers pinch, anywhere on the preview but its buttons. On text or a PDF a finger moving up or down scrolls them and only one moving sideways steps (the preview has `touch-action: none`, its scrolling cards `pan-y`). Back closes it. While it is open, Android's bars take its near-black |
| A file that cannot be attached | An error that stays ("A file could not be attached", "3 files could not be attached"), the reasons in its tooltip, where the first version said so in a line under the tiles |

**What did not work as the design assumes**, found with real touch on the
Android emulator (step 6b). With a PDF or text on the stage there was at
first nowhere to swipe to the next file: the card took every touch for its
scrolling. A pinch was judged when the last finger lifted, by which time
only one finger was left, so it never zoomed. A tap straight after a swipe
was taken for the swipe's own end and swallowed. And a swipe begun on a
picture's stand-in, which the picture replaces a moment later, stopped
being heard, as touches on an element taken out of the page no longer
reach the document. Each is mended in the script, as the table says.

### 6.17 Grains, layers and the pile

| | |
|---|---|
| A grain | One note. Its shade of sand is taken from its ID (hue 20–44°, saturation 30–62%, lightness 48–72%), so neighbours differ a little, as sand does. Drawn as a slightly irregular round, 7–12 px |
| Layers | The list is grouped by when each note was last edited: today (no label; it is the surface), This week, Earlier this month, then each month by name, with the year when it is not this one. A layer starts with its name in 11 px `--text-3` and a line of dots, 7 px apart, to the edge. Where each begins is below |
| The pile | Under the last note, when nothing is being found: the heap (below). Under it one line, "1,254 thoughts since Nov 2025", and, if any, "3 archived", which lists them |
| Elsewhere | The Rail's dots (7.7); the pile on Android's home screen (8.5) |
| Never | Counts down, sets goals, keeps streaks, celebrates a number, or shows while a note is being written |

The pile is drawn from the notes the device already has and stores nothing
of its own.

**Where a layer begins.** `dirt_core::models::Layer` names the layer of an
edit time, given the time now and how far local time is from UTC.

| Layer | Holds notes last edited |
|---|---|
| Today | On today's date, by the device's clock. And later than now, which a device whose clock is ahead can send |
| This week | On one of the six days before today |
| Earlier this month | Before that, in this month |
| October | In that month of this year |
| December 2025 | In that month of another year |

"This week" is not the calendar's week. Where a week starts differs from
country to country, and on its first day a calendar week would be empty, so
that yesterday's notes would lie under "Earlier this month". Six days back
from today is the same rule every day: on a Monday the layer reaches back
to the Tuesday before. It may reach into last month, and in the first days
of January into last year, and then those days are "This week" and not
"December 2026". In the first week of a month nothing is "Earlier this
month".

**A grain's shade** is `dirt_core::models::Grain`: the 32-bit FNV-1a hash
of the note's ID, written as text in lower case, gives the hue (20 and its
remainder by 24), the saturation (30 and the remainder by 32 of the hash
shifted right five bits) and the lightness (48 and the remainder by 24 of
the hash shifted right ten). That is the mockup's function with one
correction: the mockup's shifts keep the sign, so half of its IDs get a
saturation or a lightness below the range given here. The hash itself is
handed on, for whatever else about a grain must be the same everywhere.

**The line under the pile** counts every note that is not deleted as a
thought, the archived ones among them, since an archived note stays a
grain; "3 archived" says how many of those are archived. "Since" is the
month in which the oldest of them was made, by its ID, in local time.
`Store::pile` gives every such note's ID, edit time and whether it is
archived in one call, and `PileSummary` the numbers of the line.

**How the pile is drawn: as sand piles up.**

| | |
|---|---|
| A grain | About 1.7 px across, a third of a millimetre on most screens: too small to see one, so many read as sand, not as dots. In the heap, grains overlap a little, so no background shows between them. Each keeps its note's own shade |
| Piling | Grains are added oldest first. Each is dropped near the middle, a little to either side, as from a hand. It rolls down while the slope under it is steeper than sand's angle of repose, about 33°, and now and then rolls a step further. The heap that results has sand's own shape at any size: a rounded top, straight flanks, a foot that spreads. The newest grains are on its surface |
| Size | The heap is as big as the pile: its area grows with the number of thoughts. Ten are a pinch 15 px wide; a year's 1,250 are about 110 px wide and 50 px high. When it would be wider than its place (the widget), the grains get smaller instead |
| Light | From the upper left: the shade deepens smoothly towards the far side, by up to a quarter, with no line where the flanks meet. A soft shadow lies under the heap |
| Pointing at it | "1,254 thoughts" |

The simulation is worked out once per change, not per frame, and is the
same every time for the same notes. Each grain's chances (where it is
dropped, which way it rolls, how far it is nudged off its row) come from its
own note's grain, not from one stream for the whole heap, so a new thought
adds a grain on top and moves no grain already there; an edit or an archive
changes nothing the heap is made of, and the heap is not worked out again
for it. It is drawn as one SVG at any size: the grains of each shade,
coarsened a little past what the eye tells apart, are one path of dots (a
line of no length with round ends is a dot), so even 20,000 grains are a
few hundred elements. No canvas is needed.

What was tried first, and why not:

| | Why not |
|---|---|
| The same piling with 6 px grains | A triangle of dots, not sand. The colours mixed into mud, and the heap was too steep |
| Layers: a band for each month | A diagram, not sand |
| Dune: a drawn shape with ripples and a grain texture | Looked built, not blown |
| Days: a grain per day for a year | A streak chart under another name, which principle 5 rules out |

**Settled when it was built** (implementation plan, step 7). The heap is
`dirt_ui::pile::Heap`, worked out from `Store::pile`; the pile under the
list is `dirt_ui::components::Pile`; the settling is the script's, which
the row or card a new thought lands on asks for with `data-dui-settle`.

| | Settled |
|---|---|
| Which thoughts settle | A new thought with words, made on the page from a blank one, when it is let go by any of the ways 6.1 gives (on Android ⌄ or Back). A note opened from the list goes back to its place with no grain (5.5), and so does a page that was emptied again, or one with files and no words: there are no words to gather |
| Where the grain starts (desktop) | The page is taken as the key or press that lets it go comes (<kbd>Esc</kbd>, <kbd>Ctrl Enter</kbd>, <kbd>Ctrl N</kbd>, ✓), before the app has heard of it: its words, where they are and in which type. The script empties the page on <kbd>Esc</kbd> at once (7.9), so it is read just before |
| Where the grain starts (Android) | Where the page closes to its 7 px circle: the start of its text. The page closes on a circle shrinking to that point (the mockup's clip from a rounded rectangle to a circle cannot be played: a browser jumps between two shapes that differ in kind) |
| The row it lands on | Held out of sight until the grain lands, by an animation, which a redraw of the list leaves alone, so a sync arriving mid-flight does not show it early. Brought into sight first if the list was scrolled away from it. Where the grain started is kept 1.5 s (1.2 s on Android): a row marked later only fades in |
| The pile's drop | The note was made at its first character, so its grain was in the heap already, unseen behind the page. The newest grain is drawn on its own on top, and drops as the thought lands, if the heap is in sight. A note from another device adds its grain without a drop |
| An archive | Its grain sinks from the row's left edge while the row folds shut (5.5), from the box, <kbd>Del</kbd>, or the box on the note's page. A card swiped away has flown off already: its gap folds shut with no grain |
| Where the pile is | Under the last note, while nothing is being found and the list holds every note (once loading more has reached the end), as "3 archived" was before it. A list whose notes are all archived shows the pile, not the empty drawing (6.14): there are still grains |
| Its words | "1,254 thoughts since Nov 2025": a comma between thousands, and the month's first three letters with the year, whatever the pile's age. "3 archived" under it, a button that lists them. Pointing at the heap says "1,254 thoughts", and a screen reader reads the same |
| Its size | No wider than 250 px under the list; above about 3,700 grains they get smaller to fit. The pile widget is the plan's step 10 |
| Reduced motion | The settled row fades in; no words gather, no grain flies, no dust, and the heap does not drop a grain. An Android page fades. An archived row fades with no grain |
| What it costs | Measured on this laptop, a debug build: 1,000 grains 2.4 ms, 5,000 grains 16 ms, 20,000 grains 92 ms, the picture 34 KB, 101 KB and 271 KB. Worked out on a thread for blocking work, only when the notes it is made of change. The notes themselves are read (`Store::pile`) only when the store's counts say they may have changed, a note made, deleted or brought back, and not at every reading of the list: at 20,000 notes that keeps a reading at 5–6 ms instead of 75–113 ms (`docs/LIMITS.md`) |

### 6.18 Voice

The fastest thought is one said aloud: no keyboard to open, nothing to aim
at. Voice is a way to write, not a recorder. What is kept is a note like
any other, its words as text, with the recording on it as a file.

**One gesture, the same everywhere:**

| Do | Happens |
|---|---|
| **Hold** the mic, speak, **let go** | Kept: the thought settles onto the pile as a grain, and its words follow. One action |
| **Tap** the mic | Hands-free: it listens until ✓ (or <kbd>Enter</kbd>); ✕ (or <kbd>Esc</kbd>) throws it away |
| Hold, then **slide up** | Locks into hands-free without letting go first, for a long thought |
| Hold, then **slide left** | Throws it away |
| Started by a hardware key, with no hand on the screen | Ends with no hand too: 1.6 s of quiet after speech keeps it |

**While listening** nothing is written. A row of grains moves with the
voice, and a timer shows from the first second. Nothing else: no words, no
"Listening…", no level meter, no waveform of what was said.

Words that appear as they are spoken were given up on 2026-11-17 for words
that are right. A model that reads a whole recording once, after it ends,
makes fewer mistakes than one that has to write while it hears, and what
is kept matters more than what is watched. The mockup still shows words
appearing; this document holds.

**Afterwards.** Letting go keeps the thought at once. The recording
becomes a note, which settles onto the pile as a grain and is named by its
length ("Voice · 0:12") until it has its words. The device that recorded
it turns the recording into text in the background, and the words take the
name's place in the list, fading in, with no other sign. A short thought
should have its words within a few seconds; how long each model takes is
the plan's second voice measurement. Opened before then, the note shows
its recording and a page that can be typed on, and the words, when they
come, are put at the start of the note. If Dirt is closed first, the words
are made the next time it runs.

| | Desktop | Android |
|---|---|---|
| In the app | 🎙 in the blank page's gutter. The page itself listens: the grains and the timer, ✕ and ✓ at the bottom | The mic in the dock. A sheet rises over the lower half of the screen |
| From anywhere | **Hold** <kbd>Ctrl Alt N</kbd>: quick capture opens listening, and letting go keeps it. A tap still opens it for typing | The widget's mic (hands-free, ✓ to keep). Where the phone lets a hardware key open an app (double-press the side key on Samsung; Quick Tap on Pixel), Dirt's **Talk** opens straight into listening, over the lock screen too, and ends on a pause |
| Into a note being written | Not offered: type, or use the system's own dictation | 🎙 in the note's top bar adds what is said at the end of the note, once its words are made, and the recording to the note's files |

**Heard on the device, always.** Speech is turned into text on the device
that heard it and is never sent anywhere: not to Dirt's server, not to a
speech service. That is the only way voice keeps Dirt's promise that the
server sees nothing it can read. A kept recording is an attached file,
encrypted like any other. Settings → **Voice** has: one line, "Turned into
text here. Your voice is never sent anywhere"; **The recording** (Keep, or
Text only); **Language** (Auto, or one chosen); and on the desktop the held
hotkey, on Android a word about the side key.

**Failing well.** Nothing heard: the sheet shakes once and goes, and
nothing is made. The words cannot be made (the engine
failing): the note stays as it was kept, a recording named by
its length, and gets its words when they can be made. Only the device that
recorded it makes them: two devices doing so would add them twice. No
microphone permission yet: the first press asks, in the system's
own dialog; refused, the mic shows a slash, and its tooltip or long press
says where to allow it.

**The recording** is one file on the note, marked in the note's document
as its voice recording; that mark is what puts the mic on its card (6.2).
With **Text only** chosen the recording is taken off the note once its
words are made, and the mic with it: the note is then a note like any
other. Until then it is a file like any other and may reach the server,
encrypted; if the words cannot be made it stays, so that nothing said is
lost. A recording is compressed speech: Ogg Opus, 16 kHz mono at
24 kbit/s, which the plan's first trial settled. Ten minutes is about
1.6 MiB, well inside the limit on one file, and listening stops at ten
minutes, keeping what was said. Every platform writes it the same way, from
Rust. A web view that cannot play Ogg Opus (by its documentation, macOS's
before 15.4) is handed the same sound decoded, from memory.

**Languages.** Voice is built and tried for English first (decided
2026-11-16). Other languages come with the languages work recorded in the
system design, section 16.3, and **Language** in Settings lists only what
has been tried.

### 6.19 Archive, and delete

Most thoughts stop mattering: the bread was bought, the meeting happened.
They should leave the list. But in a pile of sand nothing is thrown away,
and a thought that seemed done is often the one looked for a year later.
So the everyday act is **archive**, and deleting is kept for what should
not exist at all.

| | Archive | Delete |
|---|---|---|
| What it does | Takes the note out of the list. It stays a grain in the pile, is found by Find, syncs, and is exported | Removes the note everywhere, as now: kept marked deleted for thirty days so the delete can sync and be undone, then purged |
| Desktop | The box on hover, <kbd>Del</kbd> in the list, the box in the note's gutter, <kbd>Ctrl Del</kbd> in a note | <kbd>Shift Del</kbd> (<kbd>Ctrl Shift Del</kbd> in a note). On an archived note: the bin, or <kbd>Del</kbd> |
| Android | Swipe a card left; the box in the note's bar | On an archived note: swipe left, or the bin |
| Then | Toast "Archived · **Undo**". The note sinks into the pile (5.5) | Toast "Note deleted · **Undo**" |
| Finding it | Find shows archived matches after the rest, under a line "Archived". "3 archived" under the pile lists them all, newest first; <kbd>Esc</kbd> or ‹ goes back | Gone |
| Bringing it back | Its "back to the list" icon (a box with an arrow up), or **Undo** | **Undo**, or <kbd>Ctrl Z</kbd> |
| An archived note | Opens and edits as any note. Its gutter shows "back to the list" and the bin instead of the box; its title is a shade lighter in lists | |
| Not an edit | Archiving, and bringing back, change neither the note's text nor its edit time, so the note keeps its layer. A note deleted on another device stays deleted: archiving does not bring it back, as an edit would | |
| Editing an archived note | Leaves it archived. Only "back to the list" or **Undo** brings it back | |
| Two devices choose differently | One choice holds, the same on every device, as for any single value in a note | |
| Its tags | Narrowing to a tag is finding, so archived notes with the tag are shown, after the rest and under the same line "Archived". The count beside a tag counts them, and a tag that only archived notes carry is still offered: a thought put away can be found by its tag | |
| Deleted, then brought back | Comes back archived, and into its place: it keeps the edit time it had (7.8) | |
| A build from before archive | Shows archived notes in its list, and keeps the mark when it edits one | |

Why both: with only delete, clearing a done thought loses it, so people
hesitate and the list fills up. With only archive, a thought that should
not exist (a password typed in the wrong place) has no way out. Giving
each its own key keeps both one action on the desktop. On Android the
swipe archives, because a swipe is easy to make by mistake and archiving
loses nothing.

---

## 7. Desktop

### 7.1 Starting

The window opens where it was left, with the list and, beside it, a blank
page with the cursor in it. It works signed out from the first second. There is no first-run
screen.

### 7.2 Quick capture

<kbd>Ctrl Alt N</kbd> from any app (or **New Note** in the tray menu) opens a
small floating page in the middle of the screen, over whatever is there.
It grows with the thought, up to about twelve lines. <kbd>Enter</kbd> is a
new line. <kbd>Esc</kbd>, <kbd>Ctrl Enter</kbd> or a click outside puts it
away, saved: it shrinks to a grain and flies to the tray, and focus goes
back to the app you were in. Closing it never loses what was typed.

**Held** instead of tapped, the same keys open it listening (6.18): speak,
let go, and it is kept.

### 7.3 Files

Drop files anywhere on the window. With a note open, they are attached to
it ("Drop to attach to this note"). With no note open, they become a new
note named after them ("Drop to make a note of it"). Pasting a picture into
the open note attaches it; pasted on the blank page, it makes a note, as a
drop does (6.16). The paperclip opens the file dialog, which takes
several.

### 7.4 Keyboard

Everything can be done without the mouse.

| Keys | Does |
|---|---|
| <kbd>Ctrl Alt N</kbd> | Quick capture, from any app. **Held**: talk; letting go keeps it |
| <kbd>Esc</kbd> or <kbd>Ctrl Enter</kbd> in a note | Done: a new thought settles onto the pile, and the page is blank again |
| <kbd>Ctrl N</kbd> | The same, from anywhere in the window |
| <kbd>Ctrl F</kbd>, <kbd>Ctrl K</kbd> or <kbd>/</kbd> | To Find, its text selected |
| <kbd>Enter</kbd> in Find | Open the first note found |
| <kbd>#</kbd> in Find | The tag list; <kbd>Tab</kbd> picks the first |
| <kbd>Backspace</kbd> in an empty Find | Take back the last tag |
| <kbd>↓</kbd> in Find | Into the list |
| <kbd>↑</kbd> <kbd>↓</kbd> in the list | Move, showing each note |
| <kbd>Enter</kbd> in the list | Edit the note |
| <kbd>Esc</kbd> in the list | Back to the page |
| Any letter in the list | To Find, and type it |
| <kbd>Enter</kbd> / <kbd>Esc</kbd> while listening | Keep / throw away |
| <kbd>Del</kbd> in the list, <kbd>Ctrl Del</kbd> in a note | Archive (delete, if the note is already archived) |
| <kbd>Shift Del</kbd> in the list, <kbd>Ctrl Shift Del</kbd> in a note | Delete for good |
| <kbd>Ctrl Z</kbd> outside the text | Undo the archive or the delete |
| <kbd>Ctrl Shift A</kbd> | Attach a file |
| <kbd>Space</kbd> on a file | Preview it; <kbd>←</kbd> <kbd>→</kbd> for the others, <kbd>Esc</kbd> or <kbd>Space</kbd> to close |
| <kbd>Ctrl Shift C</kbd> | Copy the note's text |
| <kbd>Ctrl R</kbd> | Sync now |
| <kbd>Ctrl ,</kbd> | Settings |
| <kbd>Ctrl +</kbd> / <kbd>Ctrl −</kbd> | Text size |
| <kbd>Ctrl /</kbd> | Show these keys |

On macOS, <kbd>Cmd</kbd> takes the place of <kbd>Ctrl</kbd>, and
<kbd>Cmd ⌫</kbd> the place of <kbd>Del</kbd>, since most Mac keyboards
have no Del key.

A key pressed while an input method is composing a character belongs to
the input method. <kbd>Esc</kbd>, <kbd>Enter</kbd>, <kbd>Tab</kbd> and
<kbd>Backspace</kbd> do none of the above until the composing has ended.

**The web view's own keys are turned off.** The app is drawn in a web
view, which has keys of its own: <kbd>Ctrl R</kbd> and <kbd>F5</kbd>
reload the page (losing what is being typed), <kbd>Ctrl F</kbd> opens the
browser's find bar, <kbd>Ctrl P</kbd> prints, <kbd>Ctrl +</kbd>,
<kbd>Ctrl −</kbd>, <kbd>Ctrl 0</kbd> and <kbd>Ctrl</kbd>-wheel zoom, and
<kbd>F12</kbd> opens developer tools. All of them are turned off in WebView2
and WebKitGTK settings in release builds, and the app's own meanings above
take their place: <kbd>Ctrl R</kbd> syncs, <kbd>Ctrl F</kbd> goes to Find,
and <kbd>Ctrl +</kbd> / <kbd>Ctrl −</kbd> change Settings → Appearance's
text size, which is kept and reflows the text, where the web view's zoom
would scale the whole window and be forgotten. The context menu is the
app's own (Copy, Paste, Select all), not the browser's.

### 7.5 Tray

The tray icon's menu has **New Note**, **Open Dirt** and **Quit**. Closing
the window keeps Dirt in the tray so the hotkey still works.

### 7.6 The window: no title bar

The system's title bar goes. The app reaches the top edge of the window:
Find sits at the very top of the list, and the window's controls have the
top row of the page to themselves; the note's icons sit beside the note
(6.4). Nothing says "Dirt"; the window is the app.

```
┌──────────────────────────────────┬──────────────────────────────────────┐
│ ( 🔍 Find                    # ) │                          │ —  ▢  ✕  │
│  Ask Jordan about the Q3…    4m  │  Note text…                       ✓  │
│                                  │                                   📎 │
```

| | Windows and Linux | macOS |
|---|---|---|
| Controls | Dirt's own −, ▢, ✕ at the top right: 46 × 44 px each, 14 px icons, after a 1 px divider | The system's red, yellow and green buttons, kept, at the top left over the list (transparent title bar, content under it) |
| At rest | 55% opacity; full on hover. ✕ turns `#C42B1C` with white on hover, as Windows' own does | As the system draws them |
| Words | Tooltips only: Minimise, Maximise or Restore, Close to tray | None |
| Find | Full width of the list | Starts 84 px in, after the buttons |
| While writing | Gone. They come back when the pointer comes to the top-right corner | The same, at the top left |

- **Moving the window:** any empty part of the top 52 px drags it. Find,
  the tag list, buttons and icons are not part of it. A
  double-click there maximises or restores, as a title bar's does.
- **Close** hides the window to the tray; Dirt keeps running so the hotkey
  works. **Quit** is in the tray menu.
- **Resizing** from the edges still works: Windows keeps its resize border
  for a window without decorations on the left, the right and the bottom,
  but at the top the web view lies over it, so the app draws the top edge
  and its corners there (7.9); on Linux the app adds invisible 6 px
  handles on each edge and corner that start the system's own resize.
- **Shadow and corners:** Windows draws its shadow (`with_undecorated_shadow`)
  and, on Windows 11, rounds the corners. The top-right ✕ follows the
  corner's curve.
- **Keyboard:** <kbd>Alt F4</kbd>, <kbd>Win ↑</kbd> / <kbd>Win ↓</kbd> and
  <kbd>Win Z</kbd> still work on Windows; <kbd>Cmd W</kbd>, <kbd>Cmd M</kbd>
  and <kbd>Ctrl Cmd F</kbd> on macOS.
- **Building it** (Dioxus on tao): `WindowBuilder::with_decorations(false)`
  on Windows and Linux; `with_titlebar_transparent(true)`,
  `with_fullsize_content_view(true)` and `with_title_hidden(true)` on
  macOS. The top row calls `window().drag()` on a press and toggles
  `set_maximized` on a double-click; the buttons call `set_minimized`,
  `set_maximized` and hide to tray. Quick capture already has no title bar.

**What it costs:** on Windows 11, hovering the maximise button no longer
offers **Snap Layouts**, because that flyout belongs to the system's own
buttons. Snapping by dragging to an edge and <kbd>Win Z</kbd> still work.
Bringing the flyout back would mean answering Windows' hit-test message for
the maximise button ourselves, which tao does not expose (section 11).

### 7.7 Focus while writing

Principle 4 says that while a note is written, only the note is on screen.
There are three ways to get there, each good at something, and the person
chooses in Settings → Appearance → **Focus while writing**. **Slide** is
the default.

Focus starts at the first pause in typing on the page (600 ms without a
key) or the first new line, whether a new thought or an old note. On the
first key only the controls fade; the column does not move under a word
being typed (5.5). During focus nothing comes back just because the mouse moved: each
control returns only when the pointer goes to where it lives (6.4), and the
layout never jumps. The list comes back by the way each style shows, or by
<kbd>Esc</kbd>, which ends any of them and leaves a blank page.

| Style | What happens | Way back | Good for |
|---|---|---|---|
| **Slide** (default) | The list slides out to the left and folds to nothing; the note's column glides to the middle of the window | Take the pointer to the left edge: a 4 px handle grows there as the pointer nears, and the list slides back. Or <kbd>Esc</kbd> | Quick thoughts: one motion out, one back, nothing left half-visible |
| **Rail** | The list folds to a 56 px rail of grains in their own shades of sand (6.17): the notes the list holds, newest at the top, as many as fit, and the rail scrolls for the rest. Where a month starts, a short line with its initial. The open note's grain is a clay bar | The chevron at the top of the rail, or <kbd>Esc</kbd> | Moving between notes while writing: a dot's tooltip gives its note's title, and a click opens that note without leaving focus |
| **Zen** | Slide, and more: the paper warms from `--surface` to `--bg`, the type grows by 4 px with more line height, the text starts lower, the top and bottom of the text fade, the files and window controls go | As Slide | Writing at length: the calmest page |

How each enters and leaves, step by step, is in 5.5 under "Focus while
writing". None needs anything from the platform.

Considered and left out: **Dim** (the list fading to 12%, which leaves
half-present text beside the note) and **Glass** (the list turning to
frosted glass over the desktop, which needs a transparent window and on
macOS a private API the Mac App Store refuses).

### 7.8 Settled when the main screen was built

What the sections above left open, settled when the desktop's main screen
was built (implementation plan, step 3a). Focus while writing (7.7), the
controls shown by nearness (6.4's second half) and the window without a
title bar (7.6) came after it, in step 3b, and what that settled is in 7.9.

| | Settled |
|---|---|
| The chosen row | The chosen row is the note on the page. Pressing a row opens the note with the cursor in its text. <kbd>↑</kbd> <kbd>↓</kbd> in the list open each note without leaving the list; <kbd>↑</kbd> on the first row goes back to Find, and <kbd>Home</kbd> and <kbd>End</kbd> go to the ends. The list is one stop for <kbd>Tab</kbd>; the chosen row's tags and box are the stops after it, and no other row's |
| Archive or delete from the list | <kbd>Del</kbd> and <kbd>Shift Del</kbd> in the list move the choice to the row below (above, at the end) and leave the keyboard in the list. From the note, or by a row's box, the page is blank afterwards with the cursor in it |
| <kbd>Ctrl Z</kbd> | Outside the text, as 7.4 says, and also on a blank page, which has nothing of its own to undo and is where the cursor is after <kbd>Ctrl Del</kbd> in a note. It undoes the last archive or delete only, once |
| A tag pressed on a row | Added to the tags chosen, as a token; the others stay |
| <kbd>↓</kbd> in Find | Into the list of tags while it is open, and into the notes otherwise |
| Layers | Only the list is in layers. While Find holds words or tags the notes are in the order found, and only the line "Archived" divides them (6.19). The list of archived notes, from "3 archived", is newest first with no layers |
| "3 archived" | Under the pile (6.17), once scrolling has reached the end of the list, and only while nothing is being found. Until the pile was built (step 7) it was alone at the end of the list |
| Load more | 100 notes at first, and 100 more each time the end of the list comes within 600 px of sight |
| The hollow dot | Only while signed in and let in. Signed out, every note is on this device alone, and the status says so ("This device only") instead of a dot on every row |
| Files on the blank page | 📎 or a drop makes a note of the files, with the tags chosen in Find as its text (6.1: a file with no words under it is a note). The list names such a note after its first file |
| The page's text | Saved 400 ms after typing pauses, and at once when the note is left (done, another note opened, archived). The first character that is not a space makes the note at once |
| Narrower than 720 px | The list takes the window. A note, or a page begun with <kbd>Ctrl N</kbd> or the + that appears at the foot of the list, comes over it; done goes back to the list. The breakpoint is the window's width |
| The app's own menu | Copy when something is selected, Paste and Select all in a text field, and "Copy the note" (<kbd>Ctrl Shift C</kbd>) while a note is open. Paste reads the clipboard in the app, since a web view may not read it without asking |
| Where the keyboard goes | When what had the keyboard is taken away (a row archived, the archived notes closed), the screen takes it, so that its keys still work |
| An archived row leaving | It folds shut where it was (on the exit curve, at most 320 ms) while the rows below rise into its place (snap), and its grain sinks into the pile (5.5, 6.17) |

**A delete undone, into its place** (found in 3a, settled in 3b). The
store's restore brought the note back with a new edit time, so its row came
back at the top of the list. Bringing a note back is still an edit: this
device's count of edits rises above every deletion marker, which is what
brings the note back on every device and sends the change. The time it
records is now the note's edit time as it was, not the time now. The edit
time is the latest time recorded, so it does not move, and the note comes
back into its place and its layer, on every device. The time decides
nothing in a merge; it is shown, and orders the list (`NoteDoc::bring_back`,
with tests in `dirt-core` and `dirt-sim`).

### 7.9 Settled when focus while writing and the window were built

What 7.6, 7.7 and 6.4's second half left open, settled in step 3b.

| | Settled |
|---|---|
| Leaving a note never waits | Done, opening another note and archiving or deleting the open one change the page at once. What the page held is taken as it was, and kept or let go afterwards, in order, by one queue of the store's work for the whole app (`screen::writing`). The first character makes the note under an ID the app makes, a UUID v7 (`Store::create_note_with_id`), so files attached in that moment go to it |
| The screen going | What the page holds is handed to the queue when the main screen goes and when Quit is chosen in the tray, which then waits up to five seconds for the queue before the app ends. On signing in or out the page is handed over for the store it was written in, the new store is taken into use, and the queue's work for the old one is done (waiting up to five seconds) before the old one is removed; the queue holds no store once its work is done |
| A write that fails | It is not tried again. The store already waits out a busy file inside every write (up to half a minute); what still fails is the disk or the file, which a moment's wait does not mend. The person is told plainly and given their words: an error, "A note could not be saved", with the store's reason in its tooltip and **Copy text**, which puts the words the write carried on the clipboard. It stays until it is dismissed, above the toast; ordinary toasts (archive, delete, undo) neither replace it nor clear it, nor it them. Several failures make one error ("2 notes could not be saved"), and **Copy text** gives the newest text of each note, a blank line between. If the note that could not be made is the one on the page, the page goes back to a thought not yet made, and the next key makes it, with all that is on the page then. A note is never made later from a failure: later work for a note that could not be made is not done |
| Esc and the next thought | Esc with no modifier, and the command key (Ctrl; Cmd on a Mac) with Enter or N and nothing else, pressed in the page empty it in the script as the key goes down, before the app hears it (`data-dui-page`, which names the command key). What is typed straight after is the next thought's. The app's rule is the same, key for key, in one place (`keys::empties_page`): were they to differ, the next character would be saved over the note, or the old text would become a second note. Done by any other way (✓, Ctrl N from elsewhere, Esc with a modifier) gives a fresh page |
| Opening a note | The list's copy of it is shown at once, so ↓ pressed twice moves two rows; the store's copy follows, and is taken if it differs and nothing has been typed yet. Text of the note still waiting in the queue is newer than either, and is what the note shows |
| When focus begins | At the first key on the page only the controls fade (200 ms): the note's icons and the window's to nothing, the list to half. Focus comes after 600 ms with no key, or at the first new line. Text that changes with no key in the 400 ms before (pasted with the mouse) brings it at once, as the mockup does |
| When focus ends | Done (Esc and the rest), a note opened from the list, the keyboard going into Find or the list, the handle at the left edge, the rail's chevron, archiving or deleting the open note. A note opened from the rail keeps it. It never ends by itself, and a mouse move alone never ends it |
| Narrower than 720 px | Only the controls fade; the list is already under the page, so there is nothing to fold |
| Nearness | A control is near when the pointer is within 64 px of its box: the note's gutter, and the window's controls as one group. The left edge's handle is 14 px wide, grows when the pointer is within 60 px of it and brings the list back when the pointer is on it. Only a pointer that moves counts, and only after focus began: a resting pointer, or a list sliding away under one, brings nothing back. A touch on a touch screen shows them all until the next key |
| The rail | The notes the list holds, newest first, with the archived ones when Find holds something, as the list does. A month's initial, over a short line, where the month changes, not above the first. Each grain is one of four slightly irregular rounds, chosen by its note's grain (6.17). The rail scrolls; its scrollbar is not drawn |
| Zen | The note's icons move down with its first line, which starts 56 px lower |
| The window's size | 1100 by 720 px at first, no smaller than 360 by 320. Where it was left (7.1) is not kept yet |
| Resizing (Windows) | The system's border is outside the page on the left, the right and the bottom. At the top the web view lies over it, so the app draws a 5 px top edge and 8 px corners, and a press there hands the resizing to the system |
| The top row | A press on an empty part of the top 52 px moves the window; a second press within 500 ms and 5 px of the first on the screen is the double-click, and maximises or restores. A pointer's press carries no count of clicks, so the script counts them; on the screen, not in the page, since after a short drag the window has moved with the pointer |
| Close | ✕ hides the window to the tray, as the system's close did, with the tooltip "Close to tray" and <kbd>Alt F4</kbd> |
| Linux and macOS | Keep the system's title bar for now: the window without one has not been tried there (the plan's fallback; manual checklist D44). The Linux handles and the macOS traffic lights of 7.6 come when they can be tried |
| Snap Layouts | Built without the flyout, the default of the owner's decision (section 11). Snapping by dragging to an edge, and <kbd>Win Z</kbd>, are the system's and still work |
| The style | Settings → Appearance → Focus while writing, kept on the device as `focus_style` |
| Find beside the window's controls | Narrower than 720 px the list reaches the top right corner, where the window's controls are, so Find stops short of them there |

### 7.10 Settled when quick capture was built

What 7.2 and 5.5 left open for quick capture, settled when it was built
(implementation plan, step 8a). Holding the keys to talk comes with voice
(step 9).

| | Settled |
|---|---|
| The window | A window of its own, made hidden when the app starts and kept, so that opening it is only showing it. It has no title bar, no taskbar button, and stays over other windows. It is as large as the box and the room the box's shadow needs, and is drawn nowhere else: the box is 560 px wide, and the window grows downward with it, line by line, to twelve lines, after which the text scrolls |
| Where it opens | In the middle of the screen the pointer is on: the box with three lines has its middle there, and grows downward |
| What is in it | The logo, then the text with its placeholder, "What's on your mind?". No other words, buttons or hints |
| Saving | As the page saves (7.8, 7.9), through the same queue: the first character that is not a space makes the note, what follows is saved 400 ms after typing pauses, and putting it away hands over what it holds. So the note is in the main list while it is written, and nothing typed is lost if the app ends. With nothing written, or spaces alone, putting it away makes nothing, and the box only fades |
| The keys | <kbd>Esc</kbd> with no modifier, and the command key with <kbd>Enter</kbd>, put it away; <kbd>Enter</kbd> alone is a new line. A key pressed while an input method composes is the input method's (7.4). <kbd>Ctrl N</kbd> does nothing here |
| Where the keyboard goes | Back to the window that had it before quick capture opened, as soon as it is put away by a key, not when the grain lands: what is typed meanwhile is for that app. Put away by a click outside, the keyboard is where the click put it |
| Opened again | While it is open, the keys or **New Note** give it the keyboard again. While its grain is in flight, the flight stops and a new box opens |
| The grain | The note's own grain, in its shade (6.17): the box's words fade, and the box closes into a round of that shade as it shrinks. The mockup's box shrank to a white oval at 35%; a grain of the note's colour says better what it became |
| The flight | It is the window that moves: Rust carries it along the two curves of 5.5 so that the grain, in its middle, lands on the middle of the tray icon, where the ring pulses. The page says when the box begins to shrink, and Rust moves the window from then; with reduced motion the page only fades and says nothing, so the window does not move. The tray icon's place is asked of the system each time |
| Where the tray's place is not known | The plan's fallback: the grain falls 140 px on gravity's curve (5.4), as an archived note's grain sinks, and fades, where the window was. That is Linux, where the tray does not say where it is, and a tray on a screen of another scale than quick capture's, where the window would be drawn again in mid-air |
| macOS and Linux | Not tried yet (manual checklist D68): there the keyboard is not handed back at once, so the window hides as soon as it is put away, with no flight, and the system gives the keyboard to what is in front |
| A write that fails | Told on the main screen, as every failed write is (7.9), with **Copy text**. The main window comes back for it if it was closed to the tray or minimised, and asks to be looked at (the taskbar flashes) if it is behind another window |
| Closing it otherwise | <kbd>Alt F4</kbd> puts it away, saved, and it hides at once. Quitting from the tray hands what it holds to the queue, which the app waits for (7.9) |
| How quickly | From the key reaching the app to the text field having the keyboard, measured on the development build on Windows 11 (implementation plan, step 8a). The hotkey is heard at once; the tray's menu and showing the main window are still looked for every 16 ms |

---

## 8. Android

### 8.1 Starting

Opening the app shows the list, with the dock at the bottom: the bar
"What's on your mind?" and the mic. Nothing is focused, so the keyboard
doesn't cover the list. One tap on the bar opens a page with the keyboard
up; holding the mic skips the keyboard altogether. Every way in that means
"write" goes straight to somewhere to write:

- The **home-screen widget**: the bar, with a mic (8.5).
- A **hardware key**, where the phone lets one open an app: straight into
  listening (6.18, 8.6).
- **Long press on the app icon → New note** (an app shortcut).
- **Share → Dirt** from any app: text or pictures are saved as a note at
  once, with a toast and **Undo**, and the share closes. No screen to
  confirm.
- The **lock screen**, where the phone allows it (8.6).

The widget, the shortcut and the lock screen all open the **capture
sheet** (8.5), not the app.

### 8.2 Gestures

| Gesture | Does |
|---|---|
| Tap a card | Open it |
| Tap the bar | A new page, growing out of it |
| Hold the mic | Talk; let go to keep, slide up to lock, slide left to throw away (6.18) |
| Tap 🔍 | Find slides down under the title |
| Swipe a card left | Archive it, with **Undo**. An archived card is deleted |
| Tap a tag on a card | Narrow the list to it |
| Pull the list down | Sync now |
| Long press an icon | Its tooltip |
| Back, or the back gesture, in a note | Close it into its card. A new page drops onto the list as a grain |
| Drag Settings' top edge down | Close Settings |

### 8.3 The note

Full screen, the text first. Tapping the text puts the cursor there. Top
bar: back (⌄ on a new page), mic, attach, archive (on an archived note,
"back to the list" and delete). Files under the text. It
saves as you type; there is no Save button and no "Saved".

### 8.4 What changes from today

- The Save button goes: Android saves as you type, like the desktop.
- The sync line above the list goes: the sync icon carries it.
- "Show older notes" goes: the list loads more as it nears the end.
- Theme (Auto, Light, Dark) and text size come to Android.
- Swipe to archive, pull to sync, and an app shortcut are added.
- The widget opens the capture sheet instead of a blank note in the app.
- The field at the bottom becomes the dock: a bar that opens a page, and
  the mic. Search moves behind 🔍 at the top.

### 8.5 The widget and the capture sheet

**The widget** is one bar, four cells wide by default and resizable
sideways:

```
╭──────────────────────────────────────────────╮
│ [🌱]  Write a thought                   (🎙) │
╰──────────────────────────────────────────────╯
  logo: opens Dirt   the bar: the capture sheet   🎙: the sheet, listening
```

- Pill shape, 28 dp radius, `--surface` with a `--border` hairline. It
  follows the system's light and dark themes.
- Three targets: the logo opens the app at the list; the bar opens the
  capture sheet; the clay mic opens it listening, hands-free (6.18).
- It shows no notes. A widget is seen by anyone who sees the home screen,
  and Dirt's notes are private.

**The pile widget**, 2 × 2 and optional, puts the person's pile (6.17) on
the home screen, with one line: "1,254 thoughts". After a thought is saved from the
capture sheet it is redrawn with one more grain: a home-screen widget
cannot play the falling motion. It is drawn from how many notes
there are and their shades only, never their words, so it is as private as
the bar. Tapping it opens Dirt.

**The capture sheet** is a small screen of its own, drawn over whatever was
on screen, with the keyboard already up. The full app does not start. That
is what makes it fast: the sheet is ready to type into almost at once,
where starting the app and its web view takes a second or more.

```
         (the home screen, dimmed)
╭──────────────────────────────────────────────╮
│ What's on your mind?                🎙  (✓) │
╰──────────────────────────────────────────────╯
│              keyboard                        │
```

| | |
|---|---|
| Opens | The sheet rises from the bottom as the screen behind dims (400 ms, `--e-out`); the keyboard rises with it |
| Typing | The ✓ springs in when there is text. Return is a new line |
| 🎙 | Listens, in the same sheet. ✓ keeps what was said as a voice note, whose words are made afterwards (6.18) |
| ✓ | Keeps the note. The sheet lifts 24 dp and fades (240 ms, `--e-in`); nothing says "Saved", and if the pile widget is on the home screen, it gains the grain. The person is back where they were: two actions plus typing |
| Back, a tap above the sheet, Home | Closes it; what was written stays as a draft and is there next time |
| Its words | Only the placeholder. No title, no buttons but 🎙 and ✓ |

The note is kept on the phone the moment ✓ is pressed, and it reaches the
person's other devices without Dirt being opened (decided 2026-11-16; the
plan's second trial built it on the emulator). At ✓ the sheet writes the
thought to a file of its own in Dirt's private storage, named by the
note's ID, which it makes then, so the note is dated when it was written.
It then asks Android for one short job that makes the thought a note and
syncs once, with no web view: Dirt's core, loaded on its own. The job
needs a network, so a thought kept offline waits for one. It is started by
the capture, never by a timer, so the rule that nothing syncs on a timer
holds. If Android stops the job half way it runs it again; and the app,
each time it starts or the person returns to it, makes a note of anything
still waiting. A thought is never lost in between, and never made twice.
A recording from 🎙 waits the same way, as a file beside the typed ones,
and the same job makes it a note; its words are made by a second job on
the same phone (6.18).

### 8.6 The lock screen

Writing a thought without unlocking the phone is the fastest capture there
could be, and it is partly possible. Android does not let an app put a
widget or a button of its own on a phone's lock screen. These are the ways
in, and the design uses all of them:

| Way in | Where it works | Opens without unlocking? |
|---|---|---|
| A **Quick Settings tile**, "New note", pulled down over the lock screen | Every Android phone | Depends on the phone: some open the sheet over the lock screen, others ask to unlock first. Android's own emulator opens it over the lock screen; phones have to be tried |
| Android's **note-taking role** (Android 14 and later): Dirt chosen as the phone's notes app, opened by the lock screen's note shortcut or a stylus button | Phones and tablets that offer the role, mostly ones with a stylus | Yes: this is what the role is for |
| A **lock-screen shortcut** to Dirt, on phones that let any app be one (Samsung's One UI does) | Those phones | Only if Dirt's sheet is allowed to show over the lock screen, which it would be |
| A **lock-screen widget** | Some tablets on Android 15 and 16; not phones | Yes |
| A **hardware key** set to open Dirt's **Talk** (double-press the side key on Samsung, Quick Tap on Pixel) | Those phones | Depends on the phone, as the tile does. Where it does, nothing on the screen needs touching: it listens, and a pause keeps the thought |

Over the lock screen the sheet is the same, with one quiet line above the
field: "Adds a note. Your notes stay locked."

The rule that makes this safe: **over the lock screen Dirt can add and can
show nothing.** Whoever holds a locked phone can write into it and cannot
read a note, see the list, or open the app. A draft left earlier is not
shown over the lock screen; one written there is added after it, and both
appear the next time the sheet opens on an unlocked phone. Opening the app
from there asks for the phone to be unlocked.

A thought written over the lock screen becomes a note and syncs at once,
as one written anywhere else: once the phone has been unlocked after it
started, Android keeps Dirt's storage open while it is locked, and adding
and syncing show nothing (found on the emulator by the plan's second
trial). It does not wait for the phone to be unlocked. Android would not
tell Dirt when that happens unless Dirt were already running, and waiting
would make nothing safer.

The keyboard over the lock screen is the phone's, not Dirt's, and may
offer its own buttons there, the clipboard's among them. The field asks
the keyboard not to learn from what is typed into it over the lock screen.
What each phone's keyboard shows there has to be tried.

Not possible: capture right after the phone restarts and before it is
first unlocked. Android keeps every app's storage locked until then, and
the tile says it is unavailable.

### 8.7 Settled when the main screen was built

What the sections above left open for Android, settled when its main
screen was built (implementation plan, step 4). Where 7.8 settled the same
question for the desktop, Android follows it: load more (100, and 100 more
within 600 px of the end), "3 archived" under the pile, the hollow dot only while signed in and let in, layers only in the
list, the page's text saved 400 ms after typing pauses and at once when it
is left, a file chosen on a page with no words making a note of the files
with the tags chosen in Find as its words.

| | Settled |
|---|---|
| Opening a note | The note grows out of its card with the cursor at the end of its text and the keyboard up, so that "tap it, type" is two actions (section 2). Tapping elsewhere in the text moves the cursor there (8.3). Back first puts the keyboard away, as Android does, and then closes the note |
| Leaving a page | The page closes the moment ⌄, ‹ or Back is pressed, and what it held is saved afterwards from what it held then. A page opened straight after is a page of its own: nothing typed into it is lost to the one being saved, and a note whose first character is still being made is made and saved all the same |
| A new page let go | With words, it closes to its grain, which is tossed onto its card at the top of the list (5.5, 6.17). With nothing written it goes back into the bar. Until the grain was built (step 7) it closed into its new card, as an old note closes into its own |
| Back | Closes, in this order, whatever is open: a confirmation in Settings, Settings, the page, the list of archived notes, the strip's offers, then Find. With nothing open the app goes behind the others and keeps running, as Back does on the home screen |
| Find | Its cross, or Back, closes it, and the list is whole again: its words and tags go with it. While it has the keyboard the dock goes, as the keyboard would cover it. The search key on the keyboard opens the first note found, as Enter does on the desktop; Backspace in an empty Find takes back the last tag |
| A tag tapped on a card | Brings Find down with the tag as a filled pill under it, where it is seen and taken back, and narrows the list (one action) |
| The page's bar | Fades on the first key typed, and comes back when the keyboard goes away or the page is touched anywhere; a touch that brings it back does not also press what is under it. While it is faded its buttons cannot be pressed |
| Archive and delete from the page | The page goes back into its card first, and then the card is put away: it folds shut (archive) or tears away (delete), with the toast. A note emptied on the page is let go instead, with no toast |
| A card swiped away | Once it has flown off, the gap it leaves folds shut, archive or delete alike, and the cards below rise into it (snap) |
| Pull to sync | The list holds at 58 px for as long as the sync runs, at least 600 ms and at most 15 s. Pulled while signed out, the arrow turns and settles and nothing syncs; the sync icon says why. A pull is told from a scroll by the finger's first move: down, with the list at its top. It is read from touch events, because Android's WebView takes a vertical drag of pointer events for its own scrolling |
| The sync status | An icon with no words (6.6), so a long press gives them: its words and then its tooltip ("Offline · 1 waiting. Notes wait here and go when you're back"), or the tooltip alone for Synced. A tap syncs; with a problem it opens Settings at Sync, signed out at Account, and waiting to be let in at Devices (9.7) |
| A card | Its tags have 40 dp targets around their small type. A long press anywhere else on it gives its edit time in full. Its thumbnail is shown only when the device already has the picture's thumbnail; the list fetches none, and opening the note fetches them as its files do |
| The mic | The dock is the bar alone, and a page's bar has no 🎙, until voice (step 9): no icon is shown that does nothing |
| Appearance | Settings → Appearance (9.7): the theme, the accents and the text size |
| A share | Saved at once as before, with no toast yet: the toast after a share, with **Undo**, comes with capture from outside the app (step 8). One that cannot be saved gives the error below, with the shared text to copy |
| A write that fails | As on the desktop (7.9): not tried again; the error "A note could not be saved" ("2 notes could not be saved"), the store's reason in its tooltip (a long press), and **Copy text**, which puts the newest text of each page that could not be saved on Android's clipboard. A page whose note could not be made goes back to a thought not yet made, and its next key makes it with all of it; leaving it makes nothing. An archive, a delete or an undo that fails says so the same way ("The note could not be archived", "… deleted", "… brought back"), with nothing to copy |
| One note for a page | Whatever makes the page's note (its first character, files chosen on it) asks its draft first, so a page makes one note: files chosen while the first character's note is being made wait for it and go to it, and a key typed while the files' note is being made is saved into it |
| Touch and hover | A finger leaves `:hover` on what it tapped, so where nothing hovers (`hover: none`) an icon button is grey only while it is pressed |
| Settings over the main screen | The sheet (9.7). The main screen under it is inert and hidden from screen readers, and steps back (5.5). Toasts stay above it, as they are above the desktop's Settings: an **Undo** just offered can still be pressed, and an error is never hidden |
| The cards' files | The list asks the store which of the notes it shows have files, with each one's revision (`Store::file_revisions`), and reads a note's files only when its revision changed. A thumbnail is kept while a card shows it, and nothing is kept for another account |
| A second finger | A new touch while the list is being pulled ends the pull: it settles back, as one let go short of the mark does |

**What did not work as the design assumes.** The page is hidden with
`display: none` when it is closed, not with `visibility`: Android's WebView
(133, on the emulator) was seen to keep a child of the page hidden after
it had opened again, its inherited visibility not worked out afresh once
the clip had run. And a tap that reaches the page within a few
milliseconds of Back, faster than a hand can, lands before Back has closed
the page, so what is typed then goes nowhere; at a hand's pace it goes to
the new page.

---

## 9. Flows

### 9.1 Signing in

Settings → Account → **Sign in**. The system browser opens Clerk; when it
returns, the button has shown "Waiting for browser" with a spinner the whole
time, so nothing seems stuck. "Bring the 12 notes on this device into the
account" is checked by default. Back in the app, sync starts on its own.

### 9.2 Letting a new device in

1. The new device signs in. It waits, and shows the seven pictures once a
   device already in answers.
2. Every device already in shows the banner "Work laptop wants to join".
3. **Review** opens Settings at Devices with the same seven emoji.
4. **They match**. The tick draws itself; the device appears in the list.

The new device's screen says where to go: "Open Dirt on a device that's
already in." Opening it there syncs, and the banner appears.

Two actions on the device already in. Settings → Devices → **Add a device**
does the same from the other side: it waits ("Sign in to Dirt on the new
device. It shows up here by itself.") and then compares.

### 9.3 Recovery phrase

Settings → Devices → Recovery phrase → **Make one**: the 24 words, numbered,
in four columns, with **Copy**, **Save as a file** (the owner asked for it,
2026-11-18) and "I've written it down". Getting in with the
phrase is offered on the new device's own screen, where it waits for a
device to let it in: "No other device? Use your recovery phrase."

### 9.4 A note that can't sync

The sync status turns amber, "1 note can't sync". Clicking it opens Settings
→ Sync, which names the note and gives the reason and what releases it, in
the overview's words. Clicking the note closes Settings and opens it, ready
to edit.

### 9.5 Offline

Nothing interrupts. The status says "Offline · 2 waiting"; each waiting note
has its hollow dot; files say "on this device only". When the connection
returns and the app is next used, sync runs and the dots disappear.

### 9.6 Export

Settings → Export: choose JSON or Markdown, **Export**, choose where. The
button fills as it works; a toast says how many notes went where. Files that
couldn't be had are listed under the button, as a short list with reasons,
since a toast is too small and too short-lived for them (9.7).

### 9.7 Settled when Settings was built

What 6.8 to 6.13 and 9.1 to 9.6 left open, settled when Settings was built
(implementation plan, step 5). The page, its sections, its rows and the
banner are `dirt_ui::settings`, the same for both apps; the desktop puts the
page in its panel beside the rail, Android in its sheet.

| | Settled |
|---|---|
| Where it opens | The gear and <kbd>Ctrl ,</kbd> open it where it was left. The sync status opens it at Sync when there is a problem, at Account when signed out, and at Devices on a device waiting to be let in. **Review** opens it at Devices, comparing. Signing in and then having to wait jumps to Devices |
| The rail | Its highlight is on the last section whose top has passed the top of the page, and on the last section once the page is at its end, as the mockup has it |
| Counting the actions (section 2) | Scrolling the page is not counted, as the mockup's own count does not count it: Export and the theme are "Settings, then the control", wherever the page is. With the rail instead of the wheel it is one more |
| The account | The device knows the account by its ID only, not by an address (the server keeps none, `SECURITY.md`), so the row says "Signed in" and "🔒 Encrypted on your devices", or "Waiting to be let in", and shows no address. A device that has to sign in again has a row of its own with **Sign in** |
| Waiting for the browser | Pressing the button again while it waits stops waiting (its tooltip says Cancel): a sign-in can fail in the browser and never come back |
| Bringing notes in | The box shows only when there are notes on the device, and is checked. A device that has to wait to be let in brings them in once it is in. Unchecked, or later, Account has a row "12 notes from before you signed in" with **Bring them in**. When notes come in the app reads its list again at once: offline, no sync comes to do it |
| Signing out | No question, unless it would lose notes or files that have not reached the server: then the row asks, in the overview's words. **Sign out** there asks the session again first, which syncs; it goes ahead only if what would be lost is still what was said, and otherwise asks again with the new numbers. Refused on the only device that can read the account, with the reason |
| Account actions | A disclosure under the account's card, closed at first: change the key, start the account again, delete the account, each a confirmation in its row. A device waiting to be let in is offered only starting again, which then waits (the server's seven days). A reset that somebody asked for is not hidden in it: it is a row of the account's card, with when and **Refuse**, since it needs the person |
| The devices | Each by its name, a phone's icon for a name that says Android or phone and a computer's otherwise: "This device", "Waiting to be let in" (with **Let it in**), or "Seen 4m ago" in the list's own words of time (with ✕, which asks in the row) |
| Add a device | Looks every two seconds, while Settings is open and for at most five minutes, as an exchange does, and compares with the first device that waits |
| Answering the code | **They match** sends the answer as it is pressed, naming the code drawn with it; the pictures gather while it goes. After an answer both buttons wait, so a second press sends nothing. If the code changed between the drawing and the answer, the core refuses it, and the comparison comes back with the new pictures and the line "The pictures changed: the other device began again, and nothing was sent. Compare these." The digits are under the pictures, small, with a tooltip saying what they are for |
| Being let in | On a device that waits, the exchange begins by itself while Settings is open, and stops when it closes. It does not know the other device's name, so it asks "Does your other device show these?". After **They match** on one side: "Waiting for … to say they match too" |
| How it ends | The tick stays 1.7 s and the list takes its place. A cross stays until **OK**. When the pictures differed, the cross says what `SECURITY.md` promises: someone may be between the devices; start again on a network you trust. A device turned away is signed out at once, so it is told by an error that stays, not in the section that has gone |
| Saving the phrase | **Save as a file** writes `dirt-recovery-phrase.txt` where the person chooses: the words numbered, then on one line to type, then one paragraph on what they open and how to keep them. The toast names the file and says to keep it somewhere safe. `SECURITY.md` says what such a file exposes |
| The recovery phrase | Its row says "You have one" or "Not made yet", with **Make one** or **Remove** (which asks). The words leave the screen when "I've written it down" is pressed or Settings closes, and are not shown again. On a device that waits, "No other device? Use your recovery phrase." is offered only when the account has one; with no device to let it in and no phrase, a sentence points to starting again. **Use the phrase** waits for 24 words |
| Sync | The head of the section is the overview's one sentence for everything (`SyncOverview::headline`), with "Last synced 2 minutes ago" under it and **Sync now** beside it. Its icon never shows "just synced": the page is not drawn again when that ends. Under it, each note that cannot sync, which opens with Settings closed, and each file refused, which opens its note. Signed out, or waiting, it is one sentence from the overview |
| Appearance | The theme's segments have their words on the desktop and are icons alone on Android, the words in their tooltips. Focus while writing, and the quick-capture keys, are the desktop's only |
| Export | JSON is chosen at first. The toast names the folder made, not its whole path |
| What goes wrong | An action that fails says so in an error that stays, its reason in the tooltip, as 7.9's errors do; one that worked says so in an ordinary toast |
| Questions left open | Settings closing takes every confirmation away (sign out, the account's actions, removing a device or the phrase), with the word typed for Delete: a question is answered while it is in sight or not at all. Every button that sends something waits while it is sent, so a second press sends nothing |
| Focus | As 12 asks. A confirmation that goes gives the focus back to its row, and focus that falls out of the panel or sheet goes back into it, never to the screen behind |
| Android | The page is the same, in the sheet (6.9), which a drag down, a tap above it or Back closes; the screen behind steps back. **Sign in** opens a browser tab over the app, and waits until the app has finished the sign-in from the address the browser comes back to, also after Android stopped the app meanwhile. The recovery words are in three columns. A row with two controls (Export) has its words on a line of their own and the controls under them, and a confirmation its question above its buttons. The export is one zip file saved where the person chooses on Android's own screen; its toast names the file, and backing out says nothing. **Copy** marks the phrase sensitive (`SECURITY.md`) |
| The banner on Android | Drops just under the top bar, not over it: Android's banner has no close, and over the top bar it hid the gear, Find and the sync status for as long as a device waited, while a tap meant for the gear pressed **Review** |
| The banner | Shows the first device that waits and has not been dismissed. It is hidden while Settings is open, and asks again as Settings closes, since the device may have been let in there |

**What did not work as the design assumes.** The core's code was three
groups of four digits; the seven pictures of 6.13 needed it to give them
(`ShortCode`). The digits come from the same bytes, so until every build
shows pictures the comparison has the digits too, small under the
pictures: a build from before shows only digits. And "they match" now
names the code it was said of: the core refuses an answer for a code that
has changed since, and the screen then says the pictures changed and shows
the new ones (`ExchangeView::CodeChanged`). And a long toast at the
bottom of the window lay over the list's foot, where the gear and the sync
status are; a pointer resting there held the toast, and a press meant for
the gear landed on it. On the desktop toasts now sit 58 px up, clear of
the foot (6.7's "bottom centre", lifted).

---

## 10. Building it in Dioxus

Both apps draw through a webview (WebView2 or WebKitGTK on the desktop,
Android's WebView on the phone), so the whole system is CSS.

- **One style sheet** of tokens and components, shared by `dirt-desktop` and
  `dirt-mobile`. Platform differences are classes on the root (`.desktop`,
  `.android`), not two style sheets.
- **Tokens as custom properties.** Dark theme under
  `[data-theme="dark"]` and under `prefers-color-scheme: dark` unless
  `[data-theme="light"]`, exactly as in the mockup.
- **Transitions and keyframes in CSS** for hover, press, tooltips, sheets,
  panels, toasts, segmented controls and disclosures. Components only add and
  remove classes.
- **FLIP for the list.** The script remembers where each row was laid
  out. It watches the page (a `MutationObserver`), and when the rows under
  a list change it measures them again, before the change is drawn, and
  plays the difference with the Web Animations API. Rust only changes the
  page: it cannot measure the page before it changes it, so it is not
  asked to. Only rows that were in sight or come into it are moved: a list
  may hold hundreds. One small script, shared by both apps.
- **The note growing from its card** is a `clip-path` animation from the
  card's rectangle to the screen, also one small script.
- **Gestures** (swipe, pull, sheet drag) with pointer events, writing
  `transform` directly while the finger is down and handing back to CSS on
  release. They run in the script from first touch to release. Rust is
  told once, how it ended.
- **Reduced motion:** read `prefers-reduced-motion` and shorten the duration
  tokens. Components need no special cases. Android's "Remove animations"
  reaches the WebView only when it starts (5.7).
- **Fonts** bundled as assets.
- **The web view's keys, zoom and context menu off** in release builds
  (7.4). On Windows the web view's settings do it: wry turns WebView2's
  `AreBrowserAcceleratorKeysEnabled` and `IsZoomControlEnabled` off, and
  Dioxus keeps the developer tools out of a build that turns the context
  menu off. The menu itself, and the same keys on a web view whose
  settings do not cover them (WebKitGTK, macOS), are stopped by a short
  script in the page's head, which lets each key go on to the app.
- **Voice:** each app reads the microphone (Android's `AudioRecord`, the
  desktop through `cpal`) and hands the sound to `dirt-voice`, which says
  when the speaker has stopped, writes the recording, and afterwards turns
  it into words. The settling grain and the
  listening grains are small scripts beside the list's FLIP.
- Rules from CLAUDE.md still hold: the words about sync come from
  `dirt_core::sync::overview`; no SQL, HTTP or cryptography in the client
  crates.

What is shared is the crate `dirt-ui`. Its classes begin with `dui-`, so
that they cannot meet a class of a screen not yet replaced, and its base
styles reach only what is drawn inside its root component. The script
finds what Rust renders by attribute (`data-dui-list`, `data-key`,
`data-tip`), so no component calls it. The style sheet is in the page
beside what it styles, so that it arrives with the first render and
nothing is drawn unstyled. A gallery of every component is built with each
app's `gallery` feature, for development only.

---

## 11. What this design asks of the rest of the system

Each needs a decision or a change outside the interface before it can be
built.

| Ask | Why | Where |
|---|---|---|
| **Bring a deleted note back.** Expose the store's existing restore to the clients, for **Undo** | Undo replaces the confirmation dialog, which is what makes delete one action. The CLI has it since step 1 of the plan; the apps get it with their main screens | `dirt-core` store, the desktop and Android apps, the CLI (`dirt restore <ID>`), feature-parity.md |
| **Short words for each sync state** in `dirt_core::sync::overview` ("Synced", "Offline · 2 waiting", "1 note can't sync") beside the full sentences | The sync status shows one or three words; CLAUDE.md says the words come from the overview, not from a client. The core has them since step 1 of the plan (`SyncIndicator`, 6.6); the apps show them with their main screens | `dirt-core::sync::overview` |
| **Android: save as you type** | Removes the Save button and an action from every edit | `dirt-mobile` |
| **Android: theme and text size** | Parity with the desktop | `dirt-mobile`, feature-parity.md |
| **Android: the capture sheet** (8.5), a native screen kept outside the web view so that it opens at once; a folder in the app's storage where a thought waits, as a file named by its note's ID; and a JobScheduler job that loads the core without the web view, makes the waiting thoughts notes and syncs once. The app takes in what still waits when it starts and when the person returns. The plan's second trial built all of it on the emulator | Capture in three actions from outside the app, without waiting for the app to start, and the thought on the other devices without the app being opened | `dirt-mobile`; `dirt-core`'s store makes a note under an ID it is given |
| **Android: the widget, the app shortcut, a Quick Settings tile, and the note-taking role** (8.5, 8.6) | Every way in to the capture sheet that Android offers, the lock screen among them | `dirt-mobile` |
| **SECURITY.md: capture over the lock screen** | It adds a way to write into the phone without unlocking it. What it may and may not show (8.6) belongs in the security document | `docs/SECURITY.md` |
| **Try lock-screen capture on real phones** | Whether the tile opens over the lock screen is each phone's choice, and so is what its keyboard shows there. The plan's second trial answered both only on the emulator | `docs/MANUAL_CHECKLIST.md`, L1 to L9 |
| **Android: share saves at once** with **Undo**, for text as well as pictures | Already true; keep it, and add the toast | `dirt-mobile` |
| **Desktop: drop files to make a new note**, and paste a picture | Attach in one action | `dirt-desktop`, `dirt-core::files`. Built in step 6: a pasted picture is attached from memory (`Store::attach_bytes`) |
| **Load more on scroll** instead of "Show older notes" | One less button; the store already pages | Both apps |
| **A file's decrypted bytes in memory**, from `dirt-core`, not only as a file on disk | The preview shows a file without writing a copy out (6.16). Decryption stays in the core: no cryptography in a client crate. The core has it since step 1 of the plan (`Session::file_bytes`) | `dirt-core::files`, both apps |
| **PDF in the preview: bundle a renderer or not** | WebView2 draws PDFs; WebKitGTK and Android's WebView do not. Bundling pdf.js (a few MB) makes it the same everywhere; not bundling means PDFs open in another app on Linux and Android | Decided 2026-11-18: bundled. Built in step 6: Mozilla's PDF.js 6.4.299, its build for older browsers, Apache 2.0. 3.5 MiB in 192 files: the renderer 0.50 MiB and its worker 1.26 MiB, the standard fonts 0.19 MiB (Foxit's; the GPL's Liberation Sans left out), Adobe's CJK character maps 1.11 MiB, and the image decoders 0.45 MiB. In `dirt-ui/assets/pdfjs`, loaded when a PDF is first shown; a PDF's scripts never run (no scripting sandbox is bundled, and pages are drawn to a canvas with no annotation layer); the CLI and the server carry none of it (`dirt-ui/assets/README.md`). Every PDF is drawn by it, on Windows too, so that it looks the same everywhere and needs nothing written to disk |
| **SECURITY.md: the preview** | A previewed file stays in the app's memory; **Open in another app** writes a decrypted copy and lends it. The document should say which does which | `docs/SECURITY.md`. Done in step 6 |
| **Drawn scrollbars** (6.15) | One look on every platform, and none of the space a system scrollbar takes. One small script shared by both apps, as the list's FLIP is | Both apps |
| **No title bar on the desktop** (7.6) | The app reaches the top edge; one row less of chrome and one word less ("Dirt"). Windows and Linux draw their own controls; macOS keeps the system's | `dirt-desktop` |
| **The motion system** (5): three springs as `linear()` easings, choreography from cause to destination, the View Transitions circle for theme and accent | Movement that says where things come from and go, with one feel across both apps | Both apps: one CSS file of tokens and one small animation script |
| **Focus while writing** (7.7): Slide, Rail and Zen, chosen in Settings, Slide by default | Only the note while it is written, three ways | `dirt-desktop` |
| **The page: a new note from its first word** (6.1) | The note exists before it is "done". One emptied again, with no files, must be let go without a word: taken off the device altogether if it never reached the server, deleted like any other note if it did | `dirt-core` store, both apps |
| **The note's icons beside its column, shown by nearness** (6.4) | The icons belong to the note; nothing comes back on a stray mouse move | `dirt-desktop` |
| **Grains, layers and the pile** (6.17) | 積沙成塔: the list as a pile, newest on top, and the pile drawn as a heap of sand, one grain per thought. Derived from the notes; nothing new is stored. The core gives every note in one call, each grain's shade, the layer of an edit time and the numbers of the count line since step 1 of the plan | `dirt-core` store and models, both apps |
| **Archive** (6.19): an `archived` value in each note's CRDT document, derived into its row like the deleted flag. The list leaves archived notes out; search and export keep them | Clearing a done thought without losing it. In the note's document it syncs and merges like any edit, and the server sees nothing new | `dirt-core` notes and store, every client, the CLI (`dirt archive`, `dirt unarchive`, `dirt list --archived`), feature-parity.md. Archiving is not an edit (6.19) |
| **Tags: counts for the notes left** (6.3): how many notes match the current words and tags plus each other tag, and "any of them" as well as "all of them" | The tag list that stays open, and never offers a tag that would empty the list. The store has it since step 1 of the plan (`Store::tags_to_narrow`, `find_notes_by_tags`) | `dirt-core` store: one grouped query, which starts at the full-text index when there are words and at the tag table when there are none |
| **Voice: speech to text on the device** (6.18). One engine used by both apps that reads a finished recording once and gives its words, as accurately as a phone allows, and Silero's voice-activity model for the end of a thought. Words as they are heard were given up on 2026-11-17, so the streaming model the plan's first trial chose no longer holds; the trial's second measurement chooses the engine and the model (implementation plan, "Trial 1"). A phone is still to be measured. Its model, Parakeet TDT-CTC 110m (130 MiB), is shipped inside each app (decided 2026-11-18); nothing is fetched | Voice without anyone's voice leaving the device. The engine is native code: prebuilt libraries of 21 MiB on Windows and 25 MiB for each Android ABI, fetched when it builds and shipped beside the app. So it sits in a crate of its own: the CLI, the server's tests and most CI jobs must not build it | `dirt-voice`, a new crate, kept out of the workspace's members and built only when an app's `voice` feature asks for it; both apps, which ship the model |
| **Which speech model, downloaded or shipped** | English comes first (decided 2026-11-16), and accuracy before size and speed (decided 2026-11-17). The plan's second voice measurement puts a default, a smaller and a larger model to the owner, each with its size, its mistakes and how long it takes. Downloading on first use keeps the apps small; shipping makes voice work offline from the first second | Decided: Parakeet TDT-CTC 110m (2026-11-17), shipped in the apps (2026-11-18) |
| **Microphone access**: Android's `RECORD_AUDIO`, asked on first use; the desktop's microphone, with macOS's usage string | Voice | `dirt-desktop`, `dirt-mobile` |
| **A held hotkey**: <kbd>Ctrl Alt N</kbd> must report its release as well as its press | Hold to talk from any app. The `global-hotkey` crate reports both; Linux under Wayland offers no global hotkeys at all, held or not | `dirt-desktop` |
| **Android: a Talk entry** the hardware shortcuts can open, straight into listening, over the lock screen | Capture without touching the screen | `dirt-mobile` |
| **SECURITY.md: voice** | Where speech becomes text, that a kept recording is an encrypted file like any other, and that over the lock screen voice can add and show nothing | `docs/SECURITY.md` |
| **feature-parity.md: the page, Find and voice** | New rows for every client, the CLI included (`dirt` already takes a thought as an argument) | feature-parity.md |
| **A crate for what the two apps share**, `dirt-ui`: the tokens, fonts, icons, the motion script and the components | Section 10 asks for one style sheet and shared scripts. Today the two apps share no interface code, and would build everything twice | A new crate, CLAUDE.md |
| **Android: more Kotlin than one file, and a folder of resources** | The capture sheet, the widgets, the tile, the shortcut and the Talk entry are native screens and services. Today the build takes one Kotlin file and a manifest, and writes two resource files from the build script. The plan's second trial found the way: one Gradle library module, `android/native`, which `dx` copies into its project because `src/main.rs` names it with `#[manganis::ffi]`; it holds any number of Kotlin files, resources, a manifest of its own and Gradle dependencies | `dirt-mobile`'s build |
| **The desktop's choice of font goes**; text size stays | One typeface, bundled, is part of the design (4.2) | `dirt-desktop`, feature-parity.md |
| **Other languages, later** | Find's rule of words, tags of ASCII letters, the interface's words and voice are built for English. What changing each would touch is recorded now so that nothing built first makes it harder | System design, section 16.3 |
| **Accept losing Snap Layouts on Windows 11, or win it back** | The maximise button's flyout belongs to the system's buttons. Keeping it needs a hit-test hook tao lacks: a patch to tao, or Windows code of our own in the desktop crate | Decided 2026-11-18: skipped for now; can be won back later |

---

## 12. Accessibility

- Contrast: section 4.1. Every text colour passes AA on every background
  it is drawn on, in both themes and every accent; unchecked controls pass
  3:1.
- Every icon button has an accessible name (its tooltip text); the icon
  itself is hidden from screen readers.
- Focus is always visible: a 2 px clay ring, except in text fields, which
  show focus with their border and halo.
- Everything has a keyboard path on the desktop (section 7.4).
- Controls that hide (the gutter, the window's controls, the archive box
  on a row) are never hidden from the keyboard: they stay in the tab
  order, and show while they have focus. On a touch screen a tap on the
  page shows them.
- Settings, the file preview and confirmations are dialogs: focus moves
  into them as they open, <kbd>Tab</kbd> stays inside, <kbd>Esc</kbd>
  closes, and focus goes back to what opened them.
- Toasts are announced (a polite live region), and Undo in a toast is
  reachable: <kbd>Ctrl Z</kbd> on the desktop, and on Android the toast
  stays while a screen reader is on until it is dismissed.
- Every gesture has a button too: swipe to archive has the box in the note;
  pull to sync has the sync icon.
- Voice is never the only way: everywhere there is a mic, typing works too.
  Holding has a tap alternative (tap to start, ✓ to stop), so no one has to
  keep a finger down.
- A screen reader is told when listening starts and when it ends, and
  when a voice note's words have arrived. The grains carry nothing a
  person needs to see.
- Text size from 13 to 20 px, and the apps follow the system font scale on
  Android.
- Reduced motion: section 5.5.
- Colour never carries meaning alone: the sync states differ in icon and
  word as well as colour.
