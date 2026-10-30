# Benchmark: river

- Dataset: eval/datasets/golden.automotive.vi-en.jsonl (231 cases, synthetic)
- Model config: `config/models.local.toml`
- LLM judge: strong tier of the same config

## Overall

| Metric | Value | Target |
| --- | --- | --- |
| chrF++ (corpus) | 71.1 | |
| Glossary compliance | 98.9% | ≥ 98% |
| Entity / number accuracy | 100.0% | 100% |
| Severe errors / 100 msgs (deterministic) | 0.0 | < 1 |
| Clarification accuracy | 100.0% | |
| Glossary context precision (RAGAS) | 100.0% | |
| Glossary context recall (RAGAS) | 100.0% | |
| Thread context recall | 94.1% | |
| Strong-tier share | 27.7% | 10–20% |
| Faithfulness (RAGAS, LLM judge) | 94.8% | |
| Answer correctness (LLM judge) | 92.5% | |
| MQM severe errors / 100 msgs | 12.7 | < 1 |
| MQM critical errors / 100 msgs | 1.4 | |
| MQM error-free messages | 79.8% | |
| MQM score (100 − penalty per word) | 87.3 | |

## Latency

| Metric | p50 | p95 | Target p50 / p95 |
| --- | --- | --- | --- |
| TTFT (ms) | 715.7 | 2150.9 | 400 / 1200 |
| Full translation (ms) | 716.8 | 2154.5 | – / 2000 |

Model calls: 221, cache hits: 0, errors: 0.

## By hard-case type

| Tag | n | chrF++ | Glossary | Entities | Severe/100 | Thread ctx | Faithful | Correct | MQM severe/100 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| unaccented | 124 | 70.4 | 100.0% | 100.0% | 0.0 | 100.0% | 94.1% | 91.8% | 13.8 |
| abbreviation | 52 | 68.0 | 100.0% | 100.0% | 0.0 | 100.0% | 94.3% | 90.9% | 14.6 |
| mixed | 24 | 68.4 | 100.0% | 100.0% | 0.0 | 100.0% | 97.9% | 96.7% | 0.0 |
| no_subject | 31 | 64.3 | 100.0% | 100.0% | 0.0 | 100.0% | 100.0% | 99.0% | 0.0 |
| cu | 7 | 72.5 | 100.0% | 100.0% | 0.0 | – | 100.0% | 97.9% | 0.0 |
| toi | 5 | 52.6 | 100.0% | 100.0% | 0.0 | – | 100.0% | 99.0% | 0.0 |
| vi_date | 20 | 89.7 | 100.0% | 100.0% | 0.0 | – | 100.0% | 99.0% | 0.0 |
| address_forms | 140 | 69.2 | 100.0% | 100.0% | 0.0 | 80.0% | 92.3% | 89.9% | 18.8 |
| context | 19 | 60.0 | 83.3% | 100.0% | 0.0 | 94.1% | 96.5% | 94.2% | 5.3 |
| en_vi | 33 | 66.8 | 100.0% | 100.0% | 0.0 | 80.0% | 98.8% | 96.3% | 3.7 |
| ambiguity | 18 | – | – | – | – | – | – | – | – |
| ambiguity_control | 11 | 63.3 | 100.0% | 100.0% | 0.0 | – | 81.8% | 81.8% | 54.5 |
| commitment | 14 | 68.6 | 100.0% | 100.0% | 0.0 | – | 100.0% | 97.5% | 0.0 |
| context_control | 2 | 97.1 | – | 100.0% | 0.0 | – | 100.0% | 100.0% | 0.0 |
| coreference | 4 | 57.4 | 50.0% | – | 0.0 | 100.0% | 91.7% | 88.8% | 0.0 |
| ellipsis | 2 | 89.4 | – | – | 0.0 | 100.0% | 100.0% | 100.0% | 0.0 |
| en_date | 3 | 46.7 | 100.0% | 100.0% | 0.0 | – | 100.0% | 95.0% | 0.0 |
| entity | 26 | 80.9 | 100.0% | 100.0% | 0.0 | – | 86.5% | 79.0% | 34.6 |
| long_context | 1 | 26.0 | – | – | 0.0 | 0.0% | 66.7% | 70.0% | 100.0 |
| money | 13 | 55.6 | 100.0% | 100.0% | 0.0 | – | 100.0% | 96.9% | 0.0 |
| money_range | 4 | 81.8 | 100.0% | 100.0% | 0.0 | – | 75.0% | 70.0% | 75.0 |
| money_slang | 8 | 75.2 | 100.0% | 100.0% | 0.0 | – | 83.3% | 83.7% | 50.0 |
| number | 10 | 43.7 | 100.0% | 100.0% | 0.0 | – | 86.7% | 84.0% | 30.0 |
| pii | 17 | 86.4 | 100.0% | 100.0% | 0.0 | – | 92.2% | 88.2% | 17.6 |
| question | 63 | 75.6 | 96.4% | 100.0% | 0.0 | 100.0% | 96.6% | 95.4% | 4.8 |
| time | 14 | 81.8 | 100.0% | 100.0% | 0.0 | – | 96.4% | 96.4% | 0.0 |

## Notes

- The golden set is synthetic and not yet reviewed by the glossary owner; variants of one template are correlated, so per-tag numbers on small groups are noisy.
- chrF++ matches sacreBLEU 2.x (`CHRF(word_order=2)`) against a single reference; valid paraphrases score lower.
- RAGAS-style metrics are computed without the ragas library: context precision/recall read the glossary block of the prompt actually sent; thread context recall checks whether the turns needed to translate correctly are inside the context window.
- The judge is the strong tier of the same config (self-judging bias possible) and is not deterministic; treat faithfulness, correctness and MQM as indicative, not human-rated.
