"""The timings of the second measurement side by side: the first one (taken
while two model queues and other work shared the laptop) and the second
(data/results2/retime, one model at a time, laptop otherwise quiet).

    data/venv/Scripts/python scripts/retime.py

"Calibration" is the time the program's fixed piece of arithmetic took just
before the model was timed: the lower, the faster the laptop was then.
"""
import json
import statistics
from pathlib import Path

HERE = Path(__file__).resolve().parent.parent
RESULTS = HERE / "data" / "results2"


def timing(path):
    out = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        r = json.loads(line)
        if r["kind"] == "read":
            out[r["id"]] = statistics.median(r["read_seconds"])
            out[r["id"] + "_peak"] = r["peak_bytes"] / 2 ** 20
        elif r["kind"] == "run":
            out["calibration"] = r["calibration_seconds"]
            out["load"] = r["load_seconds"]
        elif r["kind"] == "end":
            out["peak"] = r["peak_bytes"] / 2 ** 20
    return out


def cell(t, key, unit=" s", digits=2):
    return f"{t[key]:.{digits}f}{unit}" if t and key in t else "-"


def main():
    print("| Model | First: calibration | 5 s | 15 s | 60 s | Again: calibration | 5 s | 15 s | 60 s | Load | Peak memory, MiB |")
    print("|---|---|---|---|---|---|---|---|---|---|---|")
    for again_path in sorted((RESULTS / "retime").glob("*.timing.jsonl")):
        name = again_path.name[: -len(".timing.jsonl")]
        first_path = RESULTS / f"{name}.timing.jsonl"
        first = timing(first_path) if first_path.exists() else None
        again = timing(again_path)
        print(f"| {name} | {cell(first, 'calibration', '', 3)} | {cell(first, 't05')} | {cell(first, 't15')} | "
              f"{cell(first, 't60')} | {cell(again, 'calibration', '', 3)} | {cell(again, 't05')} | "
              f"{cell(again, 't15')} | {cell(again, 't60')} | {cell(again, 'load', ' s', 1)} | "
              f"{cell(again, 'peak', '', 0)} |")


if __name__ == "__main__":
    main()
