//! Suy ra cách xưng hô (anh/chị/em…) từ một tin tiếng Việt bằng luật đơn giản.
//!
//! Từ xưng hô tiếng Việt vừa là "tôi" vừa là "bạn" tuỳ người nói, nên mô hình hay
//! đảo vai ("Anh đặt cọc nhé em" bị dịch thành "Please put down…"). Luật:
//! - Hô gọi chỉ người đọc: "anh ơi, …" ở đầu câu, hoặc từ xưng hô đứng sau trợ từ ở
//!   cuối câu ("… nhé em", "… không anh?").
//! - Khi đã biết người đọc, từ xưng hô khác đứng đầu câu là người gửi tự xưng.
//!
//! Không chắc thì không đoán (trả `None`). Chỉ chạy trong RAM, không log.

use textnorm::{fold, has_vietnamese_marks, words};

/// Cách xưng hô suy ra được từ một tin.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cues {
    /// Người gửi tự xưng ("anh" trong "Anh đặt cọc nhé em").
    pub sender: Option<&'static str>,
    /// Người đọc được gọi là ("em" trong "Anh đặt cọc nhé em").
    pub reader: Option<&'static str>,
}

/// Trợ từ hay đứng trước từ hô gọi ở cuối câu (đã bỏ dấu).
const PARTICLES: &[&str] = &[
    "nhe", "nha", "nhi", "nhen", "a", "oi", "voi", "ha", "khong", "ko", "k", "chua", "di", "on",
    "day", "ne", "nao", "sao", "the", "duoc",
];

/// Từ xưng hô (chữ thường, giữ dấu) → dạng chuẩn.
fn kinship(token: &str, unaccented: bool) -> Option<&'static str> {
    match token {
        "anh" | "a" => Some("anh"),
        "chị" | "c" => Some("chị"),
        "em" | "e" => Some("em"),
        "ông" => Some("ông"),
        "bà" => Some("bà"),
        "cô" => Some("cô"),
        "chú" => Some("chú"),
        "bác" => Some("bác"),
        // Không dấu: chỉ nhận "chi"; "co", "ba", "chu" trùng từ thường gặp (có, ba…).
        "chi" if unaccented => Some("chị"),
        _ => None,
    }
}

fn sentence_cues(sentence: &str, unaccented: bool) -> Cues {
    // Từ chữ thường giữ dấu, song song với dạng đã bỏ dấu để so trợ từ.
    let raw: Vec<String> = words(sentence);
    let folded: Vec<String> = raw.iter().map(|w| fold(w)).collect();
    let kin = |i: usize| raw.get(i).and_then(|w| kinship(w, unaccented));

    let mut start = 0;
    while folded
        .get(start)
        .is_some_and(|w| matches!(w.as_str(), "da" | "vang" | "ok"))
    {
        start += 1;
    }
    let mut reader = None;
    if let Some(k) = kin(start)
        && folded.get(start + 1).is_some_and(|w| w == "oi")
    {
        reader = Some(k);
        start += 2;
    }
    let n = raw.len();
    if reader.is_none()
        && n >= 2
        && let Some(k) = kin(n - 1)
        && PARTICLES.contains(&folded[n - 2].as_str())
    {
        reader = Some(k);
    }
    let sender = match (reader, kin(start)) {
        (Some(r), Some(s)) if s != r && start + 1 < n => Some(s),
        _ => None,
    };
    Cues { sender, reader }
}

/// Cách xưng hô trong một tin tiếng Việt; câu đầu tiên có tín hiệu thì thắng.
pub fn cues(text: &str) -> Cues {
    let unaccented = !text.chars().any(has_vietnamese_marks);
    let mut out = Cues::default();
    for s in text.split(['.', '!', '?', '\n', ';']) {
        let c = sentence_cues(s, unaccented);
        out.sender = out.sender.or(c.sender);
        out.reader = out.reader.or(c.reader);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(text: &str) -> (Option<&'static str>, Option<&'static str>) {
        let c = cues(text);
        (c.sender, c.reader)
    }

    #[test]
    fn trailing_vocative_names_reader_and_leading_word_the_sender() {
        assert_eq!(
            pair("Anh đặt cọc 1tr2 trước nhé em."),
            (Some("anh"), Some("em"))
        );
        assert_eq!(
            pair("C dat coc 500k truoc nhe e."),
            (Some("chị"), Some("em"))
        );
        assert_eq!(
            pair("Chi muon lai thu khong em?"),
            (Some("chị"), Some("em"))
        );
    }

    #[test]
    fn leading_vocative_then_self_reference() {
        assert_eq!(
            pair("Em ơi, chị muốn hỏi về xe điện."),
            (Some("chị"), Some("em"))
        );
        assert_eq!(
            pair("Dạ anh ơi, giá lăn bánh bao nhiêu ạ?"),
            (None, Some("anh"))
        );
        assert_eq!(pair("A oi, gia lan banh bao nhieu a?"), (None, Some("anh")));
    }

    #[test]
    fn question_tag_vocative() {
        assert_eq!(
            pair("Xe 5 chỗ còn màu trắng không anh?"),
            (None, Some("anh"))
        );
    }

    #[test]
    fn no_guess_without_a_vocative_or_on_ambiguous_words() {
        assert_eq!(pair("Em gọi anh số 0900000001 nhé."), (None, None));
        assert_eq!(pair("Giá bao nhiêu ạ?"), (None, None));
        // "co" không dấu là "có", không phải "cô".
        assert_eq!(pair("Co mau trang khong em?"), (None, Some("em")));
        // "chi" trong câu có dấu là "chi" (chi tiêu), không phải "chị".
        assert_eq!(pair("Chi phí bảo dưỡng thế nào em?"), (None, Some("em")));
    }
}
