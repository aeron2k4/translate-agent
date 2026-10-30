//! Tra glossary đã duyệt, có phiên bản, theo mảng.
//!
//! File JSON trong `glossary/data/` do công cụ Python (`xt_admin.glossary`) quản
//! lý và tăng phiên bản; crate này chỉ đọc. Mỗi file chứa một cặp ngôn ngữ và
//! dùng được cho cả hai chiều dịch.

use serde::Deserialize;
use std::collections::HashMap;
use std::fmt;
use std::path::Path;
use std::sync::{Arc, RwLock};
use validators::TermRef;

/// Số mục glossary tối đa chèn vào prompt cho một tin.
pub const MAX_TERMS_PER_MESSAGE: usize = 20;

#[derive(Debug, Clone, Deserialize)]
pub struct Term {
    pub id: String,
    #[serde(default)]
    pub core: bool,
    /// Bản dịch theo mã ngôn ngữ, ví dụ {"vi": "lái thử", "en": "test drive"}.
    #[serde(flatten)]
    pub forms: HashMap<String, serde_json::Value>,
}

impl Term {
    fn form(&self, lang: &str) -> Option<&str> {
        self.forms.get(lang).and_then(|v| v.as_str())
    }
}

#[derive(Debug, Deserialize)]
struct File {
    domain: String,
    langs: [String; 2],
    version: u32,
    terms: Vec<Term>,
}

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Json(serde_json::Error),
    Invalid(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "đọc glossary: {e}"),
            Self::Json(e) => write!(f, "glossary sai định dạng: {e}"),
            Self::Invalid(why) => write!(f, "glossary không hợp lệ: {why}"),
        }
    }
}

impl std::error::Error for Error {}

/// Glossary của một mảng và một cặp ngôn ngữ.
#[derive(Debug)]
pub struct Glossary {
    pub domain: String,
    pub langs: [String; 2],
    pub version: u32,
    terms: Vec<Term>,
}

impl Glossary {
    pub fn from_json(s: &str) -> Result<Self, Error> {
        let f: File = serde_json::from_str(s).map_err(Error::Json)?;
        if f.version == 0 {
            return Err(Error::Invalid("version phải ≥ 1"));
        }
        for t in &f.terms {
            if f.langs.iter().any(|l| t.form(l).is_none_or(str::is_empty)) {
                return Err(Error::Invalid("thuật ngữ thiếu bản dịch cho một ngôn ngữ"));
            }
        }
        Ok(Self {
            domain: f.domain,
            langs: f.langs,
            version: f.version,
            terms: f.terms,
        })
    }

    fn oriented<'a>(&'a self, t: &'a Term, from: &str, to: &str) -> Option<TermRef<'a>> {
        Some(TermRef {
            id: &t.id,
            source: t.form(from)?,
            target: t.form(to)?,
        })
    }

    /// Thuật ngữ lõi: đặt ở phần tĩnh đầu prompt để tận dụng prompt caching.
    pub fn core_terms(&self, from: &str, to: &str) -> Vec<TermRef<'_>> {
        self.terms
            .iter()
            .filter(|t| t.core)
            .filter_map(|t| self.oriented(t, from, to))
            .collect()
    }

    /// Thuật ngữ xuất hiện trong tin (khớp không phân biệt dấu), cụm dài trước,
    /// tối đa `max` mục.
    pub fn lookup(&self, text: &str, from: &str, to: &str, max: usize) -> Vec<TermRef<'_>> {
        let words = textnorm::words(&textnorm::fold(text));
        let mut hits: Vec<TermRef<'_>> = self
            .terms
            .iter()
            .filter_map(|t| self.oriented(t, from, to))
            .filter(|t| {
                let needle = textnorm::words(&textnorm::fold(t.source));
                !needle.is_empty() && words.windows(needle.len()).any(|w| w == needle.as_slice())
            })
            .collect();
        hits.sort_by_key(|t| std::cmp::Reverse(t.source.chars().count()));
        hits.truncate(max);
        hits
    }

    pub fn len(&self) -> usize {
        self.terms.len()
    }

    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }
}

