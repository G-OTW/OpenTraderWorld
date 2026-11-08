use otw_script::diag::DiagCode;
use otw_script::lexer::{lex, Tok};

fn kinds(src: &str) -> Vec<Tok> {
    lex(src).expect("lexes").tokens.into_iter().map(|t| t.kind).collect()
}

#[test]
fn version_annotation() {
    assert_eq!(lex("//@version=6\nx=1").unwrap().version, Some(6));
    assert_eq!(lex("// @version=5\nx=1").unwrap().version, Some(5));
    assert_eq!(lex("x=1").unwrap().version, None);
}

#[test]
fn comments_dropped() {
    assert_eq!(
        kinds("x = 1 // trailing\n// whole line\ny = 2"),
        kinds("x = 1\ny = 2"),
    );
}

#[test]
fn indent_dedent() {
    let k = kinds("if c\n    x = 1\ny = 2");
    let i = k.iter().position(|t| *t == Tok::Indent).expect("indent");
    let d = k.iter().position(|t| *t == Tok::Dedent).expect("dedent");
    assert!(i < d);
    assert_eq!(k[d + 1], Tok::Ident("y".into()));
}

#[test]
fn tab_equals_four_spaces() {
    assert_eq!(kinds("if c\n\tx = 1\ny = 2"), kinds("if c\n    x = 1\ny = 2"));
}

#[test]
fn crlf_equals_lf() {
    assert_eq!(kinds("if c\r\n    x = 1\r\ny = 2\r\n"), kinds("if c\n    x = 1\ny = 2\n"));
}

#[test]
fn continuation_line() {
    // A line indented by a non-multiple of 4 continues the previous one.
    let k = kinds("a = 1 +\n  2\nb = 3");
    let plus = k.iter().position(|t| *t == Tok::Plus).unwrap();
    assert_eq!(k[plus + 1], Tok::Int(2));
    assert!(!k.contains(&Tok::Indent));
    // Inside open brackets any line break continues.
    let k = kinds("p = f(1,\n    2)\nq = 1");
    let comma = k.iter().position(|t| *t == Tok::Comma).unwrap();
    assert_eq!(k[comma + 1], Tok::Int(2));
    assert!(!k.contains(&Tok::Indent));
}

#[test]
fn color_literal() {
    assert_eq!(kinds("c = #FF000080")[2], Tok::Color(0xFF000080));
    assert_eq!(kinds("c = #00ff00")[2], Tok::Color(0x00FF00FF));
}

#[test]
fn numbers_and_strings() {
    assert_eq!(
        kinds("a = .5 + 1.5e-3 + 1e3 + 7 + 'x\\'y' + \"q\"")
            .into_iter()
            .filter(|t| !matches!(t, Tok::Plus | Tok::Assign | Tok::Ident(_) | Tok::Newline | Tok::Eof))
            .collect::<Vec<_>>(),
        vec![
            Tok::Float(0.5),
            Tok::Float(1.5e-3),
            Tok::Float(1e3),
            Tok::Int(7),
            Tok::Str("x'y".into()),
            Tok::Str("q".into()),
        ],
    );
}

#[test]
fn keyword_after_dot_is_ident() {
    let k = kinds("t = syminfo.type");
    assert_eq!(k[4], Tok::Ident("type".into()));
}

#[test]
fn unicode_string_span() {
    let lexed = lex("z = \"é📈\" + y").unwrap();
    let y = lexed.tokens.iter().find(|t| t.kind == Tok::Ident("y".into())).unwrap();
    assert_eq!((y.span.line, y.span.col), (1, 12));
}

#[test]
fn bad_char_is_syntax_error() {
    let errs = lex("a = 1 ¤ 2").unwrap_err();
    assert_eq!(errs[0].code, DiagCode::Syntax);
    assert_eq!((errs[0].span.line, errs[0].span.col), (1, 7));
}

#[test]
fn nbsp_and_bom_are_whitespace() {
    assert_eq!(kinds("\u{FEFF}x =\u{00A0}1"), kinds("x = 1"));
}

#[test]
fn string_wrapped_across_lines() {
    // A string left open at end of line continues on the next; leading spaces are dropped.
    let k = kinds("s = \"The source, and\n     \\n\\nNote that\"\nx = 1");
    assert_eq!(k[2], Tok::Str("The source, and\n\nNote that".into()));
    assert_eq!(k[3], Tok::Newline);
}
