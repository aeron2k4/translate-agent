//! Bộ nhớ ngắn hạn theo luồng (bước 2: ghép ngữ cảnh).
//!
//! - Chỉ trong RAM. Mỗi tin hết hạn sau TTL (tối đa 24 giờ).
//! - Xoá toàn bộ luồng khi phiên đóng hoặc có thành viên rời nhóm.
//! - Ngữ cảnh đưa ra dùng nhãn giả danh (P1, P2…) và đã che giá trị.

use prompts::ContextLine;
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const MAX_TTL: Duration = Duration::from_secs(24 * 60 * 60);
pub const MIN_WINDOW: usize = 6;
pub const MAX_WINDOW: usize = 10;

struct Stored {
    sender: String,
    lang: String,
    text: String,
    at: Instant,
}

#[derive(Default)]
struct Thread {
    messages: VecDeque<Stored>,
    summary: Option<(String, Instant)>,
    address_forms: HashMap<String, String>,
}

/// Ngữ cảnh đã sẵn sàng đưa vào prompt.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub lines: Vec<ContextLine>,
    pub summary: Option<String>,
    pub address_forms: Vec<(String, String)>,
    /// Nhãn giả danh của người gửi tin đang dịch.
    pub sender_label: String,
}

fn pseudonym(labels: &mut HashMap<String, String>, id: &str) -> String {
    let n = labels.len() + 1;
    labels
        .entry(id.to_owned())
        .or_insert_with(|| format!("P{n}"))
        .clone()
}

pub struct Memory {
    ttl: Duration,
    window: usize,
    threads: Mutex<HashMap<String, Thread>>,
}

impl Memory {
    /// `ttl` bị chặn ở 24 giờ, `window` ở khoảng 6–10 tin.
    pub fn new(ttl: Duration, window: usize) -> Self {
        Self {
            ttl: ttl.min(MAX_TTL),
            window: window.clamp(MIN_WINDOW, MAX_WINDOW),
            threads: Mutex::default(),
        }
    }

    pub fn ttl(&self) -> Duration {
        self.ttl
    }

    fn with_thread<R>(&self, thread: &str, f: impl FnOnce(&mut Thread) -> R) -> Option<R> {
        let mut threads = self.threads.lock().ok()?;
        Some(f(threads.entry(thread.to_owned()).or_default()))
    }

    /// Ghi nhận một tin (sau khi đã dịch) vào ngữ cảnh của luồng.
    pub fn record(&self, thread: &str, sender: &str, lang: &str, text: &str) {
        let window = self.window;
        self.with_thread(thread, |t| {
            t.messages.push_back(Stored {
                sender: sender.to_owned(),
                lang: lang.to_owned(),
                text: text.to_owned(),
                at: Instant::now(),
            });
            while t.messages.len() > window {
                t.messages.pop_front();
            }
        });
    }

    pub fn set_summary(&self, thread: &str, summary: &str) {
        self.with_thread(thread, |t| {
            t.summary = Some((summary.to_owned(), Instant::now()));
        });
    }

    /// Cách xưng hô với một thành viên ("anh", "chị", "em"…).
    pub fn set_address_form(&self, thread: &str, member: &str, form: &str) {
        self.with_thread(thread, |t| {
            t.address_forms.insert(member.to_owned(), form.to_owned());
        });
    }

