//! Cache cụm lặp ("ok anh", "em cảm ơn ạ"…).
//!
//! - Chỉ nhận cụm ngắn, không chứa thực thể nào (không số, không PII) và đã gặp
//!   ít nhất `min_hits` lần (cụm phổ biến).
//! - Khoá = HMAC(văn bản chuẩn hoá + cặp ngôn ngữ + lĩnh vực + phiên bản glossary).
//!   Glossary tăng phiên bản thì khoá cũ không còn khớp; `on_glossary_version`
//!   dọn luôn các mục cũ.
//! - Giá trị được mã hoá AES-256-GCM, gắn với khoá cache làm dữ liệu xác thực.

use aes_gcm::aead::{Aead, AeadCore, KeyInit, OsRng, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use validators::Masked;

/// Số từ tối đa của một cụm được cache.
pub const MAX_WORDS: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CacheKey([u8; 32]);

#[derive(Debug, Clone, Copy)]
pub struct Scope<'a> {
    pub domain: &'a str,
    pub from: &'a str,
    pub to: &'a str,
    pub glossary_version: u32,
}

struct Entry {
    nonce: [u8; 12],
    ciphertext: Vec<u8>,
    domain: String,
    version: u32,
    inserted: Instant,
}

#[derive(Default)]
struct Inner {
    entries: HashMap<CacheKey, Entry>,
    seen: HashMap<CacheKey, u32>,
}

pub struct PhraseCache {
    cipher: Aes256Gcm,
    mac_key: [u8; 32],
    capacity: usize,
    ttl: Duration,
    min_hits: u32,
    inner: Mutex<Inner>,
}

fn derive(secret: &[u8], label: &[u8]) -> [u8; 32] {
    let Ok(mut m) = <Hmac<Sha256> as Mac>::new_from_slice(secret) else {
        return [0; 32];
    };
    m.update(label);
    m.finalize().into_bytes().into()
}

impl PhraseCache {
    /// `secret` là khoá chỉ tồn tại trong TEE.
    pub fn new(secret: &[u8], capacity: usize, ttl: Duration, min_hits: u32) -> Self {
        let enc_key = derive(secret, b"phrase-cache/enc");
        Self {
            cipher: Aes256Gcm::new(&enc_key.into()),
            mac_key: derive(secret, b"phrase-cache/key"),
            capacity: capacity.max(1),
            ttl,
            min_hits: min_hits.max(1),
            inner: Mutex::default(),
        }
    }

    /// Tin có đủ điều kiện cache không: ngắn và không có thực thể nào.
    pub fn eligible(masked: &Masked) -> bool {
        masked.entities.is_empty() && textnorm::words(&masked.text).len() <= MAX_WORDS
    }

    pub fn key(&self, text: &str, scope: Scope<'_>) -> CacheKey {
        let Ok(mut m) = <Hmac<Sha256> as Mac>::new_from_slice(&self.mac_key) else {
            return CacheKey([0; 32]);
        };
        for part in [
            textnorm::normalize(text).as_str(),
            scope.from,
            scope.to,
            scope.domain,
            &scope.glossary_version.to_string(),
        ] {
            m.update(&(part.len() as u64).to_be_bytes());
            m.update(part.as_bytes());
        }
        CacheKey(m.finalize().into_bytes().into())
    }

    pub fn get(&self, key: &CacheKey) -> Option<String> {
        let mut inner = self.inner.lock().ok()?;
        let entry = inner.entries.get(key)?;
        if entry.inserted.elapsed() > self.ttl {
            inner.entries.remove(key);
            return None;
        }
        let plain = self
            .cipher
            .decrypt(
                Nonce::from_slice(&entry.nonce),
                Payload {
                    msg: &entry.ciphertext,
                    aad: &key.0,
                },
            )
            .ok()?;
        String::from_utf8(plain).ok()
    }

    /// Ghi nhận một bản dịch; chỉ lưu khi cụm đã gặp đủ `min_hits` lần.
    /// Trả về true nếu đã lưu.
    pub fn put(&self, key: CacheKey, scope: Scope<'_>, translation: &str) -> bool {
        let Ok(mut inner) = self.inner.lock() else {
            return false;
        };
        let seen = inner.seen.entry(key).or_default();
        *seen += 1;
        if *seen < self.min_hits {
            if inner.seen.len() > self.capacity * 4 {
                inner.seen.clear(); // bộ đếm chỉ là gợi ý, xoá khi quá lớn
            }
            return false;
        }
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let Ok(ciphertext) = self.cipher.encrypt(
            &nonce,
            Payload {
                msg: translation.as_bytes(),
                aad: &key.0,
            },
        ) else {
            return false;
        };
        if inner.entries.len() >= self.capacity {
            evict_oldest(&mut inner.entries, self.capacity / 10 + 1);
        }
        inner.entries.insert(
            key,
            Entry {
                nonce: nonce.into(),
                ciphertext,
                domain: scope.domain.to_owned(),
                version: scope.glossary_version,
                inserted: Instant::now(),
            },
        );
        true
    }

