# The motion check

A movement that looks wrong when it plays rarely shows in a screenshot or in
the code. The motion check plays it on the real app, frame by frame, from
start to end. It lays the frames out as images an agent can open and read, and
it writes the movement down as numbers that say what moved, when and how.

It is `tools/motion/`, a Node program with nothing to install (Node 22 or
later; it speaks the Chrome DevTools Protocol over Node's own WebSocket). It
needs Edge or Chrome for the mockup and for drawing the sheets, and ffmpeg on
the PATH for Android's screen recording and for videos.

```bash
node tools/motion/motion.mjs list
node tools/motion/motion.mjs run settings-panel --platform desktop --mockup
node tools/motion/motion.mjs run note-open-close --platform android --mockup
node --test tools/motion/test/analyse.test.mjs tools/motion/test/page.test.mjs
```

The second test file runs the page script (the stepped clock, the probe) in
a headless Edge or Chrome, and is skipped where neither is installed. CI
runs both (`motion-tools`), on Node 22.

A change to a movement is checked with it before and after (CLAUDE.md).

## What it answers, and how

Each scenario is captured three ways, each answering its own question.

**1. What each frame should show: stepped frames.** The movement's time is
stepped, 1/60 s at a time by default (`--step-ms`), and each step is
photographed. The result does not depend on how fast the machine is. How it
works:

- The document's timeline is stopped (DevTools' `Animation.setPlaybackRate(0)`)
  before the input is sent.
