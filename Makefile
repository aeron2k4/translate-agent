CARGO ?= cargo
VENV  ?= .venv
PY    := $(abspath $(VENV))/bin/python
RUFF  := $(abspath $(VENV))/bin/ruff

.PHONY: venv dev test lint fmt eval redteam cli golden

## make venv: tạo .venv và cài công cụ dev Python (pytest, ruff)
venv: $(VENV)/.installed

$(VENV)/.installed: python/requirements-dev.txt
	python3 -m venv $(VENV)
	$(PY) -m pip install -q --disable-pip-version-check -r python/requirements-dev.txt
	touch $@

## make dev: chạy local toàn bộ pipeline (không TEE, mock enclave, provider giả)
dev:
	$(CARGO) run --quiet -p devserver

## make test: unit + integration (Rust + Python)
test: venv
	$(CARGO) test --workspace --quiet
	cd python && $(PY) -m pytest -q

## make lint: rustfmt, clippy, ruff
lint: venv
	$(CARGO) fmt --all --check
	$(CARGO) clippy --workspace --all-targets --quiet -- -D warnings
	$(RUFF) check python
	$(RUFF) format --check python

fmt: venv
	$(CARGO) fmt --all
	$(RUFF) format python
	$(RUFF) check --fix python

cli:
	$(CARGO) build --quiet -p xt-cli

## make eval: tập kiểm thử chất lượng (glossary, số/thực thể, COMET nếu có XT_COMET_CMD)
eval: venv cli
	cd python && $(PY) -m xt_eval.eval --cli $(abspath target/debug/xt-cli)

## make redteam: prompt injection, PII, rò rỉ ngữ cảnh/log
redteam: venv cli
	cd python && $(PY) -m xt_eval.redteam --cli $(abspath target/debug/xt-cli)

## make golden: sinh lại eval/datasets/golden.*.jsonl từ mẫu viết tay trong eval/golden/
golden: venv
	cd python && $(PY) -m xt_eval.golden
