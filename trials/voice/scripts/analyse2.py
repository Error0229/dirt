"""The figures of the second measurement, from data/results2.

    data/venv/Scripts/python scripts/analyse2.py            the tables
    data/venv/Scripts/python scripts/analyse2.py --texts NAME   what one model wrote

Mistakes are the word error rate: words wrong, missing or added, out of the
reference's words. Before counting, both texts go through the normaliser
OpenAI published with Whisper and that the Open ASR Leaderboard uses
(the `whisper-normalizer` package): lower case, no punctuation, numbers as
digits ("three" and "3", "twenty dollars" and "$20"), contractions and
British spellings made one, and "um", "uh", "hmm" dropped from both. Then
words that differ only in where the space is are made the same ("off
site", "offsite"; "q 3", "q3"; "p m", "pm"), so that spacing does not
decide a ranking.

Whether a difference between two models is more than chance is asked by
drawing the utterances again with replacement, group by group, ten thousand
times, and seeing how often the order holds.
"""
import json
import os
import random
import re
import statistics
import sys
from collections import defaultdict
from pathlib import Path

from whisper_normalizer.english import EnglishTextNormalizer

HERE = Path(__file__).resolve().parent.parent
RESULTS = Path(os.environ.get("RESULTS", HERE / "data" / "results2"))
GROUPS = ["clean", "other", "meeting", "calls", "thoughts"]
normalise = EnglishTextNormalizer()
SAME = {"ok": "okay", "alright": "all right"}


def plain(text):
    words = []
    for word in normalise(text).split():
        words += SAME.get(word, word).split()
    return words


def same_compounds(hyp, ref):
    """Writes "off site" and "offsite" the way the reference does."""
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


def errors(ref, hyp):
    """Substitutions, deletions and insertions of the cheapest alignment."""
    n, m = len(ref), len(hyp)
    row = list(range(m + 1))
    back = [[0] * (m + 1) for _ in range(n + 1)]
    for i in range(1, n + 1):
        new = [i] + [0] * m
        for j in range(1, m + 1):
            sub = row[j - 1] + (ref[i - 1] != hyp[j - 1])
            best = min(sub, row[j] + 1, new[j - 1] + 1)
            new[j] = best
            back[i][j] = 0 if best == sub else 1 if best == row[j] + 1 else 2
        row = new
    i, j, s, d, ins = n, m, 0, 0, 0
    while i or j:
        if i and j and back[i][j] == 0:
            s += ref[i - 1] != hyp[j - 1]
            i, j = i - 1, j - 1
        elif i and (not j or back[i][j] == 1):
            d, i = d + 1, i - 1
        else:
            ins, j = ins + 1, j - 1
    return s, d, ins


def manifest(path):
    rows = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.strip():
            fields = line.split("\t")
            rows[fields[0]] = (fields[2], plain(fields[3]), fields[3])
    return rows


def used_bytes(name, flags):
    """The size of the files a model was read from, from the listing taken
    before it was deleted."""
    listing = RESULTS / f"{name}.files.txt"
    if not listing.exists():
        return None
    files = {}
    for line in listing.read_text(errors="replace").splitlines():
        parts = line.split()
        if len(parts) >= 9 and parts[0].startswith("-") and parts[4].isdigit():
            files[parts[-1]] = int(parts[4])
    if flags.get("engine") == "whisper":
        return sum(files.values())
    fp32 = "fp32" in flags
    total = 0
    for file, size in files.items():
        if "test_wavs" in file or file.endswith((".md", ".sh", ".py", ".wav", "LICENSE")):
            continue
        is_model = file.endswith((".onnx", ".ort", ".weights", ".data"))
        if file.endswith("tokens.txt") or file.endswith(".ort"):
            total += size
        elif is_model and ((".int8." in file) != fp32):
            total += size
    return total


def load(name):
    rows = [json.loads(l) for l in (RESULTS / f"{name}.set.jsonl").read_text(encoding="utf-8").splitlines()]
    run = rows[0]
    reads = {r["id"]: r for r in rows if r["kind"] == "read"}
    return run, reads


