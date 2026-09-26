//! AsciiMath → MathML.
//!
//! The maths in a document is written in [AsciiMath](https://asciimath.org) —
//! `sqrt(x)`, `int_0^1 x^2 dx`, `sum_(i=1)^n i` — and converted here, in the app,
//! exactly like the Markdown around it. The browser only has to render MathML,
//! which every current engine does natively, so **no maths JavaScript is needed
//! and nothing is fetched at runtime**.
//!
//! It is a small implementation of the notation rather than a complete one: the
//! constructs people actually write in prose. Anything it does not recognise
//! comes out as ordinary text rather than an error.

// ---------------------------------------------------------------------------
// The symbol table
// ---------------------------------------------------------------------------

/// What a symbol turns into.
struct Symbol {
    /// The AsciiMath spelling.
    name: &'static str,
    /// The characters to put inside `<mi>` or `<mo>`.
    content: &'static str,
    /// Operators get `<mo>` (which spaces them correctly), the rest `<mi>`.
    operator: bool,
    /// Big operators take their limits above and below rather than to the side.
    big: bool,
}

/// The symbols we know, longest name first so that matching is unambiguous.
///
/// Greek is written as ordinary letters with no mathematical-alphanumeric
/// codepoints, so it renders in any font.
const SYMBOLS: &[Symbol] = &[
    // Greek, lowercase then uppercase.
    Symbol { name: "alpha", content: "α", operator: false, big: false },
    Symbol { name: "beta", content: "β", operator: false, big: false },
    Symbol { name: "gamma", content: "γ", operator: false, big: false },
    Symbol { name: "delta", content: "δ", operator: false, big: false },
    Symbol { name: "epsilon", content: "ε", operator: false, big: false },
    Symbol { name: "zeta", content: "ζ", operator: false, big: false },
    Symbol { name: "eta", content: "η", operator: false, big: false },
    Symbol { name: "theta", content: "θ", operator: false, big: false },
    Symbol { name: "iota", content: "ι", operator: false, big: false },
    Symbol { name: "kappa", content: "κ", operator: false, big: false },
    Symbol { name: "lambda", content: "λ", operator: false, big: false },
    Symbol { name: "mu", content: "μ", operator: false, big: false },
    Symbol { name: "nu", content: "ν", operator: false, big: false },
    Symbol { name: "xi", content: "ξ", operator: false, big: false },
    Symbol { name: "omicron", content: "ο", operator: false, big: false },
    Symbol { name: "pi", content: "π", operator: false, big: false },
    Symbol { name: "rho", content: "ρ", operator: false, big: false },
    Symbol { name: "sigma", content: "σ", operator: false, big: false },
    Symbol { name: "tau", content: "τ", operator: false, big: false },
    Symbol { name: "upsilon", content: "υ", operator: false, big: false },
    Symbol { name: "phi", content: "φ", operator: false, big: false },
    Symbol { name: "chi", content: "χ", operator: false, big: false },
    Symbol { name: "psi", content: "ψ", operator: false, big: false },
    Symbol { name: "omega", content: "ω", operator: false, big: false },
    Symbol { name: "Alpha", content: "Α", operator: false, big: false },
    Symbol { name: "Beta", content: "Β", operator: false, big: false },
    Symbol { name: "Gamma", content: "Γ", operator: false, big: false },
    Symbol { name: "Delta", content: "Δ", operator: false, big: false },
    Symbol { name: "Epsilon", content: "Ε", operator: false, big: false },
    Symbol { name: "Zeta", content: "Ζ", operator: false, big: false },
    Symbol { name: "Eta", content: "Η", operator: false, big: false },
    Symbol { name: "Theta", content: "Θ", operator: false, big: false },
    Symbol { name: "Iota", content: "Ι", operator: false, big: false },
    Symbol { name: "Kappa", content: "Κ", operator: false, big: false },
    Symbol { name: "Lambda", content: "Λ", operator: false, big: false },
    Symbol { name: "Mu", content: "Μ", operator: false, big: false },
    Symbol { name: "Nu", content: "Ν", operator: false, big: false },
    Symbol { name: "Xi", content: "Ξ", operator: false, big: false },
    Symbol { name: "Omicron", content: "Ο", operator: false, big: false },
    Symbol { name: "Pi", content: "Π", operator: false, big: false },
    Symbol { name: "Rho", content: "Ρ", operator: false, big: false },
    Symbol { name: "Sigma", content: "Σ", operator: false, big: false },
    Symbol { name: "Tau", content: "Τ", operator: false, big: false },
    Symbol { name: "Upsilon", content: "Υ", operator: false, big: false },
    Symbol { name: "Phi", content: "Φ", operator: false, big: false },
    Symbol { name: "Chi", content: "Χ", operator: false, big: false },
    Symbol { name: "Psi", content: "Ψ", operator: false, big: false },
    Symbol { name: "Omega", content: "Ω", operator: false, big: false },
    // Number sets.
    Symbol { name: "RR", content: "ℝ", operator: false, big: false },
    Symbol { name: "NN", content: "ℕ", operator: false, big: false },
    Symbol { name: "ZZ", content: "ℤ", operator: false, big: false },
    Symbol { name: "QQ", content: "ℚ", operator: false, big: false },
    Symbol { name: "CC", content: "ℂ", operator: false, big: false },
    // Big operators.
    Symbol { name: "sum", content: "∑", operator: true, big: true },
    Symbol { name: "prod", content: "∏", operator: true, big: true },
    Symbol { name: "int", content: "∫", operator: true, big: false },
    Symbol { name: "oint", content: "∮", operator: true, big: false },
    Symbol { name: "lim", content: "lim", operator: false, big: true },
    Symbol { name: "max", content: "max", operator: false, big: true },
    Symbol { name: "min", content: "min", operator: false, big: true },
    // Functions, which MathML renders upright when they have several letters.
    Symbol { name: "arcsin", content: "arcsin", operator: false, big: false },
    Symbol { name: "arccos", content: "arccos", operator: false, big: false },
    Symbol { name: "arctan", content: "arctan", operator: false, big: false },
    Symbol { name: "sinh", content: "sinh", operator: false, big: false },
    Symbol { name: "cosh", content: "cosh", operator: false, big: false },
    Symbol { name: "tanh", content: "tanh", operator: false, big: false },
    Symbol { name: "sin", content: "sin", operator: false, big: false },
    Symbol { name: "cos", content: "cos", operator: false, big: false },
    Symbol { name: "tan", content: "tan", operator: false, big: false },
    Symbol { name: "sec", content: "sec", operator: false, big: false },
    Symbol { name: "csc", content: "csc", operator: false, big: false },
    Symbol { name: "cot", content: "cot", operator: false, big: false },
    Symbol { name: "log", content: "log", operator: false, big: false },
    Symbol { name: "ln", content: "ln", operator: false, big: false },
    Symbol { name: "exp", content: "exp", operator: false, big: false },
    Symbol { name: "det", content: "det", operator: false, big: false },
    Symbol { name: "dim", content: "dim", operator: false, big: false },
    Symbol { name: "gcd", content: "gcd", operator: false, big: false },
    Symbol { name: "lcm", content: "lcm", operator: false, big: false },
    Symbol { name: "mod", content: "mod", operator: false, big: false },
    // Relations and arrows.
    Symbol { name: "<=>", content: "⇔", operator: true, big: false },
    Symbol { name: "<->", content: "↔", operator: true, big: false },
    Symbol { name: "|->", content: "↦", operator: true, big: false },
    Symbol { name: "->", content: "→", operator: true, big: false },
    Symbol { name: "<-", content: "←", operator: true, big: false },
    Symbol { name: "=>", content: "⇒", operator: true, big: false },
    Symbol { name: "<=", content: "≤", operator: true, big: false },
    Symbol { name: ">=", content: "≥", operator: true, big: false },
    Symbol { name: "!=", content: "≠", operator: true, big: false },
    Symbol { name: "~=", content: "≅", operator: true, big: false },
    Symbol { name: "-=", content: "≡", operator: true, big: false },
    Symbol { name: "!in", content: "∉", operator: true, big: false },
    Symbol { name: "in", content: "∈", operator: true, big: false },
    Symbol { name: "sube", content: "⊆", operator: true, big: false },
    Symbol { name: "supe", content: "⊇", operator: true, big: false },
    Symbol { name: "sub", content: "⊂", operator: true, big: false },
    Symbol { name: "sup", content: "⊃", operator: true, big: false },
    Symbol { name: "uu", content: "∪", operator: true, big: false },
    Symbol { name: "nn", content: "∩", operator: true, big: false },
    Symbol { name: "and", content: "∧", operator: true, big: false },
    Symbol { name: "or", content: "∨", operator: true, big: false },
    Symbol { name: "^^", content: "∧", operator: true, big: false },
    Symbol { name: "vv", content: "∨", operator: true, big: false },
    // Arithmetic and others.
    Symbol { name: "cdots", content: "⋯", operator: true, big: false },
    Symbol { name: "xx", content: "×", operator: true, big: false },
    Symbol { name: "-:", content: "÷", operator: true, big: false },
    Symbol { name: "+-", content: "±", operator: true, big: false },
    Symbol { name: "-+", content: "∓", operator: true, big: false },
    Symbol { name: "**", content: "∗", operator: true, big: false },
    Symbol { name: "//", content: "/", operator: true, big: false },
    Symbol { name: "o+", content: "⊕", operator: true, big: false },
    Symbol { name: "ox", content: "⊗", operator: true, big: false },
    Symbol { name: "o.", content: "⊙", operator: true, big: false },
    Symbol { name: "@", content: "∘", operator: true, big: false },
    Symbol { name: "del", content: "∂", operator: true, big: false },
    Symbol { name: "grad", content: "∇", operator: true, big: false },
    Symbol { name: "oo", content: "∞", operator: true, big: false },
    Symbol { name: "prop", content: "∝", operator: true, big: false },
    Symbol { name: "...", content: "…", operator: true, big: false },
];

