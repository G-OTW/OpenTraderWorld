//! Checked syntax tree → `Program`: names resolved to frame slots, builtins to native
//! functions, every block and function body to a template the VM instantiates per execution
//! context (block, call site).

use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;

use crate::ast::*;
use crate::check::{key_of, types, Checked, Resolved, StmtKey};
use crate::diag::{DiagCode, Diagnostic, Span};
use crate::input::InputValue;
use crate::output::{InputKind, InputMeta, ScriptMeta};
use crate::registry::{lookup_call, Lookup, PTy};
use crate::stdlib::{self, BVar, Native, OutKind};
use crate::vm::value::Value;

/// A variable: `up` frames above the current one, slot in that frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VarRef {
    pub up: usize,
    pub slot: usize,
}

/// A nested block: its child slot in the current instance and its template.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockRef {
    pub child: usize,
    pub tmpl: usize,
}

pub enum CExpr {
    Lit(Value),
    Var(VarRef),
    VarIndex { var: VarRef, off: Box<CExpr>, span: Span },
    Bar { var: BVar, span: Span },
    BarIndex { var: BVar, off: Box<CExpr>, span: Span },
    /// History of a computed expression, kept in the frame's `hist` slot.
    Index { base: Box<CExpr>, off: Box<CExpr>, hist: usize, span: Span },
    Neg(Box<CExpr>),
    Not(Box<CExpr>),
    Bin { op: BinOp, l: Box<CExpr>, r: Box<CExpr>, int_div: bool },
    And { l: Box<CExpr>, r: Box<CExpr>, lazy: bool },
    Or { l: Box<CExpr>, r: Box<CExpr>, lazy: bool },
    Ternary { c: Box<CExpr>, a: Box<CExpr>, b: Box<CExpr> },
    Tuple(Vec<CExpr>),
    Native { f: Native, args: Vec<CExpr>, state: Option<usize>, span: Span },
    /// Evaluated once per run, then reused (inputs other than `input.source`).
    Once { slot: usize, e: Box<CExpr> },
    Output { kind: OutKind, index: usize, args: Vec<Option<CExpr>>, span: Span },
    Call { func: usize, child: usize, args: Vec<CExpr>, span: Span },
    If { branches: Vec<(Option<CExpr>, BlockRef)> },
    Switch { key: Option<Box<CExpr>>, arms: Vec<(Option<CExpr>, BlockRef)> },
    For { var: usize, from: Box<CExpr>, to: Box<CExpr>, by: Option<Box<CExpr>>, body: BlockRef, span: Span },
    While { cond: Box<CExpr>, body: BlockRef, span: Span },
    /// `for x in a` / `for [i, x] in a` over an array.
    ForIn { idx: Option<usize>, item: usize, iter: Box<CExpr>, body: BlockRef, span: Span },
    /// `T.new(…)`: one value per field of type `ty`.
    New { ty: usize, args: Vec<CExpr> },
    /// `T.copy(o)`: a shallow copy.
    CopyObj(Box<CExpr>),
    /// `o.field`.
    Field { obj: Box<CExpr>, name: Rc<str>, span: Span },
}

pub enum CStmt {
    Decl { slot: usize, persistent: bool, value: CExpr },
    TupleDecl { slots: Vec<Option<usize>>, persistent: bool, value: CExpr },
    Assign { var: VarRef, op: AssignOp, value: CExpr },
    /// `o.field := value` (or `+=`…): changes the object, not a variable.
    AssignField { obj: CExpr, field: Rc<str>, op: AssignOp, value: CExpr, span: Span },
    Expr(CExpr),
    Break,
    Continue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VarSpec {
    /// History kept: `Some(n)` = n executions, `None` = the run's maximum.
    pub cap: Option<usize>,
}

#[derive(Default)]
pub struct Template {
    pub vars: Vec<VarSpec>,
    pub n_states: usize,
    pub hists: Vec<Option<usize>>,
    pub n_children: usize,
    pub body: Vec<CStmt>,
    /// Native state initialisers, by state slot.
    pub state_init: Vec<Option<fn() -> stdlib::State>>,
}

pub struct FnDef {
    pub tmpl: usize,
    pub params: Vec<usize>,
}

/// A user-defined type: its field names, in declaration order.
pub struct TypeLayout {
    pub name: String,
    pub fields: Vec<Rc<str>>,
}

pub struct Program {
    pub(crate) meta: ScriptMeta,
    pub(crate) warnings: Vec<Diagnostic>,
    pub(crate) version: u8,
    pub(crate) templates: Vec<Template>,
    pub(crate) fns: Vec<FnDef>,
    pub(crate) types: Vec<TypeLayout>,
    pub(crate) global: usize,
    /// Number of output calls per output group (`OutKind::group`).
    pub(crate) out_counts: [usize; OutKind::GROUPS],
    pub(crate) once_slots: usize,
    /// Live labels, lines, boxes and polylines at most.
    pub(crate) draw_limits: [usize; 4],
}

impl std::fmt::Debug for Program {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Program").field("meta", &self.meta).field("templates", &self.templates.len()).finish()
    }
}