def utterance_errors(reads, refs):
    """For every utterance: (mistakes, reference words)."""
    out = {}
    for uid, (_, ref, _) in refs.items():
        s, d, i = errors(ref, same_compounds(plain(reads[uid]["text"]), ref))
        out[uid] = (s + d + i, len(ref))
    return out


def timing_row(name):
    row = {}
    timing_file = RESULTS / f"{name}.timing.jsonl"
    if not timing_file.exists():
        return row
    for r in (json.loads(l) for l in timing_file.read_text(encoding="utf-8").splitlines()):
        if r["kind"] == "read":
            row[r["id"]] = statistics.median(r["read_seconds"])
            row[r["id"] + "_all"] = [round(x, 2) for x in r["read_seconds"]]
            row[r["id"] + "_pieces"] = len(r["pieces"])
            row[r["id"] + "_peak"] = r["peak_bytes"] / 2 ** 20
        if r["kind"] == "run":
            row["load"] = r["load_seconds"]
            row["calibration"] = r["calibration_seconds"]
        if r["kind"] == "end":
            row["peak"] = r["peak_bytes"] / 2 ** 20
    return row


def rates(per_utt, refs):
    counts = defaultdict(lambda: [0, 0])
    for uid, (group, _, _) in refs.items():
        e, n = per_utt[uid]
        counts[group][0] += e
        counts[group][1] += n
    row = {g: 100 * counts[g][0] / counts[g][1] for g in GROUPS if counts[g][1]}
    row["all"] = 100 * sum(c[0] for c in counts.values()) / sum(c[1] for c in counts.values())
    row["words"] = {g: counts[g][1] for g in GROUPS}
    return row


def print_table(title, table, order, refs):
    words = table[order[0]]["words"] if order else {}
    print(f"\n## {title}\n\n{len(refs)} utterances; words: "
          + ", ".join(f"{g} {words.get(g)}" for g in GROUPS) + f"; all {sum(words.values())}\n")
    head = ("| Model | Size, MiB | All | Clean | Other | Meetings | Calls | Thoughts | Capitals and punctuation"
            " | 5 s | 15 s | 60 s | Peak memory, MiB |")
    print(head)
    print("|" + "---|" * (head.count("|") - 1))
    for name in order:
        r = table[name]
        size = f"{r['bytes'] / 2 ** 20:.0f}" if r.get("bytes") else "?"
        writes = "Yes" if r["capitals"] > 0.5 and r["punctuation"] > 0.5 else "No"
        times = " | ".join(f"{r[c]:.2f} s" if c in r else "?" for c in ("t05", "t15", "t60"))
        peak = f"{r['peak']:.0f}" if "peak" in r else "?"
        print(f"| {name} | {size} | {r['all']:.1f}% | " + " | ".join(f"{r[g]:.1f}%" for g in GROUPS)
              + f" | {writes} | {times} | {peak} |")


def chance(order, per_utt, refs, table, extra_pairs):
    """Bootstrap over utterances, group by group: B's mistakes minus A's."""
    import numpy as np

    ids = list(refs)
    index = {u: k for k, u in enumerate(ids)}
    rng = np.random.default_rng(1)
    draws = []
    for group in GROUPS:
        members = np.array([index[u] for u in ids if refs[u][0] == group])
        if len(members):
            draws.append(members[rng.integers(0, len(members), size=(10000, len(members)))])
    draws = np.concatenate(draws, axis=1)

    def arrays(name):
        e = np.array([per_utt[name][u][0] for u in ids], dtype=float)
        n = np.array([per_utt[name][u][1] for u in ids], dtype=float)
        return e, n

    print("\nIs the difference more than chance? (B's mistakes minus A's, in points;"
          " the range 95 draws in 100 fell in)")
    pairs = [(order[i], order[j]) for i in range(len(order)) for j in range(i + 1, min(i + 4, len(order)))]
    pairs += [p for p in extra_pairs if p not in pairs]
    for a, b in pairs:
        if not all(n in per_utt and all(u in per_utt[n] for u in ids) for n in (a, b)):
            continue
        ea, n = arrays(a)
        eb, _ = arrays(b)
        words = n[draws].sum(axis=1)
        diffs = np.sort(100 * (eb[draws].sum(axis=1) - ea[draws].sum(axis=1)) / words)
        low, high = diffs[250], diffs[9749]
        wins = float((diffs > 0).mean())
        verdict = "more than chance" if low > 0 or high < 0 else "within chance"
        print(f"  {a} against {b}: {table[b]['all'] - table[a]['all']:+.2f} ({low:+.2f} to {high:+.2f}); "
              f"A ahead in {100 * wins:.0f}% of draws: {verdict}")