    /// Glossary của `domain` vừa lên `version`: xoá mục của phiên bản cũ.
    pub fn on_glossary_version(&self, domain: &str, version: u32) -> usize {
        let Ok(mut inner) = self.inner.lock() else {
            return 0;
        };
        let before = inner.entries.len();
        inner
            .entries
            .retain(|_, e| e.domain != domain || e.version >= version);
        before - inner.entries.len()
    }

    pub fn len(&self) -> usize {
        self.inner.lock().map_or(0, |i| i.entries.len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

fn evict_oldest(entries: &mut HashMap<CacheKey, Entry>, n: usize) {
    let mut by_age: Vec<(Instant, CacheKey)> =
        entries.iter().map(|(k, e)| (e.inserted, *k)).collect();
    by_age.sort_by_key(|(t, _)| *t);
    for (_, k) in by_age.into_iter().take(n) {
        entries.remove(&k);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCOPE: Scope<'static> = Scope {
        domain: "automotive",
        from: "vi",
        to: "en",
        glossary_version: 1,
    };

    fn cache() -> PhraseCache {
        PhraseCache::new(b"secret", 100, Duration::from_secs(60), 1)
    }

    #[test]
    fn eligibility() {
        assert!(PhraseCache::eligible(&validators::mask(
            "Em cảm ơn ạ",
            "vi"
        )));
        assert!(!PhraseCache::eligible(&validators::mask("cọc 5 củ", "vi")));
        assert!(!PhraseCache::eligible(&validators::mask("hỏi @lan", "vi")));
        assert!(!PhraseCache::eligible(&validators::mask(
            "một hai ba bốn năm sáu bảy",
            "vi"
        )));
    }

    #[test]
    fn key_normalizes_and_includes_scope() {
        let c = cache();
        assert_eq!(c.key("OK  anh", SCOPE), c.key("ok anh", SCOPE));
        let v2 = Scope {
            glossary_version: 2,
            ..SCOPE
        };
        assert_ne!(c.key("ok anh", SCOPE), c.key("ok anh", v2));
        let rev = Scope {
            from: "en",
            to: "vi",
            ..SCOPE
        };
        assert_ne!(c.key("ok anh", SCOPE), c.key("ok anh", rev));
        // Khoá khác secret thì khác.
        let other = PhraseCache::new(b"other", 10, Duration::from_secs(1), 1);
        assert_ne!(c.key("ok anh", SCOPE), other.key("ok anh", SCOPE));
    }

    #[test]
    fn stores_encrypted_and_round_trips() {
        let c = cache();
        let k = c.key("em cảm ơn ạ", SCOPE);
        assert!(c.put(k, SCOPE, "Thank you"));
        assert_eq!(c.get(&k).as_deref(), Some("Thank you"));
        let inner = c.inner.lock().unwrap();
        let stored = &inner.entries[&k].ciphertext;
        assert!(!stored.windows(5).any(|w| w == b"Thank"));
    }

    #[test]
    fn requires_min_hits() {
        let c = PhraseCache::new(b"s", 10, Duration::from_secs(60), 2);
        let k = c.key("ok anh", SCOPE);
        assert!(!c.put(k, SCOPE, "OK"));
        assert!(c.get(&k).is_none());
        assert!(c.put(k, SCOPE, "OK"));
        assert!(c.get(&k).is_some());
    }

    #[test]
    fn glossary_bump_purges_old_entries() {
        let c = cache();
        let k = c.key("ok anh", SCOPE);
        c.put(k, SCOPE, "OK");
        assert_eq!(c.on_glossary_version("automotive", 2), 1);
        assert!(c.get(&k).is_none());
    }

    #[test]
    fn expires_and_evicts() {
        let c = PhraseCache::new(b"s", 100, Duration::ZERO, 1);
        let k = c.key("ok", SCOPE);
        c.put(k, SCOPE, "OK");
        std::thread::sleep(Duration::from_millis(2));
        assert!(c.get(&k).is_none());

        let small = PhraseCache::new(b"s", 3, Duration::from_secs(60), 1);
        for w in ["a", "b", "c", "d", "e"] {
            small.put(small.key(w, SCOPE), SCOPE, w);
        }
        assert!(small.len() <= 3);
    }
}
