"""Gọi binary `xt-cli` (Rust) để chạy pipeline dịch hoặc LLM chấm trên JSONL."""

from __future__ import annotations

import json
import os
import subprocess
from collections.abc import Iterable
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
MOCK_CONFIG = REPO / "config" / "models.toml"


def models_config() -> Path:
    """Cấu hình mô hình: `XT_MODELS_CONFIG` (tương đối với repo) hoặc provider giả."""
    return REPO / os.environ.get("XT_MODELS_CONFIG", "config/models.toml")


def is_mock(config: Path) -> bool:
    return config.resolve() == MOCK_CONFIG.resolve()


def line(case_id: str, request: dict, history: list[dict] | None = None) -> dict:
    """Một dòng vào của xt-cli; `history` được nạp vào bộ nhớ luồng trước khi dịch."""
    out: dict = {"id": case_id, "request": request}
    if history:
        out["history"] = history
    return out


def _exec(cli: str | Path, rows: Iterable[dict], extra: list[str], config: Path) -> dict[str, dict]:
    payload = "".join(json.dumps(r, ensure_ascii=False) + "\n" for r in rows)
    args = [str(cli), "--glossary-dir", str(REPO / "glossary" / "data"), "--config", str(config)]
    proc = subprocess.run(args + extra, input=payload, capture_output=True, text=True, check=False)
    if proc.returncode != 0:
        raise RuntimeError(f"xt-cli lỗi (mã {proc.returncode}): {proc.stderr.strip()}")
    out = {}
    for raw in proc.stdout.splitlines():
        row = json.loads(raw)
        out[row["id"]] = row
    return out


def run(
    cli: str | Path,
    lines: Iterable[dict],
    *,
    audit: bool = False,
    telemetry: Path | None = None,
    config: Path | None = None,
) -> dict[str, dict]:
    """Dịch từng dòng (xem `line`). `audit` kèm prompt thật đã gửi provider."""
    extra = ["--audit"] if audit else []
    if telemetry is not None:
        extra += ["--telemetry", str(telemetry)]
    return _exec(cli, lines, extra, config or models_config())


def judge(cli: str | Path, items: Iterable[dict], *, config: Path | None = None) -> dict[str, dict]:
    """LLM chấm qua model-router: mỗi item là {"id", "judge": {...JudgeInput}}."""
    return _exec(cli, items, ["--judge"], config or models_config())


def load_jsonl(path: Path) -> list[dict]:
    return [json.loads(raw) for raw in path.read_text(encoding="utf-8").splitlines() if raw]
