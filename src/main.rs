//! `claw_type` — a distraction-free Markdown editor for the desktop.
//!
//! A window, a single centred column of text, and nothing else.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};

use eframe::egui;
use egui::text::{CCursor, CCursorRange, LayoutJob, TextFormat};
use egui::{
    Align, Align2, Color32, FontId, Frame, Id, Key, Margin, Modifiers, Stroke, Vec2,
};

use crate::markdown::{MStyle, Role};
use crate::palette::{Rgb, Theme};
use crate::settings::Settings;

mod fonts;
mod html;
mod lists;
mod markdown;
mod math;
mod ollama;
mod palette;
mod preview;
mod settings;

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

/// The `Id` of the main text editor, used to read and write its caret.
const EDITOR_ID: &str = "claw_type_editor";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Modal {
    None,
    Help,
    Settings,
    ConfirmQuit,
}

/// How the last attempt to reach Ollama went.
enum Connection {
    /// Nothing has been tried yet.
    NotConnected,
    /// A request is in flight.
    Connecting,
    /// The server answered, and its models are in [`App::models`].
    Connected,
    /// The attempt failed, with something worth showing the user.
    Failed(String),
}

struct App {
    text: String,
    path: Option<PathBuf>,
    dirty: bool,
    focus: bool,
    typewriter: bool,
    show_bar: bool,
    theme: Theme,
    status: Option<String>,
    modal: Modal,

    /// How to reach Ollama, and the model to use.
    settings: Settings,
    /// The models the connected server offers.
    models: Vec<String>,
    connection: Connection,
    /// Receives the answer from a connection attempt made off the UI thread.
    connecting: Option<Receiver<Result<Vec<String>, String>>>,

    /// The browser preview, once it has been started.
    preview: Option<preview::Preview>,
    /// The Markdown last handed to the preview, so we only re-render on change.
    published: String,

    /// Line/column of the caret, refreshed each frame.
    cursor_line: usize,
    cursor_col: usize,
    /// The paragraph holding the caret, used to dim everything else.
    focus_para: (usize, usize),
    /// Caret position last frame, so typewriter mode only recentres on movement.
    prev_cursor: Option<usize>,
    /// Whether the editor should grab keyboard focus on the next frame.
    focus_requested: bool,
    /// Whether we have already tried to load a CJK font from the system.
    cjk_attempted: bool,
}

