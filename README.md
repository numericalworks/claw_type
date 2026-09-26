# claw_type

A distraction-free Markdown editor for the desktop.

`claw_type` keeps only your words on screen: one narrow, centred writing column,
no toolbars, no menus. Markdown stays visible as you write — coloured, not
hidden — and a rendered preview is one keystroke away.

```sh
cargo run --release              # the editor
cargo run --release -- notes.md  # …opening a file
```

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
- **Markdown-aware `Enter`.** Pressing Enter inside a bullet, numbered list or
  block quote carries the marker onto the next line (numbering increments), and
  an empty item drops its marker to end the list.
- **Guard rails.** Closing the window with unsaved changes, or quitting with
  `Cmd/Ctrl+Q`, asks before discarding work.
- **Unicode throughout.** Text is edited by character rather than by byte, and
  non-Latin scripts render properly (see *Scripts and fonts*).

## Scripts and fonts

egui's bundled fonts cover Latin, Cyrillic, Greek and emoji, but no Indic,
Arabic, Hebrew or CJK scripts — without help those characters fall back to
`.notdef` and show as empty boxes (tofu). `claw_type` fixes that in two tiers:

**Bundled.** ~5 MB of **Noto Sans** covers every major script *except* CJK —
Tamil, Devanagari, Bengali, Gurmukhi, Gujarati, Odia, Telugu, Kannada,
Malayalam, Sinhala, Thai, Lao, Khmer, Myanmar, Tibetan, Arabic, Hebrew,
Ethiopic, Georgian, Armenian, Thaana, Syriac, N'Ko, Adlam, Cherokee, Canadian
Aboriginal, Mongolian, and symbols. These are compiled into the binary with
`include_bytes!` (`assets/fonts/`), so they work on any machine with no
installation step, offline, and with no runtime cost beyond the file size.

**System, lazily.** CJK fonts run to 50–70 MB, so they are *not* bundled.
`claw_type` reads one from the system the first time a document actually
contains CJK text (Han, Hiragana, Katakana, Hangul), and never if it doesn't.

In both cases the Noto fonts are *appended* to each font family, so Latin text
still renders with the built-in fonts and they only pick up what is missing.
egui 0.36 shapes text with HarfBuzz (via `harfrust`), so this is enough for
correct rendering — conjuncts, the split `ை`/`ி` vowel signs, and mark
positioning such as the `்` pulli all come out right, not just the base letters.

To add a script, drop its font in `assets/fonts/` and add a line to `BUNDLED` in
`src/fonts.rs`; to change which CJK font is preferred, edit `CJK_CANDIDATES`.
The bundled fonts are from <https://github.com/notofonts/noto-fonts>, licensed
under the SIL Open Font License 1.1 (`assets/fonts/LICENSE-OFL.txt`).

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

## Design

The crate is a single binary with small, focused modules:

| Path | Responsibility |
| --- | --- |
| `src/main.rs` | The window: layout jobs, shortcuts, panels, file dialogs |
| `src/markdown.rs` | Parses Markdown into *semantically tagged* characters |
| `src/palette.rs` | The colour palette, stored as plain RGB |
| `src/lists.rs` | Bullet / numbered / quote continuation on Enter |
| `src/fonts.rs` | Bundled Noto Sans, plus lazy loading of a system CJK font |

The key idea is that the Markdown parser emits, per character, a `Role`
(heading, code, marker, link, …) rather than a colour; the window maps those
roles onto `egui`'s `TextFormat` using the shared palette. Adding another
front-end would mean writing another mapping, not another parser.

## Tests

```sh
cargo test
```

The suite covers the Markdown parser, list continuation, the bundled fonts and
their script coverage, and the editor itself — including headless frames driven
through an `egui::Context` (that the highlighted layout is byte-for-byte
identical to the source, so the caret never drifts, and that pressing Enter
continues a list while ordinary typing does not).
