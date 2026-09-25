//! `claw_type` — a distraction-free Markdown editor for the desktop.
//!
//! A window, a single centred column of text, and nothing else. The Markdown
//! engine and palette are shared with the terminal front-end via the
//! `claw_type` library.

use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui;
use egui::text::{LayoutJob, TextFormat};
use egui::{
    Align, Align2, Color32, FontId, Frame, Id, Key, Margin, Modifiers, Stroke, Vec2,
};

use claw_type::markdown::{self, MStyle, Mode, Role};
use claw_type::palette::{Rgb, Theme};

/// Base body text size, in points.
const BODY_SIZE: f32 = 18.0;
/// The writing column never grows wider than this.
const COLUMN_MAX: f32 = 760.0;
/// Blank space above and below the document.
const TOP_PAD: f32 = 44.0;

const HELP: &str = "\
claw_type — a distraction-free Markdown editor

USAGE:
    claw_type [FILE]

ARGS:
    FILE    Markdown file to open

OPTIONS:
    -h, --help       Print this help
    -V, --version    Print the version

Inside the editor, press F1 for the list of shortcuts.
";

fn main() -> eframe::Result {
    let path = parse_args();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("claw_type")
            .with_inner_size([900.0, 720.0])
            .with_min_inner_size([420.0, 320.0]),
        ..Default::default()
    };

    eframe::run_native(
        "claw_type",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(&cc.egui_ctx, path)) as Box<dyn eframe::App>)),
    )
}