/// Tập glossary đang dùng; thay thế nguyên tử khi có phiên bản mới.
#[derive(Default)]
pub struct Store {
    inner: RwLock<HashMap<String, Arc<Glossary>>>,
}

impl Store {
    pub fn load_dir(dir: &Path) -> Result<Self, Error> {
        let store = Self::default();
        for entry in std::fs::read_dir(dir).map_err(Error::Io)? {
            let path = entry.map_err(Error::Io)?.path();
            if path.extension().is_some_and(|e| e == "json") {
                let s = std::fs::read_to_string(&path).map_err(Error::Io)?;
                store.install(Glossary::from_json(&s)?);
            }
        }
        Ok(store)
    }

    /// Cài một glossary; chỉ nhận nếu phiên bản mới hơn bản đang có.
    /// Trả về phiên bản đang hiệu lực sau khi cài.
    pub fn install(&self, g: Glossary) -> u32 {
        let key = format!("{}:{}", g.domain, sorted_pair(&g.langs[0], &g.langs[1]));
        let Ok(mut map) = self.inner.write() else {
            return 0;
        };
        match map.get(&key) {
            Some(cur) if cur.version >= g.version => cur.version,
            _ => {
                let v = g.version;
                map.insert(key, Arc::new(g));
                v
            }
        }
    }

    pub fn get(&self, domain: &str, from: &str, to: &str) -> Option<Arc<Glossary>> {
        let key = format!("{domain}:{}", sorted_pair(from, to));
        self.inner.read().ok()?.get(&key).cloned()
    }
}

fn sorted_pair(a: &str, b: &str) -> String {
    if a <= b {
        format!("{a}-{b}")
    } else {
        format!("{b}-{a}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data_dir() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../glossary/data")
    }

    fn automotive() -> Arc<Glossary> {
        let store = Store::load_dir(&data_dir()).unwrap();
        store.get("automotive", "vi", "en").unwrap()
    }

    #[test]
    fn loads_repo_data() {
        let g = automotive();
        assert!(g.version >= 1);
        assert!(!g.is_empty());
        assert!(!g.core_terms("vi", "en").is_empty());
    }

    #[test]
    fn lookup_is_accent_insensitive_and_longest_first() {
        let g = automotive();
        let hits = g.lookup("a oi gia lan banh bao nhieu, co tra gop ko", "vi", "en", 20);
        let ids: Vec<&str> = hits.iter().map(|t| t.id).collect();
        assert_eq!(ids, ["auto-003", "auto-012"]);
        assert_eq!(hits[0].target, "on-the-road price");
    }

    #[test]
    fn lookup_works_in_reverse_direction() {
        let g = automotive();
        let hits = g.lookup("Can I book a test drive?", "en", "vi", 20);
        assert_eq!(hits[0].target, "lái thử");
    }

    #[test]
    fn lookup_respects_limit() {
        let g = automotive();
        let text = "lái thử đặt cọc trả góp bảo hành bảo dưỡng";
        assert_eq!(g.lookup(text, "vi", "en", 2).len(), 2);
    }

    #[test]
    fn install_only_accepts_newer_versions() {
        let store = Store::default();
        let mk = |v: u32| {
            Glossary::from_json(&format!(
                r#"{{"domain":"d","langs":["vi","en"],"version":{v},"terms":[]}}"#
            ))
            .unwrap()
        };
        assert_eq!(store.install(mk(2)), 2);
        assert_eq!(store.install(mk(1)), 2);
        assert_eq!(store.install(mk(3)), 3);
        assert_eq!(store.get("d", "en", "vi").unwrap().version, 3);
    }

    #[test]
    fn rejects_term_missing_a_language() {
        let bad = r#"{"domain":"d","langs":["vi","en"],"version":1,"terms":[{"id":"x","vi":"a"}]}"#;
        assert!(Glossary::from_json(bad).is_err());
    }
}