impl Program {
    pub fn meta(&self) -> &ScriptMeta {
        &self.meta
    }
    pub fn warnings(&self) -> &[Diagnostic] {
        &self.warnings
    }
}

struct Scope {
    tmpl: usize,
    names: HashMap<String, usize>,
}

struct UserFn<'a> {
    params: &'a [Param],
    id: usize,
}

struct Compiler<'a> {
    version: u8,
    dropped: &'a BTreeSet<StmtKey>,
    templates: Vec<Template>,
    fns: Vec<FnDef>,
    user_fns: HashMap<String, Vec<UserFn<'a>>>,
    scopes: Vec<Scope>,
    /// Outputs declared so far, per output group.
    out_counts: [usize; OutKind::GROUPS],
    errors: Vec<Diagnostic>,
    /// Inputs declared so far, and the variable name the next `input.*` call is assigned to.
    inputs: Vec<InputMeta>,
    input_key: Option<String>,
    /// `CExpr::Once` slots allocated.
    once: usize,
    /// User-defined types by name: index into `types` and the fields (for defaults).
    udts: HashMap<String, (usize, &'a [FieldDef])>,
    types: Vec<TypeLayout>,
    resolved: &'a HashMap<(u32, u32, u32), Resolved>,
}

fn err(code: DiagCode, span: Span, args: Vec<String>) -> Diagnostic {
    Diagnostic::error(code, span, args)
}

/// `5` or `-5`: a constant int (for v5's int division of constants).
fn const_int(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Int(_) => true,
        ExprKind::Unary { op: UnOp::Neg | UnOp::Plus, e } => const_int(e),
        _ => false,
    }
}

impl<'a> Compiler<'a> {
    fn new_template(&mut self) -> usize {
        self.templates.push(Template::default());
        self.templates.len() - 1
    }

    fn cur(&self) -> usize {
        self.scopes.last().expect("scope").tmpl
    }

    fn declare(&mut self, name: &str) -> usize {
        let t = self.cur();
        let slot = self.templates[t].vars.len();
        self.templates[t].vars.push(VarSpec { cap: Some(1) });
        if name != "_" {
            self.scopes.last_mut().expect("scope").names.insert(name.to_string(), slot);
        }
        slot
    }

    fn resolve(&self, name: &str) -> Option<(VarRef, usize)> {
        for (up, s) in self.scopes.iter().rev().enumerate() {
            if let Some(&slot) = s.names.get(name) {
                return Some((VarRef { up, slot }, s.tmpl));
            }
        }
        None
    }

    fn child(&mut self) -> usize {
        let t = self.cur();
        self.templates[t].n_children += 1;
        self.templates[t].n_children - 1
    }

    fn hist(&mut self, cap: Option<usize>) -> usize {
        let t = self.cur();
        self.templates[t].hists.push(cap);
        self.templates[t].hists.len() - 1
    }

    fn state(&mut self, init: fn() -> stdlib::State) -> usize {
        let t = self.cur();
        self.templates[t].state_init.push(Some(init));
        self.templates[t].n_states += 1;
        self.templates[t].n_states - 1
    }

    /// Compile a block into a fresh child template.
    fn block(&mut self, b: &Block) -> BlockRef {
        let child = self.child();
        let tmpl = self.new_template();
        self.scopes.push(Scope { tmpl, names: HashMap::new() });
        let body = self.stmts(&b.stmts);
        self.templates[tmpl].body = body;
        self.scopes.pop();
        BlockRef { child, tmpl }
    }

