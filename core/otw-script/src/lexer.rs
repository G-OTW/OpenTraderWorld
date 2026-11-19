//! Tokens for Pine v5/v6 source.
//!
//! Layout rules (Pine): a block is indented by 4 spaces or one tab per level; a line indented by
//! a non-multiple of 4 continues the previous line; any line break inside open `(` / `[`
//! continues the line. `\r\n` is `\n`. `//@version=N` is read from comments.

use crate::diag::{DiagCode, Diagnostic, Span};

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Ident(String),
    Int(i64),
    Float(f64),
    Str(String),
    /// RRGGBBAA (AA = FF when the literal has 6 digits).
    Color(u32),
    True,
    False,
    Na,
    // Keywords. `to`, `by`, `in` stay identifiers: they are contextual in `for`.
    Var,
    Varip,
    If,
    Else,
    Switch,
    For,
    While,
    Break,
    Continue,
    And,
    Or,
    Not,
    Import,
    Export,
    Type,
    Method,
    Enum,
    // Operators and punctuation.
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    EqEq,
    NotEq,
    Lt,
    Gt,
    Le,
    Ge,
    Assign,
    ColonAssign,
    PlusAssign,
    MinusAssign,
    StarAssign,
    SlashAssign,
    PercentAssign,
    Question,
    Colon,
    Arrow,
    Comma,
    Dot,
    LParen,
    RParen,
    LBracket,
    RBracket,
    // Layout.
    Newline,
    Indent,
    Dedent,
    Eof,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: Tok,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Lexed {
    pub version: Option<u8>,
    pub tokens: Vec<Token>,
}

fn keyword(word: &str) -> Option<Tok> {
    Some(match word {
        "true" => Tok::True,
        "false" => Tok::False,
        "na" => Tok::Na,
        "var" => Tok::Var,
        "varip" => Tok::Varip,
        "if" => Tok::If,
        "else" => Tok::Else,
        "switch" => Tok::Switch,
        "for" => Tok::For,
        "while" => Tok::While,
        "break" => Tok::Break,
        "continue" => Tok::Continue,
        "and" => Tok::And,
        "or" => Tok::Or,
        "not" => Tok::Not,
        "import" => Tok::Import,
        "export" => Tok::Export,
        "type" => Tok::Type,
        "method" => Tok::Method,
        "enum" => Tok::Enum,
        _ => return None,
    })
}

