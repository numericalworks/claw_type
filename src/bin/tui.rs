//! Terminal front-end entry point (`cargo run --bin tui`).

use std::io;
use std::path::PathBuf;

const HELP: &str = "\
claw_type-tui — a distraction-free Markdown editor for the terminal

USAGE:
    tui [FILE]

ARGS:
    FILE    Markdown file to open (created on first save if missing)

OPTIONS:
    -h, --help       Print this help
    -V, --version    Print the version

Run the editor and press F1 for the full list of key bindings.
";

fn main() -> io::Result<()> {
    let mut path: Option<PathBuf> = None;

    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "-h" | "--help" => {
                print!("{HELP}");
                return Ok(());
            }
            "-V" | "--version" => {
                println!("claw_type-tui {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            other if other.starts_with('-') => {
                eprintln!("unknown option: {other}\n");
                eprint!("{HELP}");
                std::process::exit(2);
            }
            other => path = Some(PathBuf::from(other)),
        }
    }

    claw_type::tui::run(path)
}
