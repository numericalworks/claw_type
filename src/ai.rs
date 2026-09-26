//! The AI actions: what to ask the model for, and how to reach it.
//!
//! Everything here is deliberately small and testable. The requests stream, so
//! the answer appears while it is being written, and a generation can be
//! stopped part way.

use std::io::{BufRead, BufReader};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::time::Duration;

use crate::ollama::describe;
use crate::settings::Settings;

/// A generation is allowed to take a while; a document can be long.
const TIMEOUT: Duration = Duration::from_secs(300);

/// Something the user picked from the AI panel.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Meaning,
    Synonyms,
    Antonyms,
    RephraseSentence,
    RephraseParagraph,
    ProofreadParagraph,
    ProofreadDocument,
}

/// Which part of the document an action works on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    /// The word at the caret.
    Word,
    /// The sentence the caret is in.
    Sentence,
    /// The paragraph the caret is in.
    Paragraph,
    /// The whole document.
    Document,
}

impl Action {
    /// Every action, in the order the panel lists them.
    pub const ALL: [Action; 7] = [
        Action::Meaning,
        Action::Synonyms,
        Action::Antonyms,
        Action::RephraseSentence,
        Action::RephraseParagraph,
        Action::ProofreadParagraph,
        Action::ProofreadDocument,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Action::Meaning => "Meaning of the word",
            Action::Synonyms => "Synonyms of the word",
            Action::Antonyms => "Antonyms of the word",
            Action::RephraseSentence => "Rephrase the sentence",
            Action::RephraseParagraph => "Rephrase the paragraph",
            Action::ProofreadParagraph => "Proofread and correct the paragraph",
            Action::ProofreadDocument => "Proofread and correct the whole document",
        }
    }

    pub fn target(self) -> Target {
        match self {
            Action::Meaning | Action::Synonyms | Action::Antonyms => Target::Word,
            Action::RephraseSentence => Target::Sentence,
            Action::RephraseParagraph | Action::ProofreadParagraph => Target::Paragraph,
            Action::ProofreadDocument => Target::Document,
        }
    }

    /// Whether the answer is meant to take the place of the text it came from.
    ///
    /// Explaining a word is not something to paste over it.
    pub fn replaces(self) -> bool {
        !matches!(self, Action::Meaning | Action::Synonyms | Action::Antonyms)
    }

    /// Lower is steadier; corrections want as little invention as possible.
    fn temperature(self) -> f32 {
        match self {
            Action::Meaning | Action::Synonyms | Action::Antonyms => 0.4,
            _ => 0.2,
        }
    }

    /// The system and user messages to send.
    ///
    /// `subject` is what is being worked on; `context` is the sentence around a
    /// word, for the actions that work on a single one.
    pub fn prompt(self, subject: &str, context: &str) -> (String, String) {
        match self {
            Action::Meaning => (
                "You explain words briefly and plainly. Do not restate the question."
                    .to_owned(),
                format!(
                    "What does \u{201c}{subject}\u{201d} mean in this sentence?\n\n{context}\n\nAnswer in one or two sentences."
                ),
            ),
            Action::Synonyms => (
                "You give synonyms. Reply with nothing but a comma-separated list.".to_owned(),
                format!(
                    "Give up to six synonyms of \u{201c}{subject}\u{201d} as it is used here:\n\n{context}\n\nReply with only the synonyms, separated by commas. If there are none, reply \u{201c}none\u{201d}."
                ),
            ),
            Action::Antonyms => (
                "You give antonyms. Reply with nothing but a comma-separated list.".to_owned(),
                format!(
                    "Give up to six antonyms of \u{201c}{subject}\u{201d} as it is used here:\n\n{context}\n\nReply with only the antonyms, separated by commas. If there are none, reply \u{201c}none\u{201d}."
                ),
            ),
            Action::RephraseSentence => (
                "You rewrite text. Reply with nothing but the rewritten text.".to_owned(),
                format!(
                    "Rewrite this sentence so that it says the same thing in different words, keeping it about the same length. Reply with only the rewritten sentence, without quotation marks and without a code fence.\n\n{subject}"
                ),
            ),
            Action::RephraseParagraph => (
                "You rewrite text. Reply with nothing but the rewritten text.".to_owned(),
                format!(
                    "Rewrite this paragraph in different words, keeping its meaning, its length and its Markdown formatting. Reply with only the rewritten paragraph, without a code fence.\n\n{subject}"
                ),
            ),
            Action::ProofreadParagraph => (
                "You are a careful copy editor. Reply with nothing but the corrected text."
                    .to_owned(),
                format!(
                    "Correct the spelling, grammar and punctuation of this Markdown paragraph. Keep the wording, the meaning and the Markdown as they are, and change only what is wrong. Reply with only the corrected paragraph, without a code fence or any comment.\n\n{subject}"
                ),
            ),
            Action::ProofreadDocument => (
                "You are a careful copy editor. Reply with nothing but the corrected text."
                    .to_owned(),
                format!(
                    "Correct the spelling, grammar and punctuation of this Markdown document. Keep the wording, the meaning and the Markdown structure as they are, and change only what is wrong. Reply with the whole corrected document, without a code fence wrapping it and without any comment.\n\n{subject}"
                ),
            ),
        }
    }
}

