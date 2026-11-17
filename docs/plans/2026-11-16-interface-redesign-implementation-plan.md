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

## Still the owner's

Each has a default that holds if the step is reached without an answer.

| Decision | Needed by | Default |
|---|---|---|
| PDF in the preview: bundle a renderer (a few MB) or not | Step 6 | Not bundled: a PDF opens in another app on Linux and Android |
| The speech model: fetched on first use, or shipped in the app | Step 9. Trial 1 has the sizes: 68 or 110 MiB | Fetched on first use |
| Which speech model: Kroko's, 68 MiB, under CC BY-SA once its makers confirm it; or the zipformer of 2023-06-26 with Moonshine tiny, 110 MiB, under Apache-2.0 and MIT (trial 1 result, "Recommendation") | Step 9 | The pair: its licences ask nothing |
| Snap Layouts on Windows 11: accept losing the flyout, or win it back | Step 3 | Accept losing it |

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

### Step 4: Android's main screen

The dock, cards, Find, the note that grows out of its card, swipe to
archive, pull to sync, saving as you type, load more on scroll. Runs
alongside step 3; the two share components through `dirt-ui`, and a
component both need is added there by whichever step reaches it first.

**Done when:** the same count as step 3, for the Android column.

### Step 5: Settings and the flows

One page in six sections on both apps: account, devices with the code
comparison and the recovery phrase, sync with the notes that cannot sync,
appearance, voice (empty until step 9), export. The banner for a waiting
device. The old settings screens are deleted.

**Done when:** every row of `docs/feature-parity.md` under "Account and
devices", "Sync" and "Export" has been done once on each app against the
trial server.

### Step 6: files

The file tile, the preview from memory, dropping files to make a note,
pasting a picture. `docs/SECURITY.md` says what the preview keeps in
memory and what "Open in another app" writes.

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

The engine trial 1 chose, listening on the page and in quick capture, the
held hotkey, Android's dock and capture sheet, the Talk entry,
Settings → Voice. `docs/SECURITY.md` gains voice. The model is put in
Dirt's own storage, which is a change to production's storage and waits
for the owner's word.

### Step 10: finishing

The pile widget. A pass over design section 12, item by item, on both
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
