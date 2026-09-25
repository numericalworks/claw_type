//! A small, self-contained Markdown parser, shared by both front-ends.
//!
//! The parser does not know about colours. It emits a stream of
//! `(char, MStyle)` pairs, where `MStyle` describes the *role* of each
//! character (heading, code, marker, ...) rather than an appearance. Each
//! front-end maps those roles onto its own styling — [`crate::palette`]
//! provides the shared colours.
//!
//! It has two jobs:
//!
//! * [`Mode::Highlight`] keeps every character of the source and just tags the
//!   Markdown punctuation, so an editor can show the raw document with colour.
//! * [`Mode::Render`] strips the punctuation and applies real styling, for a
//!   rendered preview.

/// What a character *means* in the document.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum Role {
    /// Ordinary body text.
    #[default]
    Text,
    /// Markdown punctuation (`**`, `>`, `` ` ``, ...).
    Marker,
    /// A heading's text (`1`..=`6`).
    Heading(u8),
    /// The `#`s in front of a heading.
    HeadingMarker(u8),
    /// Inline code.
    Code,
    /// The ``` fences around a code block.
    Fence,
    /// Block quote text.
    Quote,
    /// List markers and numbers.
    List,
    /// Link text.
    Link,
    /// Link destinations.
    Url,
    /// Horizontal rules.
    Rule,
}

/// The semantic style of a single character.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct MStyle {
    pub role: Role,
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub underline: bool,
}

impl MStyle {
    const fn new(role: Role) -> Self {
        Self {
            role,
            bold: false,
            italic: false,
            strike: false,
            underline: false,
        }
    }

    pub const fn text() -> Self {
        Self::new(Role::Text)
    }
    pub const fn marker() -> Self {
        Self::new(Role::Marker)
    }
    pub const fn fence() -> Self {
        Self::new(Role::Fence)
    }
    pub const fn code() -> Self {
        Self::new(Role::Code)
    }
    pub const fn rule() -> Self {
        Self::new(Role::Rule)
    }
    pub const fn quote() -> Self {
        Self {
            role: Role::Quote,
            bold: false,
            italic: true,
            strike: false,
            underline: false,
        }
    }
    pub const fn list() -> Self {
        Self {
            role: Role::List,
            bold: true,
            italic: false,
            strike: false,
            underline: false,
        }
    }
    pub const fn link() -> Self {
        Self {
            role: Role::Link,
            bold: false,
            italic: false,
            strike: false,
            underline: true,
        }
    }
    pub const fn url() -> Self {
        Self {
            role: Role::Url,
            bold: false,
            italic: false,
            strike: false,
            underline: true,
        }
    }
    pub const fn heading(level: usize) -> Self {
        Self {
            role: Role::Heading(level as u8),
            bold: true,
            italic: false,
            strike: false,
            underline: false,
        }
    }
    pub const fn heading_marker(level: usize) -> Self {
        Self::new(Role::HeadingMarker(level as u8))
    }

    pub const fn with_bold(self) -> Self {
        Self { bold: true, ..self }
    }
    pub const fn with_italic(self) -> Self {
        Self { italic: true, ..self }
    }
    pub const fn with_strike(self) -> Self {
        Self { strike: true, ..self }
    }
}

/// How a document should be interpreted.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// Keep the source text, tag the syntax.
    Highlight,
    /// Drop the syntax and style the result.
    Render,
}

/// The kind of a list marker.
#[derive(Debug, PartialEq, Eq)]
enum ListKind {
    Bullet,
    Ordered(String),
}

