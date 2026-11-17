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
│ └──────────────────┘ │   │                      │   │ Ask the landlord     │
│ This week · · · · ·  │   │                      │   │ about the boi        │
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
activity it is in, and the app's is a light one today, so on Android Auto
is light whatever the system says. Android's main screen (plan, step 4)
gives the activity a theme that follows the system, and lets the app stay
open when the system changes: today Android restarts the activity, and
the app with it.

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
| Words while listening | Inter | 21 / 1.45 (Android), note size + 3 (desktop) | 400 |
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
| The pile grows | The new grain drops 46 px onto the top of the heap (640 ms, `cubic-bezier(.55,0,1,.45)`). Nothing else moves |

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
| Quick capture opens | The box comes into focus: from 97% and blurred 10 px to sharp (snap, the blur on enter) |
| Quick capture is put away (<kbd>Esc</kbd>, <kbd>Ctrl Enter</kbd>, a click outside) | The page shrinks to a grain and flies to the tray on an arc: across quickly, down slowly then fast, so the path curves (600 ms). The tray icon pulses once as it lands |
| A file is attached | Its tile grows out of the paperclip, or out of the point it was dropped on, and bounces into place (pop). Several files follow one another, 70 ms apart |
| A preview opens and closes | The file grows out of its tile and shrinks back into it (6.16) |

**Focus while writing (7.7)**

| Moment | Motion |
|---|---|
| The first key | Nothing moves. Only the controls fade: the note's icons and the window's to nothing, the list to half (200 ms, standard). The words being typed stay exactly where they were |
| The first pause | 600 ms without a key, or the first new line, and focus comes in by the chosen style, below. Each key before that waits it out again |
| Slide in | The list's content fades out (140 ms, exit); 90 ms later the list folds to nothing (glide); 60 ms after that the note's column glides to the middle (glide). Three moves, overlapping, each led by the one before |
| Rail in | The rows fold to the left one after another, 18 ms apart; the list narrows to 56 px (glide); then the dots pop into the rail top to bottom, 24 ms apart (pop) |
| Zen in | As Slide, then the paper warms (600 ms, standard, from 260 ms), the type grows by 4 px (glide, from 300 ms), and the top and bottom of the text fade in last (500 ms, from 420 ms). It deepens in stages |
| Out, any style | Reversed and quicker: the width returns first (glide), then the list's content fades in from 200 ms, the rows arriving in a cascade from the left, 22 ms apart (snap) |
| The pointer during focus | A control fades in (200 ms) only when the pointer comes within 64 px of it: the note's icons in the gutter, the window's controls in the top corner, the list's handle at the left edge. Moving anywhere else shows nothing, and the layout never moves |

**Voice (6.18)**

| Moment | Motion |
|---|---|
| Pressing the mic | It grows to 132% (pop) as a sheet rises behind it (glide), with the lock above the mic |
| Listening | A row of grains moves with the voice, like sand on a drum: still in silence, most in the middle, higher as it gets louder. A soft ring around the mic breathes with it. Each word fades in from a 4 px blur as it is heard (280 ms); the word still being heard is grey |
| Sliding up | The lock follows the finger halfway; past 60 px it drops away as the mic turns into ✓ (pop) |
| Sliding left | "Cancel" follows the finger and fades. Past 90 px the words blow off to the left, blurring (260 ms, exit), and the sheet sinks |
| Letting go, or ✓ | The words gather into a grain and settle onto the pile, as a typed thought does; the sheet sinks (glide) |
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
  reads "Remove animations" once, when it starts: changed while Dirt is
  open, it is honoured from the next start. Reading it again each time the
  person returns to the app needs a few lines of native code, which come
  with Android's main screen (plan, step 4); the root component already
  takes the answer.

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

Thumbnail or kind icon, name, "1.8 MB · JPEG", and where it is if that
matters: "uploading" (with the clay line filling), "on this device only"
(offline or signed out), "not on this device; fetched when it is wanted".
Desktop hover shows **Open in another app**, **Save a copy** and
**Remove** as icons; Android always shows them. A file the system would run
is shown without **Open**. A click or tap anywhere else on the tile, or
<kbd>Space</kbd> when it has focus, previews it (6.16).

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
Voice (6.18), Export. On the desktop a rail on the left jumps to each section and follows
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
| PDF | Pages, where the platform can draw them. Whether to bundle a renderer so every platform can is a decision (section 11) |
| Anything else, HEIC among them | Not previewed: a click opens it in another app, as now, and nothing says it couldn't be previewed |

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

A file that is not on the device yet is fetched first: a clay ring fills on
its tile, and the preview opens when it has come. The picture's thumbnail,
already on the device, is shown at once while the whole file is decrypted.

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
same every time for the same notes. Above a few thousand grains it is drawn
on a canvas.

