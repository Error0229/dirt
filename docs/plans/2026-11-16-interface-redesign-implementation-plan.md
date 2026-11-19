---
title: "feat: Build the interface redesign of 2026-11-16"
type: feat
status: active
date: 2026-11-16
design: docs/ui/design.md
---

# feat: Build the interface redesign of 2026-11-16

## Overview

This plan builds the design in [`docs/ui/design.md`](../ui/design.md): a new
interface for the desktop app and the Android app, and the features it adds
(archive, undo, voice, capture from outside the app, the file preview, the
pile). [`docs/ui/mockup.html`](../ui/mockup.html) shows it working.

The owner approved the design on 2026-11-16. A review the same day found
what had to be settled before building; the answers are under "Decided"
below and are written into the design. What the design asks of the rest of
the system is in the system design, section 16.

Two trials come first, because either result can change what is built.
Step 1 and step 2 do not wait for them.

## Rules for every step

- The rules in `CLAUDE.md` hold. The two that matter most here: no SQL,
  HTTP or cryptography in a client crate, and `dirt-ui` counts as one; the
  words about sync come from `dirt_core::sync::overview`.
- Where the mockup and the design document disagree, the mockup holds for
  how long a movement takes and on which curve, and the document for
  everything else. A step that finds a disagreement corrects the document
  in its own pull request.
- The mockup is a reference for look and timing, not code to port. About
  450 of its 3,000 lines of script are superseded by later ones, and it
  keeps its state in the page; the apps keep theirs in Rust.
- Each app works after every pull request. The old interface is replaced
  screen by screen, and the pull request that replaces a screen deletes the
  old one.
- A pull request that changes what a client does changes its rows in
  `docs/feature-parity.md` and the items in `docs/MANUAL_CHECKLIST.md`.
- Before it merges, a step is run on Windows and on the Android emulator,
  and its pull request says what was run and what was not. What needs a
  real phone, macOS or Linux goes into the manual checklist.
- Everything is tried against the trial server, never against production.
- Motion that follows a finger or a pointer runs in the page's script from
  first touch to release and tells Rust only the outcome. A call into Rust
  for every movement is too slow on a phone.
- The script that animates the list watches the page for changes and plays
  the difference from the positions it last saw. It is not called once per
  change from Rust, because Rust cannot measure the page before it changes
  it.

## Decided (2026-11-16)

