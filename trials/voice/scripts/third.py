"""A fixed third of the test set, for the models too slow to read it all.

    python -I scripts/third.py data/set2/manifest.tsv data/set2/manifest-third.tsv

Keeps the first of every three recordings of each group, in the order of
the manifest, so that every group keeps a third of its recordings and the
choice does not depend on what any model wrote.
"""

import sys
from collections import Counter
from pathlib import Path


def main():
    source, target = Path(sys.argv[1]), Path(sys.argv[2])
    seen = Counter()
    kept = []
    for line in source.read_text(encoding="utf-8").splitlines():
        if not line.strip():
            continue
        group = line.split("\t")[2]
        if seen[group] % 3 == 0:
            kept.append(line)
        seen[group] += 1
    target.write_text("".join(l + "\n" for l in kept), encoding="utf-8")
    print(f"{len(kept)} of {sum(seen.values())}")


if __name__ == "__main__":
    main()
