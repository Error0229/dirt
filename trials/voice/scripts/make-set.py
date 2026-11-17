"""Builds the trial's test set in data/set.

Human half: ten utterances from LibriSpeech test-clean (CC BY 4.0), one
speaker each, 4 to 15 seconds, picked by a fixed rule and never by ear. The
moment each word ends comes from the Montreal Forced Aligner alignments of
LibriSpeech published by Lugosch et al. (zenodo.org/records/2619474, CC BY
4.0); only the ten files needed are read out of that archive.

Synthetic half: the twelve thoughts scripts/make-speech.ps1 spoke. The
synthesiser reports when each word starts; a word's end is taken as the
last moment before the next word at which the sound is louder than quiet.

Also makes the pause set (data/pauses): thoughts with a pause put in the
middle, clean and under noise, for the question of whether 1.6 seconds of
quiet after speech can be told from a pause inside a thought.

    python -I scripts/make-set.py data/librispeech/LibriSpeech/test-clean

Needs ffmpeg on the path and numpy. Nothing here is committed.
"""
import io
import json
import re
import subprocess
import sys
import urllib.request
import wave
import zipfile
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent.parent
SET = HERE / "data" / "set"
PAUSES = HERE / "data" / "pauses"
ALIGNMENTS = "https://zenodo.org/records/2619474/files/librispeech_alignments.zip?download=1"
RATE = 16000


class HttpFile(io.RawIOBase):
    """A file on a web server, read a range at a time."""

    def __init__(self, url):
        self.url, self.pos = url, 0
        head = urllib.request.Request(url, method="HEAD", headers={"User-Agent": "voice-trial"})
        self.size = int(urllib.request.urlopen(head).headers["Content-Length"])

    def seekable(self):
        return True

    def readable(self):
        return True

    def tell(self):
        return self.pos

    def seek(self, offset, whence=0):
        self.pos = offset if whence == 0 else self.pos + offset if whence == 1 else self.size + offset
        return self.pos

    def readinto(self, buffer):
        if self.pos >= self.size or not len(buffer):
            return 0
        end = min(self.pos + len(buffer), self.size) - 1
        request = urllib.request.Request(
            self.url, headers={"User-Agent": "voice-trial", "Range": f"bytes={self.pos}-{end}"})
        data = urllib.request.urlopen(request).read()
        buffer[: len(data)] = data
        self.pos += len(data)
        return len(data)


def read_wav(path):
    with wave.open(str(path)) as w:
        assert w.getframerate() == RATE and w.getnchannels() == 1 and w.getsampwidth() == 2, path
        return np.frombuffer(w.readframes(w.getnframes()), dtype=np.int16).astype(np.float32) / 32768


