"""make benchmark: chấm golden dataset, báo cáo tổng và theo loại ca khó.

Chỉ số:
- Tất định: chrF++ (corpus), tuân thủ glossary, khớp số/thực thể, lỗi nghiêm trọng
  tất định / 100 tin (trạng thái lỗi hoặc sai thực thể), đúng câu hỏi làm rõ.
- Kiểu RAGAS cho phần truy xuất (đọc từ prompt thật, chế độ --audit): context
  precision/recall của glossary, thread context recall của bộ nhớ luồng.
- LLM chấm qua model-router (chỉ khi cấu hình không phải provider giả):
  faithfulness (đơn vị thông tin được nguồn hỗ trợ), answer correctness, lỗi MQM.
- Độ trễ, TTFT p50/p95 từ telemetry (chỉ có số đo, không có nội dung).

Báo cáo ghi ra eval/reports/benchmark-<nhãn>.{json,md}: chỉ có số đo và id ca, không
có văn bản. Chỉ chạy trên dữ liệu tổng hợp trong eval/.
"""

from __future__ import annotations

import argparse
import json
import sys
import tempfile
from collections import defaultdict
from collections.abc import Iterable
from pathlib import Path

from . import cli, metrics
from .eval import OK_STATUSES, to_request

MQM_WEIGHTS = {"minor": 1, "major": 5, "critical": 25}
SEVERE = frozenset({"major", "critical"})
REPORTS = cli.REPO / "eval" / "reports"
# Thứ tự hiển thị các nhóm trong báo cáo (nhóm khác xếp sau theo tên).
TAG_ORDER = [
    "unaccented",
    "abbreviation",
    "mixed",
    "no_subject",
    "cu",
    "toi",
    "vi_date",
    "address_forms",
    "context",
    "en_vi",
]


def load_glossary_index(directory: Path) -> dict[tuple[str, str, str, str], str]:
    """(ngôn ngữ nguồn, ngôn ngữ đích, thuật ngữ nguồn, thuật ngữ đích) → id."""
    index: dict[tuple[str, str, str, str], str] = {}
    for f in sorted(directory.glob("*.json")):
        data = json.loads(f.read_text(encoding="utf-8"))
        a, b = data["langs"]
        for t in data["terms"]:
            index[(a, b, t[a], t[b])] = t["id"]
            index[(b, a, t[b], t[a])] = t["id"]
    return index


def glossary_lines(case: dict, index: dict[tuple[str, str, str, str], str]) -> list[str]:
    """Thuật ngữ kỳ vọng dạng "nguồn => đích" để đưa cho LLM chấm."""
    want = set(case.get("expect", {}).get("glossary_ids", []))
    src, tgt = case["source_lang"], case["target_lang"]
    return sorted(
        f"{s} => {t}" for (a, b, s, t), tid in index.items() if (a, b) == (src, tgt) and tid in want
    )


def score_case(
    case: dict, row: dict, index: dict[tuple[str, str, str, str], str]
) -> dict[str, float | int | str | bool | None]:
    """Số đo của một ca (không chứa văn bản)."""
    expect = case.get("expect", {})
    tr = row["response"]["translations"][0]
    out: dict[str, float | int | str | bool | None] = {
        "id": case["id"],
        "status": tr["status"],
        "tier": tr.get("tier"),
    }
    want_clarify = expect.get("status") == "needs_clarification"
    if want_clarify or tr["status"] == "needs_clarification":
        out["clarify_ok"] = tr["status"] == expect.get("status")
        return out

    text = tr["text"]
    ok = tr["status"] in OK_STATUSES
    lowered = text.lower()
    terms = expect.get("glossary", [])
    ents = expect.get("entities", [])
    out["glossary_hits"] = sum(t.lower() in lowered for t in terms)
    out["glossary_total"] = len(terms)
    out["entity_hits"] = sum(e in text for e in ents)
    out["entity_total"] = len(ents)
    out["severe"] = (not ok) or out["entity_hits"] < len(ents)
    if case.get("reference"):
        out["chrf"] = metrics.chrf(text, case["reference"])

    prompts = row.get("audit", {}).get("prompts", [])
    if prompts:
        view = metrics.parse_prompt(prompts[0]["body"])
        src, tgt = case["source_lang"], case["target_lang"]
        ids = [index.get((src, tgt, s, t)) for s, t in view.glossary]
        want = set(expect.get("glossary_ids", []))
        out["ctx_precision"] = metrics.context_precision([i in want for i in ids])
        out["ctx_recall"] = metrics.recall(want, [i for i in ids if i])
        needed = expect.get("context_needed")
        if needed is not None:
            present = metrics.thread_turns_present(len(case.get("history", [])), view.context_lines)
            out["thread_recall"] = metrics.recall(needed, present)
    return out