/// What a request reports back while it runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// More of the answer.
    Chunk(String),
    /// The answer is complete (including when it was stopped).
    Done,
    /// It failed, with something worth showing.
    Failed(String),
}

/// Ask the model, reporting the answer in pieces through `events`.
///
/// Blocking; run it on a background thread. Setting `cancel` stops it early,
/// leaving whatever has arrived so far.
pub fn run(settings: &Settings, action: Action, subject: &str, context: &str, events: &Sender<Event>, cancel: &Arc<AtomicBool>) {
    if let Err(error) = generate(settings, action, subject, context, events, cancel) {
        let _ = events.send(Event::Failed(error));
    }
}

fn generate(
    settings: &Settings,
    action: Action,
    subject: &str,
    context: &str,
    events: &Sender<Event>,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let url = format!("{}/api/chat", settings.base_url());
    let (system, user) = action.prompt(subject, context);

    let body = serde_json::json!({
        "model": settings.model.trim(),
        "stream": true,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user },
        ],
        "options": { "temperature": action.temperature() },
    })
    .to_string();

    let mut request = ureq::post(&url)
        .config()
        .timeout_global(Some(TIMEOUT))
        .build()
        .header("Content-Type", "application/json");

    let key = settings.api_key.trim();
    if !key.is_empty() {
        request = request.header("Authorization", format!("Bearer {key}"));
    }

    let mut response = request.send(body).map_err(|error| describe(&error))?;

    // The reply is one JSON object per line, each carrying a piece of the
    // answer.
    let reader = BufReader::new(response.body_mut().as_reader());
    for line in reader.lines() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let line = line.map_err(|error| format!("could not read the reply: {error}"))?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let chunk: serde_json::Value =
            serde_json::from_str(line).map_err(|_| "the reply was not JSON".to_owned())?;
        if let Some(error) = chunk.get("error").and_then(|error| error.as_str()) {
            return Err(error.to_owned());
        }
        if let Some(content) = chunk.pointer("/message/content").and_then(|c| c.as_str())
            && !content.is_empty()
        {
            let _ = events.send(Event::Chunk(content.to_owned()));
        }
        if chunk.get("done").and_then(|done| done.as_bool()) == Some(true) {
            break;
        }
    }

    let _ = events.send(Event::Done);
    Ok(())
}

/// Tidy an answer: no surrounding blank lines, and no code fence wrapped around
/// the whole of it.
pub fn clean(reply: &str) -> String {
    let trimmed = reply.trim();

    // Models sometimes wrap a whole answer in a fence. Only strip it when the
    // body has no fences of its own, so a document that contains code is left
    // exactly as it came back.
    if let Some(rest) = trimmed.strip_prefix("```")
        && let Some(end) = rest.rfind("```")
    {
        let inner = &rest[..end];
        let body = inner.split_once('\n').map_or(inner, |(_, body)| body);
        if !body.contains("```") && rest[end..].trim_end() == "```" {
            return body.trim().to_owned();
        }
    }

    trimmed.to_owned()
}

// ---------------------------------------------------------------------------
// Finding the text an action applies to
// ---------------------------------------------------------------------------

/// The word at `caret`, or just before it, as a range of characters.
pub fn word_at(text: &str, caret: usize) -> Option<(usize, usize)> {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return None;
    }
    let caret = caret.min(chars.len());
    let is_word = |c: char| c.is_alphanumeric() || c == '\'' || c == '-';

    let mut start = if caret < chars.len() && is_word(chars[caret]) {
        caret
    } else if caret > 0 && is_word(chars[caret - 1]) {
        caret - 1
    } else {
        return None;
    };

    let mut end = start + 1;
    while start > 0 && is_word(chars[start - 1]) {
        start -= 1;
    }
    while end < chars.len() && is_word(chars[end]) {
        end += 1;
    }
    Some((start, end))
}

