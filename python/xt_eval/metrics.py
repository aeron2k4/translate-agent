"""Chỉ số cho benchmark: chrF++ và các chỉ số kiểu RAGAS cho phần truy xuất.

- chrF++ cài lại theo sacreBLEU 2.x (char order 6 bỏ khoảng trắng, word order 2,
  beta 2, không làm trơn eps) để không thêm dependency; test so với giá trị tính bằng sacreBLEU.
- Context precision / recall (RAGAS) áp cho glossary: "tài liệu truy xuất" là các
  dòng "Glossary for this message:" trong prompt đã gửi; "cần thiết" là các id
  thuật ngữ mà golden kỳ vọng.
- Thread context recall: các lượt trước mà golden đánh dấu là cần để dịch đúng có
  nằm trong cửa sổ ngữ cảnh của prompt hay không.

Faithfulness và answer correctness cần LLM chấm (xem `prompts::judge`), được tính
trong `benchmark.py` từ verdict.
"""

from __future__ import annotations

import string
from collections import Counter
from collections.abc import Iterable, Sequence
from dataclasses import dataclass, field

CHAR_ORDER = 6
WORD_ORDER = 2
BETA = 2
_PUNCTS = frozenset(string.punctuation)


def _char_ngrams(text: str, n: int) -> Counter[str]:
    s = "".join(text.split())
    return Counter(s[i : i + n] for i in range(len(s) - n + 1))


def _words(text: str) -> list[str]:
    """Tách dấu câu dính ở đầu/cuối từ như sacreBLEU (chỉ một ký tự)."""
    out: list[str] = []
    for w in text.split():
        if len(w) == 1:
            out.append(w)
        elif w[-1] in _PUNCTS:
            out += [w[:-1], w[-1]]
        elif w[0] in _PUNCTS:
            out += [w[0], w[1:]]
        else:
            out.append(w)
    return out


def _word_ngrams(words: Sequence[str], n: int) -> Counter[tuple[str, ...]]:
    return Counter(tuple(words[i : i + n]) for i in range(len(words) - n + 1))


def chrf_stats(hyp: str, ref: str) -> list[int]:
    """[số n-gram giả thuyết, số n-gram tham chiếu, số khớp] cho từng bậc."""
    stats: list[int] = []
    for n in range(1, CHAR_ORDER + 1):
        h, r = _char_ngrams(hyp, n), _char_ngrams(ref, n)
        stats += [sum(h.values()), sum(r.values()), sum((h & r).values())]
    hw, rw = _words(hyp), _words(ref)
    for n in range(1, WORD_ORDER + 1):
        h, r = _word_ngrams(hw, n), _word_ngrams(rw, n)
        stats += [sum(h.values()), sum(r.values()), sum((h & r).values())]
    return stats


def chrf_from_stats(stats: Sequence[int]) -> float:
    """F-beta từ precision/recall trung bình qua các bậc có dữ liệu ở cả hai phía
    (mặc định của sacreBLEU 2.x), thang 0–100."""
    factor = BETA**2
    avg_prec, avg_rec, effective = 0.0, 0.0, 0
    for i in range(0, len(stats), 3):
        n_hyp, n_ref, n_match = stats[i : i + 3]
        if n_hyp > 0 and n_ref > 0:
            avg_prec += n_match / n_hyp
            avg_rec += n_match / n_ref
            effective += 1
    if not effective:
        return 0.0
    avg_prec /= effective
    avg_rec /= effective
    if not avg_prec + avg_rec:
        return 0.0
    return 100 * (1 + factor) * avg_prec * avg_rec / (factor * avg_prec + avg_rec)


def chrf(hyp: str, ref: str) -> float:
    return chrf_from_stats(chrf_stats(hyp, ref))


def corpus_chrf(pairs: Iterable[tuple[str, str]]) -> float:
    """chrF++ mức corpus: cộng thống kê của mọi câu rồi mới tính điểm."""
    total = [0] * (3 * (CHAR_ORDER + WORD_ORDER))
    for hyp, ref in pairs:
        total = [a + b for a, b in zip(total, chrf_stats(hyp, ref), strict=True)]
    return chrf_from_stats(total)


def context_precision(retrieved_relevant: Sequence[bool]) -> float | None:
    """RAGAS context precision: trung bình precision@k tại các vị trí liên quan."""
    hits, total = 0, 0.0
    for k, relevant in enumerate(retrieved_relevant, start=1):
        if relevant:
            hits += 1
            total += hits / k
    return total / hits if hits else (None if not retrieved_relevant else 0.0)


def recall(needed: Iterable[str | int], present: Iterable[str | int]) -> float | None:
    need = set(needed)
    return len(need & set(present)) / len(need) if need else None


@dataclass
class PromptView:
    """Phần body prompt đã gửi provider, tách theo khối."""

    glossary: list[tuple[str, str]] = field(default_factory=list)
    context_lines: int = 0
    summary: bool = False


def parse_prompt(body: str) -> PromptView:
    v = PromptView()
    section = None
    for raw in body.splitlines():
        line = raw.strip()
        if not line:
            section = None
            continue
        if line == "Glossary for this message:":
            section = "glossary"
        elif line.startswith("Context (reference only"):
            section = "context"
        elif line.startswith("=== BEGIN MESSAGE"):
            break
        elif section == "glossary" and line.startswith("- ") and " => " in line:
            src, tgt = line[2:].split(" => ", 1)
            v.glossary.append((src, tgt))
        elif section == "context":
            if line.startswith("Thread summary:"):
                v.summary = True
            elif " is addressed as " not in line and ": " in line:
                v.context_lines += 1
    return v


def thread_turns_present(history_len: int, context_lines: int) -> list[int]:
    """Bộ nhớ luồng giữ các tin gần nhất: các lượt có chỉ số ≥ len - số dòng ngữ cảnh."""
    return list(range(max(0, history_len - context_lines), history_len))


def percentile(values: Sequence[float], p: float) -> float | None:
    """Percentile theo nội suy tuyến tính (như numpy mặc định)."""
    if not values:
        return None
    s = sorted(values)
    pos = (len(s) - 1) * p / 100
    lo = int(pos)
    hi = min(lo + 1, len(s) - 1)
    return s[lo] + (s[hi] - s[lo]) * (pos - lo)
