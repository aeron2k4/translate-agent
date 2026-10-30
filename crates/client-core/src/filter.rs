//! Bước 1 của pipeline: lọc nhanh ở client, tránh gọi dịch khi không cần.

use crate::langdetect::{Detection, Lang, detect};
use serde::Serialize;
use validators::Kind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    Empty,
    /// Chỉ emoji / ký hiệu / dấu câu.
    EmojiOnly,
    UrlOnly,
    /// Chỉ số, mã, biển số… không có chữ để dịch.
    CodeOnly,
    SameLanguage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Decision {
    Skip { reason: SkipReason },
    Translate { detection: Detection },
}

/// Quyết định có dịch tin cho người đọc dùng `reader_lang` hay không.
/// Tin không nhận diện được vẫn gửi dịch (máy chủ sẽ tự nhận diện).
pub fn should_translate(text: &str, reader_lang: &str) -> Decision {
    let skip = |reason| Decision::Skip { reason };
    if text.trim().is_empty() {
        return skip(SkipReason::Empty);
    }
    if !text.chars().any(char::is_alphanumeric) {
        return skip(SkipReason::EmojiOnly);
    }
    let masked = validators::mask(text, "");
    let rest_has_letters = masked
        .text
        .split(['⟦', '⟧'])
        .enumerate()
        // Phần lẻ là nội dung bên trong placeholder.
        .any(|(i, part)| i % 2 == 0 && part.chars().any(char::is_alphabetic));
    if !rest_has_letters {
        let all_urls = masked.entities.iter().all(|e| e.kind == Kind::Url);
        return skip(if all_urls {
            SkipReason::UrlOnly
        } else {
            SkipReason::CodeOnly
        });
    }
    let detection = detect(text);
    let same = match detection.lang {
        Lang::Vi | Lang::En => detection.lang.code() == Some(reader_lang),
        // Tin trộn: chỉ bỏ qua khi phần lớn đã là ngôn ngữ của người đọc.
        Lang::Mixed => detection.primary.code() == Some(reader_lang),
        Lang::Unknown => false,
    };
    if same {
        return skip(SkipReason::SameLanguage);
    }
    Decision::Translate { detection }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn reason(text: &str, reader: &str) -> Option<SkipReason> {
        match should_translate(text, reader) {
            Decision::Skip { reason } => Some(reason),
            Decision::Translate { .. } => None,
        }
    }

    #[test]
    fn skips_cheap_cases() {
        assert_eq!(reason("   ", "en"), Some(SkipReason::Empty));
        assert_eq!(reason("😂😂👍", "en"), Some(SkipReason::EmojiOnly));
        assert_eq!(reason("!!!", "en"), Some(SkipReason::EmojiOnly));
        assert_eq!(reason("https://xe.x.vn/a", "en"), Some(SkipReason::UrlOnly));
        assert_eq!(reason("51A-123.45", "en"), Some(SkipReason::CodeOnly));
        assert_eq!(reason("0901234567", "en"), Some(SkipReason::CodeOnly));
        assert_eq!(reason("Xe đẹp quá", "vi"), Some(SkipReason::SameLanguage));
        assert_eq!(
            reason("See you tomorrow", "en"),
            Some(SkipReason::SameLanguage)
        );
    }

    #[test]
    fn translates_foreign_and_mixed() {
        assert_eq!(reason("Xe đẹp quá", "en"), None);
        assert_eq!(reason("a oi ko dc a", "en"), None);
        assert_eq!(reason("Anh check giúp em invoice nhé", "en"), None);
        assert_eq!(
            reason("Anh check giúp em invoice nhé", "vi"),
            Some(SkipReason::SameLanguage)
        );
    }

    #[test]
    fn stays_within_client_budget() {
        let text = "a oi xe nay gia lan banh bao nhieu, co tra gop ko, \
                    em muon dat lich lai thu thu 7 tuan sau luc 9h30 \
                    o showroom Q7, goi em 0901234567 nhe. Thanks!";
        let start = Instant::now();
        for _ in 0..100 {
            let _ = should_translate(text, "en");
        }
        // Ngân sách 10 ms/tin; để rộng cho máy CI chậm và bản build debug.
        assert!(start.elapsed() < Duration::from_millis(100 * 10));
    }
}
