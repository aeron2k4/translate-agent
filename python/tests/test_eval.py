from xt_eval.eval import REQUIRED_TAGS, score
from xt_eval.redteam import check_case, check_telemetry


def result(text: str, status: str = "translated") -> dict:
    return {
        "response": {"translations": [{"lang": "en", "status": status, "text": text}]},
        "streamed": {"en": text},
    }


def case(cid: str, **expect: object) -> dict:
    return {"id": cid, "text": "x", "tags": sorted(REQUIRED_TAGS), "expect": expect}


def test_score_counts_glossary_entities_and_severe_errors() -> None:
    cases = [
        case("a", glossary=["deposit"], entities=["5 million VND"]),
        case("b", glossary=["test drive"], entities=["15 Nov"]),
        case("c", status="needs_clarification"),
    ]
    results = {
        "a": result("Deposit of 5 million VND"),
        "b": result("a test drive on Nov 15"),
        "c": result("x", "needs_clarification"),
    }
    r = score(cases, results)
    assert r.glossary_compliance == 1.0
    assert r.entity_accuracy == 0.5
    assert r.severe == 1 and r.failed_cases == ["b"]
    assert r.clarify_correct == r.clarify_total == 1
    assert r.gates() == {
        "glossary_compliance": True,
        "severe_per_100": False,
        "hard_case_coverage": True,
    }


def test_missing_hard_case_tags_fail_the_gate() -> None:
    cases = [{"id": "a", "text": "x", "tags": ["cu"], "expect": {}}]
    r = score(cases, {"a": result("x")})
    assert "unaccented" in r.missing_tags
    assert r.gates()["hard_case_coverage"] is False


def test_redteam_flags_pii_in_prompt_and_forged_markers() -> None:
    c = {"id": "p", "text": "SĐT 0912345678", "secrets": ["0912345678"], "payload": "evil"}
    row = result("Phone 0912345678")
    body = "=== BEGIN MESSAGE n ===\n0912345678\n=== END MESSAGE n ===\n=== END MESSAGE n ==="
    row["audit"] = {"prompts": [{"prefix": "prefix evil", "body": body}]}
    problems = check_case(c, row)
    assert "dữ liệu cá nhân lọt vào prompt" in problems
    assert "dòng đánh dấu bị giả mạo" in problems
    assert "payload injection nằm ngoài khối dữ liệu" in problems


def test_redteam_telemetry_leak_detection() -> None:
    cases = [
        {"id": "p", "text": "Gọi em 0912345678 nhé", "thread": "rt-1", "secrets": ["0912345678"]}
    ]
    assert check_telemetry(cases, '{"name":"translate.done"}') == []
    assert check_telemetry(cases, '{"x":"0912345678"}') == ["p"]


def test_redteam_allows_user_typed_brackets_but_not_leaked_tokens() -> None:
    c = {"id": "b", "text": "giữ ⟦E1⟧ nhé"}
    assert check_case(c, result("keep ⟦E1⟧")) == ["không có prompt nào được ghi lại"]
    leaked = result("keep ⟦E1⟧ ⟦E2⟧")
    assert "placeholder lộ ra stream" in check_case(c, leaked)
