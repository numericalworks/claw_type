//! Drawing: the centred writing column, the status bar and the modal panels.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::palette::Theme;
use crate::tui::app::{App, Overlay, PromptKind, TOP_MARGIN, View};
use crate::tui::style::ThemeExt;
use crate::wrap::VisualRow;

/// Draw a full frame.
pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    app.viewport = (area.width, area.height);

    frame.render_widget(
        Block::default().style(Style::default().bg(app.theme.bg_color())),
        area,
    );
    app.relayout();

    let bar = u16::from(app.show_bar);
    let body = Rect {
        x: area.x,
        y: area.y + TOP_MARGIN as u16,
        width: area.width,
        height: area.height.saturating_sub(TOP_MARGIN as u16 + bar),
    };
    draw_body(frame, app, body);

    if app.show_bar {
        let bar_rect = Rect {
            x: area.x,
            y: area.y + area.height.saturating_sub(1),
            width: area.width,
            height: 1,
        };
        draw_bar(frame, app, bar_rect);
    }

    match &app.overlay {
        Overlay::None => {}
        Overlay::Help => draw_help(frame, app, area),
        Overlay::ConfirmQuit => draw_confirm(frame, app, area),
        Overlay::Prompt { kind, input, cursor } => {
            draw_prompt(frame, app, area, *kind, input, *cursor);
        }
    }
}

fn draw_body(frame: &mut Frame, app: &App, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let width = app.content_width.min(area.width as usize).max(1) as u16;
    let x = area.x + area.width.saturating_sub(width) / 2;
    let rect = Rect::new(x, area.y, width, area.height);

    if app.view == View::Write && app.buf.lines.len() == 1 && app.buf.lines[0].is_empty() {
        draw_placeholder(frame, app, rect);
        return;
    }

    let total = app.layout.rows.len();
    let start = app.scroll.min(total.saturating_sub(1));
    let end = (start + area.height as usize).min(total);

    let mut lines: Vec<Line<'static>> = Vec::with_capacity(end - start);
    for index in start..end {
        let row = &app.layout.rows[index];
        let dim = app.focus && !(row.line >= app.focus_para.0 && row.line <= app.focus_para.1);
        let cursor = if app.view == View::Write && index == app.cursor_row {
            Some(app.cursor_char)
        } else {
            None
        };
        lines.push(row_to_line(row, cursor, dim, &app.theme));
    }

    frame.render_widget(Paragraph::new(Text::from(lines)), rect);
}

/// Friendly first-run hint shown while the buffer is completely empty.
fn draw_placeholder(frame: &mut Frame, app: &App, rect: Rect) {
    let theme = &app.theme;
    let cursor = Span::styled(" ", theme.cursor_style());
    let lines = vec![
        Line::from(cursor),
        Line::from(""),
        Line::from(Span::styled("Start writing…", theme.dim_style())),
        Line::from(""),
        Line::from(Span::styled(
            "Ctrl+H shortcuts   Ctrl+P preview   Ctrl+S save",
            Style::default().fg(theme.marker_color()),
        )),
    ];
    frame.render_widget(Paragraph::new(Text::from(lines)), rect);
}

/// Turn one wrapped row into a styled line, splitting out the cursor cell.
fn row_to_line(
    row: &VisualRow,
    cursor: Option<usize>,
    dim: bool,
    theme: &Theme,
) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut run = String::new();
    let mut run_style: Option<Style> = None;

    for (i, &(ch, span)) in row.chars.iter().enumerate() {
        let style = theme.style_of(span, dim);
        if cursor == Some(i) {
            push_run(&mut spans, &mut run, &mut run_style);
            spans.push(Span::styled(display(ch).to_string(), theme.cursor_style()));
            continue;
        }
        match run_style {
            Some(s) if s == style => run.push(display(ch)),
            _ => {
                push_run(&mut spans, &mut run, &mut run_style);
                run.push(display(ch));
                run_style = Some(style);
            }
        }
    }
    push_run(&mut spans, &mut run, &mut run_style);

    if cursor == Some(row.chars.len()) {
        spans.push(Span::styled(" ", theme.cursor_style()));
    }
    if spans.is_empty() {
        spans.push(Span::raw(""));
    }
    Line::from(spans)
}