fn parse_args() -> Option<PathBuf> {
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "-h" | "--help" => {
                print!("{HELP}");
                std::process::exit(0);
            }
            "-V" | "--version" => {
                println!("claw_type {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            other if other.starts_with('-') => {}
            other => return Some(PathBuf::from(other)),
        }
    }
    None
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    Write,
    Preview,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Modal {
    None,
    Help,
    ConfirmQuit,
}

struct App {
    text: String,
    path: Option<PathBuf>,
    dirty: bool,
    view: View,
    focus: bool,
    typewriter: bool,
    show_bar: bool,
    theme: Theme,
    status: Option<String>,
    modal: Modal,

    /// Line/column of the caret, refreshed each frame.
    cursor_line: usize,
    cursor_col: usize,
    /// The paragraph holding the caret, used to dim everything else.
    focus_para: (usize, usize),
    /// Caret position last frame, so typewriter mode only recentres on movement.
    prev_cursor: Option<usize>,
    /// Whether the editor should grab keyboard focus on the next frame.
    focus_requested: bool,
}

impl App {
    fn new(ctx: &egui::Context, path: Option<PathBuf>) -> Self {
        let theme = Theme::default();
        configure(ctx, &theme);

        let mut app = Self {
            text: String::new(),
            path: None,
            dirty: false,
            view: View::Write,
            focus: false,
            typewriter: false,
            show_bar: true,
            theme,
            status: None,
            modal: Modal::None,
            cursor_line: 0,
            cursor_col: 0,
            focus_para: (0, 0),
            prev_cursor: None,
            focus_requested: true,
        };

        if let Some(path) = path {
            app.load(path);
        }
        app
    }

    // -- files --------------------------------------------------------------

    fn load(&mut self, path: PathBuf) {
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                self.text = text.replace("\r\n", "\n");
                self.path = Some(path.clone());
                self.dirty = false;
                self.prev_cursor = None;
                self.focus_requested = true;
                self.set_status(format!("Opened {}", path.display()));
            }
            Err(e) => self.set_status(format!("Could not open {}: {e}", path.display())),
        }
    }

    fn save(&mut self) {
        match self.path.clone() {
            Some(path) => self.write(path),
            None => {
                if let Some(path) = save_dialog() {
                    self.write(path);
                }
            }
        }
    }

    fn write(&mut self, path: PathBuf) {
        match std::fs::write(&path, &self.text) {
            Ok(()) => {
                self.path = Some(path.clone());
                self.dirty = false;
                self.set_status(format!("Saved {}", path.display()));
            }
            Err(e) => self.set_status(format!("Save failed: {e}")),
        }
    }

    fn open_dialog(&mut self) {
        if let Some(path) = open_dialog() {
            self.load(path);
        }
    }

    fn new_file(&mut self) {
        if self.dirty {
            self.set_status("Unsaved changes — save first (Cmd/Ctrl+S)".to_owned());
            return;
        }
        self.text.clear();
        self.path = None;
        self.dirty = false;
        self.prev_cursor = None;
        self.focus_requested = true;
        self.set_status("New file".to_owned());
    }

    fn set_status(&mut self, message: String) {
        self.status = Some(message);
    }

    // -- shortcuts ----------------------------------------------------------

    fn shortcuts(&mut self, ctx: &egui::Context) {
        if self.modal != Modal::None {
            if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
                self.modal = Modal::None;
            }
            if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::F1)) {
                self.modal = if self.modal == Modal::Help {
                    Modal::None
                } else {
                    Modal::Help
                };
            }
            return;
        }

        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::S)) {
            self.save();
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::O)) {
            self.open_dialog();
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::N)) {
            self.new_file();
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::P)) {
            self.view = match self.view {
                View::Write => View::Preview,
                View::Preview => View::Write,
            };
            if self.view == View::Write {
                self.focus_requested = true;
            }
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::F)) {
            self.focus = !self.focus;
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::T)) {
            self.typewriter = !self.typewriter;
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::B)) {
            self.show_bar = !self.show_bar;
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Q)) {
            self.request_quit(ctx);
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::F1)) {
            self.modal = Modal::Help;
        }

        // Typing while looking at the preview returns you to the editor.
        let typed = ctx.input(|i| {
            i.raw
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::Text(text) if !text.is_empty()))
        });
        if self.view == View::Preview && typed {
            self.view = View::Write;
            self.focus_requested = true;
        }

        // Dropping a file onto the window opens it.
        let dropped: Option<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .first()
                .map(|file| file.path().to_path_buf())
        });
        if let Some(path) = dropped {
            self.load(path);
        }
    }

    fn request_quit(&mut self, ctx: &egui::Context) {
        if self.dirty {
            self.modal = Modal::ConfirmQuit;
        } else {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    /// Intercept the window close button when there is unsaved work.
    fn guard_close(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) && self.dirty {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.modal = Modal::ConfirmQuit;
        }
    }

    // -- layout -------------------------------------------------------------

    fn body(&mut self, ui: &mut egui::Ui) {
        let scroll_id = match self.view {
            View::Write => "write_scroll",
            View::Preview => "preview_scroll",
        };

        egui::ScrollArea::vertical()
            .id_salt(scroll_id)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let available = ui.available_width();
                let width = available.min(COLUMN_MAX);
                let pad = ((available - width) * 0.5).max(0.0);

                ui.add_space(TOP_PAD);
                ui.horizontal(|ui| {
                    ui.add_space(pad);
                    ui.vertical(|ui| {
                        ui.set_width(width);
                        match self.view {
                            View::Write => self.write_view(ui, width),
                            View::Preview => self.preview_view(ui, width),
                        }
                    });
                });
                ui.add_space(TOP_PAD);
            });
    }

    fn write_view(&mut self, ui: &mut egui::Ui, width: f32) {
        let theme = self.theme;
        let focus = self.focus;
        let focus_para = self.focus_para;
        let font = FontId::proportional(BODY_SIZE);

        let mut layouter = |ui: &egui::Ui,
                            buffer: &dyn egui::widgets::TextBuffer,
                            wrap_width: f32|
         -> Arc<egui::Galley> {
            let job = write_job(
                buffer.as_str(),
                &theme,
                focus,
                focus_para,
                font.clone(),
                wrap_width,
            );
            ui.ctx().fonts_mut(|fonts| fonts.layout_job(job))
        };

        let output = egui::TextEdit::multiline(&mut self.text)
            .id(Id::new("claw_type_editor"))
            .frame(Frame::NONE)
            .desired_width(width)
            .desired_rows(1)
            .hint_text("Start writing…")
            .layouter(&mut layouter)
            .show(ui);

        let response = output.response.response.clone();
        if self.focus_requested {
            response.request_focus();
            self.focus_requested = false;
        }
        let cursor = output
            .cursor_range
            .as_ref()
            .map(|range| range.primary.index.0);

        if let Some(index) = cursor {
            let (line, col) = line_col(&self.text, index);
            self.cursor_line = line;
            self.cursor_col = col;
            self.focus_para = paragraph_bounds(&self.text, line);
        }

        if response.changed() {
            self.dirty = true;
            self.status = None;
        }

        // Typewriter scrolling: keep the caret vertically centred, but only
        // when it actually moves, so the user can still scroll while idle.
        let moved = cursor != self.prev_cursor;
        self.prev_cursor = cursor;
        if self.typewriter
            && moved
            && response.has_focus()
            && let Some(range) = output.cursor_range.as_ref()
        {
            let local = output.galley.pos_from_cursor(range.primary);
            let rect =
                egui::Rect::from_min_size(output.galley_pos + local.min.to_vec2(), local.size());
            ui.scroll_to_rect(rect, Some(Align::Center));
        }
    }

    fn preview_view(&mut self, ui: &mut egui::Ui, width: f32) {
        let theme = self.theme;
        let lines: Vec<String> = self.text.split('\n').map(str::to_string).collect();
        let rendered = markdown::transform(&lines, Mode::Render, 0);

        ui.spacing_mut().item_spacing.y = 0.0;

        for line in &rendered {
            let only_rule = !line.is_empty() && line.iter().all(|(_, span)| span.role == Role::Rule);
            if only_rule {
                ui.add_space(12.0);
                ui.separator();
                ui.add_space(12.0);
                continue;
            }
            if line.is_empty() {
                ui.add_space(BODY_SIZE * 0.85);
                continue;
            }

            let heading = matches!(line.first().map(|(_, s)| s.role), Some(Role::Heading(_)));
            if heading {
                ui.add_space(BODY_SIZE * 0.6);
            }
            ui.add(egui::Label::new(preview_job(line, &theme, width)).wrap());
            if heading {
                ui.add_space(BODY_SIZE * 0.2);
            }
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let theme = self.theme;
        let name = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "untitled.md".to_owned());
        let dot = if self.dirty { " ●" } else { "" };
        let mode = match self.view {
            View::Write => "WRITE",
            View::Preview => "PREVIEW",
        };

        let mut flags: Vec<&str> = Vec::new();
        if self.focus {
            flags.push("focus");
        }
        if self.typewriter {
            flags.push("typewriter");
        }
        let flags = if flags.is_empty() {
            String::new()
        } else {
            format!("  ·  {}", flags.join(" · "))
        };

        let mut text = format!(
            "{name}{dot}  ·  {mode}  ·  {} words  ·  {} chars  ·  Ln {}/{}{flags}",
            word_count(&self.text),
            self.text.chars().count(),
            self.cursor_line + 1,
            self.text.split('\n').count(),
        );
        if let Some(status) = &self.status {
            text.push_str("  ·  ");
            text.push_str(status);
        }

        ui.label(
            egui::RichText::new(text)
                .size(12.0)
                .color(rgb(theme.bar_fg)),
        );
    }

    fn modals(&mut self, ctx: &egui::Context) {
        match self.modal {
            Modal::None => {}
            Modal::Help => self.help_window(ctx),
            Modal::ConfirmQuit => self.quit_window(ctx),
        }
    }

    fn help_window(&mut self, ctx: &egui::Context) {
        egui::Window::new("Shortcuts")
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                let theme = self.theme;
                let key = |ui: &mut egui::Ui, k: &str| {
                    ui.label(
                        egui::RichText::new(k)
                            .monospace()
                            .color(rgb(theme.accent))
                            .strong(),
                    );
                };
                let description =
                    |ui: &mut egui::Ui, d: &str| ui.label(egui::RichText::new(d).color(rgb(theme.fg)));

                egui::Grid::new("shortcuts_grid")
                    .num_columns(2)
                    .spacing([18.0, 6.0])
                    .show(ui, |ui| {
                        let rows: [(&str, &str); 13] = [
                            ("Cmd/Ctrl+S", "Save"),
                            ("Cmd/Ctrl+O", "Open a file"),
                            ("Cmd/Ctrl+N", "New file"),
                            ("Cmd/Ctrl+Q", "Quit"),
                            ("Cmd/Ctrl+P", "Toggle rendered preview"),
                            ("Cmd/Ctrl+F", "Focus mode — dim other paragraphs"),
                            ("Cmd/Ctrl+T", "Typewriter scrolling"),
                            ("Cmd/Ctrl+B", "Show or hide the status bar"),
                            ("Cmd/Ctrl+Z", "Undo"),
                            ("Shift+Cmd/Ctrl+Z", "Redo"),
                            ("F1", "This help"),
                            ("Esc", "Close a panel"),
                            ("drop a file", "Open it"),
                        ];
                        for (k, d) in rows {
                            key(ui, k);
                            description(ui, d);
                            ui.end_row();
                        }
                    });
            });
    }

    fn quit_window(&mut self, ctx: &egui::Context) {
        egui::Window::new("Unsaved changes")
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label("Save your changes before quitting?");
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        self.save();
                        if !self.dirty {
                            self.modal = Modal::None;
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    }
                    if ui.button("Discard").clicked() {
                        self.dirty = false;
                        self.modal = Modal::None;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    if ui.button("Cancel").clicked() {
                        self.modal = Modal::None;
                    }
                });
            });
    }

    /// Draw the whole interface into `ui`.
    ///
    /// Split out from the [`eframe::App`] implementation so it can be driven
    /// headlessly in tests.
    fn draw(&mut self, ui: &mut egui::Ui) {
        let theme = self.theme;
        self.shortcuts(ui.ctx());
        self.guard_close(ui.ctx());

        if self.show_bar {
            egui::Panel::bottom("status_bar")
                .frame(
                    Frame::NONE
                        .fill(rgb(theme.bar_bg))
                        .inner_margin(Margin::symmetric(18, 7)),
                )
                .show(ui, |ui| self.status_bar(ui));
        }

        egui::CentralPanel::default()
            .frame(Frame::NONE.fill(rgb(theme.bg)))
            .show(ui, |ui| self.body(ui));

        self.modals(ui.ctx());
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.draw(ui);
    }
}

