//! Markdown → HTML, for the browser preview.
//!
//! Rather than re-implementing Markdown, this reuses the character-level block
//! and inline scanners from [`crate::markdown`], so the preview and the editor's
//! syntax colouring always agree about what is a heading, a list or a link.
//!
//! [`body`] returns just the `<article>` contents; the page around it (styling,
//! dark/light toggle, live updating) is the server's job, in
//! [`crate::preview`].

use crate::markdown::{self, ListKind};
use crate::math;

/// Convert a Markdown document into the body of an HTML page.
pub fn body(markdown: &str) -> String {
    let lines: Vec<Vec<char>> = markdown
        .split('\n')
        .map(|line| line.chars().collect())
        .collect();
    let mut out = String::new();
    blocks(&lines, &mut out);
    out
}

fn blocks(lines: &[Vec<char>], out: &mut String) {
    let mut i = 0;
    while i < lines.len() {
        let line = &lines[i];

        // Fenced code.
        if let Some((marker, len)) = markdown::fence_of(line) {
            let info = fence_info(line, len);
            let mut code = String::new();
            i += 1;
            while i < lines.len() {
                if let Some((m, l)) = markdown::fence_of(&lines[i])
                    && m == marker
                    && l >= len
                {
                    i += 1;
                    break;
                }
                code.extend(lines[i].iter());
                code.push('\n');
                i += 1;
            }
            out.push_str("<pre><code");
            if !info.is_empty() {
                out.push_str(" class=\"language-");
                push_escaped_all(out, &info);
                out.push('"');
            }
            out.push('>');
            push_escaped_all(out, &code);
            out.push_str("</code></pre>\n");
            continue;
        }

        if is_blank(line) {
            i += 1;
            continue;
        }

        // Display maths: handed to the browser as `\[...\]` for MathJax.
        if let Some(after) = markdown::math_fence(line) {
            let tex = if let Some(content) = markdown::math_fence_content(line, after) {
                // A complete `$$...$$` on one line.
                let tex: String = content.iter().collect();
                i += 1;
                tex
            } else {
                // Otherwise it runs to the closing `$$`.
                let mut parts: Vec<String> = vec![line[after..].iter().collect()];
                i += 1;
                while i < lines.len() {
                    if let Some(after) = markdown::math_fence(&lines[i]) {
                        parts.push(lines[i][after..].iter().collect());
                        i += 1;
                        break;
                    }
                    parts.push(lines[i].iter().collect());
                    i += 1;
                }
                parts.join("\n")
            };
            out.push_str("<div class=\"math-block\">");
            out.push_str(&math::to_mathml(tex.trim(), true));
            out.push_str("</div>\n");
            continue;
        }

        if let Some(level) = markdown::heading_level(line) {
            let mut start = level;
            if line.get(level) == Some(&' ') {
                start += 1;
            }
            out.push_str(&format!("<h{level}>"));
            out.push_str(&inline(&line[start..]));
            out.push_str(&format!("</h{level}>\n"));
            i += 1;
            continue;
        }

        if markdown::is_rule(line) {
            out.push_str("<hr>\n");
            i += 1;
            continue;
        }

        // Block quote: gather the quoted lines, strip one marker level, and
        // render them as a document in their own right (so quotes nest).
        if markdown::blockquote_prefix(line).is_some() {
            let mut inner = Vec::new();
            while i < lines.len() {
                match markdown::blockquote_prefix(&lines[i]) {
                    Some(prefix) => {
                        inner.push(lines[i][prefix..].to_vec());
                        i += 1;
                    }
                    None => break,
                }
            }
            out.push_str("<blockquote>\n");
            blocks(&inner, out);
            out.push_str("</blockquote>\n");
            continue;
        }

        if let Some((prefix_len, indent_len, kind)) = markdown::list_prefix(line) {
            list(lines, &mut i, out, prefix_len, indent_len, kind);
            continue;
        }

        // Raw HTML block: hand it to the browser as it stands, so embeds such as
        // `<iframe>` render instead of showing up as their own text.
        if html_block_start(line) {
            while i < lines.len() && !is_blank(&lines[i]) {
                out.extend(lines[i].iter());
                out.push('\n');
                i += 1;
            }
            continue;
        }

        // Paragraph: runs until a blank line or the start of another block.
        let mut paragraph = Vec::new();
        while i < lines.len() {
            let next = &lines[i];
            if is_blank(next) || (!paragraph.is_empty() && starts_block(next)) {
                break;
            }
            paragraph.push(next.clone());
            i += 1;
        }
        out.push_str("<p>");
        for (n, line) in paragraph.iter().enumerate() {
            if n > 0 {
                out.push('\n');
            }
            out.push_str(&inline(line));
        }
        out.push_str("</p>\n");
    }
}

