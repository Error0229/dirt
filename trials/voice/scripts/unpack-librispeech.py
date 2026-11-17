"""Lays LibriSpeech's two test sets out as openslr.org's archives do, from
the copy on Hugging Face (huggingface.co/datasets/openslr/librispeech_asr,
CC BY 4.0), which was many times quicker to fetch on the day:

    data/librispeech/LibriSpeech/test-clean/<speaker>/<chapter>/<id>.flac
    data/librispeech/LibriSpeech/test-clean/<speaker>/<chapter>/<speaker>-<chapter>.trans.txt

    python scripts/unpack-librispeech.py clean.parquet test-clean
    python scripts/unpack-librispeech.py other.parquet test-other

Needs pyarrow (scripts/analyse2.py's environment has it).
"""
import sys
from collections import defaultdict
from pathlib import Path

import pyarrow.parquet as pq

HERE = Path(__file__).resolve().parent.parent
source, name = sys.argv[1], sys.argv[2]
root = HERE / "data" / "librispeech" / "LibriSpeech" / name
texts = defaultdict(list)
count = 0
table = pq.ParquetFile(source)
for batch in table.iter_batches(batch_size=64):
    for row in batch.to_pylist():
        speaker, chapter, _ = row["id"].split("-")
        folder = root / speaker / chapter
        folder.mkdir(parents=True, exist_ok=True)
        (folder / f"{row['id']}.flac").write_bytes(row["audio"]["bytes"])
        texts[folder].append(f"{row['id']} {row['text']}")
        count += 1
for folder, lines in texts.items():
    speaker, chapter = folder.parts[-2:]
    (folder / f"{speaker}-{chapter}.trans.txt").write_text("\n".join(sorted(lines)) + "\n")
print(name, count, "utterances from", len({f.parts[-2] for f in texts}), "speakers")