// -- styling ----------------------------------------------------------------

fn configure(ctx: &egui::Context, theme: &Theme) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = rgb(theme.bg);
    visuals.window_fill = rgb(theme.overlay_bg);
    visuals.extreme_bg_color = rgb(theme.bar_bg);
    visuals.faint_bg_color = rgb(theme.bar_bg);
    visuals.override_text_color = Some(rgb(theme.fg));
    visuals.selection.bg_fill = blend(rgb(theme.accent), rgb(theme.bg), 0.6);
    visuals.selection.stroke = Stroke::new(1.0, rgb(theme.accent));
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, rgb(theme.border));
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, rgb(theme.border));
    ctx.set_visuals(visuals);
}

/// Build the syntax-highlighted layout for the editable view.
fn write_job(
    text: &str,
    theme: &Theme,
    focus: bool,
    focus_para: (usize, usize),
    font: FontId,
    wrap_width: f32,
) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap_width;

    let lines: Vec<String> = text.split('\n').map(str::to_string).collect();
    let tagged = markdown::transform(&lines, Mode::Highlight, 0);

    for (index, line) in tagged.iter().enumerate() {
        let dim = focus && !(index >= focus_para.0 && index <= focus_para.1);
        push_runs(&mut job, line, |span| write_format(span, theme, dim, &font));
        if index + 1 < tagged.len() {
            job.append("\n", 0.0, write_format(MStyle::text(), theme, dim, &font));
        }
    }

    job
}