/// `//@version=6`, `// @version = 5` → the number.
fn version_in_comment(comment: &str) -> Option<u8> {
    let rest = comment.trim_start_matches('/').trim_start();
    let rest = rest.strip_prefix("@version")?.trim_start();
    let rest = rest.strip_prefix('=')?.trim_start();
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

struct Lexer {
    tokens: Vec<Token>,
    errors: Vec<Diagnostic>,
    version: Option<u8>,
    depth: i32,
    /// A string literal still open at the end of the previous line: quote, text so far, start.
    open_str: Option<(char, String, Span)>,
}

/// Scan a string body from `i` (after the opening quote). Returns (index after the closing
/// quote or end of line, text, closed).
fn scan_string(chars: &[char], mut i: usize, quote: char, mut out: String) -> (usize, String, bool) {
    while i < chars.len() {
        let ch = chars[i];
        if ch == quote {
            return (i + 1, out, true);
        }
        if ch == '\\' && i + 1 < chars.len() {
            out.push(match chars[i + 1] {
                'n' => '\n',
                't' => '\t',
                'r' => '\r',
                other => other,
            });
            i += 2;
            continue;
        }
        out.push(ch);
        i += 1;
    }
    (i, out, false)
}

impl Lexer {
    fn push(&mut self, kind: Tok, line: u32, col: usize, len: usize) {
        self.tokens.push(Token { kind, span: Span { line, col: col as u32 + 1, len: len as u32 } });
    }

    fn after_dot(&self) -> bool {
        matches!(self.tokens.last(), Some(Token { kind: Tok::Dot, .. }))
    }

    /// Lex the tokens of one physical line, starting at char `start`.
    fn lex_line(&mut self, chars: &[char], start: usize, line: u32) {
        let mut i = start;
        let n = chars.len();
        while i < n {
            let c = chars[i];
            if c == ' ' || c == '\t' || c == '\u{00A0}' || c == '\u{FEFF}' {
                i += 1;
                continue;
            }
            if c == '/' && i + 1 < n && chars[i + 1] == '/' {
                let comment: String = chars[i..].iter().collect();
                if self.version.is_none() {
                    self.version = version_in_comment(&comment);
                }
                return;
            }
            if c.is_ascii_alphabetic() || c == '_' {
                let s = i;
                while i < n && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                let word: String = chars[s..i].iter().collect();
                let kind = match keyword(&word) {
                    Some(k) if !self.after_dot() => k,
                    _ => Tok::Ident(word),
                };
                self.push(kind, line, s, i - s);
                continue;
            }
            if c.is_ascii_digit() || (c == '.' && i + 1 < n && chars[i + 1].is_ascii_digit()) {
                let s = i;
                let mut float = false;
                while i < n && chars[i].is_ascii_digit() {
                    i += 1;
                }
                if i < n && chars[i] == '.' && !(i + 1 < n && (chars[i + 1].is_ascii_alphabetic() || chars[i + 1] == '_')) {
                    float = true;
                    i += 1;
                    while i < n && chars[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                if i < n && (chars[i] == 'e' || chars[i] == 'E') {
                    let mut j = i + 1;
                    if j < n && (chars[j] == '+' || chars[j] == '-') {
                        j += 1;
                    }
                    if j < n && chars[j].is_ascii_digit() {
                        float = true;
                        i = j;
                        while i < n && chars[i].is_ascii_digit() {
                            i += 1;
                        }
                    }
                }
                let text: String = chars[s..i].iter().collect();
                let kind = if float {
                    Tok::Float(text.parse().unwrap_or(f64::NAN))
                } else {
                    match text.parse::<i64>() {
                        Ok(v) => Tok::Int(v),
                        Err(_) => Tok::Float(text.parse().unwrap_or(f64::NAN)),
                    }
                };
                self.push(kind, line, s, i - s);
                continue;
            }
            if c == '"' || c == '\'' {
                let span = Span { line, col: i as u32 + 1, len: 0 };
                let (j, out, closed) = scan_string(chars, i + 1, c, String::new());
                if !closed {
                    // Left open: Pine lets a string wrap onto the next line.
                    self.open_str = Some((c, out, span));
                    return;
                }
                self.tokens.push(Token { kind: Tok::Str(out), span: Span { len: (j - i) as u32, ..span } });
                i = j;
                continue;
            }
            if c == '#' {
                let s = i;
                let mut j = i + 1;
                while j < n && chars[j].is_ascii_hexdigit() {
                    j += 1;
                }
                let hex: String = chars[i + 1..j].iter().collect();
                let ident_follows = j < n && (chars[j].is_ascii_alphanumeric() || chars[j] == '_');
                if (hex.len() == 6 || hex.len() == 8) && !ident_follows {
                    let v = u32::from_str_radix(&hex, 16).unwrap_or(0);
                    let v = if hex.len() == 6 { (v << 8) | 0xFF } else { v };
                    self.push(Tok::Color(v), line, s, j - s);
                    i = j;
                    continue;
                }
                self.errors.push(Diagnostic::error(
                    DiagCode::Syntax,
                    Span { line, col: s as u32 + 1, len: (j - s) as u32 },
                    vec!["bad color literal".into()],
                ));
                i = j;
                continue;
            }
            let two: String = chars[i..(i + 2).min(n)].iter().collect();
            let op2 = match two.as_str() {
                ":=" => Some(Tok::ColonAssign),
                "==" => Some(Tok::EqEq),
                "!=" => Some(Tok::NotEq),
                "<=" => Some(Tok::Le),
                ">=" => Some(Tok::Ge),
                "=>" => Some(Tok::Arrow),
                "+=" => Some(Tok::PlusAssign),
                "-=" => Some(Tok::MinusAssign),
                "*=" => Some(Tok::StarAssign),
                "/=" => Some(Tok::SlashAssign),
                "%=" => Some(Tok::PercentAssign),
                _ => None,
            };
            if let Some(k) = op2 {
                self.push(k, line, i, 2);
                i += 2;
                continue;
            }
            let op1 = match c {
                '+' => Some(Tok::Plus),
                '-' => Some(Tok::Minus),
                '*' => Some(Tok::Star),
                '/' => Some(Tok::Slash),
                '%' => Some(Tok::Percent),
                '<' => Some(Tok::Lt),
                '>' => Some(Tok::Gt),
                '=' => Some(Tok::Assign),
                '?' => Some(Tok::Question),
                ':' => Some(Tok::Colon),
                ',' => Some(Tok::Comma),
                '.' => Some(Tok::Dot),
                '(' => Some(Tok::LParen),
                ')' => Some(Tok::RParen),
                '[' => Some(Tok::LBracket),
                ']' => Some(Tok::RBracket),
                _ => None,
            };
            match op1 {
                Some(k) => {
                    match k {
                        Tok::LParen | Tok::LBracket => self.depth += 1,
                        Tok::RParen | Tok::RBracket => self.depth = (self.depth - 1).max(0),
                        _ => {}
                    }
                    self.push(k, line, i, 1);
                }
                None => self.errors.push(Diagnostic::error(
                    DiagCode::Syntax,
                    Span { line, col: i as u32 + 1, len: 1 },
                    vec![format!("unexpected character '{c}'")],
                )),
            }
            i += 1;
        }
    }
}

/// A line ending with a binary operator, an assignment, `?`, `:` or `,` continues on the next.
fn ends_with_operator(tokens: &[Token]) -> bool {
    matches!(
        tokens.last().map(|t| &t.kind),
        Some(
            Tok::Assign
                | Tok::ColonAssign
                | Tok::PlusAssign
                | Tok::MinusAssign
                | Tok::StarAssign
                | Tok::SlashAssign
                | Tok::PercentAssign
                | Tok::Plus
                | Tok::Minus
                | Tok::Star
                | Tok::Slash
                | Tok::Percent
                | Tok::EqEq
                | Tok::NotEq
                | Tok::Lt
                | Tok::Gt
                | Tok::Le
                | Tok::Ge
                | Tok::And
                | Tok::Or
                | Tok::Not
                | Tok::Question
                | Tok::Colon
                | Tok::Comma
        )
    )
}

/// Indentation width of a line: tab = 4 spaces. Returns (width, first non-blank char index).
fn indentation(chars: &[char]) -> (usize, usize) {
    let mut w = 0;
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            ' ' | '\u{00A0}' => w += 1,
            '\t' => w += 4,
            '\u{FEFF}' => {}
            _ => break,
        }
        i += 1;
    }
    (w, i)
}

pub fn lex(src: &str) -> Result<Lexed, Vec<Diagnostic>> {
    let src = src.replace("\r\n", "\n").replace('\r', "\n");
    let mut lx = Lexer { tokens: Vec::new(), errors: Vec::new(), version: None, depth: 0, open_str: None };
    let mut levels: Vec<usize> = vec![0];
    let mut has_line = false;

    for (ln, raw) in src.split('\n').enumerate() {
        let line = ln as u32 + 1;
        let chars: Vec<char> = raw.chars().collect();
        let (width, first) = indentation(&chars);
        if let Some((quote, text, span)) = lx.open_str.take() {
            let (j, out, closed) = scan_string(&chars, first, quote, text);
            if closed {
                lx.tokens.push(Token { kind: Tok::Str(out), span: Span { len: 1, ..span } });
                lx.lex_line(&chars, j, line);
            } else {
                lx.open_str = Some((quote, out, span));
            }
            continue;
        }
        let rest = &chars[first..];
        let blank = rest.is_empty() || (rest.len() >= 2 && rest[0] == '/' && rest[1] == '/');
        if blank {
            if !rest.is_empty() && lx.version.is_none() {
                lx.version = version_in_comment(&rest.iter().collect::<String>());
            }
            continue;
        }
        let continues = lx.depth > 0 || (has_line && (width % 4 != 0 || ends_with_operator(&lx.tokens)));
        if !continues {
            if width % 4 != 0 {
                lx.errors.push(Diagnostic::error(
                    DiagCode::Indentation,
                    Span { line, col: 1, len: first as u32 },
                    vec![width.to_string()],
                ));
            }
            if has_line {
                lx.push(Tok::Newline, line, 0, 0);
            }
            let top = *levels.last().unwrap();
            if width > top {
                levels.push(width);
                lx.push(Tok::Indent, line, 0, first);
            } else {
                while width < *levels.last().unwrap() {
                    levels.pop();
                    lx.push(Tok::Dedent, line, 0, 0);
                }
                if width != *levels.last().unwrap() {
                    lx.errors.push(Diagnostic::error(
                        DiagCode::Indentation,
                        Span { line, col: 1, len: first as u32 },
                        vec![width.to_string()],
                    ));
                }
            }
        }
        lx.lex_line(&chars, first, line);
        has_line = true;
    }
    if let Some((_, _, span)) = lx.open_str.take() {
        lx.errors.push(Diagnostic::error(DiagCode::Syntax, span, vec!["unterminated string".into()]));
    }
    let last_line = src.split('\n').count() as u32;
    if has_line {
        lx.push(Tok::Newline, last_line, 0, 0);
    }
    while levels.len() > 1 {
        levels.pop();
        lx.push(Tok::Dedent, last_line, 0, 0);
    }
    lx.push(Tok::Eof, last_line, 0, 0);

    if lx.errors.is_empty() {
        Ok(Lexed { version: lx.version, tokens: lx.tokens })
    } else {
        Err(lx.errors)
    }
}