/// A prefix construct: something that takes the following atom as its argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Prefix {
    /// `sqrt(x)` — a radical over the argument, with no visible brackets.
    Sqrt,
    /// `root(3)(x)` — the first argument is the index.
    Root,
    /// `abs(x)` — vertical bars around the argument.
    Abs,
    /// `floor(x)` / `ceil(x)`.
    Floor,
    Ceil,
}

/// An accent to put over or under the following atom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Accent {
    above: bool,
    content: &'static str,
}

/// The accents we know.
const ACCENTS: &[(&str, Accent)] = &[
    ("obrace", Accent { above: true, content: "⏞" }),
    ("ubrace", Accent { above: false, content: "⏟" }),
    ("ddot", Accent { above: true, content: "¨" }),
    ("underline", Accent { above: false, content: "¯" }),
    ("vec", Accent { above: true, content: "→" }),
    ("hat", Accent { above: true, content: "^" }),
    ("bar", Accent { above: true, content: "¯" }),
    ("dot", Accent { above: true, content: "." }),
    ("ul", Accent { above: false, content: "¯" }),
];

// ---------------------------------------------------------------------------
// Tokens
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    /// A symbol from the table.
    Symbol { content: &'static str, operator: bool, big: bool },
    /// A number, exactly as written.
    Number(String),
    /// A single letter.
    Variable(char),
    /// An operator written as ASCII punctuation, such as `+` or `=`.
    Operator(char),
    /// An opening bracket, kept so the matching one can be found.
    Open(char),
    /// A closing bracket.
    Close(char),
    /// `|`, which is either an opening or a closing bar.
    Bar,
    /// `_` or `^`.
    Script(char),
    Slash,
    Comma,
    /// The literal contents of `text(...)`.
    Quoted(String),
    /// A prefix construct.
    Prefix(Prefix),
    /// An accent.
    Accent(Accent),
}

