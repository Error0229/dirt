# Trial 1: the voice engine

Measuring code for the first trial of the interface redesign
(`docs/plans/2026-11-16-interface-redesign-implementation-plan.md`). It is
kept on the branch `trial/voice-engine` for reference and is not merged.
The result is the section "Trial 1 result" of that plan.

It is its own cargo workspace, so it touches neither the main workspace nor
its `Cargo.lock`. Models, test speech and results live in `data/`, which is
ignored: nothing of them is committed.

## What is here

| | |
|---|---|
| `src/live.rs` | Feeds an engine recorded speech in 100 ms pieces at the speed of speech (or on a clock that only counts the engine's working time), and writes down every change to what would be on screen |
| `src/whisper_engine.rs` | whisper.cpp through `whisper-rs`, made to give words as they are heard: the sound so far is read again and again, and a word is settled once two readings agree on it |
| `src/sherpa_engine.rs` | sherpa-onnx: a streaming transducer, its own rule for the end of a thought, Silero's voice-activity model beside it, and Moonshine as a second reading of the finished thought |
| `src/ogg_opus.rs` | The recording written as Ogg Opus |
| `scripts/fetch-models.sh` | Fetches the models |
| `scripts/make-speech.ps1`, `scripts/make-set.py` | Make the test set: ten utterances of LibriSpeech test-clean with word timings, twelve thoughts spoken by Windows' synthesiser, and the pause set |
| `scripts/run-windows.sh` | Runs every configuration three times |
| `scripts/analyse.py`, `scripts/analyse-pauses.py` | Turn the runs into the figures |
| `scripts/env-windows-whisper.sh` | What whisper needs to build properly on Windows |
| `scripts/build-android.sh`, `scripts/android.toolchain.cmake` | Cross-compile for Android with NDK 26.3 |
| `scripts/can-play.html`, `scripts/can-play.py` | Ask a headless browser which recording formats it really plays |
| `scripts/phone.sh` | The phone's half. Not run: it needs the owner's word |
| `scripts/sample-load.ps1` | Writes down the machine's load while a measurement runs |

## Running it again

```bash
cd trials/voice
bash scripts/fetch-models.sh
# LibriSpeech test-clean from https://www.openslr.org/12, unpacked under data/librispeech
powershell -NoProfile -File scripts/make-speech.ps1 -Out data/set
python -I scripts/make-set.py data/librispeech/LibriSpeech/test-clean

cargo build --release --features sherpa
cp target/release/voice-trial.exe data/voice-trial-sherpa.exe && cp target/release/*.dll data/
. scripts/env-windows-whisper.sh
cargo build --release --features whisper
cp target/release/voice-trial.exe data/voice-trial-whisper.exe

bash scripts/run-windows.sh sherpa
bash scripts/run-windows.sh whisper
bash scripts/run-windows.sh pauses
python -I scripts/analyse.py data/results
python -I scripts/analyse-pauses.py
```
