//! The browser preview.
//!
//! `Cmd/Ctrl+P` starts a tiny HTTP server on an ephemeral **loopback** port
//! (`127.0.0.1`, never exposed to the network) and opens the default browser at
//! it. The page polls for changes a few times a second, so the preview follows
//! the buffer as you type, and it carries its own dark/light toggle.
//!
//! The server is a handful of lines of `std::net` on purpose: it only ever
//! serves this one page, and adds no dependency.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::palette::{Rgb, Theme};

/// Where the current document goes in the page template.
const DOC: &str = "<!--DOC-->";
/// Where the current version goes in the page template.
const VERSION: &str = "<!--VERSION-->";
/// Pages must always be re-fetched; assets may be cached.
const NO_STORE: &str = "no-store";

/// highlight.js, vendored so the preview needs no network access and works
/// offline: version 11.10.0, the "common languages" bundle.
///
/// BSD-3-Clause — see `assets/highlight/LICENSE.txt`.
const HIGHLIGHT_JS: &[u8] = include_bytes!("../assets/highlight/highlight.min.js");

/// Bump this whenever the vendored `highlight.min.js` is replaced: it is part of
/// the script URL, so browsers do not keep serving a cached copy of the old one.
const HIGHLIGHT_VERSION: &str = "11.10.0";

/// The document the browser is currently showing.
#[derive(Default)]
struct Doc {
    version: u64,
    html: String,
}

/// A running preview server.
pub struct Preview {
    url: String,
    doc: Arc<Mutex<Doc>>,
}

impl Preview {
    /// Start serving on an ephemeral loopback port.
    ///
    /// Returns `None` if the port could not be bound. The server thread runs
    /// until the process exits.
    pub fn start(theme: &Theme) -> Option<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).ok()?;
        let port = listener.local_addr().ok()?.port();
        let doc = Arc::new(Mutex::new(Doc::default()));
        let shell = shell(theme);

        let server_doc = Arc::clone(&doc);
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let doc = Arc::clone(&server_doc);
                let page = shell.clone();
                thread::spawn(move || handle(stream, &doc, &page));
            }
        });

        Some(Self {
            url: format!("http://127.0.0.1:{port}/"),
            doc,
        })
    }

    /// The address the preview is served from.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Hand the browser the latest HTML; it picks it up on its next poll.
    pub fn publish(&self, html: &str) {
        let mut doc = self.doc.lock().expect("preview state poisoned");
        doc.html.clear();
        doc.html.push_str(html);
        doc.version += 1;
    }

    /// Ask the desktop to open the preview in its default browser.
    pub fn open_in_browser(&self) -> std::io::Result<()> {
        opener().arg(&self.url).spawn().map(|_| ())
    }
}

fn handle(stream: TcpStream, doc: &Arc<Mutex<Doc>>, shell: &str) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));

    // Read the request line, then drain the headers.
    let mut reader = BufReader::new(&stream);
    let mut request = String::new();
    if reader.read_line(&mut request).is_err() {
        return;
    }
    loop {
        let mut header = String::new();
        match reader.read_line(&mut header) {
            Ok(0) | Err(_) => break,
            Ok(_) if header == "\r\n" || header == "\n" => break,
            Ok(_) => {}
        }
    }

    let path = request.split_whitespace().nth(1).unwrap_or("/");
    let path = path.split('?').next().unwrap_or(path);

    match path {
        "/" => {
            let (html, version) = snapshot(doc);
            let page = shell
                .replace(DOC, &html)
                .replace(VERSION, &version.to_string());
            respond(
                &stream,
                "200 OK",
                "text/html; charset=utf-8",
                NO_STORE,
                None,
                page.as_bytes(),
            );
        }
        "/content" => {
            let (html, version) = snapshot(doc);
            respond(
                &stream,
                "200 OK",
                "text/html; charset=utf-8",
                NO_STORE,
                Some(version),
                html.as_bytes(),
            );
        }
        // Cacheable: the page asks for them once, and they never change.
        "/highlight.js" => respond(
            &stream,
            "200 OK",
            "text/javascript; charset=utf-8",
            "max-age=86400",
            None,
            HIGHLIGHT_JS,
        ),
        "/favicon.ico" => respond(
            &stream,
            "204 No Content",
            "text/plain",
            "max-age=86400",
            None,
            b"",
        ),
        _ => respond(
            &stream,
            "404 Not Found",
            "text/plain; charset=utf-8",
            NO_STORE,
            None,
            b"not found",
        ),
    }
}