impl App {
    fn new(ctx: &egui::Context, path: Option<PathBuf>) -> Self {
        let theme = Theme::default();
        configure(ctx, &theme);

        let mut app = Self {
            text: String::new(),
            path: None,
            dirty: false,
            focus: false,
            typewriter: false,
            show_bar: true,
            theme,
            status: None,
            modal: Modal::None,
            settings: Settings::load(),
            models: Vec::new(),
            connection: Connection::NotConnected,
            connecting: None,
            preview: None,
            published: String::new(),
            cursor_line: 0,
            cursor_col: 0,
            focus_para: (0, 0),
            prev_cursor: None,
            focus_requested: true,
            cjk_attempted: false,
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

    // -- preview ------------------------------------------------------------

    /// Hand the preview the current document, if it has changed.
    ///
    /// Called every frame; converting and publishing only happens when the text
    /// actually differs from what the browser already has.
    fn sync_preview(&mut self) {
        let Some(preview) = self.preview.as_ref() else {
            return;
        };
        if self.text == self.published {
            return;
        }
        preview.publish(&html::body(&self.text));
        self.published.clone_from(&self.text);
    }

    /// Start the preview server if it is not already running.
    ///
    /// Returns whether a server is available. Split out from
    /// [`App::open_preview`] so tests can start one without launching a
    /// browser.
    fn ensure_preview(&mut self) -> bool {
        if self.preview.is_none() {
            match preview::Preview::start(&self.theme) {
                Some(preview) => {
                    preview.publish(&html::body(&self.text));
                    self.published.clone_from(&self.text);
                    let url = preview.url().to_owned();
                    self.preview = Some(preview);
                    self.set_status(format!("Preview at {url}"));
                }
                None => {
                    self.set_status("Could not start the preview server".to_owned());
                    return false;
                }
            }
        }
        true
    }

    /// Start the preview server, if needed, and open it in the browser.
    fn open_preview(&mut self) {
        if !self.ensure_preview() {
            return;
        }
        // Pressing the shortcut again re-opens the page, in case it was closed.
        if let Some(Err(error)) = self.preview.as_ref().map(preview::Preview::open_in_browser) {
            self.set_status(format!("Could not open a browser: {error}"));
        }
    }

    // -- fonts --------------------------------------------------------------

    /// Load a CJK font from the system the first time the document needs one.
    ///
    /// CJK fonts run to tens of megabytes, so they are not bundled; they are
    /// read from disk only when the text actually contains CJK characters.
    fn maybe_load_cjk(&mut self, ctx: &egui::Context) {
        if self.cjk_attempted || !fonts::needs_cjk(&self.text) {
            return;
        }
        self.cjk_attempted = true;
        if fonts::install_cjk(ctx) {
            // Fonts changed, so re-lay-out the text on the next frame.
            ctx.request_repaint();
        }
    }

    // -- shortcuts ----------------------------------------------------------

    fn shortcuts(&mut self, ctx: &egui::Context) {
        if self.modal != Modal::None {
            if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
                if matches!(self.modal, Modal::Settings) {
                    self.settings.save();
                }
                self.modal = Modal::None;
                return;
            }
            // Cmd+H, Ctrl+H and F1 all toggle the help panel back closed. The
            // unsaved-changes prompt is left alone by them.
            if self.modal == Modal::Help && help_shortcut_pressed(ctx) {
                self.modal = Modal::None;
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
            self.open_preview();
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
        // Cmd/Ctrl+, for settings, as everywhere else.
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Comma)) {
            self.modal = Modal::Settings;
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Q)) {
            self.request_quit(ctx);
        }
        if help_shortcut_pressed(ctx) {
            self.modal = Modal::Help;
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
        self.settings.save();
        if self.dirty {
            self.modal = Modal::ConfirmQuit;
        } else {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    /// Intercept the window close button when there is unsaved work.
    fn guard_close(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) {
            // However this ends, the server settings are worth keeping.
            self.settings.save();
            if self.dirty {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.modal = Modal::ConfirmQuit;
            }
        }
    }

    // -- layout -------------------------------------------------------------

    fn body(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .id_salt("editor_scroll")
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
                        self.editor(ui, width);
                    });
                });
                ui.add_space(TOP_PAD);
            });
    }

    fn editor(&mut self, ui: &mut egui::Ui, width: f32) {
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

        // Snapshot what the editor is looking at before it consumes this
        // frame's input, so we can tell an Enter apart from other edits.
        let prev_len = self.text.len();
        let prev_caret = self.prev_cursor;

        let output = egui::TextEdit::multiline(&mut self.text)
            .id(Id::new(EDITOR_ID))
            .frame(Frame::NONE)
            .desired_width(width)
            .desired_rows(1)
            .hint_text("Start writing…")
            // While a panel is open the document must not take the keystrokes.
            .interactive(self.modal == Modal::None)
            .layouter(&mut layouter)
            .show(ui);

        let response = output.response.response.clone();
        if self.focus_requested {
            response.request_focus();
            self.focus_requested = false;
        }
        let mut cursor = output
            .cursor_range
            .as_ref()
            .map(|range| range.primary.index.0);

        // Continue bullet and numbered lists when Enter was pressed. The editor
        // has already inserted the newline, so this only adds the next marker.
        //
        // `TextEdit` reads events without consuming them, so the Enter is still
        // detectable here — which is what tells a real Enter apart from, say, an
        // undo that happens to re-insert a newline.
        let modifiers = ui.ctx().input(|input| input.modifiers);
        let modified =
            modifiers.shift || modifiers.alt || modifiers.ctrl || modifiers.command;
        let enter_pressed = response.has_focus()
            && ui
                .ctx()
                .input_mut(|input| input.consume_key(Modifiers::NONE, Key::Enter));

        if enter_pressed
            && !modified
            && response.changed()
            && let Some(prev_caret) = prev_caret
            && inserted_one_newline(&self.text, prev_len, prev_caret)
            && let Some(next) = continue_list(&mut self.text, prev_caret)
        {
            let mut state = output.state.clone();
            state
                .cursor
                .set_char_range(Some(CCursorRange::one(CCursor::new(next))));
            state.store(ui.ctx(), Id::new(EDITOR_ID));
            ui.ctx().request_repaint();
            cursor = Some(next);
        }

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

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let theme = self.theme;
        let name = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "untitled.md".to_owned());
        let dot = if self.dirty { " ●" } else { "" };

        let mut flags: Vec<&str> = Vec::new();
        if self.focus {
            flags.push("focus");
        }
        if self.typewriter {
            flags.push("typewriter");
        }
        if self.preview.is_some() {
            flags.push("preview");
        }
        let flags = if flags.is_empty() {
            String::new()
        } else {
            format!("  ·  {}", flags.join("  ·  "))
        };

        let mut text = format!(
            "{name}{dot}  ·  {} words  ·  {} chars  ·  Ln {}/{}{flags}",
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
            Modal::Settings => self.settings_window(ctx),
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
                        let rows: [(&str, &str); 14] = [
                            ("Cmd/Ctrl+S", "Save"),
                            ("Cmd/Ctrl+O", "Open a file"),
                            ("Cmd/Ctrl+N", "New file"),
                            ("Cmd/Ctrl+Q", "Quit"),
                            ("Cmd/Ctrl+P", "Open the preview in your browser"),
                            ("Cmd/Ctrl+,", "Settings for Ollama"),
                            ("Cmd/Ctrl+F", "Focus mode — dim other paragraphs"),
                            ("Cmd/Ctrl+T", "Typewriter scrolling"),
                            ("Cmd/Ctrl+B", "Show or hide the status bar"),
                            ("Cmd/Ctrl+Z", "Undo"),
                            ("Shift+Cmd/Ctrl+Z", "Redo"),
                            ("Cmd/Ctrl+H  or  F1", "This help"),
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

    // -- Ollama -------------------------------------------------------------

    /// Ask the configured server which models it has, off the UI thread so the
    /// window keeps drawing while the request is in flight.
    ///
    /// The settings are written when the panel is closed, not here, so that an
    /// attempt does not touch the disk.
    fn start_connecting(&mut self) {
        let settings = self.settings.clone();
        let (sender, receiver) = std::sync::mpsc::channel();
        self.connecting = Some(receiver);
        self.connection = Connection::Connecting;
        std::thread::spawn(move || {
            let _ = sender.send(ollama::list_models(&settings));
        });
    }

    /// Collect the answer to a connection attempt, once it has arrived.
    fn poll_connection(&mut self) {
        let Some(receiver) = self.connecting.take() else {
            return;
        };
        match receiver.try_recv() {
            Ok(Ok(models)) => {
                self.models = models;
                self.connection = Connection::Connected;
                if self.settings.has_model() && !self.models.contains(&self.settings.model) {
                    self.status = Some(format!("{} is not on this server", self.settings.model));
                }
            }
            Ok(Err(error)) => self.connection = Connection::Failed(error),
            // Still in flight: put it back and look again next frame.
            Err(TryRecvError::Empty) => self.connecting = Some(receiver),
            Err(TryRecvError::Disconnected) => {
                self.connection = Connection::Failed("the request did not finish".to_owned());
            }
        }
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
        let theme = self.theme;
        egui::Window::new("Settings")
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .default_width(480.0)
            .show(ctx, |ui| {
                ui.label(egui::RichText::new("Ollama").size(12.0).color(rgb(theme.dim)));
                ui.add_space(6.0);

                egui::Grid::new("ollama_fields")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("URL");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.settings.url)
                                .desired_width(330.0)
                                .hint_text(settings::DEFAULT_URL),
                        );
                        ui.end_row();

                        ui.label("API key");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.settings.api_key)
                                .desired_width(330.0)
                                .password(true)
                                .hint_text("none"),
                        );
                        ui.end_row();
                    });

                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(
                        "Leave the key empty for a local server; a hosted one needs a bearer token.",
                    )
                    .size(11.0)
                    .color(rgb(theme.dim)),
                );

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    let connecting = matches!(self.connection, Connection::Connecting);
                    if ui
                        .add_enabled(!connecting, egui::Button::new("Connect"))
                        .clicked()
                    {
                        self.start_connecting();
                    }
                    ui.add_space(10.0);

                    let (message, colour) = match &self.connection {
                        Connection::NotConnected => ("Not connected".to_owned(), theme.dim),
                        Connection::Connecting => ("Connecting…".to_owned(), theme.dim),
                        Connection::Connected => (
                            format!("Connected — {} model(s)", self.models.len()),
                            theme.list,
                        ),
                        Connection::Failed(error) => (error.clone(), theme.heading[0]),
                    };
                    ui.label(egui::RichText::new(message).size(12.0).color(rgb(colour)));
                });

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(4.0);
                ui.label(egui::RichText::new("Model").size(12.0).color(rgb(theme.dim)));
                ui.add_space(4.0);

                if self.models.is_empty() {
                    // Nothing fetched yet: name the model that is already
                    // chosen, so it is clear the choice survived the last run
                    // without having to connect again to see it.
                    match &self.connection {
                        Connection::Connected => {
                            ui.label(
                                egui::RichText::new("This server has no models.")
                                    .size(12.0)
                                    .color(rgb(theme.dim)),
                            );
                        }
                        Connection::Connecting => {
                            ui.label(
                                egui::RichText::new("Asking the server…")
                                    .size(12.0)
                                    .color(rgb(theme.dim)),
                            );
                        }
                        _ if self.settings.has_model() => {
                            ui.label(egui::RichText::new(&self.settings.model).color(rgb(theme.fg)));
                            ui.label(
                                egui::RichText::new("(press Connect to change it)")
                                    .size(11.0)
                                    .color(rgb(theme.dim)),
                            );
                        }
                        _ => {
                            ui.label(
                                egui::RichText::new("Connect to choose a model.")
                                    .size(12.0)
                                    .color(rgb(theme.dim)),
                            );
                        }
                    }
                } else {
                    let models = self.models.clone();
                    egui::ScrollArea::vertical()
                        .id_salt("models")
                        .max_height(200.0)
                        .show(ui, |ui| {
                            for model in models {
                                let selected = self.settings.model == model;
                                if ui.selectable_label(selected, model.as_str()).clicked() {
                                    self.settings.model = model;
                                    self.settings.save();
                                }
                            }
                        });
                }

                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button("Done").clicked() {
                        self.settings.save();
                        self.modal = Modal::None;
                    }
                    ui.add_space(10.0);
                    if let Some(path) = settings::config_path() {
                        ui.label(
                            egui::RichText::new(format!("saved to {}", path.display()))
                                .size(10.0)
                                .color(rgb(theme.dim)),
                        );
                    }
                });

                // Look for the answer while a request is in flight.
                if matches!(self.connection, Connection::Connecting) {
                    ui.ctx().request_repaint();
                }
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
        self.maybe_load_cjk(ui.ctx());
        self.sync_preview();

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
        self.poll_connection();
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.draw(ui);
    }
}

