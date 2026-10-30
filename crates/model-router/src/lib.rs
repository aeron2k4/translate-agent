//! Lớp trừu tượng nhà cung cấp mô hình. Mọi lời gọi LLM đi qua đây.
//!
//! - Ba tầng: nhanh (mặc định), mạnh (tin dài/tối nghĩa/giá/validator thất bại),
//!   dự phòng (LLM lỗi hoặc hội thoại cấm gửi ra ngoài — chỉ chạy nội bộ).
//! - Tên mô hình chỉ nằm trong `config/models.toml`.
//! - Nhà cung cấp phải có thoả thuận không lưu dữ liệu và đã được duyệt.

mod config;
mod mock;

pub use config::{Config, ConfigError, ProviderConfig, TierConfig};
pub use mock::MockProvider;

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tier {
    Fast,
    Strong,
    Fallback,
}

impl Tier {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fast => "fast",
            Self::Strong => "strong",
            Self::Fallback => "fallback",
        }
    }
}

/// Yêu cầu gửi mô hình. `prefix` là phần tĩnh (system prompt, glossary lõi) để
/// nhà cung cấp cache; `body` là phần thay đổi theo tin.
#[derive(Debug, Clone)]
pub struct ModelRequest {
    pub prefix: String,
    pub body: String,
    pub max_tokens: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    pub tokens_in: u32,
    pub tokens_out: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub text: String,
    pub usage: Usage,
}

/// Lỗi nhà cung cấp. Không mang thông điệp tự do (có thể lẫn nội dung).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderError {
    Timeout,
    Unavailable,
    Refused,
    BadResponse,
}

impl ProviderError {
    pub fn code(self) -> &'static str {
        match self {
            Self::Timeout => "provider_timeout",
            Self::Unavailable => "provider_unavailable",
            Self::Refused => "provider_refused",
            Self::BadResponse => "provider_bad_response",
        }
    }
}

/// Hàm nhận từng đoạn bản dịch khi stream.
pub type OnDelta<'a> = &'a (dyn Fn(&str) + Send + Sync);

pub trait Provider: Send + Sync {
    fn complete<'a>(
        &'a self,
        model: &'a str,
        req: &'a ModelRequest,
        on_delta: OnDelta<'a>,
    ) -> BoxFuture<'a, Result<Completion, ProviderError>>;
}

/// Tín hiệu để chọn tầng.
#[derive(Debug, Clone, Copy, Default)]
pub struct Signals {
    pub words: usize,
    pub ambiguous: bool,
    /// Hợp đồng, giá, cam kết.
    pub high_stakes: bool,
    pub validator_failed: bool,
    /// Hội thoại cấm gửi ra ngoài: chỉ được dùng tầng chạy nội bộ.
    pub external_forbidden: bool,
}

/// Tin từ ngưỡng này trở lên coi là dài.
pub const LONG_MESSAGE_WORDS: usize = 60;

pub fn select_tier(s: &Signals) -> Tier {
    if s.external_forbidden {
        Tier::Fallback
    } else if s.validator_failed || s.ambiguous || s.high_stakes || s.words >= LONG_MESSAGE_WORDS {
        Tier::Strong
    } else {
        Tier::Fast
    }
}

struct Route {
    provider: Arc<dyn Provider>,
    model: String,
    timeout: Duration,
    external: bool,
}

/// Hàm dựng provider theo `kind` trong cấu hình.
pub type Factory = Box<dyn Fn(&ProviderConfig) -> Arc<dyn Provider> + Send + Sync>;

#[derive(Default)]
pub struct Registry {
    factories: HashMap<String, Factory>,
}

impl Registry {
    /// Registry chỉ có provider mock (echo).
    pub fn with_mock() -> Self {
        let mut r = Self::default();
        r.register("mock", Box::new(|_| Arc::new(MockProvider::echo())));
        r
    }

    pub fn register(&mut self, kind: &str, f: Factory) {
        self.factories.insert(kind.to_owned(), f);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouterError {
    Provider(ProviderError),
    /// Hội thoại cấm gửi ra ngoài mà tầng dự phòng lại là nhà cung cấp bên ngoài.
    NoInternalRoute,
}

impl RouterError {
    pub fn code(self) -> &'static str {
        match self {
            Self::Provider(e) => e.code(),
            Self::NoInternalRoute => "no_internal_route",
        }
    }
}

pub struct Routed {
    pub completion: Completion,
    pub tier: Tier,
}

pub struct Router {
    routes: HashMap<Tier, Route>,
}

impl Router {
    pub fn new(cfg: &Config, registry: &Registry) -> Result<Self, ConfigError> {
        let mut routes = HashMap::new();
        for (tier, tc) in cfg.tiers()? {
            let pc = cfg
                .providers
                .get(&tc.provider)
                .ok_or_else(|| ConfigError::UnknownProvider(tc.provider.clone()))?;
            if !pc.no_retention || !pc.approved {
                return Err(ConfigError::NotApproved(tc.provider.clone()));
            }
            let factory = registry
                .factories
                .get(&pc.kind)
                .ok_or_else(|| ConfigError::UnknownKind(pc.kind.clone()))?;
            routes.insert(
                tier,
                Route {
                    provider: factory(pc),
                    model: tc.model.clone(),
                    timeout: Duration::from_millis(tc.timeout_ms),
                    external: pc.external,
                },
            );
        }
        Ok(Self { routes })
    }

    async fn call(
        &self,
        tier: Tier,
        req: &ModelRequest,
        on_delta: OnDelta<'_>,
    ) -> Result<Completion, ProviderError> {
        let route = self.routes.get(&tier).ok_or(ProviderError::Unavailable)?;
        tokio::time::timeout(
            route.timeout,
            route.provider.complete(&route.model, req, on_delta),
        )
        .await
        .unwrap_or(Err(ProviderError::Timeout))
    }

