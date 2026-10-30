use crate::Tier;
use serde::Deserialize;
use std::collections::HashMap;
use std::fmt;

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderConfig {
    pub kind: String,
    /// Đã ký thoả thuận không lưu dữ liệu.
    #[serde(default)]
    pub no_retention: bool,
    /// Đã được bảo mật/pháp chế duyệt.
    #[serde(default)]
    pub approved: bool,
    /// Gửi dữ liệu ra ngoài hạ tầng của Tập đoàn.
    #[serde(default = "default_true")]
    pub external: bool,
    /// Tham số riêng của provider (endpoint…). Không đặt khoá bí mật ở đây.
    #[serde(default)]
    pub options: HashMap<String, String>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
pub struct TierConfig {
    pub provider: String,
    pub model: String,
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
}

fn default_timeout() -> u64 {
    2000
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub providers: HashMap<String, ProviderConfig>,
    tiers: HashMap<String, TierConfig>,
}

#[derive(Debug)]
pub enum ConfigError {
    Parse(String),
    MissingTier(&'static str),
    UnknownProvider(String),
    UnknownKind(String),
    NotApproved(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(e) => write!(f, "cấu hình mô hình sai: {e}"),
            Self::MissingTier(t) => write!(f, "thiếu tầng {t}"),
            Self::UnknownProvider(p) => write!(f, "tầng trỏ tới provider chưa khai báo: {p}"),
            Self::UnknownKind(k) => write!(f, "chưa có cài đặt cho loại provider: {k}"),
            Self::NotApproved(p) => write!(
                f,
                "provider {p} chưa có thoả thuận không lưu dữ liệu hoặc chưa được duyệt"
            ),
        }
    }
}

impl std::error::Error for ConfigError {}

impl Config {
    pub fn parse(raw: &str) -> Result<Self, ConfigError> {
        toml::from_str(raw).map_err(|e| ConfigError::Parse(e.to_string()))
    }

    pub(crate) fn tiers(&self) -> Result<Vec<(Tier, &TierConfig)>, ConfigError> {
        [Tier::Fast, Tier::Strong, Tier::Fallback]
            .into_iter()
            .map(|t| {
                self.tiers
                    .get(t.as_str())
                    .map(|c| (t, c))
                    .ok_or(ConfigError::MissingTier(t.as_str()))
            })
            .collect()
    }
}
