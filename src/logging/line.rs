const TRUNCATED_MARKER: &[u8] = b"[truncated]";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLineAssembler {
    max_line_bytes: usize,
    pending: Vec<u8>,
    truncated: bool,
}

impl LogLineAssembler {
    pub fn new(max_line_bytes: usize) -> Self {
        Self {
            max_line_bytes,
            pending: Vec::new(),
            truncated: false,
        }
    }

    pub fn push_bytes(&mut self, bytes: &[u8]) -> Vec<String> {
        let mut lines = Vec::new();
        for byte in bytes {
            if *byte == b'\n' {
                lines.push(self.finish_line());
            } else {
                self.push_line_byte(*byte);
            }
        }
        lines
    }

    pub fn finish(&mut self) -> Option<String> {
        if self.pending.is_empty() && !self.truncated {
            None
        } else {
            Some(self.finish_line())
        }
    }

    fn push_line_byte(&mut self, byte: u8) {
        if self.max_line_bytes == 0 {
            self.truncated = true;
            return;
        }
        if self.pending.len() < self.max_line_bytes && !self.truncated {
            self.pending.push(byte);
            return;
        }
        self.mark_truncated();
    }

    fn mark_truncated(&mut self) {
        if self.truncated {
            return;
        }
        self.truncated = true;
        let marker_len = TRUNCATED_MARKER.len().min(self.max_line_bytes);
        let keep_len = self.max_line_bytes.saturating_sub(marker_len);
        self.pending.truncate(keep_len);
        self.pending
            .extend_from_slice(&TRUNCATED_MARKER[..marker_len]);
    }

    fn finish_line(&mut self) -> String {
        let line = String::from_utf8_lossy(&self.pending).into_owned();
        self.pending.clear();
        self.truncated = false;
        line
    }
}

#[cfg(test)]
mod tests {
    use super::LogLineAssembler;

    #[test]
    fn emits_complete_lines_and_keeps_fragment() {
        let mut lines = LogLineAssembler::new(100);

        assert_eq!(lines.push_bytes(b"one\ntwo\nthr"), vec!["one", "two"]);
        assert_eq!(lines.push_bytes(b"ee\n"), vec!["three"]);
        assert_eq!(lines.finish(), None);
    }

    #[test]
    fn flushes_partial_line_on_finish() {
        let mut lines = LogLineAssembler::new(100);

        assert!(lines.push_bytes(b"partial").is_empty());
        assert_eq!(lines.finish(), Some("partial".to_string()));
        assert_eq!(lines.finish(), None);
    }

    #[test]
    fn truncates_overlong_lines_with_marker() {
        let mut lines = LogLineAssembler::new(16);

        assert_eq!(
            lines.push_bytes(b"0123456789abcdefghijklmnopqrstuvwxyz\n"),
            vec!["01234[truncated]"],
        );
    }
}