    /// The script's top level: function definitions live only here.
    fn top(&mut self, body: &'a [Stmt]) -> Vec<CStmt> {
        let mut out = Vec::new();
        for s in body {
            if self.dropped.contains(&key_of(s)) {
                continue;
            }
            if let StmtKind::FuncDef { name, params, body, .. } | StmtKind::MethodDef { name, params, body, .. } = &s.kind {
                self.func(name, params, body);
            } else if let Some(c) = self.stmt(s) {
                out.push(c);
            }
        }
        out
    }

    fn stmts(&mut self, stmts: &[Stmt]) -> Vec<CStmt> {
        let mut out = Vec::new();
        for s in stmts {
            if self.dropped.contains(&key_of(s)) {
                continue;
            }
            if let Some(c) = self.stmt(s) {
                out.push(c);
            }
        }
        out
    }

    fn stmt(&mut self, s: &Stmt) -> Option<CStmt> {
        match &s.kind {
            StmtKind::Decl { mode, name, value, .. } => {
                if matches!(&value.kind, ExprKind::Call { callee, .. } if callee[0] == "input") {
                    self.input_key = Some(name.clone());
                }
                let value = self.expr(value);
                let slot = self.declare(name);
                Some(CStmt::Decl { slot, persistent: *mode != DeclMode::Plain, value })
            }
            StmtKind::TupleDecl { names, value } => {
                let value = self.expr(value);
                let slots = names.iter().map(|n| if n == "_" { None } else { Some(self.declare(n)) }).collect();
                Some(CStmt::TupleDecl { slots, persistent: false, value })
            }
            StmtKind::Assign { target, op, value } if target.len() > 1 && self.resolve(&target[0]).is_some() => {
                let value = self.expr(value);
                let obj = self.path(&target[..target.len() - 1], s.span);
                Some(CStmt::AssignField { obj, field: Rc::from(target[target.len() - 1].as_str()), op: *op, value, span: s.span })
            }
            StmtKind::Assign { target, op, value } => {
                let value = self.expr(value);
                match self.resolve(&target[0]) {
                    Some((var, _)) => Some(CStmt::Assign { var, op: *op, value }),
                    None => {
                        self.errors.push(err(DiagCode::AssignUndeclared, s.span, vec![target.join(".")]));
                        None
                    }
                }
            }
            StmtKind::Expr(e) => {
                if let ExprKind::Call { callee, .. } = &e.kind {
                    if callee.len() == 1 && matches!(callee[0].as_str(), "indicator" | "max_bars_back") {
                        return None; // declarations read by the checker
                    }
                }
                Some(CStmt::Expr(self.expr(e)))
            }
            StmtKind::Break => Some(CStmt::Break),
            StmtKind::Continue => Some(CStmt::Continue),
            _ => None,
        }
    }

    fn func(&mut self, name: &str, params: &'a [Param], body: &'a Block) {
        let tmpl = self.new_template();
        // Lexical scope: the global frame, then the function's own frame.
        let saved = std::mem::take(&mut self.scopes);
        let global_names = saved[0].names.clone();
        self.scopes = vec![Scope { tmpl: saved[0].tmpl, names: global_names }, Scope { tmpl, names: HashMap::new() }];
        let slots = params.iter().map(|p| self.declare(&p.name)).collect();
        let body = self.stmts(&body.stmts);
        self.templates[tmpl].body = body;
        // Caps raised on global variables while compiling the body must survive.
        self.scopes = saved;
        self.fns.push(FnDef { tmpl, params: slots });
        let id = self.fns.len() - 1;
        self.user_fns.entry(name.to_string()).or_default().push(UserFn { params, id });
    }

    /// `v.f1.f2…`: a variable, then its fields.
    fn path(&mut self, path: &[String], span: Span) -> CExpr {
        let (var, _) = self.resolve(&path[0]).expect("resolved");
        let mut e = CExpr::Var(var);
        for f in &path[1..] {
            e = CExpr::Field { obj: Box::new(e), name: Rc::from(f.as_str()), span };
        }
        e
    }

    fn exprs(&mut self, es: &[Expr]) -> Vec<CExpr> {
        es.iter().map(|e| self.expr(e)).collect()
    }

