# Benchmark: mock

- Dataset: eval/datasets/golden.automotive.vi-en.jsonl (231 cases, synthetic)
- Model config: `config/models.toml`
- LLM judge: not run

## Overall

| Metric | Value | Target |
| --- | --- | --- |
| chrF++ (corpus) | 33.3 | |
| Glossary compliance | 100.0% | ≥ 98% |
| Entity / number accuracy | 100.0% | 100% |
| Severe errors / 100 msgs (deterministic) | 0.0 | < 1 |
| Clarification accuracy | 100.0% | |
| Glossary context precision (RAGAS) | 100.0% | |
| Glossary context recall (RAGAS) | 100.0% | |
| Thread context recall | 94.1% | |
| Strong-tier share | 23.9% | 10–20% |

## Latency

| Metric | p50 | p95 | Target p50 / p95 |
| --- | --- | --- | --- |
| TTFT (ms) | 0.3 | 0.5 | 400 / 1200 |
| Full translation (ms) | 0.4 | 0.6 | – / 2000 |

Model calls: 213, cache hits: 0, errors: 0.

## By hard-case type

| Tag | n | chrF++ | Glossary | Entities | Severe/100 | Thread ctx |
| --- | --- | --- | --- | --- | --- | --- |
| unaccented | 124 | 33.7 | 100.0% | 100.0% | 0.0 | 100.0% |
| abbreviation | 52 | 32.2 | 100.0% | 100.0% | 0.0 | 100.0% |
| mixed | 24 | 23.5 | 100.0% | 100.0% | 0.0 | 100.0% |
| no_subject | 31 | 38.4 | 100.0% | 100.0% | 0.0 | 100.0% |
| cu | 7 | 37.2 | 100.0% | 100.0% | 0.0 | – |
| toi | 5 | 37.0 | 100.0% | 100.0% | 0.0 | – |
| vi_date | 20 | 49.1 | 100.0% | 100.0% | 0.0 | – |
| address_forms | 140 | 30.6 | 100.0% | 100.0% | 0.0 | 80.0% |
| context | 19 | 18.0 | 100.0% | 100.0% | 0.0 | 94.1% |
| en_vi | 33 | 29.9 | 100.0% | 100.0% | 0.0 | 80.0% |
| ambiguity | 18 | – | – | – | – | – |
| ambiguity_control | 11 | 43.3 | 100.0% | 100.0% | 0.0 | – |
| commitment | 14 | 33.2 | 100.0% | 100.0% | 0.0 | – |
| context_control | 2 | 30.9 | – | 100.0% | 0.0 | – |
| coreference | 4 | 20.7 | 100.0% | – | 0.0 | 100.0% |
| ellipsis | 2 | 15.7 | – | – | 0.0 | 100.0% |
| en_date | 3 | 48.4 | 100.0% | 100.0% | 0.0 | – |
| entity | 26 | 38.9 | 100.0% | 100.0% | 0.0 | – |
| long_context | 1 | 7.1 | – | – | 0.0 | 0.0% |
| money | 13 | 39.0 | 100.0% | 100.0% | 0.0 | – |
| money_range | 4 | 58.4 | 100.0% | 100.0% | 0.0 | – |
| money_slang | 8 | 41.5 | 100.0% | 100.0% | 0.0 | – |
| number | 10 | 24.9 | 100.0% | 100.0% | 0.0 | – |
| pii | 17 | 35.4 | 100.0% | 100.0% | 0.0 | – |
| question | 63 | 29.1 | 100.0% | 100.0% | 0.0 | 100.0% |
| time | 14 | 32.1 | 100.0% | 100.0% | 0.0 | – |

## Notes

- The golden set is synthetic and not yet reviewed by the glossary owner; variants of one template are correlated, so per-tag numbers on small groups are noisy.
- chrF++ matches sacreBLEU 2.x (`CHRF(word_order=2)`) against a single reference; valid paraphrases score lower.
- RAGAS-style metrics are computed without the ragas library: context precision/recall read the glossary block of the prompt actually sent; thread context recall checks whether the turns needed to translate correctly are inside the context window.
- Mock provider: quality numbers only check the mechanics (masking, glossary, validators), not real translation quality.
