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

Giao diện chat kiểu ứng dụng nhắn tin để test trực quan pipeline (`apps/chat-playground/`,
JS thuần dạng ES module, không cần build, không dependency). Chạy `make dev` rồi mở
http://127.0.0.1:8787.

- Mỗi "điện thoại" là một người tham gia với ngôn ngữ đọc riêng; gõ ở máy nào, máy khác thấy
  bản dịch hiện dần trong bong bóng, kèm nhãn "Đã dịch bởi AI" và "Xem bản gốc".
- Bấm vào bong bóng để xem bản gốc, sửa bản dịch (chỉ tôi / nhóm / đề xuất đơn vị) hoặc xem
  chi tiết kỹ thuật. Câu hỏi làm rõ chỉ hiện trên máy người gửi.
- Nút "Công cụ": mã hoá E2E (mock enclave), hội thoại cấm gửi ra ngoài, chi tiết kỹ thuật
  (tầng mô hình, cache, lỗi kiểm tra), đóng phiên, thêm người, tin mẫu golden và Inspector
  (văn bản sau khi che thực thể, dạng render vi/en, response).
- Chỉ dùng dữ liệu tổng hợp. Sửa front-end chỉ cần reload trình duyệt.
- Logic hiển thị có test: `make test` chạy `node --test` nếu máy có node.

### Dịch bằng mô hình thật

Mặc định `make dev` dùng provider giả (chỉ thay thuật ngữ glossary, không dịch cả câu).
Để nối một endpoint tương thích OpenAI:

```
cp config/models.local.example.toml config/models.local.toml   # không commit; sửa base_url, model
cp .env.example .env                                            # không commit; điền khoá API
make dev
```

`Makefile` tự nạp `.env` (khoá API và `XT_MODELS_CONFIG`) cho `make dev` và `make eval`;
`make redteam` luôn dùng provider giả.

Tin được gửi ra ngoài, nên chỉ dùng dữ liệu tổng hợp. Tầng dự phòng vẫn là provider nội bộ.
Nhãn đầu trang playground cho biết mỗi tầng đang dùng provider nào.
