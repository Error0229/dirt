"""Turns the lines `voice-trial hear` wrote into the trial's figures.

    python -I scripts/analyse.py data/results            # the table
    python -I scripts/analyse.py data/results --texts    # and what each heard

A result file is named <config>.<run>.jsonl; the figures for a config are
the median over its runs. Reference words and the moment each one ends come
from data/set/*.words.json (scripts/make-set.py).
"""
import json
import re
import statistics
import sys
from collections import defaultdict
from pathlib import Path

HERE = Path(__file__).resolve().parent.parent
SET = HERE / "data" / "set"

ONES = "zero one two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen seventeen eighteen nineteen".split()
TENS = "_ _ twenty thirty forty fifty sixty seventy eighty ninety".split()
# Spellings that are the same word to a reader.
SAME = {"ok": "okay", "cancelling": "canceling", "cue": "q", "pm": "p m", "am": "a m", "e-mail": "email",
        "colour": "color", "grey": "gray"}


def number_words(n):
    if n < 20:
        return [ONES[n]]
    if n < 100:
        return [TENS[n // 10]] + ([ONES[n % 10]] if n % 10 else [])
    if n < 1000:
        return [ONES[n // 100], "hundred"] + (number_words(n % 100) if n % 100 else [])
    if n < 10000:
        return number_words(n // 1000) + ["thousand"] + (number_words(n % 1000) if n % 1000 else [])
    return [str(n)]


def plain(text):
    """The words of a text as they are compared: lower case, no punctuation,
    numbers spelt out, so that "3 pm", "3 p.m." and "three PM" are the same."""
    text = text.lower().replace("&", " and ").replace("%", " percent ")
    text = re.sub(r"\$\s*(\d+)", r"\1 dollars", text)
    text = re.sub(r"\b([ap])\.m\.?", r"\1m", text)
    text = re.sub(r"(?<=[a-z])(?=\d)|(?<=\d)(?=[a-z])", " ", text)
    text = re.sub(r"[^a-z0-9' ]+", " ", text)
    words = []
    for word in text.split():
        word = word.strip("'")
        if not word:
            continue
        if word.isdigit():
            words += number_words(int(word))
        else:
            words += SAME.get(word, word).split()
    return words


def same_compounds(hyp, ref):
    """Writes "off site" and "offsite" the way the reference does, so that a
    difference of spacing is not counted as a mistake in hearing."""
    ref_set, ref_pairs = set(ref), {a + b: (a, b) for a, b in zip(ref, ref[1:])}
    out, i = [], 0
    while i < len(hyp):
        if i + 1 < len(hyp) and hyp[i] + hyp[i + 1] in ref_set and not (hyp[i] in ref_set and hyp[i + 1] in ref_set):
            out.append(hyp[i] + hyp[i + 1])
            i += 2
        elif hyp[i] not in ref_set and hyp[i] in ref_pairs:
            out += ref_pairs[hyp[i]]
            i += 1
        else:
            out.append(hyp[i])
            i += 1
    return out


def align(ref, hyp):
    """Edit-distance alignment. Returns (substitutions, deletions, insertions)
    and the pairs (ref index, hyp index) that are the same word."""
    n, m = len(ref), len(hyp)
    cost = [[0] * (m + 1) for _ in range(n + 1)]
    for i in range(n + 1):
        cost[i][0] = i
    for j in range(m + 1):
        cost[0][j] = j
    for i in range(1, n + 1):
        for j in range(1, m + 1):
            cost[i][j] = min(cost[i - 1][j - 1] + (ref[i - 1] != hyp[j - 1]), cost[i - 1][j] + 1, cost[i][j - 1] + 1)
    i, j, subs, dels, ins, same = n, m, 0, 0, 0, []
    while i or j:
        if i and j and cost[i][j] == cost[i - 1][j - 1] + (ref[i - 1] != hyp[j - 1]):
            if ref[i - 1] == hyp[j - 1]:
                same.append((i - 1, j - 1))
            else:
                subs += 1
            i, j = i - 1, j - 1
        elif i and cost[i][j] == cost[i - 1][j] + 1:
            dels, i = dels + 1, i - 1
        else:
            ins, j = ins + 1, j - 1
    return (subs, dels, ins), same[::-1]


def reference(thought_id):
    """A thought's reference words and when each ends, in seconds."""
    info = json.loads((SET / f"{thought_id}.words.json").read_text())
    words, ends = [], []
    timed = info["words"]
    i = 0
    while i < len(timed):
        # The synthesiser reports "Q3" twice, once for each word it says.
        j = i
        while j + 1 < len(timed) and timed[j + 1]["word"] == timed[i]["word"] and len(plain(timed[i]["word"])) > 1:
            j += 1
        spoken = plain(timed[i]["word"])
        group = timed[i:j + 1]
        if len(group) == len(spoken):
            ends += [w["end"] for w in group]
        else:
            # One report for several words ("200"): all end when the last does.
            if len(group) > 1:
                spoken = spoken * 1
            ends += [group[-1]["end"]] * len(spoken)
        words += spoken
        i = j + 1
    text_words = plain(info["text"])
    if words != text_words:
        # Timings that do not line up with the text are spread by position.
        ends = [ends[min(len(ends) - 1, k * len(ends) // max(1, len(text_words)))] for k in range(len(text_words))]
        words = text_words
    return info, words, ends


def percentile(values, q):
    values = sorted(values)
    if not values:
        return float("nan")
    k = (len(values) - 1) * q
    lo, hi = int(k), min(int(k) + 1, len(values) - 1)
    return values[lo] + (values[hi] - values[lo]) * (k - lo)


def analyse_run(path, texts=False):
    run, end, thoughts = None, None, []
    for line in path.read_text(encoding="utf-8").splitlines():
        record = json.loads(line)
        {"run": lambda: None, "end": lambda: None, "thought": lambda: thoughts.append(record)}[record["kind"]]()
        if record["kind"] == "run":
            run = record
        if record["kind"] == "end":
            end = record
    if not thoughts or end is None:
        return None
    errors = defaultdict(lambda: [0, 0])  # kind -> [mistakes, reference words]
    errors2 = defaultdict(lambda: [0, 0])
    first, stable, settled, settled_never = [], [], [], 0
    changed = grew = shown_words = settled_changed = 0
    end_engine, end_vad, second_seconds = [], [], []
    for thought in thoughts:
        info, ref, ends = reference(thought["id"])
        kind = info["kind"]
        final = same_compounds(plain(thought["last"]["settled"] + " " + thought["last"]["tentative"]), ref)
        (subs, dels, ins), same = align(ref, final)
        errors[kind][0] += subs + dels + ins
        errors[kind][1] += len(ref)
        if texts:
            print(f"  {thought['id']}: {subs + dels + ins}/{len(ref)}  {' '.join(final)}")
        if thought.get("second_pass"):
            second = same_compounds(plain(thought["second_pass"]["text"]), ref)
            (s2, d2, i2), _ = align(ref, second)
            errors2[kind][0] += s2 + d2 + i2
            errors2[kind][1] += len(ref)
            second_seconds.append(thought["second_pass"]["seconds"])
            if texts:
                print(f"     second pass: {s2 + d2 + i2}/{len(ref)}  {thought['second_pass']['text']}")

        # For every word that ended up right: when it first appeared, when it
        # last changed, and when it was shown as settled.
        events, raw_events = [], []
        for event in thought["shown"]:
            done = same_compounds(plain(event["settled"]), ref)
            all_words = same_compounds(plain(event["settled"] + " " + event["tentative"]), ref)
            events.append((event["t"], all_words, len(done)))
            # For counting changes the words are taken as written, so that a
            # word still growing ("cu", "cue") is seen as the same word.
            as_written = lambda text: re.sub(r"[^a-z0-9' ]+", " ", text.lower()).split()
            raw_events.append((event["t"], as_written(event["settled"] + " " + event["tentative"]),
                               len(as_written(event["settled"]))))
        seen = {j: [None, None, None] for _, j in same}
        for t, words, n_settled in events:
            _, here = align(final, words)
            here = dict(here)
            for _, j in same:
                slot = seen[j]
                if j in here:
                    if slot[0] is None:
                        slot[0] = t
                    if slot[1] is None:
                        slot[1] = t
                    if slot[2] is None and here[j] < n_settled:
                        slot[2] = t
                else:
                    slot[1] = None  # it went away or changed: not stable yet
        for i, j in same:
            a, b, c = seen[j]
            if a is not None:
                first.append(a - ends[i])
            if b is not None:
                stable.append(b - ends[i])
            if c is not None:
                settled.append(c - ends[i])
            else:
                settled_never += 1

        # How often a word on screen was changed or taken away.
        previous, previous_settled = [], 0
        for t, words, n_settled in raw_events:
            _, kept = align(previous, words)
            kept_from = {a for a, _ in kept}
            for index, word in enumerate(previous):
                if index in kept_from:
                    continue
                # The last word still growing ("off" to "offsite") is not a change.
                if index == len(previous) - 1 and any(w.startswith(word) for w in words[index:index + 1]):
                    grew += 1
                    continue
                changed += 1
            # A settled word is one the engine promised not to change, so
            # it is checked where it stands, not by alignment.
            settled_changed += sum(1 for index in range(previous_settled)
                                   if index >= len(words) or words[index] != previous[index])
            previous, previous_settled = words, n_settled
        shown_words += len(final)

        # The end of the thought: how long after the last word it was called.
        speech_end = ends[-1] if ends else thought["speech_seconds"]
        marks = thought["marks"]
        engine_marks = [m["heard"] for m in marks if m["what"] == "engine-endpoint" and m["heard"] > speech_end]
        if engine_marks:
            end_engine.append(engine_marks[0] - speech_end)
        quiet = [m["heard"] for m in marks if m["what"] == "vad-quiet"]
        speech = [m["heard"] for m in marks if m["what"] == "vad-speech"]
        if quiet and (not speech or quiet[-1] > speech[-1]):
            # The detector says "quiet" 0.25 s after the quiet began.
            end_vad.append(quiet[-1] - 0.25 + 1.6 - speech_end)

    def wer(table, kinds):
        mistakes = sum(table[k][0] for k in kinds)
        words = sum(table[k][1] for k in kinds)
        return 100 * mistakes / words if words else float("nan")

    return {
        "flags": run["flags"],
        "real_time": run["real_time"],
        "calibration": (run.get("calibration_seconds", float("nan")) + end.get("calibration_seconds", float("nan"))) / 2,
        "load_seconds": run["load_seconds"],
        "wer": wer(errors, ["human", "synthetic"]),
        "wer_human": wer(errors, ["human"]),
        "wer_synthetic": wer(errors, ["synthetic"]),
        "wer2": wer(errors2, ["human", "synthetic"]),
        "wer2_human": wer(errors2, ["human"]),
        "wer2_synthetic": wer(errors2, ["synthetic"]),
        "second_seconds": statistics.median(second_seconds) if second_seconds else float("nan"),
        "first_p50": percentile(first, 0.5), "first_p90": percentile(first, 0.9),
        "stable_p50": percentile(stable, 0.5), "stable_p90": percentile(stable, 0.9),
        "settled_p50": percentile(settled, 0.5), "settled_p90": percentile(settled, 0.9),
        "settled_never": 100 * settled_never / max(1, len(first)),
        "changed_per_100": 100 * changed / max(1, shown_words),
        "settled_changed": settled_changed,
        "speed": end["compute_seconds"] / end["fed_seconds"],
        "worst_lag": max(t["worst_lag_seconds"] for t in thoughts),
        "peak_mib": end["peak_bytes"] / 2 ** 20 if end.get("peak_bytes") else float("nan"),
        "end_engine_p50": percentile(end_engine, 0.5), "end_engine_max": max(end_engine, default=float("nan")),
        "end_engine_n": len(end_engine),
        "end_vad_p50": percentile(end_vad, 0.5), "end_vad_max": max(end_vad, default=float("nan")),
        "end_vad_n": len(end_vad),
        "thoughts": len(thoughts),
    }


def main():
    folder = Path(sys.argv[1])
    texts = "--texts" in sys.argv
    by_config = defaultdict(list)
    for path in sorted(folder.glob("*.jsonl")):
        config = path.name.rsplit(".", 2)[0]
        if texts:
            print(path.name)
        result = analyse_run(path, texts)
        if result:
            by_config[config].append(result)
    columns = ["wer", "wer_human", "wer_synthetic", "wer2", "second_seconds", "first_p50", "first_p90", "stable_p50",
               "stable_p90", "settled_p50", "settled_p90", "settled_never", "changed_per_100", "settled_changed",
               "speed", "worst_lag", "peak_mib", "load_seconds", "end_engine_p50", "end_engine_max", "end_vad_p50",
               "end_vad_max", "calibration"]
    out = {}
    for config, runs in by_config.items():
        row = {c: statistics.median(r[c] for r in runs) for c in columns}
        row["runs"] = len(runs)
        row["spread_speed"] = [round(r["speed"], 3) for r in runs]
        row["spread_first_p50"] = [round(r["first_p50"], 2) for r in runs]
        row["spread_calibration"] = [round(r["calibration"], 2) for r in runs]
        out[config] = row
        print(f"\n{config}  ({len(runs)} runs, {runs[0]['thoughts']} thoughts, real clock: {runs[0]['real_time']})")
        print(f"  mistakes     {row['wer']:.1f}% of words (human {row['wer_human']:.1f}, synthetic {row['wer_synthetic']:.1f})"
              + (f"; second pass {row['wer2']:.1f}% in {row['second_seconds']:.2f} s" if row["wer2"] == row["wer2"] else ""))
        print(f"  word shown   {row['first_p50']:.2f} s after it ended (9 in 10 within {row['first_p90']:.2f}); "
              f"last changed {row['stable_p50']:.2f} ({row['stable_p90']:.2f}); "
              f"settled {row['settled_p50']:.2f} ({row['settled_p90']:.2f}), never {row['settled_never']:.0f}%")
        print(f"  changes      {row['changed_per_100']:.1f} per 100 words shown; settled words changed: {row['settled_changed']}")
        print(f"  speed        {row['speed']:.3f} of real time {row['spread_speed']}, worst lag {row['worst_lag']:.2f} s, "
              f"peak {row['peak_mib']:.0f} MiB, load {row['load_seconds']:.2f} s, calibration {row['spread_calibration']}")
        print(f"  end          engine {row['end_engine_p50']:.2f} s after the last word (max {row['end_engine_max']:.2f}); "
              f"detector {row['end_vad_p50']:.2f} (max {row['end_vad_max']:.2f})")
    (folder / "summary.json").write_text(json.dumps(out, indent=1))


if __name__ == "__main__":
    main()
