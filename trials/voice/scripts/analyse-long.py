"""The long recording (five minutes, scripts/shorten-long.py): what each
front-runner made of it.

    data/venv/Scripts/python scripts/analyse-long.py

For every data/results2/long/*.jsonl: the time and memory it took, its
mistakes, and whether text was lost or repeated: every run of three or more
words in a row that are missing from what was written, or were written and
never said, is printed with where it is. Where the recording was cut at
pauses, mistakes within two words of a join are counted against mistakes
anywhere else, to see whether the joins cost anything.
"""
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from analyse2 import HERE, plain, same_compounds  # noqa: E402

LONG = HERE / "data" / "long"
RESULTS = HERE / "data" / "results2" / "long"


def align(ref, hyp):
    """The cheapest alignment as a list of (kind, ref index, hyp index):
    kind is "same", "sub", "del" (said, not written) or "ins" (written, not said)."""
    n, m = len(ref), len(hyp)
    cost = [[0] * (m + 1) for _ in range(n + 1)]
    for i in range(n + 1):
        cost[i][0] = i
    for j in range(m + 1):
        cost[0][j] = j
    for i in range(1, n + 1):
        ri, row, above = ref[i - 1], cost[i], cost[i - 1]
        for j in range(1, m + 1):
            row[j] = min(above[j - 1] + (ri != hyp[j - 1]), above[j] + 1, row[j - 1] + 1)
    steps, i, j = [], n, m
    while i or j:
        if i and j and cost[i][j] == cost[i - 1][j - 1] + (ref[i - 1] != hyp[j - 1]):
            steps.append(("same" if ref[i - 1] == hyp[j - 1] else "sub", i - 1, j - 1))
            i, j = i - 1, j - 1
        elif i and cost[i][j] == cost[i - 1][j] + 1:
            steps.append(("del", i - 1, j))
            i -= 1
        else:
            steps.append(("ins", i, j - 1))
            j -= 1
    return steps[::-1]


def main():
    ref_line = (LONG / "manifest.tsv").read_text(encoding="utf-8").split("\t")
    ref = plain(ref_line[3])
    print(f"the recording: {len(ref)} words after normalising\n")
    for path in sorted(RESULTS.glob("*.jsonl")):
        rows = [json.loads(l) for l in path.read_text(encoding="utf-8").splitlines()]
        read = next(r for r in rows if r["kind"] == "read")
        end = next(r for r in rows if r["kind"] == "end")
        # Words of each piece, so that the joins are known in the written text.
        joins, hyp = [], []
        for text in read["piece_texts"]:
            hyp += plain(text)
            joins.append(len(hyp))
        joins = joins[:-1]
        hyp_fixed = same_compounds(hyp, ref)
        exact = len(hyp_fixed) == len(hyp)
        steps = align(ref, hyp_fixed)
        wrong = [s for s in steps if s[0] != "same"]
        times = ", ".join(f"{t:.1f}" for t in read["read_seconds"])
        print(f"{path.stem}: {read['audio_seconds']:.0f} s read in {times} s "
              f"({len(read['pieces'])} piece(s), longest {max(b - a for a, b in read['pieces']):.1f} s, "
              f"finding the pauses took {read['cut_seconds']:.1f} s), peak {end['peak_bytes'] / 2 ** 20:.0f} MiB")
        median = sorted(read["read_seconds"])[len(read["read_seconds"]) // 2]
        per_minute = median / (read["audio_seconds"] / 60)
        print(f"   per minute of sound: {per_minute:.1f} s (median); ten minutes would take about "
              f"{10 * per_minute:.0f} s at this rate")
        print(f"   mistakes {100 * len(wrong) / len(ref):.2f}% ({len(wrong)} of {len(ref)})")
        if joins and exact:
            near = sum(1 for kind, _, j in wrong if any(abs(j - join) <= 2 for join in joins))
            words_near = min(len(hyp), 5 * len(joins))
            elsewhere = len(wrong) - near
            print(f"   within two words of the {len(joins)} joins: {near} mistakes in about {words_near} words "
                  f"({100 * near / max(1, words_near):.1f}%); elsewhere {elsewhere} in {len(ref) - words_near} "
                  f"({100 * elsewhere / max(1, len(ref) - words_near):.1f}%)")
        # Runs of words lost or written without being said.
        run, runs = [], []
        for step in steps + [("same", 0, 0)]:
            if step[0] in ("del", "ins") and (not run or run[-1][0] == step[0]):
                run.append(step)
            else:
                if len(run) >= 3:
                    runs.append(run)
                run = [step] if step[0] in ("del", "ins") else []
        if not runs:
            print("   no run of three or more words lost or added")
        for run in runs:
            kind = run[0][0]
            if kind == "del":
                words = " ".join(ref[s[1]] for s in run)
                print(f"   LOST {len(run)} words at word {run[0][1]} of {len(ref)}: {words[:160]}")
            else:
                words = " ".join(hyp_fixed[s[2]] for s in run)
                print(f"   ADDED {len(run)} words at word {run[0][1]} of {len(ref)}: {words[:160]}")
        print()


if __name__ == "__main__":
    main()
