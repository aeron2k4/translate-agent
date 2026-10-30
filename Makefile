CARGO ?= cargo
VENV  ?= .venv
PY    := $(abspath $(VENV))/bin/python
RUFF  := $(abspath $(VENV))/bin/ruff

.PHONY: venv test lint fmt

## make venv: tạo .venv và cài công cụ dev Python (pytest, ruff)
venv: $(VENV)/.installed

$(VENV)/.installed: python/requirements-dev.txt
	python3 -m venv $(VENV)
	$(PY) -m pip install -q --disable-pip-version-check -r python/requirements-dev.txt
	touch $@

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
