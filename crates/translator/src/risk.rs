//! Bước 4: đánh giá tối nghĩa, và tín hiệu rủi ro cho router.
//!
//! Chỉ hỏi lại người gửi khi có ≥ 2 cách hiểu hợp lý VÀ chạm tới giá, ngày, số
//! lượng, địa chỉ hoặc cam kết. Câu hỏi chỉ gửi riêng người gửi.

use serde::Serialize;
use validators::{Kind, Masked};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Topic {
    Price,
    Date,
    Quantity,
    Address,
    Commitment,
}

/// Câu hỏi làm rõ, hiển thị qua i18n ở client của người gửi.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Clarification {
    pub topic: Topic,
    pub i18n_key: &'static str,
    pub params: Vec<(&'static str, String)>,
}

const PRICE_WORDS: &[&str] = &[
    "gia",
    "bao nhieu",
    "coc",
    "dat coc",
    "tra",
    "phi",
    "tien",
    "thanh toan",
    "price",
    "cost",
    "deposit",
    "pay",
    "fee",
    "total",
];

/// Danh từ đếm / đơn vị đứng sau số: có chúng thì số không phải tiền.
const COUNTERS: &[&str] = &[
    "xe", "chiec", "cai", "nguoi", "ngay", "thang", "nam", "lan", "gio", "phut", "km", "tuan",
    "cars", "car", "people", "days", "day", "months", "years", "times", "hours", "km", "units",
];

const COMMITMENT_WORDS: &[&str] = &[
    "hop dong",
    "cam ket",
    "bao dam",
    "dam bao",
    "chot",
    "contract",
    "guarantee",
    "commit",
    "promise",
    "agree",
];

fn has_any(folded: &str, words: &[&str]) -> bool {
    words
        .iter()
        .any(|w| textnorm::contains_phrase(folded, w, false))
}

fn word_after(masked_text: &str, token: &str) -> Option<String> {
    let rest = &masked_text[masked_text.find(token)? + token.len()..];
    textnorm::words(rest)
        .into_iter()
        .next()
        .map(|w| textnorm::fold(&w))
}

pub fn assess(masked: &Masked, source_lang: &str) -> Option<Clarification> {
    let folded = textnorm::fold(&masked.text);
    for e in &masked.entities {
        match e.kind {
            // Tiếng Anh: "3/4" có thể là 4 tháng 3 hoặc 3 tháng 4.
            Kind::Date if source_lang == "en" => {
                let mut parts = e.source.split('/').map(|p| p.parse::<u32>().ok());
                if let (Some(Some(a)), Some(Some(b))) = (parts.next(), parts.next())
                    && a != b
                    && (1..=12).contains(&a)
                    && (1..=12).contains(&b)
                {
                    return Some(Clarification {
                        topic: Topic::Date,
                        i18n_key: "clarify.date_order",
                        params: vec![
                            ("value", e.source.clone()),
                            ("a", a.to_string()),
                            ("b", b.to_string()),
                        ],
                    });
                }
            }
            // "giá 500": 500 nghìn hay 500 triệu?
            Kind::Number if has_any(&folded, PRICE_WORDS) => {
                let small = e
                    .source
                    .parse::<u32>()
                    .is_ok_and(|v| (1..1000).contains(&v));
                let counted = word_after(&masked.text, &e.token)
                    .is_some_and(|w| COUNTERS.contains(&w.as_str()));
                if small && !counted {
                    return Some(Clarification {
                        topic: Topic::Price,
                        i18n_key: "clarify.amount_unit",
                        params: vec![("value", e.source.clone())],
                    });
                }
            }
            _ => {}
        }
    }
    None
}

/// Tin có giá, hợp đồng hoặc cam kết: nên dùng tầng mô hình mạnh.
pub fn high_stakes(masked: &Masked) -> bool {
    masked.entities.iter().any(|e| e.kind == Kind::Money)
        || has_any(&textnorm::fold(&masked.text), COMMITMENT_WORDS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use validators::mask;

    fn ask(text: &str, lang: &str) -> Option<Clarification> {
        assess(&mask(text, lang), lang)
    }

    #[test]
    fn asks_about_ambiguous_english_dates() {
        let c = ask("Delivery on 3/4 works", "en").unwrap();
        assert_eq!(c.topic, Topic::Date);
        assert_eq!(c.params[1], ("a", "3".to_owned()));
        assert!(ask("Delivery on 11/15 works", "en").is_none());
        assert!(ask("Delivery on 4/4 works", "en").is_none());
        // Tiếng Việt quy ước ngày/tháng: không tối nghĩa.
        assert!(ask("Giao xe ngày 3/4 nhé", "vi").is_none());
    }

    #[test]
    fn asks_about_unitless_prices_only() {
        let c = ask("giá 500 được không anh", "vi").unwrap();
        assert_eq!(c.topic, Topic::Price);
        assert_eq!(c.params, [("value", "500".to_owned())]);
        assert!(ask("giá 500k được không anh", "vi").is_none());
        assert!(ask("cọc 2 xe luôn", "vi").is_none());
        assert!(ask("3 ngày nữa giao", "vi").is_none());
        assert!(ask("gia 2000 la sao", "vi").is_none());
        // Giờ không phải số tiền.
        let en = "Your test drive is at 9:30 tomorrow, and the deposit is fully refundable.";
        assert!(ask(en, "en").is_none());
    }

    #[test]
    fn high_stakes_detection() {
        assert!(high_stakes(&mask("cọc 5 củ nhé", "vi")));
        assert!(high_stakes(&mask("em gửi hợp đồng anh xem", "vi")));
        assert!(!high_stakes(&mask("xe đẹp quá", "vi")));
    }
}
