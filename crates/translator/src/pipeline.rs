//! Pipeline dịch một tin (bước 2–7; bước 1 chạy ở client). Không đổi thứ tự
//! các bước khi chưa thảo luận.

use crate::memory::{MAX_TTL, Memory, Snapshot};
use crate::risk::{self, Clarification};
use crate::stream::StreamUnmask;
use glossary::{Glossary, MAX_TERMS_PER_MESSAGE, Store};
use model_router::{Router, Signals, Tier, select_tier};
use phrase_cache::{PhraseCache, Scope};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use telemetry::{Event, HashedId, Hasher, Sink};
use validators::{IssueCode, Masked, TermRef, check_entities, check_glossary, mask, unmask};

fn default_domain() -> String {
    "automotive".into()
}

#[derive(Debug, Clone, Deserialize)]
pub struct Request {
    pub thread_id: String,
    pub sender_id: String,
    pub text: String,
    /// Ngôn ngữ nguồn do client nhận diện; không có thì máy chủ tự nhận diện.
    #[serde(default)]
    pub source_lang: Option<String>,
    #[serde(default)]
    pub unaccented_vi: bool,
    #[serde(default = "default_domain")]
    pub domain: String,
    /// Mỗi ngôn ngữ đích chỉ dịch một lần, dùng chung cho mọi người đọc.
    pub target_langs: Vec<String>,
    /// Hội thoại cấm gửi ra ngoài: chỉ dùng mô hình nội bộ.
    #[serde(default)]
    pub external_forbidden: bool,
    /// Người gửi đã chọn "gửi bản dịch như hiện tại" sau câu hỏi làm rõ.
    #[serde(default)]
    pub skip_clarification: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Translated,
    Cached,
    SameLanguage,
    NeedsClarification,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct Translation {
    pub lang: String,
    pub status: Status,
    /// Bản dịch; khi thất bại hoặc đang chờ làm rõ thì là bản gốc.
    pub text: String,
    /// Client hiển thị nút "Thử dịch lại".
    pub retryable: bool,
    pub tier: Option<&'static str>,
    pub edited_by_human: bool,
    pub issues: Vec<IssueCode>,
    /// Khoá i18n của nhãn hiển thị kèm bản dịch.
    pub label_key: Option<&'static str>,
}

impl Translation {
    fn original(lang: &str, text: &str, status: Status) -> Self {
        Self {
            lang: lang.to_owned(),
            status,
            text: text.to_owned(),
            retryable: status == Status::Failed,
            tier: None,
            edited_by_human: false,
            issues: Vec::new(),
            label_key: (status == Status::Failed).then_some("translation.failed_retry"),
        }
    }

