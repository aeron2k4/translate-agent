//! Chuỗi hiển thị cho người dùng. Không hard-code chuỗi ở UI; luôn đi qua đây.

use std::collections::HashMap;
use std::sync::LazyLock;

type Catalog = HashMap<String, String>;

static CATALOGS: LazyLock<HashMap<&'static str, Catalog>> = LazyLock::new(|| {
    [
        ("vi", include_str!("../i18n/vi.json")),
        ("en", include_str!("../i18n/en.json")),
    ]
    .into_iter()
    .map(|(locale, raw)| (locale, serde_json::from_str(raw).unwrap_or_default()))
    .collect()
});

pub const DEFAULT_LOCALE: &str = "vi";

/// Lấy chuỗi theo khoá, thay `{tên}` bằng tham số. Thiếu bản dịch cho locale
/// thì dùng locale mặc định; thiếu hẳn thì trả về chính khoá.
pub fn t(key: &str, locale: &str, params: &[(&str, &str)]) -> String {
    let lookup = |loc: &str| CATALOGS.get(loc).and_then(|c| c.get(key));
    let mut s = lookup(locale)
        .or_else(|| lookup(DEFAULT_LOCALE))
        .cloned()
        .unwrap_or_else(|| key.to_owned());
    for (name, value) in params {
        s = s.replace(&format!("{{{name}}}"), value);
    }
    s
}

/// Toàn bộ chuỗi của một locale (cho client web/dev tải một lần).
pub fn catalog(locale: &str) -> Option<&'static HashMap<String, String>> {
    CATALOGS.get(locale)
}

pub fn locales() -> impl Iterator<Item = &'static str> {
    CATALOGS.keys().copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn all_locales_have_the_same_keys() {
        let keys: Vec<BTreeSet<&String>> = CATALOGS.values().map(|c| c.keys().collect()).collect();
        assert_eq!(keys.len(), 2, "catalog JSON phải parse được");
        assert!(keys.windows(2).all(|w| w[0] == w[1]));
        assert!(!keys[0].is_empty());
    }

    #[test]
    fn substitutes_params_and_falls_back() {
        assert_eq!(t("translation.failed_retry", "vi", &[]), "Thử dịch lại");
        assert_eq!(t("translation.failed_retry", "fr", &[]), "Thử dịch lại");
        assert_eq!(
            t(
                "clarify.date_order",
                "vi",
                &[("value", "3/4"), ("a", "3"), ("b", "4")]
            ),
            "Ngày \"3/4\" là ngày 3 tháng 4 hay ngày 4 tháng 3?"
        );
        assert_eq!(t("missing.key", "en", &[]), "missing.key");
    }
}
