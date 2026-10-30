//! Nhận diện Việt / Anh / trộn, có nhánh cho tiếng Việt không dấu.

use regex::Regex;
use serde::Serialize;
use std::collections::HashSet;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Lang {
    Vi,
    En,
    /// Trộn Việt–Anh; `primary` trong `Detection` là ngôn ngữ chiếm phần lớn.
    Mixed,
    Unknown,
}

impl Lang {
    pub fn code(self) -> Option<&'static str> {
        match self {
            Self::Vi => Some("vi"),
            Self::En => Some("en"),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Detection {
    pub lang: Lang,
    /// Ngôn ngữ chiếm phần lớn (Vi hoặc En), kể cả khi trộn.
    pub primary: Lang,
    /// Tiếng Việt viết không dấu: báo cho mô hình để hiểu đúng.
    pub unaccented_vi: bool,
}

/// Từ tiếng Việt phổ biến khi viết không dấu, kể cả viết tắt chat ("ko", "dc", "a/c").
const VI_PLAIN: &str = "anh chi em ko k khong kh hok dc duoc dk roi r nhe nha nhaa oi ah a ak \
    cho cua la va thi ma nhung co xe gia bao nhieu toi minh ban ve di den lam gi j sao nao day \
    nay kia hom mai tuan sau truoc muon biet hoi xem giup voi nua het mua tien coc dat lich hen \
    gap goi nhan tin oke vang da ac ae ntn vs bn ib sdt stk thoi luon lun cty viec xin cam loi \
    duong sang chieu toi trua ngay thang nam gio phut bay gio luc dau sap chua bi duoc hay \
    khach hang lai thu sac pin giao nhanh cham re dat tot dep moi cu chiec cai nguoi ong ba \
    chu co chau thang be";

/// Từ tiếng Anh phổ biến.
const EN_WORDS: &str = "i you he she it we they is are was were be been am will would could \
    should shall may might must have has had not no yes please thanks thank hello hey what when \
    where who why how which this that these those there here and or but if then than with \
    from for of at by about into over after before up down out our your their my his her its \
    them us all any some more most very just also only now today tomorrow yesterday week month \
    year price car cars deposit test drive book booking send sent get got make want need know \
    see call let check pay paid buy sell new used good great sure okay ok fine sorry the a an \
    to do can me so on in";

static VI_SET: LazyLock<HashSet<&'static str>> =
    LazyLock::new(|| VI_PLAIN.split_whitespace().collect());
static EN_SET: LazyLock<HashSet<&'static str>> =
    LazyLock::new(|| EN_WORDS.split_whitespace().collect());

/// Hình dạng một âm tiết tiếng Việt (đã bỏ dấu). Từ ASCII không khớp thì gần như
/// chắc chắn là tiếng Anh ("send", "invoice", "check").
static VI_SYLLABLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:ngh|ng|nh|ch|gh|gi|kh|ph|qu|th|tr|[bcdghklmnpqrstvx])?[aeiouy]{1,3}(?:ch|ng|nh|[cmnpt])?$")
        .expect("regex âm tiết")
});

static PLACEHOLDER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("⟦[^⟧]*⟧").expect("regex placeholder"));

pub fn detect(text: &str) -> Detection {
    // Bỏ số, URL, mention… trước khi đếm từ.
    let masked = validators::mask(text, "");
    let plain = PLACEHOLDER.replace_all(&masked.text, " ");
    let (mut vi_marked, mut vi_plain, mut en) = (0u32, 0u32, 0u32);
    for word in textnorm::words(&plain) {
        if word.chars().any(textnorm::has_vietnamese_marks) {
            vi_marked += 1;
            continue;
        }
        let w = textnorm::fold(&word);
        let (in_vi, in_en) = (VI_SET.contains(w.as_str()), EN_SET.contains(w.as_str()));
        match (in_vi, in_en) {
            (true, false) => vi_plain += 1,
            (false, true) => en += 1,
            (true, true) => {} // "can", "the", "an"… có nghĩa ở cả hai thứ tiếng
            (false, false) => {
                if w.is_ascii() && w.len() > 1 && !VI_SYLLABLE.is_match(&w) {
                    en += 1;
                }
            }
        }
    }
    let vi = vi_marked + vi_plain;
    let total = vi + en;
    let primary = match (vi, en) {
        (0, 0) => Lang::Unknown,
        _ if vi >= en => Lang::Vi,
        _ => Lang::En,
    };
    let lang = if total == 0 {
        Lang::Unknown
    } else {
        let share = vi as f32 / total as f32;
        if share >= 0.8 {
            Lang::Vi
        } else if share <= 0.2 {
            Lang::En
        } else {
            Lang::Mixed
        }
    };
    Detection {
        lang,
        primary,
        unaccented_vi: vi_plain > 0 && vi_marked == 0 && primary == Lang::Vi,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_accented_vietnamese() {
        let d = detect("Anh ơi xe này giá lăn bánh bao nhiêu ạ?");
        assert_eq!(d.lang, Lang::Vi);
        assert!(!d.unaccented_vi);
    }

    #[test]
    fn detects_unaccented_vietnamese_and_chat_abbreviations() {
        for text in [
            "a oi xe nay gia lan banh bao nhieu",
            "ko dc thi thoi nhe",
            "a/c cho e hoi lich lai thu",
        ] {
            let d = detect(text);
            assert_eq!(d.lang, Lang::Vi, "{text}");
            assert!(d.unaccented_vi, "{text}");
        }
    }

    #[test]
    fn detects_english() {
        let d = detect("Can you send me the invoice by Friday?");
        assert_eq!(d.lang, Lang::En);
        assert_eq!(d.primary, Lang::En);
    }

    #[test]
    fn detects_mixed_vietnamese_english() {
        let d = detect("Anh check giúp em invoice với booking nhé");
        assert_eq!(d.lang, Lang::Mixed);
        assert_eq!(d.primary, Lang::Vi);
    }

    #[test]
    fn numbers_and_links_alone_are_unknown() {
        assert_eq!(detect("0901234567 https://x.vn").lang, Lang::Unknown);
    }
}
