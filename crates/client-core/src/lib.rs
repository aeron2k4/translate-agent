//! Lõi phía client (biên dịch được sang mobile/WASM): lọc nhanh, nhận diện ngôn
//! ngữ, chuỗi i18n. Ngân sách: lọc + nhận diện ≤ 10 ms mỗi tin.

pub mod filter;
pub mod i18n;
pub mod langdetect;

pub use filter::{Decision, SkipReason, should_translate};
pub use langdetect::{Detection, Lang, detect};
