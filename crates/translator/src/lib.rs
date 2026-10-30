//! Service dịch, chạy trong TEE. Bản rõ chỉ tồn tại trong RAM của tiến trình
//! này: không ghi đĩa, không đưa vào queue, không gửi sang service khác, không log.

pub mod memory;
pub mod risk;