/// Convert a document into tagged character sequences, one per logical line.
///
/// `width` is the width of the rendering column; it is used to size horizontal
/// rules in [`Mode::Render`].
pub fn transform(lines: &[String], mode: Mode, width: usize) -> Vec<Vec<(char, MStyle)>> {
    let mut out = Vec::with_capacity(lines.len());
    // Some `(marker, length)` while inside a fenced code block.
    let mut fence: Option<(char, usize)> = None;

    for line in lines {
        let chars: Vec<char> = line.chars().collect();

        if let Some((marker, len)) = fence {
            if let Some((m, l)) = fence_of(&chars)
                && m == marker
                && l >= len
            {
                fence = None;
                out.push(fence_line(&chars, mode));
            } else {
                out.push(match mode {
                    Mode::Highlight => styled(&chars, MStyle::code()),
                    Mode::Render => {
                        let mut row = styled(&['│', ' '], MStyle::rule());
                        row.extend(inline(&chars, MStyle::code(), mode));
                        row
                    }
                });
            }
            continue;
        }

        if let Some((m, l)) = fence_of(&chars) {
            fence = Some((m, l));
            out.push(fence_line(&chars, mode));
            continue;
        }

        if let Some(level) = heading_level(&chars) {
            let mut content = level;
            if chars.get(level) == Some(&' ') {
                content += 1;
            }
            out.push(match mode {
                Mode::Highlight => {
                    let mut row = styled(&chars[..content], MStyle::heading_marker(level));
                    row.extend(inline(&chars[content..], MStyle::heading(level), mode));
                    row
                }
                Mode::Render => inline(&chars[content..], MStyle::heading(level), mode),
            });
            continue;
        }

        if is_rule(&chars) {
            out.push(match mode {
                Mode::Highlight => styled(&chars, MStyle::rule()),
                Mode::Render => vec![('─', MStyle::rule()); width.max(1)],
            });
            continue;
        }

        if let Some(prefix_len) = blockquote_prefix(&chars) {
            let content = &chars[prefix_len..];
            out.push(match mode {
                Mode::Highlight => {
                    let mut row = styled(&chars[..prefix_len], MStyle::marker());
                    row.extend(inline(content, MStyle::quote(), mode));
                    row
                }
                Mode::Render => {
                    let mut row = styled(&['│', ' '], MStyle::quote());
                    row.extend(inline(content, MStyle::quote(), mode));
                    row
                }
            });
            continue;
        }

        if let Some((prefix_len, indent_len, kind)) = list_prefix(&chars) {
            let content = &chars[prefix_len..];
            let mut row = styled(&chars[..indent_len], MStyle::text());
            match mode {
                Mode::Highlight => {
                    row.extend(styled(&chars[indent_len..prefix_len], MStyle::list()));
                }
                Mode::Render => {
                    let bullet: Vec<char> = match &kind {
                        ListKind::Bullet => vec!['•', ' '],
                        ListKind::Ordered(number) => {
                            let mut b: Vec<char> = number.chars().collect();
                            b.push(' ');
                            b
                        }
                    };
                    row.extend(styled(&bullet, MStyle::list()));
                }
            }
            row.extend(inline(content, MStyle::text(), mode));
            out.push(row);
            continue;
        }

        out.push(inline(&chars, MStyle::text(), mode));
    }

    out
}

fn fence_line(chars: &[char], mode: Mode) -> Vec<(char, MStyle)> {
    match mode {
        Mode::Highlight => styled(chars, MStyle::fence()),
        // The fence itself carries no meaning once rendered.
        Mode::Render => Vec::new(),
    }
}

fn styled(chars: &[char], style: MStyle) -> Vec<(char, MStyle)> {
    chars.iter().map(|&c| (c, style)).collect()
}

fn push_styled(out: &mut Vec<(char, MStyle)>, chars: &[char], style: MStyle) {
    out.extend(chars.iter().map(|&c| (c, style)));
}

/// Detect the opening or closing fence of a code block.
fn fence_of(chars: &[char]) -> Option<(char, usize)> {
    let mut i = 0;
    while i < chars.len() && chars[i] == ' ' {
        i += 1;
    }
    let marker = *chars.get(i)?;
    if marker != '`' && marker != '~' {
        return None;
    }
    let mut len = 0;
    while i + len < chars.len() && chars[i + len] == marker {
        len += 1;
    }
    (len >= 3).then_some((marker, len))
}

