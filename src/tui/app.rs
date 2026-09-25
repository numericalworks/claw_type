//! Application state and the mapping from key presses to actions.

use std::fs;
use std::io;
use std::path::PathBuf;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::buffer::Buffer;
use crate::markdown::{self, Mode};
use crate::palette::Theme;
use crate::wrap::{self, Wrapped};

/// Widest the writing column is ever allowed to grow.
pub const MAX_CONTENT_WIDTH: usize = 84;
/// Horizontal breathing room on either side of the column.
pub const SIDE_MARGIN: usize = 4;
/// Blank rows above the writing column.
pub const TOP_MARGIN: usize = 1;

/// Which representation of the document is on screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    /// Raw Markdown, editable, with syntax colour.
    Write,
    /// Rendered Markdown, read-only.
    Preview,
}

/// Why a text prompt is open.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PromptKind {
    Open,
    SaveAs,
}

/// A modal panel drawn on top of the editor.
#[derive(Clone, Debug)]
pub enum Overlay {
    None,
    Help,
    Prompt {
        kind: PromptKind,
        input: String,
        cursor: usize,
    },
    ConfirmQuit,
}

/// The whole editor.
pub struct App {
    pub buf: Buffer,
    pub theme: Theme,
    pub view: View,
    pub overlay: Overlay,
    /// Dim everything but the paragraph under the cursor.
    pub focus: bool,
    /// Keep the cursor line vertically centred.
    pub typewriter: bool,
    /// Show the status bar.
    pub show_bar: bool,
    pub should_quit: bool,
    /// Transient message shown in the status bar.
    pub status: Option<String>,

    // Geometry, refreshed every frame by [`App::relayout`].
    pub viewport: (u16, u16),
    pub content_width: usize,
    pub content_height: usize,
    pub scroll: usize,
    pub layout: Wrapped,
    pub cursor_row: usize,
    pub cursor_char: usize,
    /// First and last logical line of the paragraph holding the cursor.
    pub focus_para: (usize, usize),

    pending_quit: bool,
}

impl App {
    /// Create the editor, optionally loading a file.
    pub fn new(path: Option<PathBuf>) -> Self {
        let mut buf = Buffer::new();
        let mut status = None;

        if let Some(p) = path {
            match fs::read_to_string(&p) {
                Ok(text) => {
                    buf.set_text(&text);
                    buf.path = Some(p.clone());
                    status = Some(format!("Opened {}", p.display()));
                }
                Err(e) => {
                    // Remember the path so that saving creates the file.
                    buf.path = Some(p.clone());
                    status = Some(format!("Could not open {}: {e}", p.display()));
                }
            }
        }

        let mut app = Self {
            buf,
            theme: Theme::default(),
            view: View::Write,
            overlay: Overlay::None,
            focus: false,
            typewriter: false,
            show_bar: true,
            should_quit: false,
            status,
            viewport: (80, 24),
            content_width: MAX_CONTENT_WIDTH,
            content_height: 20,
            scroll: 0,
            layout: Wrapped::default(),
            cursor_row: 0,
            cursor_char: 0,
            focus_para: (0, 0),
            pending_quit: false,
        };
        app.relayout();
        app
    }

    /// Recompute wrapping, cursor position, the focus paragraph and scroll.
    ///
    /// Cheap enough to call on every frame; documents edited in a terminal are
    /// small enough that re-laying out from scratch is not a bottleneck.
    pub fn relayout(&mut self) {
        let (tw, th) = self.viewport;
        let tw = tw as usize;
        let th = th as usize;

        self.content_width = tw
            .saturating_sub(2 * SIDE_MARGIN)
            .clamp(1, MAX_CONTENT_WIDTH);
        let bar = usize::from(self.show_bar);
        self.content_height = th.saturating_sub(TOP_MARGIN + bar).max(1);

        let mode = match self.view {
            View::Write => Mode::Highlight,
            View::Preview => Mode::Render,
        };
        let styled = markdown::transform(&self.buf.lines, mode, self.content_width);
        self.layout = wrap::wrap_document(&styled, self.content_width);

        self.focus_para = paragraph_bounds(&self.buf.lines, self.buf.line);

        // Locate the cursor within the wrapped rows.
        let line = self.buf.line;
        let (first, count) = self.layout.line_span[line];
        if self.view == View::Preview {
            // The preview transforms the text, so the cursor simply tracks the
            // first row of its logical line.
            self.cursor_row = first;
            self.cursor_char = 0;
        } else {
            let rows = &self.layout.rows[first..first + count];
            let mut idx = count - 1;
            for (i, row) in rows.iter().enumerate() {
                if self.buf.col < row.start + row.chars.len() {
                    idx = i;
                    break;
                }
            }
            let row = &rows[idx];
            self.cursor_row = first + idx;
            self.cursor_char = (self.buf.col - row.start).min(row.chars.len());
        }

        // Scroll so that the cursor is visible (or centred, in typewriter mode).
        let height = self.content_height;
        if self.typewriter {
            self.scroll = self.cursor_row.saturating_sub(height / 2);
        } else if self.cursor_row < self.scroll {
            self.scroll = self.cursor_row;
        } else if self.cursor_row >= self.scroll + height {
            self.scroll = self.cursor_row + 1 - height;
        }
        let max_scroll = self.layout.rows.len().saturating_sub(height);
        self.scroll = self.scroll.min(max_scroll);
    }

