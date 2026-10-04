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
- **Focus mode** (`Shift+Cmd/Ctrl+F`). Every paragraph except the one you are in
  fades into the background.
- **Typewriter scrolling** (`Cmd/Ctrl+T`). Keeps the line you are editing
  vertically centred.
- **Find and replace** (`Cmd/Ctrl+F`). A bar along the bottom: type what to
  look for and press **Search** (or Enter) to find it — nothing is searched
  until you ask. Every match is tinted in the text and you step through them
  with Previous/Next; you can replace one match or all of them at once, and
  there is an optional *Match case* (otherwise ASCII case is ignored). Changing
  the query clears the results until you press Search again; editing the
  document keeps them up to date. `Esc` closes it.
- **Live syntax colouring.** Headings, emphasis, code, quotes, lists and links
  are highlighted in place without hiding a single character of the source.
- **Browser preview** (`Cmd/Ctrl+P`). Starts a local server, opens your
  browser, and follows the buffer as you type. It is the full reading view —
  real headings, lists, quotes, syntax-highlighted code, links and images —
  because a browser renders HTML properly. The page has its own dark/light
  theme.
- **AsciiMath in the preview** (`Cmd/Ctrl+P`). `$inline$` and `$$display$$`
  maths is converted to MathML and drawn by the browser itself, in both themes —
  see [*Maths*](#maths).
- **Ollama settings** (`Cmd/Ctrl+,`). Point the app at a local or hosted
  Ollama, connect, and pick the model to use — see [*Ollama*](#ollama).
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
| `Cmd/Ctrl+P` | Open the preview in your browser |
| `Cmd/Ctrl+I` | AI: explain, summarise, rephrase, proofread |
| `Cmd/Ctrl+F` | Find and replace |
| `Shift+Cmd/Ctrl+F` | Toggle focus mode |
| `Cmd/Ctrl+T` | Toggle typewriter scrolling |
| `Cmd/Ctrl+B` | Show or hide the status bar |
| `Cmd/Ctrl+Z` / `Shift+Cmd/Ctrl+Z` | Undo / redo |
| `Cmd/Ctrl+H` or `F1` | Shortcut help |
| `Cmd/Ctrl+,` | Settings for Ollama |
| `Esc` | Close a panel |

In the preview page, press `d` to switch between dark and light (see
[*Dark and light*](#dark-and-light)).

## Design

The crate is a single binary with small, focused modules:

| Path | Responsibility |
| --- | --- |
| `src/main.rs` | The window: layout jobs, shortcuts, panels, file dialogs |
| `src/markdown.rs` | Parses Markdown into *semantically tagged* characters |
| `src/html.rs` | Turns the same parse into HTML for the preview |
| `src/math.rs` | Converts AsciiMath into MathML |
| `src/preview.rs` | The local preview server, page template and theme toggle |
| `src/settings.rs` | Where Ollama is, and which model to use |
| `src/ollama.rs` | Asking an Ollama server what models it has |
| `src/palette.rs` | The colour palette, stored as plain RGB |
| `src/lists.rs` | Bullet / numbered / quote continuation on Enter |
| `src/fonts.rs` | Bundled Noto Sans, plus lazy loading of a system CJK font |

The key idea is that the Markdown parser emits, per character, a `Role`
(heading, code, marker, link, …) rather than a colour; the window maps those
roles onto `egui`'s `TextFormat` using the shared palette. Adding another
front-end would mean writing another mapping, not another parser.

## Preview

`Cmd/Ctrl+P` starts a small HTTP server on an ephemeral **loopback** port
(`127.0.0.1` only — it is never reachable from the network) and opens your
default browser at it. The page polls a few times a second, so edits appear as
you type; pressing the shortcut again re-opens the page.

The Markdown is converted to HTML in the app, not in the browser, and the
conversion reuses the parser's own block and inline scanners — so the preview
and the editor's syntax colouring can never disagree about what is a heading, a
list or a link.

### Dark and light

The preview page has its own theme, independent of the editor window, which
stays dark.

- **Press `d`** — click the page first so it has keyboard focus. Caps Lock and
  Shift are fine, so `D` works too.
- **Or click the pill** in the bottom-right corner, which also names the
  shortcut and says which theme it would switch you to.

Your choice is remembered (`localStorage`), so the page reopens the way you left
it. Both themes are generated from the app's palette, so the preview matches the
editor either way.

### Code highlighting

Fenced code blocks are highlighted by [highlight.js](https://highlightjs.org)
11.10.0, **vendored into the binary** (`assets/highlight/`, BSD-3-Clause) and
served from the same local server — so the preview needs no network access and
trusts no CDN at runtime. It is the "common languages" bundle, which covers
around 40 languages including Rust, Python, JavaScript/TypeScript, Go, C/C++,
Java, Ruby, shell, JSON, YAML, SQL, HTML/CSS and Markdown; a fence with no
language is auto-detected.

The token colours are **not** highlight.js's own theme: they are mapped onto the
app's palette (`Theme::syntax`), so code sits in the same palette as the rest of
the page and works in both light and dark. Licence:
`assets/highlight/LICENSE.txt`.

### Maths

Mathematics is written in [AsciiMath](https://asciimath.org) — plain ASCII, no
backslashes — and converted to **MathML** *in the app*, alongside the Markdown.
Every current browser renders MathML natively, so there is no maths JavaScript
and nothing is fetched at runtime.

**Inline**, in the middle of a sentence:

```md
Euler's identity is $e^(i pi) + 1 = 0$.
```

**Display**, opening and closing on their own lines:

```md
$$
sum_(i=1)^n i^2 = (n(n+1)(2n+1))/6
$$
```

A display block opens with a line that starts with `$$` (indentation is
allowed) and closes with the next such line. If the opening line also closes on
the same line, it is a one-line block:

```md
$$x = (-b +- sqrt(b^2 - 4ac))/(2a)$$
```

Dollars that are not maths are left as prose — *"costs $5 and $10"* stays text
— under the usual rules: an opening `$` must not be followed by a space, a
closing `$` must not be preceded by one, and a closing `$` must not be followed
by a digit. Write `\$` for a literal dollar. Inside code spans and code blocks,
`$` is never maths.

What the notation covers:

| Written | Renders as |
| --- | --- |
| `x^2`, `x_1`, `x_1^2` | powers and subscripts |
| `a/b`, `(a+b)/(c-d)` | fractions |
| `sqrt(x)`, `root(3)(x)` | roots |
| `sum_(i=1)^n`, `prod_(i=1)^n`, `lim_(x->0)` | limits above and below |
| `int_0^1`, `oint_C` | integrals, limits to the side |
| `alpha beta Gamma` | Greek by name |
| `RR NN ZZ QQ CC` | number sets |
| `<= >= != ~= -> <-> in !in sub uu nn` | relations and arrows |
| `vec v hat x bar y dot z ul u` | accents |
| `abs(x) floor(x) ceil(x) \|x\|` | bars and fences |
| `text(any text)` | upright text |

For example:

```md
vec v = (d x)/dt,  hat x,  RR^2,  alpha <= beta,  f: A -> B
```

Maths is set from the page's own fonts, so it follows the dark/light toggle and
needs nothing installed; in the editor, maths is tinted with its own colour
(`Theme::math`).

### Raw HTML

HTML in the document is passed through to the page as it stands, so embeds render
rather than showing up as their own text:

```html
<iframe width="1280" height="720" src="https://example.com/video?embedplayer=true"></iframe>
```

A line that opens a block-level tag (`<iframe>`, `<video>`, `<div>`, …) is a raw
HTML block, running to the next blank line; the tags allowed to do that are the
usual CommonMark set. HTML inside a line — `<br>`, `<span>`, an `<a>` — is passed
through too. Because it goes into the page verbatim, preview only documents you
trust; ordinary text, code and `&` are still escaped as before.

### Limitations

It is deliberately not a complete CommonMark implementation. Tables, footnotes
and reference links are not supported.

The maths is a small implementation of AsciiMath rather than a complete one. It
covers what is listed above; matrices such as `((a,b),(c,d))` and the font
commands (`bb`, `cc`, `fr`, …) are not supported and come out as plain text.
Unknown input is never an error — it just renders as ordinary letters.

## Ollama

`Cmd/Ctrl+,` opens **Settings**, where the app is pointed at an
[Ollama](https://ollama.com) server and a model is chosen for the AI features.

| Field | Meaning |
| --- | --- |
| URL | Where the server is. Defaults to `http://localhost:11434`, the address a local Ollama listens on. A missing scheme is filled in, so `localhost:11434` works as it is. |
| API key | Only for a hosted server, sent as `Authorization: Bearer …`. Leave it empty for a local one. |

**Connect** asks the server for its models (`/api/tags`) and lists them underneath;
click one to choose it. The request runs on a background thread, so the window
keeps drawing while it is in flight, and a wrong address or a bad key is
reported in the panel rather than hanging.

The URL, the API key and the chosen model are **kept between runs** — there is no
Save button to remember. They are written when you close the panel (`Done` or
`Esc`), when you click a model, and when you quit, and read back at startup. The
panel also names the chosen model before you connect, so you can see it is still
there. To change server, type a new URL and press Connect again.

What you enter is written to a small file, so it survives a restart:

| Platform | Location |
| --- | --- |
| macOS | `~/Library/Application Support/claw_type/settings.conf` |
| Linux | `$XDG_CONFIG_HOME/claw_type/settings.conf`, or `~/.config/…` |
| Windows | `%APPDATA%\claw_type\settings.conf` |

Set `CLAW_TYPE_CONFIG` to put it somewhere else. On Unix the file is created
readable only by its owner, but note that the API key is stored in plain text —
treat it as you would any credential on disk.

## Tests

```sh
cargo test
```

The suite covers the Markdown parser, list continuation, the bundled fonts and
their script coverage, the Markdown→HTML conversion, the AsciiMath→MathML
conversion (whose output is checked to be well-formed XML), and the editor
itself — including headless frames driven through an `egui::Context` (that the
highlighted layout is byte-for-byte identical to the source, so the caret never
drifts, and that pressing Enter continues a list while ordinary typing does
not). The preview server is tested by starting it on a loopback port and making
real HTTP requests against it, and the Ollama client by pointing it at a
throwaway server that answers with a canned reply.
