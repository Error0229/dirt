"""What is left of a test set after a reading was cut off.

    python -I scripts/remaining.py PART MANIFEST LEFT

PART is the `.set.jsonl.part` a cut-off reading left. Its last line may be
half written: it is dropped, and PART is written again with only whole
lines. LEFT is written as a manifest of the recordings of MANIFEST that PART
does not have, in the same folder as MANIFEST so that its file names still
point at the recordings. Prints how many are left.
"""

import json
import sys
from pathlib import Path


def main():
    part, manifest, left = (Path(a) for a in sys.argv[1:4])
    whole = []
    for line in part.read_text(encoding="utf-8").splitlines():
        try:
            whole.append(json.loads(line))
        except json.JSONDecodeError:
            break
    part.write_text("".join(json.dumps(r) + "\n" for r in whole), encoding="utf-8")
    done = {r["id"] for r in whole if r.get("kind") == "read"}
    rows = [l for l in manifest.read_text(encoding="utf-8").splitlines() if l.strip()]
    rest = [l for l in rows if l.split("\t")[0] not in done]
    left.write_text("".join(l + "\n" for l in rest), encoding="utf-8")
    print(len(rest))


if __name__ == "__main__":
    main()
