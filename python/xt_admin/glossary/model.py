from __future__ import annotations

import json
import os
import tempfile
from collections.abc import Iterable
from dataclasses import dataclass, field, replace
from pathlib import Path

from xt_common.anonymize import is_anonymized


class GlossaryError(ValueError):
    pass


@dataclass(frozen=True)
class Term:
    id: str
    forms: dict[str, str]
    core: bool = False
    example: str | None = None


@dataclass(frozen=True)
class Glossary:
    domain: str
    langs: tuple[str, str]
    version: int
    terms: tuple[Term, ...] = field(default_factory=tuple)

    def term(self, term_id: str) -> Term | None:
        return next((t for t in self.terms if t.id == term_id), None)


def from_dict(data: dict) -> Glossary:
    langs = tuple(data["langs"])
    if len(langs) != 2:
        raise GlossaryError("langs phải có đúng 2 ngôn ngữ")
    terms = tuple(
        Term(
            id=t["id"],
            forms={lang: t.get(lang, "") for lang in langs},
            core=bool(t.get("core", False)),
            example=t.get("example"),
        )
        for t in data["terms"]
    )
    return Glossary(domain=data["domain"], langs=langs, version=int(data["version"]), terms=terms)


def load(path: str | Path) -> Glossary:
    return from_dict(json.loads(Path(path).read_text(encoding="utf-8")))


def dump(g: Glossary) -> str:
    """Định dạng ổn định: mỗi thuật ngữ một dòng, sắp theo id, để diff dễ đọc."""
    lines = []
    for t in sorted(g.terms, key=lambda t: t.id):
        obj: dict[str, object] = {"id": t.id, **{lang: t.forms[lang] for lang in g.langs}}
        obj["core"] = t.core
        if t.example is not None:
            obj["example"] = t.example
        lines.append("    " + json.dumps(obj, ensure_ascii=False))
    header = (
        "{\n"
        f'  "domain": {json.dumps(g.domain, ensure_ascii=False)},\n'
        f'  "langs": {json.dumps(list(g.langs))},\n'
        f'  "version": {g.version},\n'
        '  "terms": [\n'
    )
    return header + ",\n".join(lines) + "\n  ]\n}\n"


def save(g: Glossary, path: str | Path) -> None:
    errors = validate(g)
    if errors:
        raise GlossaryError("; ".join(errors))
    path = Path(path)
    fd, tmp = tempfile.mkstemp(dir=path.parent, prefix=".glossary-")
    with os.fdopen(fd, "w", encoding="utf-8") as f:
        f.write(dump(g))
    os.replace(tmp, path)


def validate(g: Glossary) -> list[str]:
    errors: list[str] = []
    if g.version < 1:
        errors.append("version phải ≥ 1")
    seen_ids: set[str] = set()
    seen_forms: dict[str, set[str]] = {lang: set() for lang in g.langs}
    for t in g.terms:
        if t.id in seen_ids:
            errors.append(f"{t.id}: trùng id")
        seen_ids.add(t.id)
        for lang in g.langs:
            form = t.forms.get(lang, "").strip()
            if not form:
                errors.append(f"{t.id}: thiếu bản {lang}")
            elif form.lower() in seen_forms[lang]:
                errors.append(f"{t.id}: trùng thuật ngữ {lang}")
            seen_forms[lang].add(form.lower())
        if t.example is not None and not is_anonymized(t.example):
            errors.append(f"{t.id}: câu ví dụ chưa ẩn danh")
    return errors


def apply_changes(
    g: Glossary, upserts: Iterable[Term] = (), removals: Iterable[str] = ()
) -> Glossary:
    """Trả về glossary mới đã tăng phiên bản. Phiên bản mới làm khoá phrase cache
    cũ không còn khớp, nên cache liên quan tự vô hiệu hoá."""
    by_id = {t.id: t for t in g.terms}
    changed = False
    for rid in removals:
        if by_id.pop(rid, None) is not None:
            changed = True
    for t in upserts:
        if by_id.get(t.id) != t:
            by_id[t.id] = t
            changed = True
    if not changed:
        raise GlossaryError("không có thay đổi")
    new = replace(g, version=g.version + 1, terms=tuple(by_id.values()))
    errors = validate(new)
    if errors:
        raise GlossaryError("; ".join(errors))
    return new