fn snapshot(doc: &Arc<Mutex<Doc>>) -> (String, u64) {
    let doc = doc.lock().expect("preview state poisoned");
    (doc.html.clone(), doc.version)
}

fn respond(
    stream: &TcpStream,
    status: &str,
    content_type: &str,
    cache: &str,
    version: Option<u64>,
    body: &[u8],
) {
    let mut response = String::with_capacity(body.len() + 256);
    response.push_str("HTTP/1.1 ");
    response.push_str(status);
    response.push_str("\r\nContent-Type: ");
    response.push_str(content_type);
    response.push_str("\r\nContent-Length: ");
    response.push_str(&body.len().to_string());
    response.push_str("\r\nCache-Control: ");
    response.push_str(cache);
    response.push_str("\r\nConnection: close\r\n");
    if let Some(version) = version {
        response.push_str("X-Version: ");
        response.push_str(&version.to_string());
        response.push_str("\r\n");
    }
    response.push_str("\r\n");

    let mut writer = stream;
    let _ = writer.write_all(response.as_bytes());
    let _ = writer.write_all(body);
    let _ = writer.flush();
}

/// A command that hands a URL to the desktop's default browser.
fn opener() -> Command {
    #[cfg(target_os = "macos")]
    let command = Command::new("open");
    #[cfg(all(unix, not(target_os = "macos")))]
    let command = Command::new("xdg-open");
    #[cfg(target_os = "windows")]
    let command = {
        let mut command = Command::new("cmd");
        // The empty argument is the window title `start` expects.
        command.args(["/C", "start", ""]);
        command
    };
    command
}

// -- the page ---------------------------------------------------------------

fn shell(theme: &Theme) -> String {
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>claw_type preview</title>
<style>
{vars}
{base}
</style>
</head>
<body>
<article id="doc" data-version="{VERSION}">{DOC}</article>
<div class="hint" id="hint" title="Toggle dark / light"></div>
<script src="/highlight.js?v={HIGHLIGHT_VERSION}"></script>
<script>
{script}
</script>
</body>
</html>
"#,
        vars = variables(theme, &Theme::light()),
        base = BASE_CSS,
        script = SCRIPT,
    )
}

fn variables(dark: &Theme, light: &Theme) -> String {
    format!(
        ":root {{\n{}\n  color-scheme: dark;\n}}\nhtml.light {{\n{}\n  color-scheme: light;\n}}",
        theme_vars(dark),
        theme_vars(light)
    )
}

fn theme_vars(theme: &Theme) -> String {
    let mut out = String::new();
    var(&mut out, "bg", theme.bg);
    var(&mut out, "fg", theme.fg);
    var(&mut out, "dim", theme.dim);
    var(&mut out, "accent", theme.accent);
    var(&mut out, "code", theme.code);
    var(&mut out, "quote", theme.quote);
    var(&mut out, "link", theme.link);
    var(&mut out, "rule", theme.rule);
    var(&mut out, "syn-comment", theme.syntax.comment);
    var(&mut out, "syn-keyword", theme.syntax.keyword);
    var(&mut out, "syn-string", theme.syntax.string);
    var(&mut out, "syn-number", theme.syntax.number);
    var(&mut out, "syn-function", theme.syntax.function);
    var(&mut out, "syn-type", theme.syntax.ty);
    var(&mut out, "syn-punctuation", theme.syntax.punctuation);
    for (level, colour) in theme.heading.iter().enumerate() {
        var(&mut out, &format!("h{}", level + 1), *colour);
    }
    out
}

fn var(out: &mut String, name: &str, colour: Rgb) {
    out.push_str("  --");
    out.push_str(name);
    out.push_str(": ");
    out.push_str(&hex(colour));
    out.push_str(";\n");
}

fn hex(colour: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", colour.0, colour.1, colour.2)
}