What was tried first, and why not:

| | Why not |
|---|---|
| The same piling with 6 px grains | A triangle of dots, not sand. The colours mixed into mud, and the heap was too steep |
| Layers: a band for each month | A diagram, not sand |
| Dune: a drawn shape with ripples and a grain texture | Looked built, not blown |
| Days: a grain per day for a year | A streak chart under another name, which principle 5 rules out |

### 6.18 Voice

The fastest thought is one said aloud: no keyboard to open, nothing to aim
at. Voice is a way to write, not a recorder. What is kept is a note like
any other, its words as text, with the recording on it as a file.

**One gesture, the same everywhere:**

| Do | Happens |
|---|---|
| **Hold** the mic, speak, **let go** | Kept: the words settle onto the pile as a grain. One action |
| **Tap** the mic | Hands-free: it listens until ✓ (or <kbd>Enter</kbd>); ✕ (or <kbd>Esc</kbd>) throws it away |
| Hold, then **slide up** | Locks into hands-free without letting go first, for a long thought |
| Hold, then **slide left** | Throws it away |
| Started by a hardware key, with no hand on the screen | Ends with no hand too: 1.6 s of quiet after speech keeps it |

**While listening** the words appear as they are heard: the word still being
heard in grey, the settled ones in the text colour. A word shows about a
second after it is said (0.5 to 1.1 s on a laptop in the plan's first
trial; a phone is not yet measured), and a settled word is not changed.
Where a second model reads each stretch again at a pause, the words of the
stretch stay grey until it has, and settle together then.
Under them a row of grains moves with the voice. A timer shows from the
first second. Nothing else: no "Listening…", no level meter, no waveform of
what was said.

| | Desktop | Android |
|---|---|---|
| In the app | 🎙 in the blank page's gutter. The page itself listens: words at the top, the grains, ✕ and ✓ at the bottom | The mic in the dock. A sheet rises over the lower half of the screen |
| From anywhere | **Hold** <kbd>Ctrl Alt N</kbd>: quick capture opens listening, and letting go keeps it. A tap still opens it for typing | The widget's mic (hands-free, ✓ to keep). Where the phone lets a hardware key open an app (double-press the side key on Samsung; Quick Tap on Pixel), Dirt's **Talk** opens straight into listening, over the lock screen too, and ends on a pause |
| Into a note being written | Not offered: type, or use the system's own dictation | 🎙 in the note's top bar adds what is said at the end of the note |

**Heard on the device, always.** Speech is turned into text on the device
that heard it and is never sent anywhere: not to Dirt's server, not to a
speech service. That is the only way voice keeps Dirt's promise that the
server sees nothing it can read. A kept recording is an attached file,
encrypted like any other. Settings → **Voice** has: one line, "Turned into
text here. Your voice is never sent anywhere"; **The recording** (Keep, or
Text only); **Language** (Auto, or one chosen); and on the desktop the held
hotkey, on Android a word about the side key.

**Failing well.** Nothing heard: the sheet shakes once and goes, and
nothing is made. Heard, but the words could not be made out (the speech
engine missing or failing): the note is kept with the recording alone,
named by its length ("Voice · 0:12"), and gets its words when they can be
made, from the device that recorded it and from no other: two devices
adding the words would add them twice. No microphone permission yet: the first press asks, in the system's
own dialog; refused, the mic shows a slash, and its tooltip or long press
says where to allow it.

**The recording** is one file on the note, marked in the note's document
as its voice recording; that mark is what puts the mic on its card (6.2).
With **Text only** chosen there is no file and no mic: the note is a note
like any other. A recording is compressed speech: Ogg Opus, 16 kHz mono at
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
| Deleted, then brought back | Comes back archived, to where it was | |
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
the open note attaches it. The paperclip opens the file dialog, which takes
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
  for a window without decorations; on Linux the app adds invisible 6 px
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
| 🎙 | Listens, in the same sheet; what is said is added (6.18) |
| ✓ | Keeps the note. The sheet lifts 24 dp and fades (240 ms, `--e-in`); nothing says "Saved", and if the pile widget is on the home screen, it gains the grain. The person is back where they were: two actions plus typing |
| Back, a tap above the sheet, Home | Closes it; what was written stays as a draft and is there next time |
| Its words | Only the placeholder. No title, no buttons but 🎙 and ✓ |