/// Build the layout for one rendered preview line.
fn preview_job(line: &[(char, MStyle)], theme: &Theme, width: f32) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = width;
    push_runs(&mut job, line, |span| render_format(span, theme));
    job
}

/// Append a tagged line to a job, coalescing runs that share a format.
fn push_runs(
    job: &mut LayoutJob,
    line: &[(char, MStyle)],
    format: impl Fn(MStyle) -> TextFormat,
) {
    let mut run = String::new();
    let mut run_style: Option<MStyle> = None;

    let flush = |job: &mut LayoutJob, run: &mut String, style: &mut Option<MStyle>| {
        if let Some(style) = style.take()
            && !run.is_empty()
        {
            let text = std::mem::take(run);
            job.append(&text, 0.0, format(style));
        }
    };

    for &(ch, span) in line {
        let disp = if ch == '\t' { ' ' } else { ch };
        match run_style {
            Some(current) if current == span => run.push(disp),
            _ => {
                flush(job, &mut run, &mut run_style);
                run.push(disp);
                run_style = Some(span);
            }
        }
    }
    flush(job, &mut run, &mut run_style);
}

fn write_format(span: MStyle, theme: &Theme, dim: bool, font: &FontId) -> TextFormat {
    let color = if dim {
        rgb(theme.dim)
    } else {
        let base = rgb(theme.role_color(span.role));
        if matches!(span.role, Role::HeadingMarker(_)) {
            blend(base, rgb(theme.bg), 0.5)
        } else if span.bold {
            brighten(base)
        } else {
            base
        }
    };

    let mut format = TextFormat {
        font_id: font.clone(),
        color,
        ..Default::default()
    };
    format.italics = span.italic;
    if !dim && span.underline {
        format.underline = Stroke::new(1.0, color);
    }
    if !dim && span.strike {
        format.strikethrough = Stroke::new(1.0, color);
    }
    format
}

