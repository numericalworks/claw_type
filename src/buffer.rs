//! The editable document: a vector of logical lines plus a cursor, editing
//! primitives, and a small snapshot-based undo history.
//!
//! Column positions are counted in `char`s (not bytes, not display cells), so
//! that editing behaves correctly with non-ASCII text. Mapping a column to a
//! screen position happens later, during wrapping.

use std::path::PathBuf;

const MAX_UNDO: usize = 500;

#[derive(Debug, Clone)]
struct Snapshot {
    lines: Vec<String>,
    line: usize,
    col: usize,
}

/// What pressing Enter at the end of a line should do next.
///
/// Shared by both front-ends so that lists behave identically in the window and
/// in the terminal.
#[derive(Debug, PartialEq, Eq)]
pub enum Continuation {
    /// Just start a fresh empty line.
    None,
    /// Start the next line with this prefix (list / quote continuation).
    Prefix(String),
    /// The line held only a marker, so drop it and start empty.
    Clear,
}

/// An editable text buffer.
#[derive(Debug, Clone)]
pub struct Buffer {
    pub lines: Vec<String>,
    /// Index of the cursor line.
    pub line: usize,
    /// Cursor column, counted in `char`s within `lines[line]`.
    pub col: usize,
    /// Whether the buffer differs from what is on disk.
    pub dirty: bool,
    /// File the buffer is associated with, if any.
    pub path: Option<PathBuf>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    group_open: bool,
}

impl Default for Buffer {
    fn default() -> Self {
        Self::new()
    }
}

impl Buffer {
    /// A buffer holding a single empty line.
    pub fn new() -> Self {
        Self {
            lines: vec![String::new()],
            line: 0,
            col: 0,
            dirty: false,
            path: None,
            undo: Vec::new(),
            redo: Vec::new(),
            group_open: false,
        }
    }

    /// Replace the contents, resetting cursor and history.
    pub fn set_text(&mut self, text: &str) {
        self.lines = text
            .split('\n')
            .map(|l| l.strip_suffix('\r').unwrap_or(l).to_string())
            .collect();
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.line = 0;
        self.col = 0;
        self.undo.clear();
        self.redo.clear();
        self.group_open = false;
    }

    /// The whole document as a string.
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Number of `char`s in the cursor line.
    pub fn cur_len(&self) -> usize {
        self.lines[self.line].chars().count()
    }

    /// Total number of `char`s, counting line breaks.
    pub fn char_count(&self) -> usize {
        let chars: usize = self.lines.iter().map(|l| l.chars().count()).sum();
        chars + self.lines.len().saturating_sub(1)
    }

    pub fn word_count(&self) -> usize {
        self.lines.iter().map(|l| l.split_whitespace().count()).sum()
    }