fn push_run(spans: &mut Vec<Span<'static>>, run: &mut String, style: &mut Option<Style>) {
    if let Some(s) = style.take() {
        if run.is_empty() {
            run.clear();
        } else {
            let text = std::mem::take(run);
            spans.push(Span::styled(text, s));
        }
    }
}

fn display(c: char) -> char {
    if c == '\t' { ' ' } else { c }
}

fn draw_bar(frame: &mut Frame, app: &App, area: Rect) {
    if area.width == 0 {
        return;
    }
    let theme = &app.theme;
    let name = app
        .buf
        .path
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "untitled.md".to_string());
    let dirty = if app.buf.dirty { " ●" } else { "" };
    let mode = match app.view {
        View::Write => "WRITE",
        View::Preview => "PREVIEW",
    };

    let mut flags: Vec<&str> = Vec::new();
    if app.focus {
        flags.push("focus");
    }
    if app.typewriter {
        flags.push("typewriter");
    }
    let flags = if flags.is_empty() {
        String::new()
    } else {
        format!("  ·  {}", flags.join(" · "))
    };

    let mut text = format!(
        " {name}{dirty}  ·  {mode}  ·  {} words  ·  {} chars  ·  Ln {}/{}{flags}",
        app.buf.word_count(),
        app.buf.char_count(),
        app.buf.line + 1,
        app.buf.line_count(),
    );
    if let Some(status) = &app.status {
        text.push_str("  ·  ");
        text.push_str(status);
    }

    frame.render_widget(Paragraph::new(text).style(theme.bar_style()), area);
}

fn draw_help(frame: &mut Frame, app: &App, area: Rect) {
    let rows = help_lines(&app.theme);
    let width = (area.width as usize).saturating_sub(6).clamp(20, 56) as u16;
    let height = (rows.len() as u16 + 2).min(area.height);
    let rect = centered(area, width, height);

    frame.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(app.theme.border_style())
        .title(" Shortcuts ")
        .style(app.theme.overlay_style());
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    frame.render_widget(
        Paragraph::new(Text::from(rows)).wrap(Wrap { trim: true }),
        inner,
    );
}

fn help_lines(theme: &Theme) -> Vec<Line<'static>> {
    let accent = theme.accent_style();
    let text = theme.text();
    let dim = theme.dim_style();

    let heading = |s: &str| Line::from(Span::styled(format!("  {s}"), accent));
    let entry = |key: &str, desc: &str| {
        Line::from(vec![
            Span::styled(format!("  {key:<14}"), accent),
            Span::styled(desc.to_string(), text),
        ])
    };

    vec![
        heading("Writing"),
        entry("Enter", "continue lists & quotes"),
        entry("Tab", "indent two spaces"),
        entry("Shift+Tab", "outdent"),
        Line::from(""),
        heading("View"),
        entry("Ctrl+H / F1", "this help"),
        entry("Ctrl+P", "toggle rendered preview"),
        entry("Ctrl+F", "focus mode — dim other paragraphs"),
        entry("Ctrl+T", "typewriter scrolling"),
        entry("Ctrl+B / F2", "show or hide the status bar"),
        Line::from(""),
        heading("Files & history"),
        entry("Ctrl+S", "save"),
        entry("Ctrl+O", "open a file (~ is expanded)"),
        entry("Ctrl+N", "new file"),
        entry("Ctrl+Z", "undo"),
        entry("Ctrl+Y", "redo"),
        entry("Ctrl+Q", "quit"),
        Line::from(""),
        heading("Movement"),
        entry("Ctrl+← / →", "by word"),
        entry("Ctrl+Home/End", "document start / end"),
        Line::from(""),
        Line::from(Span::styled("  press any key to close", dim)),
    ]
}