    pub fn snapshot(&self, thread: &str, sender: &str) -> Snapshot {
        let ttl = self.ttl;
        let Ok(mut threads) = self.threads.lock() else {
            return Snapshot::default();
        };
        let Some(t) = threads.get_mut(thread) else {
            return Snapshot {
                sender_label: "P1".into(),
                ..Snapshot::default()
            };
        };
        t.messages.retain(|m| m.at.elapsed() <= ttl);
        if t.summary.as_ref().is_some_and(|(_, at)| at.elapsed() > ttl) {
            t.summary = None;
        }
        // Đánh nhãn theo thứ tự xuất hiện; người gửi hiện tại luôn có nhãn.
        let mut labels: HashMap<String, String> = HashMap::new();
        let lines = t
            .messages
            .iter()
            .map(|m| ContextLine {
                speaker: pseudonym(&mut labels, &m.sender),
                text: prompts::redact(&m.text, &m.lang),
            })
            .collect();
        let sender_label = pseudonym(&mut labels, sender);
        let mut address_forms: Vec<(String, String)> = t
            .address_forms
            .iter()
            .filter_map(|(member, form)| labels.get(member).map(|l| (l.clone(), form.clone())))
            .collect();
        address_forms.sort();
        Snapshot {
            lines,
            summary: t.summary.as_ref().map(|(s, _)| s.clone()),
            address_forms,
            sender_label,
        }
    }

    /// Phiên đóng: xoá toàn bộ ngữ cảnh của luồng.
    pub fn close_session(&self, thread: &str) {
        if let Ok(mut threads) = self.threads.lock() {
            threads.remove(thread);
        }
    }

    /// Thành viên rời nhóm: xoá toàn bộ ngữ cảnh của luồng (tin của họ có thể
    /// nằm trong tóm tắt, nên không chỉ xoá riêng tin của họ).
    pub fn member_left(&self, thread: &str, _member: &str) {
        self.close_session(thread);
    }

    /// Dọn tin hết hạn ở mọi luồng; trả về số luồng bị xoá hẳn.
    pub fn sweep(&self) -> usize {
        let ttl = self.ttl;
        let Ok(mut threads) = self.threads.lock() else {
            return 0;
        };
        let before = threads.len();
        threads.retain(|_, t| {
            t.messages.retain(|m| m.at.elapsed() <= ttl);
            if t.summary.as_ref().is_some_and(|(_, at)| at.elapsed() > ttl) {
                t.summary = None;
            }
            !t.messages.is_empty() || t.summary.is_some()
        });
        before - threads.len()
    }

    pub fn thread_count(&self) -> usize {
        self.threads.lock().map_or(0, |t| t.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_ttl_and_window() {
        let m = Memory::new(Duration::from_secs(48 * 3600), 50);
        assert_eq!(m.ttl(), MAX_TTL);
        assert_eq!(m.window, MAX_WINDOW);
        assert_eq!(Memory::new(MAX_TTL, 1).window, MIN_WINDOW);
    }

    #[test]
    fn keeps_last_window_with_pseudonyms_and_redaction() {
        let m = Memory::new(MAX_TTL, 6);
        for i in 0..8 {
            let sender = if i % 2 == 0 { "user-a" } else { "user-b" };
            m.record("t1", sender, "vi", &format!("tin số {i}, gọi 0901234567"));
        }
        m.set_address_form("t1", "user-a", "anh");
        let s = m.snapshot("t1", "user-c");
        assert_eq!(s.lines.len(), 6);
        assert_eq!(s.lines[0].speaker, "P1");
        assert!(s.lines.iter().all(|l| !l.text.contains("0901234567")));
        assert!(s.lines.iter().all(|l| !l.speaker.contains("user")));
        assert_eq!(s.sender_label, "P3");
        assert_eq!(s.address_forms, [("P1".to_owned(), "anh".to_owned())]);
    }

    #[test]
    fn close_and_member_left_wipe_thread() {
        let m = Memory::new(MAX_TTL, 6);
        m.record("t1", "a", "vi", "x");
        m.record("t2", "a", "vi", "y");
        m.close_session("t1");
        m.member_left("t2", "a");
        assert_eq!(m.thread_count(), 0);
    }

    #[test]
    fn expired_messages_are_dropped() {
        let m = Memory::new(Duration::ZERO, 6);
        m.record("t1", "a", "vi", "x");
        m.set_summary("t1", "s");
        std::thread::sleep(Duration::from_millis(2));
        assert!(m.snapshot("t1", "a").lines.is_empty());
        assert_eq!(m.sweep(), 1);
        assert_eq!(m.thread_count(), 0);
    }
}