impl Token {
    /// Whether this token can begin an atom, which is what implicit
    /// multiplication is detected by.
    fn starts_atom(&self) -> bool {
        !matches!(
            self,
            Token::Close(_) | Token::Bar | Token::Script(_) | Token::Slash | Token::Comma
        )
    }
}

fn matching_bracket(open: char) -> char {
    match open {
        '(' => ')',
        '[' => ']',
        '{' => '}',
        other => other,
    }
}

/// Find the longest symbol whose name matches at the start of `rest`.
fn lookup(rest: &str) -> Option<&'static Symbol> {
    SYMBOLS
        .iter()
        .filter(|s| rest.starts_with(s.name))
        .max_by_key(|s| s.name.len())
}

// ---------------------------------------------------------------------------
// Tokenizing
// ---------------------------------------------------------------------------

fn tokenize(input: &str) -> Vec<Token> {
    let chars: Vec<char> = input.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];

        if c.is_whitespace() {
            i += 1;
            continue;
        }

        // `text(...)` is taken literally, brackets and all.
        if input[byte_of(&chars, i)..].starts_with("text") {
            let after = i + 4;
            let mut j = after;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            if chars.get(j) == Some(&'(')
                && let Some(end) = chars[j + 1..].iter().position(|c| *c == ')')
            {
                tokens.push(Token::Quoted(chars[j + 1..j + 1 + end].iter().collect()));
                i = j + 1 + end + 1;
                continue;
            }
        }

        // Prefix constructs.
        let rest = &input[byte_of(&chars, i)..];
        if let Some((name, prefix)) = [
            ("sqrt", Prefix::Sqrt),
            ("root", Prefix::Root),
            ("abs", Prefix::Abs),
            ("floor", Prefix::Floor),
            ("ceil", Prefix::Ceil),
        ]
        .iter()
        .find(|(name, _)| rest.starts_with(name))
        {
            tokens.push(Token::Prefix(*prefix));
            i += name.chars().count();
            continue;
        }

        // Accents.
        if let Some((name, accent)) = ACCENTS.iter().find(|(name, _)| rest.starts_with(name)) {
            tokens.push(Token::Accent(*accent));
            i += name.chars().count();
            continue;
        }

        // Symbols, longest match first.
        if let Some(symbol) = lookup(rest) {
            tokens.push(Token::Symbol {
                content: symbol.content,
                operator: symbol.operator,
                big: symbol.big,
            });
            i += symbol.name.chars().count();
            continue;
        }

        if c.is_ascii_digit() {
            let mut number = String::new();
            while i < chars.len() && chars[i].is_ascii_digit() {
                number.push(chars[i]);
                i += 1;
            }
            if chars.get(i) == Some(&'.') && chars.get(i + 1).is_some_and(char::is_ascii_digit) {
                number.push('.');
                i += 1;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    number.push(chars[i]);
                    i += 1;
                }
            }
            tokens.push(Token::Number(number));
            continue;
        }

        match c {
            '(' | '[' | '{' => tokens.push(Token::Open(c)),
            ')' | ']' | '}' => tokens.push(Token::Close(c)),
            '|' => tokens.push(Token::Bar),
            '_' | '^' => tokens.push(Token::Script(c)),
            '/' => tokens.push(Token::Slash),
            ',' => tokens.push(Token::Comma),
            '+' | '-' | '=' | '<' | '>' | '!' | ':' | ';' => tokens.push(Token::Operator(c)),
            c if c.is_alphabetic() => tokens.push(Token::Variable(c)),
            c => tokens.push(Token::Operator(c)),
        }
        i += 1;
    }

    tokens
}