fn draw_prompt(
    frame: &mut Frame,
    app: &App,
    area: Rect,
    kind: PromptKind,
    input: &str,
    cursor: usize,
) {
    let theme = &app.theme;
    let title = match kind {
        PromptKind::Open => " Open file ",
        PromptKind::SaveAs => " Save as ",
    };

    let width = (area.width as usize).saturating_sub(6).clamp(20, 64) as u16;
    let height = 3.min(area.height);
    let y = area.y + area.height.saturating_sub(height + 2);
    let rect = centered_at(area, width, y, height);

    frame.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme.border_style())
        .title(title)
        .style(theme.overlay_style());
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    let chars: Vec<char> = input.chars().collect();
    let cursor = cursor.min(chars.len());
    let before: String = chars[..cursor].iter().collect();
    let at = chars.get(cursor).copied();
    let after: String = chars[cursor.saturating_add(1).min(chars.len())..]
        .iter()
        .collect();

    let spans = vec![
        Span::styled(before, theme.text()),
        Span::styled(
            at.map(|c| c.to_string()).unwrap_or_else(|| " ".to_string()),
            theme.cursor_style(),
        ),
        Span::styled(after, theme.text()),
    ];
    frame.render_widget(Paragraph::new(Line::from(spans)), inner);
}

fn draw_confirm(frame: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;
    let rect = centered(area, 46, 6);

    frame.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme.border_style())
        .title(" Unsaved changes ")
        .style(theme.overlay_style());
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    let lines = vec![
        Line::from(""),
        Line::from(Span::styled("Save before quitting?", theme.text())),
        Line::from(""),
        Line::from(vec![
            Span::styled("[s]", theme.accent_style()),
            Span::styled(" save    ", theme.dim_style()),
            Span::styled("[d]", theme.accent_style()),
            Span::styled(" discard    ", theme.dim_style()),
            Span::styled("[c]", theme.accent_style()),
            Span::styled(" cancel", theme.dim_style()),
        ]),
        Line::from(""),
    ];
    frame.render_widget(Paragraph::new(Text::from(lines)), inner);
}

/// A rectangle of the given size, centred in `area` and clamped to fit.
fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    Rect::new(
        area.x + area.width.saturating_sub(w) / 2,
        area.y + area.height.saturating_sub(h) / 2,
        w,
        h,
    )
}

/// A rectangle of the given size, horizontally centred at a fixed `y`.
fn centered_at(area: Rect, width: u16, y: u16, height: u16) -> Rect {
    let w = width.min(area.width);
    let x = area.x + area.width.saturating_sub(w) / 2;
    let available = area.y.saturating_add(area.height).saturating_sub(y);
    Rect::new(x, y, w, height.min(available).min(area.height))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    /// Render a frame headlessly and return it as a list of text rows.
    fn render(app: &mut App, width: u16, height: u16) -> Vec<String> {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, app)).unwrap();
        let buffer = terminal.backend().buffer();
        buffer
            .content
            .chunks(buffer.area.width as usize)
            .map(|cells| cells.iter().map(|c| c.symbol()).collect::<String>())
            .collect()
    }

    #[test]
    fn draws_the_document_in_a_centred_column() {
        let mut app = App::new(None);
        app.buf.set_text("# Hello\n\nworld");
        let rows = render(&mut app, 40, 12);
        let joined = rows.join("\n");
        assert!(joined.contains("Hello"), "{joined}");
        assert!(joined.contains("world"), "{joined}");
        let heading = rows.iter().find(|r| r.contains("Hello")).unwrap();
        assert!(heading.starts_with(' '), "column should not touch the edge");
    }

    #[test]
    fn empty_buffer_shows_the_placeholder() {
        let mut app = App::new(None);
        let rows = render(&mut app, 40, 12);
        assert!(rows.join("\n").contains("Start writing"));
    }

    #[test]
    fn preview_drops_markdown_punctuation() {
        let mut app = App::new(None);
        app.buf.set_text("# Title\n\n- item");
        app.view = View::Preview;
        let rows = render(&mut app, 40, 12);
        let joined = rows.join("\n");
        assert!(joined.contains("Title"));
        assert!(!joined.contains('#'), "{joined}");
        assert!(joined.contains('\u{2022}'), "bullets become dots: {joined}");
    }

    #[test]
    fn help_overlay_lists_shortcuts() {
        let mut app = App::new(None);
        app.overlay = Overlay::Help;
        let rows = render(&mut app, 60, 30);
        assert!(rows.join("\n").contains("Shortcuts"));
    }

    #[test]
    fn status_bar_reports_progress() {
        let mut app = App::new(None);
        app.buf.set_text("one two three");
        let rows = render(&mut app, 60, 12);
        let bar = rows.last().unwrap();
        assert!(bar.contains("3 words"), "{bar}");
    }
}