- While the page answers the input (the app's round trip to Rust), the
  clock stays at 0.
- Each step then sets every animation's current time on a clock of the
  tool's own (`tools/motion/lib/page.mjs`). Every CSS transition, every CSS
  animation and every Web Animation is one of `document.getAnimations()`, so
  all of them are stepped.
- Each animation is paused when the clock first sees it, so that what is
  drawn is the time the clock set. It is finished when the clock passes its
  end, so the app's own `finished` handlers run on time.
- Timers with a delay (`setTimeout`, `setInterval`) are held on the same
  clock and run as it passes them. That covers `dirt-ui.js`'s staged beats
  and its `after()` timers. A timer of 0 runs at once, since it means the next
  task rather than a time.
- `performance.now()` reads the clock too.

What cannot be stepped:

- **Native smooth scrolling** runs on the real clock. Settings' rail jump
  uses it. When something scrolled during a stepped capture, the report
  says so; read the real-time layer for it.
- **The app's own timers in Rust** run on the real clock, and stepping
  takes more real time than the movement does. The file preview keeps its
  file for 450 ms after it closes (`dirt_ui::files::note_files`, `CLOSING`),
  so in the stepped frames the picture goes at once. When real time still
  draws a part that the stepped frames saw vanish, the report says so and
  ranks the finding as about the capture, not the app.
- **`requestAnimationFrame` in `dirt-ui.js`** is not stepped. It is used
  only to measure (the list's positions after a resize, the nearness of
  controls, the gallery's frame counter), to wait one frame before a grain
  is tossed, and to draw the voice grains, which follow the microphone and
  not time. None of the movements the scenarios cover is driven by it.

**2. What a person sees: real time.** The movement plays at its own speed
under a DevTools trace that keeps a screenshot of every composited frame
(`disabled-by-default-devtools.screenshot`). The report gives:

- the frame rate and the gaps between frames, judged only while
  something moved, by the probe's samples. Nothing is asked of the screen
  while the app has not answered yet, or after it settles (a blinking caret
  draws a frame every half second), so a gap there is not a frame missed.
  With the probe off (`--realtime-probe off`) every gap is judged, and the
  report says which;
- when the first change came after the input (the app's answer);
- the frames not drawn, and the gaps longer than a frame and a half;
- long tasks on the page's main thread;
- Chromium's own frame reports (presented, dropped, partial).

On Android it adds `adb shell screenrecord`, cut into its frames by ffmpeg,
and `dumpsys gfxinfo <package> framestats`. The recording shows what the
web view cannot, such as the bars and the keyboard. It writes a frame only
when the screen changes, so its gaps are the gaps a person saw.

**3. How each part moved: the motion trace.** For the parts a scenario names,
every frame (each step, and each animation frame in real time) records each
element's:

- box on screen, transforms included, and cut by its own `clip-path` of
  `inset()` or `circle()`;
- opacity as drawn (its own times its ancestors');
- how much of the box can be seen: cut by the viewport, by the `clip-path`
  of every ancestor (Android's note page grows and shrinks by one, with its
  bar and text inside), and by the overflow of the ancestors on its chain of
  containing blocks (so a `position: fixed` part inside a scrolling list is
  not cut by the list);
- whether it takes the pointer;
- its scroll position.

From that, `tools/motion/lib/analyse.mjs` finds:

- **pops**: a change between two frames much bigger than the frames around
  it, or a part that appears or goes in one frame while well opaque.
  Entering sight past the edge of a scrolling list is not a pop;
- **a part outside its container** (`checks.within`), and **two parts
  seen over one another** (`checks.apart`). The probe asks the page what is
  on top (`elementsFromPoint`) at points where their boxes meet: it counts
  where nothing else is drawn over either, so a part under a backdrop is not
  seen under the other. Where neither can be hit (both take no pointer) the
  boxes alone are used, and the report says so;
- **a part that should have left** (`leaves`): still drawn at the end, or
  gone from sight but still taking the pointer. With it, when each part was
  last drawn. Also **a part that should have arrived** (`arrives`);
- **a part that stops on the way** (`straight`): on x, y, w or h it
  slows down away from both its ends and then picks up speed again. One
  movement, on a curve or a spring, only slows once it has reached its top
  speed; one that speeds up again was sent somewhere else on the way. It
  is judged on the stepped frames and in real time, since a smooth
  scroll, which a part may follow, runs only on the real clock. Settings'
  rail highlight walked through every section the scroll passed (#321);
- **a closing against the opening it is meant to mirror** (`mirrors`),
  only where the scenario declares it from the design, for the parts and
  channels it names. For each, it gives how long each took, and where the
  closing last showed the part against where the opening first showed it
  (for opacity, where each began and ended). With `curve: true`, it also
  gives how far the closing's curve is from the opening's played backwards.
  Where the design means a closing to differ (the desktop's note only fades
  as it is let go), nothing is declared and it is compared with the mockup
  alone;
- **the app against the mockup** (`--mockup`): the same steps on
  `docs/ui/mockup.html` in a headless Edge or Chrome, with a profile of its
  own in a temporary folder. Durations, start times and curve distances,
  part by part. The mockup holds for durations and curves (CLAUDE.md). The
  mockup takes its fonts from Google Fonts, so the headless browser asks for
  them; when they do not load, the report says so, since text boxes then
  differ a little.

A curve here is a channel's distance from where it started, as a share of
the furthest it gets, against its own time. A part that goes out and comes
back is measured by how far it went. A curve distance is the largest gap
between two curves at the same moment, from 0 (the same) to 1 (a whole
movement apart, the most reported). A channel that moves less than 3 px, or
0.05 of opacity, is no movement and is not compared.

## Where it runs

**Desktop: a test copy of the app, never the person's.** It starts a copy
of the dx-built app (`dx build --platform desktop --package dirt-desktop`;
`--exe` names another development build) with every development stand-in
set. A release build ignores every stand-in: its window would open on the
screen, its hotkey would be real and signing in would open the browser. So a
program that looks like one is refused, told by a `release` folder in its
path or by the lack of the overflow-check panic message that development
builds carry. The stand-ins are:

- its own `DIRT_DATA_DIR` and `WEBVIEW2_USER_DATA_FOLDER` in a temporary
  folder;
- `DIRT_WINDOW_AT` off every screen, so the window is out of sight and
  never takes the keyboard;
- `DIRT_TRAY_AT` off every screen, which is only where quick capture's
  flight lands. **The copy's tray icon is real**: while a run lasts, a
  second Dirt icon is in the person's tray. Nothing clicks it;
- `DIRT_ACTIONS_FROM_STDIN`, never sent `open dirt`, and `quit` to end it;
- `DIRT_OPENED_TO_FILE`, `DIRT_SIGN_IN_TO_FILE` and `DIRT_EXPORT_TO`, as
  files in its data folder;
- `DIRT_API_BASE_URL` pointed at a closed local port, so that no server is
  ever reached;
- its own DevTools port, the first free from 9581 (never 9222, 9333,
  9377, 9471 or 9563–9565), with Chromium's occlusion and background
  throttling turned off. A port given with `--port` that something else
  uses is refused: the tool would drive whatever web view answers there.

It is stopped by its process ID, and its folder is removed.

The app holds a single-instance lock. The test app is therefore always a
copy, in the temporary folder, whose lock has another name of the same
length. It never stops a Dirt that is running from going on, and never makes
a Dirt the person starts during a run exit without a word. Only what the app
needs is copied: the program, the libraries beside it and its `assets`
folder, the voice model only with `--voice`. A copy over 600 MB is refused
(an `--exe` in a folder like `target/debug`), since the disk may be nearly
full. Neither the repository nor the original folder is changed.
`--in-place` runs the app where it is instead. That is refused while a Dirt
is running, and warns that a Dirt started during the run will exit.

**Stopped part way.** Ctrl C, a signal or an error runs every cleanup the
run put down, the last first:
- the page gets back its own timers and an animation rate of 1;
- every process the run started is ended by its ID;
- the device's screen recording is stopped and deleted;
- every adb forward and temporary folder is removed;
- a half-made run folder is removed.

A run killed outright (Task Manager, `kill -9`) cannot clean up. If that
happens, restart the app on the device, which gives it back its own timers;
on the desktop the copy and its folder are in the temporary folder, named
`dirt-motion-*`.

A window off the screen may be held back by Chromium. Every run measures
how often the page drew while idle and says in the report whether real time
can be judged. Off the screen, with the flags above, WebView2 drew at
56–57 frames a second on a 60 Hz laptop.

**Android: the emulator.** The tool brings the app's main screen up by
name (`dev.dioxus.main.MainActivity`), running or not: the app has a second
launcher entry, Talk, and its process also runs without its screen, for a
capture job.
It finds the web view's DevTools socket (`webview_devtools_remote_<pid>`)
and forwards a local port to it from 9591. A debug build is debuggable,
because wry calls `WebView.setWebContentsDebuggingEnabled` when
`debug_assertions` are on; a release build is not.

To keep the emulator as it was, start it read-only and without a window:

```bash
emulator -avd dirt-trial -no-window -read-only -no-audio -no-boot-anim
adb -s emulator-5554 install -r target/dx/dirt-mobile/debug/android/app/app/build/outputs/apk/debug/app-debug.apk
```

Nothing installed or written there outlives it.

**Android: a phone, only when its owner runs it.** A device that is not an
emulator is refused unless named and allowed:

```bash
node tools/motion/motion.mjs run settings-panel --platform android --serial <phone> --real-phone
```

The phone needs a debug build and USB debugging.
- **Its notes are its owner's, and they sync.** A scenario with steps that
  write (it types, pastes a file, or has a step marked `"writes": true`, a
  press that makes, archives or deletes a note) is refused on a phone unless
  `--allow-writes` is given. `note-open-close.json` writes: it makes three
  notes to open. `settings-panel.json` does not.
- The screen recording is stopped by its own process ID and deleted from
  the phone however the run ends.
- An interrupted run gives the page back its timers and animation rate. If
  the run was killed outright, restart the app.

Only a phone's numbers say how a phone performs; the emulator's do not.

**The mockup.** `--platform mockup-desktop` or `mockup-android` runs a
scenario on the mockup alone. `--mockup` runs it after the app and compares
the two.

## Writing a scenario

A scenario is a JSON file in `tools/motion/scenarios/`. A selector or a list
is either one value for every platform or an object keyed by platform:
`desktop`, `android`, `mockup-desktop`, `mockup-android`, `mockup` (both
mockups) and `default`.

```json
{
  "name": "settings-panel",
  "title": "Settings opening and closing",
  "issue": 321,
  "platforms": ["desktop", "android", "mockup-desktop", "mockup-android"],
  "setup": [{ "wait": 1500 }],
  "parts": {
    "panel": { "desktop": ".dui-panel[aria-label='Settings']", "mockup-desktop": "#d-settings" },
    "railButton": { "desktop": ".dui-rail button", "mockup-desktop": "#d-settings .rail button" }
  },
  "crop": { "mockup-desktop": "#D" },
  "checks": { "within": [["railButton", "panel"]], "apart": [], "pops": { "px": 24, "opacity": 0.3 } },
  "movements": [
    { "name": "open", "steps": [{ "click": { "desktop": "button[aria-label='Settings']", "mockup-desktop": "#D [data-act='settings']" } }], "arrives": ["panel"], "ms": 1400 },
    { "name": "close", "steps": [{ "key": "Escape" }], "leaves": ["panel"], "ms": 1200,
      "mirrors": { "of": "open", "parts": { "desktop": { "panel": ["x", "y", "w", "h"] } }, "curve": false,
                   "because": "design 5.5: it grows out of the gear and shrinks back into it" } }
  ]
}
```

- **`setup`**: steps run once, before anything is captured, to bring the
  app to the start state. A fresh test copy has no notes, so a scenario
  makes them, as `note-open-close.json` does.
- **`parts`**: what the trace measures. A part should match the same
  number of elements in the app and in the mockup, or their tracks will not
  line up (`.dui-panel` alone also matches the panel of keys).
- **`crop`**: the part of the page the frames show, by a selector (the
  mockup's device frame, a panel). Default: the whole page.
- **`viewport`**: a window of another size, for the desktop only:
  `{ "desktop": { "width": 2560, "height": 1440, "scale": 1 } }`. The test
  copy stays off the screen, where it cannot be maximised, so its page is
  laid out at that size by DevTools' device metrics, before the setup runs,
  and the report gives the size. `focus-wide.json` uses it (#325). Android's
  page is the screen's and takes none.
- **`movements`**: captured in order, all of them stepped, then all of them
  in real time. So they should end where they began (open, then close), or
  `between` lists steps that reset the app between the two passes. Each
  movement has:
  - `steps`: its input;
  - `ms`: the longest it may take;
  - `leaves` and `arrives`, as above;
  - `straight`: the parts that should go straight to where they end, as above;
  - `mirrors`: `of` (the opening), `parts` (part → channels, or per platform),
    `curve` (whether the curve too is meant to be the opening's reversed, or
    only where it goes) and `because` (the design's words, with the section);
  - `crop`, for that movement only;
  - `compareIn: "realtime"`, to compare it with the mockup in real time
    rather than stepped (a smooth scroll);
  - `platforms`, to run it on some platforms only;
  - `after`: steps run once it is captured.
- **Steps**:
  - `click` / `tap` (a selector, with `index` for the n-th drawn match);
    `click` presses with the mouse, or with a touch on Android;
  - `hover`;
  - `move` (`{ "on": …, "dx": 80, "dy": 0, "steps": 8 }`: the pointer
    moved as a hand moves it, from the centre of what `on` names, or from
    where it last was, `dx` and `dy` pixels further in `steps` moves. The
    desktop brings the note's icons back only for a deliberate movement,
    40 px or more (design 7.14), which one `hover` does not make);
  - `key` (`Escape`, `Enter`, `Tab`, `' '`, the arrows; with `ctrl`,
    `shift` or `alt`). A single character (`"x"`) types it, with its key
    down and up as a hand would, where `type` inserts text with no key:
    the desktop tells the two apart, since text that came without a key
    (pasted) brings focus at once (7.9). With `ctrl` it is a shortcut and
    types nothing;
  - `type` (inserted as typed text);
  - `wheel`;
  - `wait` (ms), `waitFor` / `waitGone` (a selector);
  - `pastePicture` (a picture pasted into the page, as one copied from
    another program; the app's own paste handler takes it);
  - `androidKey` (an Android key code, on the emulator: 4 is Back);
  - `androidTap` (`[x, y]`, a tap on the emulator's screen outside the web
    view, in the screen's pixels as `uiautomator dump` gives them: in
    Android's own screen for choosing files, which the paperclip opens.
    `files-attach.json` uses it);
  - `eval` (for a mockup's start state only, never for the input of a
    movement);
  - `caretAbove` (a check, not an input: `{ "field": …, "above": … }`;
    the caret of the field, or of whichever field has the keyboard, must
    be drawn above the top of the element `above` names, or the run stops
    and says where each was). The page has no box for a caret, so it is
    measured by laying out the field's text up to it in a copy of the
    field (`__motion.caret`). `files-caret.json` uses it (#329).

  Any step can carry `then` (ms to wait after it), `platforms`,
  `repeatUntilGone` (press again until a selector has gone: Android's page
  bar takes the first touch while typing) and `writes` (it changes notes;
  refused on a phone without `--allow-writes`). After a mouse `click` the
  pointer moves off the page, so that no `:hover` is left on what opened
  under it; `"stay": true` keeps it there.

All pointer and key input goes through DevTools' `Input` domain into the
test app's own web view, as a hand would give it. Nothing reaches the
person's screen, keyboard or mouse.

**A picture on the emulator.** `pastePicture` is the desktop's paste;
Android has none. `file-preview` on Android needs its newest note to hold a
picture, made by sharing one to the app. A debug build lets `run-as` put
the file where the app's own `FileProvider` lends from, which the app can
read without a grant (a picture in the emulator's media store it cannot):

```bash
adb -s emulator-5554 push garden.png /data/local/tmp/garden.png
adb -s emulator-5554 shell "run-as com.example.DirtMobile sh -c 'mkdir -p files/dirt/opened && cat /data/local/tmp/garden.png > files/dirt/opened/garden.png'"
adb -s emulator-5554 shell am start -W -a android.intent.action.SEND -t image/png   --eu android.intent.extra.STREAM content://com.example.DirtMobile.files/opened/garden.png   -n com.example.DirtMobile/dev.dioxus.main.MainActivity
```

(In Git Bash, set `MSYS_NO_PATHCONV=1` and give `push` a Windows path.)

**Several files on one note on the emulator.** `files-fold` on Android
needs its newest note to hold three files or more. A share brings one file
to a note of its own, and `am` cannot send the list of addresses a share
of several needs, so they are chosen with the note's paperclip, in
Android's own screen for choosing files. Name the emulator in every adb
command: with a phone plugged in as well, a command without `-s` would
run on the phone.

```bash
adb -s emulator-5554 shell mkdir -p /sdcard/Download/dirt
adb -s emulator-5554 push "f01 Picture 1.png" /sdcard/Download/dirt/
adb -s emulator-5554 shell content call --uri content://media --method scan_volume --arg external_primary
adb -s emulator-5554 shell uiautomator dump /sdcard/ui.xml
adb -s emulator-5554 shell cat /sdcard/ui.xml
```

Then on the page press the paperclip and, in that screen, switch to the
list, long-press the first file and tap the others, and press **Select**.
`files-attach` on Android needs a note with three files made this way,
newest, and the pictures in a folder of their own that the screen opens
in, with `purple.png` where its `androidTap` presses (1080 × 2400: the
second column of the second row of the grid); choose one file at a time,
three times, and the screen opens in that folder again each time.
The dump says where each is (`uiautomator` may end with code 137 after
writing its file; the file is there). The screen shows about nine at a
time; press the paperclip again for more. Share the picture for
`file-preview` after, so that it is the newest note again.

**Never press a file tile's buttons.** The first opens the file in another
app, on the person's screen. Open the preview by `.dui-file-main`.
`DIRT_OPENED_TO_FILE` stands in if anything else is pressed.

## Reading the output

Each run is a folder `<target>/motion/<scenario>-<platform>-<time>/`. The
target is `CARGO_TARGET_DIR`, or the repository's `target/`, so a run made
in a worktree that shares the main target outlives the worktree. `--out`
names another place. The folder holds:

- **`report.md`**: start here. The idle frame rate, and whether real time
  can be judged. Then a **summary**, most telling first:
  - parts cut or left behind;
  - what is wrong against the design or the mockup;
  - what differs and is worth a look;
  - notes about the capture.

  Each line has its movement, its millisecond and its sheet. Then, for each
  movement:
  - the stepped frames, and the animations the clock saw (what, on what,
    delay, duration, easing);
  - the real-time frame rate and its gaps;
  - each finding with its millisecond;
  - a closing against the opening it is meant to mirror;
  - the app against the mockup.
- **`stepped-<movement>(-n).png`**: the stepped frames, at most 24 to a
  sheet, each stamped with its millisecond. A red border is a finding,
  named under the frame. A gold note is an animation starting there.
- **`realtime-<movement>(-n).png`**: the frames as composited, with the gap
  since the frame before. A red border is a gap over 25 ms.
- **`screen-<movement>(-n).png`** (Android): the device's own recording.
  Its times are the video's, from when recording began.
- **`mirror-<movement>.png`** (where the scenario declares a mirror): the
  opening forwards above, the closing backwards below, column by column at
  the same fraction of each.
- **`mockup-<movement>.png`**: the app above, the mockup below, at the same
  millisecond.
- **`trace.json`**: every probe sample, the findings and the comparisons.

The frames themselves are deleted once the sheets are made, to spare the
disk; `--keep-frames` keeps them and the DevTools traces. `--video` also
writes each stepped movement as a 60 fps video.

**How to read a sheet.** Read along the row: a part should travel a
little further each frame. Look for a frame where it jumps, where
something appears or goes all at once, or where one part has finished while
another has not started. A sheet's numbers are in the report. A curve
distance near 0 means the same movement; above about 0.3 the difference can
be seen.

## Limits

- **Stepped time starts when the page has answered the input**, not at the
  press. How long the app took to answer shows in real time, not in the
  stepped frames.
- **Smooth scrolling is not stepped** (see above).
- **Android's stepped frames can lack the text of an element whose opacity
  is being stepped.** The note page's text, in its fading body, was not
  drawn until that animation had finished, although the page reported the
  body nearly opaque. The device's recording and the composited frames
  show it fading in. Trust the numbers and the real-time layer for that
  part.
- **The app's own timers run on the real clock** (see above). When the two
  layers disagree, a person sees the real-time frames.
- **On Android the real-time probe costs frames.** Measuring every part on
  every frame is work for the page. On the emulator it cost a third or more
  of the frames. Judge Android's frame rate from
  a run with `--realtime-probe off`, which keeps the trace, the screenshots,
  the recording and `gfxinfo`. The screen recording costs the device a
  little too.
- **The emulator's frame rate is the emulator's.** Without a window, its
  GPU is emulated and slow. A phone's numbers come only from a phone.
- **A pop is a guess from numbers.** A change inside an element (text
  that appears, a picture taken out of its frame) does not move its box.
  Name the inner element as a part of its own, as `file-preview.json` names
  the picture inside the preview's frame, and read the sheets.
- **The mockup's data is not the app's.** Its notes, files and window size
  differ. The comparison is of durations, start times and the shape of
  each curve, not of positions.
- **Real time on a desktop off the screen** is judged only when the page
  drew near the display's rate while idle. The report says when it did
  not.

## Settled while building it

| | Settled |
|---|---|
| Node, no packages | Node 22 has a WebSocket of its own, so the DevTools Protocol needs nothing installed. The analysis is pure functions with `node --test` tests; the page script is tested in a headless browser; CI runs both on Node 22 (`motion-tools`) |
| Sheets drawn by the browser | The issue suggested ffmpeg for the contact sheets. They are an HTML grid photographed by the headless browser that the mockup already needs. That gives a label on every frame, real-time frames at uneven times, findings in red beside the frame that shows them, and two rows side by side, which ffmpeg's tile filter does not. ffmpeg stays for the screen recording and the video |
| The stepped clock | Stopping the timeline alone stepped the desktop correctly. Android's WebView drew a compositor-run opacity animation at its start while the page reported it nearly done, so each animation is also paused, and finished as the clock passes its end |
| Real-time stamps | WebView2 counted `requestAnimationFrame`'s timestamps from another origin than `performance.now()`, 12.7 s apart. The recorder moves them onto `performance.now()` once, at the first frame |
| A page that outlives a run | Android's web view keeps what an earlier run installed. The page script carries a version and replaces an older one |
| What mirrors what | A first version compared every closing with its opening reversed. Where the design means them to differ (Settings shrinks into the gear on the exit curve; the desktop's note only fades) that was noise, so a scenario declares what is meant to mirror, from the design, and the rest is compared with the mockup alone |
| Waiting for the page | The desktop's web view is found as soon as it exists. Twice a run read the window's size before the app had placed it and installed the page script into a document the app then replaced (`__motion is not defined`), so a run now waits until the page has loaded, the app has drawn into its root and the size has held for 600 ms |
| A cut at the input | The frame before the input is kept and compared with the first, so a part the input itself switches on or off (the preview's backdrop) is a pop, and a part drawn only before the input "went at once" |