def main():
    refs = manifest(HERE / "data" / "set2" / "manifest.tsv")
    third_ids = set(manifest(HERE / "data" / "set2" / "manifest-third.tsv"))
    third = {u: v for u, v in refs.items() if u in third_ids}
    names = sorted(p.name[: -len(".set.jsonl")] for p in RESULTS.glob("*.set.jsonl"))
    if "--texts" in sys.argv:
        name = sys.argv[sys.argv.index("--texts") + 1]
        _, reads = load(name)
        for uid, (group, ref, raw) in refs.items():
            if uid not in reads:
                continue
            hyp = same_compounds(plain(reads[uid]["text"]), ref)
            s, d, i = errors(ref, hyp)
            if s + d + i:
                print(f"{uid} [{group}] {s + d + i}/{len(ref)}\n   said:  {raw}\n   wrote: {reads[uid]['text']}")
        return

    whole, part, per_utt, info = [], [], {}, {}
    for name in names:
        run, reads = load(name)
        if set(refs) <= set(reads):
            whole.append(name)
            use = refs
        elif len(third_ids & set(reads)) >= 0.9 * len(third_ids):
            # Read the third, or most of it before a run was cut.
            part.append(name)
            use = {u: v for u, v in third.items() if u in reads}
            if len(use) < len(third):
                print(f"{name}: {len(use)} of the third's {len(third)} read before it was cut")
        else:
            print(f"{name}: {len(reads)} of {len(refs)} read, left out")
            continue
        per_utt[name] = utterance_errors(reads, use)
        texts = [reads[u]["text"] for u in use]
        info[name] = timing_row(name)
        info[name]["capitals"] = sum(bool(re.search(r"[a-z]", t)) and bool(re.search(r"[A-Z]", t)) for t in texts) / len(texts)
        info[name]["punctuation"] = sum(bool(re.search(r"[.,?]", t)) for t in texts) / len(texts)
        info[name]["bytes"] = used_bytes(name, run["flags"])

    def make_table(models, use):
        table = {}
        for name in models:
            row = rates({u: per_utt[name][u] for u in use}, use)
            row.update(info[name])
            table[name] = row
        return table

    wanted = [tuple(a.split(",")) for a in sys.argv[1:] if "," in a]

    full = make_table(whole, refs)
    order = sorted(full, key=lambda n: full[n]["all"])
    print_table("The whole set", full, order, refs)
    chance(order, per_utt, refs, full, wanted)

    if part:
        # Every model on the utterances of the third that all of them read.
        common = {u: v for u, v in third.items() if all(u in per_utt[n] for n in part)}
        sub = make_table(whole + part, common)
        sub_order = sorted(sub, key=lambda n: sub[n]["all"])
        print_table(f"The fixed third (scripts/third.py), every model: {len(common)} of its {len(third)}"
                    " that every model read", sub, sub_order, common)
        chance(sub_order, {n: {u: per_utt[n][u] for u in common} for n in sub}, common, sub, wanted)
    else:
        sub = {}

    summary = {"whole": full, "third": sub}
    (RESULTS / "summary2.json").write_text(json.dumps(summary, indent=1))


if __name__ == "__main__":
    main()
