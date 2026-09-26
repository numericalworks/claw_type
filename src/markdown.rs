//! A small, self-contained Markdown parser.
//!
//! It tags every character of the source with a *role* — heading, code, marker,
//! link, … — rather than a colour, so the editor can show the raw document with
//! syntax colour without the parser knowing anything about appearance. The
//! [`crate::palette`] maps roles to colours.
//!
//! The character-level block and inline scanners here are also reused by
//! [`crate::html`], which turns the same source into HTML for the browser
//! preview — one set of rules, so highlighting and preview cannot disagree.

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

/// The kind of a list marker.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ListKind {
    Bullet,
    Ordered(String),
}

/// Tag every character of a document, one `Vec` per logical line.
pub fn highlight(lines: &[String]) -> Vec<Vec<(char, MStyle)>> {
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
                out.push(fence_line(&chars));
            } else {
                out.push(styled(&chars, MStyle::code()));
            }
            continue;
        }

        if let Some((m, l)) = fence_of(&chars) {
            fence = Some((m, l));
            out.push(fence_line(&chars));
            continue;
        }

        if let Some(level) = heading_level(&chars) {
            let mut content = level;
            if chars.get(level) == Some(&' ') {
                content += 1;
            }
            let mut row = styled(&chars[..content], MStyle::heading_marker(level));
            row.extend(inline(&chars[content..], MStyle::heading(level)));
            out.push(row);
            continue;
        }

        if is_rule(&chars) {
            out.push(styled(&chars, MStyle::rule()));
            continue;
        }

        if let Some(prefix_len) = blockquote_prefix(&chars) {
            let mut row = styled(&chars[..prefix_len], MStyle::marker());
            row.extend(inline(&chars[prefix_len..], MStyle::quote()));
            out.push(row);
            continue;
        }

        if let Some((prefix_len, indent_len, _kind)) = list_prefix(&chars) {
            let mut row = styled(&chars[..indent_len], MStyle::text());
            row.extend(styled(&chars[indent_len..prefix_len], MStyle::list()));
            row.extend(inline(&chars[prefix_len..], MStyle::text()));
            out.push(row);
            continue;
        }

        out.push(inline(&chars, MStyle::text()));
    }

    out
}

fn fence_line(chars: &[char]) -> Vec<(char, MStyle)> {
    styled(chars, MStyle::fence())
}

fn styled(chars: &[char], style: MStyle) -> Vec<(char, MStyle)> {
    chars.iter().map(|&c| (c, style)).collect()
}

fn push_styled(out: &mut Vec<(char, MStyle)>, chars: &[char], style: MStyle) {
    out.extend(chars.iter().map(|&c| (c, style)));
}