def judge_input(case: dict, row: dict, index: dict[tuple[str, str, str, str], str]) -> dict:
    tr = row["response"]["translations"][0]
    return {
        "domain": case.get("domain", "automotive"),
        "source_lang": case["source_lang"],
        "target_lang": case["target_lang"],
        "source": case["text"],
        "translation": tr["text"],
        "reference": case.get("reference", ""),
        "context": [f"{h['sender_id']}: {h['text']}" for h in case.get("history", [])],
        "glossary": glossary_lines(case, index),
    }


def apply_verdict(scores: dict, verdict: dict, words: int) -> None:
    errors = verdict.get("errors", [])
    units = verdict["units"]
    scores["faithfulness"] = verdict["supported"] / units if units else None
    scores["correctness"] = verdict["correctness"] / 100
    scores["mqm_severe"] = sum(e["severity"] in SEVERE for e in errors)
    scores["mqm_critical"] = sum(e["severity"] == "critical" for e in errors)
    scores["mqm_penalty"] = sum(MQM_WEIGHTS.get(e["severity"], 0) for e in errors)
    scores["mqm_words"] = words


def _mean(values: Iterable[float | None]) -> float | None:
    vs = [v for v in values if v is not None]
    return sum(vs) / len(vs) if vs else None


def aggregate(cases: list[dict], scores: dict[str, dict], results: dict[str, dict]) -> dict:
    """Gộp số đo cho một nhóm ca."""
    rows = [scores[c["id"]] for c in cases]
    translated = [s for s in rows if "clarify_ok" not in s]
    clarify = [s for s in rows if "clarify_ok" in s]
    n = len(translated)

    def ratio(hits: str, total: str) -> float | None:
        t = sum(s[total] for s in translated)
        return sum(s[hits] for s in translated) / t if t else None

    pairs = [
        (results[c["id"]]["response"]["translations"][0]["text"], c["reference"])
        for c in cases
        if "chrf" in scores[c["id"]]
    ]
    out: dict = {
        "cases": len(rows),
        "translated_cases": n,
        "chrf_pp": metrics.corpus_chrf(pairs) if pairs else None,
        "glossary_compliance": ratio("glossary_hits", "glossary_total"),
        "entity_accuracy": ratio("entity_hits", "entity_total"),
        "severe_per_100": 100 * sum(bool(s["severe"]) for s in translated) / n if n else None,
        "clarify_accuracy": (
            sum(bool(s["clarify_ok"]) for s in clarify) / len(clarify) if clarify else None
        ),
        "context_precision": _mean(s.get("ctx_precision") for s in translated),
        "context_recall": _mean(s.get("ctx_recall") for s in translated),
        "thread_context_recall": _mean(s.get("thread_recall") for s in translated),
        "strong_tier_share": (
            sum(s.get("tier") == "strong" for s in translated) / n if n else None
        ),
    }
    judged = [s for s in translated if "correctness" in s]
    if judged:
        words = sum(int(s["mqm_words"]) for s in judged)
        out |= {
            "judged_cases": len(judged),
            "faithfulness": _mean(s["faithfulness"] for s in judged),
            "answer_correctness": _mean(s["correctness"] for s in judged),
            "mqm_severe_per_100": 100 * sum(int(s["mqm_severe"]) for s in judged) / len(judged),
            "mqm_critical_per_100": (
                100 * sum(int(s["mqm_critical"]) for s in judged) / len(judged)
            ),
            "mqm_error_free": sum(s["mqm_penalty"] == 0 for s in judged) / len(judged),
            "mqm_score": (
                100 * (1 - sum(int(s["mqm_penalty"]) for s in judged) / words) if words else None
            ),
        }
    return out


def latency(events: list[dict]) -> dict:
    done = [e for e in events if e.get("name") == "translate.done"]
    lat = [e["latency"] for e in done if e.get("latency") is not None]
    ttft = [e["ttft"] for e in done if e.get("ttft") is not None]
    return {
        "model_calls": len(done),
        "cache_hits": sum(e.get("name") == "translate.cache_hit" for e in events),
        "errors": sum(e.get("name") == "translate.error" for e in events),
        "latency_p50_ms": metrics.percentile(lat, 50),
        "latency_p95_ms": metrics.percentile(lat, 95),
        "ttft_p50_ms": metrics.percentile(ttft, 50),
        "ttft_p95_ms": metrics.percentile(ttft, 95),
    }