fn render_format(span: MStyle, theme: &Theme) -> TextFormat {
    let (font, mut color) = match span.role {
        Role::Heading(level) => (
            FontId::proportional(heading_size(level)),
            brighten(rgb(theme.role_color(span.role))),
        ),
        Role::HeadingMarker(_) | Role::Marker | Role::Fence => {
            (FontId::proportional(BODY_SIZE), rgb(theme.marker))
        }
        Role::Code => (FontId::monospace(BODY_SIZE * 0.94), rgb(theme.code)),
        Role::Quote => (FontId::proportional(BODY_SIZE), rgb(theme.quote)),
        Role::List => (FontId::proportional(BODY_SIZE), rgb(theme.list)),
        Role::Link => (FontId::proportional(BODY_SIZE), rgb(theme.link)),
        Role::Url => (FontId::proportional(BODY_SIZE), rgb(theme.dim)),
        Role::Rule => (FontId::monospace(BODY_SIZE), rgb(theme.rule)),
        Role::Text => (FontId::proportional(BODY_SIZE), rgb(theme.fg)),
    };

    if span.bold {
        color = brighten(color);
    }

    let mut format = TextFormat {
        font_id: font,
        color,
        ..Default::default()
    };
    format.italics = span.italic;
    if span.underline {
        format.underline = Stroke::new(1.0, color);
    }
    if span.strike {
        format.strikethrough = Stroke::new(1.0, color);
    }
    format
}

fn heading_size(level: u8) -> f32 {
    match level {
        1 => 30.0,
        2 => 25.0,
        3 => 21.0,
        4 => BODY_SIZE + 1.0,
        5 | 6 => BODY_SIZE,
        _ => BODY_SIZE,
    }
}

// -- helpers ----------------------------------------------------------------

fn rgb(color: Rgb) -> Color32 {
    Color32::from_rgb(color.0, color.1, color.2)
}

