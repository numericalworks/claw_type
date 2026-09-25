//! The terminal front-end.
//!
//! Kept as a second binary so the editor is still usable over SSH or in a
//! plain console. It shares the Markdown engine and palette with the windowed
//! front-end.

pub mod app;
pub mod style;
pub mod ui;

use std::io;
use std::path::PathBuf;

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event};

use app::App;

/// Run the editor until the user quits.
pub fn run(path: Option<PathBuf>) -> io::Result<()> {
    let mut terminal = ratatui::init();
    let mut app = App::new(path);
    let result = event_loop(&mut terminal, &mut app);
    ratatui::restore();
    result
}

fn event_loop(terminal: &mut DefaultTerminal, app: &mut App) -> io::Result<()> {
    while !app.should_quit {
        terminal.draw(|frame| ui::draw(frame, app))?;
        match event::read()? {
            Event::Key(key) => app.handle_key(key),
            Event::Resize(width, height) => app.viewport = (width, height),
            _ => {}
        }
    }
    Ok(())
}
