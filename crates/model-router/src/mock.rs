use crate::{BoxFuture, Completion, ModelRequest, OnDelta, Provider, ProviderError, Usage};
use std::sync::Arc;
use std::time::Duration;

type Respond = Arc<dyn Fn(&str, &ModelRequest) -> String + Send + Sync>;

/// Provider giả cho dev/test/eval: không gọi mạng, stream từng từ.
#[derive(Clone)]
pub struct MockProvider {
    respond: Option<Respond>,
    error: Option<ProviderError>,
    delay: Duration,
}

impl MockProvider {
    /// Trả nguyên phần body.
    pub fn echo() -> Self {
        Self::with_fn(|_, req| req.body.clone())
    }

    /// Trả lời bằng hàm `f(model, request)`.
    pub fn with_fn(f: impl Fn(&str, &ModelRequest) -> String + Send + Sync + 'static) -> Self {
        Self {
            respond: Some(Arc::new(f)),
            error: None,
            delay: Duration::ZERO,
        }
    }

    pub fn failing(e: ProviderError) -> Self {
        Self {
            respond: None,
            error: Some(e),
            delay: Duration::ZERO,
        }
    }

    pub fn with_delay(mut self, d: Duration) -> Self {
        self.delay = d;
        self
    }
}

fn approx_tokens(s: &str) -> u32 {
    u32::try_from(s.split_whitespace().count()).unwrap_or(u32::MAX)
}

impl Provider for MockProvider {
    fn complete<'a>(
        &'a self,
        model: &'a str,
        req: &'a ModelRequest,
        on_delta: OnDelta<'a>,
    ) -> BoxFuture<'a, Result<Completion, ProviderError>> {
        Box::pin(async move {
            if !self.delay.is_zero() {
                tokio::time::sleep(self.delay).await;
            }
            if let Some(e) = self.error {
                return Err(e);
            }
            let respond = self.respond.as_ref().ok_or(ProviderError::Unavailable)?;
            let text = respond(model, req);
            for piece in text.split_inclusive(' ') {
                on_delta(piece);
            }
            Ok(Completion {
                usage: Usage {
                    tokens_in: approx_tokens(&req.prefix) + approx_tokens(&req.body),
                    tokens_out: approx_tokens(&text),
                },
                text,
            })
        })
    }
}
