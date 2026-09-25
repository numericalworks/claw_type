//! `claw_type` — a distraction-free Markdown editor.
//!
//! The crate is split into a front-end-neutral core and one module per
//! front-end:
//!
//! * [`markdown`] parses Markdown into semantically tagged characters.
//! * [`palette`] holds the shared colours.
//! * [`buffer`] and [`wrap`] are the text model and soft-wrapping helpers.
//! * [`tui`] is the terminal front-end (binary `tui`).
//!
//! The windowed front-end lives in `src/main.rs`.

pub mod buffer;
pub mod markdown;
pub mod palette;
pub mod tui;
pub mod wrap;
