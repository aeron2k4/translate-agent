use crate::mask::Masked;
use regex::Regex;
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueCode {
    MissingEntity,
    DuplicateEntity,
    UnknownEntity,
    StrayNumber,
    GlossaryMiss,
}

impl IssueCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MissingEntity => "missing_entity",
            Self::DuplicateEntity => "duplicate_entity",
            Self::UnknownEntity => "unknown_entity",
            Self::StrayNumber => "stray_number",
            Self::GlossaryMiss => "glossary_miss",
        }
    }
}

/// Lỗi kiểm tra. `reference` là token placeholder hoặc id thuật ngữ —
/// không bao giờ là nội dung tin, để ghi log an toàn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Issue {
    pub code: IssueCode,
    pub reference: String,
}

/// Cặp thuật ngữ bắt buộc phải tuân thủ.
#[derive(Debug, Clone, Copy)]
pub struct TermRef<'a> {
    pub id: &'a str,
    pub source: &'a str,
    pub target: &'a str,
}

static ANY_PLACEHOLDER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("⟦[^⟧]*⟧").expect("regex placeholder"));

/// Bản dịch (còn placeholder) phải giữ mỗi thực thể đúng một lần và không tự
/// thêm chữ số nào ngoài placeholder.
pub fn check_entities(output: &str, masked: &Masked) -> Vec<Issue> {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for m in ANY_PLACEHOLDER.find_iter(output) {
        *counts.entry(m.as_str()).or_default() += 1;
    }
    let mut issues = Vec::new();
    for e in &masked.entities {
        let code = match counts.remove(e.token.as_str()) {
            None => Some(IssueCode::MissingEntity),
            Some(n) if n > 1 => Some(IssueCode::DuplicateEntity),
            _ => None,
        };
        if let Some(code) = code {
            issues.push(Issue {
                code,
                reference: e.token.clone(),
            });
        }
    }
    issues.extend(counts.into_keys().map(|tok| Issue {
        code: IssueCode::UnknownEntity,
        reference: tok.to_owned(),
    }));
    if ANY_PLACEHOLDER
        .replace_all(output, "")
        .chars()
        .any(|c| c.is_numeric())
    {
        issues.push(Issue {
            code: IssueCode::StrayNumber,
            reference: String::new(),
        });
    }
    issues
}

/// Thuật ngữ nào xuất hiện trong bản gốc (so khớp không phân biệt dấu) thì bản
/// dịch phải dùng đúng bản dịch đã duyệt.
pub fn check_glossary(source: &str, output: &str, terms: &[TermRef<'_>]) -> Vec<Issue> {
    terms
        .iter()
        .filter(|t| {
            textnorm::contains_phrase(source, t.source, true)
                && !textnorm::contains_phrase(output, t.target, false)
        })
        .map(|t| Issue {
            code: IssueCode::GlossaryMiss,
            reference: t.id.to_owned(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mask::mask;

    fn codes(issues: &[Issue]) -> Vec<IssueCode> {
        issues.iter().map(|i| i.code).collect()
    }

    #[test]
    fn entity_checks() {
        let m = mask("Cọc 2 tỏi trước 15/11", "vi");
        assert!(check_entities("Deposit ⟦E1⟧ before ⟦E2⟧", &m).is_empty());
        assert_eq!(
            codes(&check_entities("Deposit ⟦E1⟧", &m)),
            [IssueCode::MissingEntity]
        );
        assert_eq!(
            codes(&check_entities("⟦E1⟧ ⟦E1⟧ ⟦E2⟧ ⟦E9⟧", &m)),
            [IssueCode::DuplicateEntity, IssueCode::UnknownEntity]
        );
        assert_eq!(
            codes(&check_entities("Deposit ⟦E1⟧ before ⟦E2⟧, 3 days", &m)),
            [IssueCode::StrayNumber]
        );
    }

    #[test]
    fn glossary_check_matches_unaccented_source() {
        let terms = [TermRef {
            id: "auto-001",
            source: "lái thử",
            target: "test drive",
        }];
        assert!(check_glossary("a muon lai thu xe", "I want a test drive", &terms).is_empty());
        let issues = check_glossary("a muon lai thu xe", "I want to try driving", &terms);
        assert_eq!(issues[0].reference, "auto-001");
        assert!(check_glossary("xe đẹp", "nice car", &terms).is_empty());
    }
}
