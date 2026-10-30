use regex::Regex;
use serde::Serialize;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Chuỗi trông giống placeholder có sẵn trong tin.
    Literal,
    Url,
    Email,
    Mention,
    Phone,
    Cccd,
    BankAccount,
    Plate,
    Money,
    Date,
    /// Giờ kiểu Việt: "9h30", "14h".
    Time,
    ProductCode,
    Number,
}

/// Một đoạn trong tin được thay bằng `token` trước khi gửi mô hình.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entity {
    pub token: String,
    pub kind: Kind,
    pub source: String,
    pub source_lang: String,
}

impl Entity {
    /// Dữ liệu cá nhân: không được cache, không được lưu.
    pub fn is_pii(&self) -> bool {
        matches!(
            self.kind,
            Kind::Email
                | Kind::Phone
                | Kind::Cccd
                | Kind::BankAccount
                | Kind::Plate
                | Kind::Mention
        )
    }
}

/// Tin đã che thực thể.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Masked {
    pub text: String,
    pub entities: Vec<Entity>,
}

impl Masked {
    pub fn has_pii(&self) -> bool {
        self.entities.iter().any(Entity::is_pii)
    }
}

pub(crate) const NUM: &str = r"[0-9]+(?:[.,][0-9]+)*";
// Có cả dạng không dấu: "cu" = củ (triệu), "toi" = tỏi (tỷ). Thứ tự quan trọng:
// dạng dài đứng trước dạng ngắn ("triệu" trước "tr", "dong" trước "d").
pub(crate) const VI_SCALE: &str =
    "tỷ|tỉ|tỏi|triệu|trieu|tr|củ|nghìn|ngàn|nghin|ngan|ty|ti|toi|cu|k";
pub(crate) const VI_CURRENCY: &str = "đồng|dong|vnđ|vnd|đ|d";

struct Rule {
    kind: Kind,
    re: Regex,
}

/// Thứ tự là độ ưu tiên: luật đứng trước giành đoạn văn bản trước.
/// Mỗi biểu thức bắt thực thể ở nhóm 1, theo sau là ranh giới (không phải chữ/số).
static RULES: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    let money_vi = format!(
        r"(?i:{NUM}(?:\s?[-–]\s?{NUM})?\s?(?:(?:{VI_SCALE})(?:[0-9]{{1,3}})?(?:\s?(?:{VI_CURRENCY}))?|(?:{VI_CURRENCY})))"
    );
    let money_en = format!(
        r"(?i:\$\s?{NUM}(?:\s?(?:k|m|million|billion))?|{NUM}\s?(?:usd|dollars?)|{NUM}\s?(?:thousand|million|billion)?\s?(?:vnd|dong))"
    );
    let defs: Vec<(Kind, String)> = vec![
        (Kind::Literal, "⟦[^⟧]*⟧|[⟦⟧]".into()),
        (Kind::Url, r"(?i:https?://|www\.)\S+".into()),
        (
            Kind::Email,
            r"[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}".into(),
        ),
        (Kind::Mention, r"@[\p{L}\p{N}_.]*[\p{L}\p{N}_]".into()),
        (
            Kind::Phone,
            r"(?:\+84|0)[35789][0-9](?:[ .]?[0-9]){7}".into(),
        ),
        (Kind::Cccd, "[0-9]{12}".into()),
        (Kind::BankAccount, "[0-9]{8,19}".into()),
        (
            Kind::Plate,
            r"[0-9]{2}[A-Z]{1,2}[0-9]?-[0-9]{3}\.[0-9]{2}|[0-9]{2}[A-Z]{1,2}[0-9]?-[0-9]{4,5}"
                .into(),
        ),
        (Kind::Money, money_vi),
        (Kind::Money, money_en),
        (
            Kind::Date,
            "[0-9]{1,2}/[0-9]{1,2}(?:/[0-9]{4}|/[0-9]{2})?".into(),
        ),
        (Kind::Time, "[0-9]{1,2}[hg](?:[0-5][0-9])?".into()),
        (Kind::ProductCode, "[A-Z]{1,4}-?[0-9]{1,4}[A-Z]{0,2}".into()),
        (
            Kind::Number,
            r"\p{L}*[0-9][\p{L}\p{N}]*(?:[.,][0-9]+)*[\p{L}%]*".into(),
        ),
    ];
    defs.into_iter()
        .map(|(kind, body)| Rule {
            kind,
            re: Regex::new(&format!(r"({body})(?:[^\p{{L}}\p{{N}}]|$)"))
                .expect("luật che thực thể phải biên dịch được"),
        })
        .collect()
});

