//! Text input: a one-line field and a multi-line area. Cursor positions count characters.

#[derive(Clone, Debug, Default)]
pub struct TextField {
    chars: Vec<char>,
    pub cursor: usize,
}

impl TextField {
    pub fn new(text: &str) -> TextField {
        let chars: Vec<char> = text.chars().collect();
        TextField {
            cursor: chars.len(),
            chars,
        }
    }
    pub fn value(&self) -> String {
        self.chars.iter().collect()
    }
    pub fn len(&self) -> usize {
        self.chars.len()
    }
    pub fn insert(&mut self, c: char) {
        self.chars.insert(self.cursor, c);
        self.cursor += 1;
    }
    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.chars.remove(self.cursor);
        }
    }
    pub fn delete(&mut self) {
        if self.cursor < self.chars.len() {
            self.chars.remove(self.cursor);
        }
    }
    pub fn left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }
    pub fn right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.chars.len());
    }
    pub fn home(&mut self) {
        self.cursor = 0;
    }
    pub fn end(&mut self) {
        self.cursor = self.chars.len();
    }
}

#[derive(Clone, Debug)]
pub struct TextArea {
    lines: Vec<Vec<char>>,
    pub row: usize,
    pub col: usize,
}

impl TextArea {
    pub fn new(text: &str) -> TextArea {
        let mut a = TextArea {
            lines: Vec::new(),
            row: 0,
            col: 0,
        };
        a.set(text);
        a
    }
    pub fn set(&mut self, text: &str) {
        self.lines = text.split('\n').map(|l| l.chars().collect()).collect();
        if self.lines.is_empty() {
            self.lines.push(Vec::new());
        }
        self.row = self.row.min(self.lines.len() - 1);
        self.col = self.col.min(self.lines[self.row].len());
    }
    pub fn value(&self) -> String {
        self.lines
            .iter()
            .map(|l| l.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }
    pub fn lines(&self) -> Vec<String> {
        self.lines.iter().map(|l| l.iter().collect()).collect()
    }
    pub fn insert(&mut self, c: char) {
        self.lines[self.row].insert(self.col, c);
        self.col += 1;
    }
    pub fn newline(&mut self) {
        let rest = self.lines[self.row].split_off(self.col);
        self.row += 1;
        self.lines.insert(self.row, rest);
        self.col = 0;
    }
    pub fn backspace(&mut self) {
        if self.col > 0 {
            self.col -= 1;
            self.lines[self.row].remove(self.col);
        } else if self.row > 0 {
            let line = self.lines.remove(self.row);
            self.row -= 1;
            self.col = self.lines[self.row].len();
            self.lines[self.row].extend(line);
        }
    }
    pub fn delete(&mut self) {
        if self.col < self.lines[self.row].len() {
            self.lines[self.row].remove(self.col);
        } else if self.row + 1 < self.lines.len() {
            let next = self.lines.remove(self.row + 1);
            self.lines[self.row].extend(next);
        }
    }
    pub fn left(&mut self) {
        if self.col > 0 {
            self.col -= 1;
        } else if self.row > 0 {
            self.row -= 1;
            self.col = self.lines[self.row].len();
        }
    }
    pub fn right(&mut self) {
        if self.col < self.lines[self.row].len() {
            self.col += 1;
        } else if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.col = 0;
        }
    }
    pub fn up(&mut self) {
        if self.row > 0 {
            self.row -= 1;
            self.col = self.col.min(self.lines[self.row].len());
        }
    }
    pub fn down(&mut self) {
        if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.col = self.col.min(self.lines[self.row].len());
        }
    }
    pub fn home(&mut self) {
        self.col = 0;
    }
    pub fn end(&mut self) {
        self.col = self.lines[self.row].len();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_edits_at_the_cursor() {
        let mut f = TextField::new("chre");
        f.left();
        f.left();
        f.insert('o');
        assert_eq!(f.value(), "chore");
        f.home();
        f.delete();
        f.end();
        f.backspace();
        assert_eq!(f.value(), "hor");
    }

    #[test]
    fn area_joins_and_splits_lines() {
        let mut a = TextArea::new("ab\ncd");
        a.down();
        a.home();
        a.backspace();
        assert_eq!(a.value(), "abcd");
        a.newline();
        assert_eq!(a.value(), "ab\ncd");
        assert_eq!((a.row, a.col), (1, 0));
        a.up();
        a.end();
        a.delete();
        assert_eq!(a.value(), "abcd");
    }
}
