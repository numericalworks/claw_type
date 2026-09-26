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
            respond(&stream, "200 OK", "text/html; charset=utf-8", None, &page);
        }
        "/content" => {
            let (html, version) = snapshot(doc);
            respond(
                &stream,
                "200 OK",
                "text/html; charset=utf-8",
                Some(version),
                &html,
            );
        }
        "/favicon.ico" => respond(&stream, "204 No Content", "text/plain", None, ""),
        _ => respond(
            &stream,
            "404 Not Found",
            "text/plain; charset=utf-8",
            None,
            "not found",
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
    version: Option<u64>,
    body: &str,
) {
    let mut response = String::with_capacity(body.len() + 256);
    response.push_str("HTTP/1.1 ");
    response.push_str(status);
    response.push_str("\r\nContent-Type: ");
    response.push_str(content_type);
    response.push_str("\r\nContent-Length: ");
    response.push_str(&body.len().to_string());
    response.push_str("\r\nCache-Control: no-store\r\nConnection: close\r\n");
    if let Some(version) = version {
        response.push_str("X-Version: ");
        response.push_str(&version.to_string());
        response.push_str("\r\n");
    }
    response.push_str("\r\n");
    response.push_str(body);

    let mut writer = stream;
    let _ = writer.write_all(response.as_bytes());
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

try { apply(localStorage.getItem(KEY) || 'dark'); } catch (e) { apply('dark'); }

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
