//! Che thực thể trước khi dịch và kiểm tra bản dịch sau đó.
//!
//! Mọi chữ số trong tin đều bị che bằng placeholder `⟦E1⟧`, nên mô hình không thể
//! đổi giá, số lượng, biển số… Bản dịch được kiểm tra: đủ placeholder, không tự
//! thêm số, tuân thủ glossary. `Issue` chỉ chứa mã lỗi và token/id, không chứa
//! nội dung, nên ghi log được.

mod check;
mod mask;
mod render;

pub use check::{Issue, IssueCode, TermRef, check_entities, check_glossary};
pub use mask::{Entity, Kind, Masked, mask, unmask};