const BASE_CSS: &str = r#"* { box-sizing: border-box; }
html, body { margin: 0; padding: 0; background: var(--bg); }
body {
  color: var(--fg);
  font: 18px/1.65 -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif;
  -webkit-text-size-adjust: 100%;
}
article { max-width: 760px; margin: 0 auto; padding: 4rem 1.5rem 6rem; }
h1, h2, h3, h4, h5, h6 { line-height: 1.25; margin: 1.6em 0 .6em; font-weight: 600; }
h1 { color: var(--h1); font-size: 1.9em; }
h2 { color: var(--h2); font-size: 1.55em; }
h3 { color: var(--h3); font-size: 1.3em; }
h4 { color: var(--h4); font-size: 1.12em; }
h5 { color: var(--h5); font-size: 1em; }
h6 { color: var(--h6); font-size: .95em; }
p { margin: 0 0 1.1em; }
a { color: var(--link); }
code {
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: .92em; color: var(--code);
  background: rgba(128, 128, 128, .13); padding: .12em .32em; border-radius: 4px;
}
pre { background: rgba(128, 128, 128, .13); padding: 1rem 1.1rem; border-radius: 8px; overflow-x: auto; }
pre code { background: none; padding: 0; color: var(--fg); }
blockquote { margin: 0 0 1.1em; padding-left: 1.1rem; border-left: 3px solid var(--rule); color: var(--quote); font-style: italic; }
hr { border: 0; border-top: 1px solid var(--rule); margin: 2rem 0; }
img { max-width: 100%; height: auto; }
ul, ol { padding-left: 1.4rem; margin: 0 0 1.1em; }
li { margin: .25em 0; }
li > ul, li > ol { margin-bottom: 0; }
del { opacity: .65; }
/* Maths: converted to MathML in the app and drawn by the browser itself. */
.math-block { margin: 1.4em 0; overflow-x: auto; overflow-y: hidden; }
.math-block > math { display: block; width: fit-content; margin: 0 auto; }
/* highlight.js token classes, mapped onto the palette so both themes work */
.hljs-comment, .hljs-quote { color: var(--syn-comment); font-style: italic; }
.hljs-keyword, .hljs-literal, .hljs-selector-tag, .hljs-name, .hljs-built_in, .hljs-meta { color: var(--syn-keyword); }
.hljs-string, .hljs-regexp, .hljs-addition, .hljs-template-variable, .hljs-attribute { color: var(--syn-string); }
.hljs-number, .hljs-symbol, .hljs-bullet, .hljs-attr, .hljs-selector-attr { color: var(--syn-number); }
.hljs-title, .hljs-title.function_, .hljs-section, .hljs-selector-id { color: var(--syn-function); }
.hljs-type, .hljs-class .hljs-title, .hljs-title.class_, .hljs-params, .hljs-selector-class { color: var(--syn-type); }
.hljs-punctuation, .hljs-operator { color: var(--syn-punctuation); }
.hljs-variable, .hljs-property, .hljs-subst { color: var(--fg); }
.hljs-emphasis { font-style: italic; }
.hljs-strong { font-weight: 600; }
.hint {
  position: fixed; right: 16px; bottom: 14px;
  font-size: 12px; padding: 5px 11px; border-radius: 999px;
  border: 1px solid var(--rule); background: rgba(128, 128, 128, .14);
  color: var(--quote); cursor: pointer; user-select: none;
}
.hint:hover { color: var(--fg); border-color: var(--dim); }
"#;

const SCRIPT: &str = r#"const doc = document.getElementById('doc');
const hint = document.getElementById('hint');
const root = document.documentElement;
const KEY = 'claw_type_theme';

function isLight() {
  return root.classList.contains('light');
}

function apply(theme) {
  root.classList.toggle('light', theme === 'light');
  updateHint();
}

function updateHint() {
  hint.textContent = isLight() ? '\u263e  press d for dark' : '\u2600  press d for light';
}

function toggle() {
  const next = isLight() ? 'dark' : 'light';
  try { localStorage.setItem(KEY, next); } catch (e) {}
  apply(next);
}

function highlight() {
  if (window.hljs) {
    hljs.highlightAll();
  }
}

try { apply(localStorage.getItem(KEY) || 'dark'); } catch (e) { apply('dark'); }

// Code blocks arrive already escaped, so silence highlight.js's escaping warning.
if (window.hljs) hljs.configure({ ignoreUnescapedHTML: true });
highlight();

hint.addEventListener('click', toggle);

addEventListener('keydown', (e) => {
  // `toLowerCase` so Caps Lock and Shift still work.
  if (e.key.toLowerCase() === 'd' && !e.metaKey && !e.ctrlKey && !e.altKey) {
    toggle();
  }
});

