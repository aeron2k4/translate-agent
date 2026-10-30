# X Translate Agent

AI Agent dịch tin nhắn realtime hai chiều cho hệ sinh thái Tập đoàn X.
MVP: Việt↔Anh, mảng ô tô (Xe X), chat 1-1 và nhóm ≤ 50 người, chỉ văn bản.

## Stack

- **Rust** (`crates/`): đường nóng và mọi thứ chạm bản rõ — translator (trong TEE),
  validators, model-router, prompts, phrase-cache, client-core.
- **Python** (`python/`): ngoài đường nóng — quản trị glossary, review, eval, redteam.
  Python không chạm bản rõ của tin thật; gọi Rust qua binary `xt-cli` (JSONL).

## Lệnh

```
make venv         # tạo .venv, cài pytest + ruff (một lần)
make dev          # chạy local toàn bộ service (không TEE, dùng mock enclave)
make test         # cargo test + pytest
make eval         # chạy tập kiểm thử chất lượng
make redteam      # chạy bộ prompt injection / PII
make lint         # rustfmt, clippy, ruff
```
