//! Glyphs and the colors of the text output (the TUI uses ratatui styles).

use crate::cli::ColorWhen;
use std::io::IsTerminal;
use std::sync::OnceLock;

pub struct Glyphs {
    pub spin: &'static [&'static str],
    pub ok: &'static str,
    pub fail: &'static str,
    pub skip: &'static str,
    pub wait: &'static str,
    pub miss: &'static str,
    pub arrow: &'static str,
    pub up: &'static str,
    pub add: &'static str,
    pub del: &'static str,
    pub on: &'static str,
    pub off: &'static str,
    pub some: &'static str,
}

const UTF8: Glyphs = Glyphs {
    spin: &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"],
    ok: "✔",
    fail: "✖",
    skip: "⊘",
    wait: "·",
    miss: "—",
    arrow: "→",
    up: "↑",
    add: "+",
    del: "−",
    on: "●",
    off: "○",
    some: "◐",
};

const ASCII: Glyphs = Glyphs {
    spin: &["|", "/", "-", "\\"],
    ok: "+",
    fail: "x",
    skip: "s",
    wait: ".",
    miss: "-",
    arrow: "->",
    up: "^",
    add: "+",
    del: "-",
    on: "*",
    off: "o",
    some: "~",
};

/// UTF-8 glyphs when the locale says UTF-8, plain ASCII otherwise.
pub fn glyphs() -> &'static Glyphs {
    static G: OnceLock<bool> = OnceLock::new();
    let utf8 = *G.get_or_init(|| {
        let loc = ["LC_ALL", "LC_CTYPE", "LANG"]
            .iter()
            .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
            .unwrap_or_default()
            .to_lowercase();
        loc.contains("utf-8") || loc.contains("utf8")
    });
    if utf8 { &UTF8 } else { &ASCII }
}

/// ANSI colors for the text output. All methods return the text unchanged when color is off.
#[derive(Clone, Copy)]
pub struct Paint {
    pub on: bool,
}

impl Paint {
    pub fn new(when: ColorWhen) -> Paint {
        let on = match when {
            ColorWhen::Always => true,
            ColorWhen::Never => false,
            ColorWhen::Auto => {
                std::io::stdout().is_terminal()
                    && std::env::var("TERM").map(|t| t != "dumb").unwrap_or(false)
                    && std::env::var_os("NO_COLOR").is_none()
            }
        };
        Paint { on }
    }
    fn wrap(&self, code: &str, s: &str) -> String {
        if self.on {
            format!("\x1b[{code}m{s}\x1b[0m")
        } else {
            s.to_string()
        }
    }
    pub fn bold(&self, s: &str) -> String {
        self.wrap("1", s)
    }
    pub fn dim(&self, s: &str) -> String {
        self.wrap("2", s)
    }
    pub fn green(&self, s: &str) -> String {
        self.wrap("32", s)
    }
    pub fn red(&self, s: &str) -> String {
        self.wrap("31", s)
    }
    pub fn yellow(&self, s: &str) -> String {
        self.wrap("33", s)
    }
    pub fn cyan(&self, s: &str) -> String {
        self.wrap("36", s)
    }
    pub fn grey(&self, s: &str) -> String {
        self.wrap("90", s)
    }
}

/// `mm:ss`.
pub fn duration(d: std::time::Duration) -> String {
    let s = d.as_secs();
    format!("{:02}:{:02}", s / 60, s % 60)
}

/// "5s ago", "12m ago", "3h ago", "2d ago".
pub fn age(secs: u64) -> String {
    match secs {
        0..60 => format!("{secs}s ago"),
        60..3600 => format!("{}m ago", secs / 60),
        3600..86400 => format!("{}h ago", secs / 3600),
        _ => format!("{}d ago", secs / 86400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_times() {
        assert_eq!(duration(std::time::Duration::from_secs(84)), "01:24");
        assert_eq!(age(30), "30s ago");
        assert_eq!(age(7200), "2h ago");
    }

    #[test]
    fn paint_off_returns_plain_text() {
        assert_eq!(Paint { on: false }.red("x"), "x");
        assert_eq!(Paint { on: true }.red("x"), "\x1b[31mx\x1b[0m");
    }
}