def write_wav(path, samples):
    data = (np.clip(samples, -1, 1) * 32767).astype(np.int16)
    with wave.open(str(path), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(data.tobytes())


def loudness(samples, frame=160):
    """RMS of every 10 ms, in dB below full scale."""
    n = len(samples) // frame
    rms = np.sqrt((samples[: n * frame].reshape(n, frame) ** 2).mean(axis=1) + 1e-12)
    return 20 * np.log10(rms)


def pick_librispeech(root, count=10):
    """One utterance from each of the first `count` speakers, by number: the
    first in the speaker's first chapter that is 4 to 15 seconds long."""
    picked = []
    for speaker in sorted(root.iterdir(), key=lambda p: int(p.name)):
        chapter = sorted(speaker.iterdir(), key=lambda p: int(p.name))[0]
        texts = {}
        for line in (chapter / f"{speaker.name}-{chapter.name}.trans.txt").read_text().splitlines():
            key, text = line.split(" ", 1)
            texts[key] = text
        for flac in sorted(chapter.glob("*.flac")):
            seconds = float(subprocess.check_output(
                ["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", str(flac)]))
            if 4.0 <= seconds <= 15.0:
                picked.append((flac, texts[flac.stem]))
                break
        if len(picked) == count:
            break
    return picked


def textgrid_words(text):
    """The intervals of a TextGrid's "words" tier, silences left out."""
    tier = text.split('name = "words"', 1)[1].split('name = "phones"', 1)[0]
    found = re.findall(r'xmin = ([\d.]+)\s+xmax = ([\d.]+)\s+text = "([^"]*)"', tier)
    return [{"word": w.lower(), "start": float(a), "end": float(b)} for a, b, w in found if w.strip()]


def human_half():
    root = Path(sys.argv[1])
    picked = pick_librispeech(root)
    archive = zipfile.ZipFile(io.BufferedReader(HttpFile(ALIGNMENTS), buffer_size=1 << 20))
    lines = []
    for flac, text in picked:
        speaker, chapter, _ = flac.stem.split("-")
        wav = SET / f"libri-{flac.stem}.wav"
        subprocess.check_call(["ffmpeg", "-v", "error", "-y", "-i", str(flac), "-ar", str(RATE), "-ac", "1",
                               "-c:a", "pcm_s16le", str(wav)])
        grid = archive.read(f"test-clean/{speaker}/{chapter}/{flac.stem}.TextGrid").decode()
        words = textgrid_words(grid)
        info = {"id": wav.stem, "kind": "human", "source": f"LibriSpeech test-clean {flac.stem}",
                "text": text.lower(), "words": words}
        (SET / f"{wav.stem}.words.json").write_text(json.dumps(info, indent=1))
        lines.append(f"{wav.stem}\t{wav.name}\thuman\t{text.lower()}")
        print(wav.stem, f"{len(read_wav(wav)) / RATE:.1f} s", len(words), "words")
    return lines


def synthetic_half():
    lines = []
    for path in sorted(SET.glob("synth-*.synth.json")):
        info = json.loads(path.read_text(encoding="utf-8-sig"))
        samples = read_wav(SET / f"{info['id']}.wav")
        level = loudness(samples)
        quiet = level.max() - 35
        starts = sorted(info["words"], key=lambda w: w["start"])
        words = []
        for i, word in enumerate(starts):
            first = int(word["start"] * 100)
            last = int(starts[i + 1]["start"] * 100) if i + 1 < len(starts) else len(level)
            loud = np.nonzero(level[first:last] > quiet)[0]
            end = (first + loud[-1] + 1) / 100 if len(loud) else last / 100
            words.append({"word": word["word"].lower(), "start": word["start"], "end": round(end, 3)})
        out = {"id": info["id"], "kind": "synthetic", "source": info["voice"], "text": info["text"],
               "words": words}
        (SET / f"{info['id']}.words.json").write_text(json.dumps(out, indent=1))
        lines.append(f"{info['id']}\t{info['id']}.wav\tsynthetic\t{info['text']}")
        print(info["id"], f"{len(samples) / RATE:.1f} s", len(words), "words")
    return lines


def pink_noise(n, rng):
    """Noise with more low than high, as a room has."""
    spectrum = np.fft.rfft(rng.standard_normal(n))
    spectrum /= np.sqrt(np.arange(len(spectrum)) + 1)
    noise = np.fft.irfft(spectrum, n)
    return noise / np.sqrt((noise ** 2).mean())


def pause_set():
    """Each of four thoughts cut at a word boundary near its middle, with a
    pause of 0.6, 1.0 or 1.4 seconds put there; then the same under pink
    noise 20 dB and 10 dB below the speech. The pause is room quiet, not
    zeros. Written with where the speech really stops, for the analysis."""
    PAUSES.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(1)
    lines = []
    for source in ["synth-05", "synth-08", "libri-1089-134686-0000", "libri-121-121726-0000"]:
        info_path = SET / f"{source}.words.json"
        if not info_path.exists():
            continue
        info = json.loads(info_path.read_text())
        samples = read_wav(SET / f"{source}.wav")
        words = info["words"]
        middle = words[len(words) // 2 - 1]
        cut = int((middle["end"] + words[len(words) // 2]["start"]) / 2 * RATE)
        speech_power = (samples ** 2).mean()
        for pause in [0.6, 1.0, 1.4]:
            gap = rng.standard_normal(int(pause * RATE)).astype(np.float32) * 0.0002
            joined = np.concatenate([samples[:cut], gap, samples[cut:]])
            for snr in [None, 20, 10]:
                out = joined
                if snr is not None:
                    noise = pink_noise(len(joined) + 3 * RATE, rng) * np.sqrt(speech_power / 10 ** (snr / 10))
                    out = joined + noise[: len(joined)].astype(np.float32)
                name = f"{source}-pause{int(pause * 10):02d}-{'clean' if snr is None else f'snr{snr}'}"
                write_wav(PAUSES / f"{name}.wav", out)
                meta = {"id": name, "source": source, "pause_seconds": pause, "snr_db": snr,
                        "pause_starts": middle["end"], "pause_ends": words[len(words) // 2]["start"] + pause,
                        "speech_ends": words[-1]["end"] + pause, "text": info["text"]}
                (PAUSES / f"{name}.json").write_text(json.dumps(meta, indent=1))
                lines.append(f"{name}\t{name}.wav")
    (PAUSES / "manifest.tsv").write_text("\n".join(lines) + "\n")
    print(len(lines), "pause files")


if __name__ == "__main__":
    SET.mkdir(parents=True, exist_ok=True)
    lines = human_half() + synthetic_half()
    (SET / "manifest.tsv").write_text("\n".join(lines) + "\n", encoding="utf-8")
    pause_set()
