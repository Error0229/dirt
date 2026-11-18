"""The long recording cut down to `seconds` (five minutes by default), at the
end of the last utterance that fits, with its text and marks to match.

    python -I scripts/shorten-long.py data/long10 data/long [300]

The source is what scripts/make-set2.py wrote to data/long. The trial used
five minutes instead of ten to save time: what it tests is what happens at
the joins once a recording is longer than a model's window, and five
minutes already has dozens of them. The design's ten-minute limit is
unchanged; time and memory are also given per minute of sound.
"""
import json
import sys
import wave
from pathlib import Path

LIBRI = Path(__file__).resolve().parent.parent / "data" / "librispeech" / "LibriSpeech" / "test-clean"


def text(uid):
    speaker, chapter, _ = uid.split("-")
    for line in (LIBRI / speaker / chapter / f"{speaker}-{chapter}.trans.txt").read_text().splitlines():
        if line.startswith(uid + " "):
            return line.split(" ", 1)[1].lower()
    raise KeyError(uid)


def main():
    source, target = Path(sys.argv[1]), Path(sys.argv[2])
    seconds = float(sys.argv[3]) if len(sys.argv) > 3 else 300.0
    marks = json.loads((source / "marks.json").read_text())
    whole = (source / "manifest.tsv").read_text(encoding="utf-8").split("\t")[3].strip()
    assert " ".join(text(m["id"]) for m in marks) == whole, "the marks do not match the text"
    kept = [m for m in marks if m["end"] <= seconds]
    with wave.open(str(source / "long.wav")) as w:
        rate, params = w.getframerate(), w.getparams()
        # Up to the end of the last utterance and the 0.4 s of quiet after it.
        frames = w.readframes(int(min(kept[-1]["end"] + 0.4, seconds) * rate))
    target.mkdir(parents=True, exist_ok=True)
    with wave.open(str(target / "long.wav"), "wb") as w:
        w.setparams(params)
        w.writeframes(frames)
    words = " ".join(text(m["id"]) for m in kept)
    (target / "manifest.tsv").write_text(f"long\tlong.wav\tlong\t{words}\n", encoding="utf-8")
    (target / "marks.json").write_text(json.dumps(kept, indent=1))
    print(f"{len(frames) / 2 / rate:.1f} s, {len(kept)} utterances, {len(words.split())} words")


if __name__ == "__main__":
    main()
