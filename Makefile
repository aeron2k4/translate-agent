CARGO ?= cargo
VENV  ?= .venv
PY    := $(abspath $(VENV))/bin/python
RUFF  := $(abspath $(VENV))/bin/ruff
NODE  ?= node

# Biến môi trường local (khoá API, cấu hình mô hình) từ .env — không commit, xem .env.example.
ifneq (,$(wildcard .env))
include .env
export $(shell sed -n 's/^[[:space:]]*\([A-Za-z_][A-Za-z0-9_]*\)[[:space:]]*=.*/\1/p' .env)
endif
WEB   := apps/chat-playground
# Chat playground là JS thuần, không dependency; máy không có node thì bỏ qua.
HAS_NODE := $(shell command -v $(NODE) 2>/dev/null)

.PHONY: venv dev test lint fmt eval redteam cli golden benchmark

## make venv: tạo .venv và cài công cụ dev Python (pytest, ruff)
venv: $(VENV)/.installed

$(VENV)/.installed: python/requirements-dev.txt
	python3 -m venv $(VENV)
	$(PY) -m pip install -q --disable-pip-version-check -r python/requirements-dev.txt
	touch $@

## make dev: chạy local toàn bộ pipeline (không TEE, mock enclave; mô hình theo XT_MODELS_CONFIG)
dev:
	$(CARGO) run --quiet -p devserver

## make test: unit + integration (Rust + Python + JS của playground)
test: venv
	$(CARGO) test --workspace --quiet
	cd python && $(PY) -m pytest -q
ifdef HAS_NODE
	cd $(WEB) && $(NODE) --test test/
else
	@echo "bỏ qua test JS của playground: không có node"
endif

## make lint: rustfmt, clippy, ruff, node --check
lint: venv
	$(CARGO) fmt --all --check
	$(CARGO) clippy --workspace --all-targets --quiet -- -D warnings
	$(RUFF) check python
	$(RUFF) format --check python
ifdef HAS_NODE
	@for f in $(WEB)/js/*.js $(WEB)/test/*.js; do $(NODE) --check $$f || exit 1; done
else
	@echo "bỏ qua kiểm tra JS của playground: không có node"
endif

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

## make benchmark: chấm golden dataset theo loại ca khó (chrF++, RAGAS, LLM chấm, độ trễ)
benchmark: venv cli
	cd python && $(PY) -m xt_eval.benchmark --cli $(abspath target/debug/xt-cli) $(BENCH_ARGS)

## make golden: sinh lại eval/datasets/golden.*.jsonl từ mẫu viết tay trong eval/golden/
golden: venv
	cd python && $(PY) -m xt_eval.golden