pub(crate) static PLACEHOLDER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("⟦E[0-9]+⟧").expect("regex placeholder"));

fn is_word(c: char) -> bool {
    c.is_alphanumeric()
}

/// "2h nữa", "mất 2h", "trong 2h": khoảng thời gian, không phải giờ trên đồng hồ.
fn is_duration(text: &str, s: usize, e: usize) -> bool {
    let before = textnorm::words(&text[..s])
        .pop()
        .map(|w| textnorm::fold(&w));
    let after = textnorm::words(&text[e..])
        .into_iter()
        .next()
        .map(|w| textnorm::fold(&w));
    matches!(before.as_deref(), Some("trong" | "mat"))
        || matches!(after.as_deref(), Some("nua" | "tieng"))
}

/// Che mọi thực thể trong `text`. `source_lang` ("vi"/"en") quyết định cách đọc
/// dấu phân cách số và ngày khi render sang ngôn ngữ đích.
pub fn mask(text: &str, source_lang: &str) -> Masked {
    let mut spans: Vec<(usize, usize, Kind)> = Vec::new();
    for rule in RULES.iter() {
        let mut pos = 0;
        while pos < text.len() {
            let Some(m) = rule.re.captures_at(text, pos).and_then(|c| c.get(1)) else {
                break;
            };
            let (s, mut e) = (m.start(), m.end());
            if rule.kind == Kind::Url {
                e = s + text[s..e]
                    .trim_end_matches(['.', ',', ';', ':', '!', '?', ')', '"', '\''])
                    .len();
            }
            let lead_ok = rule.kind == Kind::Literal
                || text[..s].chars().next_back().is_none_or(|c| !is_word(c));
            let free = spans.iter().all(|&(a, b, _)| e <= a || b <= s);
            if lead_ok && e > s && free {
                let kind = if rule.kind == Kind::Time && is_duration(text, s, e) {
                    Kind::Number
                } else {
                    rule.kind
                };
                spans.push((s, e, kind));
                pos = e;
            } else {
                pos = s + text[s..].chars().next().map_or(1, char::len_utf8);
            }
        }
    }
    spans.sort_by_key(|&(s, _, _)| s);

    let mut out = String::with_capacity(text.len());
    let mut entities = Vec::with_capacity(spans.len());
    let mut last = 0;
    for (i, &(s, e, kind)) in spans.iter().enumerate() {
        let token = format!("⟦E{}⟧", i + 1);
        out.push_str(&text[last..s]);
        out.push_str(&token);
        entities.push(Entity {
            token,
            kind,
            source: text[s..e].to_owned(),
            source_lang: source_lang.to_owned(),
        });
        last = e;
    }
    out.push_str(&text[last..]);
    Masked {
        text: out,
        entities,
    }
}

