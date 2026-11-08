//! Recursive-descent parser over `lexer` tokens. It accepts the whole v5/v6 grammar, including
//! constructs the engine does not run yet, so the checker can name them precisely.

use crate::ast::*;
use crate::diag::{DiagCode, Diagnostic, Span};
use crate::lexer::{lex, Tok, Token};

type PResult<T> = Result<T, Diagnostic>;

/// Deepest expression or block nesting accepted. It bounds every recursive pass (checker,
/// compiler, VM), so a hostile script cannot overflow a host thread's stack.
pub const MAX_DEPTH: usize = 256;

struct Parser {
    toks: Vec<Token>,
    pos: usize,
    errors: Vec<Diagnostic>,
    depth: usize,
}

fn join(a: Span, b: Span) -> Span {
    if a.line == b.line && b.col >= a.col {
        Span { line: a.line, col: a.col, len: b.col + b.len - a.col }
    } else {
        a
    }
}

impl Parser {
    fn peek(&self) -> &Tok {
        &self.toks[self.pos.min(self.toks.len() - 1)].kind
    }
    fn peek_at(&self, k: usize) -> &Tok {
        &self.toks[(self.pos + k).min(self.toks.len() - 1)].kind
    }
    fn span(&self) -> Span {
        self.toks[self.pos.min(self.toks.len() - 1)].span
    }
    fn prev_span(&self) -> Span {
        self.toks[self.pos.saturating_sub(1)].span
    }
    fn bump(&mut self) -> Token {
        let t = self.toks[self.pos.min(self.toks.len() - 1)].clone();
        if self.pos < self.toks.len() - 1 {
            self.pos += 1;
        }
        t
    }
    fn at(&self, t: &Tok) -> bool {
        self.peek() == t
    }
    fn eat(&mut self, t: &Tok) -> bool {
        if self.at(t) {
            self.bump();
            true
        } else {
            false
        }
    }
    /// One more level of nesting; refuses past `MAX_DEPTH`.
    fn enter(&mut self) -> PResult<()> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(Diagnostic::error(DiagCode::TooDeep, self.span(), vec![MAX_DEPTH.to_string()]));
        }
        Ok(())
    }

    fn err<T>(&self, what: &str) -> PResult<T> {
        Err(Diagnostic::error(DiagCode::Syntax, self.span(), vec![format!("{what}, found {:?}", self.peek())]))
    }
    fn expect(&mut self, t: &Tok, what: &str) -> PResult<Token> {
        if self.at(t) {
            Ok(self.bump())
        } else {
            self.err(&format!("expected {what}"))
        }
    }
    fn ident(&mut self) -> PResult<String> {
        let name = match self.peek() {
            Tok::Ident(s) => s.clone(),
            Tok::Type => "type".into(),
            Tok::Method => "method".into(),
            Tok::Enum => "enum".into(),
            _ => return self.err("expected a name"),
        };
        self.bump();
        Ok(name)
    }

    /// An identifier, or a contextual keyword (`type`, `method`, `enum`) used as a name.
    fn name_at(&self, k: usize) -> bool {
        matches!(self.peek_at(k), Tok::Ident(_) | Tok::Type | Tok::Method | Tok::Enum)
    }

    /// Token count of a type at offset `k`: `T`, `a.T`, `T<…>`, `T[]`. None if not a type.
    fn type_len(&self, k: usize) -> Option<usize> {
        let mut j = k;
        if !self.name_at(j) {
            return None;
        }
        j += 1;
        while self.peek_at(j) == &Tok::Dot && self.name_at(j + 1) {
            j += 2;
        }
        if self.peek_at(j) == &Tok::Lt {
            let mut depth = 0;
            loop {
                match self.peek_at(j) {
                    Tok::Lt => depth += 1,
                    Tok::Gt => {
                        depth -= 1;
                        if depth == 0 {
                            j += 1;
                            break;
                        }
                    }
                    Tok::Ident(_) | Tok::Comma | Tok::Dot => {}
                    _ => return None,
                }
                j += 1;
            }
        }
        if self.peek_at(j) == &Tok::LBracket && self.peek_at(j + 1) == &Tok::RBracket {
            j += 2;
        }
        Some(j - k)
    }

    // ── Statements ──

    fn script(&mut self) -> Vec<Stmt> {
        self.stmts(false)
    }

    /// Statements until end of file (top level) or the block's `Dedent` (consumed).
    fn stmts(&mut self, in_block: bool) -> Vec<Stmt> {
        let mut body = Vec::new();
        loop {
            match self.peek() {
                Tok::Eof => break,
                Tok::Dedent if in_block => {
                    self.bump();
                    break;
                }
                Tok::Newline => {
                    self.bump();
                    continue;
                }
                _ => {}
            }
            let before = self.pos;
            let depth = self.depth;
            let parsed = self.stmt();
            self.depth = depth;
            match parsed {
                Ok(s) => {
                    body.push(s);
                    if !self.end_of_stmt() {
                        let e = self.err::<()>("expected end of line").unwrap_err();
                        self.errors.push(e);
                        self.sync();
                    }
                }
                Err(e) => {
                    self.errors.push(e);
                    self.sync();
                }
            }
            // Never loop without progress (a stray Dedent at top level, a token sync stops on).
            if self.pos == before {
                self.bump();
            }
        }
        body
    }

    /// After a statement: a newline, a comma (Pine allows `a = 1, b = 2`), or the statement
    /// ended with a block (its Dedent consumed).
    fn end_of_stmt(&mut self) -> bool {
        if self.eat(&Tok::Comma) {
            self.eat(&Tok::Newline);
            return true;
        }
        if self.eat(&Tok::Newline) {
            return true;
        }
        matches!(self.peek(), Tok::Eof | Tok::Dedent)
            || matches!(self.toks[self.pos.saturating_sub(1)].kind, Tok::Dedent)
    }

    /// Skip to the next statement at the current nesting level.
    fn sync(&mut self) {
        let mut depth = 0i32;
        loop {
            match self.peek() {
                Tok::Eof => return,
                Tok::Indent => depth += 1,
                Tok::Dedent => {
                    if depth == 0 {
                        return;
                    }
                    depth -= 1;
                }
                Tok::Newline if depth == 0 => {
                    self.bump();
                    return;
                }
                _ => {}
            }
            self.bump();
        }
    }

    /// `Newline Indent stmts Dedent`.
    fn block(&mut self) -> PResult<Block> {
        self.enter()?;
        let span = self.span();
        self.expect(&Tok::Newline, "a new line")?;
        self.expect(&Tok::Indent, "an indented block")?;
        let stmts = self.stmts(true);
        self.depth -= 1;
        Ok(Block { stmts, span })
    }

    /// Skip an indented block without parsing it (type and enum bodies).
    fn skip_block(&mut self) {
        if self.at(&Tok::Newline) && self.peek_at(1) == &Tok::Indent {
            self.bump();
            self.bump();
            let mut depth = 1;
            while depth > 0 && !self.at(&Tok::Eof) {
                match self.bump().kind {
                    Tok::Indent => depth += 1,
                    Tok::Dedent => depth -= 1,
                    _ => {}
                }
            }
        }
    }

    fn stmt(&mut self) -> PResult<Stmt> {
        let start = self.span();
        let mk = |kind| Ok(Stmt { kind, span: start });
        let export = self.eat(&Tok::Export);
        match self.peek().clone() {
            Tok::Import => {
                self.bump();
                let mut path = String::new();
                let mut alias = None;
                loop {
                    match self.peek().clone() {
                        Tok::Ident(w) if w == "as" => {
                            self.bump();
                            alias = Some(self.ident()?);
                        }
                        Tok::Ident(w) => {
                            self.bump();
                            path.push_str(&w);
                        }
                        Tok::Int(v) => {
                            self.bump();
                            path.push_str(&v.to_string());
                        }
                        Tok::Slash => {
                            self.bump();
                            path.push('/');
                        }
                        _ => break,
                    }
                }
                return mk(StmtKind::Import { path, alias });
            }
            Tok::Type if self.name_at(1) && matches!(self.peek_at(2), Tok::Newline | Tok::Eof) => {
                self.bump();
                let name = self.ident()?;
                let fields = self.fields()?;
                return mk(StmtKind::TypeDef { name, fields, export });
            }
            Tok::Enum if self.name_at(1) && matches!(self.peek_at(2), Tok::Newline | Tok::Eof) => {
                self.bump();
                let name = self.ident()?;
                self.skip_block();
                return mk(StmtKind::EnumDef { name, export });
            }
            Tok::Method if self.name_at(1) && self.peek_at(2) == &Tok::LParen => {
                self.bump();
                let name = self.ident()?;
                let params = self.params()?;
                self.expect(&Tok::Arrow, "'=>'")?;
                let body = self.body()?;
                return mk(StmtKind::MethodDef { name, params, body, export });
            }
            Tok::Break => {
                self.bump();
                return mk(StmtKind::Break);
            }
            Tok::Continue => {
                self.bump();
                return mk(StmtKind::Continue);
            }
            Tok::Var | Tok::Varip => {
                let mode = if self.at(&Tok::Var) { DeclMode::Var } else { DeclMode::Varip };
                self.bump();
                return self.decl(mode, start);
            }
            _ => {}
        }
        let typed_fn = self.type_len(0).filter(|&n| self.peek_at(n + 1) == &Tok::LParen && self.is_func_def_at(n));
        if let Some(n) = typed_fn {
            for _ in 0..n {
                self.bump();
            }
        }
        if typed_fn.is_some() || self.is_func_def() {
            let name = self.ident()?;
            let params = self.params()?;
            self.expect(&Tok::Arrow, "'=>'")?;
            let body = self.body()?;
            return mk(StmtKind::FuncDef { name, params, body, export });
        }
        if export {
            return self.err("expected a function, method, type or enum after 'export'");
        }
        if self.at(&Tok::LBracket) && self.tuple_decl_ahead() {
            self.bump();
            let mut names = vec![self.ident()?];
            while self.eat(&Tok::Comma) {
                names.push(self.ident()?);
            }
            self.expect(&Tok::RBracket, "']'")?;
            self.expect(&Tok::Assign, "'='")?;
            let value = self.expr()?;
            return mk(StmtKind::TupleDecl { names, value });
        }
        if let Some(op) = self.assign_ahead() {
            let mut target = vec![self.ident()?];
            while self.eat(&Tok::Dot) {
                target.push(self.ident()?);
            }
            self.bump();
            let value = self.expr()?;
            return mk(StmtKind::Assign { target, op, value });
        }
        if self.typed_decl_ahead() || (self.name_at(0) && self.peek_at(1) == &Tok::Assign) {
            return self.decl(DeclMode::Plain, start);
        }
        let e = self.expr()?;
        Ok(Stmt { span: e.span, kind: StmtKind::Expr(e) })
    }

    /// `[qualifier] [type] name = expr` after `var`/`varip` (or a plain typed declaration).
    fn decl(&mut self, mode: DeclMode, start: Span) -> PResult<Stmt> {
        self.skip_qualifier();
        let ty = if self.name_at(0) && self.peek_at(1) == &Tok::Assign {
            None
        } else {
            Some(self.type_ref()?)
        };
        let name = self.ident()?;
        self.expect(&Tok::Assign, "'='")?;
        let value = self.expr()?;
        Ok(Stmt { kind: StmtKind::Decl { mode, ty, name, value }, span: start })
    }

    fn skip_qualifier(&mut self) {
        if let Tok::Ident(w) = self.peek() {
            if matches!(w.as_str(), "series" | "simple" | "const" | "input") && matches!(self.peek_at(1), Tok::Ident(_)) {
                self.bump();
            }
        }
    }

    /// `[ a, b ] =` ahead.
    fn tuple_decl_ahead(&self) -> bool {
        let mut k = 1;
        loop {
            match self.peek_at(k) {
                Tok::Ident(_) | Tok::Comma => k += 1,
                Tok::RBracket => return self.peek_at(k + 1) == &Tok::Assign,
                _ => return false,
            }
        }
    }

    /// `name(.name)* (:= | += | …)` ahead: returns the operator.
    fn assign_ahead(&self) -> Option<AssignOp> {
        let mut k = 0;
        loop {
            if !self.name_at(k) {
                return None;
            }
            k += 1;
            if self.peek_at(k) == &Tok::Dot {
                k += 1;
                continue;
            }
            return match self.peek_at(k) {
                Tok::ColonAssign => Some(AssignOp::Reassign),
                Tok::PlusAssign => Some(AssignOp::Add),
                Tok::MinusAssign => Some(AssignOp::Sub),
                Tok::StarAssign => Some(AssignOp::Mul),
                Tok::SlashAssign => Some(AssignOp::Div),
                Tok::PercentAssign => Some(AssignOp::Mod),
                _ => None,
            };
        }
    }

    /// `[qualifier] Type[.Type][<…>][[]] name =` ahead.
    fn typed_decl_ahead(&self) -> bool {
        let mut k = 0;
        if let Tok::Ident(w) = self.peek() {
            if matches!(w.as_str(), "series" | "simple" | "const" | "input") && self.name_at(1) {
                k = 1;
            }
        }
        match self.type_len(k) {
            Some(n) => self.name_at(k + n) && self.peek_at(k + n + 1) == &Tok::Assign,
            None => false,
        }
    }

    /// `name(` … `)` `=>` at the statement start.
    fn is_func_def(&self) -> bool {
        self.is_func_def_at(0)
    }

    /// `name(` … `)` `=>` at offset `at`.
    fn is_func_def_at(&self, at: usize) -> bool {
        if !self.name_at(at) || self.peek_at(at + 1) != &Tok::LParen {
            return false;
        }
        let mut depth = 0;
        let mut k = at + 1;
        loop {
            match self.peek_at(k) {
                Tok::LParen | Tok::LBracket => depth += 1,
                Tok::RParen | Tok::RBracket => {
                    depth -= 1;
                    if depth == 0 {
                        return self.peek_at(k + 1) == &Tok::Arrow;
                    }
                }
                Tok::Eof | Tok::Newline => return false,
                _ => {}
            }
            k += 1;
        }
    }

    /// A type's indented field list: `[varip] Type name [= default]` per line.
    fn fields(&mut self) -> PResult<Vec<FieldDef>> {
        let mut out = Vec::new();
        if !(self.at(&Tok::Newline) && self.peek_at(1) == &Tok::Indent) {
            return Ok(out);
        }
        self.bump();
        self.bump();
        loop {
            while self.eat(&Tok::Newline) {}
            if self.at(&Tok::Dedent) || self.at(&Tok::Eof) {
                self.eat(&Tok::Dedent);
                return Ok(out);
            }
            let span = self.span();
            self.eat(&Tok::Varip);
            self.skip_qualifier();
            let ty = self.type_ref()?;
            let name = self.ident()?;
            let default = if self.eat(&Tok::Assign) { Some(self.expr()?) } else { None };
            out.push(FieldDef { ty, name, default, span });
            if !self.at(&Tok::Dedent) && !self.at(&Tok::Eof) {
                self.expect(&Tok::Newline, "a new line")?;
            }
        }
    }

    /// `( [qualifier] [Type] name [= default], … )`
    fn params(&mut self) -> PResult<Vec<Param>> {
        self.expect(&Tok::LParen, "'('")?;
        let mut out = Vec::new();
        while !self.at(&Tok::RParen) {
            self.eat(&Tok::Var);
            self.skip_qualifier();
            let named_here = self.name_at(0) && matches!(self.peek_at(1), Tok::Comma | Tok::RParen | Tok::Assign);
            let ty = if named_here { None } else { Some(self.type_ref()?) };
            let name = self.ident()?;
            let default = if self.eat(&Tok::Assign) { Some(self.expr()?) } else { None };
            out.push(Param { name, ty, default });
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        self.expect(&Tok::RParen, "')'")?;
        Ok(out)
    }

    /// A function/arm body: one expression on the same line, or an indented block.
    fn body(&mut self) -> PResult<Block> {
        if self.at(&Tok::Newline) {
            return self.block();
        }
        let span = self.span();
        let mut stmts = vec![self.stmt()?];
        // `=> a, b`: several statements on one (wrapped) line.
        while self.at(&Tok::Comma) && !matches!(self.peek_at(1), Tok::Newline | Tok::Dedent | Tok::Eof | Tok::Arrow) {
            self.bump();
            stmts.push(self.stmt()?);
        }
        Ok(Block { stmts, span })
    }

    // ── Control expressions ──

    fn if_expr(&mut self) -> PResult<ExprKind> {
        let cond = self.expr()?;
        let then = self.block()?;
        let mut elifs = Vec::new();
        let mut els = None;
        while self.at(&Tok::Else) {
            self.bump();
            if self.eat(&Tok::If) {
                let c = self.expr()?;
                let b = self.block()?;
                elifs.push((c, b));
            } else {
                els = Some(self.block()?);
                break;
            }
        }
        Ok(ExprKind::If(Box::new(IfExpr { cond, then, elifs, els })))
    }

    fn switch_expr(&mut self) -> PResult<ExprKind> {
        let key = if self.at(&Tok::Newline) { None } else { Some(self.expr()?) };
        self.expect(&Tok::Newline, "a new line")?;
        self.expect(&Tok::Indent, "indented switch arms")?;
        let mut arms = Vec::new();
        loop {
            match self.peek() {
                Tok::Dedent => {
                    self.bump();
                    break;
                }
                Tok::Eof => break,
                Tok::Newline => {
                    self.bump();
                    continue;
                }
                _ => {}
            }
            let before = self.pos;
            let pat = if self.at(&Tok::Arrow) { None } else { Some(self.expr()?) };
            if self.pos == before && !self.at(&Tok::Arrow) {
                return self.err("expected a switch arm");
            }
            self.expect(&Tok::Arrow, "'=>'")?;
            let body = self.body()?;
            arms.push((pat, body));
            self.end_of_stmt();
        }
        Ok(ExprKind::Switch(Box::new(SwitchExpr { key, arms })))
    }

    fn for_expr(&mut self) -> PResult<ExprKind> {
        if self.eat(&Tok::LBracket) {
            let idx = self.ident()?;
            self.expect(&Tok::Comma, "','")?;
            let item = self.ident()?;
            self.expect(&Tok::RBracket, "']'")?;
            self.contextual("in")?;
            let iter = self.expr()?;
            let body = self.block()?;
            return Ok(ExprKind::ForIn(Box::new(ForInExpr { idx: Some(idx), item, iter, body })));
        }
        let in_at = |k: usize| matches!(self.peek_at(k), Tok::Ident(w) if w == "in");
        if self.name_at(0) && self.name_at(1) && !in_at(1) && (self.peek_at(2) == &Tok::Assign || in_at(2)) {
            self.bump(); // `for int i = …`: the loop variable's type
        }
        let var = self.ident()?;
        if matches!(self.peek(), Tok::Ident(w) if w == "in") {
            self.bump();
            let iter = self.expr()?;
            let body = self.block()?;
            return Ok(ExprKind::ForIn(Box::new(ForInExpr { idx: None, item: var, iter, body })));
        }
        self.expect(&Tok::Assign, "'='")?;
        let from = self.expr()?;
        self.contextual("to")?;
        let to = self.expr()?;
        let by = if matches!(self.peek(), Tok::Ident(w) if w == "by") {
            self.bump();
            Some(self.expr()?)
        } else {
            None
        };
        let body = self.block()?;
        Ok(ExprKind::For(Box::new(ForExpr { var, from, to, by, body })))
    }

    fn contextual(&mut self, word: &str) -> PResult<()> {
        if matches!(self.peek(), Tok::Ident(w) if w == word) {
            self.bump();
            Ok(())
        } else {
            self.err(&format!("expected '{word}'"))
        }
    }

    // ── Expressions ──

    pub fn expr(&mut self) -> PResult<Expr> {
        self.enter()?;
        let r = self.ternary();
        self.depth -= 1;
        r
    }

    fn ternary(&mut self) -> PResult<Expr> {
        let c = self.or_expr()?;
        if self.eat(&Tok::Question) {
            let a = self.expr()?;
            self.expect(&Tok::Colon, "':'")?;
            let b = self.expr()?;
            let span = join(c.span, b.span);
            return Ok(Expr { kind: ExprKind::Ternary { c: Box::new(c), a: Box::new(a), b: Box::new(b) }, span });
        }
        Ok(c)
    }

    fn binary(&mut self, l: Expr, op: BinOp, r: Expr) -> Expr {
        let span = join(l.span, r.span);
        Expr { kind: ExprKind::Binary { op, l: Box::new(l), r: Box::new(r) }, span }
    }

    fn or_expr(&mut self) -> PResult<Expr> {
        let base = self.depth;
        let mut l = self.and_expr()?;
        while self.eat(&Tok::Or) {
            self.enter()?;
            let r = self.and_expr()?;
            l = self.binary(l, BinOp::Or, r);
        }
        self.depth = base;
        Ok(l)
    }

    fn and_expr(&mut self) -> PResult<Expr> {
        let base = self.depth;
        let mut l = self.eq_expr()?;
        while self.eat(&Tok::And) {
            self.enter()?;
            let r = self.eq_expr()?;
            l = self.binary(l, BinOp::And, r);
        }
        self.depth = base;
        Ok(l)
    }

    fn eq_expr(&mut self) -> PResult<Expr> {
        let base = self.depth;
        let mut l = self.cmp_expr()?;
        loop {
            let op = match self.peek() {
                Tok::EqEq => BinOp::Eq,
                Tok::NotEq => BinOp::Ne,
                _ => {
                    self.depth = base;
                    return Ok(l);
                }
            };
            self.bump();
            self.enter()?;
            let r = self.cmp_expr()?;
            l = self.binary(l, op, r);
        }
    }

    fn cmp_expr(&mut self) -> PResult<Expr> {
        let base = self.depth;
        let mut l = self.add_expr()?;
        loop {
            let op = match self.peek() {
                Tok::Lt => BinOp::Lt,
                Tok::Gt => BinOp::Gt,
                Tok::Le => BinOp::Le,
                Tok::Ge => BinOp::Ge,
                _ => {
                    self.depth = base;
                    return Ok(l);
                }
            };
            self.bump();
            self.enter()?;
            let r = self.add_expr()?;
            l = self.binary(l, op, r);
        }
    }

    fn add_expr(&mut self) -> PResult<Expr> {
        let base = self.depth;
        let mut l = self.mul_expr()?;
        loop {
            let op = match self.peek() {
                Tok::Plus => BinOp::Add,
                Tok::Minus => BinOp::Sub,
                _ => {
                    self.depth = base;
                    return Ok(l);
                }
            };
            self.bump();
            self.enter()?;
            let r = self.mul_expr()?;
            l = self.binary(l, op, r);
        }
    }

    fn mul_expr(&mut self) -> PResult<Expr> {
        let base = self.depth;
        let mut l = self.unary()?;
        loop {
            let op = match self.peek() {
                Tok::Star => BinOp::Mul,
                Tok::Slash => BinOp::Div,
                Tok::Percent => BinOp::Mod,
                _ => {
                    self.depth = base;
                    return Ok(l);
                }
            };
            self.bump();
            self.enter()?;
            let r = self.unary()?;
            l = self.binary(l, op, r);
        }
    }

    fn unary(&mut self) -> PResult<Expr> {
        if !matches!(self.peek(), Tok::Minus | Tok::Plus | Tok::Not) {
            return self.postfix();
        }
        self.enter()?;
        let r = self.unary_inner();
        self.depth -= 1;
        r
    }

    fn unary_inner(&mut self) -> PResult<Expr> {
        let start = self.span();
        let op = match self.peek() {
            Tok::Minus => UnOp::Neg,
            Tok::Plus => UnOp::Plus,
            Tok::Not => UnOp::Not,
            _ => return self.postfix(),
        };
        self.bump();
        // Pine's precedence table puts `not`, unary `-` and `+` on the same level.
        let e = self.unary()?;
        let span = join(start, e.span);
        Ok(Expr { kind: ExprKind::Unary { op, e: Box::new(e) }, span })
    }

    fn args(&mut self) -> PResult<Vec<Arg>> {
        self.expect(&Tok::LParen, "'('")?;
        let mut out = Vec::new();
        while !self.at(&Tok::RParen) {
            let name = match (self.peek().clone(), self.peek_at(1)) {
                (Tok::Ident(n), Tok::Assign) => {
                    self.bump();
                    self.bump();
                    Some(n)
                }
                _ => None,
            };
            let value = self.expr()?;
            out.push(Arg { name, value });
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        self.expect(&Tok::RParen, "')'")?;
        Ok(out)
    }

    fn postfix(&mut self) -> PResult<Expr> {
        let mut e = self.primary()?;
        if matches!(self.toks[self.pos.saturating_sub(1)].kind, Tok::Dedent) {
            return Ok(e); // `if`/`switch`/`for`/`while` blocks take no `[…]`, call or `.`
        }
        loop {
            match self.peek() {
                Tok::LBracket => {
                    self.bump();
                    let offset = self.expr()?;
                    self.expect(&Tok::RBracket, "']'")?;
                    let span = join(e.span, self.prev_span());
                    e = Expr { kind: ExprKind::Index { base: Box::new(e), offset: Box::new(offset) }, span };
                }
                Tok::LParen if matches!(e.kind, ExprKind::Name(_)) => {
                    let callee = match e.kind {
                        ExprKind::Name(p) => p,
                        _ => unreachable!(),
                    };
                    let args = self.args()?;
                    let span = join(e.span, self.prev_span());
                    e = Expr { kind: ExprKind::Call { callee, generic: None, args }, span };
                }
                Tok::Lt if matches!(e.kind, ExprKind::Name(_)) && self.generic_ahead() => {
                    let generic = self.generic()?;
                    let callee = match e.kind {
                        ExprKind::Name(p) => p,
                        _ => unreachable!(),
                    };
                    let args = self.args()?;
                    let span = join(e.span, self.prev_span());
                    e = Expr { kind: ExprKind::Call { callee, generic: Some(generic), args }, span };
                }
                Tok::Dot => {
                    self.bump();
                    let name = self.ident()?;
                    if self.at(&Tok::LParen) {
                        let args = self.args()?;
                        let span = join(e.span, self.prev_span());
                        e = Expr { kind: ExprKind::MethodCall { recv: Box::new(e), name, args }, span };
                    } else {
                        let span = join(e.span, self.prev_span());
                        e = Expr { kind: ExprKind::Field { recv: Box::new(e), name }, span };
                    }
                }
                _ => return Ok(e),
            }
        }
    }

    /// `<` Type (`,` Type)* `>` `(` ahead: a generic call, not a comparison.
    fn generic_ahead(&self) -> bool {
        let mut k = 1;
        let mut depth = 1;
        loop {
            match self.peek_at(k) {
                Tok::Lt => depth += 1,
                Tok::Gt => {
                    depth -= 1;
                    if depth == 0 {
                        return self.peek_at(k + 1) == &Tok::LParen;
                    }
                }
                Tok::Ident(_) | Tok::Comma | Tok::Dot => {}
                _ => return false,
            }
            k += 1;
        }
    }

    fn type_ref(&mut self) -> PResult<TypeRef> {
        let mut name = self.ident()?;
        while self.at(&Tok::Dot) {
            self.bump();
            name.push('.');
            name.push_str(&self.ident()?);
        }
        let mut args = Vec::new();
        if self.at(&Tok::Lt) {
            args = self.generic()?.args;
        }
        let t = TypeRef { name, args };
        if self.at(&Tok::LBracket) && self.peek_at(1) == &Tok::RBracket {
            self.bump();
            self.bump();
            return Ok(TypeRef { name: "array".into(), args: vec![t] });
        }
        Ok(t)
    }

    fn generic(&mut self) -> PResult<TypeRef> {
        self.expect(&Tok::Lt, "'<'")?;
        let mut args = vec![self.type_ref()?];
        while self.eat(&Tok::Comma) {
            args.push(self.type_ref()?);
        }
        self.expect(&Tok::Gt, "'>'")?;
        Ok(TypeRef { name: String::new(), args })
    }

    fn primary(&mut self) -> PResult<Expr> {
        let t = self.bump();
        let span = t.span;
        let kind = match t.kind {
            Tok::Int(v) => ExprKind::Int(v),
            Tok::Float(v) => ExprKind::Num(v),
            Tok::Str(s) => ExprKind::Str(s),
            Tok::True => ExprKind::Bool(true),
            Tok::False => ExprKind::Bool(false),
            Tok::Color(c) => ExprKind::Color(c),
            Tok::Na => {
                if self.at(&Tok::LParen) {
                    ExprKind::Name(vec!["na".into()])
                } else {
                    ExprKind::Na
                }
            }
            Tok::Ident(_) | Tok::Type | Tok::Method | Tok::Enum => {
                let first = match t.kind {
                    Tok::Ident(s) => s,
                    Tok::Type => "type".into(),
                    Tok::Method => "method".into(),
                    _ => "enum".into(),
                };
                let mut path = vec![first];
                let mut end = span;
                while self.at(&Tok::Dot) && self.name_at(1) {
                    self.bump();
                    end = self.span();
                    path.push(self.ident()?);
                }
                return Ok(Expr { kind: ExprKind::Name(path), span: join(span, end) });
            }
            Tok::LParen => {
                let e = self.expr()?;
                self.expect(&Tok::RParen, "')'")?;
                return Ok(e);
            }
            Tok::If => self.if_expr()?,
            Tok::Switch => self.switch_expr()?,
            Tok::For => self.for_expr()?,
            Tok::While => {
                let cond = self.expr()?;
                let body = self.block()?;
                ExprKind::While(Box::new(WhileExpr { cond, body }))
            }
            Tok::LBracket => {
                let mut items = Vec::new();
                while !self.at(&Tok::RBracket) {
                    items.push(self.expr()?);
                    if !self.eat(&Tok::Comma) {
                        break;
                    }
                }
                self.expect(&Tok::RBracket, "']'")?;
                ExprKind::Tuple(items)
            }
            _ => {
                self.pos -= 1;
                return self.err("expected an expression");
            }
        };
        Ok(Expr { kind, span })
    }
}

pub fn parse(src: &str) -> Result<Script, Vec<Diagnostic>> {
    let lexed = lex(src)?;
    let version = match lexed.version {
        None => return Err(vec![Diagnostic::error(DiagCode::VersionMissing, Span { line: 1, col: 1, len: 0 }, vec![])]),
        Some(v) if !(5..=6).contains(&v) => {
            return Err(vec![Diagnostic::error(DiagCode::VersionUnsupported, Span { line: 1, col: 1, len: 0 }, vec![v.to_string()])])
        }
        Some(v) => v,
    };
    let mut p = Parser { toks: lexed.tokens, pos: 0, errors: Vec::new(), depth: 0 };
    let body = p.script();
    if p.errors.is_empty() {
        if let Some(span) = too_deep(&body) {
            p.errors.push(Diagnostic::error(DiagCode::TooDeep, span, vec![MAX_DEPTH.to_string()]));
        }
    }
    if p.errors.is_empty() {
        Ok(Script { version, body })
    } else {
        Err(p.errors)
    }
}

/// Parse a lone expression (registry defaults such as `"high"` or `"plot.style_line"`).
pub fn parse_expr(src: &str) -> Result<Expr, Vec<Diagnostic>> {
    let lexed = lex(src)?;
    let mut p = Parser { toks: lexed.tokens, pos: 0, errors: Vec::new(), depth: 0 };
    p.expr().map_err(|e| vec![e])
}

/// The first expression nested deeper than `MAX_DEPTH` (operator chains such as `1 + 1 + …`
/// are parsed iteratively but nest the tree). Walks with an explicit stack: no recursion.
fn too_deep(body: &[Stmt]) -> Option<Span> {
    let mut work: Vec<(&Expr, usize)> = Vec::new();
    fn block(b: &Block) -> Vec<&Expr> {
        b.stmts.iter().flat_map(stmt_exprs).collect()
    }
    fn stmt_exprs(s: &Stmt) -> Vec<&Expr> {
        match &s.kind {
            StmtKind::Decl { value, .. } | StmtKind::TupleDecl { value, .. } | StmtKind::Assign { value, .. } | StmtKind::Expr(value) => vec![value],
            StmtKind::FuncDef { body, params, .. } | StmtKind::MethodDef { body, params, .. } => {
                body.stmts.iter().flat_map(stmt_exprs).chain(params.iter().filter_map(|p| p.default.as_ref())).collect()
            }
            _ => vec![],
        }
    }
    for s in body {
        for e in stmt_exprs(s) {
            work.push((e, 1));
        }
    }
    while let Some((e, d)) = work.pop() {
        if d > MAX_DEPTH {
            return Some(e.span);
        }
        let next: Vec<&Expr> = match &e.kind {
            ExprKind::Call { args, .. } => args.iter().map(|a| &a.value).collect(),
            ExprKind::MethodCall { recv, args, .. } => std::iter::once(&**recv).chain(args.iter().map(|a| &a.value)).collect(),
            ExprKind::Field { recv, .. } => vec![recv],
            ExprKind::Index { base, offset } => vec![base, offset],
            ExprKind::Unary { e, .. } => vec![e],
            ExprKind::Binary { l, r, .. } => vec![l, r],
            ExprKind::Ternary { c, a, b } => vec![c, a, b],
            ExprKind::Tuple(items) => items.iter().collect(),
            ExprKind::If(i) => {
                let mut v = vec![&i.cond];
                v.extend(block(&i.then));
                for (c, b) in &i.elifs {
                    v.push(c);
                    v.extend(block(b));
                }
                if let Some(b) = &i.els {
                    v.extend(block(b));
                }
                v
            }
            ExprKind::Switch(sw) => {
                let mut v: Vec<&Expr> = sw.key.iter().collect();
                for (p, b) in &sw.arms {
                    v.extend(p.iter());
                    v.extend(block(b));
                }
                v
            }
            ExprKind::For(f) => {
                let mut v = vec![&f.from, &f.to];
                v.extend(f.by.iter());
                v.extend(block(&f.body));
                v
            }
            ExprKind::ForIn(f) => std::iter::once(&f.iter).chain(block(&f.body)).collect(),
            ExprKind::While(w) => std::iter::once(&w.cond).chain(block(&w.body)).collect(),
            _ => vec![],
        };
        work.extend(next.into_iter().map(|x| (x, d + 1)));
    }
    None
}
