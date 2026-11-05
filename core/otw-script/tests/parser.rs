use otw_script::ast::{ExprKind, StmtKind};
use otw_script::diag::DiagCode;
use otw_script::parser::parse;

/// Parse `//@version=6\nx = <expr>` and render the expression as an s-expression.
fn expr(src: &str) -> String {
    let script = parse(&format!("//@version=6\nx = {src}")).unwrap_or_else(|e| panic!("{e:?}"));
    match &script.body[0].kind {
        StmtKind::Decl { value, .. } => value.to_sexpr(),
        other => panic!("not a decl: {other:?}"),
    }
}

#[test]
fn precedence() {
    assert_eq!(expr("a or b and c == d + e * -f[1]"), "(or a (and b (== c (+ d (* e (neg (idx f 1)))))))");
    assert_eq!(expr("a - b - c"), "(- (- a b) c)");
    assert_eq!(expr("not a < b"), "(< (not a) b)"); // Pine table: `not` is unary, like `-`
    assert_eq!(expr("(a + b) * c % 2"), "(% (* (+ a b) c) 2)");
}

#[test]
fn ternary_right_assoc() {
    assert_eq!(expr("a ? b : c ? d : e"), "(? a b (? c d e))");
    assert_eq!(expr("a > 1 ? b + 1 : na"), "(? (> a 1) (+ b 1) na)");
}

#[test]
fn named_args() {
    assert_eq!(
        expr("plot(x, title=\"t\", color=color.red)"),
        "(call plot x title=\"t\" color=color.red)"
    );
}

#[test]
fn generic_call() {
    assert_eq!(expr("array.new<float>(0)"), "(call array.new<float> 0)");
    assert_eq!(expr("map.new<string, int>()"), "(call map.new<string,int>)");
}

#[test]
fn history_of_expr() {
    assert_eq!(expr("(a + b)[2]"), "(idx (+ a b) 2)");
    assert_eq!(expr("ta.sma(close, 3)[1]"), "(idx (call ta.sma close 3) 1)");
}

#[test]
fn method_call() {
    // A dotted call stays a `Call`; the checker decides namespace vs method on a variable.
    assert_eq!(expr("x.method(1)"), "(call x.method 1)");
    // A call on a non-name receiver is a `MethodCall`.
    assert_eq!(expr("f(1).get(0)"), "(mcall (call f 1) get 0)");
}

#[test]
fn literals() {
    assert_eq!(expr("#FF0000"), "#FF0000FF");
    assert_eq!(expr("na(x)"), "(call na x)");
    assert_eq!(expr("true"), "true");
    assert_eq!(expr("1.5"), "1.5");
}