/// ATX heading level (`#`..`######`), if the line is a heading.
fn heading_level(chars: &[char]) -> Option<usize> {
    let mut hashes = 0;
    while hashes < chars.len() && chars[hashes] == '#' {
        hashes += 1;
    }
    if hashes == 0 || hashes > 6 {
        return None;
    }
    match chars.get(hashes) {
        None | Some(' ') => Some(hashes),
        _ => None,
    }
}

/// A thematic break (`---`, `***`, `___`, ...).
fn is_rule(chars: &[char]) -> bool {
    let mut marker: Option<char> = None;
    let mut count = 0;
    for &c in chars {
        if c == ' ' {
            continue;
        }
        match marker {
            None => {
                if c == '-' || c == '*' || c == '_' {
                    marker = Some(c);
                    count = 1;
                } else {
                    return false;
                }
            }
            Some(m) if c == m => count += 1,
            Some(_) => return false,
        }
    }
    count >= 3
}

/// Length of a leading `>` quote marker, including indentation.
fn blockquote_prefix(chars: &[char]) -> Option<usize> {
    let mut i = 0;
    while i < chars.len() && chars[i] == ' ' {
        i += 1;
    }
    if chars.get(i) != Some(&'>') {
        return None;
    }
    i += 1;
    if chars.get(i) == Some(&' ') {
        i += 1;
    }
    Some(i)
}

/// `(prefix_len, indent_len, kind)` for a list item.
fn list_prefix(chars: &[char]) -> Option<(usize, usize, ListKind)> {
    let mut i = 0;
    while i < chars.len() && chars[i] == ' ' {
        i += 1;
    }
    let indent = i;

    if matches!(chars.get(i), Some('-' | '*' | '+')) && chars.get(i + 1) == Some(&' ') {
        return Some((i + 2, indent, ListKind::Bullet));
    }

    let start = i;
    while i < chars.len() && chars[i].is_ascii_digit() {
        i += 1;
    }
    if i > start && matches!(chars.get(i), Some('.' | ')')) && chars.get(i + 1) == Some(&' ') {
        let mut number: String = chars[start..i].iter().collect();
        number.push(chars[i]);
        return Some((i + 2, indent, ListKind::Ordered(number)));
    }

    None
}

fn run_len(chars: &[char], from: usize, c: char) -> usize {
    let mut n = 0;
    while from + n < chars.len() && chars[from + n] == c {
        n += 1;
    }
    n
}

/// Find the next run of at least `len` `c`s starting at or after `from`.
fn find_run(chars: &[char], from: usize, c: char, len: usize) -> Option<usize> {
    let mut k = from;
    while k + len <= chars.len() {
        if chars[k..k + len].iter().all(|&x| x == c) {
            return Some(k);
        }
        k += 1;
    }
    None
}

fn find_char(chars: &[char], from: usize, c: char) -> Option<usize> {
    (from..chars.len()).find(|&k| chars[k] == c)
}

/// Whether `chars[i]` may open an emphasis span. Underscores are not allowed
/// to open *inside* a word, so identifiers such as `snake_case` are left alone.
fn can_open(chars: &[char], i: usize, c: char) -> bool {
    c != '_' || i == 0 || !chars[i - 1].is_alphanumeric()
}

/// Find a closing emphasis run that is allowed to close the span at `from`.
fn find_emph_close(chars: &[char], from: usize, c: char, len: usize) -> Option<usize> {
    let mut k = from;
    while let Some(pos) = find_run(chars, k, c, len) {
        let closes_word = pos + len >= chars.len() || !chars[pos + len].is_alphanumeric();
        if c != '_' || closes_word {
            return Some(pos);
        }
        k = pos + len;
    }
    None
}