def by_tag(cases: list[dict]) -> dict[str, list[dict]]:
    groups: dict[str, list[dict]] = defaultdict(list)
    for c in cases:
        for t in c.get("tags", []):
            groups[t].append(c)
        if not c.get("tags"):
            groups["plain"].append(c)
    order = {t: i for i, t in enumerate(TAG_ORDER)}
    return dict(sorted(groups.items(), key=lambda kv: (order.get(kv[0], len(order)), kv[0])))


def _fmt(v: float | int | None, kind: str = "pct") -> str:
    if v is None:
        return "–"
    if kind == "pct":
        return f"{100 * v:.1f}%"
    if kind == "num":
        return f"{v:.1f}"
    return str(v)


def markdown(report: dict) -> str:
    o, lat, meta = report["overall"], report["latency"], report["meta"]
    judged = "answer_correctness" in o
    lines = [
        f"# Benchmark: {meta['label']}",
        "",
        f"- Dataset: {meta['dataset']} ({o['cases']} cases, synthetic)",
        f"- Model config: `{meta['config']}`",
        f"- LLM judge: {'strong tier of the same config' if judged else 'not run'}",
        "",
        "## Overall",
        "",
        "| Metric | Value | Target |",
        "| --- | --- | --- |",
        f"| chrF++ (corpus) | {_fmt(o['chrf_pp'], 'num')} | |",
        f"| Glossary compliance | {_fmt(o['glossary_compliance'])} | ≥ 98% |",
        f"| Entity / number accuracy | {_fmt(o['entity_accuracy'])} | 100% |",
        f"| Severe errors / 100 msgs (deterministic) | {_fmt(o['severe_per_100'], 'num')} | < 1 |",
        f"| Clarification accuracy | {_fmt(o['clarify_accuracy'])} | |",
        f"| Glossary context precision (RAGAS) | {_fmt(o['context_precision'])} | |",
        f"| Glossary context recall (RAGAS) | {_fmt(o['context_recall'])} | |",
        f"| Thread context recall | {_fmt(o['thread_context_recall'])} | |",
        f"| Strong-tier share | {_fmt(o['strong_tier_share'])} | 10–20% |",
    ]
    if judged:
        lines += [
            f"| Faithfulness (RAGAS, LLM judge) | {_fmt(o['faithfulness'])} | |",
            f"| Answer correctness (LLM judge) | {_fmt(o['answer_correctness'])} | |",
            f"| MQM severe errors / 100 msgs | {_fmt(o['mqm_severe_per_100'], 'num')} | < 1 |",
            f"| MQM critical errors / 100 msgs | {_fmt(o['mqm_critical_per_100'], 'num')} | |",
            f"| MQM error-free messages | {_fmt(o['mqm_error_free'])} | |",
            f"| MQM score (100 − penalty per word) | {_fmt(o['mqm_score'], 'num')} | |",
        ]
    lines += [
        "",
        "## Latency",
        "",
        "| Metric | p50 | p95 | Target p50 / p95 |",
        "| --- | --- | --- | --- |",
        f"| TTFT (ms) | {_fmt(lat['ttft_p50_ms'], 'num')} | {_fmt(lat['ttft_p95_ms'], 'num')}"
        " | 400 / 1200 |",
        f"| Full translation (ms) | {_fmt(lat['latency_p50_ms'], 'num')}"
        f" | {_fmt(lat['latency_p95_ms'], 'num')} | – / 2000 |",
        "",
        f"Model calls: {lat['model_calls']}, cache hits: {lat['cache_hits']},"
        f" errors: {lat['errors']}.",
        "",
        "## By hard-case type",
        "",
    ]
    head = ["Tag", "n", "chrF++", "Glossary", "Entities", "Severe/100", "Thread ctx"]
    if judged:
        head += ["Faithful", "Correct", "MQM severe/100"]
    lines += ["| " + " | ".join(head) + " |", "|" + " --- |" * len(head)]
    for tag, g in report["by_tag"].items():
        cells = [
            tag,
            str(g["cases"]),
            _fmt(g["chrf_pp"], "num"),
            _fmt(g["glossary_compliance"]),
            _fmt(g["entity_accuracy"]),
            _fmt(g["severe_per_100"], "num"),
            _fmt(g["thread_context_recall"]),
        ]
        if judged:
            cells += [
                _fmt(g.get("faithfulness")),
                _fmt(g.get("answer_correctness")),
                _fmt(g.get("mqm_severe_per_100"), "num"),
            ]
        lines.append("| " + " | ".join(cells) + " |")
    lines += [
        "",
        "## Notes",
        "",
        "- The golden set is synthetic and not yet reviewed by the glossary owner;"
        " variants of one template are correlated, so per-tag numbers on small groups are noisy.",
        "- chrF++ matches sacreBLEU 2.x (`CHRF(word_order=2)`) against a single reference;"
        " valid paraphrases score lower.",
        "- RAGAS-style metrics are computed without the ragas library: context precision/recall"
        " read the glossary block of the prompt actually sent; thread context recall checks"
        " whether the turns needed to translate correctly are inside the context window.",
    ]
    if judged:
        lines.append(
            "- The judge is the strong tier of the same config (self-judging bias possible) and"
            " is not deterministic; treat faithfulness, correctness and MQM as indicative,"
            " not human-rated."
        )
    else:
        lines.append(
            "- Mock provider: quality numbers only check the mechanics (masking, glossary,"
            " validators), not real translation quality."
        )
    if report.get("judge_errors"):
        lines.append(f"- Judge failures (not scored): {report['judge_errors']}.")
    return "\n".join(lines) + "\n"