fn byte_of(chars: &[char], index: usize) -> usize {
    chars.iter().take(index).map(|c| c.len_utf8()).sum()
}

// ---------------------------------------------------------------------------
// Converting to MathML
// ---------------------------------------------------------------------------

/// Convert AsciiMath into MathML.
///
/// `display` marks the result as display maths, which is what centres it and
/// uses the larger operator forms.
pub fn to_mathml(asciimath: &str, display: bool) -> String {
    if asciimath.trim().is_empty() {
        return String::new();
    }
    let mut parser = Parser {
        tokens: tokenize(asciimath),
        position: 0,
    };
    let body = parser.expression();
    if display {
        format!("<math display=\"block\">{body}</math>")
    } else {
        format!("<math>{body}</math>")
    }
}

struct Parser {
    tokens: Vec<Token>,
    position: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.position)
    }

    fn next(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.position).cloned();
        self.position += 1;
        token
    }

    fn eat(&mut self, token: &Token) -> bool {
        if self.peek() == Some(token) {
            self.position += 1;
            true
        } else {
            false
        }
    }

    /// A sequence of terms joined by operators, or by nothing at all.
    fn expression(&mut self) -> String {
        let mut parts = Vec::new();

        // A leading sign, as in `-x`.
        if let Some(Token::Operator(c @ ('+' | '-'))) = self.peek() {
            let c = *c;
            self.position += 1;
            parts.push(format!("<mo>{}</mo>", escape(c)));
        }
        parts.push(self.term());

        loop {
            match self.peek() {
                // An operator written as punctuation…
                Some(Token::Operator(c)) => {
                    let c = *c;
                    self.position += 1;
                    parts.push(format!("<mo>{}</mo>", escape(c)));
                }
                // …or one from the symbol table, such as `→` or `≤`.
                Some(Token::Symbol {
                    content,
                    operator: true,
                    ..
                }) => {
                    let content = *content;
                    self.position += 1;
                    parts.push(format!("<mo>{}</mo>", escape_str(content)));
                }
                // Juxtaposition, which an invisible times keeps legible.
                Some(token) if token.starts_atom() => {
                    parts.push("<mo>\u{2062}</mo>".to_owned());
                }
                _ => break,
            }
            parts.push(self.term());
        }

        mrow(parts)
    }

    /// A term: an atom, any sub/superscripts, any function application, and any
    /// fractions it divides.
    fn term(&mut self) -> String {
        let (base, big) = self.atom();
        let mut base = self.scripts(base, big);

        // `sin(x)` is one factor, so that `sin(x)/x` puts the whole of `sin(x)`
        // over `x` rather than dividing and then multiplying.
        while let Some(Token::Open(_)) = self.peek() {
            let (argument, _) = self.atom();
            base = mrow(vec![base, argument]);
        }

        // Fractions bind more tightly than the operators around them, so that
        // `a + b/c` is `a + (b/c)`.
        while self.eat(&Token::Slash) {
            let numerator = base;
            let (denominator, big) = self.atom();
            let denominator = self.scripts(denominator, big);
            base = format!("<mfrac>{numerator}{denominator}</mfrac>");
        }

        base
    }

    /// An atom, with the flag saying whether it takes limits above and below.
    fn atom(&mut self) -> (String, bool) {
        match self.next() {
            Some(Token::Symbol { content, operator, big }) => {
                let tag = if operator { "mo" } else { "mi" };
                (format!("<{tag}>{}</{tag}>", escape_str(content)), big)
            }
            Some(Token::Number(number)) => (format!("<mn>{}</mn>", escape_str(&number)), false),
            Some(Token::Variable(c)) => (format!("<mi>{}</mi>", escape(c)), false),
            Some(Token::Operator(c)) => (format!("<mo>{}</mo>", escape(c)), false),
            Some(Token::Quoted(text)) => (format!("<mtext>{}</mtext>", escape_str(&text)), false),
            Some(Token::Open('{')) => {
                // Braces group without being displayed.
                (self.group('}'), false)
            }
            Some(Token::Open(open)) => {
                let inner = self.group(matching_bracket(open));
                (
                    format!(
                        "<mrow><mo>{}</mo>{inner}<mo>{}</mo></mrow>",
                        escape(open),
                        escape(matching_bracket(open))
                    ),
                    false,
                )
            }
            Some(Token::Bar) => {
                let inner = self.group('|');
                (
                    format!("<mrow><mo>|</mo>{inner}<mo>|</mo></mrow>"),
                    false,
                )
            }
            Some(Token::Prefix(prefix)) => (self.prefix(prefix), false),
            Some(Token::Accent(accent)) => {
                let argument = self.argument();
                let (tag, attribute) = if accent.above {
                    ("mover", "accent=\"true\"")
                } else {
                    ("munder", "accentunder=\"true\"")
                };
                (
                    format!(
                        "<{tag} {attribute}>{argument}<mo>{}</mo></{tag}>",
                        escape_str(accent.content)
                    ),
                    false,
                )
            }
            // Anything unexpected, including running out of input.
            other => (
                match other {
                    Some(token) => self.token_text(&token),
                    None => String::new(),
                },
                false,
            ),
        }
    }

    /// The argument of a prefix construct or an accent: a bracket group, with
    /// the brackets dropped, or a single atom.
    ///
    /// Scripts are deliberately *not* applied here: in `x_1^2` the `^2` belongs
    /// to `x`, not to the `1`.
    fn argument(&mut self) -> String {
        match self.peek() {
            Some(Token::Open(open)) => {
                let open = *open;
                self.position += 1;
                self.group(matching_bracket(open))
            }
            Some(Token::Bar) => {
                self.position += 1;
                self.group('|')
            }
            _ => self.atom().0,
        }
    }

    fn prefix(&mut self, prefix: Prefix) -> String {
        match prefix {
            Prefix::Sqrt => format!("<msqrt>{}</msqrt>", self.argument()),
            Prefix::Root => {
                let index = self.argument();
                let radicand = self.argument();
                format!("<mroot>{radicand}{index}</mroot>")
            }
            Prefix::Abs => format!(
                "<mrow><mo>|</mo>{}<mo>|</mo></mrow>",
                self.argument()
            ),
            Prefix::Floor => format!(
                "<mrow><mo>⌊</mo>{}<mo>⌋</mo></mrow>",
                self.argument()
            ),
            Prefix::Ceil => format!(
                "<mrow><mo>⌈</mo>{}<mo>⌉</mo></mrow>",
                self.argument()
            ),
        }
    }

    /// Parse a bracket group, consuming the closing bracket.
    fn group(&mut self, close: char) -> String {
        let mut parts = vec![self.expression()];
        while self.eat(&Token::Comma) {
            parts.push(self.expression());
        }
        if self.peek() == Some(&Token::Close(close)) {
            self.position += 1;
        }

        if parts.len() == 1 {
            parts.pop().unwrap_or_default()
        } else {
            let mut out = String::from("<mrow>");
            for (i, part) in parts.iter().enumerate() {
                if i > 0 {
                    out.push_str("<mo>,</mo>");
                }
                out.push_str(part);
            }
            out.push_str("</mrow>");
            out
        }
    }

    /// Attach any `_` and `^` to a base.
    fn scripts(&mut self, base: String, big: bool) -> String {
        let mut sub = None;
        let mut sup = None;
        loop {
            match self.peek() {
                Some(Token::Script('_')) if sub.is_none() => {
                    self.position += 1;
                    sub = Some(self.argument());
                }
                Some(Token::Script('^')) if sup.is_none() => {
                    self.position += 1;
                    sup = Some(self.argument());
                }
                _ => break,
            }
        }

        match (sub, sup) {
            (None, None) => base,
            (Some(sub), None) if big => format!("<munder>{base}{sub}</munder>"),
            (Some(sub), None) => format!("<msub>{base}{sub}</msub>"),
            (None, Some(sup)) if big => format!("<mover>{base}{sup}</mover>"),
            (None, Some(sup)) => format!("<msup>{base}{sup}</msup>"),
            (Some(sub), Some(sup)) if big => {
                format!("<munderover>{base}{sub}{sup}</munderover>")
            }
            (Some(sub), Some(sup)) => format!("<msubsup>{base}{sub}{sup}</msubsup>"),
        }
    }

    fn token_text(&self, token: &Token) -> String {
        match token {
            Token::Close(c) | Token::Open(c) => escape(*c),
            Token::Bar => "<mo>|</mo>".to_owned(),
            Token::Slash => "<mo>/</mo>".to_owned(),
            Token::Comma => "<mo>,</mo>".to_owned(),
            Token::Script(c) => escape(*c),
            _ => String::new(),
        }
    }
}