    /// Dispatch a key event.
    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.kind == KeyEventKind::Release {
            return;
        }
        if matches!(&self.overlay, Overlay::Help) {
            self.handle_help_key(key);
        } else if matches!(&self.overlay, Overlay::ConfirmQuit) {
            self.handle_confirm_key(key);
        } else if matches!(&self.overlay, Overlay::Prompt { .. }) {
            self.handle_prompt_key(key);
        } else {
            self.handle_editor_key(key);
        }
    }

    // -- overlays -----------------------------------------------------------

    fn handle_help_key(&mut self, _key: KeyEvent) {
        // The panel promises "press any key to close", so that is exactly what
        // happens; it also makes Ctrl+H a toggle.
        self.overlay = Overlay::None;
    }

    fn handle_confirm_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('s') => {
                if self.buf.path.is_some() {
                    self.save();
                    self.should_quit = true;
                } else {
                    self.pending_quit = true;
                    self.open_save_as_prompt();
                }
            }
            KeyCode::Char('d') => self.should_quit = true,
            KeyCode::Char('c') | KeyCode::Esc => {
                self.overlay = Overlay::None;
                self.pending_quit = false;
            }
            _ => {}
        }
    }

    fn handle_prompt_key(&mut self, key: KeyEvent) {
        let (kind, mut input, mut cursor) = match &self.overlay {
            Overlay::Prompt { kind, input, cursor } => (*kind, input.clone(), *cursor),
            _ => return,
        };
        let mut chars: Vec<char> = input.chars().collect();

        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => {
                self.overlay = Overlay::None;
                self.pending_quit = false;
                return;
            }
            KeyCode::Enter => {
                self.submit_prompt(kind, input);
                return;
            }
            KeyCode::Char('u') if ctrl => {
                chars.clear();
                cursor = 0;
            }
            KeyCode::Char(c) if !ctrl => {
                let at = cursor.min(chars.len());
                chars.insert(at, c);
                cursor = at + 1;
            }
            KeyCode::Backspace => {
                if cursor > 0 {
                    chars.remove(cursor - 1);
                    cursor -= 1;
                }
            }
            KeyCode::Delete => {
                if cursor < chars.len() {
                    chars.remove(cursor);
                }
            }
            KeyCode::Left => cursor = cursor.saturating_sub(1),
            KeyCode::Right => cursor = (cursor + 1).min(chars.len()),
            KeyCode::Home => cursor = 0,
            KeyCode::End => cursor = chars.len(),
            _ => {}
        }

        input = chars.into_iter().collect();
        self.overlay = Overlay::Prompt {
            kind,
            input,
            cursor,
        };
    }

    fn submit_prompt(&mut self, kind: PromptKind, input: String) {
        self.overlay = Overlay::None;
        let raw = input.trim().to_string();
        if raw.is_empty() {
            self.pending_quit = false;
            return;
        }
        let path = expand_path(&raw);
        match kind {
            PromptKind::Open => self.open_path(path),
            PromptKind::SaveAs => {
                if self.save_to(path).is_ok() && self.pending_quit {
                    self.should_quit = true;
                }
            }
        }
        self.pending_quit = false;
    }

    // -- editor keys --------------------------------------------------------

    fn handle_editor_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);

        if ctrl {
            match key.code {
                KeyCode::Char('s') => return self.save(),
                KeyCode::Char('o') => return self.open_prompt(),
                KeyCode::Char('n') => return self.new_file(),
                KeyCode::Char('q') => return self.request_quit(),
                KeyCode::Char('p') => return self.toggle_preview(),
                KeyCode::Char('f') => {
                    self.focus = !self.focus;
                    return;
                }
                KeyCode::Char('t') => {
                    self.typewriter = !self.typewriter;
                    return;
                }
                KeyCode::Char('b') => {
                    self.show_bar = !self.show_bar;
                    return;
                }
                KeyCode::Char('h') => {
                    self.overlay = Overlay::Help;
                    return;
                }
                KeyCode::Char('z') | KeyCode::Char('Z') => {
                    if shift {
                        self.buf.redo();
                    } else {
                        self.buf.undo();
                    }
                    return;
                }
                KeyCode::Char('y') => {
                    self.buf.redo();
                    return;
                }
                KeyCode::Left => return self.buf.move_word_left(),
                KeyCode::Right => return self.buf.move_word_right(),
                KeyCode::Up => return self.move_vertical(-1),
                KeyCode::Down => return self.move_vertical(1),
                KeyCode::Home => return self.buf.move_doc_start(),
                KeyCode::End => return self.buf.move_doc_end(),
                _ => {}
            }
        }

        // Typing in the preview returns you to the editable view.
        let is_edit = matches!(
            key.code,
            KeyCode::Char(_)
                | KeyCode::Backspace
                | KeyCode::Delete
                | KeyCode::Enter
                | KeyCode::Tab
                | KeyCode::BackTab
        );
        if self.view == View::Preview && is_edit {
            self.view = View::Write;
        }

        match key.code {
            KeyCode::Char(c) => {
                if !key.modifiers.contains(KeyModifiers::ALT) {
                    self.buf.insert_char(c);
                }
            }
            KeyCode::Tab => self.buf.insert_str("  "),
            KeyCode::BackTab => self.buf.outdent(),
            KeyCode::Enter => self.buf.newline(),
            KeyCode::Backspace => self.buf.backspace(),
            KeyCode::Delete => self.buf.delete(),
            KeyCode::Left => self.buf.move_left(),
            KeyCode::Right => self.buf.move_right(),
            KeyCode::Up => self.move_vertical(-1),
            KeyCode::Down => self.move_vertical(1),
            KeyCode::Home => self.buf.move_home(),
            KeyCode::End => self.buf.move_end(),
            KeyCode::PageUp => {
                for _ in 0..10 {
                    self.move_vertical(-1);
                }
            }
            KeyCode::PageDown => {
                for _ in 0..10 {
                    self.move_vertical(1);
                }
            }
            KeyCode::Esc => self.status = None,
            KeyCode::F(1) => self.overlay = Overlay::Help,
            KeyCode::F(2) => self.show_bar = !self.show_bar,
            _ => {}
        }
    }

    /// Move the cursor one screen row up or down, preserving the visual column.
    fn move_vertical(&mut self, delta: i32) {
        self.relayout();
        if self.layout.rows.is_empty() {
            return;
        }
        let current = &self.layout.rows[self.cursor_row];
        let offset = self.buf.col.saturating_sub(current.start);
        let target = self.cursor_row as i64 + delta as i64;

        if target < 0 {
            self.buf.set_cursor(0, 0);
            return;
        }
        if target as usize >= self.layout.rows.len() {
            let last = self.buf.lines.len() - 1;
            let end = self.buf.lines[last].chars().count();
            self.buf.set_cursor(last, end);
            return;
        }

        let row = &self.layout.rows[target as usize];
        let col = row.start + offset.min(row.chars.len());
        let line = row.line;
        self.buf.set_cursor(line, col);
    }

    // -- commands -----------------------------------------------------------

    fn toggle_preview(&mut self) {
        self.view = match self.view {
            View::Write => View::Preview,
            View::Preview => View::Write,
        };
        // Avoid a stale scroll position when the layout changes shape.
        self.scroll = 0;
    }

    fn new_file(&mut self) {
        if self.buf.dirty {
            self.status = Some("Unsaved changes — Ctrl+S to save first".into());
            return;
        }
        self.buf = Buffer::new();
        self.scroll = 0;
        self.status = Some("New file".into());
    }

    fn request_quit(&mut self) {
        if self.buf.dirty {
            self.overlay = Overlay::ConfirmQuit;
        } else {
            self.should_quit = true;
        }
    }

    fn open_prompt(&mut self) {
        let seed = self
            .buf
            .path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let cursor = seed.chars().count();
        self.overlay = Overlay::Prompt {
            kind: PromptKind::Open,
            input: seed,
            cursor,
        };
    }

    fn open_save_as_prompt(&mut self) {
        let seed = self
            .buf
            .path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "untitled.md".to_string());
        let cursor = seed.chars().count();
        self.overlay = Overlay::Prompt {
            kind: PromptKind::SaveAs,
            input: seed,
            cursor,
        };
    }

    /// Save to the current path, prompting for one if there is none.
    fn save(&mut self) {
        match self.buf.path.clone() {
            Some(path) => {
                let _ = self.save_to(path);
            }
            None => self.open_save_as_prompt(),
        }
    }

    fn save_to(&mut self, path: PathBuf) -> io::Result<()> {
        match fs::write(&path, self.buf.text()) {
            Ok(()) => {
                self.buf.path = Some(path.clone());
                self.buf.dirty = false;
                self.status = Some(format!("Saved {}", path.display()));
                Ok(())
            }
            Err(e) => {
                self.status = Some(format!("Save failed: {e}"));
                Err(e)
            }
        }
    }

    fn open_path(&mut self, path: PathBuf) {
        match fs::read_to_string(&path) {
            Ok(text) => {
                self.buf.set_text(&text);
                self.buf.path = Some(path.clone());
                self.scroll = 0;
                self.status = Some(format!("Opened {}", path.display()));
            }
            Err(e) => self.status = Some(format!("Could not open {}: {e}", path.display())),
        }
    }
}

