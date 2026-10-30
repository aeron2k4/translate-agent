//! Mở che placeholder ngay trong lúc stream, không để lộ "⟦E1⟧" ra client.

use validators::{Entity, unmask};

pub(crate) struct StreamUnmask<'a> {
    entities: &'a [Entity],
    target: &'a str,
    buf: String,
}

impl<'a> StreamUnmask<'a> {
    pub(crate) fn new(entities: &'a [Entity], target: &'a str) -> Self {
        Self {
            entities,
            target,
            buf: String::new(),
        }
    }

    /// Nhận một đoạn từ mô hình, trả phần đã an toàn để hiển thị. Placeholder
    /// chưa đóng được giữ lại chờ đoạn sau.
    pub(crate) fn push(&mut self, delta: &str) -> String {
        self.buf.push_str(delta);
        let cut = match self.buf.rfind('⟦') {
            Some(i) if !self.buf[i..].contains('⟧') => i,
            _ => self.buf.len(),
        };
        let ready: String = self.buf.drain(..cut).collect();
        unmask(&ready, self.entities, self.target)
    }

    pub(crate) fn finish(&mut self) -> String {
        let rest = std::mem::take(&mut self.buf);
        unmask(&rest, self.entities, self.target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holds_partial_placeholders() {
        let m = validators::mask("cọc 5 củ", "vi");
        let mut s = StreamUnmask::new(&m.entities, "en");
        assert_eq!(s.push("Deposit ⟦"), "Deposit ");
        assert_eq!(s.push("E1"), "");
        assert_eq!(s.push("⟧ now"), "5 million VND now");
        assert_eq!(s.finish(), "");
    }
}
