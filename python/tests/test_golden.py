import json
import re

import pytest

from xt_admin.glossary import Glossary, Term
from xt_eval.golden import (
    GOLDEN_DIR,
    GoldenError,
    abbreviate,
    build_all,
    contains_phrase,
    fold,
    generate,
)


def test_committed_golden_matches_templates() -> None:
    outputs = build_all()
    assert outputs
    for path, content in outputs.items():
        assert path.read_text(encoding="utf-8") == content, f"{path.name}: chạy `make golden`"


def test_every_slot_value_is_used() -> None:
    for path, content in build_all().items():
        stem = path.name.removeprefix("golden.").removesuffix(".jsonl")
        spec = json.loads((GOLDEN_DIR / f"{stem}.json").read_text(encoding="utf-8"))
        rows = [json.loads(line) for line in content.splitlines()]
        texts = "\n".join(r["text"] for r in rows)
        used = {n for t in spec["templates"] for n in re.findall(r"\{(\w+)\}", t[t["from"]])}
        for kind in used & spec["slots"].keys():
            for v in spec["slots"][kind]:
                src = "en" if kind.endswith("_en") or kind == "usd" else "vi"
                assert v[src] in texts, f"giá trị slot {kind}={v[src]} chưa được dùng"


def test_fold_and_abbreviate() -> None:
    assert fold("Đặt cọc 5 củ nhé Anh") == "Dat coc 5 cu nhe Anh"
    table = {"không": "ko", "anh": "a", "được": "đc"}
    assert abbreviate("Anh làm được không?", table) == "A làm đc ko?"
    assert contains_phrase("a muon lai thu xe", "lái thử")
    assert not contains_phrase("pinch", "pin")


GLOSSARY = Glossary(
    domain="automotive",
    langs=("vi", "en"),
    version=1,
    terms=(Term("auto-001", {"vi": "lái thử", "en": "test drive"}),),
)


def spec(reference: str) -> dict:
    return {
        "address_pairs": [["anh", "em"]],
        "abbreviations": {"em": "e"},
        "slots": {"time": [{"vi": "9h30", "en": "9:30"}]},
        "templates": [
            {"id": "t1", "from": "vi", "vi": "{Self} muốn lái thử lúc {time}.", "en": reference}
        ],
    }


def test_generates_variants_with_independent_expectations() -> None:
    rows = generate(spec("I'd like a test drive at {time}."), GLOSSARY)
    assert [r["meta"]["variant"] for r in rows] == ["base", "unaccented", "chat"]
    base, unaccented, chat = rows
    assert base["text"] == "Em muốn lái thử lúc 9h30."
    assert unaccented["text"] == "Em muon lai thu luc 9h30."
    assert chat["text"] == "E muon lai thu luc 9h30."
    assert {"unaccented", "abbreviation"} <= set(chat["tags"])
    assert base["reference"] == "I'd like a test drive at 9:30."
    assert base["expect"] == {
        "entities": ["9:30"],
        "glossary": ["test drive"],
        "glossary_ids": ["auto-001"],
    }
    assert "address_forms" in base["tags"] and "unaccented" in unaccented["tags"]


def dialogue_spec(needs: list[int]) -> dict:
    return {
        **spec("unused"),
        "templates": [],
        "speakers": {"customer": "vi", "advisor": "en"},
        "dialogues": [
            {
                "id": "d1",
                "tags": ["coreference"],
                "history": [
                    {"speaker": "customer", "text": "Chị muốn lái thử xe điện."},
                    {"speaker": "advisor", "text": "Sure, which day?"},
                ],
                "speaker": "customer",
                "text": "Thứ bảy nhé em.",
                "reference": "Saturday, please.",
                "needs": needs,
            }
        ],
    }


def test_dialogues_carry_history_sender_and_needed_turns() -> None:
    rows = generate(dialogue_spec([1]), GLOSSARY)
    assert [r["meta"]["variant"] for r in rows] == ["base", "unaccented"]
    base = rows[0]
    assert base["sender_id"] == "customer" and base["target_lang"] == "en"
    assert base["history"][1] == {"sender_id": "advisor", "lang": "en", "text": "Sure, which day?"}
    assert base["expect"]["context_needed"] == [1]
    assert "context" in base["tags"]


def test_dialogue_needs_must_point_to_history() -> None:
    with pytest.raises(GoldenError, match="needs"):
        generate(dialogue_spec([2]), GLOSSARY)


def test_rejects_reference_that_breaks_the_glossary() -> None:
    with pytest.raises(GoldenError, match="auto-001"):
        generate(spec("I'd like to try driving it at {time}."), GLOSSARY)


def test_rejects_reference_missing_an_entity() -> None:
    with pytest.raises(GoldenError, match="9:30"):
        generate(spec("I'd like a test drive this morning."), GLOSSARY)
