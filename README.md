# claw_type

A distraction-free Markdown editor — a native window on your desktop, and an
optional terminal front-end.

`claw_type` keeps only your words on screen: one narrow, centred writing column,
no toolbars, no menus. Markdown stays visible as you write — coloured, not
hidden — and a rendered preview is one keystroke away.

```sh
cargo run --release              # the desktop app
cargo run --release -- notes.md  # …opening a file
cargo run --release --bin tui    # the terminal version
```

## Two front-ends, one engine

Both front-ends share the same Markdown parser and colour palette, so they render
identically.

| Binary | What it is |
| --- | --- |
| `claw_type` (default) | A native window built with [`egui`/`eframe`](https://github.com/emilk/egui) |
| `tui` | A terminal editor built with [`ratatui`](https://github.com/ratatui/ratatui), for SSH or a plain console |

## Features

- **Centred writing column.** The text stays in a comfortable reading measure
  however wide the window is.
- **Focus mode** (`Cmd/Ctrl+F`). Every paragraph except the one you are in fades
  into the background.
- **Typewriter scrolling** (`Cmd/Ctrl+T`). Keeps the line you are editing
  vertically centred.
- **Live syntax colouring.** Headings, emphasis, code, quotes, lists and links
  are highlighted in place without hiding a single character of the source.
- **Rendered preview** (`Cmd/Ctrl+P`). A clean reading view with the Markdown
  resolved — headings sized, lists bulleted, code in monospace. Start typing and
  you are back in the editor.
- **Native file dialogs** for open and save, and you can drag a file onto the
  window to open it. `Cmd/Ctrl+S` saves straight to the current path.
- **A hideable status bar** (`Cmd/Ctrl+B`) showing the file, mode, word and
  character counts and the caret position — for a completely bare screen.
- **Guard rails.** Closing the window with unsaved changes, or quitting with
  `Cmd/Ctrl+Q`, asks before discarding work.
- **Non-ASCII text is measured properly** — CJK characters are counted by
  display width, not by byte length.

## Key bindings

| Key | Action |
| --- | --- |
| `Cmd/Ctrl+S` | Save (prompts for a name on first save) |
| `Cmd/Ctrl+O` | Open a file |
| `Cmd/Ctrl+N` | New file |
| `Cmd/Ctrl+Q` | Quit (confirms if there is unsaved work) |
| `Cmd/Ctrl+P` | Toggle the rendered preview |
| `Cmd/Ctrl+F` | Toggle focus mode |
| `Cmd/Ctrl+T` | Toggle typewriter scrolling |
| `Cmd/Ctrl+B` | Show or hide the status bar |
| `Cmd/Ctrl+Z` / `Shift+Cmd/Ctrl+Z` | Undo / redo |
| `Cmd/Ctrl+H` or `F1` | Shortcut help |
| `Esc` | Close a panel |

The terminal front-end has the same bindings (its status bar also toggles with
`F2`), plus soft wrapping with cursor movement by screen row, and
Markdown-aware `Enter` that continues lists and quotes.

## Design

The crate is a small library plus one binary per front-end:

| Path | Responsibility |
| --- | --- |
| `src/markdown.rs` | Parses Markdown into *semantically tagged* characters |
| `src/palette.rs` | The shared colours, stored as plain RGB |
| `src/buffer.rs` | The text model: lines, cursor, editing, undo history |
| `src/wrap.rs` | Display-width-aware soft wrapping and row mapping |
| `src/tui/` | The terminal front-end (app state, drawing, `ratatui` styling) |
| `src/main.rs` | The windowed front-end (layout jobs, shortcuts, dialogs) |

The key idea is that the Markdown parser emits, per character, a `Role`
(heading, code, marker, link, …) rather than a colour. Each front-end maps those
roles onto its own styling — `ratatui::style::Style` in one, `egui`'s
`TextFormat` in the other — from the same palette. Adding a third front-end
would only mean writing another mapping.

In the terminal front-end there is a further split between the *logical*
document (lines and `char` offsets) and the *visual* rows it is drawn as:
`wrap.rs` produces those rows and remembers which character each row starts at,
which is what lets the cursor sit correctly on a wrapped line.

## Tests

```sh
cargo test
```

The suite covers editing and undo, list continuation, wrapping (including wide
characters), the Markdown parser, headless frame rendering for the terminal via
Ratatui's `TestBackend`, and the windowed front-end's text layout (that the
highlighted layout is byte-for-byte identical to the source, so the caret never
drifts).
