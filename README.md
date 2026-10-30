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

## Chat playground

Giao diện chat để test trực quan pipeline (`apps/chat-playground/`, HTML/JS thuần,
không cần build). Chạy `make dev` rồi mở http://127.0.0.1:8787.

- Mỗi cột là một người tham gia với ngôn ngữ đọc riêng; gõ ở cột nào, cột khác thấy bản dịch stream.
- Bấm vào một tin để xem Inspector: văn bản sau khi che thực thể (`⟦E1⟧`), dạng render vi/en,
  quyết định lọc ở client và response JSON.
- Công tắc mã hoá E2E (mock enclave) và "hội thoại cấm gửi ra ngoài"; nút đóng phiên, rời nhóm.
- Tin mẫu lấy từ golden dataset (dữ liệu tổng hợp). Không nhập dữ liệu thật vào playground.

Sửa file front-end chỉ cần reload trình duyệt; devserver đọc file từ đĩa mỗi request.