/// The sentence containing `caret`, as a range of characters.
///
/// A sentence ends at `.`, `!` or `?`, or at a blank line. Soft line breaks
/// inside a paragraph do not end one, since that is how Markdown is written.
pub fn sentence_at(text: &str, caret: usize) -> (usize, usize) {
    let chars: Vec<char> = text.chars().collect();
    let caret = caret.min(chars.len());

    let mut start = caret;
    while start > 0 {
        if matches!(chars[start - 1], '.' | '!' | '?') {
            break;
        }
        if start >= 2 && chars[start - 1] == '\n' && chars[start - 2] == '\n' {
            break;
        }
        start -= 1;
    }

    let mut end = caret;
    while end < chars.len() {
        if matches!(chars[end], '.' | '!' | '?') {
            end += 1;
            break;
        }
        if end + 1 < chars.len() && chars[end] == '\n' && chars[end + 1] == '\n' {
            break;
        }
        end += 1;
    }

    // Hand the model a clean sentence, without the whitespace around it.
    while start < end && chars[start].is_whitespace() {
        start += 1;
    }
    while end > start && chars[end - 1].is_whitespace() {
        end -= 1;
    }
    (start, end)
}

/// The range of characters covering whole `line`, without its newline.
pub fn line_range(text: &str, line: usize) -> (usize, usize) {
    let mut start = 0;
    for (index, current) in text.split('\n').enumerate() {
        let length = current.chars().count();
        if index == line {
            return (start, start + length);
        }
        start += length + 1; // the newline
    }
    (start.saturating_sub(1), start.saturating_sub(1))
}

/// Put `replacement` in place of the characters in `range`.
pub fn splice(text: &str, range: (usize, usize), replacement: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let start = range.0.min(chars.len());
    let end = range.1.clamp(start, chars.len());
    let mut out: String = chars[..start].iter().collect();
    out.push_str(replacement);
    out.extend(&chars[end..]);
    out
}

