from dataclasses import replace
from pathlib import Path

import pytest

from xt_admin.glossary import GlossaryError, Term, apply_changes, dump, load, save, validate
from xt_common.anonymize import anonymize

DATA = Path(__file__).resolve().parents[2] / "glossary" / "data"
AUTOMOTIVE = DATA / "automotive.vi-en.json"


def test_repo_glossaries_are_valid() -> None:
    files = list(DATA.glob("*.json"))
    assert files
    for f in files:
        assert validate(load(f)) == [], f


def test_dump_round_trips_repo_file_byte_for_byte() -> None:
    assert dump(load(AUTOMOTIVE)) == AUTOMOTIVE.read_text(encoding="utf-8")


def test_apply_changes_bumps_version() -> None:
    g = load(AUTOMOTIVE)
    new = apply_changes(g, upserts=[Term("auto-999", {"vi": "cần gạt mưa", "en": "wiper"})])
    assert new.version == g.version + 1
    assert new.term("auto-999") is not None
    with pytest.raises(GlossaryError):
        apply_changes(new, upserts=[new.term("auto-999")])


def test_rejects_duplicates_and_raw_examples() -> None:
    g = load(AUTOMOTIVE)
    dup = Term("auto-998", {"vi": "Lái thử", "en": "trial"})
    with pytest.raises(GlossaryError, match="trùng thuật ngữ vi"):
        apply_changes(g, upserts=[dup])
    raw = Term("auto-997", {"vi": "gạt mưa", "en": "wiper"}, example="Gọi em 0901234567 nhé")
    with pytest.raises(GlossaryError, match="ẩn danh"):
        apply_changes(g, upserts=[raw])


def test_save_is_atomic_and_validated(tmp_path: Path) -> None:
    g = load(AUTOMOTIVE)
    out = tmp_path / "g.json"
    save(g, out)
    assert load(out) == g
    with pytest.raises(GlossaryError):
        save(replace(g, version=0), out)
    assert load(out) == g


def test_anonymize() -> None:
    assert (
        anonymize("Gọi 0901 234 567 hoặc mail a@x.vn, hỏi @lan, giá 5 củ")
        == "Gọi ⟨số⟩ hoặc mail ⟨email⟩, hỏi ⟨mention⟩, giá ⟨số⟩ củ"
    )