/// Mix `a` toward `b`; `t` of 1.0 gives `b`.
fn blend(a: Color32, b: Color32, t: f32) -> Color32 {
    let mix = |x: u8, y: u8| (x as f32 * (1.0 - t) + y as f32 * t).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()))
}

fn brighten(color: Color32) -> Color32 {
    blend(color, Color32::WHITE, 0.3)
}

fn word_count(text: &str) -> usize {
    text.split_whitespace().count()
}

/// Convert a character index into a `(line, column)` pair.
fn line_col(text: &str, char_index: usize) -> (usize, usize) {
    let mut line = 0;
    let mut col = 0;
    for (i, ch) in text.chars().enumerate() {
        if i >= char_index {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    (line, col)
}

/// The paragraph (block of non-blank lines) containing `line`.
fn paragraph_bounds(text: &str, line: usize) -> (usize, usize) {
    let lines: Vec<&str> = text.split('\n').collect();
    if lines.is_empty() {
        return (0, 0);
    }
    let line = line.min(lines.len() - 1);
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

fn open_dialog() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter("Markdown", &["md", "markdown", "mdown", "txt"])
        .pick_file()
}

fn save_dialog() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter("Markdown", &["md", "markdown"])
        .set_file_name("untitled.md")
        .save_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_col_counts_characters() {
        let text = "hello\nworld";
        assert_eq!(line_col(text, 0), (0, 0));
        assert_eq!(line_col(text, 3), (0, 3));
        assert_eq!(line_col(text, 6), (1, 0));
        assert_eq!(line_col(text, 8), (1, 2));
    }

    #[test]
    fn line_col_handles_multibyte() {
        let text = "héllo\nwörld";
        assert_eq!(line_col(text, 6), (1, 0));
        assert_eq!(line_col(text, 8), (1, 2));
    }

    #[test]
    fn paragraph_bounds_follow_blank_lines() {
        let text = "a\nb\n\nc\nd\ne";
        assert_eq!(paragraph_bounds(text, 1), (0, 1));
        assert_eq!(paragraph_bounds(text, 4), (3, 5));
        assert_eq!(paragraph_bounds(text, 2), (2, 2));
    }

    #[test]
    fn word_count_ignores_whitespace_runs() {
        assert_eq!(word_count("  one   two\nthree "), 3);
    }

    #[test]
    fn job_text_matches_the_source_exactly() {
        let theme = Theme::default();
        let source = "# Title\n\nsome **bold** text\n- a\n";
        let job = write_job(
            source,
            &theme,
            false,
            (0, 0),
            FontId::proportional(BODY_SIZE),
            400.0,
        );
        assert_eq!(job.text, source);
    }

    #[test]
    fn job_text_preserves_trailing_blank_lines() {
        let theme = Theme::default();
        let source = "one\n\n\n";
        let job = write_job(
            source,
            &theme,
            false,
            (0, 0),
            FontId::proportional(BODY_SIZE),
            400.0,
        );
        assert_eq!(job.text, source);
    }

    #[test]
    fn tabs_are_displayed_as_spaces() {
        let theme = Theme::default();
        let job = write_job(
            "\tx",
            &theme,
            false,
            (0, 0),
            FontId::proportional(BODY_SIZE),
            400.0,
        );
        assert_eq!(job.text, " x");
    }

    /// Drive the whole interface headlessly: this exercises the panels, the
    /// `TextEdit` with its custom layouter, the fonts and the preview.
    #[test]
    fn draws_a_frame_without_panicking() {
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, None);
        app.text = "# Hello\n\nsome **bold** and `code` text\n\n- one\n- two\n\n> quote".to_owned();

        let frame = |app: &mut App| {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| app.draw(ui));
            // No renderer is attached, so the texture deltas are discarded
            // explicitly rather than left dangling.
            output.textures_delta.clear();
        };

        frame(&mut app);

        app.view = View::Preview;
        frame(&mut app);

        app.focus = true;
        app.typewriter = true;
        app.show_bar = false;
        frame(&mut app);

        app.modal = Modal::Help;
        frame(&mut app);
    }
}