/// Render one list (and anything nested inside its items).
fn list(
    lines: &[Vec<char>],
    i: &mut usize,
    out: &mut String,
    prefix_len: usize,
    indent: usize,
    kind: ListKind,
) {
    let ordered = matches!(kind, ListKind::Ordered(_));
    let tag = if ordered { "ol" } else { "ul" };

    out.push('<');
    out.push_str(tag);
    if let ListKind::Ordered(start) = &kind {
        let start = ordered_start(start);
        if start != 1 {
            out.push_str(&format!(" start=\"{start}\""));
        }
    }
    out.push_str(">\n");

    while *i < lines.len() {
        let Some((item_prefix, item_indent, item_kind)) = markdown::list_prefix(&lines[*i]) else {
            break;
        };
        if item_indent != indent || matches!(item_kind, ListKind::Ordered(_)) != ordered {
            break;
        }

        out.push_str("<li>");
        out.push_str(&inline(&lines[*i][item_prefix..]));
        *i += 1;

        // Gather the lines that belong to this item: anything indented deeper.
        let mut nested = Vec::new();
        while *i < lines.len() {
            let line = &lines[*i];
            if is_blank(line) {
                break;
            }
            let leading = line.iter().take_while(|c| **c == ' ').count();
            if leading <= indent {
                break;
            }
            // Drop the parent item's marker width so the nesting is preserved
            // relative to the item's text.
            nested.push(line[leading.min(prefix_len)..].to_vec());
            *i += 1;
        }
        if !nested.is_empty() {
            out.push('\n');
            blocks(&nested, out);
        }
        out.push_str("</li>\n");
    }

    out.push_str("</");
    out.push_str(tag);
    out.push_str(">\n");
}