fn mrow(parts: Vec<String>) -> String {
    match parts.len() {
        1 => parts.into_iter().next().unwrap_or_default(),
        _ => format!("<mrow>{}</mrow>", parts.concat()),
    }
}

/// Escape a character for the text content of a tag.
fn escape(c: char) -> String {
    match c {
        '&' => "&amp;".to_owned(),
        '<' => "&lt;".to_owned(),
        '>' => "&gt;".to_owned(),
        '"' => "&quot;".to_owned(),
        '\'' => "&#39;".to_owned(),
        _ => c.to_string(),
    }
}

fn escape_str(text: &str) -> String {
    text.chars().map(escape).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mathml(asciimath: &str) -> String {
        to_mathml(asciimath, false)
    }

    fn contains(asciimath: &str, expected: &str) {
        let out = mathml(asciimath);
        assert!(out.contains(expected), "{asciimath} -> {out}");
    }

    #[test]
    fn empty_input_produces_nothing() {
        assert_eq!(mathml(""), "");
        assert_eq!(mathml("   "), "");
    }

    #[test]
    fn numbers_and_letters() {
        assert_eq!(mathml("x"), "<math><mi>x</mi></math>");
        assert_eq!(mathml("42"), "<math><mn>42</mn></math>");
        assert_eq!(mathml("3.5"), "<math><mn>3.5</mn></math>");
    }

    #[test]
    fn scripts() {
        assert_eq!(mathml("x^2"), "<math><msup><mi>x</mi><mn>2</mn></msup></math>");
        assert_eq!(mathml("x_1"), "<math><msub><mi>x</mi><mn>1</mn></msub></math>");
        assert_eq!(
            mathml("x_1^2"),
            "<math><msubsup><mi>x</mi><mn>1</mn><mn>2</mn></msubsup></math>"
        );
        assert_eq!(
            mathml("x^2_1"),
            "<math><msubsup><mi>x</mi><mn>1</mn><mn>2</mn></msubsup></math>"
        );
        // Grouped scripts.
        contains("x^(i+1)", "<msup><mi>x</mi><mrow>");
    }

    #[test]
    fn fractions() {
        assert_eq!(mathml("a/b"), "<math><mfrac><mi>a</mi><mi>b</mi></mfrac></math>");
        // The fraction binds tighter than the surrounding operators.
        contains("a + b/c", "<mfrac><mi>b</mi><mi>c</mi></mfrac>");
        // Fractions are left-associative.
        assert_eq!(
            mathml("a/b/c"),
            "<math><mfrac><mfrac><mi>a</mi><mi>b</mi></mfrac><mi>c</mi></mfrac></math>"
        );
    }

    #[test]
    fn roots() {
        assert_eq!(mathml("sqrt(x)"), "<math><msqrt><mi>x</mi></msqrt></math>");
        // The brackets are grouping, not displayed.
        contains("sqrt(a+b)", "<msqrt><mrow>");
        // `root` takes the index first, then the radicand.
        assert_eq!(
            mathml("root(3)(x)"),
            "<math><mroot><mi>x</mi><mn>3</mn></mroot></math>"
        );
    }

    #[test]
    fn big_operators_take_limits_above_and_below() {
        contains("sum_(i=1)^n i", "<munderover><mo>∑</mo>");
        contains("prod_(i=1)^n i", "<munderover><mo>∏</mo>");
        contains("lim_(x->0) f", "<munder><mi>lim</mi>");
        // An integral keeps its limits at the side, as it is usually written.
        contains("int_0^1 x dx", "<msubsup><mo>∫</mo>");
    }

    #[test]
    fn brackets_are_kept_and_group() {
        assert_eq!(
            mathml("(a+b)"),
            "<math><mrow><mo>(</mo><mrow><mi>a</mi><mo>+</mo><mi>b</mi></mrow><mo>)</mo></mrow></math>"
        );
        // Braces group without being displayed.
        assert_eq!(mathml("{x}"), "<math><mi>x</mi></math>");
        contains("f(x)", "<mrow><mo>(</mo><mi>x</mi><mo>)</mo></mrow>");
        contains("|x|", "<mo>|</mo>");
    }

    #[test]
    fn accents_use_the_standard_attributes() {
        contains("vec v", "<mover accent=\"true\"><mi>v</mi><mo>→</mo></mover>");
        contains("hat x", "<mover accent=\"true\">");
        contains("bar x", "<mover accent=\"true\">");
        contains("ul u", "<munder accentunder=\"true\">");
    }

    #[test]
    fn function_application_binds_tighter_than_a_fraction() {
        // `sin(x)/x` is sin(x) over x, not sin times (x/x).
        let out = mathml("sin(x)/x");
        assert!(
            out.contains(
                "<mfrac><mrow><mi>sin</mi><mrow><mo>(</mo><mi>x</mi><mo>)</mo></mrow></mrow><mi>x</mi></mfrac>"
            ),
            "{out}"
        );
    }

    #[test]
    fn operators_are_not_glued_to_their_operands() {
        assert_eq!(
            mathml("x <= y"),
            "<math><mrow><mi>x</mi><mo>≤</mo><mi>y</mi></mrow></math>"
        );
        assert_eq!(
            mathml("f: A -> B"),
            "<math><mrow><mi>f</mi><mo>:</mo><mi>A</mi><mo>→</mo><mi>B</mi></mrow></math>"
        );
        // A leading sign is a prefix, not a multiplication.
        assert_eq!(mathml("-x"), "<math><mrow><mo>-</mo><mi>x</mi></mrow></math>");
    }

    #[test]
    fn functions_are_upright_and_juxtaposition_keeps_its_spacing() {
        contains("sin(2x) + 3", "<mi>sin</mi>");
        contains("2x", "<mo>\u{2062}</mo>");
    }

    #[test]
    fn text_is_literal() {
        assert_eq!(
            mathml("text(hello world)"),
            "<math><mtext>hello world</mtext></math>"
        );
    }

    #[test]
    fn greek_is_plain_and_correct() {
        // These are the letters a buggy table gets wrong most often.
        for (name, letter) in [
            ("alpha", "α"),
            ("sigma", "σ"),
            ("tau", "τ"),
            ("upsilon", "υ"),
            ("phi", "φ"),
            ("chi", "χ"),
            ("psi", "ψ"),
            ("omega", "ω"),
            ("omicron", "ο"),
            ("delta", "δ"),
            ("Omega", "Ω"),
            ("Sigma", "Σ"),
        ] {
            let out = mathml(name);
            assert!(
                out.contains(&format!("<mi>{letter}</mi>")),
                "{name} should be {letter}: {out}"
            );
        }
        // …and no mathematical-alphanumeric codepoints, which need a maths font.
        let out = mathml("alpha sigma Omega");
        assert!(
            !out.chars().any(|c| (0x1D400..=0x1D7FF).contains(&(c as u32))),
            "{out}"
        );
    }

    #[test]
    fn operators() {
        contains("x <= y", "<mo>≤</mo>");
        contains("a != b", "<mo>≠</mo>");
        contains("f: A -> B", "<mo>→</mo>");
        contains("x in RR", "<mo>∈</mo>");
        contains("RR", "<mi>ℝ</mi>");
        contains("grad f", "<mo>∇</mo>");
        contains("del x", "<mo>∂</mo>");
        contains("oo", "<mo>∞</mo>");
        // Longest match wins: `int` is the integral, not `in` then `t`.
        contains("int", "<mo>∫</mo>");
        contains("delta", "<mi>δ</mi>");
    }

    #[test]
    fn escapes_text_content() {
        // A bare `<` must not survive into the markup…
        assert_eq!(
            mathml("a < b"),
            "<math><mrow><mi>a</mi><mo>&lt;</mo><mi>b</mi></mrow></math>"
        );
        assert!(mathml("a > b").contains("<mo>&gt;</mo>"));
        // …and neither must anything inside `text()`.
        assert_eq!(
            mathml("text(a < b & c)"),
            "<math><mtext>a &lt; b &amp; c</mtext></math>"
        );
    }

    #[test]
    fn input_cannot_inject_markup() {
        let out = mathml("text(<script>alert(1)</script>)");
        assert!(!out.contains("<script"), "{out}");
        assert!(out.contains("&lt;script&gt;"), "{out}");
        let out = mathml("x <img src=x onerror=alert(1)>");
        assert!(!out.contains("<img"), "{out}");
    }

    #[test]
    fn display_maths_is_marked() {
        assert!(to_mathml("x", true).starts_with("<math display=\"block\">"));
        assert!(to_mathml("x", true).ends_with("</math>"));
        assert!(!mathml("x").contains("display"));
    }

    #[test]
    fn output_is_balanced_for_awkward_input() {
        for input in [
            "a < b",
            "sqrt(x)/(1-y)",
            "sum_(i=1)^n i^2",
            "(unclosed",
            "x_",
            "^^^",
            "text(",
            "/",
            "|a",
            "root(2)(x)+abs(y)",
        ] {
            let out = mathml(input);
            assert_eq!(
                out.matches('<').count(),
                out.matches('>').count(),
                "unbalanced for {input}: {out}"
            );
            assert!(!out.contains("<<"), "bare < for {input}: {out}");
        }
    }
}