    /// A registry default (`"14"`, `"high"`, `"plot.style_line"`; text for string params).
    fn default_value(&mut self, d: &str, ty: PTy, span: Span) -> CExpr {
        if d.is_empty() || (ty == PTy::Str && d != "na") {
            return CExpr::Lit(Value::str(d));
        }
        match crate::parser::parse_expr(d) {
            Ok(e) => self.expr(&Expr { span, ..e }),
            Err(_) => CExpr::Lit(Value::Na),
        }
    }

    fn not_impl(&mut self, name: &str, span: Span) -> CExpr {
        self.errors.push(err(DiagCode::Unsupported, span, vec![name.to_string(), "1".into()]));
        CExpr::Lit(Value::Na)
    }

    fn index_offset_cap(off: &Expr) -> Option<usize> {
        match &off.kind {
            ExprKind::Int(k) if *k >= 0 => Some(*k as usize + 1),
            _ => None,
        }
    }

    fn raise_cap(&mut self, tmpl: usize, slot: usize, cap: Option<usize>) {
        let spec = &mut self.templates[tmpl].vars[slot];
        spec.cap = match (spec.cap, cap) {
            (_, None) | (None, _) => None,
            (Some(a), Some(b)) => Some(a.max(b)),
        };
    }

    fn expr(&mut self, e: &Expr) -> CExpr {
        match &e.kind {
            ExprKind::Num(v) => CExpr::Lit(Value::float(*v)),
            ExprKind::Int(v) => CExpr::Lit(Value::Int(*v)),
            ExprKind::Str(s) => CExpr::Lit(Value::str(s)),
            ExprKind::Bool(b) => CExpr::Lit(Value::Bool(*b)),
            ExprKind::Color(c) => CExpr::Lit(Value::Color(stdlib::rgba_from_u32(*c))),
            ExprKind::Na => CExpr::Lit(Value::Na),
            ExprKind::Name(path) => {
                // `display.all` stays the builtin even when a variable is named `display`.
                let builtin = path.len() > 1 && matches!(crate::registry::lookup(&path.join(".")), Lookup::Supported(_));
                if self.resolve(&path[0]).is_some() && !builtin {
                    return self.path(path, e.span);
                }
                let name = path.join(".");
                if let Some(f) = stdlib::state_var(&name) {
                    let state = f.init.map(|init| self.state(init));
                    return CExpr::Native { f, args: Vec::new(), state, span: e.span };
                }
                if let Some(var) = stdlib::bar_var(&name) {
                    return CExpr::Bar { var, span: e.span };
                }
                if let Some(v) = stdlib::constant(&name) {
                    return CExpr::Lit(v);
                }
                self.not_impl(&name, e.span)
            }
            ExprKind::Index { base, offset } => {
                let off = Box::new(self.expr(offset));
                let cap = Self::index_offset_cap(offset);
                match &base.kind {
                    ExprKind::Name(path) if path.len() == 1 && self.resolve(&path[0]).is_some() => {
                        let (var, tmpl) = self.resolve(&path[0]).expect("resolved");
                        self.raise_cap(tmpl, var.slot, cap);
                        CExpr::VarIndex { var, off, span: e.span }
                    }
                    ExprKind::Name(path) if stdlib::bar_var(&path.join(".")).is_some() => {
                        CExpr::BarIndex { var: stdlib::bar_var(&path.join(".")).expect("bar var"), off, span: e.span }
                    }
                    _ => {
                        let base = Box::new(self.expr(base));
                        let hist = self.hist(cap);
                        CExpr::Index { base, off, hist, span: e.span }
                    }
                }
            }
            ExprKind::Unary { op, e: inner } => {
                let c = self.expr(inner);
                match op {
                    UnOp::Neg => CExpr::Neg(Box::new(c)),
                    UnOp::Plus => c,
                    UnOp::Not => CExpr::Not(Box::new(c)),
                }
            }
            ExprKind::Binary { op, l, r } => {
                let lazy = self.version >= 6;
                let int_div = self.version < 6 && *op == BinOp::Div && const_int(l) && const_int(r);
                let (lc, rc) = (Box::new(self.expr(l)), Box::new(self.expr(r)));
                match op {
                    BinOp::And => CExpr::And { l: lc, r: rc, lazy },
                    BinOp::Or => CExpr::Or { l: lc, r: rc, lazy },
                    _ => CExpr::Bin { op: *op, l: lc, r: rc, int_div },
                }
            }
            ExprKind::Ternary { c, a, b } => CExpr::Ternary { c: Box::new(self.expr(c)), a: Box::new(self.expr(a)), b: Box::new(self.expr(b)) },
            ExprKind::Tuple(items) => CExpr::Tuple(self.exprs(items)),
            ExprKind::Call { callee, args, generic } => self.call(callee, args, generic.as_ref(), e.span),
            ExprKind::If(i) => {
                let mut branches = Vec::new();
                let c = self.expr(&i.cond);
                let b = self.block(&i.then);
                branches.push((Some(c), b));
                for (c, b) in &i.elifs {
                    let c = self.expr(c);
                    let b = self.block(b);
                    branches.push((Some(c), b));
                }
                if let Some(b) = &i.els {
                    let b = self.block(b);
                    branches.push((None, b));
                }
                CExpr::If { branches }
            }
            ExprKind::Switch(s) => {
                let key = s.key.as_ref().map(|k| Box::new(self.expr(k)));
                let arms = s
                    .arms
                    .iter()
                    .map(|(p, b)| {
                        let p = p.as_ref().map(|p| self.expr(p));
                        (p, self.block(b))
                    })
                    .collect();
                CExpr::Switch { key, arms }
            }
            ExprKind::For(f) => {
                let from = Box::new(self.expr(&f.from));
                let to = Box::new(self.expr(&f.to));
                let by = f.by.as_ref().map(|b| Box::new(self.expr(b)));
                // The loop variable lives in the body's frame.
                let child = self.child();
                let tmpl = self.new_template();
                self.scopes.push(Scope { tmpl, names: HashMap::new() });
                let var = self.declare(&f.var);
                let body = self.stmts(&f.body.stmts);
                self.templates[tmpl].body = body;
                self.scopes.pop();
                CExpr::For { var, from, to, by, body: BlockRef { child, tmpl }, span: e.span }
            }
            ExprKind::While(w) => {
                let cond = Box::new(self.expr(&w.cond));
                let body = self.block(&w.body);
                CExpr::While { cond, body, span: e.span }
            }
            ExprKind::MethodCall { recv, name, args } => {
                let mut all = vec![Arg { name: None, value: (**recv).clone() }];
                all.extend(args.iter().cloned());
                if self.user_fns.contains_key(name) && !self.resolved.contains_key(&(e.span.line, e.span.col, e.span.len)) {
                    return self.user_call(name, &all, e.span);
                }
                self.method(name, &all, e.span)
            }
            ExprKind::ForIn(f) => {
                let iter = Box::new(self.expr(&f.iter));
                // The index and the item live in the body's frame.
                let child = self.child();
                let tmpl = self.new_template();
                self.scopes.push(Scope { tmpl, names: HashMap::new() });
                let idx = f.idx.as_ref().map(|i| self.declare(i));
                let item = self.declare(&f.item);
                let body = self.stmts(&f.body.stmts);
                self.templates[tmpl].body = body;
                self.scopes.pop();
                CExpr::ForIn { idx, item, iter, body: BlockRef { child, tmpl }, span: e.span }
            }
            ExprKind::Field { recv, name } => CExpr::Field { obj: Box::new(self.expr(recv)), name: Rc::from(name.as_str()), span: e.span },
        }
    }

