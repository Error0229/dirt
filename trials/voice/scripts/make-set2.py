"""Builds the test set of the second measurement in data/set2, the three
clips that are timed in data/timing, and the ten-minute recording in
data/long.

Five groups, each picked by a fixed rule and never by ear:

  clean      60 utterances of LibriSpeech test-clean (CC BY 4.0): read
             audiobooks, clear voices.
  other      60 of LibriSpeech test-other (CC BY 4.0): the same kind of
             reading by voices and in recordings that recognisers found
             harder.
  meeting    40 of the AMI Meeting Corpus's test split, headset
             microphones (CC BY 4.0): people talking in meetings, unscripted,
             many of them not native speakers. From the copy at
             huggingface.co/datasets/edinburghcstr/ami.
  calls      40 of Earnings-22's test split (CC BY-SA 4.0): company results
             calls with speakers from many countries, telephone sound,
             unscripted answers. From huggingface.co/datasets/distil-whisper/earnings22.
  thoughts   The synthetic thoughts scripts/make-speech.ps1 spoke (run it
             into data/set2 first).

The last two are read a row at a time from Hugging Face's dataset viewer, so
that only the utterances used are fetched.

    python -I scripts/make-set2.py data/librispeech/LibriSpeech

Needs ffmpeg on the path and numpy. Nothing here is committed.
"""
import json
import subprocess
import sys
import time
import urllib.parse
import urllib.request
import wave
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent.parent
SET = HERE / "data" / "set2"
TIMING = HERE / "data" / "timing"
LONG = HERE / "data" / "long"
RATE = 16000
ROWS = "https://datasets-server.huggingface.co/rows"


def seconds_of(path):
    return float(subprocess.check_output(
        ["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", str(path)]))


def to_wav(source, wav):
    subprocess.check_call(["ffmpeg", "-nostdin", "-v", "error", "-y", "-i", str(source), "-ar", str(RATE), "-ac", "1",
                           "-c:a", "pcm_s16le", str(wav)], stdin=subprocess.DEVNULL)


def read_wav(path):
    with wave.open(str(path)) as w:
        assert w.getframerate() == RATE and w.getnchannels() == 1 and w.getsampwidth() == 2, path
        return np.frombuffer(w.readframes(w.getnframes()), dtype=np.int16)


