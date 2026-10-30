"""Gọi binary `xt-cli` (Rust) để chạy pipeline dịch trên JSONL."""

from __future__ import annotations

import json
import os
import subprocess
from collections.abc import Iterable
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]


def models_config() -> Path:
    """Cấu hình mô hình: `XT_MODELS_CONFIG` (tương đối với repo) hoặc provider giả."""
    return REPO / os.environ.get("XT_MODELS_CONFIG", "config/models.toml")


def run(
    cli: str | Path,
    requests: Iterable[tuple[str, dict]],
    *,
    audit: bool = False,
    telemetry: Path | None = None,
) -> dict[str, dict]:
    lines = "".join(
        json.dumps({"id": cid, "request": req}, ensure_ascii=False) + "\n" for cid, req in requests
    )
    args = [
        str(cli),
        "--glossary-dir",
        str(REPO / "glossary" / "data"),
        "--config",
        str(models_config()),
    ]
    if audit:
        args.append("--audit")
    if telemetry is not None:
        args += ["--telemetry", str(telemetry)]
    proc = subprocess.run(args, input=lines, capture_output=True, text=True, check=False)
    if proc.returncode != 0:
        raise RuntimeError(f"xt-cli lỗi (mã {proc.returncode}): {proc.stderr.strip()}")
    out = {}
    for line in proc.stdout.splitlines():
        row = json.loads(line)
        out[row["id"]] = row
    return out


def load_jsonl(path: Path) -> list[dict]:
    return [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines() if line]
