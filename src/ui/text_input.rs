use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// A text buffer with a movable cursor, shared by every TUI prompt.
///
/// The cursor is a *character* offset kept inside `0..=len`, so editing
/// multi-byte input can never slice a UTF-8 codepoint in half. Display columns
/// are a rendering concern and are measured by the caller — a full-width glyph
/// is one cursor step but two columns wide.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct TextInput {
    text: String,
    cursor: usize,
}

impl TextInput {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// Cursor position as a character offset into [`Self::text`].
    pub(crate) fn cursor(&self) -> usize {
        self.cursor
    }

    /// Length in characters, which is the cursor's upper bound.
    pub(crate) fn len(&self) -> usize {
        self.text.chars().count()
    }

    /// Apply `key` as an editing keystroke, reporting whether it was consumed.
    ///
    /// Callers match their own bindings (`Enter`, `Esc`, `Ctrl-S`) first and
    /// hand everything else here, so the editing keys behave identically in
    /// every prompt.
    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> bool {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            // A control chord is never literal text: an unbound one is ignored
            // rather than inserting the bare letter it was typed with.
            match key.code {
                KeyCode::Char('a') => self.move_home(),
                KeyCode::Char('e') => self.move_end(),
                KeyCode::Char('w') => self.delete_word_before(),
                KeyCode::Char('u') => self.delete_to_start(),
                _ => return false,
            }
            return true;
        }
        match key.code {
            KeyCode::Char(ch) => self.insert(ch),
            KeyCode::Left => self.move_left(),
            KeyCode::Right => self.move_right(),
            KeyCode::Home => self.move_home(),
            KeyCode::End => self.move_end(),
            KeyCode::Backspace => self.delete_before(),
            KeyCode::Delete => self.delete_at(),
            _ => return false,
        }
        true
    }

    pub(crate) fn insert(&mut self, ch: char) {
        let at = self.byte_offset(self.cursor);
        self.text.insert(at, ch);
        self.cursor += 1;
    }

    fn move_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    fn move_right(&mut self) {
        self.cursor = self.cursor.saturating_add(1).min(self.len());
    }

    fn move_home(&mut self) {
        self.cursor = 0;
    }

    fn move_end(&mut self) {
        self.cursor = self.len();
    }

    /// `Backspace`: remove the character before the cursor. A no-op at offset 0.
    fn delete_before(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let start = self.byte_offset(self.cursor - 1);
        let end = self.byte_offset(self.cursor);
        self.text.replace_range(start..end, "");
        self.cursor -= 1;
    }

    /// `Delete`: remove the character under the cursor. A no-op at the end.
    fn delete_at(&mut self) {
        let start = self.byte_offset(self.cursor);
        let end = self.byte_offset(self.cursor + 1);
        if start == end {
            return;
        }
        self.text.replace_range(start..end, "");
    }

    /// `Ctrl-W`: drop the whitespace run left of the cursor and the word before
    /// it, so repeating it walks back word by word instead of stalling on the
    /// separator.
    fn delete_word_before(&mut self) {
        let chars: Vec<char> = self.text.chars().collect();
        let mut start = self.cursor;
        while start > 0 && chars[start - 1].is_whitespace() {
            start -= 1;
        }
        while start > 0 && !chars[start - 1].is_whitespace() {
            start -= 1;
        }
        let from = self.byte_offset(start);
        let to = self.byte_offset(self.cursor);
        self.text.replace_range(from..to, "");
        self.cursor = start;
    }

    /// `Ctrl-U`: drop everything left of the cursor, keeping the tail.
    fn delete_to_start(&mut self) {
        let end = self.byte_offset(self.cursor);
        self.text.replace_range(..end, "");
        self.cursor = 0;
    }

    /// Byte index of the `chars`-th character, or the end of the string when
    /// `chars` is at or past the last one.
    fn byte_offset(&self, chars: usize) -> usize {
        self.text
            .char_indices()
            .nth(chars)
            .map_or(self.text.len(), |(index, _)| index)
    }
}

impl From<String> for TextInput {
    fn from(text: String) -> Self {
        let cursor = text.chars().count();
        Self { text, cursor }
    }
}

impl From<&str> for TextInput {
    fn from(text: &str) -> Self {
        Self::from(text.to_string())
    }
}

impl PartialEq<str> for TextInput {
    fn eq(&self, other: &str) -> bool {
        self.text == other
    }
}

#[cfg(test)]
#[path = "text_input_tests.rs"]
mod tests;
