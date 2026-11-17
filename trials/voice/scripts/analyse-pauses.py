"""Can 1.6 seconds of quiet after speech be told from a pause inside a thought?

Each file of the pause set (scripts/make-set.py) is a thought with a pause
of 0.6, 1.0 or 1.4 seconds put in its middle, clean or under pink noise,
followed by three seconds of room quiet. A way of finding the end is right
on a file when it does not call the end inside the pause and does call it
after the last word; how long after is its delay.

Four ways are compared, each asked for 1.6 seconds of quiet:
  - the recogniser's own rule (sherpa-onnx: quiet since the last word it decoded),
  - Silero's voice-activity model run by sherpa-onnx as the sound arrives,
  - the same model as whisper.cpp runs it (over the finished sound),
  - loudness alone: quiet is anything under a level 8 dB above the room's.

One file's pause really is 1.8 s (the speaker's own gap at a comma plus the
1.4 s put in); ending there is right and is not counted as a mistake.

    python -I scripts/analyse-pauses.py
"""
import json
import statistics
import wave
from collections import defaultdict
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent.parent
PAUSES = HERE / "data" / "pauses"
RESULTS = HERE / "data" / "pause-results"
QUIET = 1.6


def judge(meta, quiet_runs, total):
    """`quiet_runs`: (start, end) of every stretch judged quiet, in seconds.
    Returns (called the end inside the pause, delay of the real end or None)."""
    false_end = any(a < meta["pause_ends"] - 0.05 and b - a >= QUIET for a, b in quiet_runs
                    if a >= meta["pause_starts"] - 0.5)
    early = any(b - a >= QUIET and a + QUIET < meta["speech_ends"] for a, b in quiet_runs)
    if long_pause(meta):
        # The speaker's own gap and the pause put in add up to 1.6 s or
        # nearly: ending there is what was asked for, not a mistake.
        false_end = early = False
    final = [a for a, b in quiet_runs if b >= total - 0.05 and b - a >= QUIET]
    delay = final[0] + QUIET - meta["speech_ends"] if final else None
    return false_end or early, delay


def long_pause(meta):
    return meta["pause_ends"] - meta["pause_starts"] >= QUIET - 0.1


def runs_from_flags(flags, frame):
    """Stretches where `flags` (one per frame, True = speech) is False."""
    runs, start = [], None
    for i, speech in enumerate(list(flags) + [True]):
        if not speech and start is None:
            start = i
        if speech and start is not None:
            runs.append((start * frame, i * frame))
            start = None
    return runs


def loudness_runs(path):
    with wave.open(str(path)) as w:
        samples = np.frombuffer(w.readframes(w.getnframes()), dtype=np.int16).astype(np.float32) / 32768
    # Three seconds of room quiet follow every file when it is fed.
    rng = np.random.default_rng(7)
    samples = np.concatenate([samples, (rng.random(3 * 16000) - 0.5).astype(np.float32) * 0.0006])
    n = len(samples) // 160
    level = 10 * np.log10((samples[: n * 160].reshape(n, 160) ** 2).mean(axis=1) + 1e-12)
    # The room's level is taken from the quietest tenth of the first two seconds heard so far.
    flags, floor = [], level[0]
    for i in range(n):
        if i % 25 == 0:
            floor = np.percentile(level[: max(i + 1, 30)], 10)
        flags.append(level[i] > floor + 8)
    # A single loud frame is not speech: ask for three in five.
    smooth = [sum(flags[max(0, i - 4): i + 1]) >= 3 for i in range(n)]
    return runs_from_flags(smooth, 0.01), n * 0.01


def main():
    table = defaultdict(lambda: defaultdict(list))  # way -> condition -> [(false end, delay)]
    metas = {p.stem: json.loads(p.read_text()) for p in PAUSES.glob("*.json")}

    for path in sorted(RESULTS.glob("pauses-*.jsonl")):
        for line in path.read_text().splitlines():
            record = json.loads(line)
            if record["kind"] == "thought":
                meta = metas[record["id"]]
                condition = "clean" if meta["snr_db"] is None else f"noise {meta['snr_db']} dB under"
                total = record["fed_seconds"]
                marks = record["marks"]
                # The recogniser's own rule fires once the quiet has lasted.
                ends = [m["heard"] for m in marks if m["what"] == "engine-endpoint"]
                inside = any(meta["pause_starts"] < t < meta["pause_ends"] + 0.3 for t in ends) and not long_pause(meta)
                early = any(t < meta["speech_ends"] for t in ends) and not long_pause(meta)
                after = [t for t in ends if t >= meta["speech_ends"]]
                name = path.name.split(".")[0].replace("pauses-", "recogniser's rule, ")
                table[name][condition].append((inside or early, after[0] - meta["speech_ends"] if after else None))
                # Silero beside it, if it ran: "quiet" is reported 0.25 s after it began.
                if any(m["what"].startswith("vad") for m in marks):
                    runs, start = [], 0.0
                    for m in marks:
                        if m["what"] == "vad-speech" and start is not None:
                            runs.append((start, m["heard"]))
                            start = None
                        if m["what"] == "vad-quiet":
                            start = m["heard"] - 0.25
                    if start is not None:
                        runs.append((start, total))
                    table["Silero, as the sound arrives (sherpa-onnx)"][condition].append(judge(meta, runs[1:] if runs and runs[0][0] == 0.0 else runs, total))
            elif record["kind"] == "vad":
                meta = metas[record["id"]]
                condition = "clean" if meta["snr_db"] is None else f"noise {meta['snr_db']} dB under"
                flags = [p > 0.5 for p in record["probabilities"]]
                frame = record["frame_seconds"]
                runs = [r for r in runs_from_flags(flags, frame) if r[0] > 0.0]
                table["Silero, over finished sound (whisper.cpp)"][condition].append(judge(meta, runs, len(flags) * frame))

    for name, meta in metas.items():
        condition = "clean" if meta["snr_db"] is None else f"noise {meta['snr_db']} dB under"
        runs, total = loudness_runs(PAUSES / f"{name}.wav")
        table["loudness alone"][condition].append(judge(meta, [r for r in runs if r[0] > 0.3], total))

    print(f"{'way':48} {'condition':20} files  ended in pause  end missed  delay after last word: median (worst)")
    for way, conditions in table.items():
        for condition in ["clean", "noise 20 dB under", "noise 10 dB under"]:
            rows = conditions.get(condition, [])
            if not rows:
                continue
            delays = [d for _, d in rows if d is not None]
            median = statistics.median(delays) if delays else float("nan")
            worst = max(delays) if delays else float("nan")
            print(f"{way:48} {condition:20} {len(rows):5}  {sum(f for f, _ in rows):14}  {sum(d is None for _, d in rows):10}  "
                  f"{median:.2f} s ({worst:.2f} s)")


if __name__ == "__main__":
    main()
