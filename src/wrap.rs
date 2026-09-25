//! Soft-wrapping of styled character sequences into screen rows.
//!
//! The editor stores text as logical lines, but renders a narrow centred
//! column, so logical lines usually need to be split across several screen
//! rows. This module performs that split while keeping track of *which*
//! character each row starts at, which is what lets the cursor be positioned
//! correctly on a wrapped line.

use crate::markdown::MStyle;
use unicode_width::UnicodeWidthChar;

/// A single screen row produced by wrapping.
#[derive(Debug, Clone)]
pub struct VisualRow {
    /// Index of the logical line this row came from.
    pub line: usize,
    /// Char index (within the logical line) of this row's first character.
    pub start: usize,
    /// The characters to draw, each with its style.
    pub chars: Vec<(char, MStyle)>,
}

/// Result of wrapping a whole document.
#[derive(Debug, Clone, Default)]
pub struct Wrapped {
    pub rows: Vec<VisualRow>,
    /// For each logical line, `(first_row, row_count)`.
    pub line_span: Vec<(usize, usize)>,
}

/// Display width of a character in terminal cells.
pub fn cell_width(c: char) -> usize {
    // Tabs are rendered as a single space by the callers, so treat them as one
    // cell. Zero-width characters (combining marks) contribute nothing.
    if c == '\t' {
        1
    } else {
        UnicodeWidthChar::width(c).unwrap_or(0)
    }
}

/// Wrap a document (a slice of styled logical lines) into screen rows.
pub fn wrap_document(lines: &[Vec<(char, MStyle)>], width: usize) -> Wrapped {
    let width = width.max(1);
    let mut rows = Vec::new();
    let mut line_span = Vec::with_capacity(lines.len());

    for (index, line) in lines.iter().enumerate() {
        let first = rows.len();
        for (start, chars) in wrap_line(line, width) {
            rows.push(VisualRow {
                line: index,
                start,
                chars,
            });
        }
        line_span.push((first, rows.len() - first));
    }

    Wrapped { rows, line_span }
}

/// Wrap one styled line, returning `(start_char_index, chars)` per row.
///
/// Breaks prefer whitespace but fall back to a hard break when a single word
/// does not fit.
fn wrap_line(chars: &[(char, MStyle)], width: usize) -> Vec<(usize, Vec<(char, MStyle)>)> {
    let n = chars.len();
    if n == 0 {
        return vec![(0, Vec::new())];
    }

    let mut rows = Vec::new();
    let mut start = 0;

    while start < n {
        let mut cells = 0usize;
        let mut end = start;
        let mut last_break: Option<usize> = None;

        while end < n {
            let w = cell_width(chars[end].0);
            if cells + w > width && end > start {
                break;
            }
            if chars[end].0.is_whitespace() {
                last_break = Some(end + 1);
            }
            cells += w;
            end += 1;
        }

        let break_at = if end >= n {
            n
        } else if let Some(b) = last_break {
            b
        } else {
            end
        };
        // Guarantee forward progress.
        let break_at = if break_at <= start { start + 1 } else { break_at };

        rows.push((start, chars[start..break_at].to_vec()));
        start = break_at;
    }

    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(s: &str) -> Vec<(char, MStyle)> {
        s.chars().map(|c| (c, MStyle::default())).collect()
    }

    fn text_of(chars: &[(char, MStyle)]) -> String {
        chars.iter().map(|(c, _)| *c).collect()
    }

    #[test]
    fn empty_line_yields_one_row() {
        let w = wrap_document(&[Vec::new()], 10);
        assert_eq!(w.rows.len(), 1);
        assert_eq!(w.line_span, vec![(0, 1)]);
    }

    #[test]
    fn short_line_is_not_split() {
        let w = wrap_document(&[plain("hello")], 10);
        assert_eq!(w.rows.len(), 1);
        assert_eq!(text_of(&w.rows[0].chars), "hello");
    }

    #[test]
    fn breaks_on_whitespace() {
        let w = wrap_document(&[plain("hello world")], 8);
        assert_eq!(w.rows.len(), 2);
        assert_eq!(text_of(&w.rows[0].chars), "hello ");
        assert_eq!(text_of(&w.rows[1].chars), "world");
        assert_eq!(w.rows[1].start, 6);
    }

    #[test]
    fn hard_breaks_long_words() {
        let w = wrap_document(&[plain("abcdefghij")], 4);
        assert_eq!(w.rows.len(), 3);
        assert_eq!(text_of(&w.rows[0].chars), "abcd");
        assert_eq!(text_of(&w.rows[2].chars), "ij");
    }

    #[test]
    fn tracks_offsets_across_multiple_lines() {
        let w = wrap_document(&[plain("one two"), plain("three")], 4);
        assert_eq!(w.line_span, vec![(0, 2), (2, 2)]);
        assert_eq!(w.rows[2].line, 1);
        assert_eq!(w.rows[2].start, 0);
        assert_eq!(w.rows[3].start, 4);
    }

    #[test]
    fn wide_characters_use_two_cells() {
        // Three CJK characters at width 4 fit two per row.
        let w = wrap_document(&[plain("日本語")], 4);
        assert_eq!(w.rows.len(), 2);
        assert_eq!(text_of(&w.rows[0].chars), "日本");
        assert_eq!(text_of(&w.rows[1].chars), "語");
    }

    #[test]
    fn always_makes_progress_when_a_wide_char_overflows() {
        let w = wrap_document(&[plain("日")], 1);
        assert_eq!(w.rows.len(), 1);
        assert_eq!(text_of(&w.rows[0].chars), "日");
    }
}
