//! Chuẩn hoá văn bản: chữ thường, bỏ dấu tiếng Việt, tách từ.
//! Dùng chung cho nhận diện ngôn ngữ, tra glossary, kiểm tra sau dịch và khoá cache.

use std::collections::HashMap;
use std::sync::LazyLock;

static FOLD: LazyLock<HashMap<char, char>> = LazyLock::new(|| {
    let groups = [
        ('a', "àáảãạăằắẳẵặâầấẩẫậ"),
        ('e', "èéẻẽẹêềếểễệ"),
        ('i', "ìíỉĩị"),
        ('o', "òóỏõọôồốổỗộơờớởỡợ"),
        ('u', "ùúủũụưừứửữự"),
        ('y', "ỳýỷỹỵ"),
        ('d', "đ"),
    ];
    let mut m = HashMap::new();
    for (base, chars) in groups {
        for c in chars.chars() {
            m.insert(c, base);
        }
    }
    m
});

/// Dấu tổ hợp (dạng NFD), ví dụ "e\u{301}".
fn is_combining(c: char) -> bool {
    ('\u{300}'..='\u{36f}').contains(&c)
}

fn lower(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// Ký tự có phải chữ cái tiếng Việt có dấu (hoặc đ) không.
pub fn has_vietnamese_marks(c: char) -> bool {
    FOLD.contains_key(&lower(c))
}

/// Chữ thường, bỏ dấu tiếng Việt (kể cả dấu tổ hợp).
pub fn fold(s: &str) -> String {
    s.chars()
        .filter(|c| !is_combining(*c))
        .flat_map(char::to_lowercase)
        .map(|c| FOLD.get(&c).copied().unwrap_or(c))
        .collect()
}

/// Tách thành các từ chữ thường; mọi ký tự không phải chữ/số là dấu ngắt.
pub fn words(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || is_combining(c)))
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Gộp khoảng trắng và chuyển chữ thường, giữ dấu.
pub fn normalize(s: &str) -> String {
    s.to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// `phrase` có xuất hiện trong `text` theo ranh giới từ không.
/// `fold = true` thì không phân biệt dấu (cho tiếng Việt không dấu).
pub fn contains_phrase(text: &str, phrase: &str, fold_marks: bool) -> bool {
    let (hay, needle) = if fold_marks {
        (words(&fold(text)), words(&fold(phrase)))
    } else {
        (words(text), words(phrase))
    };
    !needle.is_empty() && hay.windows(needle.len()).any(|w| w == needle.as_slice())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fold_strips_vietnamese_marks() {
        for (input, want) in [
            ("Trạm Sạc", "tram sac"),
            ("Đặt cọc", "dat coc"),
            ("giá lăn bánh", "gia lan banh"),
            ("Xe e\u{301}", "xe e"),
            ("already plain", "already plain"),
        ] {
            assert_eq!(fold(input), want, "fold({input:?})");
        }
    }

    #[test]
    fn contains_phrase_respects_word_boundaries() {
        assert!(contains_phrase("Anh muốn lái thử xe", "lái thử", false));
        assert!(contains_phrase("a muon lai thu xe", "lái thử", true));
        assert!(!contains_phrase("a muon lai thu xe", "lái thử", false));
        assert!(!contains_phrase("pinch me", "pin", false));
        assert!(!contains_phrase("anything", "", false));
    }

    #[test]
    fn marks_detection() {
        assert!(has_vietnamese_marks('Đ'));
        assert!(has_vietnamese_marks('ở'));
        assert!(!has_vietnamese_marks('o'));
    }
}
