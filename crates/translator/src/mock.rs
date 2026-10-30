//! Provider giả dùng cho dev, test và eval cơ chế (KHÔNG phải mô hình dịch thật).
//!
//! Lấy tin trong prompt, thay các thuật ngữ glossary có trong prompt bằng bản
//! dịch đã duyệt, giữ nguyên placeholder. Đủ để kiểm tra toàn bộ đường ống
//! (che thực thể, glossary, validator, stream) mà không gọi mạng.

use model_router::{MockProvider, ModelRequest};

fn glossary_pairs(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|l| l.strip_prefix("- "))
        .filter_map(|l| l.split_once(" => "))
        .map(|(s, t)| (s.to_owned(), t.to_owned()))
        .collect()
}

/// Thay cụm `from` (khớp không phân biệt dấu, theo ranh giới từ) bằng `to`.
fn replace_phrase(text: &str, from: &str, to: &str) -> String {
    let folded_from = textnorm::fold(from);
    let words: Vec<&str> = text.split(' ').collect();
    let n = from.split_whitespace().count();
    if n == 0 || words.len() < n {
        return text.to_owned();
    }
    let mut out: Vec<String> = Vec::with_capacity(words.len());
    let mut i = 0;
    while i < words.len() {
        if i + n <= words.len() {
            let window = words[i..i + n].join(" ");
            let core = window.trim_end_matches([',', '.', '?', '!']);
            if textnorm::fold(core) == folded_from {
                out.push(format!("{to}{}", &window[core.len()..]));
                i += n;
                continue;
            }
        }
        out.push(words[i].to_owned());
        i += 1;
    }
    out.join(" ")
}

pub fn glossary_translate(req: &ModelRequest) -> String {
    let Some(message) = prompts::extract_message(&req.body) else {
        return String::new();
    };
    let mut pairs = glossary_pairs(&req.body);
    pairs.extend(glossary_pairs(&req.prefix));
    pairs.sort_by_key(|(s, _)| std::cmp::Reverse(s.chars().count()));
    pairs
        .iter()
        .fold(message.to_owned(), |acc, (s, t)| replace_phrase(&acc, s, t))
}

pub fn glossary_mock() -> MockProvider {
    MockProvider::with_fn(|_, req| glossary_translate(req))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_glossary_terms_and_keeps_placeholders() {
        assert_eq!(
            replace_phrase("muốn lai thu, cọc ⟦E1⟧", "lái thử", "test drive"),
            "muốn test drive, cọc ⟦E1⟧"
        );
    }
}
