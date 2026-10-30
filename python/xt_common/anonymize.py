"""Ẩn danh câu ví dụ trước khi lưu vào glossary / review.

Thay mọi chuỗi có thể định danh (email, URL, @mention, chữ số) bằng nhãn cố định.
Không dùng cho tin thật trên đường nóng — việc đó do crate `validators` làm trong TEE.
"""

from __future__ import annotations

import re

_PATTERNS: list[tuple[re.Pattern[str], str]] = [
    (re.compile(r"[\w.%+-]+@[\w-]+(?:\.[\w-]+)*\.[A-Za-z]{2,}"), "⟨email⟩"),
    (re.compile(r"(?:https?://|www\.)\S+", re.IGNORECASE), "⟨url⟩"),
    (re.compile(r"@[\w.]*\w"), "⟨mention⟩"),
    (re.compile(r"\+?\d(?:[\d.,/ -]*\d)?"), "⟨số⟩"),
]


def anonymize(text: str) -> str:
    for pattern, label in _PATTERNS:
        text = pattern.sub(label, text)
    return text


def is_anonymized(text: str) -> bool:
    return anonymize(text) == text
