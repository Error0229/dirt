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
node tools/motion/motion.mjs run settings-panel --platform desktop --beside-running --mockup
node tools/motion/motion.mjs run note-open-close --platform android --mockup
node --test tools/motion/test/analyse.test.mjs
```

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
- **`requestAnimationFrame` in `dirt-ui.js`** is not stepped. It is used
  only to measure (the list's positions after a resize, the nearness of
  controls, the gallery's frame counter), to wait one frame before a grain
  is tossed, and to draw the voice grains, which follow the microphone and
  not time. None of the movements the scenarios cover is driven by it.

**2. What a person sees: real time.** The movement plays at its own speed
under a DevTools trace that keeps a screenshot of every composited frame
(`disabled-by-default-devtools.screenshot`). The report gives:

- the frame rate and the gaps between frames, over the time something
  moves;
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

- box on screen, transforms included, and cut by a `clip-path` of
  `inset()` or `circle()`;
- opacity as drawn (its own times its ancestors');
- how much of the box is not clipped by the viewport or a scrolling
  ancestor;
- whether it takes the pointer;
- its scroll position.

From that, `tools/motion/lib/analyse.mjs` finds:

- **pops**: a change between two frames much bigger than the frames around
  it, or a part that appears or goes in one frame while well opaque.
  Entering sight past the edge of a scrolling list is not a pop;
- **a part outside its container** (`checks.within`), and **two parts
  covering each other** (`checks.apart`);
- **a part that should have left** (`leaves`): still drawn at the end, or
  gone from sight but still taking the pointer. With it, when each part was
  last drawn. Also **a part that should have arrived** (`arrives`);
- **closing against opening reversed** (`reverseOf`): for each part and
  channel, how long each took, whether closing ends where opening began, and
  how far the closing's curve is from the opening's played backwards (0 is
  the same curve);
- **the app against the mockup** (`--mockup`): the same steps on
  `docs/ui/mockup.html` in a headless Edge or Chrome, with a profile of its
  own in a temporary folder. Durations, start times and curve distances,
  part by part. The mockup holds for durations and curves (CLAUDE.md).

## Where it runs

**Desktop: a test copy of the app, never the person's.** It starts the
dx-built app (`dx build --platform desktop --package dirt-desktop`; `--exe`
names another) with every development stand-in set:

- its own `DIRT_DATA_DIR` and `WEBVIEW2_USER_DATA_FOLDER` in a temporary
  folder;
- `DIRT_WINDOW_AT` and `DIRT_TRAY_AT` off every screen, so the copy never
  takes the keyboard;
- `DIRT_ACTIONS_FROM_STDIN`, never sent `open dirt`, and `quit` to end it;
- `DIRT_OPENED_TO_FILE`, `DIRT_SIGN_IN_TO_FILE` and `DIRT_EXPORT_TO`, as
  files in its data folder;
- `DIRT_API_BASE_URL` pointed at a closed local port, so that no server is
  ever reached;
- its own DevTools port from 9581 (never 9222, 9333, 9377, 9471 or
  9563–9565), with Chromium's occlusion and background throttling turned
  off.

It is stopped by its process ID, and its folder is removed.

The app holds a single-instance lock, so a copy will not start beside a
Dirt that is already running. `--beside-running` copies the app's folder to
the temporary folder and renames the lock in the copy, keeping its length.
Neither the repository nor the original folder is changed. The copy leaves
out the voice model unless `--voice` is given.

A window off the screen may be held back by Chromium. Every run measures
how often the page drew while idle and says in the report whether real time
can be judged. Off the screen, with the flags above, WebView2 drew at
56–57 frames a second on a 60 Hz laptop.

**Android: the emulator.** The tool starts the app if it is not running.
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
node tools/motion/motion.mjs run note-open-close --platform android --serial <phone> --real-phone
```

The phone needs a debug build and USB debugging. Everything runs the same,
the screen recording included, and the recording is deleted from the phone
afterwards. Only a phone's numbers say how a phone performs; the
emulator's do not.

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
    { "name": "close", "steps": [{ "key": "Escape" }], "reverseOf": "open", "leaves": ["panel"], "ms": 1200 }
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
- **`movements`**: captured in order, all of them stepped, then all of them
  in real time. So they should end where they began (open, then close), or
  `between` lists steps that reset the app between the two passes. Each
  movement has:
  - `steps`: its input;
  - `ms`: the longest it may take;
  - `reverseOf`, `leaves` and `arrives`, as above;
  - `crop`, for that movement only;
  - `compareIn: "realtime"`, to compare it with the mockup in real time
    rather than stepped (a smooth scroll);
  - `platforms`, to run it on some platforms only;
  - `after`: steps run once it is captured.
