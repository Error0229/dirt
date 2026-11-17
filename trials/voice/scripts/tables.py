"""Prints the measured tables of the trial's result as Markdown, from
data/results/summary.json (scripts/analyse.py writes it).

    python -I scripts/analyse.py data/results > /dev/null
    python -I scripts/tables.py
"""
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent.parent
rows = json.loads((HERE / "data" / "results" / "summary.json").read_text())

LIVE = [
    ("sherpa-zipformer-20M", "sherpa-onnx, zipformer-en-20M (8-bit)", "42"),
    ("sherpa-zipformer-2023-06-26", "sherpa-onnx, zipformer-en-2023-06-26 (8-bit)", "68"),
    ("sherpa-kroko", "sherpa-onnx, Kroko English", "68"),
    ("sherpa-nemo-480ms", "sherpa-onnx, NeMo fast conformer 480 ms (8-bit)", "131"),
    ("moonshine-tiny-sliding", "sherpa-onnx, Moonshine tiny read again every 0.3 s", "42"),
    ("moonshine-tiny-sliding-500", "sherpa-onnx, Moonshine tiny read again every 0.5 s", "42"),
    ("moonshine-base-sliding-500", "sherpa-onnx, Moonshine base read again every 0.5 s", "135"),
    ("whisper-tiny.en", "whisper.cpp, `tiny.en`, full window", "74"),
    ("whisper-tiny.en-short", "whisper.cpp, `tiny.en`, short window", "74"),
    ("whisper-base.en-short", "whisper.cpp, `base.en`, short window", "141"),
    ("whisper-base.en-q5-short", "whisper.cpp, `base.en` 5-bit, short window", "57"),
    ("whisper-small.en-short", "whisper.cpp, `small.en`, short window", "465"),
]

print("| Engine and model | Model, MiB | Mistakes: all (human, synthetic) | Shown after the word ended: median (9 in 10) | Changed, per 100 words | Speed, of real time | Worst lag | Memory, MiB | Load |")
print("|---|---|---|---|---|---|---|---|---|")
for key, name, size in LIVE:
    r = rows.get(key)
    if not r:
        continue
    runs = "" if r["runs"] == 3 else f" ({r['runs']} runs)"
    print(f"| {name}{runs} | {size} | {r['wer']:.1f}% ({r['wer_human']:.1f}, {r['wer_synthetic']:.1f}) | "
          f"{r['first_p50']:.2f} s ({r['first_p90']:.2f} s) | {r['changed_per_100']:.0f} | {r['speed']:.2f} | "
          f"{r['worst_lag']:.1f} s | {r['peak_mib']:.0f} | {r['load_seconds']:.1f} s |")

print()
SECOND = [
    ("second-moonshine-tiny", "Moonshine tiny (8-bit), through sherpa-onnx", "42"),
    ("second-moonshine-base", "Moonshine base (8-bit), through sherpa-onnx", "135"),
    ("whisper-tiny.en-short", "whisper.cpp, `tiny.en`", "74"),
    ("whisper-base.en-q5-short", "whisper.cpp, `base.en` 5-bit", "57"),
    ("whisper-base.en-short", "whisper.cpp, `base.en`", "141"),
    ("whisper-small.en-short", "whisper.cpp, `small.en`", "465"),
]
print("| Model | Model, MiB | Mistakes | Time for one thought, median |")
print("|---|---|---|---|")
for key, name, size in SECOND:
    r = rows.get(key)
    if r:
        print(f"| {name} | {size} | {r['wer2']:.1f}% | {r['second_seconds']:.2f} s |")

print()
print("| | After the last word: median (worst) |")
print("|---|---|")
for key, name in [("sherpa-zipformer-20M", "Silero's detector, 22 thoughts"),
                  ("sherpa-zipformer-2023-06-26", "recogniser's rule, zipformer-en-2023-06-26"),
                  ("sherpa-kroko", "recogniser's rule, Kroko"),
                  ("sherpa-nemo-480ms", "recogniser's rule, NeMo")]:
    r = rows.get(key)
    if r:
        value = (r["end_vad_p50"], r["end_vad_max"]) if "Silero" in name else (r["end_engine_p50"], r["end_engine_max"])
        print(f"| {name} | {value[0]:.2f} s ({value[1]:.2f} s) |")

print()
print("calibration, s:", sorted({c for r in rows.values() for c in r["spread_calibration"]}))
