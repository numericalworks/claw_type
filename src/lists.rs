//! Markdown list and block-quote continuation.
//!
//! When Enter is pressed at the end of a line, this decides what the next line
//! should start with: the next bullet, the incremented number, the quote marker —
//! or nothing at all. An item that holds only a marker ends the list instead.

/// What pressing Enter at the end of a line should do next.
#[derive(Debug, PartialEq, Eq)]
pub enum Continuation {
    /// Just start a fresh empty line.
    None,
    /// Start the next line with this prefix (list / quote continuation).
    Prefix(String),
    /// The line held only a marker, so this Enter ends the list.
    Clear,
}

/// Decide how to continue a line when Enter is pressed.
///
/// `line_before_caret` is the text on the current line *up to the caret*, which
/// is what decides whether we are inside a bullet, a numbered item or a quote.
pub fn continuation(line_before_caret: &str) -> Continuation {
    let line = line_before_caret;
    let indent_len = line.len() - line.trim_start().len();
    let indent = &line[..indent_len];
    let rest = &line[indent_len..];

    if rest == ">" {
        return Continuation::Clear;
    }
    if let Some(after) = rest.strip_prefix("> ") {
        return if after.trim().is_empty() {
            Continuation::Clear
        } else {
            Continuation::Prefix(format!("{indent}> "))
        };
    }

    for marker in ["-", "*", "+"] {
        if rest == marker {
            return Continuation::Clear;
        }
        let with_space = format!("{marker} ");
        if let Some(after) = rest.strip_prefix(&with_space) {
            return if after.trim().is_empty() {
                Continuation::Clear
            } else {
                Continuation::Prefix(format!("{indent}{marker} "))
            };
        }
    }

    let digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 {
        let number = &rest[..digits];
        let after = &rest[digits..];
        if let Some(delim) = after.chars().next()
            && (delim == '.' || delim == ')')
        {
            let tail = &after[delim.len_utf8()..];
            if let Some(content) = tail.strip_prefix(' ') {
                if content.trim().is_empty() {
                    return Continuation::Clear;
                }
                let next = number
                    .parse::<u64>()
                    .ok()
                    .and_then(|n| n.checked_add(1))
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| number.to_string());
                return Continuation::Prefix(format!("{indent}{next}{delim} "));
            }
        }
    }

    Continuation::None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn next(line: &str) -> Continuation {
        continuation(line)
    }

    #[test]
    fn carries_bullets_forward() {
        for marker in ["-", "*", "+"] {
            assert_eq!(
                next(&format!("{marker} item")),
                Continuation::Prefix(format!("{marker} "))
            );
        }
    }

    #[test]
    fn increments_numbered_items() {
        assert_eq!(next("1. one"), Continuation::Prefix("2. ".to_owned()));
        assert_eq!(next("9. nine"), Continuation::Prefix("10. ".to_owned()));
        assert_eq!(next("1) one"), Continuation::Prefix("2) ".to_owned()));
        assert_eq!(next("41. x"), Continuation::Prefix("42. ".to_owned()));
    }

    #[test]
    fn carries_quotes_forward() {
        assert_eq!(next("> quote"), Continuation::Prefix("> ".to_owned()));
    }

    #[test]
    fn keeps_indentation() {
        assert_eq!(next("  - item"), Continuation::Prefix("  - ".to_owned()));
        assert_eq!(next("   1. item"), Continuation::Prefix("   2. ".to_owned()));
    }

    #[test]
    fn quote_continuation_is_one_level() {
        // Nested quotes are continued at the outer level, like most editors.
        assert_eq!(next("> > deep"), Continuation::Prefix("> ".to_owned()));
    }

    #[test]
    fn an_empty_marker_ends_the_list() {
        assert_eq!(next("- "), Continuation::Clear);
        assert_eq!(next("* "), Continuation::Clear);
        assert_eq!(next("1. "), Continuation::Clear);
        assert_eq!(next("7) "), Continuation::Clear);
        assert_eq!(next("> "), Continuation::Clear);
        assert_eq!(next(">"), Continuation::Clear);
        assert_eq!(next("  - "), Continuation::Clear);
    }

    #[test]
    fn plain_lines_get_an_ordinary_newline() {
        for line in ["", "hello", "1.", "1.x", "-x", ">x", "2024 was a year"] {
            assert_eq!(next(line), Continuation::None, "{line:?}");
        }
    }
}
