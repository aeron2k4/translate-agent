"""Hàng chờ review.

Chỉ lưu cặp thuật ngữ + câu ví dụ đã ẩn danh + id người đề xuất đã hash — không
bao giờ lưu cả tin. Đề xuất đến từ `translator` (Rust, trong TEE) dưới dạng JSONL.
"""

from __future__ import annotations

import json
import os
import re
import tempfile
import uuid
from dataclasses import asdict, dataclass, field
from datetime import UTC, datetime
from pathlib import Path
from typing import Literal

from xt_admin.glossary import Glossary, Term, apply_changes, load, save
from xt_common.anonymize import is_anonymized

Status = Literal["pending", "approved", "rejected"]

MAX_EXAMPLE_CHARS = 200
_HASHED_ID = re.compile(r"^[0-9a-f]{24}$")


class ReviewError(ValueError):
    pass


@dataclass
class Suggestion:
    domain: str
    source_lang: str
    target_lang: str
    source_term: str
    target_term: str
    proposer: str  # id đã hash
    example: str | None = None
    id: str = field(default_factory=lambda: uuid.uuid4().hex[:12])
    status: Status = "pending"
    decided_by: str | None = None
    reason: str | None = None
    created_at: str = field(default_factory=lambda: datetime.now(UTC).isoformat())

    def validate(self) -> None:
        if not self.source_term.strip() or not self.target_term.strip():
            raise ReviewError("thiếu thuật ngữ")
        if not _HASHED_ID.match(self.proposer):
            raise ReviewError("proposer phải là id đã hash")
        if self.example is not None:
            if len(self.example) > MAX_EXAMPLE_CHARS:
                raise ReviewError("câu ví dụ quá dài")
            if not is_anonymized(self.example):
                raise ReviewError("câu ví dụ chưa ẩn danh")


def _next_term_id(g: Glossary) -> str:
    prefix = g.domain[:4]
    numbers = [
        int(t.id.rsplit("-", 1)[1])
        for t in g.terms
        if t.id.startswith(prefix + "-") and t.id.rsplit("-", 1)[1].isdigit()
    ]
    return f"{prefix}-{max(numbers, default=0) + 1:03d}"


class ReviewQueue:
    """Hàng chờ lưu trong một file JSON. `owners` ánh xạ lĩnh vực → id chủ sở hữu."""

    def __init__(self, path: str | Path, owners: dict[str, set[str]]) -> None:
        self._path = Path(path)
        self._owners = owners
        self._items: dict[str, Suggestion] = {}
        if self._path.exists():
            for raw in json.loads(self._path.read_text(encoding="utf-8")):
                s = Suggestion(**raw)
                self._items[s.id] = s

    def _persist(self) -> None:
        data = [asdict(s) for s in self._items.values()]
        fd, tmp = tempfile.mkstemp(dir=self._path.parent, prefix=".review-")
        with os.fdopen(fd, "w", encoding="utf-8") as f:
            json.dump(data, f, ensure_ascii=False, indent=2)
        os.replace(tmp, self._path)

    def submit(self, s: Suggestion) -> Suggestion:
        s.validate()
        for other in self.pending(s.domain):
            if (
                other.source_lang,
                other.target_lang,
                other.source_term.lower(),
                other.target_term.lower(),
            ) == (s.source_lang, s.target_lang, s.source_term.lower(), s.target_term.lower()):
                return other  # gộp đề xuất trùng
        self._items[s.id] = s
        self._persist()
        return s

    def pending(self, domain: str | None = None) -> list[Suggestion]:
        return [
            s
            for s in self._items.values()
            if s.status == "pending" and (domain is None or s.domain == domain)
        ]

    def get(self, sid: str) -> Suggestion:
        try:
            return self._items[sid]
        except KeyError:
            raise ReviewError("không có đề xuất") from None

    def _decide(self, sid: str, owner: str) -> Suggestion:
        s = self.get(sid)
        if s.status != "pending":
            raise ReviewError("đề xuất đã được xử lý")
        if owner not in self._owners.get(s.domain, set()):
            raise ReviewError("chỉ chủ sở hữu glossary của lĩnh vực mới được duyệt")
        return s

    def approve(self, sid: str, owner: str, glossary_path: str | Path) -> int:
        """Duyệt: thêm/cập nhật thuật ngữ, tăng phiên bản glossary. Trả về phiên bản mới.
        Phiên bản mới làm khoá phrase cache cũ không còn khớp."""
        s = self._decide(sid, owner)
        g = load(glossary_path)
        if {s.source_lang, s.target_lang} != set(g.langs):
            raise ReviewError("cặp ngôn ngữ không khớp glossary")
        existing = next(
            (t for t in g.terms if t.forms[s.source_lang].lower() == s.source_term.lower()),
            None,
        )
        forms = {s.source_lang: s.source_term, s.target_lang: s.target_term}
        term = (
            Term(existing.id, forms, existing.core, s.example or existing.example)
            if existing
            else Term(_next_term_id(g), forms, False, s.example)
        )
        new = apply_changes(g, upserts=[term])
        save(new, glossary_path)
        s.status, s.decided_by = "approved", owner
        self._persist()
        return new.version

    def reject(self, sid: str, owner: str, reason: str) -> None:
        s = self._decide(sid, owner)
        s.status, s.decided_by, s.reason = "rejected", owner, reason
        self._persist()