/// Convert inline markup within one line.
fn inline(chars: &[char]) -> String {
    let n = chars.len();
    let mut out = String::with_capacity(n);
    let mut i = 0;

    while i < n {
        let c = chars[i];

        // Backslash escapes.
        if c == '\\' && i + 1 < n && chars[i + 1].is_ascii_punctuation() {
            push_escaped(&mut out, chars[i + 1]);
            i += 2;
            continue;
        }

        // Code spans.
        if c == '`' {
            let run = markdown::run_len(chars, i, '`');
            if let Some(close) = markdown::find_run(chars, i + run, '`', run) {
                out.push_str("<code>");
                push_escaped_all(&mut out, &chars[i + run..close].iter().collect::<String>());
                out.push_str("</code>");
                i = close + run;
                continue;
            }
        }

        // Inline maths, converted to MathML here in the app.
        if c == '$'
            && let Some(close) = markdown::find_math_close(chars, i)
        {
            let asciimath: String = chars[i + 1..close].iter().collect();
            out.push_str(&math::to_mathml(asciimath.trim(), false));
            i = close + 1;
            continue;
        }

        // Strong emphasis.
        if (c == '*' || c == '_')
            && i + 1 < n
            && chars[i + 1] == c
            && markdown::can_open(chars, i, c)
            && let Some(close) = markdown::find_emph_close(chars, i + 2, c, 2)
        {
            out.push_str("<strong>");
            out.push_str(&inline(&chars[i + 2..close]));
            out.push_str("</strong>");
            i = close + 2;
            continue;
        }

        // Emphasis.
        if (c == '*' || c == '_')
            && markdown::can_open(chars, i, c)
            && let Some(close) = markdown::find_emph_close(chars, i + 1, c, 1)
        {
            out.push_str("<em>");
            out.push_str(&inline(&chars[i + 1..close]));
            out.push_str("</em>");
            i = close + 1;
            continue;
        }

        // Strikethrough.
        if c == '~'
            && i + 1 < n
            && chars[i + 1] == '~'
            && let Some(close) = markdown::find_run(chars, i + 2, '~', 2)
        {
            out.push_str("<del>");
            out.push_str(&inline(&chars[i + 2..close]));
            out.push_str("</del>");
            i = close + 2;
            continue;
        }

        // Images and links.
        let is_image = c == '!' && chars.get(i + 1) == Some(&'[');
        if c == '[' || is_image {
            let text_start = if is_image { i + 2 } else { i + 1 };
            if let Some(close) = markdown::find_char(chars, text_start, ']')
                && chars.get(close + 1) == Some(&'(')
                && let Some(end) = markdown::find_char(chars, close + 2, ')')
            {
                let destination: String = chars[close + 2..end].iter().collect();
                if is_image {
                    let alt: String = chars[text_start..close].iter().collect();
                    out.push_str("<img src=\"");
                    push_escaped_all(&mut out, destination.trim());
                    out.push_str("\" alt=\"");
                    push_escaped_all(&mut out, &alt);
                    out.push_str("\">");
                } else {
                    out.push_str("<a href=\"");
                    push_escaped_all(&mut out, destination.trim());
                    out.push_str("\">");
                    out.push_str(&inline(&chars[text_start..close]));
                    out.push_str("</a>");
                }
                i = end + 1;
                continue;
            }
        }

        // Inline HTML, passed straight through so the browser renders it.
        if c == '<'
            && let Some(end) = html_tag_end(chars, i)
        {
            out.extend(chars[i..end].iter());
            i = end;
            continue;
        }

        push_escaped(&mut out, c);
        i += 1;
    }

    out
}

/// Whether `line` opens a raw HTML block: a comment or declaration, or a
/// block-level tag (up to three spaces in). A plain inline tag on a line of its
/// own is left to the paragraph, which renders it inline.
fn html_block_start(line: &[char]) -> bool {
    let mut i = 0;
    while i < line.len() && i < 3 && line[i] == ' ' {
        i += 1;
    }
    if line.get(i) != Some(&'<') {
        return false;
    }
    // Comments (`<!--`), declarations (`<!`) and processing instructions (`<?`).
    if matches!(line.get(i + 1), Some('!') | Some('?')) {
        return true;
    }
    let closing = line.get(i + 1) == Some(&'/');
    let start = i + if closing { 2 } else { 1 };
    if !matches!(line.get(start), Some(c) if c.is_ascii_alphabetic()) {
        return false;
    }
    let mut end = start;
    while matches!(line.get(end), Some(c) if c.is_ascii_alphanumeric() || *c == '-') {
        end += 1;
    }
    let name: String = line[start..end].iter().collect();
    is_block_tag(&name)
}