    fn translated(lang: &str, text: String, status: Status, tier: Option<Tier>) -> Self {
        Self {
            lang: lang.to_owned(),
            status,
            text,
            retryable: false,
            tier: tier.map(Tier::as_str),
            edited_by_human: false,
            issues: Vec::new(),
            label_key: Some("translation.by_ai"),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Response {
    pub source_lang: Option<String>,
    pub translations: Vec<Translation>,
    /// Câu hỏi làm rõ — chỉ gửi riêng người gửi.
    pub clarification: Option<Clarification>,
}

#[derive(Debug, Clone, Copy)]
pub enum StreamEvent<'a> {
    Delta {
        lang: &'a str,
        text: &'a str,
    },
    /// Bỏ phần đã stream cho `lang` (sắp thử lại hoặc đã thất bại).
    Reset {
        lang: &'a str,
    },
}

pub type OnStream<'a> = &'a (dyn Fn(StreamEvent<'_>) + Send + Sync);

/// Khoá bí mật, chỉ tồn tại trong TEE.
pub struct Secrets {
    pub cache_key: Vec<u8>,
    pub id_hash_key: Vec<u8>,
}

pub struct Translator {
    glossaries: Arc<Store>,
    router: Arc<Router>,
    memory: Memory,
    cache: PhraseCache,
    sink: Arc<dyn Sink>,
    hasher: Hasher,
}

/// Dữ liệu dùng chung cho mọi ngôn ngữ đích của một tin.
struct Prepared<'a> {
    req: &'a Request,
    source: &'a str,
    masked: Masked,
    snapshot: Snapshot,
    unaccented_vi: bool,
    high_stakes: bool,
    words: usize,
    thread: HashedId,
}

impl Translator {
    pub fn new(
        glossaries: Arc<Store>,
        router: Arc<Router>,
        sink: Arc<dyn Sink>,
        secrets: &Secrets,
    ) -> Self {
        Self {
            glossaries,
            router,
            memory: Memory::new(MAX_TTL, 8),
            cache: PhraseCache::new(&secrets.cache_key, 10_000, MAX_TTL, 2),
            sink,
            hasher: Hasher::new(&secrets.id_hash_key),
        }
    }

    pub fn with_cache(mut self, cache: PhraseCache) -> Self {
        self.cache = cache;
        self
    }

    pub fn memory(&self) -> &Memory {
        &self.memory
    }

    pub fn cache(&self) -> &PhraseCache {
        &self.cache
    }

    pub fn glossaries(&self) -> &Store {
        &self.glossaries
    }

    pub async fn translate(&self, req: &Request, on_stream: OnStream<'_>) -> Response {
        let started = Instant::now();
        let thread = self.hasher.id(&req.thread_id);
        let detection = client_core::detect(&req.text);
        let source = req
            .source_lang
            .clone()
            .or_else(|| detection.primary.code().map(str::to_owned));
        let mut targets: Vec<&str> = Vec::new();
        for t in &req.target_langs {
            if !targets.contains(&t.as_str()) {
                targets.push(t);
            }
        }
        let Some(source) = source else {
            self.emit(Event {
                name: "translate.error",
                thread: Some(thread),
                error: Some("unknown_language"),
                latency: Some(started.elapsed()),
                ..Event::default()
            });
            return Response {
                source_lang: None,
                translations: targets
                    .iter()
                    .map(|t| Translation::original(t, &req.text, Status::Failed))
                    .collect(),
                clarification: None,
            };
        };
        let unaccented_vi = if req.source_lang.is_some() {
            req.unaccented_vi
        } else {
            detection.unaccented_vi
        };

        // Bước 2 + 3 chạy song song: ghép ngữ cảnh ‖ che thực thể + tra glossary.
        let glossaries: Vec<Option<Arc<Glossary>>> = targets
            .iter()
            .map(|t| self.glossaries.get(&req.domain, &source, t))
            .collect();
        let (snapshot, masked, terms) = std::thread::scope(|s| {
            let ctx = s.spawn(|| self.memory.snapshot(&req.thread_id, &req.sender_id));
            let masked = mask(&req.text, &source);
            let terms: Vec<Vec<TermRef<'_>>> = glossaries
                .iter()
                .zip(&targets)
                .map(|(g, t)| {
                    g.as_ref().map_or_else(Vec::new, |g| {
                        g.lookup(&req.text, &source, t, MAX_TERMS_PER_MESSAGE)
                    })
                })
                .collect();
            (ctx.join().unwrap_or_default(), masked, terms)
        });

        // Bước 4: tối nghĩa → hỏi riêng người gửi, chưa dịch.
        if !req.skip_clarification
            && let Some(c) = risk::assess(&masked, &source)
        {
            self.emit(Event {
                name: "translate.clarify",
                thread: Some(thread),
                latency: Some(started.elapsed()),
                ..Event::default()
            });
            return Response {
                source_lang: Some(source.clone()),
                translations: targets
                    .iter()
                    .map(|t| Translation::original(t, &req.text, Status::NeedsClarification))
                    .collect(),
                clarification: Some(c),
            };
        }

        let prepared = Prepared {
            req,
            source: &source,
            high_stakes: risk::high_stakes(&masked),
            words: textnorm::words(&req.text).len(),
            masked,
            snapshot,
            unaccented_vi,
            thread,
        };
        // Bước 5–7: mỗi ngôn ngữ đích một lần.
        let mut translations = Vec::with_capacity(targets.len());
        for (i, target) in targets.iter().enumerate() {
            let t = if *target == source {
                Translation::original(target, &req.text, Status::SameLanguage)
            } else {
                self.translate_one(
                    &prepared,
                    target,
                    glossaries[i].as_deref(),
                    &terms[i],
                    on_stream,
                )
                .await
            };
            translations.push(t);
        }
        self.memory
            .record(&req.thread_id, &req.sender_id, &source, &req.text);
        Response {
            source_lang: Some(source.clone()),
            translations,
            clarification: None,
        }
    }

    async fn translate_one(
        &self,
        p: &Prepared<'_>,
        to: &str,
        glossary: Option<&Glossary>,
        terms: &[TermRef<'_>],
        on_stream: OnStream<'_>,
    ) -> Translation {
        let started = Instant::now();
        let req = p.req;
        let scope = Scope {
            domain: &req.domain,
            from: p.source,
            to,
            glossary_version: glossary.map_or(0, |g| g.version),
        };
        let cache_key = PhraseCache::eligible(&p.masked).then(|| self.cache.key(&req.text, scope));
        if let Some(hit) = cache_key.as_ref().and_then(|k| self.cache.get(k)) {
            on_stream(StreamEvent::Delta {
                lang: to,
                text: &hit,
            });
            self.emit(Event {
                name: "translate.cache_hit",
                thread: Some(p.thread.clone()),
                latency: Some(started.elapsed()),
                ..Event::default()
            });
            return Translation::translated(to, hit, Status::Cached, None);
        }

        let core = glossary.map_or_else(Vec::new, |g| g.core_terms(p.source, to));
        let mut signals = Signals {
            words: p.words,
            high_stakes: p.high_stakes,
            external_forbidden: req.external_forbidden,
            ..Signals::default()
        };
        loop {
            let nonce = prompts::nonce();
            let model_req = prompts::build(&prompts::Input {
                domain: &req.domain,
                from: p.source,
                to,
                core_terms: &core,
                terms,
                summary: p.snapshot.summary.as_deref(),
                context: &p.snapshot.lines,
                address_forms: &p.snapshot.address_forms,
                unaccented_vi: p.unaccented_vi,
                message: &p.masked.text,
                nonce: &nonce,
            });
            let first_delta: Mutex<Option<Duration>> = Mutex::new(None);
            let unmasker = Mutex::new(StreamUnmask::new(&p.masked.entities, to));
            let on_delta = |d: &str| {
                if let Ok(mut f) = first_delta.lock() {
                    f.get_or_insert_with(|| started.elapsed());
                }
                let out = unmasker.lock().map(|mut u| u.push(d)).unwrap_or_default();
                if !out.is_empty() {
                    on_stream(StreamEvent::Delta {
                        lang: to,
                        text: &out,
                    });
                }
            };
            let routed = match self
                .router
                .complete(
                    select_tier(&signals),
                    &model_req,
                    req.external_forbidden,
                    &on_delta,
                )
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    on_stream(StreamEvent::Reset { lang: to });
                    self.emit(Event {
                        name: "translate.error",
                        thread: Some(p.thread.clone()),
                        latency: Some(started.elapsed()),
                        error: Some(e.code()),
                        ..Event::default()
                    });
                    return Translation::original(to, &req.text, Status::Failed);
                }
            };
            let rest = unmasker.lock().map(|mut u| u.finish()).unwrap_or_default();
            if !rest.is_empty() {
                on_stream(StreamEvent::Delta {
                    lang: to,
                    text: &rest,
                });
            }

            // Bước 6: kiểm tra sau dịch.
            let raw = &routed.completion.text;
            let text = unmask(raw, &p.masked.entities, to);
            let mut issues = check_entities(raw, &p.masked);
            issues.extend(check_glossary(&req.text, &text, terms));
            self.emit(Event {
                name: "translate.done",
                thread: Some(p.thread.clone()),
                tier: Some(routed.tier.as_str()),
                latency: Some(started.elapsed()),
                ttft: first_delta.lock().ok().and_then(|f| *f),
                tokens_in: Some(routed.completion.usage.tokens_in),
                tokens_out: Some(routed.completion.usage.tokens_out),
                error: issues.first().map(|i| i.code.as_str()),
            });

            if issues.is_empty() {
                if let Some(k) = cache_key {
                    self.cache.put(k, scope, &text);
                }
                return Translation::translated(to, text, Status::Translated, Some(routed.tier));
            }
            // Sai → chuyển mô hình mạnh một lần.
            if !signals.validator_failed && routed.tier != Tier::Strong && !req.external_forbidden {
                signals.validator_failed = true;
                on_stream(StreamEvent::Reset { lang: to });
                continue;
            }
            // Sai số/thực thể thì không bao giờ hiển thị; chỉ lệch glossary thì
            // vẫn hiển thị kèm danh sách lỗi.
            if issues.iter().any(|i| i.code != IssueCode::GlossaryMiss) {
                on_stream(StreamEvent::Reset { lang: to });
                return Translation::original(to, &req.text, Status::Failed);
            }
            let mut t = Translation::translated(to, text, Status::Translated, Some(routed.tier));
            t.issues = issues.into_iter().map(|i| i.code).collect();
            return t;
        }
    }

    fn emit(&self, e: Event) {
        self.sink.emit(e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::glossary_mock;
    use model_router::{Config, MockProvider, ProviderError, Registry};
    use std::path::Path;
    use telemetry::Recorder;

    fn translator_with(provider: MockProvider) -> (Translator, Arc<Recorder>) {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../glossary/data");
        let store = Store::load_dir(&dir).unwrap();
        let provider = Arc::new(provider);
        let mut reg = Registry::default();
        reg.register("mock", Box::new(move |_| provider.clone()));
        let cfg = Config::parse(include_str!("../../../config/models.toml")).unwrap();
        let router = Router::new(&cfg, &reg).unwrap();
        let rec = Arc::new(Recorder::default());
        let secrets = Secrets {
            cache_key: b"cache".to_vec(),
            id_hash_key: b"ids".to_vec(),
        };
        let t = Translator::new(Arc::new(store), Arc::new(router), rec.clone(), &secrets)
            .with_cache(PhraseCache::new(b"cache", 100, MAX_TTL, 1));
        (t, rec)
    }

    fn request(text: &str, targets: &[&str]) -> Request {
        Request {
            thread_id: "thread-1".into(),
            sender_id: "user-1".into(),
            text: text.into(),
            source_lang: None,
            unaccented_vi: false,
            domain: "automotive".into(),
            target_langs: targets.iter().map(|s| s.to_string()).collect(),
            external_forbidden: false,
            skip_clarification: false,
        }
    }

    fn collect() -> (Arc<Mutex<String>>, impl Fn(StreamEvent<'_>) + Send + Sync) {
        let buf = Arc::new(Mutex::new(String::new()));
        let b = buf.clone();
        let f = move |e: StreamEvent<'_>| match e {
            StreamEvent::Delta { text, .. } => b.lock().unwrap().push_str(text),
            StreamEvent::Reset { .. } => b.lock().unwrap().clear(),
        };
        (buf, f)
    }

    #[tokio::test]
    async fn translates_with_entities_glossary_and_streaming() {
        let (t, _) = translator_with(glossary_mock());
        let (streamed, on) = collect();
        let r = t
            .translate(
                &request("a oi dat coc 5 củ de lai thu nhe", &["en", "en", "vi"]),
                &on,
            )
            .await;
        assert_eq!(r.source_lang.as_deref(), Some("vi"));
        assert_eq!(
            r.translations.len(),
            2,
            "trùng ngôn ngữ đích chỉ dịch một lần"
        );
        let en = &r.translations[0];
        assert_eq!(en.status, Status::Translated);
        assert!(en.text.contains("5 million VND"), "{}", en.text);
        assert!(en.text.contains("deposit") && en.text.contains("test drive"));
        assert!(en.issues.is_empty());
        assert_eq!(*streamed.lock().unwrap(), en.text);
        assert!(!streamed.lock().unwrap().contains('⟦'));
        assert_eq!(r.translations[1].status, Status::SameLanguage);
        assert_eq!(t.memory().snapshot("thread-1", "user-2").lines.len(), 1);
    }

    #[tokio::test]
    async fn retries_on_strong_tier_when_validator_fails() {
        let mock = MockProvider::with_fn(|model, req| {
            let msg = prompts::extract_message(&req.body).unwrap_or_default();
            if model == "mock-fast" {
                "Delivery in three days".into() // làm mất placeholder
            } else {
                msg.to_owned()
            }
        });
        let (t, rec) = translator_with(mock);
        let (streamed, on) = collect();
        let r = t
            .translate(&request("Xe đẹp, giao trong 3 ngày", &["en"]), &on)
            .await;
        assert_eq!(r.translations[0].status, Status::Translated);
        assert_eq!(r.translations[0].tier, Some("strong"));
        assert!(!streamed.lock().unwrap().contains("three"));
        let tiers: Vec<_> = rec.events().iter().filter_map(|e| e.tier).collect();
        assert_eq!(tiers, ["fast", "strong"]);
    }

    #[tokio::test]
    async fn money_goes_straight_to_strong_tier() {
        let (t, rec) = translator_with(glossary_mock());
        let (_, on) = collect();
        t.translate(&request("Xe đẹp, cọc 5 củ", &["en"]), &on)
            .await;
        let tiers: Vec<_> = rec.events().iter().filter_map(|e| e.tier).collect();
        assert_eq!(tiers, ["strong"]);
    }

    #[tokio::test]
    async fn never_shows_wrong_numbers() {
        let (t, _) = translator_with(MockProvider::with_fn(|_, _| "Deposit 6 million".into()));
        let (streamed, on) = collect();
        let r = t
            .translate(&request("Xe đẹp quá, cọc 5 củ", &["en"]), &on)
            .await;
        let tr = &r.translations[0];
        assert_eq!(tr.status, Status::Failed);
        assert!(tr.retryable);
        assert_eq!(tr.text, "Xe đẹp quá, cọc 5 củ");
        assert_eq!(tr.label_key, Some("translation.failed_retry"));
        assert!(streamed.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn provider_failure_returns_original() {
        let (t, rec) = translator_with(MockProvider::failing(ProviderError::Unavailable));
        let (_, on) = collect();
        let r = t.translate(&request("Xe đẹp quá", &["en"]), &on).await;
        assert_eq!(r.translations[0].status, Status::Failed);
        assert_eq!(r.translations[0].text, "Xe đẹp quá");
        assert!(
            rec.events()
                .iter()
                .any(|e| e.error == Some("provider_unavailable"))
        );
    }

    #[tokio::test]
    async fn asks_sender_before_translating_ambiguous_dates() {
        let (t, _) = translator_with(glossary_mock());
        let (_, on) = collect();
        let mut req = request("Can we do the test drive on 3/4?", &["vi"]);
        let r = t.translate(&req, &on).await;
        assert!(r.clarification.is_some());
        assert_eq!(r.translations[0].status, Status::NeedsClarification);
        assert_eq!(
            t.memory().thread_count(),
            0,
            "chưa ghi ngữ cảnh khi chờ làm rõ"
        );
        req.skip_clarification = true;
        let r = t.translate(&req, &on).await;
        assert!(r.clarification.is_none());
        assert_eq!(r.translations[0].status, Status::Translated);
        assert!(r.translations[0].text.contains("lái thử"));
    }

    #[tokio::test]
    async fn short_phrases_hit_the_cache() {
        let (t, _) = translator_with(glossary_mock());
        let (_, on) = collect();
        let req = request("vâng em cảm ơn ạ", &["en"]);
        assert_eq!(
            t.translate(&req, &on).await.translations[0].status,
            Status::Translated
        );
        assert_eq!(
            t.translate(&req, &on).await.translations[0].status,
            Status::Cached
        );
    }

    #[tokio::test]
    async fn telemetry_never_contains_message_content() {
        let (t, rec) = translator_with(glossary_mock());
        let (_, on) = collect();
        let secret_text = "Gọi em 0901234567, cọc 5 củ ở showroom Quận 7";
        t.translate(&request(secret_text, &["en"]), &on).await;
        let events = rec.events();
        assert!(!events.is_empty());
        let json = serde_json::to_string(&events).unwrap();
        for needle in [
            "0901234567",
            "showroom",
            "Quận",
            "thread-1",
            "user-1",
            "million",
        ] {
            assert!(!json.contains(needle), "telemetry lộ {needle}");
        }
    }
}
