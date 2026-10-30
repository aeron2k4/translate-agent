//! Human-in-the-loop: bản sửa của người dùng có hiệu lực ngay theo phạm vi.
//!
//! - `Me`: chỉ người sửa thấy; xử lý ở client, máy chủ không lưu gì.
//! - `Group`: cả luồng thấy bản đã sửa của tin đó; cặp thuật ngữ (nếu có) được
//!   ưu tiên cho các tin sau trong luồng. Lưu trong RAM, TTL ≤ 24 giờ, xoá cùng luồng.
//! - `Unit`: như `Group`, cộng thêm một đề xuất (chỉ cặp thuật ngữ + câu ví dụ đã
//!   ẩn danh) gửi hàng chờ review → chủ sở hữu duyệt → glossary tăng phiên bản.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Me,
    Group,
    Unit,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Correction {
    pub thread_id: String,
    pub message_id: String,
    pub user_id: String,
    pub scope: Scope,
    pub domain: String,
    pub source_lang: String,
    pub target_lang: String,
    /// Bản dịch đã sửa của cả tin.
    pub corrected_text: String,
    /// Cặp thuật ngữ người dùng chỉ ra (tuỳ chọn).
    #[serde(default)]
    pub term: Option<TermPair>,
    /// Tin gốc, chỉ dùng để tạo câu ví dụ đã ẩn danh cho đề xuất `Unit`.
    #[serde(default)]
    pub original_text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TermPair {
    pub source: String,
    pub target: String,
}

/// Đề xuất gửi `python/xt_admin/review`. Không chứa tin gốc hay id thật.
#[derive(Debug, Clone, Serialize)]
pub struct Suggestion {
    pub domain: String,
    pub source_lang: String,
    pub target_lang: String,
    pub source_term: String,
    pub target_term: String,
    pub example: Option<String>,
    /// Id người đề xuất đã hash.
    pub proposer: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Outcome {
    pub scope: Scope,
    pub stored: bool,
    pub suggestion: Option<Suggestion>,
}

/// Câu ví dụ dài hơn thế này bị cắt.
const MAX_EXAMPLE_CHARS: usize = 200;

struct ThreadHitl {
    terms: Vec<(String, String, TermPair, Instant)>, // (from, to, cặp, lúc tạo)
    corrected: HashMap<(String, String), (String, Instant)>, // (message_id, lang)
}

pub struct Hitl {
    ttl: Duration,
    threads: Mutex<HashMap<String, ThreadHitl>>,
}

impl Hitl {
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl: ttl.min(crate::memory::MAX_TTL),
            threads: Mutex::default(),
        }
    }

    /// `proposer` là id người dùng đã hash (telemetry::Hasher).
    pub fn submit(&self, c: &Correction, proposer: &str) -> Outcome {
        if c.scope == Scope::Me {
            return Outcome {
                scope: c.scope,
                stored: false,
                suggestion: None,
            };
        }
        let now = Instant::now();
        let stored = self
            .threads
            .lock()
            .map(|mut threads| {
                let t = threads
                    .entry(c.thread_id.clone())
                    .or_insert_with(|| ThreadHitl {
                        terms: Vec::new(),
                        corrected: HashMap::new(),
                    });
                t.corrected.insert(
                    (c.message_id.clone(), c.target_lang.clone()),
                    (c.corrected_text.clone(), now),
                );
                if let Some(term) = &c.term {
                    let folded = textnorm::fold(&term.source);
                    t.terms.retain(|(f, to, p, _)| {
                        !(f == &c.source_lang
                            && to == &c.target_lang
                            && textnorm::fold(&p.source) == folded)
                    });
                    t.terms.push((
                        c.source_lang.clone(),
                        c.target_lang.clone(),
                        term.clone(),
                        now,
                    ));
                }
            })
            .is_ok();
        let suggestion = match (c.scope, &c.term) {
            (Scope::Unit, Some(term)) => Some(Suggestion {
                domain: c.domain.clone(),
                source_lang: c.source_lang.clone(),
                target_lang: c.target_lang.clone(),
                source_term: term.source.clone(),
                target_term: term.target.clone(),
                example: c.original_text.as_deref().map(|t| {
                    prompts::redact(t, &c.source_lang)
                        .chars()
                        .take(MAX_EXAMPLE_CHARS)
                        .collect()
                }),
                proposer: proposer.to_owned(),
            }),
            _ => None,
        };
        Outcome {
            scope: c.scope,
            stored,
            suggestion,
        }
    }

    /// Bản dịch đã được sửa ở phạm vi nhóm (hiển thị nhãn "đã chỉnh bởi người").
    pub fn corrected(&self, thread: &str, message_id: &str, lang: &str) -> Option<String> {
        let threads = self.threads.lock().ok()?;
        let (text, at) = threads
            .get(thread)?
            .corrected
            .get(&(message_id.to_owned(), lang.to_owned()))?;
        (at.elapsed() <= self.ttl).then(|| text.clone())
    }

    /// Cặp thuật ngữ nhóm đã sửa, ưu tiên hơn glossary trong luồng này.
    pub fn thread_terms(&self, thread: &str, from: &str, to: &str) -> Vec<TermPair> {
        let Ok(threads) = self.threads.lock() else {
            return Vec::new();
        };
        threads.get(thread).map_or_else(Vec::new, |t| {
            t.terms
                .iter()
                .filter(|(f, tt, _, at)| f == from && tt == to && at.elapsed() <= self.ttl)
                .map(|(_, _, p, _)| p.clone())
                .collect()
        })
    }

    pub fn clear_thread(&self, thread: &str) {
        if let Ok(mut threads) = self.threads.lock() {
            threads.remove(thread);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn correction(scope: Scope) -> Correction {
        Correction {
            thread_id: "t1".into(),
            message_id: "m1".into(),
            user_id: "u1".into(),
            scope,
            domain: "automotive".into(),
            source_lang: "vi".into(),
            target_lang: "en".into(),
            corrected_text: "I'd like a test ride".into(),
            term: Some(TermPair {
                source: "chạy thử".into(),
                target: "test ride".into(),
            }),
            original_text: Some("Anh muốn chạy thử, gọi 0901234567".into()),
        }
    }

    #[test]
    fn me_scope_stores_nothing() {
        let h = Hitl::new(crate::memory::MAX_TTL);
        let out = h.submit(&correction(Scope::Me), "hash");
        assert!(!out.stored && out.suggestion.is_none());
        assert!(h.corrected("t1", "m1", "en").is_none());
    }

    #[test]
    fn group_scope_applies_immediately_to_thread() {
        let h = Hitl::new(crate::memory::MAX_TTL);
        let out = h.submit(&correction(Scope::Group), "hash");
        assert!(out.stored && out.suggestion.is_none());
        assert_eq!(
            h.corrected("t1", "m1", "en").as_deref(),
            Some("I'd like a test ride")
        );
        assert_eq!(h.thread_terms("t1", "vi", "en")[0].target, "test ride");
        assert!(h.thread_terms("t2", "vi", "en").is_empty());
        h.clear_thread("t1");
        assert!(h.corrected("t1", "m1", "en").is_none());
    }

    #[test]
    fn unit_scope_builds_anonymized_suggestion() {
        let h = Hitl::new(crate::memory::MAX_TTL);
        let s = h
            .submit(&correction(Scope::Unit), "hash")
            .suggestion
            .unwrap();
        assert_eq!(s.source_term, "chạy thử");
        assert_eq!(s.example.as_deref(), Some("Anh muốn chạy thử, gọi ⟨phone⟩"));
        assert_eq!(s.proposer, "hash");
    }
}
