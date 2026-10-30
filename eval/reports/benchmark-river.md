# Benchmark: river

- Dataset: eval/datasets/golden.automotive.vi-en.jsonl (231 cases, synthetic)
- Model config: `config/models.local.toml`
- LLM judge: strong tier of the same config

## Overall

| Metric | Value | Target |
| --- | --- | --- |
| chrF++ (corpus) | 71.5 | |
| Glossary compliance | 98.9% | ≥ 98% |
| Entity / number accuracy | 100.0% | 100% |
| Severe errors / 100 msgs (deterministic) | 0.0 | < 1 |
| Clarification accuracy | 100.0% | |
| Glossary context precision (RAGAS) | 100.0% | |
| Glossary context recall (RAGAS) | 100.0% | |
| Thread context recall | 94.1% | |
| Strong-tier share | 27.7% | 10–20% |
| Faithfulness (RAGAS, LLM judge) | 94.9% | |
| Answer correctness (LLM judge) | 92.3% | |
| MQM severe errors / 100 msgs | 10.8 | < 1 |
| MQM critical errors / 100 msgs | 2.3 | |
| MQM error-free messages | 79.8% | |
| MQM score (100 − penalty per word) | 85.8 | |

## Latency

| Metric | p50 | p95 | Target p50 / p95 |
| --- | --- | --- | --- |
| TTFT (ms) | 765.7 | 2243.5 | 400 / 1200 |
| Full translation (ms) | 809.2 | 2252.3 | – / 2000 |

Model calls: 221, cache hits: 0, errors: 0.

## By hard-case type

| Tag | n | chrF++ | Glossary | Entities | Severe/100 | Thread ctx | Faithful | Correct | MQM severe/100 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| unaccented | 124 | 70.8 | 100.0% | 100.0% | 0.0 | 100.0% | 92.8% | 90.2% | 14.7 |
| abbreviation | 52 | 69.4 | 100.0% | 100.0% | 0.0 | 100.0% | 92.5% | 89.6% | 14.6 |
| mixed | 24 | 67.8 | 100.0% | 100.0% | 0.0 | 100.0% | 94.4% | 94.6% | 4.2 |
| no_subject | 31 | 64.9 | 100.0% | 100.0% | 0.0 | 100.0% | 98.9% | 98.1% | 0.0 |
| cu | 7 | 71.7 | 100.0% | 100.0% | 0.0 | – | 100.0% | 97.1% | 0.0 |
| toi | 5 | 54.4 | 100.0% | 100.0% | 0.0 | – | 93.3% | 95.0% | 0.0 |
| vi_date | 20 | 90.7 | 100.0% | 100.0% | 0.0 | – | 100.0% | 98.2% | 0.0 |
| address_forms | 140 | 69.4 | 100.0% | 100.0% | 0.0 | 80.0% | 92.1% | 89.1% | 17.2 |
| context | 19 | 57.8 | 83.3% | 100.0% | 0.0 | 94.1% | 98.2% | 95.5% | 0.0 |
| en_vi | 33 | 67.4 | 100.0% | 100.0% | 0.0 | 80.0% | 100.0% | 97.4% | 0.0 |
| ambiguity | 18 | – | – | – | – | – | – | – | – |
| ambiguity_control | 11 | 66.0 | 100.0% | 100.0% | 0.0 | – | 81.8% | 80.9% | 54.5 |
| commitment | 14 | 68.4 | 100.0% | 100.0% | 0.0 | – | 97.6% | 95.7% | 7.1 |
| context_control | 2 | 97.1 | – | 100.0% | 0.0 | – | 100.0% | 100.0% | 0.0 |
| coreference | 4 | 56.0 | 50.0% | – | 0.0 | 100.0% | 91.7% | 87.5% | 0.0 |
| ellipsis | 2 | 89.4 | – | – | 0.0 | 100.0% | 100.0% | 100.0% | 0.0 |
| en_date | 3 | 47.0 | 100.0% | 100.0% | 0.0 | – | 100.0% | 93.3% | 0.0 |
| entity | 26 | 79.8 | 100.0% | 100.0% | 0.0 | – | 84.6% | 75.8% | 38.5 |
| long_context | 1 | 36.9 | – | – | 0.0 | 0.0% | 100.0% | 100.0% | 0.0 |
| money | 13 | 56.4 | 100.0% | 100.0% | 0.0 | – | 100.0% | 97.3% | 0.0 |
| money_range | 4 | 90.2 | 100.0% | 100.0% | 0.0 | – | 91.7% | 85.0% | 25.0 |
| money_slang | 8 | 78.6 | 100.0% | 100.0% | 0.0 | – | 100.0% | 97.5% | 0.0 |
| number | 10 | 49.8 | 100.0% | 100.0% | 0.0 | – | 88.3% | 80.0% | 40.0 |
| pii | 17 | 84.3 | 100.0% | 100.0% | 0.0 | – | 89.2% | 85.0% | 23.5 |
| question | 63 | 76.0 | 96.4% | 100.0% | 0.0 | 100.0% | 96.0% | 94.3% | 6.3 |
| time | 14 | 81.7 | 100.0% | 100.0% | 0.0 | – | 92.9% | 95.0% | 0.0 |

## Notes

- The golden set is synthetic and not yet reviewed by the glossary owner; variants of one template are correlated, so per-tag numbers on small groups are noisy.
- chrF++ matches sacreBLEU 2.x (`CHRF(word_order=2)`) against a single reference; valid paraphrases score lower.
- RAGAS-style metrics are computed without the ragas library: context precision/recall read the glossary block of the prompt actually sent; thread context recall checks whether the turns needed to translate correctly are inside the context window.
- The judge is the strong tier of the same config (self-judging bias possible) and is not deterministic; treat faithfulness, correctness and MQM as indicative, not human-rated.