/// A throwaway server that answers with streamed NDJSON, so the whole request
/// path can be exercised without a model.
///
/// Returns its address, and a channel that yields the raw request it received.
#[cfg(test)]
pub(crate) fn canned_server(
    lines: &'static [&'static str],
) -> (String, std::sync::mpsc::Receiver<String>) {
    use std::io::{Read as _, Write as _};
    use std::net::TcpListener;

    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let (tx, rx) = std::sync::mpsc::channel();

    std::thread::spawn(move || {
        if let Ok((stream, _)) = listener.accept() {
            let mut reader = BufReader::new(&stream);
            let mut request = String::new();
            let _ = reader.read_line(&mut request);
            let mut line = String::new();
            let mut length = 0;
            while let Ok(read) = reader.read_line(&mut line) {
                if read == 0 || line == "\r\n" {
                    break;
                }
                if let Some(value) = line.to_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse().unwrap_or(0);
                }
                request.push_str(&line);
                line.clear();
            }
            let mut body = vec![0u8; length];
            let _ = reader.read_exact(&mut body);
            request.push_str(&String::from_utf8_lossy(&body));
            let _ = tx.send(request);

            let mut response = String::from(
                "HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nConnection: close\r\n\r\n",
            );
            for line in lines {
                response.push_str(line);
                response.push('\n');
            }
            let mut writer = &stream;
            let _ = writer.write_all(response.as_bytes());
            let _ = writer.flush();
        }
    });

    (format!("http://127.0.0.1:{port}"), rx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_word_under_the_caret() {
        let text = "a well-known example";
        // Inside the word, at its start, at its end, and just after it.
        assert_eq!(word_at(text, 5), Some((2, 12)));
        assert_eq!(word_at(text, 2), Some((2, 12)));
        assert_eq!(word_at(text, 11), Some((2, 12)));
        assert_eq!(word_at(text, 12), Some((2, 12)));
        // The caret just after a word still picks that word up…
        assert_eq!(word_at(text, 1), Some((0, 1)));
        // …but with no word at or before the caret there is none.
        assert_eq!(word_at("a  b", 2), None);
        assert_eq!(word_at("", 0), None);
    }

    #[test]
    fn finds_the_sentence_around_the_caret() {
        let text = "One sentence. Two here! And a third?";
        assert_eq!(sentence_at(text, 3), (0, 13));
        assert_eq!(sentence_at(text, 14), (14, 23));
        assert_eq!(sentence_at(text, 25), (24, 36));
    }

    #[test]
    fn a_soft_line_break_does_not_end_a_sentence() {
        // Markdown is usually wrapped, so a sentence may span lines.
        let text = "This sentence is\nwrapped over lines. And another.";
        assert_eq!(
            &text.chars().collect::<String>()[sentence_at(text, 3).0..sentence_at(text, 3).1],
            "This sentence is\nwrapped over lines."
        );
    }

    #[test]
    fn a_blank_line_ends_a_sentence() {
        let text = "A paragraph\n\nAnother one";
        let (start, end) = sentence_at(text, 14);
        assert_eq!(&text.chars().collect::<String>()[start..end], "Another one");
    }

    #[test]
    fn finds_a_line_range() {
        let text = "first\nsecond\nthird";
        assert_eq!(line_range(text, 0), (0, 5));
        assert_eq!(line_range(text, 1), (6, 12));
        assert_eq!(line_range(text, 2), (13, 18));
    }

    #[test]
    fn splices_a_replacement_in() {
        let text = "Hello world";
        assert_eq!(splice(text, (0, 5), "Goodbye"), "Goodbye world");
        assert_eq!(splice(text, (6, 11), "there"), "Hello there");
        assert_eq!(splice(text, (11, 11), "!"), "Hello world!");
        // Out of range is clamped rather than panicking.
        assert_eq!(splice(text, (0, 99), "x"), "x");
    }

    #[test]
    fn cleaning_trims_and_unwraps_a_fence() {
        assert_eq!(clean("  hello\n"), "hello");
        assert_eq!(clean("```markdown\nhello\n```"), "hello");
        assert_eq!(clean("```\nhello\n```"), "hello");
    }

    #[test]
    fn cleaning_leaves_code_that_was_already_there() {
        // A document that contains a code block must come back untouched.
        let document = "Text\n\n```rust\nlet x = 1;\n```\n\nMore";
        assert_eq!(clean(document), document);
    }

    #[test]
    fn prompts_mention_the_text_and_ask_for_nothing_else() {
        let (_, user) = Action::ProofreadParagraph.prompt("teh cat", "");
        assert!(user.contains("teh cat"), "{user}");
        assert!(user.contains("Reply with only"), "{user}");

        let (_, user) = Action::Synonyms.prompt("happy", "She was happy.");
        assert!(user.contains("happy"), "{user}");
        assert!(user.contains("She was happy."), "{user}");
    }

    #[test]
    fn only_the_rewriting_actions_replace_their_text() {
        assert!(!Action::Meaning.replaces());
        assert!(!Action::Synonyms.replaces());
        assert!(!Action::Antonyms.replaces());
        for action in [
            Action::RephraseSentence,
            Action::RephraseParagraph,
            Action::ProofreadParagraph,
            Action::ProofreadDocument,
        ] {
            assert!(action.replaces(), "{action:?}");
        }
    }

    #[test]
    fn streams_the_answer_and_reports_the_request() {
        let (url, request) = canned_server(&[
            r#"{"message":{"role":"assistant","content":"Hel"},"done":false}"#,
            r#"{"message":{"role":"assistant","content":"lo"},"done":false}"#,
            r#"{"message":{"role":"assistant","content":""},"done":true}"#,
        ]);
        let settings = Settings {
            url,
            model: "test-model".to_owned(),
            api_key: "sk-secret".to_owned(),
        };

        let (sender, events) = std::sync::mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        run(&settings, Action::Meaning, "hello", "a context", &sender, &cancel);
        // The request is done, so drop the sender: the receiver would otherwise
        // wait forever for more events.
        drop(sender);

        let received: Vec<Event> = events.into_iter().collect();
        assert_eq!(
            received,
            vec![
                Event::Chunk("Hel".to_owned()),
                Event::Chunk("lo".to_owned()),
                Event::Done
            ]
        );

        let request = request.recv_timeout(Duration::from_secs(5)).expect("request");
        assert!(request.starts_with("POST /api/chat "), "{request}");
        assert!(request.contains("Bearer sk-secret"), "{request}");
        assert!(request.contains("\"model\":\"test-model\""), "{request}");
        assert!(request.contains("\"stream\":true"), "{request}");
        assert!(request.contains("a context"), "{request}");
    }

    #[test]
    fn stopping_early_keeps_what_arrived() {
        let (url, _request) = canned_server(&[
            r#"{"message":{"content":"one"},"done":false}"#,
            r#"{"message":{"content":"two"},"done":false}"#,
        ]);
        let settings = Settings {
            url,
            model: "test-model".to_owned(),
            ..Settings::default()
        };

        let (sender, events) = std::sync::mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(true)); // stop immediately
        run(&settings, Action::Meaning, "word", "", &sender, &cancel);
        drop(sender);

        let received: Vec<Event> = events.into_iter().collect();
        assert_eq!(received, vec![Event::Done], "nothing should have streamed");
    }

    #[test]
    fn an_error_chunk_is_reported() {
        let (url, _request) = canned_server(&[r#"{"error":"model not found"}"#]);
        let settings = Settings {
            url,
            model: "nope".to_owned(),
            ..Settings::default()
        };

        let (sender, events) = std::sync::mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        run(&settings, Action::Meaning, "word", "", &sender, &cancel);
        drop(sender);

        let received: Vec<Event> = events.into_iter().collect();
        match received.first() {
            Some(Event::Failed(error)) => assert!(error.contains("model not found"), "{error}"),
            other => panic!("expected a failure, got {other:?}"),
        }
    }
}
