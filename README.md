# X Translate Agent

A real-time, two-way chat translation agent for the Group X ecosystem (Xe X car sales,
ride-hailing, partners). It keeps conversation context, uses approved industry terminology,
lets people correct translations (human-in-the-loop), and is designed to work with
end-to-end encrypted chats by translating only inside a trusted execution environment (TEE).

**MVP scope:** Vietnamese ↔ English, automotive domain, 1-1 and group chats (≤ 50 people),
text only.

> **Status:** prototype. By default everything runs against a mock model provider and a mock
> enclave, so it works offline and never sends text anywhere. Connecting a real model is opt-in
> (see [Using a real model](#using-a-real-model)).

## How a message is translated

The pipeline runs in this order for every message:

1. **Client-side filter**: skip messages already in the reader's language, emoji-only messages,
   URLs and bare codes. Detect the language, including Vietnamese typed without diacritics and
   mixed Vietnamese–English.
2. **Context**: the 6–10 most recent messages, a thread summary, how members address each
   other (anh/chị/em…), and the domain label. Vietnamese kinship words mean both "I" and
   "you", so address forms are inferred from vocatives ("anh ơi…", "…nhé em") and the prompt
   states who is the sender and who is the reader. They are kept with the same TTL as the
   messages, even after those messages leave the window.
3. **Glossary lookup** before calling the model (up to ~20 entries). The model never calls
   tools on the hot path.
4. **Ambiguity check**: ask the sender privately, only when there are at least two reasonable
   readings *and* it affects a price, date, quantity, address or commitment
   (e.g. "giá 500": 500 thousand or 500 million?).
5. **Entity masking**: numbers, money, plates, @mentions, product codes, ID and bank numbers,
   and phone numbers become placeholders (`⟦E1⟧`); the prompt names each token's type (not its
   value) so the model still translates the words around it ("Xe ⟦E1⟧ chỗ" → "⟦E1⟧-seat car").
   The text is then translated with streaming,
   and the entities are restored in the target language's format
   ("5 củ" → "5 million VND", "9h30" → "9:30").
6. **Post-translation checks**: entities and glossary terms must match. On failure, retry
   once on the strong model tier.
7. **One translation per target language**, shared by every reader of that language.

If translation fails, readers always see the original text with a "Retry translation" action.
Translation errors never block a message from being displayed.

## Architecture

| Area | Language | Why |
| --- | --- | --- |
| Hot path and anything that touches plaintext | **Rust** (`crates/`) | Runs inside the TEE; compiles for server, mobile and WASM |
| Glossary admin, review queue, eval, red team | **Python ≥ 3.12** (`python/`) | Never touches real plaintext: only term pairs, anonymized examples and synthetic data |
| Chat playground | **Plain JS** (`apps/chat-playground/`) | Dev tool, ES modules, no dependencies, no build step |

Python talks to Rust through the `xt-cli` binary (JSONL over stdin/stdout), not FFI. The
glossary is versioned JSON written by Python and read by Rust.

```
crates/
  textnorm/        Lowercasing, Vietnamese diacritic folding, tokenizing
  telemetry/       Events with latency, tokens, error codes and hashed ids only
  validators/      Entity masking (⟦E1⟧), target-language rendering, post-translation checks
  glossary/        Versioned per-domain glossary lookup
  client-core/     Fast filter, language detection, i18n catalogs
  model-router/    Provider trait; fast / strong / fallback tiers; TOML config
  prompts/         Domain system prompts and approved few-shot examples
  phrase-cache/    Encrypted cache of short, common phrases
  translator/      Runs in the TEE: context, ambiguity, pipeline, human corrections
  xt-cli/          JSONL binary used by eval and red team
  devserver/       `make dev`: HTTP API, mock enclave, chat playground
python/
  xt_admin/glossary/   Validate glossary files and bump versions
  xt_admin/review/     Queue of user-suggested terms awaiting owner approval
  xt_eval/             Eval and red-team runners, golden dataset generator
apps/chat-playground/  Messaging-app style UI for manual testing
glossary/data/         Approved glossaries (versioned JSON)
config/models.toml     Model routing (the only place model names live)
eval/
  golden/              Hand-written golden templates (references and expected entities)
  datasets/            Test sets: smoke set + generated golden set (synthetic)
  redteam/             Prompt injection, PII and offensive-content cases
```

## Getting started

Requirements: Rust (edition 2024, stable toolchain with `rustfmt` and `clippy`),
Python ≥ 3.12, and optionally Node.js (for the playground's JS tests).

```sh
make venv    # one-time: create .venv with pytest and ruff
make test    # cargo test + pytest + node --test (if node is installed)
make dev     # start the dev server at http://127.0.0.1:8787
```

| Command | What it does |
| --- | --- |
| `make venv` | Create `.venv` and install Python dev tools (pytest, ruff) |
| `make dev` | Run the whole pipeline locally (no TEE, mock enclave) plus the chat playground |
| `make test` | `cargo test --workspace`, `pytest`, and the playground's `node --test` |
| `make lint` | `cargo fmt --check`, `clippy -D warnings`, `ruff check`, `ruff format --check`, `node --check` |
| `make eval` | Quality eval: entity and number accuracy, glossary compliance, clarifications, COMET if configured |
| `make redteam` | Prompt-injection, PII and context-leak checks on the prompts actually sent to the model |
| `make golden` | Regenerate `eval/datasets/golden.*.jsonl` from `eval/golden/*.json` |
| `make benchmark` | Score the golden set per hard-case type: chrF++, RAGAS-style metrics, LLM judge (MQM), latency |

The dev server address can be changed with `XT_DEV_ADDR` (default `127.0.0.1:8787`).

## Chat playground

Run `make dev` and open http://127.0.0.1:8787.

- Each **phone** is one participant with their own reading language (by default a Vietnamese
  customer and an English-speaking sales advisor). Type on one phone and watch the translation
  stream into the bubble on the other, labelled "Translated by AI" with a one-tap
  "View original".
- **Tap a bubble** to view the original, edit the translation (only me / this group / suggest to
  my unit) or open technical details. Clarification questions appear only on the sender's phone.
- **Tools** drawer: mock E2E encryption, "no external models" for sensitive chats, technical
  details (model tier, cache, check issues), close the session, add participants, golden sample
  messages, and an inspector showing the masked text, entity renderings and the raw response.

Use synthetic data only. The dev server reads the front-end files from disk on every request,
so edits only need a browser reload.

## Using a real model

By default the mock provider only swaps glossary terms and entities; it does not really translate.
To connect an OpenAI-compatible endpoint (`/v1/chat/completions` with streaming):

```sh
cp config/models.local.example.toml config/models.local.toml  # git-ignored: set base_url and model
cp .env.example .env                                           # git-ignored: set the API key
make dev
```

- The Makefile loads `.env` automatically; `XT_MODELS_CONFIG` selects the model config for
  `make dev` and `make eval`. `make redteam` always uses the mock provider.
- API keys live only in environment variables; config files name the variable
  (`api_key_env`), never the key itself.
- The fallback tier must run internally (`external = false`); chats marked "no external models"
  only use it.
- The router refuses to start unless every provider is marked `approved` and `no_retention`.
  For production these flags mean the provider has passed security/legal review and signed a
  no-retention agreement. With an external endpoint, message text leaves your machine, so use
  synthetic data only.

The playground header shows which provider each tier uses.

## Evaluation

```sh
make eval      # gates: glossary ≥ 98%, < 1 severe error per 100 messages, hard-case coverage
```

To inspect individual results, dump source, translation and reference per case:

```sh
PYTHONPATH=python .venv/bin/python -m xt_eval.eval --cli target/debug/xt-cli --dump var/eval/run.jsonl
```

- Hard cases are always covered: unaccented Vietnamese, chat abbreviations ("ko", "đc", "a/c"),
  mixed Vietnamese–English, missing subjects, money slang ("5 củ" = 5 million, "2 tỏi" =
  2 billion), Vietnamese dates, and address forms (anh/chị/em).
- The golden set is generated from hand-written templates in `eval/golden/`. Edit the templates
  and run `make golden`; never edit `golden.*.jsonl` directly, and never change an expectation
  to match the code.
- Set `XT_COMET_CMD` to a command that reads `{src, mt, ref}` JSONL on stdin and prints a score
  to enable the COMET gate (target ≥ 0.85).

## Benchmark

```sh
make benchmark                                    # model from XT_MODELS_CONFIG (.env)
make benchmark XT_MODELS_CONFIG=config/models.toml   # mock provider: mechanics only
```

Scores the golden set (single messages plus multi-turn dialogues whose last message needs
earlier turns) and writes `eval/reports/benchmark-<label>.{json,md}`, overall and per
hard-case type. Reports hold numbers and case ids only, no text.

| Group | Metrics |
| --- | --- |
| Reference-based | chrF++ (identical to sacreBLEU 2.x `CHRF(word_order=2)`), glossary compliance, entity/number accuracy, deterministic severe errors per 100 messages, clarification accuracy |
| Retrieval (RAGAS-style) | Glossary context precision (rank-aware) and recall, read from the prompt actually sent; thread context recall: are the turns needed for a correct translation inside the context window |
| LLM judge | Faithfulness (share of atomic information units supported by the source), answer correctness (0–100), MQM errors (minor / major / critical) |
| Latency | TTFT and full-translation p50 / p95 from telemetry |

The RAGAS-style metrics are implemented in `python/xt_eval/metrics.py` rather than with the
`ragas` library, so that every model call goes through `crates/model-router`. The judge runs on
the strong tier of the same config (`xt-cli --judge`, prompt in
`crates/prompts/templates/judge.txt`), so judge scores carry self-judging bias. With the mock
provider the judge is skipped. Extra flags go through `BENCH_ARGS`, e.g.
`make benchmark BENCH_ARGS="--no-judge --label quick"`.

## Glossary and review

```sh
PYTHONPATH=python .venv/bin/python -m xt_admin.glossary validate glossary/data/*.json
PYTHONPATH=python .venv/bin/python -m xt_admin.review var/review/queue.json ingest < var/review/suggestions.jsonl
PYTHONPATH=python .venv/bin/python -m xt_admin.review var/review/queue.json list
```

"Suggest to my unit" corrections from the playground are written to
`var/review/suggestions.jsonl` (term pair and anonymized example only). Once the glossary owner
approves a suggestion, the glossary version is bumped and related cache entries are invalidated.

## Security and privacy

- Message content is never logged, not even at debug level or in test snapshots. Logs contain
  only latency, token counts, error codes and hashed ids.
- Plaintext exists only in the memory of the `translator` service inside the TEE: never on disk,
  in queues or in other services.
- Thread memory expires after at most 24 hours and is wiped when a session closes or a member
  leaves.
- The glossary and review queue store only term pairs and anonymized example sentences.
- The phrase cache holds short, common, PII-free phrases only, keyed by
  hash(normalized text + language pair + domain + glossary version).
- Message text is always treated as data to translate, inside nonce-delimited blocks.
  Instructions such as "ignore previous instructions" are translated literally.
- No real data in tests or fixtures; all datasets are synthetic.
- Designed to comply with Vietnam's Personal Data Protection Law (91/2025/QH15) and
  Decree 356/2025/NĐ-CP.

## Targets

- Latency: time to first token p50 ≤ 400 ms, p95 ≤ 1.2 s; messages under 50 words complete
  within 2 s at p95.
- Quality: COMET (vi↔en) ≥ 0.85, glossary compliance ≥ 98%, < 1 severe (MQM) error per
  100 messages.