def _rel(path: Path) -> str:
    p = path.resolve()
    return str(p.relative_to(cli.REPO)) if p.is_relative_to(cli.REPO) else str(p)


def dump(path: Path, cases: list[dict], results: dict, verdicts: dict[str, dict]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as f:
        for c in cases:
            tr = results[c["id"]]["response"]["translations"][0]
            row = {
                "id": c["id"],
                "src": c["text"],
                "mt": tr["text"],
                "ref": c.get("reference"),
                "status": tr["status"],
                "errors": verdicts.get(c["id"], {}).get("verdict", {}).get("errors"),
            }
            f.write(json.dumps(row, ensure_ascii=False) + "\n")


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(prog="xt_eval.benchmark")
    ap.add_argument("--cli", required=True)
    ap.add_argument(
        "--dataset",
        type=Path,
        default=cli.REPO / "eval" / "datasets" / "golden.automotive.vi-en.jsonl",
    )
    ap.add_argument("--label", help="tên báo cáo (mặc định: tên file cấu hình mô hình)")
    ap.add_argument("--out", type=Path, default=REPORTS)
    ap.add_argument("--no-judge", action="store_true", help="bỏ qua LLM chấm")
    ap.add_argument(
        "--dump",
        type=Path,
        help="ghi nguồn, bản dịch, tham chiếu và lỗi MQM từng ca ra JSONL để phân tích lỗi; "
        "chỉ dùng với dataset tổng hợp, đặt trong var/ (không commit)",
    )
    args = ap.parse_args(argv)

    config = cli.models_config()
    mock = cli.is_mock(config)
    label = args.label or ("mock" if mock else config.stem.replace(".", "-"))
    cases = cli.load_jsonl(args.dataset)
    index = load_glossary_index(cli.REPO / "glossary" / "data")

    with tempfile.TemporaryDirectory() as tmp:
        tel = Path(tmp) / "telemetry.jsonl"
        results = cli.run(
            args.cli,
            (cli.line(c["id"], to_request(c), c.get("history")) for c in cases),
            audit=True,
            telemetry=tel,
            config=config,
        )
        events = [json.loads(x) for x in tel.read_text(encoding="utf-8").splitlines() if x]

    scores = {c["id"]: score_case(c, results[c["id"]], index) for c in cases}

    judge_errors: dict[str, int] = {}
    verdicts: dict[str, dict] = {}
    if not mock and not args.no_judge:
        todo = [
            c
            for c in cases
            if "clarify_ok" not in scores[c["id"]]
            and results[c["id"]]["response"]["translations"][0]["status"] in OK_STATUSES
        ]
        verdicts = cli.judge(
            args.cli,
            ({"id": c["id"], "judge": judge_input(c, results[c["id"]], index)} for c in todo),
            config=config,
        )
        for c in todo:
            v = verdicts.get(c["id"], {})
            if "verdict" in v:
                apply_verdict(scores[c["id"]], v["verdict"], len(c["text"].split()))
            else:
                code = v.get("error", "missing")
                judge_errors[code] = judge_errors.get(code, 0) + 1

    report = {
        "meta": {
            "label": label,
            "dataset": _rel(args.dataset),
            "config": _rel(config),
        },
        "overall": aggregate(cases, scores, results),
        "latency": latency(events),
        "by_tag": {t: aggregate(g, scores, results) for t, g in by_tag(cases).items()},
        "judge_errors": judge_errors,
        "cases": [scores[c["id"]] for c in cases],
    }
    if args.dump:
        dump(args.dump, cases, results, verdicts)
    args.out.mkdir(parents=True, exist_ok=True)
    stem = args.out / f"benchmark-{label}"
    stem.with_suffix(".json").write_text(
        json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    stem.with_suffix(".md").write_text(markdown(report), encoding="utf-8")
    print(markdown(report))
    print(f"Đã ghi {stem}.json và {stem}.md", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