    fn call(&mut self, callee: &[String], args: &[Arg], generic: Option<&TypeRef>, span: Span) -> CExpr {
        // `x.m(…)` on a variable (or a field of one): a user method, else `array.m(x, …)`.
        if callee.len() >= 2 && self.resolve(&callee[0]).is_some() && !matches!(lookup_call(&callee.join(".")), Lookup::Supported(_)) {
            let (path, m) = callee.split_at(callee.len() - 1);
            let mut all = vec![Arg { name: None, value: Expr { kind: ExprKind::Name(path.to_vec()), span } }];
            all.extend(args.iter().cloned());
            if self.user_fns.contains_key(&m[0]) && !self.resolved.contains_key(&(span.line, span.col, span.len)) {
                return self.user_call(&m[0], &all, span);
            }
            return self.method(&m[0], &all, span);
        }
        // A point form the checker picked (`label.new#point`).
        if let Some(Resolved::Builtin(b)) = self.resolved.get(&(span.line, span.col, span.len)) {
            if b.ends_with("#point") && callee.join(".") == b.trim_end_matches("#point") {
                let parts: Vec<String> = b.split('.').map(str::to_string).collect();
                return self.call(&parts, args, generic, span);
            }
        }
        // `T.new(…)` / `T.copy(o)` on a user-defined type.
        if callee.len() == 2 && self.udts.contains_key(&callee[0]) {
            let (ty, fields) = self.udts[&callee[0]];
            return match callee[1].as_str() {
                "copy" if args.len() == 1 => CExpr::CopyObj(Box::new(self.expr(&args[0].value))),
                "new" => {
                    let mut filled: Vec<Option<&Expr>> = vec![None; fields.len()];
                    for (i, a) in args.iter().filter(|a| a.name.is_none()).enumerate() {
                        if let Some(f) = filled.get_mut(i) {
                            *f = Some(&a.value);
                        }
                    }
                    for a in args.iter().filter(|a| a.name.is_some()) {
                        if let Some(i) = fields.iter().position(|f| Some(&f.name) == a.name.as_ref()) {
                            filled[i] = Some(&a.value);
                        }
                    }
                    let args = fields
                        .iter()
                        .zip(filled)
                        .map(|(f, a)| match a.or(f.default.as_ref()) {
                            Some(e) => self.expr(e),
                            None => CExpr::Lit(Value::Na),
                        })
                        .collect();
                    CExpr::New { ty, args }
                }
                _ => self.not_impl(&callee.join("."), span),
            };
        }
        // `na(float)`: a typed na.
        if callee.len() == 1 && callee[0] == "na" && args.len() == 1 {
            if let ExprKind::Name(p) = &args[0].value.kind {
                if p.len() == 1 && types::named(&p[0]).is_some() && self.resolve(&p[0]).is_none() {
                    return CExpr::Lit(Value::Na);
                }
            }
        }
        if callee.len() == 1 && self.user_fns.contains_key(&callee[0]) {
            return self.user_call(&callee[0], args, span);
        }
        let name = callee.join(".");
        let Lookup::Supported(b) = lookup_call(&name) else {
            return self.not_impl(&name, span);
        };
        let bound = match types::bind_args(b.params, b.variadic, args) {
            Ok(bound) => bound,
            Err((code, what)) => {
                self.errors.push(err(code, span, vec![what]));
                return CExpr::Lit(Value::Na);
            }
        };
        if let Some(kind) = stdlib::output(&name) {
            let index = self.out_counts[kind.group()];
            self.out_counts[kind.group()] += 1;
            let mut cargs = Vec::new();
            for (pi, slot) in bound.slots.iter().enumerate() {
                // Parameters the output does not read are never evaluated.
                if !stdlib::plot::reads(kind, pi) {
                    cargs.push(None);
                    continue;
                }
                cargs.push(match slot {
                    Some(ai) => Some(self.expr(&args[*ai].value)),
                    None => b.params[pi].default.map(|d| self.default_value(d, b.params[pi].ty, span)),
                });
            }
            return CExpr::Output { kind, index, args: cargs, span };
        }
        // `input(close)`: a source input, a series like `input.source`.
        let source_input = name == "input" && types::bind_args(b.params, b.variadic, args).ok().and_then(|bd| bd.slots[0]).is_some_and(|ai| matches!(literal(&args[ai].value), Some(InputValue::Source(_))));
        let Some(f) = stdlib::native(if source_input { "input.source" } else { &name }) else {
            return self.not_impl(&name, span);
        };
        let input_key = (callee[0] == "input").then(|| self.input_meta(&name, b.params, &bound, args));
        let mut cargs = Vec::new();
        for (pi, slot) in bound.slots.iter().enumerate() {
            cargs.push(match slot {
                Some(ai) => self.expr(&args[*ai].value),
                None => match b.params[pi].default {
                    Some(d) => self.default_value(d, b.params[pi].ty, span),
                    None => CExpr::Lit(Value::Na),
                },
            });
        }
        for &ai in &bound.extra {
            cargs.push(self.expr(&args[ai].value));
        }
        if name == "array.new" {
            let elem = generic.and_then(|g| g.args.first()).map(|t| t.name.clone()).unwrap_or_default();
            cargs.push(CExpr::Lit(Value::str(&elem)));
        }
        if let Some(key) = input_key {
            cargs.push(CExpr::Lit(Value::str(&key)));
        }
        let state = f.init.map(|init| self.state(init));
        let call = CExpr::Native { f, args: cargs, state, span };
        if callee[0] == "input" && name != "input.source" && !source_input {
            self.once += 1;
            return CExpr::Once { slot: self.once - 1, e: Box::new(call) };
        }
        call
    }

