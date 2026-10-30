//! Provider gọi endpoint tương thích OpenAI (`/chat/completions`, stream SSE).
//!
//! Tuỳ chọn trong `[providers.<tên>.options]` của `config/models*.toml`:
//! - `base_url`: ví dụ `https://host/v1`.
//! - `api_key_env`: TÊN biến môi trường chứa khoá (không đặt khoá trong file cấu hình).
//! - `disable_thinking = "true"`: tắt chế độ suy nghĩ của mô hình kiểu Qwen3
//!   (`chat_template_kwargs.enable_thinking = false`).
//!
//! Lỗi chỉ trả mã (`ProviderError`), không mang nội dung phản hồi.

use crate::{
    BoxFuture, Completion, ModelRequest, OnDelta, Provider, ProviderConfig, ProviderError, Usage,
};
use serde_json::{Value, json};
use std::time::Duration;

pub struct OpenAiCompat {
    http: reqwest::Client,
    url: String,
    api_key: Option<String>,
    disable_thinking: bool,
}

impl OpenAiCompat {
    /// Thiếu khoá thì vẫn dựng được, nhưng mọi lời gọi trả `Refused` để router
    /// chuyển sang tầng dự phòng. Dùng `missing_key` để báo lúc khởi động.
    pub fn from_config(pc: &ProviderConfig) -> Self {
        let base = pc.options.get("base_url").map_or("", String::as_str);
        Self {
            http: reqwest::Client::new(),
            url: format!("{}/chat/completions", base.trim_end_matches('/')),
            api_key: Self::key_env(pc).and_then(|name| std::env::var(name).ok()),
            disable_thinking: pc
                .options
                .get("disable_thinking")
                .is_some_and(|v| v == "true"),
        }
    }

    fn key_env(pc: &ProviderConfig) -> Option<&str> {
        pc.options.get("api_key_env").map(String::as_str)
    }

    /// Tên biến môi trường chứa khoá nếu nó chưa được đặt.
    pub fn missing_key(pc: &ProviderConfig) -> Option<&str> {
        Self::key_env(pc).filter(|name| std::env::var(name).is_err())
    }

    fn payload(&self, model: &str, req: &ModelRequest) -> Value {
        let mut body = json!({
            "model": model,
            // Phần tĩnh đứng đầu để nhà cung cấp cache prompt.
            "messages": [
                {"role": "system", "content": req.prefix},
                {"role": "user", "content": req.body},
            ],
            "max_tokens": req.max_tokens,
            "temperature": 0,
            "stream": true,
            "stream_options": {"include_usage": true},
        });
        if self.disable_thinking {
            body["chat_template_kwargs"] = json!({"enable_thinking": false});
        }
        body
    }
}

const RETRY_CAP: Duration = Duration::from_secs(1);
const RETRY_DEFAULT: Duration = Duration::from_millis(300);

fn retry_after(headers: &reqwest::header::HeaderMap) -> Duration {
    headers
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|s| s.is_finite() && *s >= 0.0)
        .map_or(RETRY_DEFAULT, |s| Duration::from_secs_f64(s).min(RETRY_CAP))
}

fn status_error(status: reqwest::StatusCode) -> ProviderError {
    match status.as_u16() {
        400 | 401 | 403 | 404 | 422 => ProviderError::Refused,
        _ => ProviderError::Unavailable,
    }
}

impl Provider for OpenAiCompat {
    fn complete<'a>(
        &'a self,
        model: &'a str,
        req: &'a ModelRequest,
        on_delta: OnDelta<'a>,
    ) -> BoxFuture<'a, Result<Completion, ProviderError>> {
        Box::pin(async move {
            let key = self.api_key.as_deref().ok_or(ProviderError::Refused)?;
            let body = self.payload(model, req).to_string();
            let send = || {
                self.http
                    .post(&self.url)
                    .bearer_auth(key)
                    .header("content-type", "application/json")
                    .body(body.clone())
                    .send()
            };
            let mut res = send().await.map_err(|_| ProviderError::Unavailable)?;
            // Bị giới hạn tần suất: thử lại đúng một lần, chờ tối đa RETRY_CAP.
            // Router vẫn áp timeout của tầng lên cả hai lần gọi.
            if res.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                tokio::time::sleep(retry_after(res.headers())).await;
                res = send().await.map_err(|_| ProviderError::Unavailable)?;
            }
            if !res.status().is_success() {
                return Err(status_error(res.status()));
            }

            let mut sse = SseParser::default();
            let mut think = ThinkFilter::default();
            let mut text = String::new();
            let mut usage = Usage::default();
            let mut emit = |s: String| {
                if !s.is_empty() {
                    on_delta(&s);
                    text.push_str(&s);
                }
            };
            while let Some(chunk) = res.chunk().await.map_err(|_| ProviderError::Unavailable)? {
                for event in sse.push(&chunk) {
                    match parse_event(&event)? {
                        Event::Delta(d) => emit(think.push(&d)),
                        Event::Usage(u) => usage = u,
                        Event::Done | Event::Other => {}
                    }
                }
            }
            emit(think.finish());
            if text.is_empty() {
                return Err(ProviderError::BadResponse);
            }
            Ok(Completion { text, usage })
        })
    }
}