/// Parse inline markup within a single line.
fn inline(chars: &[char], base: MStyle, mode: Mode) -> Vec<(char, MStyle)> {
    let n = chars.len();
    let mut out = Vec::with_capacity(n);
    let mut i = 0;

    while i < n {
        let c = chars[i];

        // Backslash escapes.
        if c == '\\' && i + 1 < n && chars[i + 1].is_ascii_punctuation() {
            match mode {
                Mode::Highlight => {
                    out.push(('\\', MStyle::marker()));
                    out.push((chars[i + 1], base));
                }
                Mode::Render => out.push((chars[i + 1], base)),
            }
            i += 2;
            continue;
        }

        // Code spans.
        if c == '`' {
            let run = run_len(chars, i, '`');
            if let Some(close) = find_run(chars, i + run, '`', run) {
                let inner = &chars[i + run..close];
                match mode {
                    Mode::Highlight => {
                        for _ in 0..run {
                            out.push(('`', MStyle::marker()));
                        }
                        push_styled(&mut out, inner, MStyle::code());
                        for _ in 0..run {
                            out.push(('`', MStyle::marker()));
                        }
                    }
                    Mode::Render => push_styled(&mut out, inner, MStyle::code()),
                }
                i = close + run;
                continue;
            }
        }

        // Strong emphasis.
        if (c == '*' || c == '_')
            && i + 1 < n
            && chars[i + 1] == c
            && can_open(chars, i, c)
            && let Some(close) = find_emph_close(chars, i + 2, c, 2)
        {
            let inner = &chars[i + 2..close];
            let style = base.with_bold();
            match mode {
                Mode::Highlight => {
                    for _ in 0..2 {
                        out.push((c, MStyle::marker()));
                    }
                    out.extend(inline(inner, style, mode));
                    for _ in 0..2 {
                        out.push((c, MStyle::marker()));
                    }
                }
                Mode::Render => out.extend(inline(inner, style, mode)),
            }
            i = close + 2;
            continue;
        }

        // Emphasis.
        if (c == '*' || c == '_')
            && can_open(chars, i, c)
            && let Some(close) = find_emph_close(chars, i + 1, c, 1)
        {
            let inner = &chars[i + 1..close];
            let style = base.with_italic();
            match mode {
                Mode::Highlight => {
                    out.push((c, MStyle::marker()));
                    out.extend(inline(inner, style, mode));
                    out.push((c, MStyle::marker()));
                }
                Mode::Render => out.extend(inline(inner, style, mode)),
            }
            i = close + 1;
            continue;
        }

        // Strikethrough.
        if c == '~'
            && i + 1 < n
            && chars[i + 1] == '~'
            && let Some(close) = find_run(chars, i + 2, '~', 2)
        {
            let inner = &chars[i + 2..close];
            let style = base.with_strike();
            match mode {
                Mode::Highlight => {
                    for _ in 0..2 {
                        out.push(('~', MStyle::marker()));
                    }
                    out.extend(inline(inner, style, mode));
                    for _ in 0..2 {
                        out.push(('~', MStyle::marker()));
                    }
                }
                Mode::Render => out.extend(inline(inner, style, mode)),
            }
            i = close + 2;
            continue;
        }

        // Links and images.
        let is_image = c == '!' && chars.get(i + 1) == Some(&'[');
        if c == '[' || is_image {
            let text_start = if is_image { i + 2 } else { i + 1 };
            if let Some(close) = find_char(chars, text_start, ']')
                && chars.get(close + 1) == Some(&'(')
                && let Some(end) = find_char(chars, close + 2, ')')
            {
                let text = &chars[text_start..close];
                let url = &chars[close + 2..end];
                match mode {
                    Mode::Highlight => {
                        if is_image {
                            out.push(('!', MStyle::marker()));
                        }
                        out.push(('[', MStyle::marker()));
                        out.extend(inline(text, MStyle::link(), mode));
                        out.push((']', MStyle::marker()));
                        out.push(('(', MStyle::marker()));
                        push_styled(&mut out, url, MStyle::url());
                        out.push((')', MStyle::marker()));
                    }
                    Mode::Render => {
                        out.extend(inline(text, MStyle::link(), mode));
                    }
                }
                i = end + 1;
                continue;
            }
        }

        out.push((c, base));
        i += 1;
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(line: &[(char, MStyle)]) -> String {
        line.iter().map(|(c, _)| *c).collect()
    }

    fn lines(input: &[&str]) -> Vec<String> {
        input.iter().map(|s| s.to_string()).collect()
    }

    fn render(input: &[&str]) -> Vec<String> {
        transform(&lines(input), Mode::Render, 10)
            .iter()
            .map(|l| text_of(l))
            .collect()
    }

    fn highlight(input: &[&str]) -> Vec<String> {
        transform(&lines(input), Mode::Highlight, 10)
            .iter()
            .map(|l| text_of(l))
            .collect()
    }

    fn role_of(input: &str, needle: char) -> Role {
        let out = transform(&lines(&[input]), Mode::Render, 10);
        out[0]
            .iter()
            .find(|(c, _)| *c == needle)
            .map(|(_, s)| s.role)
            .unwrap()
    }

    #[test]
    fn highlight_preserves_source() {
        let input = ["# Title", "some **bold** text", "- item"];
        assert_eq!(highlight(&input), input);
    }

    #[test]
    fn render_strips_heading_hashes() {
        assert_eq!(render(&["### Deep"]), vec!["Deep"]);
    }

    #[test]
    fn render_strips_inline_markers() {
        assert_eq!(render(&["a **b** c"]), vec!["a b c"]);
        assert_eq!(render(&["*i* and `code`"]), vec!["i and code"]);
        assert_eq!(render(&["~~gone~~"]), vec!["gone"]);
    }

    #[test]
    fn render_keeps_link_text_drops_url() {
        assert_eq!(render(&["see [docs](http://x)"]), vec!["see docs"]);
    }

    #[test]
    fn render_turns_bullets_into_dots() {
        assert_eq!(render(&["- a", "* b", "+ c"]), vec!["• a", "• b", "• c"]);
    }

    #[test]
    fn render_keeps_ordered_numbers() {
        assert_eq!(render(&["3. three"]), vec!["3. three"]);
        assert_eq!(render(&["1) one"]), vec!["1) one"]);
    }

    #[test]
    fn render_quotes_with_a_bar() {
        assert_eq!(render(&["> hi"]), vec!["│ hi"]);
    }

    #[test]
    fn rules_span_the_column() {
        let out = render(&["---"]);
        assert_eq!(out[0].chars().count(), 10);
    }

    #[test]
    fn code_fence_drops_its_delimiters() {
        let out = render(&["```", "let x = 1;", "```"]);
        assert_eq!(out, vec!["", "│ let x = 1;", ""]);
    }

    #[test]
    fn unmatched_markers_render_literally() {
        assert_eq!(render(&["a * b"]), vec!["a * b"]);
    }

    #[test]
    fn underscores_inside_words_are_literal() {
        assert_eq!(render(&["call snake_case_now"]), vec!["call snake_case_now"]);
    }

    #[test]
    fn roles_describe_the_syntax() {
        assert_eq!(role_of("# hi", 'h'), Role::Heading(1));
        assert_eq!(role_of("`x`", 'x'), Role::Code);
        assert_eq!(role_of("> q", 'q'), Role::Quote);
        assert_eq!(role_of("- i", 'i'), Role::Text);
        assert_eq!(role_of("- i", '•'), Role::List);
    }

    #[test]
    fn emphasis_flags_reach_the_characters() {
        let out = transform(&lines(&["**b**"]), Mode::Render, 10);
        assert!(out[0][0].1.bold);
        let out = transform(&lines(&["*i*"]), Mode::Render, 10);
        assert!(out[0][0].1.italic);
    }
}