/// The block-level HTML tags, per CommonMark; embeds like `iframe` and `video`
/// are among them, so a line that opens one is passed through as a block.
fn is_block_tag(name: &str) -> bool {
    matches!(
        name,
        "address"
            | "article"
            | "aside"
            | "base"
            | "basefont"
            | "blockquote"
            | "body"
            | "caption"
            | "center"
            | "col"
            | "colgroup"
            | "dd"
            | "details"
            | "dialog"
            | "dir"
            | "div"
            | "dl"
            | "dt"
            | "fieldset"
            | "figcaption"
            | "figure"
            | "footer"
            | "form"
            | "frame"
            | "frameset"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "head"
            | "header"
            | "hr"
            | "html"
            | "iframe"
            | "legend"
            | "li"
            | "link"
            | "main"
            | "menu"
            | "menuitem"
            | "nav"
            | "noframes"
            | "ol"
            | "optgroup"
            | "option"
            | "p"
            | "param"
            | "pre"
            | "script"
            | "search"
            | "section"
            | "style"
            | "summary"
            | "table"
            | "tbody"
            | "td"
            | "textarea"
            | "tfoot"
            | "th"
            | "thead"
            | "title"
            | "tr"
            | "track"
            | "ul"
            | "video"
    )
}

/// If a raw HTML tag starts at `i`, the index just past its closing `>`.
///
/// An attribute value may hold a `>`, so quotes are respected.
fn html_tag_end(chars: &[char], i: usize) -> Option<usize> {
    if chars.get(i) != Some(&'<') {
        return None;
    }
    match chars.get(i + 1) {
        Some(c) if c.is_ascii_alphabetic() => {
            let mut j = i + 2;
            while matches!(chars.get(j), Some(c) if c.is_ascii_alphanumeric() || *c == '-') {
                j += 1;
            }
            match chars.get(j) {
                Some(c) if c.is_whitespace() || *c == '/' || *c == '>' => {}
                _ => return None,
            }
        }
        Some('/') => match chars.get(i + 2) {
            Some(c) if c.is_ascii_alphabetic() => {}
            _ => return None,
        },
        Some('!') | Some('?') => {}
        _ => return None,
    }

    let mut quote: Option<char> = None;
    let mut j = i + 1;
    while j < chars.len() {
        let c = chars[j];
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == '>' => return Some(j + 1),
            None => {}
        }
        j += 1;
    }
    None
}

fn is_blank(line: &[char]) -> bool {
    line.iter().all(|c| c.is_whitespace())
}

fn starts_block(line: &[char]) -> bool {
    markdown::fence_of(line).is_some()
        || markdown::heading_level(line).is_some()
        || markdown::is_rule(line)
        || markdown::blockquote_prefix(line).is_some()
        || markdown::list_prefix(line).is_some()
        || html_block_start(line)
}

/// The info string after a fence, e.g. `rust` in ```` ```rust ````.
fn fence_info(chars: &[char], len: usize) -> String {
    let mut i = 0;
    while i < chars.len() && chars[i] == ' ' {
        i += 1;
    }
    chars[(i + len).min(chars.len())..]
        .iter()
        .collect::<String>()
        .trim()
        .to_owned()
}

/// The number an ordered list should start at.
fn ordered_start(marker: &str) -> u64 {
    marker
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .unwrap_or(1)
}

fn push_escaped(out: &mut String, c: char) {
    match c {
        '&' => out.push_str("&amp;"),
        '<' => out.push_str("&lt;"),
        '>' => out.push_str("&gt;"),
        '"' => out.push_str("&quot;"),
        '\'' => out.push_str("&#39;"),
        _ => out.push(c),
    }
}