#[test]
fn tuple_literal() {
    let script = parse("//@version=6\nf() => [1, 2]").unwrap();
    match &script.body[0].kind {
        StmtKind::FuncDef { body, .. } => match &body.stmts[0].kind {
            StmtKind::Expr(e) => assert!(matches!(e.kind, ExprKind::Tuple(_))),
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

#[test]
fn version_required() {
    assert_eq!(parse("x = 1").unwrap_err()[0].code, DiagCode::VersionMissing);
    let e = parse("//@version=4\nx = 1").unwrap_err();
    assert_eq!(e[0].code, DiagCode::VersionUnsupported);
    assert_eq!(e[0].args, vec!["4".to_string()]);
    assert!(parse("//@version=5\nx = 1").is_ok());
}

#[test]
fn syntax_error_reported() {
    let e = parse("//@version=6\nx = (1 + \ny = 2").unwrap_err();
    assert_eq!(e[0].code, DiagCode::Syntax);
}

// ── Statements (Task 3) ──

use otw_script::ast::{AssignOp, DeclMode, Script};

fn ok(src: &str) -> Script {
    parse(&format!("//@version=6\n{src}")).unwrap_or_else(|e| panic!("{e:?}"))
}

#[test]
fn decl_forms() {
    let s = ok("float x = na\nvar int c = 0\nvarip y = 1\nvar array<float> a = array.new<float>()\nseries float z = close\n[m, s, h] = ta.macd(close, 12, 26, 9)");
    let k: Vec<_> = s.body.iter().map(|st| &st.kind).collect();
    assert!(matches!(k[0], StmtKind::Decl { mode: DeclMode::Plain, ty: Some(t), name, .. } if t.name == "float" && name == "x"));
    assert!(matches!(k[1], StmtKind::Decl { mode: DeclMode::Var, ty: Some(t), name, .. } if t.name == "int" && name == "c"));
    assert!(matches!(k[2], StmtKind::Decl { mode: DeclMode::Varip, ty: None, .. }));
    assert!(matches!(k[3], StmtKind::Decl { ty: Some(t), .. } if t.render() == "array<float>"));
    assert!(matches!(k[4], StmtKind::Decl { ty: Some(t), name, .. } if t.name == "float" && name == "z"));
    assert!(matches!(k[5], StmtKind::TupleDecl { names, .. } if names == &["m", "s", "h"]));
}

#[test]
fn assign_ops() {
    let s = ok("x = 0\nx := 1\nx += 2\nx -= 1\nx *= 3\nx /= 2\nx %= 2\nobj.f := 3");
    let ops: Vec<_> = s.body[1..].iter().map(|st| match &st.kind {
        StmtKind::Assign { op, target, .. } => (*op, target.join(".")),
        o => panic!("{o:?}"),
    }).collect();
    assert_eq!(ops, vec![
        (AssignOp::Reassign, "x".into()), (AssignOp::Add, "x".into()), (AssignOp::Sub, "x".into()),
        (AssignOp::Mul, "x".into()), (AssignOp::Div, "x".into()), (AssignOp::Mod, "x".into()),
        (AssignOp::Reassign, "obj.f".into()),
    ]);
}

#[test]
fn if_else_if_chain_as_expression() {
    let s = ok("r = if a\n    1\nelse if b\n    2\nelse\n    3\nplot(r)");
    match &s.body[0].kind {
        StmtKind::Decl { value, .. } => match &value.kind {
            ExprKind::If(i) => {
                assert_eq!(i.elifs.len(), 1);
                assert!(i.els.is_some());
            }
            o => panic!("{o:?}"),
        },
        o => panic!("{o:?}"),
    }
    assert_eq!(s.body.len(), 2);
}

#[test]
fn if_statement_nested() {
    let s = ok("if a\n    if b\n        x := 1\n    else\n        x := 2\ny = 1");
    assert_eq!(s.body.len(), 2);
}

#[test]
fn switch_both_forms() {
    let s = ok("s = switch m\n    \"a\" => 1\n    \"b\" =>\n        z = 2\n        z\n    => 0\nt = switch\n    a > b => 1\n    => 2");
    for st in &s.body {
        match &st.kind {
            StmtKind::Decl { value, .. } => match &value.kind {
                ExprKind::Switch(sw) => assert!(sw.arms.last().unwrap().0.is_none()),
                o => panic!("{o:?}"),
            },
            o => panic!("{o:?}"),
        }
    }
}

#[test]
fn for_to_by() {
    let s = ok("for i = 0 to 10 by 2\n    x += i");
    match &s.body[0].kind {
        StmtKind::Expr(e) => assert!(matches!(&e.kind, ExprKind::For(f) if f.var == "i" && f.by.is_some())),
        o => panic!("{o:?}"),
    }
}

#[test]
fn for_in_tuple() {
    let s = ok("for [i, v] in arr\n    x += v\nfor v in arr\n    x += v");
    assert!(matches!(&s.body[0].kind, StmtKind::Expr(e) if matches!(&e.kind, ExprKind::ForIn(f) if f.idx.as_deref() == Some("i") && f.item == "v")));
    assert!(matches!(&s.body[1].kind, StmtKind::Expr(e) if matches!(&e.kind, ExprKind::ForIn(f) if f.idx.is_none())));
}

#[test]
fn while_break_continue() {
    let s = ok("while x < 10\n    x += 1\n    if x == 5\n        continue\n    if x == 8\n        break");
    match &s.body[0].kind {
        StmtKind::Expr(e) => assert!(matches!(&e.kind, ExprKind::While(w) if w.body.stmts.len() == 3)),
        o => panic!("{o:?}"),
    }
}

#[test]
fn func_single_and_block() {
    let s = ok("f(x) => x * 2\ng(x) =>\n    y = x + 1\n    y * 2\nplot(g(f(close)))");
    assert!(matches!(&s.body[0].kind, StmtKind::FuncDef { body, .. } if body.stmts.len() == 1));
    assert!(matches!(&s.body[1].kind, StmtKind::FuncDef { body, .. } if body.stmts.len() == 2));
    assert_eq!(s.body.len(), 3);
}

#[test]
fn func_default_param() {
    let s = ok("f(x, n = 14, series float src = close, simple int len) => x");
    match &s.body[0].kind {
        StmtKind::FuncDef { params, .. } => {
            assert_eq!(params.len(), 4);
            assert!(params[1].default.is_some());
            assert_eq!(params[2].ty.as_ref().unwrap().name, "float");
            assert_eq!(params[3].name, "len");
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn type_method_import_parse() {
    let s = ok("import TradingView/ta/7 as tv\ntype Pt\n    float x = 0.0\n    int y\nexport method m(Pt this, int k) =>\n    this.x + k\nenum E\n    a\n    b = \"B\"\nexport f(x) => x");
    assert!(matches!(&s.body[0].kind, StmtKind::Import { path, alias } if path == "TradingView/ta/7" && alias.as_deref() == Some("tv")));
    assert!(matches!(&s.body[1].kind, StmtKind::TypeDef { name, .. } if name == "Pt"));
    assert!(matches!(&s.body[2].kind, StmtKind::MethodDef { name, export: true, .. } if name == "m"));
    assert!(matches!(&s.body[3].kind, StmtKind::EnumDef { name, .. } if name == "E"));
    assert!(matches!(&s.body[4].kind, StmtKind::FuncDef { export: true, .. }));
}

#[test]
fn indentation_error() {
    let e = parse("//@version=6\nif a\n   x := 1").unwrap_err();
    assert!(e.iter().any(|d| d.code == DiagCode::Indentation || d.code == DiagCode::Syntax), "{e:?}");
}

#[test]
fn stray_indentation_terminates_with_error() {
    // An indented line that opens no block: error, never a hang.
    let e = parse("//@version=6\nx = 1\n        y = 2\nz = 3").unwrap_err();
    assert!(!e.is_empty());
}

#[test]
fn comma_separated_statements() {
    let s = ok("a = 1, b = 2\nc = 3");
    assert_eq!(s.body.len(), 3);
    let s = ok("st = switch m\n    'Dashed' => 1 ,\n    => 2\nplot(st)");
    assert_eq!(s.body.len(), 2);
}

#[test]
fn error_at_block_end_terminates() {
    // Truncated switch arm with a stray token: error, never a hang.
    assert!(parse("//@version=6\ns = switch m\n    'a' => 1 )").is_err());
}

// ── Corpus regressions (minimised constructs) ──

#[test]
fn tuple_after_if_block_is_not_history() {
    let s = ok("f() =>\n    if a\n        b := 1\n    [a, b]\nplot(close)");
    assert_eq!(s.body.len(), 2);
}

#[test]
fn bracket_array_type_syntax() {
    let s = ok("var int[] ix = array.new_int()\nf(string[] parts, var float[] px) =>\n    string[] p = parts\n    p");
    assert!(matches!(&s.body[0].kind, StmtKind::Decl { ty: Some(t), .. } if t.render() == "array<int>"));
}

#[test]
fn return_typed_function() {
    let s = ok("float rate(int x, int n) =>\n    n > 0 ? 100.0 * x / n : na");
    assert!(matches!(&s.body[0].kind, StmtKind::FuncDef { name, .. } if name == "rate"));
}

#[test]
fn contextual_keywords_as_names() {
    let s = ok("calc(source, length, type) =>\n    switch type\n        'SMA' => ta.sma(source, length)\nadr(simple int length, simple int method = 1) => method");
    assert_eq!(s.body.len(), 2);
}

#[test]
fn typed_for_variable() {
    let s = ok("for int i = 1 to 3\n    x += i");
    assert!(matches!(&s.body[0].kind, StmtKind::Expr(e) if matches!(&e.kind, ExprKind::For(f) if f.var == "i")));
}

#[test]
fn trailing_operator_continues_line() {
    let s = ok("go =\n    (a and b) or\n    (c and d)\nplot(close)");
    assert_eq!(s.body.len(), 2);
}

#[test]
fn arm_body_with_comma_separated_statements() {
    let s = ok("switch c\n    true => f(1),\n             g(2)\n    =>       h(3),\n             k(4)\nplot(close)");
    match &s.body[0].kind {
        StmtKind::Expr(e) => match &e.kind {
            ExprKind::Switch(sw) => {
                assert_eq!(sw.arms.len(), 2);
                assert_eq!(sw.arms[0].1.stmts.len(), 2);
                assert_eq!(sw.arms[1].1.stmts.len(), 2);
            }
            o => panic!("{o:?}"),
        },
        o => panic!("{o:?}"),
    }
}