The note is kept on the phone the moment ✓ is pressed, and it should
reach the person's other devices without Dirt being opened (decided
2026-11-16). The sheet starts a short piece of background work that makes
the thought a note and syncs once, with no web view. That is an event, not
a timer, so the rule that nothing syncs on a timer holds. Whether Dirt's
core can run that way on Android is the plan's second trial. If it cannot,
the thought becomes a note and syncs the next time the app is opened.
Either way it is never lost in between. A thought written over the lock
screen waits until the phone is unlocked.

### 8.6 The lock screen

Writing a thought without unlocking the phone is the fastest capture there
could be, and it is partly possible. Android does not let an app put a
widget or a button of its own on a phone's lock screen. These are the ways
in, and the design uses all of them:

| Way in | Where it works | Opens without unlocking? |
|---|---|---|
| A **Quick Settings tile**, "New note", pulled down over the lock screen | Every Android phone | Depends on the phone: some open the sheet over the lock screen, others ask to unlock first. Has to be tried on devices |
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

Not possible: capture right after the phone restarts and before it is
first unlocked. Android keeps every app's storage locked until then.

---

## 9. Flows

### 9.1 Signing in

Settings → Account → **Sign in**. The system browser opens Clerk; when it
returns, the button has shown "Waiting for browser" with a spinner the whole
time, so nothing seems stuck. "Bring the 12 notes on this device into the
account" is checked by default. Back in the app, sync starts on its own.

### 9.2 Letting a new device in

