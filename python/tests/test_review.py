import shutil
from pathlib import Path

import pytest

from xt_admin.glossary import load
from xt_admin.review import ReviewError, ReviewQueue, Suggestion

DATA = Path(__file__).resolve().parents[2] / "glossary" / "data" / "automotive.vi-en.json"
PROPOSER = "a1b2c3d4e5f60718293a4b5c"
OWNER = "owner-1"


@pytest.fixture
def setup(tmp_path: Path) -> tuple[ReviewQueue, Path]:
    glossary = tmp_path / "automotive.vi-en.json"
    shutil.copy(DATA, glossary)
    return ReviewQueue(tmp_path / "queue.json", owners={"automotive": {OWNER}}), glossary


def suggestion(**kw: str) -> Suggestion:
    base = {
        "domain": "automotive",
        "source_lang": "vi",
        "target_lang": "en",
        "source_term": "chạy thử",
        "target_term": "test ride",
        "proposer": PROPOSER,
        "example": "Anh muốn chạy thử, gọi ⟨phone⟩",
    }
    return Suggestion(**{**base, **kw})


def test_approve_adds_term_and_bumps_version(setup: tuple[ReviewQueue, Path]) -> None:
    queue, glossary = setup
    before = load(glossary).version
    s = queue.submit(suggestion())
    assert queue.approve(s.id, OWNER, glossary) == before + 1
    g = load(glossary)
    assert any(t.forms["vi"] == "chạy thử" and t.id == "auto-026" for t in g.terms)
    assert queue.pending() == []


def test_approve_updates_existing_term(setup: tuple[ReviewQueue, Path]) -> None:
    queue, glossary = setup
    s = queue.submit(suggestion(source_term="lái thử", target_term="test ride"))
    queue.approve(s.id, OWNER, glossary)
    term = load(glossary).term("auto-001")
    assert term is not None and term.forms["en"] == "test ride"


def test_only_owners_can_decide(setup: tuple[ReviewQueue, Path]) -> None:
    queue, glossary = setup
    s = queue.submit(suggestion())
    with pytest.raises(ReviewError, match="chủ sở hữu"):
        queue.approve(s.id, "someone-else", glossary)
    queue.reject(s.id, OWNER, "duplicate")
    with pytest.raises(ReviewError, match="đã được xử lý"):
        queue.approve(s.id, OWNER, glossary)


def test_rejects_raw_examples_and_unhashed_proposers(setup: tuple[ReviewQueue, Path]) -> None:
    queue, _ = setup
    with pytest.raises(ReviewError, match="ẩn danh"):
        queue.submit(suggestion(example="Gọi em 0901234567"))
    with pytest.raises(ReviewError, match="hash"):
        queue.submit(suggestion(proposer="user-42"))


def test_duplicates_merge_and_queue_persists(
    setup: tuple[ReviewQueue, Path], tmp_path: Path
) -> None:
    queue, _ = setup
    a = queue.submit(suggestion())
    b = queue.submit(suggestion(source_term="Chạy thử"))
    assert a.id == b.id
    reopened = ReviewQueue(tmp_path / "queue.json", owners={})
    assert [s.id for s in reopened.pending()] == [a.id]
