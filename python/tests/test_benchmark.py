import pytest

from xt_eval import metrics
from xt_eval.benchmark import aggregate, apply_verdict, glossary_lines, latency, score_case

# Giá trị tham chiếu tính bằng sacreBLEU 2.4.3: CHRF(word_order=2).
SACREBLEU_CHRF_PP = [
    ("Can I test drive it this weekend?", "Can I book a test drive this weekend?", 64.943023),
    ("Deposit 5 million VND (refundable).", "A deposit of 5 million VND, refundable.", 57.884698),
    ("Chiều nay em gửi chị báo giá nhé.", "Em sẽ gửi báo giá cho chị chiều nay.", 39.322750),
    ("(hi) there!!", "hi there!", 48.703106),
    ("", "Something", 0.0),
]

INDEX = {
    ("vi", "en", "lái thử", "test drive"): "auto-001",
    ("en", "vi", "test drive", "lái thử"): "auto-001",
    ("vi", "en", "đặt cọc", "deposit"): "auto-002",
    ("en", "vi", "deposit", "đặt cọc"): "auto-002",
}


@pytest.mark.parametrize(("hyp", "ref", "want"), SACREBLEU_CHRF_PP)
def test_chrf_pp_matches_sacrebleu(hyp: str, ref: str, want: float) -> None:
    assert metrics.chrf(hyp, ref) == pytest.approx(want, abs=1e-6)


def test_corpus_chrf_sums_statistics() -> None:
    pairs = [(h, r) for h, r, _ in SACREBLEU_CHRF_PP]
    assert metrics.corpus_chrf(pairs) != pytest.approx(
        sum(metrics.chrf(h, r) for h, r in pairs) / len(pairs)
    )
    assert metrics.corpus_chrf([("a b c", "a b c")] * 3) == pytest.approx(100.0)


def test_context_precision_is_rank_aware() -> None:
    assert metrics.context_precision([True, False]) == 1.0
    assert metrics.context_precision([False, True]) == 0.5
    assert metrics.context_precision([False]) == 0.0
    assert metrics.context_precision([]) is None


def test_parse_prompt_reads_glossary_and_context_blocks() -> None:
    body = (
        "Glossary for this message:\n- lái thử => test drive\n- đặt cọc => deposit\n\n"
        "Context (reference only, do not translate):\n"
        'Thread summary: x\nP1 is addressed as "anh"\nP1: Hello\nP2: Chào em\n\n'
        "=== BEGIN MESSAGE n ===\nP3: not context\n=== END MESSAGE n ==="
    )
    v = metrics.parse_prompt(body)
    assert v.glossary == [("lái thử", "test drive"), ("đặt cọc", "deposit")]
    assert v.context_lines == 2 and v.summary
    assert metrics.thread_turns_present(5, 2) == [3, 4]


def test_percentile_interpolates() -> None:
    assert metrics.percentile([], 50) is None
    assert metrics.percentile([1, 2, 3, 4], 50) == 2.5
    assert metrics.percentile([10.0], 95) == 10.0


def _case(**kw: object) -> dict:
    base = {
        "id": "c",
        "source_lang": "vi",
        "target_lang": "en",
        "text": "Anh muốn lái thử",
        "reference": "I'd like a test drive",
        "tags": ["address_forms"],
        "expect": {"glossary": ["test drive"], "glossary_ids": ["auto-001"], "entities": []},
    }
    return base | kw


def _row(text: str, body: str = "", status: str = "translated", tier: str = "fast") -> dict:
    tr = {"lang": "en", "status": status, "text": text, "tier": tier}
    row: dict = {"response": {"translations": [tr]}}
    if body:
        row["audit"] = {"prompts": [{"prefix": "", "body": body}]}
    return row


def test_score_case_retrieval_and_thread_recall() -> None:
    body = (
        "Glossary for this message:\n- đặt cọc => deposit\n- lái thử => test drive\n\n"
        "Context (reference only, do not translate):\nP1: b\nP2: c\n\n=== BEGIN MESSAGE n ==="
    )
    case = _case(history=[{}, {}, {}])
    case["expect"] |= {"context_needed": [0, 2]}
    s = score_case(case, _row("I'd like a test drive", body), INDEX)
    assert s["glossary_hits"] == 1 and not s["severe"]
    assert s["ctx_precision"] == 0.5 and s["ctx_recall"] == 1.0
    assert s["thread_recall"] == 0.5  # lượt 0 đã trôi khỏi cửa sổ ngữ cảnh
    assert s["chrf"] == pytest.approx(100.0)


def test_failed_status_is_severe_and_clarify_is_separate() -> None:
    assert score_case(_case(), _row("Anh muốn lái thử", status="failed"), INDEX)["severe"]
    clar = _case(expect={"status": "needs_clarification"})
    s = score_case(clar, _row("", status="needs_clarification"), INDEX)
    assert s == {"id": "c", "status": "needs_clarification", "tier": "fast", "clarify_ok": True}


def test_verdict_and_aggregate() -> None:
    cases = [_case(id="a"), _case(id="b")]
    results = {"a": _row("I'd like a test drive"), "b": _row("I want to drive")}
    scores = {c["id"]: score_case(c, results[c["id"]], INDEX) for c in cases}
    apply_verdict(scores["a"], {"units": 2, "supported": 2, "correctness": 100, "errors": []}, 4)
    errs = [{"severity": "major", "category": "terminology", "span": "drive"}]
    apply_verdict(scores["b"], {"units": 2, "supported": 1, "correctness": 60, "errors": errs}, 4)
    g = aggregate(cases, scores, results)
    assert g["glossary_compliance"] == 0.5
    assert g["faithfulness"] == 0.75 and g["answer_correctness"] == 0.8
    assert g["mqm_severe_per_100"] == 50.0 and g["mqm_error_free"] == 0.5
    assert g["mqm_score"] == pytest.approx(100 * (1 - 5 / 8))


def test_glossary_lines_follow_direction() -> None:
    case = _case(source_lang="en", target_lang="vi")
    assert glossary_lines(case, INDEX) == ["test drive => lái thử"]


def test_latency_uses_only_numeric_telemetry() -> None:
    events = [
        {"name": "translate.done", "latency": 100.0, "ttft": 40.0},
        {"name": "translate.done", "latency": 300.0, "ttft": 60.0},
        {"name": "translate.cache_hit", "latency": 1.0},
    ]
    lat = latency(events)
    assert lat["model_calls"] == 2 and lat["cache_hits"] == 1
    assert lat["latency_p50_ms"] == 200.0 and lat["ttft_p95_ms"] == pytest.approx(59.0)