/// Thay placeholder trong bản dịch bằng thực thể đã render cho ngôn ngữ đích.
pub fn unmask(translated: &str, entities: &[Entity], target_lang: &str) -> String {
    PLACEHOLDER
        .replace_all(translated, |c: &regex::Captures<'_>| {
            let tok = &c[0];
            entities
                .iter()
                .find(|e| e.token == tok)
                .map_or_else(|| tok.to_owned(), |e| e.render(target_lang))
        })
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(m: &Masked) -> Vec<(Kind, &str)> {
        m.entities
            .iter()
            .map(|e| (e.kind, e.source.as_str()))
            .collect()
    }

    #[test]
    fn masks_slang_money_and_phone() {
        let m = mask("Giá lăn bánh 1tr2 nhé, gọi 0901234567", "vi");
        assert_eq!(m.text, "Giá lăn bánh ⟦E1⟧ nhé, gọi ⟦E2⟧");
        assert_eq!(
            kinds(&m),
            [(Kind::Money, "1tr2"), (Kind::Phone, "0901234567")]
        );
        assert!(m.has_pii());
    }

    #[test]
    fn masks_each_kind() {
        let cases: &[(&str, Kind, &str)] = &[
            ("xe 5 củ thôi", Kind::Money, "5 củ"),
            ("cọc 2 tỏi", Kind::Money, "2 tỏi"),
            ("tầm 5-7 triệu đồng", Kind::Money, "5-7 triệu đồng"),
            ("phí 1.200.000đ", Kind::Money, "1.200.000đ"),
            ("price $1,200 ok", Kind::Money, "$1,200"),
            ("hẹn 15/11 nhé", Kind::Date, "15/11"),
            ("coc 5 cu nhe", Kind::Money, "5 cu"),
            ("tam 2 toi", Kind::Money, "2 toi"),
            ("lan banh 30 trieu", Kind::Money, "30 trieu"),
            ("phi 850.000d nhe", Kind::Money, "850.000d"),
            ("gia 1,5 ty", Kind::Money, "1,5 ty"),
            ("hẹn 9h30 sáng", Kind::Time, "9h30"),
            ("mở đến 14h", Kind::Time, "14h"),
            ("2h nữa giao", Kind::Number, "2h"),
            ("mất 3h thôi", Kind::Number, "3h"),
            ("biển 51A-123.45 ạ", Kind::Plate, "51A-123.45"),
            ("biển 30E-12345 ạ", Kind::Plate, "30E-12345"),
            ("CCCD 001203004567", Kind::Cccd, "001203004567"),
            (
                "STK 19036612345016 nhé",
                Kind::BankAccount,
                "19036612345016",
            ),
            ("gọi +84901234567", Kind::Phone, "+84901234567"),
            ("hỏi @Lan.Nguyen nhé", Kind::Mention, "@Lan.Nguyen"),
            ("mail a.b@x-group.vn", Kind::Email, "a.b@x-group.vn"),
            (
                "xem https://xe.x.vn/dat-lich.",
                Kind::Url,
                "https://xe.x.vn/dat-lich",
            ),
            ("bản VF8 Plus", Kind::ProductCode, "VF8"),
            ("nặng 500kg", Kind::Number, "500kg"),
            ("đời 2026", Kind::Number, "2026"),
            ("text ⟦E1⟧ giả", Kind::Literal, "⟦E1⟧"),
        ];
        for (input, kind, src) in cases {
            let m = mask(input, "vi");
            assert_eq!(kinds(&m), [(*kind, *src)], "input {input:?}");
        }
    }

    #[test]
    fn no_digit_survives_masking() {
        for input in [
            "a ơi xe e 5 củ ko đc à, 3 ngày nữa giao",
            "Model3 abc123 x2 4G 150kW 2.0L 99%",
            "dat lich 9h30 ngay 2/12 tai Q7",
            "anh book 2 xe VF 8 cho 15 nguoi",
            "Sđt 0987.654.321 nhé, STK 0123456789",
        ] {
            let m = mask(input, "vi");
            let rest = PLACEHOLDER.replace_all(&m.text, "");
            assert!(
                !rest.chars().any(|c| c.is_ascii_digit()),
                "{input:?} → {}",
                m.text
            );
        }
    }

    #[test]
    fn unmask_renders_for_target() {
        let m = mask("Cọc 2 tỏi trước 15/11", "vi");
        let out = unmask("Deposit ⟦E1⟧ before ⟦E2⟧", &m.entities, "en");
        assert_eq!(out, "Deposit 2 billion VND before 15 Nov");
        assert_eq!(unmask(&m.text, &m.entities, "vi"), "Cọc 2 tỏi trước 15/11");
    }
}
