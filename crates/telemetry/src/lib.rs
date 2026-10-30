//! Số đo vận hành.
//!
//! Quy tắc bảo mật: sự kiện chỉ chứa độ trễ, số token, mã lỗi và id đã hash.
//! Không có trường nào nhận nội dung tin nhắn; đừng thêm.

use hmac::{Hmac, Mac};
use serde::Serialize;
use sha2::Sha256;
use std::io::Write;
use std::sync::Mutex;
use std::time::Duration;

/// Id đã băm bằng HMAC, không đảo ngược được nếu không có khoá.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct HashedId(String);

impl HashedId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Băm id người dùng / luồng chat trước khi đưa vào log.
pub struct Hasher {
    key: Vec<u8>,
}

impl Hasher {
    pub fn new(key: &[u8]) -> Self {
        Self { key: key.to_vec() }
    }

    pub fn id(&self, raw: &str) -> HashedId {
        // HMAC nhận khoá độ dài bất kỳ nên new_from_slice không bao giờ lỗi.
        let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(&self.key) else {
            return HashedId(String::new());
        };
        mac.update(raw.as_bytes());
        HashedId(hex::encode(&mac.finalize().into_bytes()[..12]))
    }
}

/// Tên bước cố định trong pipeline, không phải văn bản tự do.
pub type EventName = &'static str;

/// Mã lỗi cố định, không phải thông điệp lỗi (thông điệp có thể lẫn nội dung).
pub type ErrorCode = &'static str;

#[derive(Debug, Clone, Default, Serialize)]
pub struct Event {
    pub name: EventName,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread: Option<HashedId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none", with = "opt_ms")]
    pub latency: Option<Duration>,
    #[serde(skip_serializing_if = "Option::is_none", with = "opt_ms")]
    pub ttft: Option<Duration>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens_in: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens_out: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorCode>,
}

mod opt_ms {
    use serde::Serializer;
    use std::time::Duration;

    pub fn serialize<S: Serializer>(d: &Option<Duration>, s: S) -> Result<S::Ok, S::Error> {
        match d {
            Some(d) => s.serialize_f64(d.as_secs_f64() * 1000.0),
            None => s.serialize_none(),
        }
    }
}

pub trait Sink: Send + Sync {
    fn emit(&self, e: Event);
}

/// Bỏ qua mọi sự kiện.
pub struct Nop;

impl Sink for Nop {
    fn emit(&self, _: Event) {}
}

/// Ghi mỗi sự kiện thành một dòng JSON.
pub struct JsonSink<W: Write + Send> {
    w: Mutex<W>,
}

impl<W: Write + Send> JsonSink<W> {
    pub fn new(w: W) -> Self {
        Self { w: Mutex::new(w) }
    }

    pub fn into_inner(self) -> W {
        self.w.into_inner().unwrap_or_else(|p| p.into_inner())
    }
}

impl<W: Write + Send> Sink for JsonSink<W> {
    fn emit(&self, e: Event) {
        let Ok(mut line) = serde_json::to_vec(&e) else {
            return;
        };
        line.push(b'\n');
        if let Ok(mut w) = self.w.lock() {
            let _ = w.write_all(&line);
        }
    }
}

/// Giữ sự kiện trong bộ nhớ, dùng cho test.
#[derive(Default)]
pub struct Recorder {
    events: Mutex<Vec<Event>>,
}

impl Recorder {
    pub fn events(&self) -> Vec<Event> {
        self.events.lock().map(|e| e.clone()).unwrap_or_default()
    }
}

impl Sink for Recorder {
    fn emit(&self, e: Event) {
        if let Ok(mut ev) = self.events.lock() {
            ev.push(e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hasher_is_stable_and_hides_raw_id() {
        let h = Hasher::new(b"k");
        let a = h.id("thread-123");
        assert_eq!(a, h.id("thread-123"));
        assert!(!a.as_str().contains("thread-123"));
        assert_ne!(Hasher::new(b"k2").id("thread-123"), a);
    }

    #[test]
    fn json_sink_writes_one_line_per_event() {
        let sink = JsonSink::new(Vec::new());
        sink.emit(Event {
            name: "translate.done",
            latency: Some(Duration::from_millis(5)),
            tokens_in: Some(10),
            ..Default::default()
        });
        sink.emit(Event {
            name: "translate.error",
            error: Some("provider_timeout"),
            ..Default::default()
        });
        let out = String::from_utf8(sink.into_inner()).unwrap();
        assert_eq!(out.lines().count(), 2);
        assert!(out.contains("\"latency\":5.0"));
    }
}
