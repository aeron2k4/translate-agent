//! Service dịch, chạy trong TEE. Bản rõ chỉ tồn tại trong RAM của tiến trình
//! này: không ghi đĩa, không đưa vào queue, không gửi sang service khác, không log.

pub mod address;
pub mod hitl;
pub mod memory;
pub mod mock;
pub mod pipeline;
pub mod risk;
mod stream;

pub use pipeline::{Request, Response, Secrets, Status, StreamEvent, Translation, Translator};