fn push_escaped_all(out: &mut String, text: &str) {
    for c in text.chars() {
        push_escaped(out, c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn html(markdown: &str) -> String {
        body(markdown)
    }

    #[test]
    fn renders_headings_with_their_level() {
        assert_eq!(html("# One"), "<h1>One</h1>\n");
        assert_eq!(html("### Three"), "<h3>Three</h3>\n");
    }

    #[test]
    fn renders_paragraphs_and_joins_soft_wraps() {
        assert_eq!(html("hello\nworld"), "<p>hello\nworld</p>\n");
        assert_eq!(html("one\n\ntwo"), "<p>one</p>\n<p>two</p>\n");
    }

    #[test]
    fn renders_inline_maths_as_mathml() {
        assert_eq!(
            html("$x^2$"),
            "<p><math><msup><mi>x</mi><mn>2</mn></msup></math></p>\n"
        );
        assert!(html("and $a/b$ too").contains("<mfrac><mi>a</mi><mi>b</mi></mfrac>"));
    }

    #[test]
    fn renders_display_maths_as_display_mathml() {
        assert_eq!(
            html("$$\nx^2 + y^2\n$$"),
            "<div class=\"math-block\"><math display=\"block\">\
<mrow><msup><mi>x</mi><mn>2</mn></msup><mo>+</mo><msup><mi>y</mi><mn>2</mn></msup></mrow></math></div>\n"
        );
        // …and the same thing all on one line.
        assert_eq!(
            html("$$x^2$$"),
            "<div class=\"math-block\"><math display=\"block\">\
<msup><mi>x</mi><mn>2</mn></msup></math></div>\n"
        );
    }

    #[test]
    fn maths_is_converted_rather_than_read_as_markdown() {
        // `<` has to survive inside the maths, and is escaped for the document.
        assert_eq!(
            html("$a < b$"),
            "<p><math><mrow><mi>a</mi><mo>&lt;</mo><mi>b</mi></mrow></math></p>\n"
        );
    }

    #[test]
    fn dollars_in_prose_are_left_alone() {
        assert_eq!(html("costs $5 and $10"), "<p>costs $5 and $10</p>\n");
        // An escaped dollar is literal, too.
        assert_eq!(html(r"\$5"), "<p>$5</p>\n");
    }

    #[test]
    fn maths_inside_code_is_untouched() {
        assert_eq!(html("`$x$`"), "<p><code>$x$</code></p>\n");
        assert!(html("```\n$x$\n```").contains("$x$"));
    }

    #[test]
    fn a_document_with_maths() {
        let out = html(
            "# Maths\n\nInline $e^(i pi) + 1 = 0$ and display:\n\n$$\nsum_(i=1)^n i\n$$\n",
        );
        assert!(out.contains("<h1>Maths</h1>"), "{out}");
        assert!(out.contains("Inline <math>"), "{out}");
        assert!(
            out.contains("<div class=\"math-block\"><math display=\"block\">"),
            "{out}"
        );
        // The sum gets its limits above and below.
        assert!(out.contains("<munderover"), "{out}");
        assert!(out.contains('\u{2211}'), "{out}");
    }

    #[test]
    fn renders_inline_markup() {
        assert!(html("**bold**").contains("<strong>bold</strong>"));
        assert!(html("*italic*").contains("<em>italic</em>"));
        assert!(html("~~struck~~").contains("<del>struck</del>"));
        assert!(html("`code`").contains("<code>code</code>"));
    }

    #[test]
    fn renders_links_and_images() {
        assert_eq!(
            html("[docs](https://example.com)"),
            "<p><a href=\"https://example.com\">docs</a></p>\n"
        );
        assert_eq!(
            html("![alt](/pic.png)"),
            "<p><img src=\"/pic.png\" alt=\"alt\"></p>\n"
        );
    }

    #[test]
    fn renders_unordered_lists() {
        assert_eq!(
            html("- one\n- two"),
            "<ul>\n<li>one</li>\n<li>two</li>\n</ul>\n"
        );
        assert!(html("* one").contains("<ul>"));
        assert!(html("+ one").contains("<ul>"));
    }

    #[test]
    fn renders_ordered_lists_and_honours_the_start() {
        assert_eq!(
            html("1. one\n2. two"),
            "<ol>\n<li>one</li>\n<li>two</li>\n</ol>\n"
        );
        assert!(html("3. three").starts_with("<ol start=\"3\">"));
    }

    #[test]
    fn renders_nested_lists() {
        let out = html("- outer\n  - inner");
        assert_eq!(out, "<ul>\n<li>outer\n<ul>\n<li>inner</li>\n</ul>\n</li>\n</ul>\n");
    }

    #[test]
    fn renders_block_quotes_that_can_nest() {
        assert_eq!(html("> quoted"), "<blockquote>\n<p>quoted</p>\n</blockquote>\n");
        assert!(html("> > deep").contains("<blockquote>\n<blockquote>"));
    }

    #[test]
    fn renders_fenced_code_and_its_language() {
        assert_eq!(
            html("```rust\nlet x = 1;\n```"),
            "<pre><code class=\"language-rust\">let x = 1;\n</code></pre>\n"
        );
        // An unterminated fence still renders.
        assert!(html("```\ncode").contains("<pre><code>code\n</code></pre>"));
    }

    #[test]
    fn renders_rules() {
        assert_eq!(html("---"), "<hr>\n");
        assert_eq!(html("***"), "<hr>\n");
    }

    #[test]
    fn passes_raw_html_through_but_escapes_text() {
        // A block tag, such as an embed, is handed to the browser as it stands.
        assert_eq!(
            html("<iframe src=\"https://example.com\"></iframe>"),
            "<iframe src=\"https://example.com\"></iframe>\n"
        );
        // …and so is inline HTML in the middle of a line.
        assert_eq!(html("a <br> b"), "<p>a <br> b</p>\n");
        assert_eq!(html("<span>x</span>"), "<p><span>x</span></p>\n");
        // A `>` inside a quoted attribute does not end the tag early.
        assert_eq!(html("<a title=\"a>b\">x</a>"), "<p><a title=\"a>b\">x</a></p>\n");
        // What is not a tag is still escaped: prose…
        assert_eq!(html("a & b"), "<p>a &amp; b</p>\n");
        // …and a near-miss like a broken tag is left as text, not passed on.
        assert_eq!(html("a < 3 and 5 > 4"), "<p>a &lt; 3 and 5 &gt; 4</p>\n");
        // Code is always literal, whatever it holds.
        assert!(html("```\n<b>&</b>\n```").contains("&lt;b&gt;&amp;&lt;/b&gt;"));
        assert!(html("[x](https://e.com/?a=1&b=2)").contains("href=\"https://e.com/?a=1&amp;b=2\""));
    }

    #[test]
    fn renders_an_embedded_iframe_verbatim() {
        let line = "<iframe width=\"4000\" height=\"2250\" frameborder=\"0\" loading=\"lazy\" allow=\"autoplay; fullscreen; picture-in-picture; clipboard-write; web-share\" allowfullscreen src=\"https://commons.wikimedia.org/wiki/File:Big_Buck_Bunny_4K.webm?embedplayer=true\" />";
        assert_eq!(html(line), format!("{line}\n"));
    }

    #[test]
    fn renders_a_multi_line_html_block() {
        assert_eq!(
            html("<div class=\"note\">\nhello\n</div>"),
            "<div class=\"note\">\nhello\n</div>\n"
        );
    }

    #[test]
    fn an_autolink_like_line_is_not_html() {
        // `<http://…>` is not a tag, so it stays escaped text.
        assert_eq!(html("<http://example.com>"), "<p>&lt;http://example.com&gt;</p>\n");
    }

    #[test]
    fn code_spans_are_not_treated_as_markup() {
        assert_eq!(html("`**not bold**`"), "<p><code>**not bold**</code></p>\n");
    }

    #[test]
    fn a_document_with_everything() {
        let out = html(
            "# Title\n\nSome *prose* with a [link](/x).\n\n- a\n- b\n\n> quote\n\n```sh\nls\n```\n",
        );
        for expected in [
            "<h1>Title</h1>",
            "<em>prose</em>",
            "<a href=\"/x\">link</a>",
            "<ul>",
            "<li>a</li>",
            "<blockquote>",
            "<pre><code class=\"language-sh\">",
        ] {
            assert!(out.contains(expected), "missing {expected} in:\n{out}");
        }
    }
}