/// Gom byte thành các dòng `data: …` hoàn chỉnh.
#[derive(Default)]
struct SseParser {
    buf: Vec<u8>,
}

impl SseParser {
    fn push(&mut self, bytes: &[u8]) -> Vec<String> {
        self.buf.extend_from_slice(bytes);
        let mut out = Vec::new();
        while let Some(i) = self.buf.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.buf.drain(..=i).collect();
            let line = String::from_utf8_lossy(&line);
            if let Some(data) = line.trim_end().strip_prefix("data:") {
                out.push(data.trim_start().to_owned());
            }
        }
        out
    }
}

#[derive(Debug, PartialEq)]
enum Event {
    Delta(String),
    Usage(Usage),
    Done,
    Other,
}

fn parse_event(data: &str) -> Result<Event, ProviderError> {
    if data == "[DONE]" {
        return Ok(Event::Done);
    }
    let v: Value = serde_json::from_str(data).map_err(|_| ProviderError::BadResponse)?;
    if let Some(u) = v.get("usage").filter(|u| u.is_object()) {
        let n = |k| {
            u.get(k)
                .and_then(Value::as_u64)
                .and_then(|x| u32::try_from(x).ok())
                .unwrap_or(0)
        };
        return Ok(Event::Usage(Usage {
            tokens_in: n("prompt_tokens"),
            tokens_out: n("completion_tokens"),
        }));
    }
    // Chỉ lấy `content`; `reasoning_content` (phần suy nghĩ) bị bỏ qua.
    Ok(v["choices"][0]["delta"]["content"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map_or(Event::Other, |s| Event::Delta(s.to_owned())))
}

/// Bỏ khối `<think>…</think>` nếu mô hình chèn vào `content`, và khoảng trắng đầu
/// bản dịch. Giữ lại phần đuôi có thể là đầu của thẻ cho tới khi đủ chắc.
#[derive(Default)]
struct ThinkFilter {
    pending: String,
    in_think: bool,
    started: bool,
}

const OPEN: &str = "<think>";
const CLOSE: &str = "</think>";

impl ThinkFilter {
    fn push(&mut self, s: &str) -> String {
        self.pending.push_str(s);
        let mut out = String::new();
        loop {
            let tag = if self.in_think { CLOSE } else { OPEN };
            if let Some(i) = self.pending.find(tag) {
                if !self.in_think {
                    out.push_str(&self.pending[..i]);
                }
                self.pending.drain(..i + tag.len());
                self.in_think = !self.in_think;
                continue;
            }
            // Giữ lại hậu tố trùng với đầu thẻ (ví dụ "<thi").
            let keep = (1..tag.len())
                .rev()
                .find(|&n| self.pending.ends_with(&tag[..n]))
                .unwrap_or(0);
            let cut = self.pending.len() - keep;
            if !self.in_think {
                out.push_str(&self.pending[..cut]);
            }
            self.pending.drain(..cut);
            break;
        }
        self.trim_start(out)
    }

    fn finish(&mut self) -> String {
        let rest = std::mem::take(&mut self.pending);
        if self.in_think {
            String::new()
        } else {
            self.trim_start(rest)
        }
    }

    fn trim_start(&mut self, s: String) -> String {
        if self.started {
            return s;
        }
        let t = s.trim_start();
        self.started = !t.is_empty();
        t.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn sse_parser_handles_split_lines() {
        let mut p = SseParser::default();
        assert!(p.push(b"data: {\"a\"").is_empty());
        assert_eq!(
            p.push(b":1}\n\ndata: [DONE]\n"),
            vec!["{\"a\":1}", "[DONE]"]
        );
    }

    #[test]
    fn parses_content_usage_and_ignores_reasoning() {
        let d = r#"{"choices":[{"delta":{"content":"Hi"}}],"usage":null}"#;
        assert_eq!(parse_event(d).unwrap(), Event::Delta("Hi".into()));
        let r = r#"{"choices":[{"delta":{"reasoning_content":"hmm"}}]}"#;
        assert_eq!(parse_event(r).unwrap(), Event::Other);
        let u = r#"{"choices":[],"usage":{"prompt_tokens":43,"completion_tokens":14}}"#;
        assert_eq!(
            parse_event(u).unwrap(),
            Event::Usage(Usage {
                tokens_in: 43,
                tokens_out: 14
            })
        );
        assert_eq!(parse_event("[DONE]").unwrap(), Event::Done);
        assert_eq!(parse_event("{oops"), Err(ProviderError::BadResponse));
    }

    #[test]
    fn think_filter_strips_blocks_split_across_chunks() {
        let mut f = ThinkFilter::default();
        let mut out = String::new();
        for c in [
            "<thi",
            "nk>plan",
            " things</th",
            "ink>\n\n",
            "Hello ",
            "⟦E1⟧<",
            "b",
        ] {
            out += &f.push(c);
        }
        out += &f.finish();
        assert_eq!(out, "Hello ⟦E1⟧<b");
    }

    /// Máy chủ HTTP tối giản: trả `body` dạng SSE và ghi lại request nhận được.
    async fn serve_once(status: &'static str, body: &'static str) -> (String, Arc<Mutex<String>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(String::new()));
        let seen2 = seen.clone();
        tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = vec![0; 16384];
            let n = sock.read(&mut buf).await.unwrap();
            *seen2.lock().unwrap() = String::from_utf8_lossy(&buf[..n]).into_owned();
            let head = format!(
                "HTTP/1.1 {status}\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n"
            );
            sock.write_all(head.as_bytes()).await.unwrap();
            sock.write_all(body.as_bytes()).await.unwrap();
        });
        (format!("http://{addr}/v1"), seen)
    }

    fn provider(base_url: &str, key_env: &str) -> OpenAiCompat {
        let pc = ProviderConfig {
            kind: "openai_compatible".into(),
            no_retention: true,
            approved: true,
            external: true,
            options: HashMap::from([
                ("base_url".into(), base_url.into()),
                ("api_key_env".into(), key_env.into()),
                ("disable_thinking".into(), "true".into()),
            ]),
        };
        OpenAiCompat::from_config(&pc)
    }

    fn request() -> ModelRequest {
        ModelRequest {
            prefix: "SYSTEM".into(),
            body: "BODY".into(),
            max_tokens: 64,
        }
    }

    #[tokio::test]
    async fn streams_completion_from_sse() {
        let (url, seen) = serve_once(
            "200 OK",
            "data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n\
             data: {\"choices\":[{\"delta\":{\"content\":\" there\"}}]}\n\n\
             data: {\"choices\":[],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2}}\n\n\
             data: [DONE]\n\n",
        )
        .await;
        let mut p = provider(&url, "XT_TEST_UNSET_KEY");
        p.api_key = Some("test-key".into());
        let deltas = Mutex::new(Vec::new());
        let on = |s: &str| deltas.lock().unwrap().push(s.to_owned());
        let c = p.complete("m-1", &request(), &on).await.unwrap();
        assert_eq!(c.text, "Hello there");
        assert_eq!(c.usage.tokens_out, 2);
        assert_eq!(*deltas.lock().unwrap(), vec!["Hello", " there"]);
        let raw = seen.lock().unwrap().clone();
        assert!(raw.starts_with("POST /v1/chat/completions"));
        assert!(raw.contains("authorization: Bearer test-key"));
        assert!(raw.contains("\"enable_thinking\":false"));
        assert!(raw.contains("\"model\":\"m-1\""));
    }

    #[tokio::test]
    async fn retries_once_after_rate_limit() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            for head in [
                "HTTP/1.1 429 Too Many Requests\r\nretry-after: 0\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n\
                 data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\ndata: [DONE]\n\n",
            ] {
                let (mut sock, _) = listener.accept().await.unwrap();
                let mut buf = vec![0; 16384];
                let _ = sock.read(&mut buf).await.unwrap();
                sock.write_all(head.as_bytes()).await.unwrap();
            }
        });
        let mut p = provider(&format!("http://{addr}/v1"), "XT_TEST_UNSET_KEY");
        p.api_key = Some("k".into());
        let c = p.complete("m", &request(), &|_| {}).await.unwrap();
        assert_eq!(c.text, "ok");
    }

    #[test]
    fn retry_after_is_capped() {
        let mut h = reqwest::header::HeaderMap::new();
        assert_eq!(retry_after(&h), RETRY_DEFAULT);
        h.insert("retry-after", "30".parse().unwrap());
        assert_eq!(retry_after(&h), RETRY_CAP);
        h.insert("retry-after", "0.2".parse().unwrap());
        assert_eq!(retry_after(&h), Duration::from_millis(200));
    }

    #[tokio::test]
    async fn maps_errors_to_codes() {
        let (url, _) = serve_once("401 Unauthorized", "").await;
        let mut p = provider(&url, "XT_TEST_UNSET_KEY");
        p.api_key = Some("k".into());
        let err = p.complete("m", &request(), &|_| {}).await.unwrap_err();
        assert_eq!(err, ProviderError::Refused);

        let p = provider(&url, "XT_TEST_UNSET_KEY");
        assert_eq!(
            p.complete("m", &request(), &|_| {}).await.unwrap_err(),
            ProviderError::Refused,
            "thiếu khoá thì từ chối, không gọi mạng"
        );
    }
}