    /// `recv.name(…)` (receiver first in `all`): the builtin the checker resolved (a drawing's
    /// method), a run-time pick by drawing kind, else `array.name`.
    fn method(&mut self, name: &str, all: &[Arg], span: Span) -> CExpr {
        match self.resolved.get(&(span.line, span.col, span.len)).cloned() {
            Some(Resolved::Builtin(b)) => {
                let parts: Vec<String> = b.splitn(2, '.').map(str::to_string).collect();
                self.call(&parts, all, None, span)
            }
            Some(Resolved::Dispatch(b)) => {
                let parts: Vec<String> = b.splitn(2, '.').map(str::to_string).collect();
                match self.call(&parts, all, None, span) {
                    CExpr::Native { mut args, state, span, .. } => {
                        args.push(CExpr::Lit(Value::str(name)));
                        CExpr::Native { f: stdlib::draw::dispatch(), args, state, span }
                    }
                    other => other,
                }
            }
            None => self.call(&["array".to_string(), name.to_string()], all, None, span),
        }
    }

    /// Record an input's metadata from its literal arguments; returns its key.
    fn input_meta(&mut self, name: &str, params: &[crate::registry::Param], bound: &types::Bound, args: &[Arg]) -> String {
        let arg = |p: &str| -> Option<&Expr> {
            let pi = params.iter().position(|x| x.name == p)?;
            bound.slots[pi].map(|ai| &args[ai].value)
        };
        let default = arg("defval").and_then(literal).unwrap_or(InputValue::Float(f64::NAN));
        let kind = match name {
            "input.int" => InputKind::Int,
            "input.float" => InputKind::Float,
            "input.bool" => InputKind::Bool,
            "input.string" => InputKind::Str,
            "input.source" => InputKind::Source,
            "input.color" => InputKind::Color,
            "input.session" => InputKind::Session,
            _ => match &default {
                InputValue::Int(_) => InputKind::Int,
                InputValue::Bool(_) => InputKind::Bool,
                InputValue::Str(_) => InputKind::Str,
                InputValue::Source(_) => InputKind::Source,
                InputValue::Color(_) => InputKind::Color,
                InputValue::Float(_) => InputKind::Float,
            },
        };
        let title = match arg("title").map(|e| &e.kind) {
            Some(ExprKind::Str(t)) => Some(t.clone()),
            _ => None,
        };
        let key = self.input_key.take().or_else(|| title.clone()).unwrap_or_else(|| format!("input{}", self.inputs.len() + 1));
        let num = |e: Option<&Expr>| match e.and_then(literal) {
            Some(InputValue::Int(i)) => Some(i as f64),
            Some(InputValue::Float(f)) => Some(f),
            _ => None,
        };
        let options = match arg("options").map(|e| &e.kind) {
            Some(ExprKind::Tuple(items)) => items
                .iter()
                .filter_map(literal)
                .map(|v| match v {
                    InputValue::Str(s) => s,
                    InputValue::Int(i) => i.to_string(),
                    InputValue::Float(f) => f.to_string(),
                    other => format!("{other:?}"),
                })
                .collect(),
            _ => Vec::new(),
        };
        self.inputs.push(InputMeta {
            key: key.clone(),
            title: title.unwrap_or_else(|| key.clone()),
            kind,
            default,
            minval: num(arg("minval")),
            maxval: num(arg("maxval")),
            step: num(arg("step")),
            options,
        });
        key
    }

