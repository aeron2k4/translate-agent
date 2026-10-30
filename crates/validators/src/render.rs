use crate::mask::{Entity, Kind, NUM, VI_CURRENCY, VI_SCALE};
use regex::Regex;
use std::sync::LazyLock;

impl Entity {
    /// Dạng hiển thị trong ngôn ngữ đích. Chỉ đổi cách viết (dấu phân cách,
    /// đơn vị tiền, tên tháng), không bao giờ đổi giá trị.
    pub fn render(&self, target: &str) -> String {
        if target == self.source_lang {
            return self.source.clone();
        }
        let from = self.source_lang.as_str();
        let out = match self.kind {
            Kind::Money => render_money(&self.source, from, target),
            Kind::Date => render_date(&self.source, from, target),
            Kind::Number => render_number(&self.source, from, target),
            _ => None,
        };
        out.unwrap_or_else(|| self.source.clone())
    }
}

/// Tách phần nguyên / thập phân theo quy ước ngôn ngữ nguồn:
/// tiếng Việt "1.200.000,5", tiếng Anh "1,200,000.5".
fn parse_num(s: &str, lang: &str) -> Option<(String, String)> {
    let (thousands, decimal) = if lang == "en" { (',', '.') } else { ('.', ',') };
    if !s
        .chars()
        .all(|c| c.is_ascii_digit() || c == '.' || c == ',')
    {
        return None;
    }
    let seps: Vec<char> = s.chars().filter(|c| !c.is_ascii_digit()).collect();
    let groups: Vec<&str> = s.split(['.', ',']).collect();
    if groups.iter().any(|g| g.is_empty()) {
        return None;
    }
    let all_three = |gs: &[&str]| gs.iter().all(|g| g.len() == 3);
    let n = seps.len();
    if n == 0 {
        Some((s.to_owned(), String::new()))
    } else if seps.iter().all(|&c| c == thousands) && all_three(&groups[1..]) {
        Some((groups.concat(), String::new()))
    } else if n == 1 {
        Some((groups[0].to_owned(), groups[1].to_owned()))
    } else if seps[n - 1] == decimal
        && seps[..n - 1].iter().all(|&c| c == thousands)
        && all_three(&groups[1..n])
    {
        Some((groups[..n].concat(), groups[n].to_owned()))
    } else {
        None
    }
}

fn format_num(int_part: &str, frac: &str, lang: &str) -> String {
    let (thousands, decimal) = if lang == "en" { (',', '.') } else { ('.', ',') };
    let digits = int_part.trim_start_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(thousands);
        }
        out.push(c);
    }
    if !frac.is_empty() {
        out.push(decimal);
        out.push_str(frac);
    }
    out
}

fn render_number(src: &str, from: &str, to: &str) -> Option<String> {
    if !src.contains(['.', ',']) {
        return Some(src.to_owned()); // năm, số đếm: giữ nguyên, không tự chèn dấu phân cách
    }
    let (i, f) = parse_num(src, from)?;
    Some(format_num(&i, &f, to))
}

fn to_f64(s: &str, lang: &str) -> Option<f64> {
    let (i, f) = parse_num(s, lang)?;
    let f = if f.is_empty() { "0".to_owned() } else { f };
    format!("{i}.{f}").parse().ok()
}

static MONEY_VI: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"(?i)^({NUM})(?:\s?[-–]\s?({NUM}))?\s?(?:({VI_SCALE})([0-9]{{1,3}})?(?:\s?(?:{VI_CURRENCY}))?|(?:{VI_CURRENCY}))$"
    ))
    .expect("regex tiền Việt")
});

static MONEY_EN_VND: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"(?i)^({NUM})\s?(thousand|million|billion)?\s?(?:vnd|dong)$"
    ))
    .expect("regex VND tiếng Anh")
});

fn vi_scale(word: &str) -> f64 {
    match word.to_lowercase().as_str() {
        "tỷ" | "tỉ" | "tỏi" => 1e9,
        "triệu" | "tr" | "củ" => 1e6,
        "nghìn" | "ngàn" | "k" => 1e3,
        _ => 1.0,
    }
}

fn render_money(src: &str, from: &str, to: &str) -> Option<String> {
    match to {
        "en" => {
            let c = MONEY_VI.captures(src)?;
            let scale = c.get(3).map_or(1.0, |m| vi_scale(m.as_str()));
            let mut lo = to_f64(&c[1], from)?;
            let hi = match c.get(2) {
                Some(m) => to_f64(m.as_str(), from)?,
                None => {
                    if let Some(d) = c.get(4) {
                        // "1tr2" = 1,2 triệu
                        lo += format!("0.{}", d.as_str()).parse::<f64>().ok()?;
                    }
                    lo
                }
            };
            Some(format_vnd_english(lo * scale, hi * scale))
        }
        "vi" => {
            // $, USD: giữ nguyên
            let c = MONEY_EN_VND.captures(src)?;
            let v = to_f64(&c[1], from)?;
            let word = match c.get(2).map(|m| m.as_str().to_lowercase()).as_deref() {
                Some("thousand") => " nghìn",
                Some("million") => " triệu",
                Some("billion") => " tỷ",
                _ => "",
            };
            Some(format!("{}{word} đồng", trim_float(v, "vi")))
        }
        _ => None,
    }
}