    // -- history ------------------------------------------------------------

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            lines: self.lines.clone(),
            line: self.line,
            col: self.col,
        }
    }

    /// Open an undo group. Repeated edits without an intervening
    /// [`Buffer::end_group`] collapse into a single undo step, so a burst of
    /// typing can be undone with one keystroke.
    pub fn begin_group(&mut self) {
        if !self.group_open {
            if self.undo.len() >= MAX_UNDO {
                self.undo.remove(0);
            }
            self.undo.push(self.snapshot());
            self.redo.clear();
            self.group_open = true;
        }
    }

    /// Close the current undo group.
    pub fn end_group(&mut self) {
        self.group_open = false;
    }

    pub fn undo(&mut self) -> bool {
        self.end_group();
        match self.undo.pop() {
            Some(snap) => {
                self.redo.push(self.snapshot());
                self.lines = snap.lines;
                self.line = snap.line;
                self.col = snap.col;
                self.dirty = true;
                true
            }
            None => false,
        }
    }

    pub fn redo(&mut self) -> bool {
        self.end_group();
        match self.redo.pop() {
            Some(snap) => {
                self.undo.push(self.snapshot());
                self.lines = snap.lines;
                self.line = snap.line;
                self.col = snap.col;
                self.dirty = true;
                true
            }
            None => false,
        }
    }

    // -- cursor -------------------------------------------------------------

    /// Move the cursor, clamping to the document.
    pub fn set_cursor(&mut self, line: usize, col: usize) {
        self.end_group();
        self.line = line.min(self.lines.len() - 1);
        self.col = col.min(self.cur_len());
    }

    pub fn move_left(&mut self) {
        self.end_group();
        if self.col > 0 {
            self.col -= 1;
        } else if self.line > 0 {
            self.line -= 1;
            self.col = self.cur_len();
        }
    }

    pub fn move_right(&mut self) {
        self.end_group();
        if self.col < self.cur_len() {
            self.col += 1;
        } else if self.line + 1 < self.lines.len() {
            self.line += 1;
            self.col = 0;
        }
    }

    pub fn move_home(&mut self) {
        self.end_group();
        self.col = 0;
    }

    pub fn move_end(&mut self) {
        self.end_group();
        self.col = self.cur_len();
    }

    pub fn move_doc_start(&mut self) {
        self.end_group();
        self.line = 0;
        self.col = 0;
    }

    pub fn move_doc_end(&mut self) {
        self.end_group();
        self.line = self.lines.len() - 1;
        self.col = self.cur_len();
    }

    pub fn move_word_left(&mut self) {
        self.end_group();
        if self.col == 0 {
            if self.line > 0 {
                self.line -= 1;
                self.col = self.cur_len();
            }
            return;
        }
        let chars: Vec<char> = self.lines[self.line].chars().collect();
        let mut i = self.col;
        while i > 0 && chars[i - 1].is_whitespace() {
            i -= 1;
        }
        while i > 0 && !chars[i - 1].is_whitespace() {
            i -= 1;
        }
        self.col = i;
    }

    pub fn move_word_right(&mut self) {
        self.end_group();
        let len = self.cur_len();
        if self.col >= len {
            if self.line + 1 < self.lines.len() {
                self.line += 1;
                self.col = 0;
            }
            return;
        }
        let chars: Vec<char> = self.lines[self.line].chars().collect();
        let mut i = self.col;
        while i < len && !chars[i].is_whitespace() {
            i += 1;
        }
        while i < len && chars[i].is_whitespace() {
            i += 1;
        }
        self.col = i;
    }

    // -- editing ------------------------------------------------------------

    /// Insert a single character at the cursor.
    pub fn insert_char(&mut self, c: char) {
        self.begin_group();
        let byte = byte_of(&self.lines[self.line], self.col);
        self.lines[self.line].insert(byte, c);
        self.col += 1;
        self.dirty = true;
    }

    /// Insert a string, as a single undo step.
    pub fn insert_str(&mut self, s: &str) {
        self.begin_group();
        for c in s.chars() {
            let byte = byte_of(&self.lines[self.line], self.col);
            self.lines[self.line].insert(byte, c);
            self.col += 1;
        }
        self.dirty = true;
    }

    /// Split the current line at the cursor, continuing lists and quotes.
    pub fn newline(&mut self) {
        self.begin_group();
        let line = self.lines[self.line].clone();
        let byte = byte_of(&line, self.col);
        let left = line[..byte].to_string();
        let right = line[byte..].to_string();

        let prefix = match list_continuation(&left) {
            Continuation::Prefix(prefix) => prefix,
            Continuation::None => String::new(),
            Continuation::Clear => {
                // The line held only a marker, so this Enter ends the list:
                // drop the marker and stay on this line, keeping whatever
                // followed the cursor.
                self.lines[self.line] = right;
                self.col = 0;
                self.dirty = true;
                return;
            }
        };

        self.lines[self.line] = left;
        self.lines.insert(self.line + 1, format!("{prefix}{right}"));
        self.line += 1;
        self.col = prefix.chars().count();
        self.dirty = true;
    }

    /// Delete the character before the cursor (joining lines at column 0).
    pub fn backspace(&mut self) {
        if self.col == 0 {
            if self.line == 0 {
                return;
            }
            self.begin_group();
            let prev_len = self.lines[self.line - 1].chars().count();
            let current = self.lines.remove(self.line);
            self.lines[self.line - 1].push_str(&current);
            self.line -= 1;
            self.col = prev_len;
            self.dirty = true;
        } else {
            self.begin_group();
            let end = byte_of(&self.lines[self.line], self.col);
            let start = byte_of(&self.lines[self.line], self.col - 1);
            self.lines[self.line].replace_range(start..end, "");
            self.col -= 1;
            self.dirty = true;
        }
    }

    /// Delete the character under the cursor (joining lines at end of line).
    pub fn delete(&mut self) {
        if self.col >= self.cur_len() {
            if self.line + 1 >= self.lines.len() {
                return;
            }
            self.begin_group();
            let next = self.lines.remove(self.line + 1);
            self.lines[self.line].push_str(&next);
            self.dirty = true;
        } else {
            self.begin_group();
            let start = byte_of(&self.lines[self.line], self.col);
            let end = byte_of(&self.lines[self.line], self.col + 1);
            self.lines[self.line].replace_range(start..end, "");
            self.dirty = true;
        }
    }

    /// Remove up to two leading spaces from the cursor line.
    pub fn outdent(&mut self) {
        let line = self.lines[self.line].clone();
        let n = line
            .chars()
            .take(2)
            .take_while(|c| *c == ' ')
            .count();
        if n == 0 {
            return;
        }
        self.begin_group();
        let byte = byte_of(&line, n);
        self.lines[self.line].replace_range(..byte, "");
        self.col = self.col.saturating_sub(n);
        self.dirty = true;
    }
}

