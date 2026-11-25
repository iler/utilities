//! Turns raw command output into clean lines.
//!
//! Tools draw progress with "go to column 1" and "erase" codes and no line break. These codes
//! count as line breaks here, or all spinner frames join into one very long line. Other escape
//! codes and control characters are removed.

#[derive(Default, Clone, Copy, PartialEq)]
enum Esc {
    #[default]
    None,
    Start,
    Csi,
    Osc,
}

#[derive(Default)]
pub struct LineTail {
    /// The end of a UTF-8 sequence that the last chunk cut.
    pending: Vec<u8>,
    esc: Esc,
    line: String,
    last: String,
    /// Every finished line, when the caller wants them all.
    lines: Option<Vec<String>>,
}

impl LineTail {
    pub fn new() -> LineTail {
        LineTail::default()
    }

    fn collecting() -> LineTail {
        LineTail {
            lines: Some(Vec::new()),
            ..LineTail::default()
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        self.pending.extend_from_slice(bytes);
        let data = std::mem::take(&mut self.pending);
        let mut rest: &[u8] = &data;
        loop {
            match std::str::from_utf8(rest) {
                Ok(s) => {
                    self.chars(s);
                    break;
                }
                Err(e) => {
                    let (good, bad) = rest.split_at(e.valid_up_to());
                    self.chars(std::str::from_utf8(good).unwrap_or_default());
                    match e.error_len() {
                        // Incomplete: keep it for the next chunk.
                        None => {
                            self.pending = bad.to_vec();
                            break;
                        }
                        Some(n) => rest = &bad[n..],
                    }
                }
            }
        }
    }

    fn chars(&mut self, s: &str) {
        for c in s.chars() {
            match self.esc {
                Esc::None => match c {
                    '\x1b' => self.esc = Esc::Start,
                    '\r' | '\n' => self.brk(),
                    '\t' => self.line.push(' '),
                    c if c.is_control() => {}
                    c => self.line.push(c),
                },
                Esc::Start => {
                    self.esc = match c {
                        '[' => Esc::Csi,
                        ']' => Esc::Osc,
                        _ => Esc::None,
                    }
                }
                Esc::Csi => {
                    if ('\x40'..='\x7e').contains(&c) {
                        self.esc = Esc::None;
                        if matches!(c, 'G' | 'J' | 'K') {
                            self.brk();
                        }
                    }
                }
                Esc::Osc => match c {
                    '\x07' => self.esc = Esc::None,
                    '\x1b' => self.esc = Esc::Start,
                    _ => {}
                },
            }
        }
    }

    fn brk(&mut self) {
        let t = self.line.trim();
        if !t.is_empty() {
            self.last = t.to_string();
            if let Some(lines) = &mut self.lines {
                lines.push(self.line.trim_end().to_string());
            }
        }
        self.line.clear();
    }

    /// The line that the tool shows now: the open line, or else the last finished one.
    pub fn current(&self) -> &str {
        let t = self.line.trim();
        if t.is_empty() { &self.last } else { t }
    }
}

/// All non-empty lines of `bytes`, cleaned.
pub fn lines(bytes: &[u8]) -> Vec<String> {
    let mut t = LineTail::collecting();
    t.feed(bytes);
    t.brk();
    t.lines.unwrap_or_default()
}

/// The last non-empty line of `text`.
pub fn last_line(text: &str) -> Option<String> {
    lines(text.as_bytes()).pop().map(|l| l.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spinner_frames_become_lines() {
        let mut t = LineTail::new();
        t.feed(b"Downloading\r\x1b[K\xe2\xa0\x8b working 1\r\x1b[2K\xe2\xa0\x99 working 2");
        assert_eq!(t.current(), "⠙ working 2");
    }

    #[test]
    fn removes_colors_and_keeps_split_utf8() {
        let mut t = LineTail::new();
        t.feed(b"\x1b[32mok\x1b[0m caf\xc3");
        t.feed(b"\xa9\n");
        assert_eq!(t.current(), "ok café");
    }

    #[test]
    fn osc_titles_are_dropped() {
        assert_eq!(
            lines(b"\x1b]0;title\x07hello\nworld"),
            vec!["hello", "world"]
        );
    }

    #[test]
    fn empty_lines_are_skipped() {
        assert_eq!(last_line("a\n\n  \nb\n\n"), Some("b".into()));
        assert_eq!(last_line(""), None);
    }
}
