"""Số đo vận hành phía Python.

Quy tắc bảo mật: sự kiện chỉ chứa độ trễ, số đếm, mã lỗi và id đã hash.
Không có trường nào nhận nội dung tin nhắn; đừng thêm.
"""

from __future__ import annotations

import hashlib
import hmac
import json
from dataclasses import asdict, dataclass
from typing import TextIO


def hash_id(key: bytes, raw: str) -> str:
    """Băm id bằng HMAC-SHA256, cùng định dạng với crate `telemetry`."""
    return hmac.new(key, raw.encode(), hashlib.sha256).digest()[:12].hex()


@dataclass(frozen=True)
class Event:
    name: str
    latency_ms: float | None = None
    count: int | None = None
    error: str | None = None
    ref: str | None = None  # id đã hash hoặc id thuật ngữ, không phải nội dung


class JsonSink:
    def __init__(self, out: TextIO) -> None:
        self._out = out

    def emit(self, event: Event) -> None:
        data = {k: v for k, v in asdict(event).items() if v is not None}
        self._out.write(json.dumps(data, ensure_ascii=False) + "\n")