/// The paragraph (block of non-blank lines) containing `line`.
fn paragraph_bounds(lines: &[String], line: usize) -> (usize, usize) {
    if lines.is_empty() {
        return (0, 0);
    }
    let blank = |i: usize| lines[i].trim().is_empty();
    if blank(line) {
        return (line, line);
    }
    let mut start = line;
    let mut end = line;
    while start > 0 && !blank(start - 1) {
        start -= 1;
    }
    while end + 1 < lines.len() && !blank(end + 1) {
        end += 1;
    }
    (start, end)
}

/// Expand a leading `~` to the user's home directory.
fn expand_path(input: &str) -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        if input == "~" {
            return PathBuf::from(home);
        }
        if let Some(rest) = input.strip_prefix("~/") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(text: &str) -> App {
        let mut a = App::new(None);
        a.buf.set_text(text);
        // 18 columns leaves a 10-cell writing column (2 x 4 side margins).
        a.viewport = (18, 24);
        a.relayout();
        a
    }

    #[test]
    fn paragraph_extends_over_adjacent_lines() {
        let lines: Vec<String> = vec!["a", "b", "", "c", "d", "e"]
            .into_iter()
            .map(String::from)
            .collect();
        assert_eq!(paragraph_bounds(&lines, 1), (0, 1));
        assert_eq!(paragraph_bounds(&lines, 4), (3, 5));
        assert_eq!(paragraph_bounds(&lines, 2), (2, 2));
    }

    #[test]
    fn cursor_lands_on_the_wrapped_row() {
        let mut a = app("aaaaaaaaaabbbbb");
        assert_eq!(a.content_width, 10);
        a.buf.set_cursor(0, 12);
        a.relayout();
        assert_eq!(a.cursor_row, 1);
        assert_eq!(a.cursor_char, 2);
    }

    #[test]
    fn vertical_movement_uses_screen_rows() {
        let mut a = app("aaaaaaaaaabbbbb\nshort");
        a.buf.set_cursor(0, 2);
        a.relayout();
        a.move_vertical(1);
        // Moving down from row 0 keeps the visual column (2) on row 1.
        assert_eq!(a.buf.line, 0);
        assert_eq!(a.buf.col, 12);
        a.move_vertical(1);
        assert_eq!(a.buf.line, 1);
        assert_eq!(a.buf.col, 2);
    }

    #[test]
    fn ctrl_h_toggles_help() {
        let mut a = app("hello");
        a.handle_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::CONTROL));
        assert!(matches!(a.overlay, Overlay::Help));
        // The panel promises "press any key to close".
        a.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        assert!(matches!(a.overlay, Overlay::None));
    }

    #[test]
    fn f1_opens_help_too() {
        let mut a = app("hello");
        a.handle_key(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE));
        assert!(matches!(a.overlay, Overlay::Help));
        a.handle_key(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE));
        assert!(matches!(a.overlay, Overlay::None));
    }

    #[test]
    fn expand_path_handles_home() {
        // Only the shape is asserted; HOME differs between machines.
        let p = expand_path("~/notes.md");
        assert!(p.to_string_lossy().ends_with("notes.md"));
    }
}