    /// Gọi tầng `tier`; nếu lỗi thì chuyển sang tầng dự phòng. Khi
    /// `external_forbidden`, chỉ gọi tầng chạy nội bộ.
    pub async fn complete(
        &self,
        tier: Tier,
        req: &ModelRequest,
        external_forbidden: bool,
        on_delta: OnDelta<'_>,
    ) -> Result<Routed, RouterError> {
        let is_external = |t: Tier| self.routes.get(&t).is_none_or(|r| r.external);
        let tier = if external_forbidden {
            Tier::Fallback
        } else {
            tier
        };
        if external_forbidden && is_external(Tier::Fallback) {
            return Err(RouterError::NoInternalRoute);
        }
        match self.call(tier, req, on_delta).await {
            Ok(completion) => Ok(Routed { completion, tier }),
            Err(e) if tier == Tier::Fallback => Err(RouterError::Provider(e)),
            Err(_) => self
                .call(Tier::Fallback, req, on_delta)
                .await
                .map(|completion| Routed {
                    completion,
                    tier: Tier::Fallback,
                })
                .map_err(RouterError::Provider),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn config(extra: &str) -> Config {
        Config::parse(&format!(
            r#"
            [providers.ext]
            kind = "ext"
            no_retention = true
            approved = true
            external = true
            [providers.local]
            kind = "local"
            no_retention = true
            approved = true
            external = false
            [tiers.fast]
            provider = "ext"
            model = "m-fast"
            timeout_ms = 50
            [tiers.strong]
            provider = "ext"
            model = "m-strong"
            timeout_ms = 50
            [tiers.fallback]
            provider = "local"
            model = "m-mt"
            timeout_ms = 50
            {extra}
            "#
        ))
        .unwrap()
    }

    fn registry(ext: MockProvider) -> Registry {
        let ext = Arc::new(ext);
        let mut r = Registry::default();
        r.register("ext", Box::new(move |_| ext.clone()));
        r.register(
            "local",
            Box::new(|_| Arc::new(MockProvider::with_fn(|_, _| "from-local".into()))),
        );
        r
    }

    fn req() -> ModelRequest {
        ModelRequest {
            prefix: "p".into(),
            body: "b".into(),
            max_tokens: 64,
        }
    }

    #[test]
    fn tier_policy() {
        assert_eq!(select_tier(&Signals::default()), Tier::Fast);
        let long = Signals {
            words: LONG_MESSAGE_WORDS,
            ..Default::default()
        };
        assert_eq!(select_tier(&long), Tier::Strong);
        let pricey = Signals {
            high_stakes: true,
            ..Default::default()
        };
        assert_eq!(select_tier(&pricey), Tier::Strong);
        let internal = Signals {
            external_forbidden: true,
            validator_failed: true,
            ..Default::default()
        };
        assert_eq!(select_tier(&internal), Tier::Fallback);
    }

    #[tokio::test]
    async fn uses_model_name_from_config_and_streams() {
        let seen = Arc::new(Mutex::new(String::new()));
        let s = seen.clone();
        let ext = MockProvider::with_fn(move |model, _| {
            *s.lock().unwrap() = model.to_owned();
            "hello world".into()
        });
        let router = Router::new(&config(""), &registry(ext)).unwrap();
        let chunks = Mutex::new(Vec::new());
        let out = router
            .complete(Tier::Strong, &req(), false, &|d| {
                chunks.lock().unwrap().push(d.to_owned())
            })
            .await
            .ok()
            .unwrap();
        assert_eq!(out.tier, Tier::Strong);
        assert_eq!(out.completion.text, "hello world");
        assert_eq!(*seen.lock().unwrap(), "m-strong");
        assert_eq!(chunks.lock().unwrap().concat(), "hello world");
    }

    #[tokio::test]
    async fn falls_back_on_error_and_timeout() {
        for ext in [
            MockProvider::failing(ProviderError::Unavailable),
            MockProvider::echo().with_delay(Duration::from_millis(200)),
        ] {
            let router = Router::new(&config(""), &registry(ext)).unwrap();
            let out = router
                .complete(Tier::Fast, &req(), false, &|_| {})
                .await
                .ok()
                .unwrap();
            assert_eq!(out.tier, Tier::Fallback);
            assert_eq!(out.completion.text, "from-local");
        }
    }

    #[tokio::test]
    async fn external_forbidden_never_calls_external_provider() {
        let ext = MockProvider::with_fn(|_, _| panic!("không được gọi nhà cung cấp bên ngoài"));
        let router = Router::new(&config(""), &registry(ext)).unwrap();
        let out = router
            .complete(Tier::Strong, &req(), true, &|_| {})
            .await
            .ok()
            .unwrap();
        assert_eq!(out.tier, Tier::Fallback);
    }

    #[test]
    fn rejects_unapproved_or_retaining_providers() {
        for flags in [
            "no_retention = false\napproved = true",
            "no_retention = true\napproved = false",
        ] {
            let cfg = Config::parse(&format!(
                r#"
                [providers.p]
                kind = "mock"
                {flags}
                [tiers.fast]
                provider = "p"
                model = "m"
                [tiers.strong]
                provider = "p"
                model = "m"
                [tiers.fallback]
                provider = "p"
                model = "m"
                "#
            ))
            .unwrap();
            assert!(matches!(
                Router::new(&cfg, &Registry::with_mock()),
                Err(ConfigError::NotApproved(_))
            ));
        }
    }

    #[test]
    fn repo_config_is_valid() {
        let raw = include_str!("../../../config/models.toml");
        let cfg = Config::parse(raw).unwrap();
        assert!(Router::new(&cfg, &Registry::with_mock()).is_ok());
    }
}