- **Steps**:
  - `click` / `tap` (a selector, with `index` for the n-th drawn match);
    `click` presses with the mouse, or with a touch on Android;
  - `hover`;
  - `key` (`Escape`, `Enter`, `Tab`, `' '`, the arrows; with `ctrl`,
    `shift` or `alt`);
  - `type` (inserted as typed text);
  - `wheel`;
  - `wait` (ms), `waitFor` / `waitGone` (a selector);
  - `pastePicture` (a picture pasted into the page, as one copied from
    another program; the app's own paste handler takes it);
  - `androidKey` (an Android key code, on the emulator: 4 is Back);
  - `eval` (for a mockup's start state only, never for the input of a
    movement).

  Any step can carry `then` (ms to wait after it), `platforms` and
  `repeatUntilGone` (press again until a selector has gone: Android's page
  bar takes the first touch while typing).

All pointer and key input goes through DevTools' `Input` domain into the
test app's own web view, as a hand would give it. Nothing reaches the
person's screen, keyboard or mouse.

**Never press a file tile's buttons.** The first opens the file in another
app, on the person's screen. Open the preview by `.dui-file-main`.
`DIRT_OPENED_TO_FILE` stands in if anything else is pressed.

## Reading the output

Each run is a folder `<target>/motion/<scenario>-<platform>-<time>/`. The
target is `CARGO_TARGET_DIR`, or the repository's `target/`, so a run made
in a worktree that shares the main target outlives the worktree. `--out`
names another place. The folder holds:

- **`report.md`**: start here. The idle frame rate, and whether real time
  can be judged. For each movement:
  - the stepped frames, and the animations the clock saw (what, on what,
    delay, duration, easing);
  - the real-time frame rate and its gaps;
  - each finding with its millisecond;
  - closing against opening reversed;
  - the app against the mockup.
- **`stepped-<movement>(-n).png`**: the stepped frames, at most 24 to a
  sheet, each stamped with its millisecond. A red border is a finding,
  named under the frame. A gold note is an animation starting there.
- **`realtime-<movement>(-n).png`**: the frames as composited, with the gap
  since the frame before. A red border is a gap over 25 ms.
- **`screen-<movement>(-n).png`** (Android): the device's own recording.
  Its times are the video's, from when recording began.
- **`reversed-<movement>.png`**: the opening forwards above, the closing
  backwards below, column by column at the same fraction of each. If one
  were the other reversed, each column's two frames would match.
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
- **A stepped finding that surprises is checked against real time.**
  Once, closing the file preview, the stepped capture lost the picture
  inside the preview's frame at 33 ms. Real time showed it shrinking into
  its tile to the end. The cause was not found. When the two layers
  disagree, a person sees the real-time frames.
- **On Android the real-time probe costs frames.** Measuring every part on
  every frame is work for the page. On the emulator, opening Settings drew
  14 to 19 frames a second with it and 23 to 27 without. Judge Android's frame rate from
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
| Node, no packages | Node 22 has a WebSocket of its own, so the DevTools Protocol needs nothing installed. The analysis is pure functions with `node --test` tests, and CI runs them (`motion-tools`) |
| Sheets drawn by the browser | The issue suggested ffmpeg for the contact sheets. They are an HTML grid photographed by the headless browser that the mockup already needs. That gives a label on every frame, real-time frames at uneven times, findings in red beside the frame that shows them, and two rows side by side, which ffmpeg's tile filter does not. ffmpeg stays for the screen recording and the video |
| The stepped clock | Stopping the timeline alone stepped the desktop correctly. Android's WebView drew a compositor-run opacity animation at its start while the page reported it nearly done, so each animation is also paused, and finished as the clock passes its end |
| Real-time stamps | WebView2 counted `requestAnimationFrame`'s timestamps from another origin than `performance.now()`, 12.7 s apart. The recorder moves them onto `performance.now()` once, at the first frame |
| A page that outlives a run | Android's web view keeps what an earlier run installed. The page script carries a version and replaces an older one |
