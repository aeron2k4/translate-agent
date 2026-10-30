"""python -m xt_admin.review <queue.json> ingest|list

- ingest: đọc đề xuất (JSONL, do translator xuất) từ stdin vào hàng chờ.
- list: in id + lĩnh vực + cặp thuật ngữ của các đề xuất đang chờ.
"""

from __future__ import annotations

import json
import sys

from .queue import ReviewError, ReviewQueue, Suggestion

_FIELDS = (
    "domain",
    "source_lang",
    "target_lang",
    "source_term",
    "target_term",
    "proposer",
    "example",
)


def main(argv: list[str]) -> int:
    if len(argv) != 2 or argv[1] not in {"ingest", "list"}:
        print(__doc__, file=sys.stderr)
        return 2
    queue = ReviewQueue(argv[0], owners={})
    if argv[1] == "list":
        for s in queue.pending():
            print(f"{s.id}\t{s.domain}\t{s.source_term} => {s.target_term}")
        return 0
    rejected = 0
    for line in sys.stdin:
        if not line.strip():
            continue
        raw = json.loads(line)
        try:
            queue.submit(Suggestion(**{k: raw.get(k) for k in _FIELDS}))
        except (ReviewError, TypeError):
            rejected += 1
    if rejected:
        print(f"bỏ qua {rejected} đề xuất không hợp lệ", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