let version = doc.dataset.version;
async function poll() {
  try {
    const res = await fetch('/content', { cache: 'no-store' });
    const next = res.headers.get('X-Version');
    if (res.ok && next !== version) {
      version = next;
      const scroll = window.scrollY;
      doc.innerHTML = await res.text();
      highlight();
      window.scrollTo(0, scroll);
    }
  } catch (e) {}
  setTimeout(poll, 300); // a few times a second
}
poll();
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    /// A GET request to a loopback server, returning the raw response.
    fn get(port: u16, path: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        write!(stream, "GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n").expect("write");
        let mut response = String::new();
        stream.read_to_string(&mut response).expect("read");
        response
    }

    fn start() -> Option<(Preview, u16)> {
        let preview = Preview::start(&Theme::default())?;
        let port = preview
            .url()
            .trim_end_matches('/')
            .rsplit(':')
            .next()?
            .parse()
            .ok()?;
        Some((preview, port))
    }

    #[test]
    fn serves_the_page_and_the_content() {
        let Some((preview, port)) = start() else {
            eprintln!("cannot bind a loopback port here; skipping");
            return;
        };
        preview.publish("<p>hello</p>");

        let content = get(port, "/content");
        assert!(content.contains("X-Version: 1"), "{content}");
        assert!(content.ends_with("<p>hello</p>"), "{content}");

        let page = get(port, "/");
        assert!(page.contains("<p>hello</p>"), "{page}");
        assert!(page.contains("data-version=\"1\""), "{page}");
        // The page carries the theme toggle and the polling loop.
        assert!(page.contains("claw_type_theme"), "{page}");
        assert!(page.contains("setTimeout(poll, 300)"), "{page}");
    }

    #[test]
    fn publishes_bump_the_version() {
        let Some((preview, port)) = start() else {
            return;
        };
        preview.publish("<p>one</p>");
        preview.publish("<p>two</p>");
        let content = get(port, "/content");
        assert!(content.contains("X-Version: 2"), "{content}");
        assert!(content.contains("<p>two</p>"), "{content}");
    }

    #[test]
    fn unknown_paths_are_not_found() {
        let Some((_preview, port)) = start() else {
            return;
        };
        assert!(get(port, "/nope").starts_with("HTTP/1.1 404"));
    }

    #[test]
    fn serves_the_vendored_highlighter() {
        // The vendored file is the real thing, not a placeholder.
        assert!(HIGHLIGHT_JS.len() > 50_000);
        let banner = String::from_utf8_lossy(&HIGHLIGHT_JS[..200]);
        assert!(banner.contains("Highlight.js"), "{banner}");

        let Some((_preview, port)) = start() else {
            return;
        };
        let response = get(port, "/highlight.js");
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert!(
            response.contains("Content-Type: text/javascript"),
            "{response}"
        );
        // Cacheable, unlike the pages themselves.
        assert!(response.contains("Cache-Control: max-age=86400"), "{response}");
        assert!(response.contains("var hljs="), "the script body must be served");
        assert!(
            response.contains(&format!("Content-Length: {}", HIGHLIGHT_JS.len())),
            "the whole file must be served"
        );
    }

    #[test]
    fn the_page_loads_the_highlighter_and_styles_its_tokens() {
        let page = shell(&Theme::default());
        assert!(page.contains("<script src=\"/highlight.js?v="), "{page}");
        // The URL is versioned so a browser cannot serve a stale cached copy.
        assert!(
            page.contains(&format!("?v={HIGHLIGHT_VERSION}\">")),
            "the asset URL must carry the version"
        );
        assert!(page.contains("hljs.highlightAll()"));
        // Token colours are per-theme, like everything else.
        assert!(page.contains("--syn-keyword"));
        assert!(page.contains(".hljs-keyword"));
        assert!(page.contains(".hljs-string"));
        assert!(page.contains(".hljs-comment"));
    }

    #[test]
    fn the_page_styles_display_maths() {
        // Maths is converted to MathML by the app, so the page only needs the
        // styling that centres a display block.
        let page = shell(&Theme::default());
        assert!(page.contains(".math-block"), "{page}");
        assert!(!page.contains("MathJax"), "no maths JavaScript is needed");
        assert!(!page.contains("/mathjax.js"), "nothing extra is fetched");
    }

    #[test]
    fn the_page_has_both_themes_and_a_way_to_switch() {
        let page = shell(&Theme::default());
        assert!(page.contains("html.light"));
        assert!(page.contains("color-scheme: dark"));
        assert!(page.contains("color-scheme: light"));
        // The `d` shortcut, case-insensitively so Caps Lock does not break it.
        assert!(
            page.contains("e.key.toLowerCase() === 'd'"),
            "the 'd' shortcut must be handled"
        );
        // …and the same toggle on the visible pill, for discoverability.
        assert!(page.contains("id=\"hint\""), "the hint must be in the page");
        assert!(page.contains("hint.addEventListener('click', toggle)"));
    }
}
