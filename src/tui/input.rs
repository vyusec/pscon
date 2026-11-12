use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use unicode_width::UnicodeWidthChar;

/// A single-line editor. Cursor offsets are UTF-8 boundaries, never terminal columns.
#[derive(Clone, Debug, Default)]
pub(super) struct Input {
    pub text: String,
    pub cursor: usize,
}
impl Input {
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        Self {
            cursor: text.len(),
            text,
        }
    }
    pub fn insert(&mut self, text: &str) {
        let clean: String = text.chars().filter(|c| !c.is_control()).collect();
        self.text.insert_str(self.cursor, &clean);
        self.cursor += clean.len();
    }
    pub fn key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.text.len(),
            KeyCode::Left => {
                self.cursor = self.text[..self.cursor]
                    .char_indices()
                    .last()
                    .map_or(0, |(i, _)| i)
            }
            KeyCode::Right => {
                if let Some(c) = self.text[self.cursor..].chars().next() {
                    self.cursor += c.len_utf8();
                }
            }
            KeyCode::Backspace => {
                if self.cursor > 0 {
                    let prev = self.text[..self.cursor].char_indices().last().unwrap().0;
                    self.text.drain(prev..self.cursor);
                    self.cursor = prev;
                }
            }
            KeyCode::Delete => {
                if let Some(c) = self.text[self.cursor..].chars().next() {
                    self.text.drain(self.cursor..self.cursor + c.len_utf8());
                }
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.text.clear();
                self.cursor = 0;
            }
            KeyCode::Char(c)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.insert(&c.to_string())
            }
            _ => {}
        }
    }
    /// A horizontally scrolled slice and cursor column, with room for the caret.
    pub fn visible(&self, width: u16) -> (String, u16) {
        if width == 0 {
            return (String::new(), 0);
        }
        let budget = usize::from(width - 1);
        let mut start = self.cursor;
        let mut col = 0;
        for (i, c) in self.text[..self.cursor].char_indices().rev() {
            let w = c.width().unwrap_or(0);
            if col + w > budget {
                break;
            }
            col += w;
            start = i;
        }
        let mut shown = String::new();
        let mut used = 0;
        for c in self.text[start..].chars() {
            let w = c.width().unwrap_or(0);
            if used + w > usize::from(width) {
                break;
            }
            shown.push(c);
            used += w;
        }
        (shown, col as u16)
    }
}
