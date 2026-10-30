"""Sinh golden dataset từ mẫu câu song ngữ viết tay (`eval/golden/*.json`).

Bản tham chiếu và dạng đúng của thực thể trong ngôn ngữ đích được viết tay trong
file mẫu, KHÔNG suy ra từ code Rust — để golden là chuẩn độc lập. Trình sinh chỉ:

- điền slot theo vòng (không ngẫu nhiên) để mọi giá trị slot đều được dùng;
- xoay vòng cặp xưng hô anh/chị/em;
- thêm biến thể tiếng Việt không dấu và kiểu chat (viết tắt + không dấu);
- suy ra thuật ngữ glossary phải có, và báo lỗi nếu bản tham chiếu không dùng đúng.

python -m xt_eval.golden          # ghi lại eval/datasets/golden.*.jsonl
python -m xt_eval.golden --check  # báo lỗi nếu file đã commit lệch với mẫu
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import unicodedata
from collections import defaultdict
from pathlib import Path

from xt_admin.glossary import Glossary, load

from .cli import REPO

GOLDEN_DIR = REPO / "eval" / "golden"
DATASET_DIR = REPO / "eval" / "datasets"
GLOSSARY_DIR = REPO / "glossary" / "data"

SLOT_RE = re.compile(r"\{(\w+)\}")
ADDRESS_SLOTS = {"Addr", "addr", "Self", "self"}
INSTANCES_PER_TEMPLATE = {"vi": 2, "en": 3}


class GoldenError(ValueError):
    pass


def fold(text: str) -> str:
    """Bỏ dấu tiếng Việt, giữ hoa/thường (như người gõ không dấu)."""
    text = text.replace("đ", "d").replace("Đ", "D")
    stripped = "".join(
        c for c in unicodedata.normalize("NFD", text) if unicodedata.category(c) != "Mn"
    )
    return unicodedata.normalize("NFC", stripped)


def abbreviate(text: str, table: dict[str, str]) -> str:
    def sub(m: re.Match[str]) -> str:
        word = m.group(0)
        short = table[word.lower()]
        return short.capitalize() if word[0].isupper() else short

    pattern = r"\b(" + "|".join(map(re.escape, table)) + r")\b"
    return re.sub(pattern, sub, text, flags=re.IGNORECASE)


def _words(text: str) -> list[str]:
    return re.findall(r"\w+", fold(text).lower())


def contains_phrase(text: str, phrase: str) -> bool:
    hay, needle = _words(text), _words(phrase)
    return bool(needle) and any(
        hay[i : i + len(needle)] == needle for i in range(len(hay) - len(needle) + 1)
    )


def _fill(template: str, values: dict[str, dict], lang: str, pair: tuple[str, str] | None) -> str:
    def sub(m: re.Match[str]) -> str:
        name = m.group(1)
        if name in ADDRESS_SLOTS:
            if pair is None:
                raise GoldenError(f"thiếu cặp xưng hô cho {{{name}}}")
            word = pair[0] if name.lower() == "addr" else pair[1]
            return word.capitalize() if name[0].isupper() else word
        return values[name][lang]

    return SLOT_RE.sub(sub, template)


def _glossary_expectations(g: Glossary, text: str, src: str, tgt: str) -> list[tuple[str, str]]:
    return [(t.id, t.forms[tgt]) for t in g.terms if contains_phrase(text, t.forms[src])]


def generate(spec: dict, glossary: Glossary) -> list[dict]:
    slots: dict[str, list[dict]] = spec["slots"]
    pairs = [tuple(p) for p in spec["address_pairs"]]
    cursor: dict[str, int] = defaultdict(int)  # xoay vòng giá trị mỗi loại slot

    def next_value(kind: str) -> dict:
        values = slots[kind]
        v = values[cursor[kind] % len(values)]
        cursor[kind] += 1
        return v

    rows: list[dict] = []
    seen_texts: set[tuple[str, str]] = set()
    for tpl in spec["templates"]:
        src = tpl["from"]
        tgt = "en" if src == "vi" else "vi"
        names = SLOT_RE.findall(tpl[src])
        value_slots = sorted({n for n in names if n not in ADDRESS_SLOTS})
        uses_address = any(n in ADDRESS_SLOTS for n in names)
        for n in value_slots:
            if n not in slots:
                raise GoldenError(f"{tpl['id']}: slot {{{n}}} chưa khai báo")
        n_instances = INSTANCES_PER_TEMPLATE[src] if value_slots or uses_address else 1
        clarify = tpl.get("status") == "needs_clarification"

        for k in range(n_instances):
            values = {n: next_value(n) for n in value_slots}
            pair = None
            if uses_address:
                pair = pairs[cursor["__pair"] % len(pairs)]
                cursor["__pair"] += 1
            text = _fill(tpl[src], values, src, pair)
            reference = _fill(tpl[tgt], values, tgt, pair) if tpl.get(tgt) else None
            if reference is None and not clarify:
                raise GoldenError(f"{tpl['id']}: thiếu bản tham chiếu")

            tags = set(tpl.get("tags", []))
            for v in values.values():
                tags.update(v.get("tags", []))
            if uses_address:
                tags.add("address_forms")

            if clarify:
                entities: list[str] = []
                glossary_terms: list[str] = []
            else:
                entities = tpl.get("entities") or [values[n][tgt] for n in value_slots]
                expected = _glossary_expectations(glossary, text, src, tgt)
                for term_id, target in expected:
                    if target.lower() not in reference.lower():
                        raise GoldenError(
                            f"{tpl['id']}: bản tham chiếu không dùng thuật ngữ {term_id}"
                        )
                for e in entities:
                    if e not in reference:
                        raise GoldenError(f"{tpl['id']}: bản tham chiếu thiếu thực thể {e!r}")
                glossary_terms = [target for _, target in expected]

            variants = [("base", text, set())]
            if src == "vi":
                variants.append(("unaccented", fold(text), {"unaccented"}))
                chat = fold(abbreviate(text, spec["abbreviations"]))
                if chat != fold(text):
                    variants.append(("chat", chat, {"unaccented", "abbreviation"}))
            for name, variant_text, extra in variants:
                if (variant_text, tgt) in seen_texts:
                    continue
                seen_texts.add((variant_text, tgt))
                suffix = "" if name == "base" else f"-{name}"
                expect: dict = {"entities": entities, "glossary": glossary_terms}
                if clarify:
                    expect["status"] = "needs_clarification"
                rows.append(
                    {
                        "id": f"golden-{tpl['id']}-{k + 1}{suffix}",
                        "source_lang": src,
                        "target_lang": tgt,
                        "text": variant_text,
                        "reference": reference,
                        "tags": sorted(tags | extra),
                        "expect": expect,
                        "meta": {
                            "template": tpl["id"],
                            "variant": name,
                            "review_status": "synthetic-unreviewed",
                        },
                    }
                )
    return rows


def render(rows: list[dict]) -> str:
    return "".join(json.dumps(r, ensure_ascii=False) + "\n" for r in rows)


def build_all() -> dict[Path, str]:
    """Ánh xạ file đích → nội dung, cho mọi file mẫu trong eval/golden/."""
    out = {}
    for spec_path in sorted(GOLDEN_DIR.glob("*.json")):
        spec = json.loads(spec_path.read_text(encoding="utf-8"))
        glossary = load(GLOSSARY_DIR / spec_path.name)
        rows = generate(spec, glossary)
        out[DATASET_DIR / f"golden.{spec_path.stem}.jsonl"] = render(rows)
    return out


def stats(rows: list[dict]) -> dict:
    by_tag: dict[str, int] = defaultdict(int)
    for r in rows:
        for t in r["tags"]:
            by_tag[t] += 1
    by_dir = defaultdict(int)
    for r in rows:
        by_dir[f"{r['source_lang']}->{r['target_lang']}"] += 1
    return {"rows": len(rows), "by_direction": dict(by_dir), "by_tag": dict(sorted(by_tag.items()))}


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(prog="xt_eval.golden")
    ap.add_argument("--check", action="store_true", help="chỉ kiểm tra, không ghi")
    args = ap.parse_args(argv)
    try:
        outputs = build_all()
    except GoldenError as e:
        print(f"golden: {e}", file=sys.stderr)
        return 1
    stale = [
        p for p, content in outputs.items() if not p.exists() or p.read_text("utf-8") != content
    ]
    if args.check:
        for p in stale:
            print(
                f"golden: {p.relative_to(REPO)} lệch với mẫu — chạy `make golden`", file=sys.stderr
            )
        return 1 if stale else 0
    for p, content in outputs.items():
        p.write_text(content, encoding="utf-8")
        rows = [json.loads(line) for line in content.splitlines()]
        print(f"{p.relative_to(REPO)}: {json.dumps(stats(rows), ensure_ascii=False)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