/// Detect the opening or closing fence of a code block.
pub(crate) fn fence_of(chars: &[char]) -> Option<(char, usize)> {
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
pub(crate) fn heading_level(chars: &[char]) -> Option<usize> {
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
pub(crate) fn is_rule(chars: &[char]) -> bool {
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
pub(crate) fn blockquote_prefix(chars: &[char]) -> Option<usize> {
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
pub(crate) fn list_prefix(chars: &[char]) -> Option<(usize, usize, ListKind)> {
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

pub(crate) fn run_len(chars: &[char], from: usize, c: char) -> usize {
    let mut n = 0;
    while from + n < chars.len() && chars[from + n] == c {
        n += 1;
    }
    n
}

/// Find the next run of at least `len` `c`s starting at or after `from`.
pub(crate) fn find_run(chars: &[char], from: usize, c: char, len: usize) -> Option<usize> {
    let mut k = from;
    while k + len <= chars.len() {
        if chars[k..k + len].iter().all(|&x| x == c) {
            return Some(k);
        }
        k += 1;
    }
    None
}

pub(crate) fn find_char(chars: &[char], from: usize, c: char) -> Option<usize> {
    (from..chars.len()).find(|&k| chars[k] == c)
}

/// Whether `chars[i]` may open an emphasis span. Underscores are not allowed
/// to open *inside* a word, so identifiers such as `snake_case` are left alone.
pub(crate) fn can_open(chars: &[char], i: usize, c: char) -> bool {
    c != '_' || i == 0 || !chars[i - 1].is_alphanumeric()
}

/// Find a closing emphasis run that is allowed to close the span at `from`.
pub(crate) fn find_emph_close(chars: &[char], from: usize, c: char, len: usize) -> Option<usize> {
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

/// Tag inline markup within a single line, keeping every character.
pub(crate) fn inline(chars: &[char], base: MStyle) -> Vec<(char, MStyle)> {
    let n = chars.len();
    let mut out = Vec::with_capacity(n);
    let mut i = 0;

    while i < n {
        let c = chars[i];

        // Backslash escapes.
        if c == '\\' && i + 1 < n && chars[i + 1].is_ascii_punctuation() {
            out.push(('\\', MStyle::marker()));
            out.push((chars[i + 1], base));
            i += 2;
            continue;
        }

        // Code spans.
        if c == '`' {
            let run = run_len(chars, i, '`');
            if let Some(close) = find_run(chars, i + run, '`', run) {
                for _ in 0..run {
                    out.push(('`', MStyle::marker()));
                }
                push_styled(&mut out, &chars[i + run..close], MStyle::code());
                for _ in 0..run {
                    out.push(('`', MStyle::marker()));
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
            for _ in 0..2 {
                out.push((c, MStyle::marker()));
            }
            out.extend(inline(&chars[i + 2..close], base.with_bold()));
            for _ in 0..2 {
                out.push((c, MStyle::marker()));
            }
            i = close + 2;
            continue;
        }

        // Emphasis.
        if (c == '*' || c == '_')
            && can_open(chars, i, c)
            && let Some(close) = find_emph_close(chars, i + 1, c, 1)
        {
            out.push((c, MStyle::marker()));
            out.extend(inline(&chars[i + 1..close], base.with_italic()));
            out.push((c, MStyle::marker()));
            i = close + 1;
            continue;
        }

        // Strikethrough.
        if c == '~'
            && i + 1 < n
            && chars[i + 1] == '~'
            && let Some(close) = find_run(chars, i + 2, '~', 2)
        {
            for _ in 0..2 {
                out.push(('~', MStyle::marker()));
            }
            out.extend(inline(&chars[i + 2..close], base.with_strike()));
            for _ in 0..2 {
                out.push(('~', MStyle::marker()));
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
                if is_image {
                    out.push(('!', MStyle::marker()));
                }
                out.push(('[', MStyle::marker()));
                out.extend(inline(&chars[text_start..close], MStyle::link()));
                out.push((']', MStyle::marker()));
                out.push(('(', MStyle::marker()));
                push_styled(&mut out, &chars[close + 2..end], MStyle::url());
                out.push((')', MStyle::marker()));
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

    fn lines(input: &[&str]) -> Vec<String> {
        input.iter().map(|s| s.to_string()).collect()
    }

    fn text_of(line: &[(char, MStyle)]) -> String {
        line.iter().map(|(c, _)| *c).collect()
    }

    fn highlighted(input: &[&str]) -> Vec<Vec<(char, MStyle)>> {
        highlight(&lines(input))
    }

    /// The role of the first character equal to `needle`.
    fn role_in(input: &str, needle: char) -> Role {
        let row = &highlighted(&[input])[0];
        row.iter()
            .find(|(c, _)| *c == needle)
            .map(|(_, style)| style.role)
            .unwrap()
    }

    fn style_in(input: &str, needle: char) -> MStyle {
        let row = &highlighted(&[input])[0];
        row.iter().find(|(c, _)| *c == needle).unwrap().1
    }

    /// The invariant everything else depends on: highlighting never changes the
    /// text, so the caret can be positioned by character index.
    #[test]
    fn highlighting_preserves_the_source_exactly() {
        let input = [
            "# Title",
            "",
            "Some **bold**, *italic*, `code` and ~~struck~~ text.",
            "",
            "- a bullet",
            "1. a numbered item",
            "> a quote",
            "",
            "```rust",
            "let x = 1; // <code>",
            "```",
            "",
            "---",
            "",
            "[a link](https://example.com) and ![an image](pic.png)",
            r"an escaped \* star",
        ];
        let out = highlighted(&input);
        let rebuilt: Vec<String> = out.iter().map(|row| text_of(row)).collect();
        assert_eq!(rebuilt, input);
    }

    #[test]
    fn tags_headings_and_their_hashes() {
        assert_eq!(role_in("### Deep", '#'), Role::HeadingMarker(3));
        assert_eq!(role_in("### Deep", 'D'), Role::Heading(3));
    }

    #[test]
    fn tags_inline_markup() {
        assert_eq!(role_in("a **b** c", '*'), Role::Marker);
        assert_eq!(role_in("a **b** c", 'b'), Role::Text);
        assert_eq!(role_in("`x`", 'x'), Role::Code);
        assert_eq!(role_in("`x`", '`'), Role::Marker);
    }

    #[test]
    fn tags_blocks() {
        assert_eq!(role_in("> quoted", '>'), Role::Marker);
        assert_eq!(role_in("> quoted", 'q'), Role::Quote);
        assert_eq!(role_in("- item", '-'), Role::List);
        assert_eq!(role_in("- item", 'i'), Role::Text);
        assert_eq!(role_in("1. item", '1'), Role::List);
        assert_eq!(role_in("---", '-'), Role::Rule);
    }

    #[test]
    fn tags_fenced_code() {
        let out = highlighted(&["```", "let x = 1;", "```"]);
        assert_eq!(out[0][0].1.role, Role::Fence);
        assert!(out[1].iter().all(|(_, s)| s.role == Role::Code));
        assert_eq!(out[2][0].1.role, Role::Fence);
    }

    #[test]
    fn tags_links_and_their_urls() {
        let input = "[docs](http://x)";
        assert_eq!(role_in(input, '['), Role::Marker);
        assert_eq!(role_in(input, 'd'), Role::Link);
        assert_eq!(role_in(input, 'h'), Role::Url);
    }

    #[test]
    fn emphasis_flags_reach_the_characters() {
        assert!(style_in("**b**", 'b').bold);
        assert!(style_in("*i*", 'i').italic);
        assert!(style_in("~~s~~", 's').strike);
    }

    #[test]
    fn code_spans_are_literal_inside_emphasis() {
        // A code span takes the code role and does not inherit emphasis: what is
        // inside backticks is literal.
        let style = style_in("**bold `code`**", 'c');
        assert_eq!(style.role, Role::Code);
        assert!(!style.bold);
    }

    #[test]
    fn underscores_inside_words_are_literal() {
        assert_eq!(role_in("call snake_case_now", '_'), Role::Text);
    }

    #[test]
    fn unmatched_markers_are_literal() {
        let row = &highlighted(&["a * b"])[0];
        // Nothing is marked up, so every character is body text.
        assert!(row.iter().all(|(_, s)| s.role == Role::Text));
    }

    #[test]
    fn escaped_punctuation_is_literal() {
        let row = &highlighted(&[r"a \* b"])[0];
        let star = row.iter().find(|(c, _)| *c == '*').unwrap();
        assert_eq!(star.1.role, Role::Text);
    }
}