| Question | Decision |
|---|---|
| A system notification on Android when a device asks to join | Not built. The banner inside the app is enough: comparing the code needs both devices in hand |
| Must a thought captured from the widget reach other devices without opening the app | Yes, if trial 2 finds a way. Otherwise it syncs the next time the app is opened |
| Chinese: search, tags, voice | Not in this work. English first. Recorded as later work in the system design, section 16.3 |
| Who settles what the design leaves open or gets wrong | Whoever builds it, writing the answer into the design document |
| Words on screen while the person speaks (decided 2026-11-17, after trial 1's first result) | Given up. What matters is that the words kept are accurate, and a larger model is welcome for that. Nothing is written while listening; the recording is kept at once as a note and its words are made afterwards. Trial 1's second measurement chooses the engine and the model |
| Which speech model (decided 2026-11-17, after trial 1's second measurement) | Parakeet TDT-CTC 110m first: 130 MiB, 6.4% of words wrong, about 0.6 GiB while it works, three times as fast as Parakeet TDT 0.6b v2, whose 4.8% is not worth five times the size and twice the memory for now. It is English only, as voice is (system design 16.3); its licence, CC BY 4.0, asks for credit, which step 9 gives in the app, beside the model's name Parakeet v2 stays the step up if the 110m's words prove not good enough |
| PDF in the preview (decided 2026-11-18) | A renderer is bundled, so every platform draws a PDF's pages in the preview: fewer steps than opening another app. pdf.js, a few MB, is the one the design names; step 6 chooses and records it |
| The speech model: fetched on first use, or shipped in the app (decided 2026-11-18) | Shipped in both apps. Parakeet TDT-CTC 110m is 130 MiB; voice works offline from the first second, nothing is fetched, and Dirt's storage is not touched |
| Snap Layouts on Windows 11 (decided 2026-11-18) | Skipped for now: the window keeps working without the flyout. Winning it back stays possible later |

## Still the owner's

Each has a default that holds if the step is reached without an answer.

| Decision | Needed by | Default |
|---|---|---|
| Nothing at present | | |

## Trials

A trial is code on a branch that answers questions and is thrown away. Its
result is a section added to this document.

### Trial 1: the voice engine

The design needs words on screen as they are spoken, on a phone, with
nothing leaving the device. Whisper works on a finished recording, not on
speech as it arrives.

Measure, for whisper.cpp with a sliding window, for a streaming recogniser
(sherpa-onnx), and on Android for the system's on-device recogniser:

- The time from a word being spoken to its being shown, and how often a
  shown word later changes.
- How fast it runs against real time, and the memory it takes, on this
  laptop and on a phone.
- Mistakes on a fixed set of about twenty short spoken thoughts in English.
- Whether a pause of 1.6 seconds after speech can be told from a pause in
  the middle of a thought.
- Whether it builds for Windows, macOS, Linux and both Android targets, and
  what it adds to a build.
- The model each needs, and its size.
- On Android, whether the system's recogniser can be given sound the app
  records itself, so that the recording can be kept.
- A format for the recording that every platform can write and every web
  view can play, at about ten minutes inside the 10 MiB limit on a file.

The phone's half runs a test program on the owner's phone through `adb` and
needs the owner's word first. The emulator's timings say nothing about a
phone.

**It decides:** the engine and where it lives (a `voice` feature of
`dirt-core`, or a `dirt-voice` crate), the model put to the owner, and the
recording's format.

### Trial 1 result

> **Superseded in part.** The streaming recommendation below was superseded
> by the owner's decision of 2026-11-17 that words need not appear while the
> person speaks. See "Trial 1, second measurement: accuracy after the
> recording ends", after this section, for the engine's model and how a
> finished recording is read.

Run on 2026-11-16 and 17. The measuring code is on the branch
`trial/voice-engine`, in `trials/voice/`; it is not merged.

**What it found, in short**

- **The engine is sherpa-onnx with a streaming model.** It showed a word
  0.5 to 1.1 seconds after the word ended, never changed a word it had
  shown, and worked at 5 to 13% of real time on a laptop busy with other
  work. whisper.cpp read through a sliding window showed half its words as
  soon, but it took 83 to 148% of real time on the same busy laptop, lagged
  the microphone by 2.5 to 6.7 seconds at worst, and replaced unsettled
  words 13 to 90 times for every hundred words it showed.
- **It lives in a crate of its own, `dirt-voice`.**
- **Android uses the same engine**, not the system's recogniser.
- **The end of a thought** is found by Silero's voice-activity model, which
  sherpa-onnx runs: it never took a pause of up to 1.4 seconds for the end.
- **The recording is Ogg Opus**, 16 kHz mono at 24 kbit/s: 1.6 MiB for ten
  minutes.
- **The model is the owner's choice between two**, 68 or 110 MiB, set out
  under "Recommendation".
- **The phone was not run.** Everything about a phone's speed is unknown.

#### How it was measured

**The test set** is 22 short thoughts, 374 words, 146 seconds. None of it
was recorded for the trial and no microphone was used.

| | Thoughts | Words | Length of each | What it is |
|---|---|---|---|---|
| Human | 10 | 229 | 4 to 13 s | LibriSpeech test-clean (CC BY 4.0): ten readers, one sentence each from an audiobook. The first utterance of 4 to 15 s in the first chapter of each of the first ten speakers, by number |
| Synthetic | 12 | 145 | 2 to 7 s | Thoughts of the kind Dirt is for ("Ask Jordan about the Q3 offsite", "Buy oat milk and stamps"), spoken by Windows' two built-in English voices |

A synthetic voice flatters a recogniser: it is clean and evenly paced. The
human half flatters in another way: the two zipformer models from 2023 were
trained on LibriSpeech's training set, read by other people from other
books, and book English is not how a thought is said. Neither half has an
accent, a noisy room or someone thinking aloud. The mistakes below rank the
models; they do not say how many mistakes a person will see.

**Feeding.** The program hands an engine the sound in pieces of 100 ms, each
at the moment its last sample would have been recorded, followed by three
seconds of room quiet. It writes down every change to what would be on
screen and when. Each configuration ran three times; the figures are
medians. The runs used a clock that counts the engine's working time as
time passed and waits for nothing else, which gives the same delays as the
wall clock and finishes sooner. One sherpa-onnx configuration was run on
the wall clock to check it, and showed a word 0.62 s after it ended against
0.61 s.

**When a word counts as shown.** A word's delay is the time from the end of
the word in the recording to the first moment it is on screen, counted for
the words that ended up right. The end of each word comes from the
published forced alignments of LibriSpeech (Lugosch et al., CC BY 4.0) for
the human half, and for the synthetic half from the synthesiser's own
report of where each word starts.

**Mistakes** are the word error rate: words wrong, missing or added, out of
the 374. Case and punctuation are not counted, numbers are spelt out
("3 pm" and "three p m" are the same), and "off-site", "offsite" and "off
site" are the same word.

**The machine was busy, and the speeds are pessimistic.** A laptop with an
Intel Core i5-13500H (12 cores, 16 threads), Windows 11. Two other tasks
were compiling on it and another program held about six cores. While the
figures in the tables were taken, 81 to 96% of processor time was in use on
average (55% during one run of `small.en`) and the processor ran at about 60
to 75% of its nominal clock speed. The measuring program ran with 4 threads
at high priority. Some of the trial's own builds ran beside the whisper
runs.

The other work stopped once, for a few minutes, and one whisper
configuration ran in that time: it was 2.7 times as fast as in the table
(see "whisper with the laptop quiet" below). So the speeds in the tables
are slower than this laptop can do, by about that much for whisper;
sherpa-onnx was not run with the laptop quiet. A third batch of sherpa-onnx
runs, hours later and busy again, gave the same delays to within 0.03 s.

The laptop also went to sleep in the middle of the first batch of whisper
runs. The run it cut through was thrown away. The four whisper
configurations that had finished before the sleep are one of each one's
three runs; `small.en`'s three are all from after it.

#### Measured on this laptop: words as they are heard

Median of three runs, 4 threads, laptop busy.

| Engine and model | Model, MiB | Mistakes: all (human, synthetic) | Shown after the word ended: median (9 in 10) | Changed, per 100 words | Speed, of real time | Worst lag | Memory, MiB | Load |
|---|---|---|---|---|---|---|---|---|
| sherpa-onnx, zipformer-en-20M (8-bit) | 42 | 24.3% (22.3, 27.6) | 0.51 s (0.72 s) | 0 | 0.05 | 0.0 s | 113 | 1.6 s |
| sherpa-onnx, zipformer-en-2023-06-26 (8-bit) | 68 | 4.8% (1.7, 9.7) | 0.61 s (0.83 s) | 0 | 0.10 | 0.0 s | 163 | 7.9 s |
| sherpa-onnx, Kroko English | 68 | 2.4% (3.9, 0.0) | 1.08 s (1.64 s) | 0 | 0.05 | 0.1 s | 182 | 4.9 s |
| sherpa-onnx, NeMo fast conformer 480 ms (8-bit) | 131 | 3.7% (2.6, 5.5) | 0.75 s (1.02 s) | 0 | 0.13 | 0.0 s | 227 | 4.9 s |
| sherpa-onnx, Moonshine tiny read again every 0.3 s | 42 | 7.0% (7.9, 5.5) | 0.23 s (0.63 s) | 124 | 0.63 | 0.8 s | 174 | 1.9 s |
| sherpa-onnx, Moonshine tiny read again every 0.5 s | 42 | 5.6% (5.7, 5.5) | 0.31 s (0.67 s) | 77 | 0.33 | 0.7 s | 173 | 1.3 s |
| sherpa-onnx, Moonshine base read again every 0.5 s | 135 | 3.5% (3.1, 4.1) | 0.39 s (0.80 s) | 89 | 0.56 | 1.2 s | 358 | 1.6 s |
| whisper.cpp, `tiny.en`, full window | 74 | 4.3% (5.2, 2.8) | 2.25 s (3.47 s) | 13 | 1.16 | 3.0 s | 243 | 0.4 s |
| whisper.cpp, `tiny.en`, short window | 74 | 5.1% (6.6, 2.8) | 0.55 s (1.44 s) | 29 | 0.83 | 2.5 s | 243 | 0.3 s |
| whisper.cpp, `base.en`, short window | 141 | 2.1% (3.1, 0.7) | 0.74 s (1.75 s) | 90 | 1.03 | 4.0 s | 356 | 0.7 s |
| whisper.cpp, `base.en` 5-bit, short window | 57 | 3.5% (5.2, 0.7) | 0.73 s (1.84 s) | 31 | 1.01 | 3.5 s | 272 | 0.3 s |
| whisper.cpp, `small.en`, short window | 465 | 1.3% (1.7, 0.7) | 2.92 s (5.73 s) | 14 | 1.48 | 6.7 s | 795 | 1.5 s |

- **Shown** is the delay of a word's first appearance: the median, and the
  delay nine words in ten came in under.
- **Changed** is how many times a word on screen was replaced or taken
  away, for every hundred words finally shown. A last word still growing
  ("off", then "offsite") is not counted.
- **Speed** is the engine's working time as a share of the length of the
  sound, the three seconds of quiet included. Near 1.0 it is working all
  the time; over 1.0 it cannot keep up.
- **Worst lag** is the longest the engine was behind the microphone.
- **Memory** is the process's peak working set, model included.
- **Load** is the time to load the model before the first word can be
  heard.
- **Full window, short window.** whisper reads thirty seconds whatever it
  is given. "Short window" tells it to read only as much as the sound so
  far, which whisper.cpp allows and warns can cost accuracy. "Read again"
  for Moonshine is the same sliding window, with a model that reads only
  what it is given by design.

**whisper with the laptop quiet.** One run, on the wall clock, when 20% of
processor time was in use: `tiny.en` with the short window took 0.30 of
real time against 0.83 in the table, showed a word 0.24 s after it ended
(9 in 10 within 0.61 s), was 0.8 s behind at worst, and changed 31 words
per 100. Read once over a finished thought it took 0.55 s against 1.64 s.

What the table does not show:

- **A streaming model never takes a word back.** Read greedily, it only
  adds. Nothing it showed changed, in any run. Its delay belongs to the
  model more than to the machine: it reads sound in pieces of about a third
  of a second and waits for what follows a word before committing to it.
  The delays were the same in all three batches, whatever else the laptop
  was doing.
- **A sliding window churns.** whisper and Moonshine read this way show a
  word early and then keep changing the words not yet settled: 13 to 124
  changes for every hundred words. No settled word ever changed, because a
  word settles only when two readings agree on it.
- **whisper can loop.** With the short window it sometimes fell into
  repeating itself on a thought it had heard half of ("blue silk stockings,
  blue silk stockings, …" for 128 words; "OUT OUT OUT …" for 225). The loop
  was on screen as unsettled words until the next reading, and cost that
  reading several seconds. The usual guard, reading again at a higher
  temperature, was turned off because it multiplies the time of a reading;
  a build for people would need a guard of its own.
- **A sliding window costs more as the thought grows.** Every reading
  reads all the sound so far. That is where whisper's worst lags come from,
  on the thoughts over ten seconds.
- **The smallest streaming model loses the first words** of a thought
  spoken straight away. It does so on its own sample file too, and half a
  second of quiet put in front did not cure it (24.9%). That is most of its
  mistakes here.
- **What each writes.** The two zipformer models of 2023 write capitals
  only and no punctuation ("ASKED JORDAN ABOUT THE …"); NeMo's writes small
  letters only; Kroko's, Moonshine and whisper write capitals and
  punctuation as a person would ("Ask Jordan about the Q three offsite.").
  A note kept from the first two kinds needs a second reading, or it reads
  badly.
- **Modern words.** The zipformer model of 2023-06-26 made 1.7% mistakes
  on the audiobook sentences and 9.7% on the thoughts: it had heard only
  books. "Q3", "Kyoto", "Priya", "login" and "email" came out as "keeth
  re", "Quito", "brio", "logging" and "emile".

**Others looked at and not measured.** NVIDIA's larger streaming models
that sherpa-onnx publishes (Parakeet and Nemotron, 0.6 billion parameters)
are 460 to 500 MB each, too large to ask a phone to fetch for a note app.
Vosk's small English model is 40 MB, but its Rust crate was last released
in 2024 and needs its own prebuilt library on every platform; with
sherpa-onnx already giving a streaming model, a detector and a second
reading from one library, it was left out.

#### Measured on this laptop: reading a finished thought once more

A second, slower model can read a stretch of speech again when it ends, and
its text is what is kept. Time is for one thought of 2 to 13 seconds, laptop
busy.

| Model | Model, MiB | Mistakes | Time for one thought, median |
|---|---|---|---|
| Moonshine tiny (8-bit), through sherpa-onnx | 42 | 2.4% | 0.20 s |
| Moonshine base (8-bit), through sherpa-onnx | 135 | 2.1% | 0.33 s |
| whisper.cpp, `tiny.en` | 74 | 4.0% | 1.64 s |
| whisper.cpp, `base.en` 5-bit | 57 | 1.3% | 3.23 s |
| whisper.cpp, `base.en` | 141 | 1.1% | 3.37 s |
| whisper.cpp, `small.en` | 465 | 1.3% | 11.89 s |

Moonshine tiny added 112 MiB to the peak memory of the run it was loaded
beside, and Moonshine base 288 MiB.

#### Measured on this laptop: the end of a thought

Two questions. On the 22 thoughts: how long after the last word is the end
called, when 1.6 s of quiet is asked for? And on a pause set: is a pause
inside a thought taken for the end? The pause set is four of the thoughts,
each cut at a word boundary near its middle with a pause of 0.6, 1.0 or
1.4 s put in, clean and under pink noise 20 dB and 10 dB below the speech:
36 files.

| Way of finding the end | 22 thoughts: end called after the last word, median (worst) | Pause set: ended inside a pause | Pause set: no end within 3 s | Pause set: end called after the last word, clean / under noise |
|---|---|---|---|---|
| Silero's detector as the sound arrives (sherpa-onnx) | 1.75 s (2.47 s) | 0 of 36 | 0 | 1.74 s / 2.24 s |
| The same model over finished sound (whisper.cpp) | not run | 0 of 36 | 0 | 1.65 s / 1.67 s |
| The recogniser's own rule, NeMo's model | 2.23 s (2.60 s) | 0 of 36 | 0 | 2.26 s / 2.23 s |
| The recogniser's own rule, zipformer-en-2023-06-26 | 2.32 s (2.58 s) | not run | not run | not run |
| The recogniser's own rule, Kroko's model | 2.35 s (3.76 s) | 0 of 36 | 7 | 2.59 s / 2.69 s |
| Loudness alone | not run | 0 of 36 | 8, all of them clean files | 1.94 s / 1.54 s |

- **Silero's model tells them apart.** It called no end inside a pause of
  up to 1.4 s, in quiet or in noise, and missed no real end. The model is
  0.6 MiB. sherpa-onnx runs it as the sound arrives; whisper.cpp has the
  same model but runs it over finished sound, so a listening app would run
  it over the last few seconds again and again.
- **The recogniser's own rule is late.** sherpa-onnx can end a thought on
  quiet since the last word it decoded. Asked for 1.6 s it fired 2.2 to
  2.7 s after the last word, because the model's own delay is added, and
  with Kroko's model it did not fire within three seconds for 7 of 36
  files. It can be asked for less to land on 1.6, but the detector needs no
  such tuning.
- **Loudness alone is not enough.** A rule of "quiet is anything near the
  room's level" missed the end in 8 of 12 clean files.
- **No detector can do more than time the quiet.** A speaker who stops for
  1.6 s in the middle of a thought has ended it. One file of the pause set
  shows it: the speaker's own gap at a comma plus the 1.4 s put in came to
  1.8 s, and Silero's model ended the thought there when the room was
  quiet, as asked. That is the design's rule working, and the rule holds
  only for a thought started by a hardware key (design 6.18).

#### Measured on this laptop: building

Windows was built here with Visual Studio 2022's compiler (MSVC 14.40) and
cmake 4.4. Android was cross-compiled here from Windows with NDK 26.3 for
API level 24. Times are for a build that already has everything but the
engine, on the busy machine. "Adds" is against the same program built
without the engine, uncompressed.

| | whisper.cpp through `whisper-rs` 0.16 | sherpa-onnx 2.0.1 through its `sherpa-onnx` crate | The recording: `opus` 0.4 and `ogg` 0.9 |
|---|---|---|---|
| Tools it needs | cmake, a C++ compiler, and libclang for bindgen | None: its build script downloads prebuilt libraries | cmake and a C compiler; libopus comes with the crate |
| **Windows**: builds | Yes, after repairs 1 and 2 below | Yes, with the shared libraries | Yes |
| Windows: time | 159 s | 35 s, 8 MB downloaded | 61 s |
| Windows: adds | 1.2 MiB to the program | 0.1 MiB to the program and 21.1 MiB of DLLs (`onnxruntime.dll` 17.0, `sherpa-onnx-c-api.dll` 4.1) | 0.4 MiB |
| **Android arm64**: builds | Yes, after repairs 1, 3 and 4 | Yes | Yes |
| Android arm64: time | 133 s | 168 s with 45 MB downloaded for all four ABIs; 33 s once it is there | 85 s |
| Android arm64: adds | 1.1 MiB, and `libc++_shared.so`, 1.7 MiB | 0.1 MiB and 25.3 MiB of libraries (`libonnxruntime.so` 21.2, `libsherpa-onnx-c-api.so` 4.1) | 0.3 MiB |
| **Android x86_64**: builds | Yes, 123 s | Yes, 33 s | Yes, 119 s |
| Android x86_64: adds | 1.3 MiB, and `libc++_shared.so`, 1.6 MiB | 0.1 MiB and 29.0 MiB of libraries | 0.4 MiB |
| Libraries aligned for 16 KB pages | It ships none. What it builds is aligned when the linker is told; NDK 26.3's `libc++_shared.so`, which it needs, is not (`0x1000`) | Yes: every LOAD segment is `0x4000`, for arm64-v8a and x86_64 | It ships none; libopus is linked into the app's own library |
| **macOS, Linux**: from documentation, not built | whisper-rs's BUILDING.md gives both: cmake, clang and llvm on Linux; cmake on macOS | Its build script names prebuilt archives for Linux and macOS, x64 and arm64, and the files for 2.0.1 are published (9 to 13 MB each) | Not looked up |

What building found:

- **sherpa-onnx is not compiled.** Its crate's build script downloads the
  project's prebuilt libraries from its GitHub release and links them. It
  needs no cmake, clang or bindgen. It also means a build fetches binaries
  from the network, and the script does not check them against a hash. The
  build can be pointed at a folder instead (`SHERPA_ONNX_LIB_DIR`,
  `SHERPA_ONNX_ARCHIVE_DIR`): the real build should fetch the archive
  itself, check its hash, and do that.
- **sherpa-onnx's libraries are shipped beside the app.** On Android there
  are only shared ones. On Windows the static ones did not link here: they
  want the C runtime linked statically and a newer MSVC than 14.40
  (`__std_search_1` and others were unresolved), and they are 123 MB to
  download. The trial used the shared ones everywhere.
- **whisper needed four repairs to build from Windows**, all in
  `trials/voice/scripts/`:
  1. bindgen needs a libclang and must be told where the C headers are.
     Without it the crate falls back to bindings generated on Linux, which
     do not compile for Windows. Visual Studio's own LLVM served, so
     nothing had to be installed.
  2. With Visual Studio's generator the build compiled whisper.cpp with no
     optimisation, release build or not: one reading with `tiny.en` took 20
     seconds. Naming the release flags in the environment cured it.
  3. Cross-compiling for Android from a Windows computer, the crate adds
     MSVC's `/utf-8` to the compiler's flags and asks the linker for
     `advapi32`, because it tests the computer doing the build and not the
     target. The first is cured by naming the flags; the second needed an
     empty library of that name, and would need a patch to the crate.
  4. The NDK's toolchain file must be told the ABI through a wrapper,
     because the crate passes on only some variables.
- **whisper on Android links `libc++_shared.so`**, which must be shipped,
  and NDK 26.3's copy is aligned to 4 KB: it fails the 16 KB rule that
  `crates/dirt-mobile/build.rs` exists for.
- **On Windows whisper.cpp builds for the processor it is built on**
  (`GGML_NATIVE` is on by default and found AVX2 here). A release would
  have to name a baseline.

#### Models

| Model | On disk | Published at | Licence |
|---|---|---|---|
| whisper `tiny.en` | 74 MiB | huggingface.co/ggerganov/whisper.cpp | MIT |
| whisper `base.en` | 141 MiB; 57 MiB at 5 bits | The same | MIT |
| whisper `small.en` | 465 MiB | The same | MIT |
| Streaming zipformer-en-20M (2023-02-17), 8-bit | 42 MiB | sherpa-onnx's GitHub release `asr-models` | Apache-2.0 |
| Streaming zipformer-en (2023-06-26), 8-bit | 68 MiB | The same | Apache-2.0 |
| Kroko English, community (2025-08-06) | 68 MiB | The same; its makers at huggingface.co/Banafo/Kroko-ASR | Its makers' page says CC BY-SA for the community models. The page's licence field says "other", and its LICENSE file came back empty: to be confirmed before it is used |
| NeMo streaming fast conformer, 480 ms, 8-bit | 131 MiB | The same release; from NVIDIA's `stt_en_fastconformer_hybrid_large_streaming_multi` | CC BY 4.0 |
| Moonshine tiny, 8-bit (2026-02-27) | 42 MiB | The same release | MIT |
| Moonshine base, 8-bit (2026-02-27) | 135 MiB | The same release | MIT |
| Silero's voice-activity model | 0.6 MiB | The same release; in whisper.cpp's form at huggingface.co/ggml-org/whisper-vad, 0.8 MiB | MIT |

The engines: sherpa-onnx is Apache-2.0, whisper.cpp MIT, `whisper-rs`
Unlicense, libopus BSD.

#### Android's own recogniser

**From documentation, not measured.** No phone was touched, and `adb` was
not used.

| Question | Answer, from Android's reference for `SpeechRecognizer` and `RecognizerIntent` |
|---|---|
| Is it there? | `createOnDeviceSpeechRecognizer` is Android 12 (API 31), and throws unless `isOnDeviceRecognitionAvailable` is true. Dirt supports API 24 and later |
| Partial results | `EXTRA_PARTIAL_RESULTS` asks for them, and "the server may ignore a request for partial results in some or all cases" |
| Languages on the device | No list is published. `checkRecognitionSupport` (API 33) asks the phone's recognition service what it has. `ERROR_LANGUAGE_UNAVAILABLE` means "supported, but not available currently (e.g. not downloaded yet)" |
| Sound the app records itself | `EXTRA_AUDIO_SOURCE` (API 33) takes a `ParcelFileDescriptor`, 16-bit PCM by default, with the rate and channel count in companion extras. "If this extra is not set or the recognizer does not support this feature, the recognizer will open the mic": the app is not told which happened |
| Capitals and punctuation | `EXTRA_ENABLE_FORMATTING` (API 33); "depending on the recognizer implementation, this value may have no effect" |

It does not fit. It is absent below Android 12 and wherever the phone has
no such service; each answer above ends in "if the implementation supports
it"; the recording could be kept only from Android 13 and only if the
service agrees; and what the service does with the sound would be its
promise, not Dirt's. With the same engine as the desktop, Android behaves
as the desktop does, and the promise in design 6.18 is Dirt's own to keep.

#### The recording's format

Ten minutes of the test set's speech, 16 kHz mono, written in each format
here. The sizes are measured.

| Format | Ten minutes | Written from Rust | WebView2 | Android WebView | WKWebView | WebKitGTK |
|---|---|---|---|---|---|---|
| Ogg Opus, 16 kbit/s | 1.08 MiB | libopus and the `ogg` crate: done here | Plays | Yes | macOS 15.4 and later | Yes, expected |
| Ogg Opus, 24 kbit/s | 1.59 MiB | The same | Plays | Yes | macOS 15.4 and later | Yes, expected |
| Ogg Opus, 32 kbit/s | 2.10 MiB | The same | Plays | Yes | macOS 15.4 and later | Yes, expected |
| WebM Opus, 24 kbit/s | 1.72 MiB | libopus and a WebM writer: not tried | Plays | Yes | Safari 17.4 and later | Yes, expected |
| M4A, AAC-LC, 32 kbit/s | 2.38 MiB | No encoder in Rust. A C library, or each platform's own; Linux has none | Plays | Yes | Yes | Only with a further GStreamer plugin |
| FLAC | 9.27 MiB | A pure Rust encoder exists (`flacenc`): not tried | Plays | Yes | Yes | Yes, expected |
| WAV, 16-bit | 18.31 MiB | Trivial | Plays | Yes | Yes | Yes, expected |

- **WebView2: measured, through a stand-in.** Headless Edge 158, the same
  engine at the same version as the WebView2 on this machine. The page
  asked `canPlayType`, then loaded a file of each format and played it
  until the clock moved. "Plays" means both. No window was shown.
- **The other three columns are from documentation, not tested.** Android
  WebView and WKWebView are from caniuse.com's tables for Android's
  browser and for Safari, which WKWebView follows. WebKitGTK plays what
  the GStreamer plugins on the computer can: Ogg, Opus, FLAC and WAV are
  in the plugin sets a distribution installs with it, AAC is not always.
- **No compressed format is certain in all four.** Ogg Opus needs macOS
  15.4; WebM Opus needs Safari 17.4; AAC on Linux needs a plugin that is
  usually there and not always, and cannot be written from Rust alone.
  FLAC plays everywhere, but ten minutes came to 9.3 MiB on clean speech,
  too near the 10 MiB limit to rely on. WAV is over it.
- **Ogg Opus was written from Rust here** with the `opus` crate (libopus)
  and the `ogg` crate (pure Rust), in about eighty lines. The file played
  in headless Edge and decoded in ffmpeg, and the code cross-compiled for
  both Android targets. Writing ten minutes took 20 s on the busy laptop,
  3% of real time. Android's own encoders are not needed, which matters
  because (by Android's list of media formats) its Opus encoder exists only
  from Android 10.
- **An Ogg file does not say how long it is at its start.** Served in a
  way that cannot be jumped about in, the web view gave its length as
  endless; handed over from memory as a blob, it gave 5.14 s. The app
  hands a file's bytes over from memory (design 6.16), so the second case
  is the one that applies.

#### Recommendation

**Engine: sherpa-onnx, with a streaming model and Silero's detector.** One
engine and one library for the desktop and Android, with nothing to
compile. It never rewrote a word, and it took a twentieth to an eighth of
real time on a busy laptop: between six and sixteen times less than
whisper's smallest model beside it. That margin is the best evidence there
is for a phone until a phone is run. whisper's `base.en` made the fewest
mistakes of anything near real time, and on a quiet laptop its smallest
model kept up easily. What counts against it is that its unsettled words
churn, its cost grows with the thought, and its build needed four repairs
and fails the 16 KB rule on Android as it stands.

**Where it lives: `dirt-voice`, a crate of its own**, not a `voice` feature
of `dirt-core`.

- Cargo joins a crate's features across everything built in one command.
  With a `voice` feature on `dirt-core`, turned on by the desktop app,
  `cargo test --all` and `cargo clippy --workspace` would build `dirt-core`
  with it for the CLI and `dirt-sim` too. The design asks that the CLI, the
  server's tests and most CI jobs do not build the engine; a feature of
  `dirt-core` keeps that only for commands that name one package.
- The engine downloads native libraries when it builds and needs them
  shipped beside the app. That belongs in one crate where it can be seen,
  not in the crate that holds the store and sync.
- `dirt-voice` takes sound and a model's folder, and gives back words, the
  end of the thought, and the recording as Ogg Opus. It has no SQL, HTTP or
  cryptography. Fetching the model and checking its hash is a download like
  any other and stays in `dirt-core`.
- The two apps depend on it behind a `voice` feature of their own. The
  root `Cargo.toml` takes `crates/*` as members, so `dirt-voice` is named
  in `exclude` and built only when an app's feature asks for it.
  `--all-features` in the lint command would ask for it: step 9 decides
  whether the lint job is the one job that builds it.

**Android: the same engine**, through `dirt-voice`, fed by `AudioRecord`.
Not the system's recogniser, for the reasons above.

**The model: one of two, and the choice is the owner's.**

| | One model: Kroko English | Two models: zipformer-en-2023-06-26 for the words as they are heard, Moonshine tiny for the text that is kept |
|---|---|---|
| To fetch | 68 MiB | 110 MiB (68 and 42) |
| Licence | CC BY-SA by its makers' page, to be confirmed with them. Dirt must credit it, and a changed model must be shared alike | Apache-2.0 and MIT |
| Word shown after it ended | 1.08 s (9 in 10 within 1.64 s) | 0.61 s (0.83 s) |
| Mistakes in the words as shown | 2.4% | 4.8%, and 9.7% on the thoughts |
| Mistakes in the text kept | 2.4% | 2.4% |
| Capitals and punctuation | As the words appear | When a stretch of speech ends at a pause: the second reading replaces it, in 0.2 s here |
| Memory while listening | 182 MiB | 163 MiB measured for the first model; about 275 MiB with the second, going by the 112 MiB it added to another run |
| What can go wrong | The licence | Two models to fetch, check and keep in step; the kept words can differ from the grey ones |

Put Kroko's to the owner first. It is the smaller download and the simpler
build, it made the fewest mistakes, and it alone writes a thought the way
it would be typed while it is being said. It costs half a second more
before a word shows, and a licence the owner has to accept and its makers
have to confirm. If the owner would rather not, or step 9 is reached
without an answer, the pair is the default: its licences ask nothing.
Either way the app also carries sherpa-onnx's libraries: 21 MiB on Windows
and 25 MiB for each Android ABI, uncompressed.

**The recording: Ogg Opus**, 16 kHz mono, 24 kbit/s, written by
`dirt-voice` from the same samples the recogniser hears, in a file ending
`.opus`. Where the web view says it cannot play it (`canPlayType`; by the
documentation that is macOS before 15.4), the app plays a WAV that
`dirt-voice` decodes into memory: ten minutes is 18 MiB there, and nothing
is written to disk. libopus is already linked for writing, so reading costs
nothing more.

**For whoever builds step 9:**

- Start the microphone before the model is ready and let the words catch
  up. Loading took 5 to 10 seconds here for either 68 MiB model, on a busy
  machine. The engine reads sound many times faster than it is spoken, so
  it catches up at once; kept loaded, there is no wait.
- Ask Silero for the 1.6 seconds, not the recogniser.
- When a word is settled depends on the model, because a settled word
  must never change (design 6.18). With one model that reads once (Kroko),
  the grey word is the last word of the recogniser's text and everything
  before it is settled. With the two models, the first one's words are all
  grey until the pause at which the second reads the stretch again; what
  the second gives replaces them and is settled then, corrections and all.
- Fetch sherpa-onnx's libraries by a pinned hash, not as the crate's build
  script does.

#### Still unknown until it is run on a real phone

- **Speed and delay on arm64.** Every figure is this laptop's. The margin
  is wide, but a phone's cores, its mix of fast and slow ones under 4
  threads, and what heat does over ten minutes are not known.
- **Memory on a phone**, and whether Android lets the app hold 160 to
  300 MiB while it listens over the lock screen.
- **How long the model takes to load** from a phone's storage.
- **The second model's time** for a stretch of speech on a phone.
- **16 KB pages in fact.** The libraries are aligned for it; none was
  loaded on a phone that has such pages.
- **Battery** over a long thought.
- **The microphone.** Every figure is for clean recorded sound: no
  `AudioRecord`, no `cpal`, no room, no pocket.
- **A long thought.** The longest in the set is 13 seconds; the design
  allows ten minutes.

Also not known, and not the phone's to answer:

- **sherpa-onnx with the laptop quiet.** Its speeds are from the busy
  laptop only.
- **macOS and Linux.** Neither was built.
- **Playing the recording in WKWebView, WebKitGTK and Android's WebView.**
  `trials/voice/scripts/can-play.html` is the page to open in each.
- **Real speech**: accents, noise, and thinking aloud with its false
  starts.
- **Kroko's licence**, until its makers confirm it.

#### The phone's half: the commands

Not run. It needs the owner's word, a phone with USB debugging on, and no
other device connected. It installs nothing: it pushes a folder to
`/data/local/tmp`, runs a program there, and removes the folder. The
program is proven to build for arm64 (`scripts/build-android.sh
aarch64-linux-android "whisper sherpa"`). All of it is
`trials/voice/scripts/phone.sh` on the branch. In short, from
`trials/voice`, with the models fetched and the test set made as its
README says:

```bash
export MSYS_NO_PATHCONV=1                      # Git Bash must leave /data/... alone
there=/data/local/tmp/voice-trial
libs=target/sherpa-onnx-prebuilt/jniLibs/arm64-v8a
ndk=$ANDROID_HOME/ndk/26.3.11579264/toolchains/llvm/prebuilt/windows-x86_64
model=sherpa-onnx-streaming-zipformer-en-2023-06-26

adb devices                                    # one phone, and the right one
adb shell "mkdir -p $there/set $there/models/$model"
adb push target/aarch64-linux-android/release/voice-trial $there/
adb push $libs/libsherpa-onnx-c-api.so $libs/libonnxruntime.so $there/
adb push $ndk/sysroot/usr/lib/aarch64-linux-android/libc++_shared.so $there/
adb push data/set/manifest.tsv data/set/*.wav $there/set/
adb push data/models/sherpa/$model/*.int8.onnx data/models/sherpa/$model/tokens.txt $there/models/$model/
adb push data/models/sherpa/silero_vad.onnx $there/models/
adb shell "chmod 755 $there/voice-trial"

adb shell "cd $there && LD_LIBRARY_PATH=. ./voice-trial hear --engine sherpa \
  --model models/$model --vad models/silero_vad.onnx \
  --set set/manifest.tsv --threads 4 --out phone-sherpa-zipformer.1.jsonl"

mkdir -p data/phone-results
adb pull $there/phone-sherpa-zipformer.1.jsonl data/phone-results/
adb shell "rm -r $there"
python -I scripts/analyse.py data/phone-results
```

The script runs each configuration three times on the wall clock, and does
the same with Moonshine as the second model and with whisper's `tiny.en`
and `base.en`. Kroko's model is run the same way by naming its folder.

### Trial 1, second measurement: accuracy after the recording ends

Run on 2026-11-17, after the owner decided that words need not appear
while the person speaks: the recording is kept as a note at once, and its
words are made afterwards on the device that recorded it. So the question
became which engine and model write the most accurate English text for a
finished recording, read once, within what a phone can do. Streaming and
words that change no longer count. The measuring code is on the branch
`trial/voice-engine`, in `trials/voice/` (`voice-trial read`,
`scripts/run2.sh`, `scripts/analyse2.py`, `scripts/analyse-long.py`); it is
not merged.

**What it found, in short**

- **The engine stays sherpa-onnx**, now with an offline model that reads a
  finished recording. Its offline models come in the same libraries as its
  streaming ones, so the build costs nothing more than "Trial 1 result"
  says.
- **Put NVIDIA's Parakeet TDT 0.6b v2 (8-bit, 631 MiB, CC BY 4.0) to the
  owner as the default.** It made 4.8% mistakes on 4,598 words of human and
  synthetic English. whisper `small.en` made 7.1% and whisper
  `large-v3-turbo` 6.5%; both differences are more than chance. It writes
  capitals and punctuation. On this laptop it read a 60 second recording
  in 6.1 s, holding 1.1 GiB.
- **The smaller alternative is Parakeet TDT-CTC 110m** (130 MiB, CC BY
  4.0): 6.4% mistakes, 1.8 s for 60 s, 0.6 GiB.
- **No larger model earns its size.** The only model ahead of Parakeet v2
  (NVIDIA's Parakeet unified, 4.5%) is the same size, ahead by less than
  chance, and under a licence of NVIDIA's own. The large whisper models made
  more mistakes than Parakeet v2, were 3 to 14 times slower on a minute of
  speech, and held up to twice the memory. If the owner wants a larger one
  all the same, it is whisper `large-v3-turbo` through sherpa-onnx
  (989 MiB, 6.5%).
- **A recording longer than 25 seconds is cut at pauses** found by
  Silero's model. With the cutter the trial ended with, which keeps every
  sample, Parakeet v2 made 6.1% mistakes on a five-minute recording, read
  in 33 s, with no words lost at the joins. whisper `small.en` made 6.6%
  reading the same recording whole and took 93 s. The trial's first cutter
  dropped sound between pieces and lost words; step 9 must not do that.
- **The phone was not run.** Whether a phone can give the default model
  1.1 GiB, and how long it takes there, is what decides between the
  default and the smaller model.

#### How it was measured

**The test set** is 227 recordings, 4,598 words, 29 minutes, in five
groups. None of it was recorded for the trial and no microphone was used.
Each group was picked by a fixed rule (`scripts/make-set2.py`), never by
ear.

| Group | Recordings | Words | What it is | Voice |
|---|---|---|---|---|
| Clean | 60 | 1,447 | LibriSpeech test-clean (CC BY 4.0): audiobooks read clearly | Human |
| Other | 60 | 1,353 | LibriSpeech test-other (CC BY 4.0): the same kind of reading, by voices and in recordings that recognisers find harder | Human |
| Meetings | 31 | 570 | The AMI Meeting Corpus's test split, headset microphones (CC BY 4.0): unscripted talk in meetings, many speakers not native | Human |
| Calls | 40 | 804 | Earnings-22's test split (CC BY-SA 4.0): company results calls, telephone sound, speakers from many countries, unscripted answers | Human |
| Thoughts | 36 | 424 | Thoughts of the kind Dirt is for ("Tell Maya the demo moved to Thursday at half past two"), spoken by Windows' two English voices | Synthetic |

Meetings and calls are the nearest thing to a thought said into a phone
that could be had freely: unscripted, with false starts, accents and poor
sound. Common Voice was not used: its sentences are read, not said, and
the dataset comes as a whole. Every recording is 3 to 15 seconds long. A
synthetic voice flatters a recogniser, so the thoughts group says little
beyond whether a model writes such sentences sensibly. Some models were
trained on LibriSpeech's training set (other readers, other books), which
flatters them on the first two groups.

**Mistakes** are the word error rate: words wrong, missing or added, out
of the reference's words. Both texts first go through the normaliser
OpenAI published with Whisper, which the Open ASR Leaderboard also uses.
It makes everything lower case, drops punctuation, writes numbers as
digits ("three" and "3", "twenty dollars" and "$20"), makes contractions
and British spellings one, and drops "um" and "uh". Then words that
differ only in a space are made the same ("off site" and "offsite", "Q 3"
and "Q3"). So "Q3" against "Q three" does not decide a ranking. Whether a
difference is more than chance is asked by drawing the recordings again
with replacement, group by group, 10,000 times.

**Reading.** Each model read each recording once. sherpa-onnx (version
2.0.1, greedy decoding, 8-bit models unless marked 32-bit) is handed a
recording of up to 25 seconds whole, and every recording in the set is
shorter. whisper.cpp (`whisper-rs` 0.16, beam search of 5) is handed any
recording whole and steps through it 30 seconds at a time itself. Every
model ran with 4 threads at high priority.

**Time and memory** are for three clips of LibriSpeech test-clean, 5, 15
and 60 seconds long, each read three times; the figure is the median. The
time runs from handing over the sound to having the text, after the model
was loaded; loading is given apart. Memory is the process's peak working
set.

**The machine** is the laptop of "Trial 1 result" (Intel Core i5-13500H,
12 cores, 16 threads, Windows 11).

- **The first timings were disturbed.** Two copies of the measuring
  program read beside each other (a copy that was timing made the other
  wait), and an emulator for another task ran. From 15:43 to 16:30, 94% of
  processor time was in use on average, and the processor ran at 72% of
  its nominal clock.
- **Calibration showed by how much.** The program times a fixed piece of
  arithmetic before each model. It took 0.05 to 0.18 s, so some first
  timings were three times slower than others.
- **The disturbed ones were timed again.** The front-runners, and every
  model first timed at a calibration above 0.1 s, were timed a second time
  one at a time between 20:52 and 21:55, with the laptop otherwise quiet:
  44% of processor time in use, the trial's own 4 threads included, and a
  calibration of 0.05 to 0.07 s. The tables give the second timing where
  there is one.

**A fixed third for the slowest models.** whisper.cpp spends as much on a
5 second recording as on a 30 second one. Its `medium.en` and
`large-v3-turbo` would have taken many hours over the whole set on the
busy laptop, so they read the first of every three recordings of each
group (`scripts/third.py`, 77 recordings). `large-v3-turbo` was cut off at
71 of the 77 when the laptop was taken offline, and the table of the third
compares every model on those 71.

The 5-bit forms of the large whisper.cpp models, and `distil-large-v3` in
whisper.cpp, were timed but did not read the set:

- The 5-bit `small.en` and `base.en` were within chance of their full
  forms, so the 5-bit large ones would add little.
- `distil-large-v3` read the whole set through sherpa-onnx.

#### Measured on this laptop: mistakes on the whole set

227 recordings, 4,598 words, best first. "cpp" is whisper.cpp; every other
model ran in sherpa-onnx.

| Model | On disk, MiB | All | Clean | Other | Meetings | Calls | Thoughts | Capitals and punctuation |
|---|---|---|---|---|---|---|---|---|
| Parakeet unified en 0.6b | 632 | 4.5% | 0.9% | 2.7% | 7.5% | 13.6% | 1.7% | Yes |
| **Parakeet TDT 0.6b v2** | 631 | **4.8%** | 1.2% | 2.7% | 10.5% | 13.2% | 0.7% | Yes |
| Parakeet Ultra (from v3) | 640 | 5.2% | 1.5% | 3.7% | 8.9% | 14.2% | 1.2% | Yes |
| Parakeet TDT 0.6b v3 | 640 | 5.9% | 1.8% | 3.8% | 11.6% | 15.4% | 0.9% | Yes |
| Zipformer GigaSpeech | 70 | 6.2% | 1.8% | 5.2% | 12.1% | 13.2% | 2.8% | No |
| Parakeet Redux (from v3) | 640 | 6.2% | 1.9% | 5.4% | 8.6% | 15.3% | 3.3% | Yes |
| **Parakeet TDT-CTC 110m** | 130 | **6.4%** | 1.8% | 4.5% | 12.6% | 15.9% | 1.2% | Yes |
| whisper `large-v3-turbo` | 989 | 6.5% | 1.2% | 3.8% | 14.4% | 18.4% | 0.2% | Yes |
| whisper `small.en`, cpp | 465 | 7.1% | 2.3% | 6.4% | 15.3% | 14.4% | 0.9% | Yes |
| whisper `small.en`, cpp, 5-bit | 181 | 7.3% | 2.5% | 6.4% | 14.9% | 15.0% | 1.7% | Yes |
| whisper `distil-small.en`, cpp | 321 | 7.5% | 2.6% | 5.8% | 14.2% | 16.7% | 3.1% | Yes |
| whisper `small.en` | 358 | 7.6% | 2.3% | 6.1% | 17.2% | 15.7% | 2.4% | Yes |
| whisper `distil-small.en` | 285 | 7.8% | 2.8% | 6.6% | 15.4% | 15.4% | 3.8% | Yes |
| whisper `small.en`, 32-bit | 925 | 7.9% | 2.4% | 6.4% | 17.4% | 16.3% | 2.1% | Yes |
| Canary 180m flash | 198 | 8.1% | 1.5% | 3.9% | 11.1% | 27.7% | 2.4% | Yes |
| whisper `distil-large-v3` | 938 | 8.3% | 1.9% | 6.5% | 17.2% | 20.1% | 1.2% | Yes |
| whisper `base.en`, cpp | 141 | 8.4% | 2.5% | 8.1% | 15.4% | 18.2% | 0.9% | Yes |
| Moonshine base | 135 | 8.5% | 2.3% | 8.4% | 11.6% | 20.3% | 3.1% | Yes |
| whisper `base.en`, cpp, 5-bit | 57 | 8.5% | 2.1% | 8.4% | 15.6% | 18.9% | 1.2% | Yes |
| whisper `base.en`, 32-bit | 279 | 9.6% | 2.8% | 9.1% | 18.2% | 21.1% | 1.4% | Yes |
| whisper `base.en` | 153 | 10.0% | 2.9% | 8.9% | 20.7% | 20.9% | 2.8% | Yes |
| Moonshine tiny | 42 | 11.6% | 3.0% | 11.7% | 16.1% | 28.0% | 3.8% | Yes |

"Capitals and punctuation" is "Yes" where most of what the model wrote
has both. GigaSpeech's zipformer writes in capitals without punctuation.

**Whether a difference is more than chance.** B's mistakes minus A's, in
points, with the range 95 draws in 100 fell in:

| A | B | B minus A | Range | More than chance? |
|---|---|---|---|---|
| Parakeet unified | Parakeet v2 | +0.33 | −0.39 to +1.08 | No |
| Parakeet v2 | Parakeet Ultra | +0.39 | −0.55 to +1.30 | No |
| Parakeet v2 | Parakeet v3 | +1.07 | +0.31 to +1.85 | Yes |
| Parakeet v2 | Zipformer GigaSpeech | +1.30 | +0.47 to +2.13 | Yes |
| Parakeet v2 | Parakeet 110m | +1.50 | +0.65 to +2.31 | Yes |
| Parakeet v2 | whisper `large-v3-turbo` | +1.65 | +0.55 to +2.93 | Yes |
| Parakeet v2 | whisper `distil-large-v3` | +3.41 | +2.14 to +4.82 | Yes |
| Parakeet 110m | Zipformer GigaSpeech | −0.20 | −0.97 to +0.55 | No |
| Parakeet 110m | whisper `large-v3-turbo` | +0.15 | −0.80 to +1.35 | No |
| Parakeet 110m | whisper `small.en`, cpp | +0.76 | +0.07 to +1.48 | Yes |
| whisper `small.en`, cpp | whisper `small.en`, cpp, 5-bit | +0.17 | −0.34 to +0.85 | No |
| whisper `base.en`, cpp | whisper `base.en`, cpp, 5-bit | +0.13 | −0.32 to +0.62 | No |

What the mistakes were:

- **Canary 180m flash ran away once.** On one call it wrote "THE" over and
  over: 118 words for a recording of 24, and 114 of its 223 mistakes on
  calls. Without that one recording its calls figure would be about 14%.
  No whisper model ran away like that in this set; `distil-large-v3` once
  wrote twice what was said. Parakeet and the zipformer are transducers,
  which cannot repeat themselves that way.
- **The transducers instead sometimes wrote nothing.** Parakeet v2 wrote
  nothing for one meeting recording ("it's just uh they'd just be if
  statements..."), and v3 for that one and two calls full of false starts.
  whisper's models wrote one to three words for two calls each. An empty
  result has to be shown as "no words found", not as a note with nothing
  in it.

#### Measured on this laptop: mistakes on the fixed third

The 71 recordings of the third that every model read, 1,485 words: the
large whisper.cpp models beside the front-runners.

| Model | On disk, MiB | All | Clean | Other | Meetings | Calls | Thoughts |
|---|---|---|---|---|---|---|---|
| Parakeet unified en 0.6b | 632 | 4.1% | 0.4% | 1.7% | 7.3% | 11.3% | 4.1% |
| **Parakeet TDT 0.6b v2** | 631 | **4.2%** | 0.7% | 2.8% | 7.3% | 11.0% | 0.0% |
| whisper `large-v3-turbo`, cpp | 1,549 | 5.8% | 0.2% | 3.2% | 16.0% | 12.4% | 0.0% |
| whisper `large-v3-turbo` | 989 | 6.0% | 0.4% | 3.5% | 13.2% | 14.8% | 0.0% |
| Parakeet TDT-CTC 110m | 130 | 6.6% | 1.1% | 5.0% | 12.8% | 14.5% | 1.4% |
| whisper `medium.en`, cpp | 1,463 | 6.9% | 1.1% | 4.3% | 14.2% | 15.9% | 1.4% |
| whisper `small.en`, cpp | 465 | 7.3% | 1.1% | 6.0% | 16.4% | 13.8% | 0.0% |
| whisper `distil-large-v3` | 938 | 9.3% | 1.6% | 9.3% | 14.2% | 20.1% | 0.0% |

Parakeet v2 was ahead of whisper.cpp's `large-v3-turbo` by 1.55 points
(+0.13 to +3.09) and of its `medium.en` by 2.63 (+1.28 to +4.07): both
more than chance, even on a third of the set. whisper.cpp's `medium.en`
was not told apart from its `small.en`.

#### Measured on this laptop: time and memory

Median of three, 4 threads. "Again" marks a model timed a second time, one
at a time, with the laptop quiet. The others were timed once, at the
calibration shown, where 0.05 to 0.07 s is a quiet laptop.

| Model | 5 s | 15 s | 60 s | Loading | Peak memory, MiB | Calibration |
|---|---|---|---|---|---|---|
| **Parakeet TDT 0.6b v2** | 0.49 s | 1.38 s | 6.14 s | 2.6 s | 1,156 | Again, 0.073 |
| Parakeet unified en 0.6b | 0.44 s | 1.25 s | 5.30 s | 3.5 s | 1,160 | Again, 0.069 |
| Parakeet TDT 0.6b v3 | 0.37 s | 1.00 s | 4.13 s | 2.2 s | 1,168 | Again, 0.058 |
| Parakeet Ultra | 0.35 s | 0.87 s | 3.76 s | 3.2 s | 1,167 | 0.054 |
| Parakeet Redux | 0.41 s | 1.03 s | 4.16 s | 2.9 s | 1,168 | 0.059 |
| **Parakeet TDT-CTC 110m** | 0.14 s | 0.35 s | 1.79 s | 1.8 s | 604 | 0.055 |
| Zipformer GigaSpeech | 0.07 s | 0.20 s | 1.15 s | 2.2 s | 558 | Again, 0.055 |
| Canary 180m flash | 0.65 s | 2.51 s | 9.96 s | 2.3 s | 717 | 0.067 |
| Moonshine base | 0.15 s | 0.58 s | 2.56 s | 0.8 s | 356 | Again, 0.052 |
| Moonshine tiny | 0.08 s | 0.37 s | 1.65 s | 0.5 s | 222 | Again, 0.053 |
| whisper `base.en` | 0.41 s | 1.38 s | 5.68 s | 1.0 s | 737 | 0.054 |
| whisper `small.en` | 1.99 s | 6.68 s | 24.72 s | 3.2 s | 1,252 | 0.052 |
| whisper `distil-small.en` | 0.58 s | 1.78 s | 7.69 s | 0.7 s | 945 | Again, 0.053 |
| whisper `large-v3-turbo` | 1.76 s | 4.56 s | 19.03 s | 2.0 s | 2,096 | Again, 0.052 |
| whisper `distil-large-v3` | 3.33 s | 7.33 s | 21.90 s | 4.3 s | 1,803 | 0.075 |
| whisper `base.en`, cpp | 1.65 s | 1.84 s | 6.09 s | 0.2 s | 452 | 0.065 |
| whisper `base.en`, cpp, 5-bit | 1.72 s | 1.99 s | 6.31 s | 0.1 s | 368 | 0.074 |
| whisper `small.en`, cpp | 6.37 s | 8.68 s | 22.37 s | 0.8 s | 964 | Again, 0.069 |
| whisper `small.en`, cpp, 5-bit | 4.63 s | 5.78 s | 19.95 s | 0.2 s | 726 | Again, 0.054 |
| whisper `distil-small.en`, cpp | 4.11 s | 4.66 s | 19.88 s | 0.4 s | 698 | 0.074 |
| whisper `medium.en`, cpp | 20.66 s | 24.74 s | 86.62 s | 2.4 s | 2,323 | 0.100 |
| whisper `medium.en`, cpp, 5-bit | 17.81 s | 21.30 s | 59.66 s | 0.7 s | 1,383 | Again, 0.065 |
| whisper `large-v3-turbo`, cpp | 27.13 s | 31.10 s | 74.45 s | 2.8 s | 2,108 | 0.081 |
| whisper `large-v3-turbo`, cpp, 5-bit | 23.58 s | 24.23 s | 75.89 s | 0.5 s | 1,121 | Again, 0.052 |
| whisper `distil-large-v3`, cpp | 23.91 s | 25.04 s | 77.77 s | 1.4 s | 1,977 | Again, 0.053 |

- **whisper.cpp spends a 30 second window on any recording**, so 5
  seconds of speech took nearly as long as 15. sherpa-onnx's whisper does
  not. It read the 5 second clip 3 to 15 times as fast as whisper.cpp did
  with the same model. On the 60 second clip it was about as fast for
  `base.en` and `small.en`, and four times as fast for `large-v3-turbo`. Most thoughts
  last a few seconds, so this is most of the time a person waits.
- **Parakeet v2 holds about 1.1 GiB** however short the recording, because
  its 8-bit encoder alone is 622 MiB. Parakeet 110m holds 0.6 GiB, the
  zipformer 0.55 GiB, and Moonshine base 0.35 GiB.
- **The sherpa-onnx models timed again were 1.4 to 2.6 times faster**
  than at first. whisper.cpp's times hardly changed (19.95 against 21.01 s
  for 60 s with the 5-bit `small.en`; 22.37 against 20.20 s with
  `small.en`), though the laptop was quieter. Why is not known.

#### Measured on this laptop: a long recording

The design lets listening run to ten minutes. The trial used five, to save
time: what is tested is what happens at the joins once a recording is
longer than a model takes at once, and five minutes already has dozens of
them. The design's ten-minute limit is unchanged, and nothing here
questions it. Time is also given per minute of sound, so that ten minutes
can be worked out.

The recording is 36 utterances of LibriSpeech test-clean, one after
another, from several readers, with 0.4 s of quiet between them: 287
seconds, 832 words after normalising (`scripts/shorten-long.py` cuts the
ten-minute recording at the end of the last utterance before 5:00). Each
reading ran three times; the time is the median.

**How each model handles a long recording.** Parakeet and the other
sherpa-onnx models' memory grows with what they are handed: Parakeet 110m
handed the 60 second clip whole held 1,055 MiB, against 604 MiB with it cut
in pieces. So `voice-trial read` cuts a
recording longer than 25 seconds into pieces with Silero's model, reads
them one after another, and joins the text. whisper.cpp can take the whole
recording and step through it 30 seconds at a time itself; it was also
run on the same pieces, to compare.

| Model | Read as | Pieces | Time | Per minute of sound | Ten minutes at that rate | Peak memory, MiB | Mistakes | Mistakes within two words of a join, and elsewhere | Three or more words in a row lost or added |
|---|---|---|---|---|---|---|---|---|---|
| **Parakeet TDT 0.6b v2** | Pieces | 15 | 32.6 s | 6.8 s | About 68 s | 1,186 | 6.1% | 8.6%; 5.9% | None |
| Parakeet TDT-CTC 110m | Pieces | 15 | 9.9 s | 2.1 s | About 21 s | 632 | 7.6% | 14.3%; 7.0% | Once: "I couldn't" |
| whisper `small.en`, cpp | Whole | 1 | 92.9 s | 19.4 s | About 194 s | 1,036 | 6.6% | No joins | None |
| whisper `small.en`, cpp | Pieces | 15 | 113.1 s | 23.6 s | About 236 s | 959 | 7.5% | 10.0%; 7.2% | None |

- **The mistakes are higher than on the clean group** (1.2% for Parakeet
  v2) because the chapter is full of invented names and dialect
  ("Boolooroo", "Ghip-Ghisizzle", "Cap'n Bill", "consid'ble"). Compare the
  rows with each other, not with the tables above.
- **The first cutter lost words.** It kept only what Silero heard as speech
  and 0.2 s either side, threw away the sound between pieces, and cut any
  stretch longer than the window into equal parts, through words. With it,
  Parakeet v2 made 7.2% mistakes, a quarter of the words near a join were
  wrong (24.7%), and three runs of words were lost at joins ("and
  they're", "I couldn't", "don't worry"). Parakeet 110m made 8.9%, and whisper
  in pieces 9.6%.
- **The cutter the trial ended with** cuts in the middle of the latest
  pause that keeps a piece within 25 seconds and throws nothing away.
  Where Silero hears no pause in the second half of the window, it cuts at
  the quietest tenth of a second there (`trials/voice/src/finished.rs`).
  Recordings shorter than 25 seconds were never cut by either, so the
  figures for the test set are not touched by this.
- **Memory does not grow with the length** for a model read in pieces: it
  is that of the longest piece. Reading ten minutes would hold what five
  did. whisper.cpp reading the whole recording held 7% more than for a
  minute.

#### Building

Nothing changes from "Trial 1 result" for the engine. sherpa-onnx's
offline recogniser (`SherpaOnnxCreateOfflineRecognizer`) and Silero
(`SherpaOnnxCreateVoiceActivityDetector`) are in the same
`libsherpa-onnx-c-api` as the streaming recogniser. The program that reads
a finished recording links the same two libraries on Windows and Android.

The program was cross-compiled here for both Android targets with NDK 26.3
for API level 24:

- **arm64:** with whisper.cpp and sherpa-onnx, in 74 s, including the 43
  MiB download of sherpa-onnx's Android archive.
- **x86_64:** with sherpa-onnx alone, in 29 s, with the archive already
  there.

Every library in that archive is aligned for 16 KB pages (its LOAD
segments are `0x4000`) for both ABIs.

To pin the prebuilt libraries by hash: the crate's build script reads the
archive from `SHERPA_ONNX_ARCHIVE_DIR` instead of downloading it. The real
build should fetch the archive itself, check it against a SHA-256 kept in
the repository, and point the variable at it. For 2.0.1 the archives used
here were:

| Archive | Size | SHA-256 |
|---|---|---|
| `sherpa-onnx-v2.0.1-android.tar.bz2` | 43.2 MiB | `39003753ec293425ed56723ea9b972b98841a490af3311670c3060548d654330` |
| `sherpa-onnx-v2.0.1-win-x64-shared-MT-Release-lib.tar.bz2` | 7.5 MiB | `afa4de3ca4ab0afd53bb4aab40d28ee15608364b549ffc0fe0caf6489bfc39a5` |

The model is not part of the build. The app fetches it once, checks it
against its own SHA-256, and keeps it in Dirt's storage, as step 9 says.

#### Models

| Model | Through | On disk | Published at | Licence |
|---|---|---|---|---|
| Parakeet TDT 0.6b v2, 8-bit | sherpa-onnx | 631 MiB | sherpa-onnx's GitHub release `asr-models`, from huggingface.co/nvidia/parakeet-tdt-0.6b-v2 | CC BY 4.0, by the package's README and NVIDIA's model card |
| Parakeet TDT-CTC 110m, 8-bit | sherpa-onnx | 130 MiB | The same release, from huggingface.co/nvidia/parakeet-tdt_ctc-110m | CC BY 4.0, by NVIDIA's model card; the package has no licence file |
| Parakeet unified en 0.6b, 8-bit | sherpa-onnx | 632 MiB | The same release, from huggingface.co/nvidia/parakeet-unified-en-0.6b | NVIDIA Open Model License, by NVIDIA's model card; the package has no licence file |
| Parakeet TDT 0.6b v3, 8-bit (25 languages) | sherpa-onnx | 640 MiB | The same release, from huggingface.co/nvidia/parakeet-tdt-0.6b-v3 | CC BY 4.0 |
| Parakeet Ultra and Redux, 8-bit (25 languages) | sherpa-onnx | 640 MiB each | The same release, from huggingface.co/moondream; both are post-trainings of v3 | CC BY 4.0, by the package's README |
| Canary 180m flash, 8-bit (English, Spanish, German, French) | sherpa-onnx | 198 MiB | The same release, from huggingface.co/nvidia/canary-180m-flash | CC BY 4.0 |
| Zipformer GigaSpeech (2023-12-12) | sherpa-onnx | 70 MiB | The same release | Apache-2.0 |
| Moonshine tiny and base, 8-bit | sherpa-onnx | 42 and 135 MiB | The same release | MIT, by its LICENSE |
| whisper `base.en`, `small.en`, `distil-small.en`, `large-v3-turbo`, `distil-large-v3`, 8-bit | sherpa-onnx | 153, 358, 285, 989 and 938 MiB | The same release | MIT (OpenAI's weights and distil-whisper's) |
| whisper `base.en`, `small.en`, `medium.en`, `large-v3-turbo`, and their 5-bit forms; `distil-small.en`, `distil-large-v3` | whisper.cpp | 141, 465, 1,463 and 1,549 MiB; 57, 181, 514 and 547 MiB at 5 bits; 321 and 1,449 MiB | huggingface.co/ggerganov/whisper.cpp; huggingface.co/distil-whisper | MIT |

CC BY 4.0 asks that Dirt credit NVIDIA and say the model was converted,
and asks nothing else.

#### Recommendation

**Engine: sherpa-onnx, in `dirt-voice`**, as "Trial 1 result" set out. It
reads the finished recording with an offline model in the background.
Silero's model stays, for the 1.6 seconds and to find the pauses at which
a long recording is cut.

| | Default: Parakeet TDT 0.6b v2 | Smaller: Parakeet TDT-CTC 110m | Larger: whisper `large-v3-turbo` |
|---|---|---|---|
| To fetch | 631 MiB | 130 MiB | 989 MiB (8-bit, sherpa-onnx) |
| Licence | CC BY 4.0 | CC BY 4.0 | MIT |
| Mistakes on the whole set | 4.8% | 6.4% | 6.5% |
| Mistakes on meetings, and on calls | 10.5%, 13.2% | 12.6%, 15.9% | 14.4%, 18.4% |
| Capitals and punctuation | Yes | Yes | Yes |
| 5, 15 and 60 s on this laptop | 0.49, 1.38, 6.14 s | 0.14, 0.35, 1.79 s | 1.76, 4.56, 19.03 s |
| Peak memory | 1.1 GiB | 0.6 GiB | 2.0 GiB |
| Five minutes, in pieces | 33 s, 6.1% mistakes | 10 s, 7.6% | Not measured |
| What on a phone would rule it out | Android not letting the app hold 1.1 GiB while it works in the background, or a thought taking much longer to read than it took to say | Little: it is a fifth of the size and three times as fast | Twice the default's memory, for more mistakes |

Put Parakeet v2 to the owner. It made the fewest mistakes of any model
whose licence asks only for credit, on every group of human speech, and
it reads a thought in a tenth of its own length on this laptop. Its cost
is the download and the memory: 631 MiB is many times the app, and
1.1 GiB is a lot for a phone to give an app that is not on screen.

**The owner chose Parakeet TDT-CTC 110m** (2026-11-17; see "Decided"), for its size, speed and memory. Step 9 builds with it. A phone run is still worth doing before step 9, for the time a thought takes to read there.

If a phone run shows that it cannot have that, or the owner would rather
not ask a person to fetch 631 MiB, Parakeet 110m is the default instead,
with 1.5 points more mistakes at a fifth of the size. whisper
`large-v3-turbo` would mean more memory and a larger download in exchange
for more mistakes. It is listed only because a larger model was asked for.

**For whoever builds step 9:**

- **Cut a recording longer than 25 seconds at pauses, and keep every
  sample.** Cut in the middle of a pause, never throw away the sound
  between pieces, and where there is no pause cut at the quietest moment.
  The trial's first cutter did otherwise and lost words.
- **Read the pieces one at a time**, so that memory stays that of one
  piece.
- **Load the model only to read, and let it go afterwards.** It is 1.1 GiB
  held for seconds, not for as long as the app is open.
- **Show an empty result as "no words found".** Parakeet now and then
  writes nothing for unclear speech.
- **Credit NVIDIA.** A credit in Settings meets the licence.

#### Not measured

- **Any phone.** Time, memory, loading from a phone's storage, heat, and
  whether Android lets an app hold 1.1 GiB while it makes words in the
  background: not measured.
- **whisper `distil-large-v3.5` and `medium.en` through sherpa-onnx.**
  Skipped to save time. whisper.cpp's `medium.en` had already made 6.9%
  mistakes on the third, and `large-v3-turbo` 6.5% on the whole set, both
  behind Parakeet v2's 4.8%. Neither of the two would likely change the
  recommendation. To run them if it is ever questioned:
  `bash scripts/run2.sh sherpa-whisper-distil-large-v3.5
  sherpa-whisper-medium.en`.
- **The large whisper.cpp models on the whole set**, their 5-bit forms on
  any of it, and the last 6 recordings of the third for whisper.cpp's
  `large-v3-turbo`: see "A fixed third" above.
- **Ten minutes.** Five were measured; see "a long recording".
- **Real microphones, rooms, and accents beyond those in the meetings and
  calls.**
- **macOS and Linux**: as in "Trial 1 result", from documentation only.

#### The phone's half: the commands

Not run. It needs the owner's word, a phone with USB debugging on, and no
other device connected. `trials/voice/scripts/phone.sh read NAME...` runs
the same comparison on a phone, one model at a time. It pushes the
program, its libraries and the test sets to `/data/local/tmp/voice-trial`.
Then, for each model, it fetches the model here, pushes it, times the
three clips three times, reads the whole set once and the long recording
three times, brings the results back, and removes the model. At the end it
removes the folder. From `trials/voice`, with the test sets made as its
README says:

```bash
bash scripts/build-android.sh aarch64-linux-android "whisper sherpa"
adb devices                                   # one phone, and the right one
bash scripts/phone.sh read parakeet-v2 parakeet-110m zipformer-gigaspeech
RESULTS=data/phone-results2 data/venv/Scripts/python scripts/analyse2.py
```

For one model by hand it comes to:

```bash
export MSYS_NO_PATHCONV=1                      # Git Bash must leave /data/... alone
there=/data/local/tmp/voice-trial
libs=target/sherpa-onnx-prebuilt/jniLibs/arm64-v8a
model=sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8
MODE=fetch KEEP=1 bash scripts/run2.sh parakeet-v2      # fetches it to data/models2
adb shell "mkdir -p $there/models $there/timing"
adb push target/aarch64-linux-android/release/voice-trial $there/
adb push $libs/libsherpa-onnx-c-api.so $libs/libonnxruntime.so $there/
adb push data/timing/manifest.tsv data/timing/*.wav $there/timing/
adb push data/models2/$model data/models2/silero_vad.onnx $there/models/
adb shell "chmod 755 $there/voice-trial"
adb shell "cd $there && LD_LIBRARY_PATH=. ./voice-trial read --engine sherpa \
  --kind nemo-transducer --model models/$model \
  --vad models/silero_vad.onnx --window 25 --threads 4 \
  --set timing/manifest.tsv --repeat 3 --out parakeet-v2.timing.jsonl"
mkdir -p data/phone-results2
adb pull $there/parakeet-v2.timing.jsonl data/phone-results2/
adb shell "rm -r $there"
```

Write down the phone's model, its Android version, whether it was on a
charger, and whether it was warm.

### Trial 2: capture on Android without the app

The capture sheet is a native screen that opens without the web view
(design 8.5). The owner wants what it captures to reach other devices
without the app being opened.

Find out:

- How long from a tap on the widget to a field that can be typed in, for a
  native screen with no web view.
- Whether the Rust library can be loaded and used where the app's own
  screen never started: open the store, read the device's secrets from
  Android's encrypted preferences, make a note, sync once.
- What starts that work and lets it finish after the sheet has closed.
- That the sheet writing while the app is open loses nothing. The store is
  already safe for two processes on a desktop.
- How more Kotlin files and a folder of resources get into the Gradle
  project `dx` generates, in a way that survives a clean build.
- On the emulator, whether the sheet and a Quick Settings tile open over
  the lock screen, and that nothing of a note can be seen from there.

**It decides:** whether design 8.5's background work is built or its
fallback, and how Android's native code is laid out for steps 8 and 9.

### Trial 2 result

Run on 2026-11-17 on the Android 16 emulator (x86_64, no window) on a
Windows laptop, with a debug build. The code is on the branch
`trial/android-capture`, with the scripts and timings in
`trials/android-capture/`; it is not merged. The server was the real one
run on the same laptop (`cargo run -p dirt-sim --example serve`), and the
CLI, with a data folder of its own, was the second device. No phone was
touched: **every figure and every lock-screen answer below is the
emulator's.**

**What it found, in short**

- **Design 8.5's background work can be built, and was.** The sheet keeps
  the thought in a file, under a note ID made at that moment, and asks
  Android's JobScheduler for one expedited job that needs a network. The
  job loads the app's Rust library through JNI, in a process where the web
  view's activity never started, makes the note, and syncs once. A thought
  reached the CLI as a synced note about 2.6 s after ✓, with the app never
  opened.
- **The fallback is kept underneath it, not instead of it.** The app takes
  in whatever still waits each time it starts and each time the person
  returns to it. A thought is never lost between ✓ and the note: it is on
  the disk before the sheet closes.
- **Native code goes in one Gradle library module**,
  `crates/dirt-mobile/android/native`, which `dx` 0.7.9 copies into the
  project it generates because one Rust declaration names it. It can carry
  any number of Kotlin files, resources and Gradle dependencies of its own.
- **Over the lock screen** the sheet and the Quick Settings tile open
  without unlocking, typing works, the app itself cannot be reached, and
  the store and the secrets can be used. So the thought need not wait for
  the phone to be unlocked: it becomes a note and syncs at once. Before the
  first unlock after a restart the tile is unavailable.
- **The sheet is quicker to type into than the app, but not by as much as
  hoped on this emulator,** and the keyboard is most of what is left. Real
  phones are needed for figures that mean anything.

#### Measured: how fast the sheet can be typed into

Every figure is a median, with the least and the most, from the emulator's
own log. Each run was started with `adb shell am start -W`; "cold" means
the app's process had been stopped (`am force-stop`), "warm" that it was
running and the person had gone Home. The figures mean:

- **Shown**: `am start -W`'s `TotalTime`, Android's own figure (the same
  as logcat's "Displayed" line): from the system taking the start to the
  window's first frame being drawn. For a translucent window it includes
  the opening animation.
- **Field**: from the system's `START` line to the sheet's first frame
  with the cursor in the field. A hardware keyboard could type from here.
- **Keyboard**: from the same line to the window being told the on-screen
  keyboard is showing. This is when a person can type.
- **App's field**: from the same line to the app's page saying its
  editor's text box exists. The app was opened with the widget's
  quick-capture intent, which opens the editor. Its keyboard was not
  measured, and comes after.

Two sessions. The first followed a fresh start of the emulator, on a
quiet laptop. In the second the laptop was busy at about half its CPU
with another task; that session has the app as well as the sheet, so
compare within it.

| | Session | Runs | Shown | Field | Keyboard | App's field |
|---|---|---|---|---|---|---|
| Sheet, cold | 1 | 6 | 964 (925–1078) ms | 633 (590–683) ms | 1532 (1347–2574) ms | |
| Sheet, warm | 1 | 6 | 968 (678–1315) ms | 261 (89–542) ms | 2326 (2159–2674) ms | |
| Sheet, cold | 2 | 6 | 2670 (2394–3321) ms | 1788 (1315–2458) ms | 3628 (3444–5033) ms | |
| Sheet, warm | 2 | 6 | 1260 (1156–1288) ms | 216 (153–350) ms | 2287 (2161–2637) ms | |
| App, cold | 2 | 5 | 3388 (2955–4389) ms | | | 5731 (5064–7303) ms |
| App, warm | 2 | 5 | 375 (264–1155) ms | | | |

- **Cold, the sheet's keyboard was up 2.1 s before the app even had its
  text box** (session 2), and the app's keyboard comes after that.
- **Warm, the app is quicker to show than the sheet**: Android brings back
  the screen it had (`HOT`) in about 0.4 s. But that screen is whatever was
  left open, not an empty editor, and the sheet's field is there in 0.2 s.
- **The keyboard is the slow part** of the sheet: 1.3 to 2.1 s after its
  field, warm or cold. The emulator's keyboard is slow to show; over the
  lock screen it took 4.1 to 8.3 s against 1.4 to 2.5 s unlocked. A phone's
  should be much quicker, and nothing in Dirt can make it quicker.
- **Compiling the app ahead of time did not help.** After
  `cmd package compile -m speed` the sheet's cold field was 1762 ms and its
  keyboard 4528 ms. The cold start is Android making the process, not
  Kotlin being interpreted. A release build was not made (no release builds
  on this machine) and is still to be measured: its Java code is smaller,
  and its library much smaller.
- **A process of its own for the sheet** (`android:process=":capture"`)
  was timed only while the laptop was at full load, which made every figure
  ten times the others, so it is not compared here. It worked.
- **From a tap on the widget or the tile** the path is the same as
  `am start` once Android has the tap; the finger itself was not timed.

#### Measured: the core without the web view

| | Figure |
|---|---|
| The app's library, `libmain.so`, debug build, x86_64 | 128 MB (128,244,040 bytes), already without symbols (stripping changed nothing); 30 MB compressed. A release build was not made |
| Loading it from Kotlin and the first call, in a new process | 138, 175, 193, 304, 413 and 477 ms; 639 ms over the lock screen with the laptop busy |
| The same where the process already had it | 0 to 8 ms |
| Opening the secrets in Android's encrypted preferences | 811 ms the first time ever (the key is made); 286 to 1131 ms after; 1.5 to 3.0 s in a new process on the busy laptop |
| Making the note from the waiting file | 79 to 303 ms |
| One sync with the local server | 0.5 to 1.4 s; 6.3 s to send the 37 notes of the race below |
| The whole job, from its start to its end | 1.7 to 4.1 s |
| From asking for the expedited job to its start | 341 to 652 ms; 4.8 s once, with the emulator busy |
| From ✓ to the note listed by the CLI after its own sync | About 2.6 s |

#### The answers

**1. Speed.** As measured above. The sheet is a plain Activity with views
made in code, a translucent theme of the module's own, `singleInstance`
in a task of its own (`taskAffinity=""`, so the app's screen never comes
forward behind it) and kept out of the recent apps. It does not load the
Rust library at all: nothing of it is waited for.

**2. Getting native code into the build.** `dx` 0.7.9 bundles a Gradle
module when the program declares one with Manganis's foreign-function
macro:

```rust
#[cfg(target_os = "android")]
mod native_module {
    #[manganis::ffi("/android/native")]
    extern "Kotlin" {
        pub type DirtNative;
    }
}
```

The macro leaves a record in the built library. After linking, `dx` reads
it, copies the folder into the generated project as `plugins/dirtnative`,
adds it to `settings.gradle`, adds
`implementation(project(":plugins:dirtnative"))` to the app, and strips
any version from the module's plugins, since the generated root project
puts the Android Gradle plugin and Kotlin on the class path. The module is
an ordinary Android library: Kotlin under `src/main/kotlin`, resources
under `src/main/res` (layouts, drawables, strings, `values-night`, `xml/`
for the widget's description and the shortcuts), and a manifest that
Gradle merges into the app's. `manganis` 0.7 (the version Dioxus brings) is
the one new dependency of `dirt-mobile`.

- **It survives a clean build**: `dx` copies the folder afresh on every
  build, so nothing of it lives only in the generated project. The
  module's classes and resources and its merged manifest were all in the
  APK.
- **A `dx` upgrade**: as well as can be judged. It depends on `dx` reading
  Manganis's records and on the `plugins/` layout, both in `dx` 0.7.9's
  own source (`packages/cli/src/build/android.rs`); it is how Dioxus's own
  mobile plugins are bundled, which makes it less likely to go than the
  `WRY_ANDROID_KOTLIN_FILES_OUT_DIR` folder `build.rs` writes into today.
  Not yet known: that the record survives a release build, with its
  link-time optimisation.
- **Gradle dependencies**: the module's own `build.gradle.kts` can name
  them. WorkManager 2.9.1 was added there once: it resolved, built, and its
  classes were in the APK. So both WorkManager and JobScheduler were open;
  JobScheduler was chosen (4).
- **Found:** on Android 16, a job that needs a network may not be asked for
  without the `ACCESS_NETWORK_STATE` permission (`SecurityException` from
  `schedule()`, which ended the app). It is a normal permission, asked of
  nobody; the module's manifest has it.
- An app shortcut's `<meta-data>` must be on the launcher activity, so
  that one line stays in the app's own manifest; the shortcuts' XML is in
  the module.

**3. The core without the web view.** It works, with three things changed
from how the app's Rust code does it today:

- **Loading the library runs nothing of the app.** It has no
  `JNI_OnLoad`; the app starts when `WryActivity` calls its own `create`.
  Kotlin loads `libmain.so` with `System.loadLibrary("main")` and calls an
  exported function. The app opened later in the same process worked, and
  the job then found the library already loaded.
- **`ndk_context` cannot be used**: tao sets it in the activity's `create`
  and clears it in `destroy`, so in the job's process it is empty, or
  belongs to an activity that may go at any moment. The job is handed a
  `Context` by Kotlin and the JavaVM from JNI's own `JNIEnv`, and opens the
  encrypted preferences with those (`EncryptedPrefs::open_with` beside the
  existing `open`). Nothing of the job sets `ndk_context`.
- **The data folder comes from Kotlin.** `config::default_mobile_data_directory`
  works it out from the process's name, which is the package's only in the
  app's main process; in a `:capture` process it would point at a folder
  that does not exist. Kotlin passes `Context.getFilesDir()`.
- **An async runtime per call**: a current-thread tokio runtime, built
  for the call, ran `Session::open` with background sync off and one sync
  cycle, as the CLI does.

**4. What runs the work.** Three were tried, each by killing the process
(`kill -9`) at a chosen point:

| | Killed before the note was made | Killed with the note made, not synced |
|---|---|---|
| A plain thread in the sheet's process | Nothing runs again. The thought waits in its file until the app is opened, which makes it a note under its capture-time ID and syncs | Not tried: it would be the same, and the next sync from anywhere sends the note |
| An expedited JobScheduler job | Android started the process again for the job 24 ms after it died and ran it; the note was made and synced | Run again at once (the process was started 49 ms after it died): it found the note made, made nothing, and synced it |
| WorkManager | Not run. From its documentation it is JobScheduler underneath, with a database of its own and a start-up step in every process | |

**JobScheduler is recommended**: it is the platform's own, needs no
dependency, and does what is needed; WorkManager would add a second queue
for one job. The job is asked for by ✓ and by nothing else: it has no
period, and it is run again only by Android, when the process running it
died. It needs a network, so a thought kept offline waits for one; that is
an event, and the app's own start takes it in too. From documentation, not
tried: expedited jobs have a quota for each app, and beyond it run as
ordinary jobs, which Android may put off; a phone's own battery saving may
put them off too. The note is in the store by then, and the next sync
from anywhere sends it. The case offline was not tried.

**5. Two at once.** Nothing was lost in any of three:

- The sheet over the open app, in the app's process. The sheet closed, the
  app took the thought in and showed it, and the job, starting after, had
  nothing to make and synced.
- The same with the sheet and its job in a `:capture` process of their
  own, the app open in its own. Two processes, the same store; the same
  outcome.
- A race on purpose: 60 thoughts written into the waiting folder, then the
  app brought forward and the `:capture` job started at the same moment.
  The app made 31 notes and the job 30, one of them the thought that
  started the race; none twice, and all 61 reached the CLI. The job found
  7 notes made by the app before their files were gone, and 12 files gone
  between its listing them and reading them; it counted those as left
  waiting, though none was. Step 8 should tell a file that has gone from
  one that cannot be read.

**Found:** the app's list did not learn of the note by itself. The sheet
is translucent, so the page under it is never hidden, and the app's
"returned" event (the page becoming visible) does not fire when it closes.
`MainActivity.onResume` now tells the page, which takes in what waits and
reads the list again; a note the job made is already in the store, and
reading again is all that is needed.

**The sheet should run in the app's process**: one copy of the library and
of the store's connections, and when the app is already running the job
starts with everything loaded. The separate process works as well; it
would only help if the sheet's start were slowed by the app's, which it is
not, since the sheet never loads the library.

**6. The lock screen, on the emulator.** With a PIN set
(`locksettings set-pin`) and the emulator locked:

- **The sheet shows over the lock screen** (`setShowWhenLocked`, and the
  manifest's `showWhenLocked`), with the lock screen still there under it,
  and the line "Adds a note. Your notes stay locked." Typing worked.
- **The Quick Settings tile opens it without unlocking**, pulled down over
  the lock screen, with the app's process running and with it stopped. The
  tile uses `startActivityAndCollapse` with a `PendingIntent` (Android 14
  and later), not `unlockAndRun`, which would ask for the PIN first.
- **Nothing of a note can be reached from there.** The sheet shows no
  draft written before the lock; one written over the lock screen was
  added after it in the draft's file, which is what an unlocked sheet
  shows. Starting the app's own activity while locked left the lock screen
  in front, and nothing of the app was shown. The sheet has no way to the
  app.
- **The store and the secrets can be used while locked**, once the phone
  has been unlocked since it started: Android's file encryption keeps an
  app's storage open from the first unlock. The job made the note and
  synced with the emulator locked, from a running process and from a
  stopped one (`deviceLocked=true` in the job's log). The encrypted
  preferences' key is not made with `setUnlockedDeviceRequired`; if Dirt
  ever made it so, the job would fail while locked and the thought would
  wait in its file for the app.
- **Before the first unlock after a restart** the tile showed
  "Unavailable" and pressing it did nothing; no process started. Android
  keeps the app's storage shut then, as design 8.6 says. (An app can be
  made to run before the first unlock with storage of the device's kind,
  which the PIN does not protect. That is not recommended, and nothing was
  built for it.)
- **Android does not tell an app that is not running that the phone was
  unlocked.** A receiver for `USER_PRESENT` in the manifest was not called
  on an unlock (one try), as Android 8's limits on such broadcasts say. So
  "waits until the phone is unlocked" could not be built without the app
  running, and it is not needed: the rule that keeps the lock screen safe
  is that nothing is shown, and the job shows nothing.
- **Seen, not tried further:** the keyboard over the lock screen (the
  emulator's Gboard) showed its own row of buttons, the clipboard's among
  them. Whether the clipboard or learned words show over a lock screen is
  the keyboard's choice; the field should ask for no personalised learning
  there (`IME_FLAG_NO_PERSONALIZED_LEARNING`), and phones have to be tried.

The PIN was removed afterwards. **This is the emulator's answer only.**
Whether a tile or a lock-screen shortcut opens over the lock screen is each
phone's choice, and Samsung's in particular is not known.

**7. The fallback, and where a thought waits.** A thought waits as a file
of its own in the app's private storage, named by the note's ID:

```text
<files>/dirt/waiting/<UUID v7>.txt
```

- **Kotlin makes the ID**, at ✓, the way `Uuid::now_v7` does (48 bits of
  Unix milliseconds, then random bits, with the version and variant set),
  so the sheet needs nothing from Rust. A note's creation time is read
  from its ID, so it is the time of capture however long the file waited.
- The file is written whole as `<ID>.tmp`, flushed to the disk, and
  renamed, so a `.txt` is never half a thought.
- `take_in` (`dirt-mobile`'s `capture.rs`) makes each a note with
  `Store::create_note_with_id`, new in `dirt-core`: it makes the note only
  if no note has that ID, deleted or not, and dates the edit at the time
  the ID carries (never later than now). Then it removes the file. Taking
  the same file in twice, at once or after a crash, makes one note; a
  test shows it.
- The app runs it when it starts and each time the person returns;
  the job runs it before its sync.

**8. The pile widget.** A sketch, not built. The app, and the job, write
the few numbers the drawing needs to a small file each time the notes
change: how many there are, and how many of each shade in the layers the
widget draws (no words, no IDs). The widget's provider, in Kotlin, draws
the heap from those numbers onto a bitmap and gives it to `RemoteViews`,
adding a grain for each file still waiting. After ✓ the sheet asks the
widget to redraw, so the new grain is there before the job has run. The
same numbers serve the "1,254 thoughts" line. Nothing has to be running to
redraw it: a widget's provider is started by Android on its own.

**9. Voice: where a recording waits, and who makes its words.** Reasoned
from what the trial found, not built.

- **Where it waits**: beside the typed thoughts, as `<ID>.ogg` in the same
  waiting folder, written as `.tmp` while it records and renamed at ✓.
  Its ID is made at the first sound, so the note's time is when the person
  spoke. Over the lock screen it is the same, since the storage is open.
- **Who writes it**: design 6.18 has every platform write Ogg Opus from
  Rust. The sheet would then load the library when 🎙 is pressed, which
  took 0.1 to 0.6 s in a new process; Kotlin's `AudioRecord` can start
  hearing at once and hand the sound over when the library is ready, so
  nothing said is lost. (Android's own `MediaRecorder` writes Ogg Opus from
  Android 10, by its documentation; that would keep the sheet free of the
  library, at the cost of a second way of writing the recording.)
- **The same job carries it.** `take_in` would make a waiting `.ogg` a note
  named by its length, with the recording attached as its voice recording,
  which the core encrypts like any file, and the sync sends it. That is the
  job measured above, unchanged but for the file.
- **Its words need a second job**, asked for by the first once the note
  exists, with no need of a network, on the device that recorded it (6.18's
  rule). It loads the model (68 to 110 MiB) and reads the recording once.
  An expedited job is for short work; a long recording should be read by an
  ordinary job, which Android may put off and may stop after some minutes,
  and which Android runs again if its process dies. If it never runs, the
  app makes the words the next time it runs, as 6.18 already says. Over
  the lock screen the model is in the same open storage. Not known: whether
  the microphone may be used, or its permission asked for, by a sheet over
  the lock screen; it is safest that a first use there asks for the phone to
  be unlocked.

#### How Android's native code should be laid out for steps 8 and 9

```text
crates/dirt-mobile/
├── Dioxus.toml               # the app's manifest and MainActivity.kt, as today
├── build.rs                  # the bootstrap config only; the XML it writes today moves into the module
├── android/
│   ├── AndroidManifest.xml   # the launcher activity, and its shortcuts' <meta-data>
│   ├── MainActivity.kt       # the web view's activity only; the old widget goes
│   └── native/               # a Gradle library module, `:plugins:dirtnative`
│       ├── build.gradle.kts  # no plugin versions; its own dependencies, if any
│       └── src/main/
│           ├── AndroidManifest.xml   # the sheet, the job, the tile, the widgets, the Talk entry, permissions
│           ├── kotlin/dev/dirt/capture/
│           │   ├── CaptureActivity.kt    # the sheet
│           │   ├── CaptureJobService.kt  # the job
│           │   ├── Waiting.kt            # the waiting folder and the draft
│           │   ├── Core.kt               # the JNI calls into the library
│           │   └── …                     # tile, widgets, Talk
│           └── res/                      # layout, drawable, values, values-night, xml
└── src/
    ├── main.rs               # declares the module with #[manganis::ffi]
    └── capture.rs            # take_in and the JNI functions; no SQL, HTTP or cryptography
```

`Store::create_note_with_id` belongs in `dirt-core`, as the trial put it.
The sheet's look should be made with the tokens of `dirt_ui::tokens`, as
colours written into the module's `values` and `values-night` by a small
generator, so that the colours are still written once.

#### Still unknown until it is tried on real phones

- Every figure above, the keyboard's above all.
- Whether the tile, a Samsung lock-screen shortcut and the note-taking role
  open the sheet over the lock screen on a Pixel and on a Samsung
  (`docs/MANUAL_CHECKLIST.md`, L1 and L6).
- What the phone's keyboard offers over the lock screen: the clipboard,
  suggestions learned from what was typed before.
- How long a phone's battery saving, or a maker's own (Samsung's "sleeping
  apps"), puts the job off, and the expedited quota in daily use.
- A release build: the library's size and load time, and that `dx` still
  finds the module's record after link-time optimisation.
- The phone's 16 KB memory pages (found on the owner's Pixel on
  2026-11-13) with a library loaded from a job.
- A thought kept offline, then the phone going online.

## Steps

### Step 1: what the interface needs from the core

No screen changes. Each item is a pull request with tests in `dirt-core`,
and in `dirt-sim` where two devices are involved.

| Item | What |
|---|---|
| Archive | An `archived` value in the note's document, derived into its row. Not an edit: it changes neither the edit time nor a deletion (design 6.19). The list leaves archived notes out; search gives them after the rest; an export marks them. `dirt archive`, `dirt unarchive`, `dirt list --archived` |
| Undo of a delete | The store's restore, offered to the clients. `dirt restore <ID>` |
| A note emptied again | With no files: taken off the device altogether if it never reached the server, deleted like any other if it did |
| Tags | For the words and tags chosen, each other tag with how many notes it would leave; "any of them" as well as "all of them" |
| The pile and the layers | Every live note's ID and edit time in one cheap call, since the apps hold only the notes their list shows |
| Sync, in short | A short form of each state in `dirt_core::sync::overview` beside the full sentences |
| A file in memory | A file's decrypted bytes from `dirt-core::files` without writing them to disk |

**Settle first:** how a change that is not an edit is noticed as unsent and
pushed; what a note's document from before archive reads as.

**Settled**, with the pull request that built archive, undo and the note
emptied again:

- *A change that is not an edit* is noticed by the row, not by the
  document. Each note's row has a revision that rises with every write
  made on the device, and the note is unsent while that differs from the
  revision last pushed (`dirt-core::store::sync_state`). A delete is not
  an edit either, and has always been sent this way. Archiving saves
  through the same path, so an archive made offline is unsent and the next
  cycle pushes it, and one that arrives from another device is merged and
  derived into the row without making the note unsent. Nothing in the
  protocol or on the server changes.
- *A document from before archive* reads as not archived: the mark is
  `archived` in a map `marks` of the document, and such a document has no
  such map. A build from before archive keeps the mark in a document it
  merges, edits and sends on, because yrs stores and writes out what it
  merged under a name the program never asked for.
  `crates/dirt-core/tests/notes_archive.rs` plays such a build. For the
  same reason a store from before archive reads its documents once when it
  is upgraded, to find the notes that are already archived.

Two things followed from these and are in the system design (6.2, 6.5 and
16.2). The row also keeps the revision of the note's last edit, so that a
copy that is unsent only for having been archived is not kept as a new note
when the server has purged the note. And "never reached the server" is a
mark of its own, set before a push leaves and not when its answer arrives,
because the answer can be lost.

Left open by the design and settled here, in `docs/ui/design.md` 6.1 and
6.19: a note of spaces and line breaks alone is empty; narrowing to a tag
is finding, so it keeps archived notes after the rest, and the count beside
a tag counts them.

**Settled**, with the pull request that built the other four rows. Each is
written into `docs/ui/design.md` where it belongs (6.3, 6.6, 6.16, 6.17):

- *Tags.* The number beside a tag is the number of notes Find then gives
  for the same words and the chosen tags with that tag added, under the
  rule in force, archived notes among them. A test holds the two to each
  other for every pair of tags and both rules. Under "any of them" a tag
  takes nothing away: its count is what there is and what it would add,
  and it is offered when a note that matches the words carries it. The
  order is the most notes first, then by name. `find_notes` and the lists
  by one tag are as they were; `find_notes_by_tags`, `list_notes_by_tags`
  and `list_archived_notes_by_tags` take several tags and a rule.
- *Layers.* "This week" is the six days before today, not the calendar's
  week, so that no layer depends on where a week starts and yesterday is
  never "Earlier this month". It may reach into last month or last year.
  An edit time later than now is today. Dates are local, by the one
  offset from UTC the caller gives for now.
- *The pile.* Oldest first, by when each note was made. A thought is any
  note that is not deleted; archived notes are counted among them and
  again by themselves. It is read by the index the list already has,
  which holds the edit time and the archived mark, so that only the ID is
  taken from each note's row and long notes cost no more than short ones
  (`docs/LIMITS.md`). No index was added for it.
- *A grain* is the mockup's own hash of the note's ID, with the mockup's
  shifts made unsigned so that every shade is in the design's range.
- *Sync, in short.* Six states and no seventh: a file the server refused
  and a sync that waiting does not cure (sign in again, update the app)
  are both "a problem", with words of their own ("1 file can't sync",
  "Can't sync"). "Offline" is any failure sync tries again by itself.
  "2 waiting" counts notes, and files only when no note waits. A device
  waiting to be let in shows as signed out. The worker's status now
  counts refused files, so that an app has the state without reading the
  store.
- *A file in memory.* The device's own copy of a file was never
  encrypted (`docs/SECURITY.md`), so "not written to disk" can only mean
  that no further copy is made. A file fetched to be shown is kept as any
  fetched file is, and read into memory from there; the design's 6.16
  said more than that and is corrected.
- *The desktop's `time_format`* stays where it is. It makes the label of
  the old note row ("2d", "1w"), which step 3 replaces with the design's
  ("Thu", "Oct 3") and deletes. The core took only what both apps need
  the same answer to: which layer a time falls in.

**Done when:** two simulated devices that archive, bring back, edit and
delete the same note in every order end the same; a store from the
previous schema opens and keeps its notes; the CLI's new commands are in
`docs/feature-parity.md`.

### Step 2: `dirt-ui`, the foundation

A new crate that both apps use. It holds no screen of either app yet.

- The tokens of design section 4 as one style sheet: light, dark, the six
  accents. A test computes the contrast of every pair section 4.1 lists
  and fails under the ratios it promises.
- Inter and JetBrains Mono, bundled. The icons of 4.4.
- The motion tokens of section 5, taken from the mockup, with the fallback
  for a web view without `linear()`, and one script for the springs, the
  list's animation and reduced motion.
- The small components of section 6: icon button with its tooltip, the
  four kinds of button, toast with **Undo**, banner, segmented control,
  slider, switch, accent dots, panel and sheet, confirmation in place,
  the drawn scrollbar.
- Theme, accent and text size, kept on the device, on both apps.
- The web view's own keys turned off in release builds (design 7.4).
- A gallery, built only for development, that shows every component on the
  desktop and on Android.

**Done when:** the gallery runs on Windows and on the emulator; a list in
it reorders at the display's rate; a card follows a finger with no call
into Rust until release; turning on reduced motion takes effect at once;
the contrast test passes.

**Left by this step for later ones**, each in the step that has what it
needs:

| What | Step |
|---|---|
| The text size's default becomes 15 px (design 4.2). The old desktop editor reads the same setting, at 14, until it is replaced | 3 |
| <kbd>Ctrl Z</kbd> after the toast has gone. The script presses **Undo** on a toast that is showing; the key on its own is one of 7.4's | 3 |
| The app's own context menu (Copy, Paste, Select all). Release builds show no menu until then, as they did before | 3 |
| Back closes a sheet. The script closes it on <kbd>Esc</kbd> and on a drag; Android's Back is the app's to hand on | 4 |
| Pull to sync, and the note growing out of its card: the two gestures of 5.5 the script does not have yet | 4 |
| Auto on Android. The WebView takes light or dark from the activity's theme, which is a light one, so Auto is light there. The activity needs a theme that follows the system and `uiMode` among the changes it handles itself: without that, Android restarts the activity when the system's theme changes, and the app ends (seen on the emulator) | 4 |
| Reduced motion on Android while the app is open. The WebView reads "Remove animations" only when it starts. Read `Settings.Global.ANIMATOR_DURATION_SCALE` each time the person returns, and give the answer to `UiRoot`'s `reduced_motion` | 4 |
| Wallpaper, the seventh accent on Android 12 and later (design 4.1.1), with its contrast check. A stored accent a build does not know reads as Clay | 5 |
| A toast that stays while a screen reader is on (design 12). `Toast::hold` does the staying; knowing that TalkBack is on needs native code | 10 |
| Trying the fallbacks where they are used: a web view without `linear()` or without View Transitions (WebKitGTK), and the web view's keys in a release build on each system | The manual checklist, G1 to G9 |

### Step 3: the desktop's main screen

The page, the list in its layers, Find and tags, archive, delete and undo,
the sync status, focus while writing (Slide, then Rail and Zen), the window
without a title bar, and every key in design 7.4. The old settings stay
reachable until step 5. The app's root becomes `dirt_ui::UiRoot`, and the
text size's default becomes 15 px.

**Fallback written in advance:** a platform where the window without a
title bar cannot be moved or resized keeps the system's title bar.

**Done when:** every desktop row of design section 2 that this step covers
takes the number of actions the table gives, counted by hand.

**Split in two.** 3a is the screen: the page, the list in its layers, Find
and tags, archive, delete and undo, the sync status, the app's own menu,
<kbd>Ctrl Z</kbd> after the toast has gone, the text size's default of
15 px, and every key of 7.4 but those of later steps' features. What it
settled is in design section 7.8; its shared parts (the row, Find and the
list of tags, the sync status, the layer line, the menu, load more) are in
`dirt-ui` for step 4. 3b is focus while writing (7.7), the controls shown
by nearness (6.4's second half) and the window without a title bar (7.6).

3b also fixes two things 3a's review left open:

- **Typing while a note is left is lost.** Done (Esc, Ctrl Enter, ✓) and
  opening another note wait on the store to save and, for an emptied
  note, let it go, before the page is blank or shows the other note.
  What is typed in that moment goes into a page about to be replaced.
  Two arrow presses before the first note is shown move one row, and
  attaching a file while the first character's note is still being made
  makes a second note. The page should change at once and the leaving
  be done from what it held, so that the next thought can be typed
  straight after Esc: capture in under two seconds is the product.
- **A delete undone comes back at the top of the list**, not into its
  place (design 6.3). The store's restore is a change to the document,
  and moves the edit time. Restoring should keep the edit time the note
  had, as unarchiving does; that is `dirt-core`'s, with its own tests,
  and must still sync as a change.

Both were fixed in 3b; design sections 7.8 and 7.9 say how. 3b built the
window without a title bar on Windows only: Linux and macOS keep the
system's title bar until it has been tried there, as the fallback above
allows.

### Step 4: Android's main screen

The dock, cards, Find, the note that grows out of its card, swipe to
archive, pull to sync, saving as you type, load more on scroll. Runs
alongside step 3; the two share components through `dirt-ui`, and a
component both need is added there by whichever step reaches it first.

**Done when:** the same count as step 3, for the Android column.

**Built.** What it settled is in design section 8.7. Of the rows step 2
left to it: Back is the page's (the activity asks `DirtUi.back()`, which
closes a sheet, the note, the archived notes or Find, and otherwise the
app goes behind the others); pull to sync and the note growing out of its
card are in the script; Auto follows the system, with the activity on
AppCompat's DayNight theme and `uiMode` among the changes it takes itself;
"Remove animations" is read when the app starts and each time the person
returns. The new parts in `dirt-ui` are `NoteCard`, `GrowPage`,
`PullToSync`, `TagStrip`, Find's cross and the sync status's dot. The
toast after a share stays with step 8. Until step 5, Settings and Devices
are the old screens over the main one, and Settings has an Appearance
card with the theme, the accents and the text size.

### Step 5: Settings and the flows

One page in six sections on both apps: account, devices with the code
comparison and the recovery phrase, sync with the notes that cannot sync,
appearance, voice (empty until step 9), export. The banner for a waiting
device. The old settings screens are deleted.

**Done when:** every row of `docs/feature-parity.md` under "Account and
devices", "Sync" and "Export" has been done once on each app against the
trial server.

**Split in two.** 5a is the shared page and the desktop; 5b is Android.

**5a built.** `dirt_ui::settings` is the page in five sections (Voice comes
with step 9), the rail, the code comparison, the recovery words and the
banner; each app gives it what only the app can do (the browser for
signing in, where an export goes, the clipboard, opening a note). The
desktop's old Settings, its tabs and the controls only it used are gone.
What it settled is in design section 9.7. The core gives the code as
seven pictures (`dirt_core::enrolment::ShortCode`), and the overview the
head of Settings → Sync. Every row of the three sections was done on the
desktop against `dirt-trial`, with the CLI as the other devices; a note the
server refuses, offline and a file an export cannot have were done against
the local server (`docs/MANUAL_CHECKLIST.md`, Runs).

**5b built.** Android's Settings is the same page in the shared sheet,
with the banner; the old Settings and Devices screens, their styles and
buttons, and the light theme they were drawn in are gone. Android's own
parts are callbacks, as the desktop's are: signing in in a browser tab,
the export as one zip file, the clipboard marked sensitive. Every row of
the three sections was done on the emulator against `dirt-trial`, but for
a note that cannot sync: a note too large to sync cannot be typed on the
phone, since text past a few hundred kilobytes does not reach the app
from the web view (seen on the desktop too). That is the main screen's,
and is for step 10.

### Step 6: files

The file tile, the preview from memory, dropping files to make a note,
pasting a picture. A PDF renderer is bundled, so PDFs show their pages in
the preview on every platform (decided 2026-11-18). `docs/SECURITY.md` says what the preview keeps in
memory and what "Open in another app" writes.

**Split in two.** 6a is the shared tile and preview and the desktop; 6b is
Android.

**6a built.** `dirt_ui::files` is the tile, the preview and what they
share (`NoteFiles`), the same for both apps; each app gives them another
app and a save dialog. The bytes the preview shows are held in memory and
answered to the web view from there (`dirt_ui::files::served`). PDF.js
6.4.299 is bundled in `dirt-ui`, 3.5 MiB, Apache 2.0, and draws every PDF.
The core gives a tile its words (`AttachedFile::whereabouts`) and attaches
a pasted picture from memory (`Store::attach_bytes`). On the desktop, files
dropped anywhere on the window are attached or make a note, and a picture
pasted into the page is attached. The desktop's old list of files is gone.
What it settled is in design section 6.16. Each kind was attached,
previewed, opened in another app and saved against the local server, a
file not on the device was fetched with its ring, and both themes were
looked at (the pull request says how); drag and drop from a file manager,
a real paste and a real program opening a file are in the manual
checklist (D17 to D23, D57 to D62).

**6b built.** Android's old list of files (`views/attachments.rs`) is
gone; the note's files are `dirt-ui`'s tiles and preview, with what
Android alone does in `dirt-mobile`'s `files` module: Android's picker,
the `FileProvider`'s lend with a read grant, and Android's save screen.
The answer to the web view's requests for the preview's bytes is built
once, in `dirt-ui`, for both apps. One choosing or share takes at most a
note's 20 files. Everything the review hardened on the desktop holds here
too. Run on the emulator against the local server with real touch (taps
and swipes by `adb shell input`, the pinch by the emulator's multi-touch
device): four kinds attached, each previewed, swiped between and pinched,
opened in another app and saved, a picture shared from Files, a file not
on the device fetched with its ring, both themes. Real touch found four
faults in the script, mended and written into design 6.16.

### Step 7: grains and the pile

The settling of a new thought, the grain of each note, the pile under the
list, the Rail's dots. The pile is worked out in Rust, once per change,
from step 1's call.

### Step 8: capture from outside the app

- Desktop: quick capture as its own small window, kept ready so that it
  shows at once. Today it is the main window made small.
- Android: the capture sheet, the widget, the app shortcut, the Quick
  Settings tile, the note-taking role, the toast after a share, and the
  background work or its fallback as trial 2 found.
- `docs/SECURITY.md` gains capture over the lock screen.

**Fallback written in advance:** where the tray icon's place on screen
cannot be known, the grain falls and fades where the window was instead of
flying to the tray.

### Step 9: voice

The engine and model that trial 1's second measurement chooses, listening
on the page and in quick capture, the held hotkey, Android's dock and
capture sheet, the Talk entry, Settings → Voice. A recording is kept as a
note at once, and its words are made afterwards in the background by the
device that recorded it (design 6.18). `docs/SECURITY.md` gains voice. The model, Parakeet
TDT-CTC 110m, is shipped inside both apps (decided 2026-11-18): nothing is
fetched and no storage of Dirt's is changed.

### Step 10: finishing

The pile widget. Long text in the page: past a few hundred kilobytes it
does not reach the app from the web view on either app, so the note is
not made (found in step 5). A pass over design section 12, item by item, on both
apps. `docs/MANUAL_CHECKLIST.md` rewritten for the new interface. What is
left of the old interface and its dependencies removed. `README.md` and
`CLAUDE.md` brought up to date.

## What runs alongside what

```
trial 1 ─────────────────────────────────────────────┐
trial 2 ──────────────────────────────┐              │
step 1 ──┐                            │              │
step 2 ──┴─ step 3 ─┐                 │              │
            step 4 ─┴─ step 5 ─ step 6 ┬─ step 8 ─ step 9 ─ step 10
                                step 7 ┘
```

At most three run at once on the one machine that builds them.
