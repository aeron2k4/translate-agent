"""python -m xt_admin.glossary validate <file.json>..."""

from __future__ import annotations

import sys

from .model import GlossaryError, load, validate


def main(argv: list[str]) -> int:
    if len(argv) < 2 or argv[0] != "validate":
        print("dùng: python -m xt_admin.glossary validate <file.json>...", file=sys.stderr)
        return 2
    failed = False
    for path in argv[1:]:
        try:
            errors = validate(load(path))
        except (GlossaryError, KeyError, ValueError) as e:
            errors = [str(e)]
        for err in errors:
            print(f"{path}: {err}", file=sys.stderr)
        failed |= bool(errors)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