/// Byte offset of the `index`-th `char`, clamped to the end of the string.
fn byte_of(s: &str, index: usize) -> usize {
    s.char_indices()
        .nth(index)
        .map(|(b, _)| b)
        .unwrap_or(s.len())
}

/// Decide how to continue a line when Enter is pressed.
///
/// `line_before_caret` is the text on the current line *up to the caret*, which
/// is what decides whether we are inside a bullet, a numbered item or a quote.
pub fn list_continuation(line_before_caret: &str) -> Continuation {
    let line = line_before_caret;
    let indent_len = line.len() - line.trim_start().len();
    let indent = &line[..indent_len];
    let rest = &line[indent_len..];

    if rest == ">" {
        return Continuation::Clear;
    }
    if let Some(after) = rest.strip_prefix("> ") {
        return if after.trim().is_empty() {
            Continuation::Clear
        } else {
            Continuation::Prefix(format!("{indent}> "))
        };
    }

    for marker in ["-", "*", "+"] {
        if rest == marker {
            return Continuation::Clear;
        }
        let with_space = format!("{marker} ");
        if let Some(after) = rest.strip_prefix(&with_space) {
            return if after.trim().is_empty() {
                Continuation::Clear
            } else {
                Continuation::Prefix(format!("{indent}{marker} "))
            };
        }
    }

    let digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 {
        let number = &rest[..digits];
        let after = &rest[digits..];
        if let Some(delim) = after.chars().next()
            && (delim == '.' || delim == ')')
        {
            let tail = &after[delim.len_utf8()..];
            if let Some(content) = tail.strip_prefix(' ') {
                if content.trim().is_empty() {
                    return Continuation::Clear;
                }
                let next = number
                    .parse::<u64>()
                    .ok()
                    .and_then(|n| n.checked_add(1))
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| number.to_string());
                return Continuation::Prefix(format!("{indent}{next}{delim} "));
            }
        }
    }

    Continuation::None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buf(text: &str) -> Buffer {
        let mut b = Buffer::new();
        b.set_text(text);
        b
    }

    #[test]
    fn round_trips_text() {
        let b = buf("a\nb\n");
        assert_eq!(b.lines, vec!["a", "b", ""]);
        assert_eq!(b.text(), "a\nb\n");
    }

    #[test]
    fn strips_carriage_returns() {
        let b = buf("a\r\nb");
        assert_eq!(b.lines, vec!["a", "b"]);
    }

    #[test]
    fn counts_words_and_chars() {
        let b = buf("hello world\nbye");
        assert_eq!(b.word_count(), 3);
        assert_eq!(b.char_count(), 15);
    }

    #[test]
    fn inserts_and_moves() {
        let mut b = buf("ac");
        b.set_cursor(0, 1);
        b.insert_char('b');
        assert_eq!(b.text(), "abc");
        assert_eq!(b.col, 2);
        // Backspace removes the character before the cursor.
        b.backspace();
        assert_eq!(b.text(), "ac");
        b.move_left();
        assert_eq!(b.col, 0);
    }

    #[test]
    fn handles_multibyte_columns() {
        let mut b = buf("héllo");
        b.set_cursor(0, 2);
        b.backspace();
        assert_eq!(b.text(), "hllo");
    }

    #[test]
    fn newline_splits_at_cursor() {
        let mut b = buf("hello world");
        b.set_cursor(0, 5);
        b.newline();
        assert_eq!(b.lines, vec!["hello", " world"]);
        assert_eq!((b.line, b.col), (1, 0));
    }

    #[test]
    fn newline_continues_bullet_lists() {
        let mut b = buf("- one");
        b.set_cursor(0, 5);
        b.newline();
        assert_eq!(b.lines, vec!["- one", "- "]);
        assert_eq!(b.col, 2);
    }

    #[test]
    fn newline_increments_ordered_lists() {
        let mut b = buf("9. nine");
        b.set_cursor(0, 7);
        b.newline();
        assert_eq!(b.lines[1], "10. ");
    }

    #[test]
    fn newline_on_empty_item_ends_the_list() {
        let mut b = buf("  - ");
        b.set_cursor(0, 4);
        b.newline();
        // The marker is dropped and we stay on the now-empty line.
        assert_eq!(b.lines, vec![""]);
        assert_eq!((b.line, b.col), (0, 0));
    }

    #[test]
    fn newline_continues_quotes() {
        let mut b = buf("> quoted");
        b.set_cursor(0, 8);
        b.newline();
        assert_eq!(b.lines, vec!["> quoted", "> "]);
    }

    #[test]
    fn backspace_joins_lines() {
        let mut b = buf("ab\ncd");
        b.set_cursor(1, 0);
        b.backspace();
        assert_eq!(b.text(), "abcd");
        assert_eq!((b.line, b.col), (0, 2));
    }

    #[test]
    fn delete_joins_lines() {
        let mut b = buf("ab\ncd");
        b.set_cursor(0, 2);
        b.delete();
        assert_eq!(b.text(), "abcd");
    }

    #[test]
    fn word_movement_skips_whitespace() {
        let mut b = buf("foo bar  baz");
        b.set_cursor(0, 0);
        b.move_word_right();
        assert_eq!(b.col, 4);
        b.move_word_right();
        assert_eq!(b.col, 9);
        b.move_word_left();
        assert_eq!(b.col, 4);
    }

    #[test]
    fn outdent_removes_up_to_two_spaces() {
        let mut b = buf("    x");
        b.set_cursor(0, 5);
        b.outdent();
        assert_eq!(b.text(), "  x");
        assert_eq!(b.col, 3);
    }

    #[test]
    fn typing_run_collapses_into_one_undo() {
        let mut b = buf("");
        for c in "hello".chars() {
            b.insert_char(c);
        }
        assert_eq!(b.text(), "hello");
        assert!(b.undo());
        assert_eq!(b.text(), "");
        assert!(b.redo());
        assert_eq!(b.text(), "hello");
    }

    #[test]
    fn navigation_breaks_undo_groups() {
        let mut b = buf("");
        b.insert_char('a');
        b.move_left();
        b.insert_char('b');
        b.undo();
        // Only the 'b' insertion is undone.
        assert_eq!(b.text(), "a");
    }
}