// -- styling ----------------------------------------------------------------

fn configure(ctx: &egui::Context, theme: &Theme) {
    // Compile-time font fallbacks for every major non-CJK script, so those
    // languages render instead of showing as empty boxes. CJK is loaded later,
    // only if the document needs it (see `App::maybe_load_cjk`).
    fonts::install_bundled(ctx);

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
    visuals.text_cursor.stroke = Stroke::new(2.0, rgb(theme.accent));
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
    let tagged = markdown::highlight(&lines);

    for (index, line) in tagged.iter().enumerate() {
        let dim = focus && !(index >= focus_para.0 && index <= focus_para.1);
        push_runs(&mut job, line, |span| write_format(span, theme, dim, &font));
        if index + 1 < tagged.len() {
            job.append("\n", 0.0, write_format(MStyle::text(), theme, dim, &font));
        }
    }

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

/// Byte offset of the `index`-th `char`, clamped to the end of the string.
fn byte_index(text: &str, index: usize) -> usize {
    text.char_indices()
        .nth(index)
        .map(|(byte, _)| byte)
        .unwrap_or(text.len())
}

/// Whether this frame's edit was a single newline inserted at `prev_caret`.
///
/// Typing an ordinary character also grows the text by one byte, so the length
/// alone is not enough: the inserted byte itself has to be the newline.
fn inserted_one_newline(text: &str, prev_len: usize, prev_caret: usize) -> bool {
    text.len() == prev_len + 1 && text.as_bytes().get(byte_index(text, prev_caret)) == Some(&b'\n')
}

/// Continue a bullet, numbered or quoted list after the newline at `caret`.
///
/// The editor has already inserted the newline, so this only adds the next
/// marker and returns the caret position it should sit at. Returns `None` when
/// the line is not a list item, in which case nothing is changed.
fn continue_list(text: &mut String, caret: usize) -> Option<usize> {
    let chars: Vec<char> = text.chars().collect();
    let caret = caret.min(chars.len());
    let line_start = chars[..caret]
        .iter()
        .rposition(|c| *c == '\n')
        .map(|position| position + 1)
        .unwrap_or(0);
    let before_caret: String = chars[line_start..caret].iter().collect();

    match lists::continuation(&before_caret) {
        lists::Continuation::None => None,
        lists::Continuation::Prefix(prefix) => {
            // The newline Enter added sits at `caret`; the marker follows it.
            let insert_at = caret + 1;
            let byte = byte_index(text, insert_at);
            text.insert_str(byte, &prefix);
            Some(insert_at + prefix.chars().count())
        }
        lists::Continuation::Clear => {
            // The line held only a marker, so this Enter ends the list: drop the
            // marker *and* the newline the editor just added, leaving one empty
            // line. `caret` is the newline, so removing up to `caret + 1` keeps
            // whatever followed the caret on this line.
            let start = byte_index(text, line_start);
            let end = byte_index(text, caret + 1);
            text.replace_range(start..end, "");
            Some(line_start)
        }
    }
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

/// True if the user pressed a "show help" shortcut: Cmd+H or Ctrl+H, or F1.
///
/// `Modifiers::COMMAND` and `Modifiers::CTRL` are distinct chords on macOS, so
/// both are accepted; on other platforms they coincide.
fn help_shortcut_pressed(ctx: &egui::Context) -> bool {
    ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::H))
        || ctx.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::H))
        || ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::F1))
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
    use egui::widgets::text_edit::TextEditState;

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

    /// Run one frame headlessly. No renderer is attached, so the texture deltas
    /// are discarded explicitly rather than left dangling.
    fn frame(ctx: &egui::Context, app: &mut App, events: Vec<egui::Event>) {
        let mut output = ctx.run_ui(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ui| app.draw(ui),
        );
        output.textures_delta.clear();
    }

    /// A synthetic key-press event.
    fn key(key: Key, modifiers: Modifiers) -> Vec<egui::Event> {
        vec![egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }]
    }

    /// The advance width of every glyph the given text lays out to.
    fn glyph_widths(ctx: &egui::Context, text: &str) -> Vec<i32> {
        // The font system is only built once the context has run a frame.
        let mut output = ctx.run_ui(egui::RawInput::default(), |_ui| {});
        output.textures_delta.clear();

        let mut job = LayoutJob::default();
        job.append(
            text,
            0.0,
            TextFormat {
                font_id: FontId::proportional(BODY_SIZE),
                color: Color32::WHITE,
                ..Default::default()
            },
        );
        let galley = ctx.fonts_mut(|fonts| fonts.layout_job(job));
        galley
            .rows
            .iter()
            .flat_map(|row| row.row.glyphs.iter())
            .map(|glyph| (glyph.advance_width * 64.0).round() as i32)
            .collect()
    }

    fn distinct(values: &[i32]) -> usize {
        let mut sorted = values.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        sorted.len()
    }

    /// Registering the bundled fonts must change the glyphs, not merely the
    /// font: without them every character falls back to one `.notdef` box and
    /// therefore shares a single advance width.
    #[test]
    fn bundled_fallbacks_change_the_glyphs() {
        let text = "வாழ்க வையகம்";

        let without = glyph_widths(&egui::Context::default(), text);

        let ctx = egui::Context::default();
        fonts::install_bundled(&ctx);
        let with = glyph_widths(&ctx, text);

        assert!(!without.is_empty() && !with.is_empty());
        assert!(
            distinct(&with) > distinct(&without),
            "bundled fonts should bring in real glyphs: without={without:?} with={with:?}"
        );
    }

    /// Every bundled script renders with real, varying glyphs — i.e. no tofu.
    #[test]
    fn bundled_fonts_render_many_scripts() {
        let ctx = egui::Context::default();
        fonts::install_bundled(&ctx);

        let samples = [
            ("Tamil", "வாழ்க வையகம்"),
            ("Devanagari", "नमस्ते दुनिया"),
            ("Bengali", "হ্যালো বিশ্ব"),
            ("Gujarati", "નમસ્તે દુનિયા"),
            ("Telugu", "హలో ప్రపంచం"),
            ("Kannada", "ಹಲೋ ವಿಶ್ವ"),
            ("Malayalam", "ഹലോ വേൾഡ്"),
            ("Sinhala", "හෙලෝ වර්ල්ඩ්"),
            ("Thai", "สวัสดีชาวโลก"),
            ("Khmer", "សួស្តី"),
            ("Arabic", "مرحبا بالعالم"),
            ("Hebrew", "שלום עולם"),
            ("Georgian", "გამარჯობა"),
            ("Armenian", "Բարեւ աշխարհ"),
            ("Ethiopic", "ሰላም ልዑል"),
        ];

        for (name, text) in samples {
            let widths = glyph_widths(&ctx, text);
            assert!(
                distinct(&widths) >= 2,
                "{name} fell back to a single glyph width (tofu?): {widths:?}"
            );
        }
    }

    #[test]
    fn continues_numbered_lists() {
        // The editor has already inserted the newline after "1. Something".
        let mut text = "1. Something\n".to_owned();
        assert_eq!(continue_list(&mut text, 12), Some(16));
        assert_eq!(text, "1. Something\n2. ");
    }

    #[test]
    fn increments_the_list_number() {
        let mut text = "9. nine\n".to_owned();
        assert_eq!(continue_list(&mut text, 7), Some(12));
        assert_eq!(text, "9. nine\n10. ");
    }

    #[test]
    fn continues_bullet_lists() {
        let mut text = "- one\n".to_owned();
        assert_eq!(continue_list(&mut text, 5), Some(8));
        assert_eq!(text, "- one\n- ");
    }

    #[test]
    fn continues_quotes() {
        let mut text = "> hi\n".to_owned();
        assert_eq!(continue_list(&mut text, 4), Some(7));
        assert_eq!(text, "> hi\n> ");
    }

    #[test]
    fn enter_on_an_empty_item_ends_the_list() {
        // The editor has already added the newline after "- ".
        let mut text = "- \n".to_owned();
        assert_eq!(continue_list(&mut text, 2), Some(0));
        assert_eq!(text, "");
    }

    #[test]
    fn plain_lines_are_left_alone() {
        let mut text = "just prose\n".to_owned();
        assert_eq!(continue_list(&mut text, 10), None);
        assert_eq!(text, "just prose\n");
    }

    /// A synthetic text-input event, as a keystroke produces.
    fn text_event(text: &str) -> Vec<egui::Event> {
        vec![egui::Event::Text(text.to_owned())]
    }

    /// Put the caret at `caret` and focus the editor, as a click would.
    fn focus_editor_at(ctx: &egui::Context, caret: usize) {
        let id = Id::new(EDITOR_ID);
        let mut state = TextEditState::default();
        state
            .cursor
            .set_char_range(Some(CCursorRange::one(CCursor::new(caret))));
        state.store(ctx, id);
        ctx.memory_mut(|memory| memory.request_focus(id));
    }

    #[test]
    fn only_a_newline_counts_as_enter() {
        // `prev_len` is the byte length *before* this frame's edit.
        // Typing one ASCII character grows the text by one byte too…
        assert!(!inserted_one_newline("2. T", 3, 3));
        // …a multi-byte character grows it by more than one…
        assert!(!inserted_one_newline("2. \u{e9}", 3, 3));
        // …and two characters at once grows it by two.
        assert!(!inserted_one_newline("2. ab", 3, 3));
        // Only a real newline at the caret qualifies.
        assert!(inserted_one_newline("2. \n", 3, 3));
        assert!(!inserted_one_newline("2. \n", 3, 2));
    }

    /// Regression: typing inside a list must not spray markers everywhere.
    #[test]
    fn typing_after_a_marker_adds_no_more_markers() {
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, None);
        app.text = "1. Some stuff".to_owned();
        focus_editor_at(&ctx, 13);

        frame(&ctx, &mut app, Vec::new());
        frame(&ctx, &mut app, key(Key::Enter, Modifiers::NONE));
        assert_eq!(app.text, "1. Some stuff\n2. ");

        // Type a phrase one character per frame, the way a keypress arrives.
        for ch in "This is cool".chars() {
            frame(&ctx, &mut app, text_event(&ch.to_string()));
        }
        assert_eq!(app.text, "1. Some stuff\n2. This is cool");
    }

    /// End to end: pressing Enter in the editor continues a list.
    #[test]
    fn enter_continues_a_list_in_the_editor() {
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, None);
        app.text = "1. Something".to_owned();

        // Put the caret at the end of the line, and focus the editor.
        focus_editor_at(&ctx, 12);

        // A frame with no input, so the app learns where the caret is.
        frame(&ctx, &mut app, Vec::new());
        assert_eq!(app.text, "1. Something");

        // Then Enter.
        frame(&ctx, &mut app, key(Key::Enter, Modifiers::NONE));
        assert_eq!(app.text, "1. Something\n2. ");
    }

    /// Enter on the fresh, empty item ends the list without a stray blank line.
    #[test]
    fn enter_again_ends_the_list_in_the_editor() {
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, None);
        app.text = "1. one".to_owned();
        focus_editor_at(&ctx, 6);

        frame(&ctx, &mut app, Vec::new());
        frame(&ctx, &mut app, key(Key::Enter, Modifiers::NONE));
        assert_eq!(app.text, "1. one\n2. ");

        frame(&ctx, &mut app, key(Key::Enter, Modifiers::NONE));
        assert_eq!(app.text, "1. one\n");
    }

    /// Plain prose gets an ordinary newline, not a marker.
    #[test]
    fn enter_in_prose_adds_no_marker() {
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, None);
        app.text = "just prose".to_owned();

        focus_editor_at(&ctx, 10);

        frame(&ctx, &mut app, Vec::new());
        frame(&ctx, &mut app, key(Key::Enter, Modifiers::NONE));
        assert_eq!(app.text, "just prose\n");
    }

    /// The lazy CJK load fires for CJK documents and stays dormant otherwise.
    #[test]
    fn cjk_text_triggers_the_cjk_font_path() {
        let ctx = egui::Context::default();

        let mut latin = App::new(&ctx, None);
        latin.text = "வாழ்க வையகம் नमस्ते مرحبا".to_owned();
        frame(&ctx, &mut latin, Vec::new());
        assert!(
            !latin.cjk_attempted,
            "non-CJK text should not load a CJK font"
        );

        let mut cjk = App::new(&ctx, None);
        cjk.text = "日本語 and 한국어".to_owned();
        frame(&ctx, &mut cjk, Vec::new());
        assert!(cjk.cjk_attempted, "CJK text should attempt a CJK font");
    }

    /// The window lays out Tamil text without panicking.
    #[test]
    fn tamil_draws_in_the_editor() {
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, None);
        app.text = "# தலைப்பு\n\nவாழ்க வையகம்\n".to_owned();
        frame(&ctx, &mut app, Vec::new());
    }

    /// Drive the whole interface headlessly: this exercises the panels, the
    /// The preview server is started lazily and follows the document.
    #[test]
    fn the_preview_follows_the_document() {
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, None);
        app.text = "# Title".to_owned();

        // Starting directly avoids launching a browser from a test.
        if !app.ensure_preview() {
            eprintln!("cannot bind a loopback port here; skipping");
            return;
        }
        let url = app.preview.as_ref().expect("server").url().to_owned();
        assert!(url.starts_with("http://127.0.0.1:"), "{url}");
        assert_eq!(app.published, "# Title");

        // A frame picks up edits and republishes them.
        app.text = "# Title\n\nnew **text**".to_owned();
        frame(&ctx, &mut app, Vec::new());
        assert_eq!(app.published, "# Title\n\nnew **text**");

        // And the status bar says the preview is on.
        assert!(app.status.as_deref().unwrap_or_default().contains("Preview at"), "{:?}", app.status);
    }

    /// The converted HTML is what the server actually serves.
    #[test]
    fn the_server_serves_the_converted_html() {
        use std::io::{Read, Write};

        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, None);
        app.text = "# Title\n\nwith **bold** and $sqrt(x)$".to_owned();

        if !app.ensure_preview() {
            eprintln!("cannot bind a loopback port here; skipping");
            return;
        }
        let url = app.preview.as_ref().expect("server").url().to_owned();
        let address = url.trim_start_matches("http://").trim_end_matches('/');

        let mut stream = std::net::TcpStream::connect(address).expect("connect");
        write!(stream, "GET /content HTTP/1.1\r\nHost: localhost\r\n\r\n").expect("write");
        let mut response = String::new();
        stream.read_to_string(&mut response).expect("read");

        assert!(response.contains("X-Version: 1"), "{response}");
        assert!(response.contains("<h1>Title</h1>"), "{response}");
        assert!(response.contains("<strong>bold</strong>"), "{response}");
        // AsciiMath reaches the browser as MathML, converted in the app.
        assert!(response.contains("<msqrt><mi>x</mi></msqrt>"), "{response}");
    }

    /// Without a preview there is nothing to publish.
    #[test]
    fn nothing_is_published_until_the_preview_is_opened() {
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, None);
        app.text = "# Title".to_owned();
        frame(&ctx, &mut app, Vec::new());
        assert!(app.preview.is_none());
        assert!(app.published.is_empty());
    }

    /// The text egui drew this frame, so a test can see the interface.
    fn frame_text(ctx: &egui::Context, app: &mut App, events: Vec<egui::Event>) -> String {
        let mut output = ctx.run_ui(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ui| app.draw(ui),
        );
        output.textures_delta.clear();
        let mut text = String::new();
        for clipped in &output.shapes {
            collect_text(&clipped.shape, &mut text);
        }
        text
    }

    fn collect_text(shape: &egui::Shape, out: &mut String) {
        match shape {
            egui::Shape::Text(text) => {
                out.push_str(text.galley.text());
                out.push('\n');
            }
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect_text(shape, out);
                }
            }
            _ => {}
        }
    }

    /// The settings panel opens on the shortcut, shows what it needs to, lists
    /// the models once connected, and keeps the document out of the way.
    #[test]
    fn the_settings_panel_lists_the_models() {
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, None);
        app.text = "hello".to_owned();

        frame(&ctx, &mut app, Vec::new());
        frame(&ctx, &mut app, key(Key::Comma, Modifiers::COMMAND));
        assert!(app.modal == Modal::Settings, "Cmd+, should open settings");

        let text = frame_text(&ctx, &mut app, Vec::new());
        for expected in ["Settings", "Ollama", "URL", "API key", "Connect"] {
            assert!(text.contains(expected), "missing {expected} in:\n{text}");
        }

        // While the panel is up, the document must not take the keystrokes.
        frame(&ctx, &mut app, text_event("x"));
        assert_eq!(app.text, "hello", "settings should not edit the document");

        // Once connected the models are listed and one is selected.
        app.connection = Connection::Connected;
        app.models = vec!["llama3.2:latest".to_owned(), "qwen2.5-coder:7b".to_owned()];
        app.settings.model = "qwen2.5-coder:7b".to_owned();
        let text = frame_text(&ctx, &mut app, Vec::new());
        for expected in ["Connected", "llama3.2:latest", "qwen2.5-coder:7b"] {
            assert!(text.contains(expected), "missing {expected} in:\n{text}");
        }
    }

    /// A model chosen on an earlier run is shown before reconnecting.
    #[test]
    fn a_saved_model_is_shown_without_reconnecting() {
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, None);
        app.modal = Modal::Settings;
        app.settings.model = "llama3.2:latest".to_owned();
        assert!(app.models.is_empty(), "nothing has been fetched");

        // A window needs one pass to size itself before it draws its contents.
        frame(&ctx, &mut app, Vec::new());
        let text = frame_text(&ctx, &mut app, Vec::new());
        assert!(text.contains("llama3.2:latest"), "{text}");
        assert!(text.contains("press Connect"), "{text}");
    }

    /// A failed connection is reported rather than hanging or panicking.
    #[test]
    fn a_failed_connection_is_reported() {
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, None);
        // Nothing listens on port 1, so this refuses immediately.
        app.settings.url = "http://127.0.0.1:1".to_owned();

        app.start_connecting();
        assert!(matches!(app.connection, Connection::Connecting));

        for _ in 0..200 {
            app.poll_connection();
            if !matches!(app.connection, Connection::Connecting) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        match &app.connection {
            Connection::Failed(error) => assert!(!error.is_empty()),
            _ => panic!("expected a failure to be reported"),
        }
    }

    /// `TextEdit` with its custom layouter, the fonts and the panels.
    #[test]
    fn draws_a_frame_without_panicking() {
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, None);
        app.text = "# Hello\n\nsome **bold** and `code` text\n\n- one\n- two\n\n> quote".to_owned();

        frame(&ctx, &mut app, Vec::new());

        app.focus = true;
        app.typewriter = true;
        app.show_bar = false;
        frame(&ctx, &mut app, Vec::new());

        app.modal = Modal::Help;
        frame(&ctx, &mut app, Vec::new());
    }

    /// The help panel opens with Cmd+H, Ctrl+H or F1, and each of them also
    /// closes it again.
    #[test]
    fn help_shortcuts_all_toggle_help() {
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, None);

        let chords: [(&str, Vec<egui::Event>); 3] = [
            ("Cmd+H", key(Key::H, Modifiers::COMMAND)),
            // Also accepted, because macOS may take Cmd+H for its Hide item.
            ("Ctrl+H", key(Key::H, Modifiers::CTRL)),
            ("F1", key(Key::F1, Modifiers::NONE)),
        ];

        for (label, events) in chords {
            frame(&ctx, &mut app, events.clone());
            assert!(app.modal == Modal::Help, "{label} should open help");
            frame(&ctx, &mut app, events);
            assert!(app.modal == Modal::None, "{label} should close help");
        }
    }
}