1. The new device signs in. It shows seven emoji and waits.
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
in four columns, with **Copy** and "I've written it down". Getting in with the
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
couldn't be had are listed in the toast's place as a short list with reasons.

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
  desktop through `cpal`) and hands the sound to `dirt-voice`, which turns
  it into words, streams them back as they settle, says when the speaker
  has stopped, and writes the recording. The settling grain and the
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
| **Android: the capture sheet** (8.5), a native screen kept outside the web view so that it opens at once, with a place on the phone where a thought waits until the app makes it a note | Capture in three actions from outside the app, without waiting for the app to start | `dirt-mobile` |
| **Android: the widget, the app shortcut, a Quick Settings tile, and the note-taking role** (8.5, 8.6) | Every way in to the capture sheet that Android offers, the lock screen among them | `dirt-mobile` |
| **SECURITY.md: capture over the lock screen** | It adds a way to write into the phone without unlocking it. What it may and may not show (8.6) belongs in the security document | `docs/SECURITY.md` |
| **Try lock-screen capture on real phones** | Whether the tile opens over the lock screen is each phone's choice | `docs/MANUAL_CHECKLIST.md` |
| **Android: share saves at once** with **Undo**, for text as well as pictures | Already true; keep it, and add the toast | `dirt-mobile` |
| **Desktop: drop files to make a new note**, and paste a picture | Attach in one action | `dirt-desktop`, `dirt-core::files` |
| **Load more on scroll** instead of "Show older notes" | One less button; the store already pages | Both apps |
| **A file's decrypted bytes in memory**, from `dirt-core`, not only as a file on disk | The preview shows a file without writing a copy out (6.16). Decryption stays in the core: no cryptography in a client crate. The core has it since step 1 of the plan (`Session::file_bytes`) | `dirt-core::files`, both apps |
| **PDF in the preview: bundle a renderer or not** | WebView2 draws PDFs; WebKitGTK and Android's WebView do not. Bundling pdf.js (a few MB) makes it the same everywhere; not bundling means PDFs open in another app on Linux and Android | Owner's decision |
| **SECURITY.md: the preview** | A previewed file stays in the app's memory; **Open in another app** writes a decrypted copy and lends it. The document should say which does which | `docs/SECURITY.md` |
| **Drawn scrollbars** (6.15) | One look on every platform, and none of the space a system scrollbar takes. One small script shared by both apps, as the list's FLIP is | Both apps |
| **No title bar on the desktop** (7.6) | The app reaches the top edge; one row less of chrome and one word less ("Dirt"). Windows and Linux draw their own controls; macOS keeps the system's | `dirt-desktop` |
| **The motion system** (5): three springs as `linear()` easings, choreography from cause to destination, the View Transitions circle for theme and accent | Movement that says where things come from and go, with one feel across both apps | Both apps: one CSS file of tokens and one small animation script |
| **Focus while writing** (7.7): Slide, Rail and Zen, chosen in Settings, Slide by default | Only the note while it is written, three ways | `dirt-desktop` |
| **The page: a new note from its first word** (6.1) | The note exists before it is "done". One emptied again, with no files, must be let go without a word: taken off the device altogether if it never reached the server, deleted like any other note if it did | `dirt-core` store, both apps |
| **The note's icons beside its column, shown by nearness** (6.4) | The icons belong to the note; nothing comes back on a stray mouse move | `dirt-desktop` |
| **Grains, layers and the pile** (6.17) | 積沙成塔: the list as a pile, newest on top, and the pile drawn as a heap of sand, one grain per thought. Derived from the notes; nothing new is stored. The core gives every note in one call, each grain's shade, the layer of an edit time and the numbers of the count line since step 1 of the plan | `dirt-core` store and models, both apps |
| **Archive** (6.19): an `archived` value in each note's CRDT document, derived into its row like the deleted flag. The list leaves archived notes out; search and export keep them | Clearing a done thought without losing it. In the note's document it syncs and merges like any edit, and the server sees nothing new | `dirt-core` notes and store, every client, the CLI (`dirt archive`, `dirt unarchive`, `dirt list --archived`), feature-parity.md. Archiving is not an edit (6.19) |
| **Tags: counts for the notes left** (6.3): how many notes match the current words and tags plus each other tag, and "any of them" as well as "all of them" | The tag list that stays open, and never offers a tag that would empty the list. The store has it since step 1 of the plan (`Store::tags_to_narrow`, `find_notes_by_tags`) | `dirt-core` store: one grouped query, which starts at the full-text index when there are words and at the tag table when there are none |
| **Voice: speech to text on the device** (6.18). One engine used by both apps, giving words as they are heard: sherpa-onnx with a streaming model, and Silero's voice-activity model for the end of a thought. The plan's first trial chose it over whisper.cpp read through a sliding window and over Android's own on-device recogniser (implementation plan, "Trial 1 result"); a phone is still to be measured. Its model is fetched once on first use from Dirt's own storage, and its hash is checked against one built into the app | Voice without anyone's voice leaving the device. The engine is native code: prebuilt libraries of 21 MiB on Windows and 25 MiB for each Android ABI, fetched when it builds and shipped beside the app. So it sits in a crate of its own: the CLI, the server's tests and most CI jobs must not build it | `dirt-voice`, a new crate, kept out of the workspace's members and built only when an app's `voice` feature asks for it; both apps; `dirt-core` fetches the model |
| **Which speech model, downloaded or shipped** | English comes first (decided 2026-11-16), so the smallest model that hears English well is enough. The trial found two that fit (implementation plan, "Trial 1 result"): one model of 68 MiB under CC BY-SA, or two under Apache-2.0 and MIT that come to 110 MiB. Downloading on first use keeps the apps small; shipping makes voice work offline from the first second | Owner's decisions: which of the two, and downloaded or shipped |
| **Microphone access**: Android's `RECORD_AUDIO`, asked on first use; the desktop's microphone, with macOS's usage string | Voice | `dirt-desktop`, `dirt-mobile` |
| **A held hotkey**: <kbd>Ctrl Alt N</kbd> must report its release as well as its press | Hold to talk from any app. The `global-hotkey` crate reports both; Linux under Wayland offers no global hotkeys at all, held or not | `dirt-desktop` |
| **Android: a Talk entry** the hardware shortcuts can open, straight into listening, over the lock screen | Capture without touching the screen | `dirt-mobile` |
| **SECURITY.md: voice** | Where speech becomes text, that a kept recording is an encrypted file like any other, and that over the lock screen voice can add and show nothing | `docs/SECURITY.md` |
| **feature-parity.md: the page, Find and voice** | New rows for every client, the CLI included (`dirt` already takes a thought as an argument) | feature-parity.md |
| **A crate for what the two apps share**, `dirt-ui`: the tokens, fonts, icons, the motion script and the components | Section 10 asks for one style sheet and shared scripts. Today the two apps share no interface code, and would build everything twice | A new crate, CLAUDE.md |
| **Android: more Kotlin than one file, and a folder of resources** | The capture sheet, the widgets, the tile, the shortcut and the Talk entry are native screens and services. Today the build takes one Kotlin file and a manifest, and writes two resource files from the build script | `dirt-mobile`'s build |
| **The desktop's choice of font goes**; text size stays | One typeface, bundled, is part of the design (4.2) | `dirt-desktop`, feature-parity.md |
| **Other languages, later** | Find's rule of words, tags of ASCII letters, the interface's words and voice are built for English. What changing each would touch is recorded now so that nothing built first makes it harder | System design, section 16.3 |
| **Accept losing Snap Layouts on Windows 11, or win it back** | The maximise button's flyout belongs to the system's buttons. Keeping it needs a hit-test hook tao lacks: a patch to tao, or Windows code of our own in the desktop crate | Owner's decision |

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
- The words being heard are a live region: a screen reader reads them as
  they settle. The grains carry nothing a person needs to see.
- Text size from 13 to 20 px, and the apps follow the system font scale on
  Android.
- Reduced motion: section 5.5.
- Colour never carries meaning alone: the sync states differ in icon and
  word as well as colour.
