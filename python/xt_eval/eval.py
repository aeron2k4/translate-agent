"""make eval: chạy tập kiểm thử chất lượng.

Đo: tuân thủ glossary, khớp số/thực thể, lỗi nghiêm trọng / 100 tin, đúng/sai câu
hỏi làm rõ, độ phủ ca khó; COMET nếu đặt biến môi trường XT_COMET_CMD (lệnh nhận
JSONL {src, mt, ref} ở stdin và in một điểm hệ thống). Không hạ ngưỡng để CI qua.

Lưu ý: cấu hình mặc định dùng provider giả, nên kết quả chỉ kiểm tra cơ chế
(che thực thể, glossary, validator), không đo chất lượng dịch thật.
"""

from __future__ import annotations

import argparse
import json
import os
import shlex
import subprocess
import sys
from dataclasses import asdict, dataclass, field
from pathlib import Path

from . import cli

# Ngưỡng theo CLAUDE.md.
MIN_GLOSSARY = 0.98
MIN_COMET = 0.85
MAX_SEVERE_PER_100 = 1.0
REQUIRED_TAGS = frozenset(
    {
        "unaccented",
        "abbreviation",
        "mixed",
        "no_subject",
        "cu",
        "toi",
        "vi_date",
        "address_forms",
    }
)

OK_STATUSES = {"translated", "cached"}


@dataclass
class Report:
    cases: int = 0
    glossary_hits: int = 0
    glossary_total: int = 0
    entity_hits: int = 0
    entity_total: int = 0
    severe: int = 0
    clarify_correct: int = 0
    clarify_total: int = 0
    comet: float | None = None
    missing_tags: list[str] = field(default_factory=list)
    failed_cases: list[str] = field(default_factory=list)

    @property
    def glossary_compliance(self) -> float:
        return self.glossary_hits / self.glossary_total if self.glossary_total else 1.0

    @property
    def entity_accuracy(self) -> float:
        return self.entity_hits / self.entity_total if self.entity_total else 1.0

    @property
    def severe_per_100(self) -> float:
        return 100 * self.severe / self.cases if self.cases else 0.0

    def gates(self) -> dict[str, bool]:
        gates = {
            "glossary_compliance": self.glossary_compliance >= MIN_GLOSSARY,
            "severe_per_100": self.severe_per_100 < MAX_SEVERE_PER_100,
            "hard_case_coverage": not self.missing_tags,
        }
        if self.comet is not None:
            gates["comet"] = self.comet >= MIN_COMET
        return gates

    def summary(self) -> dict:
        return {
            **{k: v for k, v in asdict(self).items() if k not in {"failed_cases"}},
            "glossary_compliance": round(self.glossary_compliance, 4),
            "entity_accuracy": round(self.entity_accuracy, 4),
            "severe_per_100": round(self.severe_per_100, 2),
            "comet": self.comet if self.comet is not None else "not measured",
            "failed_cases": self.failed_cases,
            "gates": self.gates(),
        }


def to_request(case: dict) -> dict:
    return {
        "thread_id": f"eval-{case['id']}",
        "sender_id": "eval-sender",
        "text": case["text"],
        "target_langs": [case["target_lang"]],
        "domain": case.get("domain", "automotive"),
    }


def score(cases: list[dict], results: dict[str, dict]) -> Report:
    r = Report(cases=len(cases))
    tags = {t for c in cases for t in c.get("tags", [])}
    r.missing_tags = sorted(REQUIRED_TAGS - tags)
    for case in cases:
        expect = case.get("expect", {})
        tr = results[case["id"]]["response"]["translations"][0]
        want_status = expect.get("status")
        if want_status == "needs_clarification" or tr["status"] == "needs_clarification":
            r.clarify_total += 1
            if tr["status"] == want_status:
                r.clarify_correct += 1
            else:
                r.failed_cases.append(case["id"])
            continue
        text = tr["text"]
        severe = tr["status"] not in OK_STATUSES
        lowered = text.lower()
        for term in expect.get("glossary", []):
            r.glossary_total += 1
            r.glossary_hits += term.lower() in lowered
        for ent in expect.get("entities", []):
            r.entity_total += 1
            if ent in text:
                r.entity_hits += 1
            else:
                severe = True  # sai số/thực thể là lỗi nghiêm trọng
        if severe:
            r.severe += 1
            r.failed_cases.append(case["id"])
    return r


def comet_score(cases: list[dict], results: dict[str, dict]) -> float | None:
    cmd = os.environ.get("XT_COMET_CMD")
    if not cmd:
        return None
    rows = [
        {
            "src": c["text"],
            "mt": results[c["id"]]["response"]["translations"][0]["text"],
            "ref": c["reference"],
        }
        for c in cases
        if results[c["id"]]["response"]["translations"][0]["status"] in OK_STATUSES
    ]
    payload = "".join(json.dumps(row, ensure_ascii=False) + "\n" for row in rows)
    proc = subprocess.run(
        shlex.split(cmd), input=payload, capture_output=True, text=True, check=True
    )
    return float(proc.stdout.strip().splitlines()[-1])


def dump(path: Path, cases: list[dict], results: dict[str, dict]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as f:
        for c in cases:
            tr = results[c["id"]]["response"]["translations"][0]
            row = {
                "id": c["id"],
                "tags": c.get("tags", []),
                "src": c["text"],
                "mt": tr["text"],
                "ref": c.get("reference"),
                "status": tr["status"],
                "tier": tr["tier"],
                "issues": tr["issues"],
            }
            f.write(json.dumps(row, ensure_ascii=False) + "\n")


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(prog="xt_eval.eval")
    ap.add_argument("--cli", required=True)
    ap.add_argument("--dataset", type=Path, default=cli.REPO / "eval" / "datasets")
    ap.add_argument(
        "--dump",
        type=Path,
        help="ghi từng ca (nguồn, bản dịch, tham chiếu) ra JSONL để phân tích lỗi; "
        "chỉ dùng với dataset tổng hợp",
    )
    args = ap.parse_args(argv)

    cases = [c for f in sorted(args.dataset.glob("*.jsonl")) for c in cli.load_jsonl(f)]
    results = cli.run(args.cli, ((c["id"], to_request(c)) for c in cases))
    report = score(cases, results)
    report.comet = comet_score(cases, results)
    if args.dump:
        dump(args.dump, cases, results)
    print(json.dumps(report.summary(), ensure_ascii=False, indent=2))
    if report.comet is None:
        print("COMET: chưa đo (đặt XT_COMET_CMD để đo).", file=sys.stderr)
    if cli.models_config().name == "models.toml":
        note = "Provider giả: số đo chỉ kiểm tra cơ chế, không phải chất lượng dịch."
        print(note, file=sys.stderr)
    return 0 if all(report.gates().values()) else 1


if __name__ == "__main__":
    sys.exit(main())