def write_wav(path, samples):
    with wave.open(str(path), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(np.asarray(samples, dtype=np.int16).tobytes())


def chapter_texts(chapter):
    texts = {}
    for trans in chapter.glob("*.trans.txt"):
        for line in trans.read_text().splitlines():
            key, text = line.split(" ", 1)
            texts[key] = text.lower()
    return texts


def librispeech(root, group, count):
    """From every speaker's first chapter the first utterance of 4 to 20 s;
    then, going round the speakers again, the last such; until `count`."""
    speakers = sorted(root.iterdir(), key=lambda p: int(p.name))
    fitting = {}
    for speaker in speakers:
        chapter = sorted(speaker.iterdir(), key=lambda p: int(p.name))[0]
        texts = chapter_texts(chapter)
        flacs = sorted(chapter.glob("*.flac"))
        first = next((f for f in flacs if 4.0 <= seconds_of(f) <= 20.0), None)
        last = next((f for f in reversed(flacs) if 4.0 <= seconds_of(f) <= 20.0), None)
        fitting[speaker.name] = [f for f in dict.fromkeys([first, last]) if f], texts
    lines = []
    for turn in range(2):
        for speaker in speakers:
            flacs, texts = fitting[speaker.name]
            if turn < len(flacs) and len(lines) < count:
                flac = flacs[turn]
                wav = SET / f"{group}-{flac.stem}.wav"
                to_wav(flac, wav)
                lines.append(f"{wav.stem}\t{wav.name}\t{group}\t{texts[flac.stem]}")
    print(group, len(lines), "utterances")
    return lines


def rows(dataset, config, offset, length=100):
    query = urllib.parse.urlencode({"dataset": dataset, "config": config, "split": "test",
                                    "offset": offset, "length": length})
    for attempt in range(6):
        try:
            request = urllib.request.Request(f"{ROWS}?{query}", headers={"User-Agent": "voice-trial"})
            return json.load(urllib.request.urlopen(request, timeout=120))["rows"]
        except Exception as error:  # the viewer is sometimes busy
            print("  again:", error)
            time.sleep(5 * (attempt + 1))
    return []


def from_viewer(group, dataset, config, total, text_of, who_of, length_of, count, per_speaker=2):
    """Walks the split in `count` evenly spaced places and takes, at each,
    the first utterance of 4 to 15 s and eight words or more from a speaker
    (or call) who has not yet given `per_speaker`."""
    lines, taken = [], {}
    stride = total // (count + 8)
    offset = 0
    while len(lines) < count and offset < total:
        for item in rows(dataset, config, offset, 60):
            row = item["row"]
            text, who = text_of(row).strip(), who_of(row)
            if not (4.0 <= length_of(row) <= 15.0) or len(text.split()) < 8 or taken.get(who, 0) >= per_speaker:
                continue
            raw = SET / f"{group}-raw.bin"
            request = urllib.request.Request(row["audio"][0]["src"], headers={"User-Agent": "voice-trial"})
            try:
                raw.write_bytes(urllib.request.urlopen(request, timeout=120).read())
            except Exception as error:
                print("  skipped:", error)
                continue
            wav = SET / f"{group}-{len(lines) + 1:02d}.wav"
            to_wav(raw, wav)
            raw.unlink()
            taken[who] = taken.get(who, 0) + 1
            lines.append(f"{wav.stem}\t{wav.name}\t{group}\t{text}")
            break
        offset += stride
    print(group, len(lines), "utterances from", len(taken), "speakers or calls")
    return lines


def thoughts():
    lines = []
    for path in sorted(SET.glob("synth-*.synth.json")):
        info = json.loads(path.read_text(encoding="utf-8-sig"))
        lines.append(f"{info['id']}\t{info['id']}.wav\tthoughts\t{info['text']}")
    print("thoughts", len(lines), "utterances")
    return lines


def one_reader(root, want_seconds):
    """Consecutive utterances of LibriSpeech test-clean, speaker after
    speaker from the last by number, with 0.4 s of quiet between them, as
    long as the whole stays under `want_seconds`."""
    rng = np.random.default_rng(3)
    pieces, texts, marks, total = [], [], [], 0
    for speaker in sorted(root.iterdir(), key=lambda p: -int(p.name)):
        for chapter in sorted(speaker.iterdir(), key=lambda p: int(p.name)):
            words = chapter_texts(chapter)
            for flac in sorted(chapter.glob("*.flac")):
                wav = LONG / "piece.wav"
                to_wav(flac, wav)
                samples = read_wav(wav)
                wav.unlink()
                gap = (rng.standard_normal(int(0.4 * RATE)) * 6).astype(np.int16)
                if total + len(samples) + len(gap) > want_seconds * RATE:
                    return np.concatenate(pieces), texts, marks
                marks.append({"id": flac.stem, "start": total / RATE, "end": (total + len(samples)) / RATE})
                pieces += [samples, gap]
                texts.append(words[flac.stem])
                total += len(samples) + len(gap)
    return np.concatenate(pieces), texts, marks


if __name__ == "__main__":
    libri = Path(sys.argv[1])
    for folder in (SET, TIMING, LONG):
        folder.mkdir(parents=True, exist_ok=True)

    lines = librispeech(libri / "test-clean", "clean", 60) + librispeech(libri / "test-other", "other", 60)
    lines += from_viewer("meeting", "edinburghcstr/ami", "ihm", 12643,
                         lambda r: r["text"], lambda r: r["speaker_id"],
                         lambda r: r["end_time"] - r["begin_time"], 40)
    lines += from_viewer("calls", "distil-whisper/earnings22", "chunked", 57391,
                         lambda r: r["transcription"], lambda r: r["file_id"],
                         lambda r: r["end_ts"] - r["start_ts"], 40)
    lines += thoughts()
    (SET / "manifest.tsv").write_text("\n".join(lines) + "\n", encoding="utf-8")

    # The clips that are timed: 5, 15 and 60 seconds of one reader.
    samples, _, _ = one_reader(libri / "test-clean", 90)
    timing = []
    for seconds in (5, 15, 60):
        write_wav(TIMING / f"t{seconds:02d}.wav", samples[: seconds * RATE])
        timing.append(f"t{seconds:02d}\tt{seconds:02d}.wav")
    (TIMING / "manifest.tsv").write_text("\n".join(timing) + "\n")

    # The longest recording the design allows: ten minutes.
    samples, texts, marks = one_reader(libri / "test-clean", 600)
    write_wav(LONG / "long.wav", samples)
    (LONG / "manifest.tsv").write_text("long\tlong.wav\tlong\t" + " ".join(texts) + "\n")
    (LONG / "marks.json").write_text(json.dumps(marks, indent=1))
    print(f"long: {len(samples) / RATE:.0f} s, {len(marks)} utterances, {sum(len(t.split()) for t in texts)} words")