    fn user_call(&mut self, name: &str, args: &[Arg], span: Span) -> CExpr {
        let positional: Vec<&Arg> = args.iter().filter(|a| a.name.is_none()).collect();
        let overloads: Vec<(usize, &'a [Param])> = self.user_fns[name].iter().map(|f| (f.id, f.params)).collect();
        for (id, params) in overloads {
            if positional.len() > params.len() {
                continue;
            }
            let mut filled: Vec<Option<&Expr>> = vec![None; params.len()];
            for (i, a) in positional.iter().enumerate() {
                filled[i] = Some(&a.value);
            }
            let mut ok = true;
            for a in args.iter().filter(|a| a.name.is_some()) {
                match params.iter().position(|p| Some(&p.name) == a.name.as_ref()) {
                    Some(i) => filled[i] = Some(&a.value),
                    None => ok = false,
                }
            }
            if !ok || params.iter().zip(&filled).any(|(p, f)| f.is_none() && p.default.is_none()) {
                continue;
            }
            let cargs = params
                .iter()
                .zip(filled)
                .map(|(p, f)| match f {
                    Some(e) => self.expr(e),
                    None => self.expr(p.default.as_ref().expect("default")),
                })
                .collect();
            let child = self.child();
            return CExpr::Call { func: id, child, args: cargs, span };
        }
        self.errors.push(err(DiagCode::ArgCount, span, vec![name.to_string()]));
        CExpr::Lit(Value::Na)
    }
}

/// A literal argument as an input value (`14`, `-1.5`, `"A"`, `close`, `color.red`).
fn literal(e: &Expr) -> Option<InputValue> {
    Some(match &e.kind {
        ExprKind::Int(i) => InputValue::Int(*i),
        ExprKind::Num(f) => InputValue::Float(*f),
        ExprKind::Bool(b) => InputValue::Bool(*b),
        ExprKind::Str(s) => InputValue::Str(s.clone()),
        ExprKind::Color(c) => InputValue::Color(stdlib::rgba_from_u32(*c)),
        ExprKind::Unary { op: UnOp::Neg, e } => match literal(e)? {
            InputValue::Int(i) => InputValue::Int(-i),
            InputValue::Float(f) => InputValue::Float(-f),
            _ => return None,
        },
        ExprKind::Name(p) if p.len() == 1 && matches!(p[0].as_str(), "open" | "high" | "low" | "close" | "volume" | "hl2" | "hlc3" | "ohlc4" | "hlcc4") => {
            InputValue::Source(p[0].clone())
        }
        ExprKind::Name(p) => match stdlib::constant(&p.join("."))? {
            Value::Color(c) => InputValue::Color(c),
            _ => return None,
        },
        _ => return None,
    })
}

pub fn compile(script: &Script, checked: Checked) -> Result<Program, Vec<Diagnostic>> {
    let mut c = Compiler {
        version: script.version,
        dropped: &checked.dropped,
        templates: Vec::new(),
        fns: Vec::new(),
        user_fns: HashMap::new(),
        scopes: Vec::new(),
        out_counts: [0; OutKind::GROUPS],
        errors: Vec::new(),
        inputs: Vec::new(),
        input_key: None,
        once: 0,
        udts: HashMap::new(),
        // `chart.point` is every program's first type (`stdlib::draw::CHART_POINT_TY`).
        types: vec![TypeLayout { name: "chart.point".into(), fields: stdlib::draw::CHART_POINT_FIELDS.iter().map(|f| Rc::from(*f)).collect() }],
        resolved: &checked.resolved,
    };
    // Types first: a type is used by name wherever it is declared.
    for s in &script.body {
        if let StmtKind::TypeDef { name, fields, .. } = &s.kind {
            c.udts.insert(name.clone(), (c.types.len(), fields));
            c.types.push(TypeLayout { name: name.clone(), fields: fields.iter().map(|f| Rc::from(f.name.as_str())).collect() });
        }
    }
    let global = c.new_template();
    c.scopes.push(Scope { tmpl: global, names: HashMap::new() });
    let body = c.top(&script.body);
    c.templates[global].body = body;
    if !c.errors.is_empty() {
        return Err(c.errors);
    }
    let mut meta = checked.meta;
    meta.inputs = c.inputs;
    Ok(Program {
        meta,
        warnings: checked.diags.into_iter().filter(|d| !d.is_error()).collect(),
        version: script.version,
        templates: c.templates,
        fns: c.fns,
        types: c.types,
        global,
        out_counts: c.out_counts,
        once_slots: c.once,
        draw_limits: checked.draw_limits,
    })
}