fn trim_float(v: f64, lang: &str) -> String {
    let s = format!("{}", (v * 1000.0).round() / 1000.0);
    if lang == "vi" {
        s.replacen('.', ",", 1)
    } else {
        s
    }
}

fn format_vnd_english(lo: f64, hi: f64) -> String {
    let (unit, div) = if hi >= 1e9 {
        (" billion", 1e9)
    } else if hi >= 1e6 {
        (" million", 1e6)
    } else {
        ("", 1.0)
    };
    let part = |v: f64| {
        if div == 1.0 {
            format_num(&format!("{}", v.round() as i64), "", "en")
        } else {
            trim_float(v / div, "en")
        }
    };
    if lo == hi {
        format!("{}{unit} VND", part(lo))
    } else {
        format!("{}–{}{unit} VND", part(lo), part(hi))
    }
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

fn render_date(src: &str, from: &str, to: &str) -> Option<String> {
    let parts: Vec<&str> = src.split('/').collect();
    let a: usize = parts.first()?.parse().ok()?;
    let b: usize = parts.get(1)?.parse().ok()?;
    // Tiếng Việt: ngày/tháng. Tiếng Anh: chỉ đổi khi không thể hiểu khác.
    let (day, month) = if from == "en" {
        match (a, b) {
            _ if a == b => (a, b),
            _ if a > 12 && b <= 12 => (a, b),
            _ if b > 12 && a <= 12 => (b, a),
            _ => return None, // cả hai cách đều hợp lý: giữ nguyên, để bước tối nghĩa xử lý
        }
    } else {
        (a, b)
    };
    if !(1..=31).contains(&day) || !(1..=12).contains(&month) {
        return None;
    }
    let year = parts.get(2);
    match to {
        "en" => Some(match year {
            Some(y) => format!("{day} {} {y}", MONTHS[month - 1]),
            None => format!("{day} {}", MONTHS[month - 1]),
        }),
        "vi" => Some(match year {
            Some(y) => format!("{day}/{month}/{y}"),
            None => format!("{day}/{month}"),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ent(kind: Kind, src: &str, lang: &str) -> Entity {
        Entity {
            token: "⟦E1⟧".into(),
            kind,
            source: src.into(),
            source_lang: lang.into(),
        }
    }

    #[test]
    fn money_vi_to_en() {
        for (src, want) in [
            ("5 củ", "5 million VND"),
            ("2 tỏi", "2 billion VND"),
            ("1tr2", "1.2 million VND"),
            ("500k", "500,000 VND"),
            ("5-7 triệu", "5–7 million VND"),
            ("1,5 tỷ", "1.5 billion VND"),
            ("1.200.000đ", "1.2 million VND"),
            ("850.000 đồng", "850,000 VND"),
        ] {
            assert_eq!(ent(Kind::Money, src, "vi").render("en"), want, "{src}");
        }
    }

    #[test]
    fn money_en_to_vi() {
        assert_eq!(
            ent(Kind::Money, "5 million VND", "en").render("vi"),
            "5 triệu đồng"
        );
        assert_eq!(ent(Kind::Money, "$1,200", "en").render("vi"), "$1,200");
    }

    #[test]
    fn numbers_follow_target_convention() {
        assert_eq!(
            ent(Kind::Number, "1.200.000", "vi").render("en"),
            "1,200,000"
        );
        assert_eq!(ent(Kind::Number, "1,5", "vi").render("en"), "1.5");
        assert_eq!(ent(Kind::Number, "2026", "vi").render("en"), "2026");
        assert_eq!(ent(Kind::Number, "1,200.50", "en").render("vi"), "1.200,50");
        assert_eq!(ent(Kind::Number, "500kg", "vi").render("en"), "500kg");
    }

    #[test]
    fn dates() {
        assert_eq!(ent(Kind::Date, "15/11", "vi").render("en"), "15 Nov");
        assert_eq!(
            ent(Kind::Date, "2/12/2026", "vi").render("en"),
            "2 Dec 2026"
        );
        assert_eq!(ent(Kind::Date, "11/15", "en").render("vi"), "15/11");
        // 3/4 tiếng Anh: tháng 3 hay tháng 4 đều hợp lý → giữ nguyên.
        assert_eq!(ent(Kind::Date, "3/4", "en").render("vi"), "3/4");
    }

    #[test]
    fn same_language_is_untouched() {
        assert_eq!(ent(Kind::Money, "5 củ", "vi").render("vi"), "5 củ");
    }
}
