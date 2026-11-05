//! Semantic checks over the syntax tree: name resolution and scopes, the indicator declaration,
//! every unsupported feature (each occurrence), unused functions. Types live in `types`,
//! visual-only skipping and causality in `flow`.

use std::collections::{BTreeSet, HashMap};

use crate::ast::*;
use crate::diag::{DiagCode, Diagnostic, Span};
use crate::output::ScriptMeta;
use crate::registry::{lookup, lookup_call, Builtin, Lookup};

pub mod flow;
pub mod types;
use types::Ty;

/// A statement's identity: the (line, col) where it starts.
pub type StmtKey = (u32, u32);

pub fn key_of(s: &Stmt) -> StmtKey {
    (s.span.line, s.span.col)
}

/// Result of checking a script. Errors in `diags` mean the script is refused.
#[derive(Debug, Clone, PartialEq)]
pub struct Checked {
    pub diags: Vec<Diagnostic>,
    /// User functions reachable from the top level (others are skipped).
    pub reachable_fns: BTreeSet<String>,
    pub meta: ScriptMeta,
    /// Uses of visual-only builtins not run yet: (name, span, innermost statement). `flow`
    /// decides skip vs refuse.
    pub visual_uses: Vec<(String, Span, Option<StmtKey>)>,
    /// Innermost statement of each entry of `diags` (None = script level).
    pub diag_stmts: Vec<Option<StmtKey>>,
    /// Statements the run skips: they only feed drawings, alerts or nothing at all.
    pub dropped: BTreeSet<StmtKey>,
    /// Calls the compiler cannot resolve by name, by call span (line, column, length): a drawing
    /// method (`l.set_x(…)` is `label.set_x(l, …)`), a point form (`label.new#point`), or a
    /// method on a value of unknown type run by the drawing's kind (`Dispatch`).
    pub resolved: HashMap<(u32, u32, u32), Resolved>,
    /// Live labels, lines, boxes and polylines at most (`indicator(max_*_count)`, Pine's 50).
    pub draw_limits: [usize; 4],
}

#[derive(Debug, Clone, PartialEq)]
pub enum Resolved {
    /// The builtin to call (a method's receiver goes first).
    Builtin(String),
    /// `recv.name(…)` with `recv` of unknown type: the drawing's kind picks the builtin at run
    /// time; the arguments bind as for `Builtin`'s.
    Dispatch(String),
}

impl Checked {
    pub fn has_errors(&self) -> bool {
        self.diags.iter().any(|d| d.is_error())
    }
}

const DRAWING_TYPES: &[&str] = &["label", "line", "box", "table", "linefill", "polyline"];

struct FnDef<'a> {
    params: &'a [Param],
    body: &'a Block,
    /// Number of values when the body ends with a tuple (`=> [a, b]`).
    ret_arity: Option<usize>,
    /// Declared with `method`: callable as `x.name(…)`.
    is_method: bool,
}

struct Checker<'a> {
    diags: Vec<Diagnostic>,
    scopes: Vec<HashMap<String, Ty>>,
    version: u8,
    /// User functions by name; Pine allows overloads by parameter count.
    funcs: HashMap<String, Vec<FnDef<'a>>>,
    reachable: BTreeSet<String>,
    visual_uses: Vec<(String, Span, Option<StmtKey>)>,
    diag_stmts: Vec<Option<StmtKey>>,
    /// User-defined types (`type P`) with their fields, enums, library aliases (`import … as lib`).
    udts: HashMap<String, &'a [FieldDef]>,
    enums: BTreeSet<String>,
    libs: BTreeSet<String>,
    /// User functions whose definition has been reached: Pine refuses a call before it.
    defined: BTreeSet<String>,
    /// Index of the current function's own scope (None at the top level).
    fn_scope: Option<usize>,
    resolved: HashMap<(u32, u32, u32), Resolved>,
}

fn unsupported(feature: &str, phase: u8, span: Span) -> Diagnostic {
    Diagnostic::error(DiagCode::Unsupported, span, vec![feature.to_string(), phase.to_string()])
}

/// Every user function called (by single-segment name) inside a block of statements.
fn calls_in_stmts(stmts: &[Stmt], out: &mut BTreeSet<String>) {
    for s in stmts {
        match &s.kind {
            StmtKind::Decl { value, .. } | StmtKind::TupleDecl { value, .. } | StmtKind::Assign { value, .. } => calls_in_expr(value, out),
            StmtKind::Expr(e) => calls_in_expr(e, out),
            _ => {}
        }
    }
}

fn calls_in_block(b: &Block, out: &mut BTreeSet<String>) {
    calls_in_stmts(&b.stmts, out);
}

fn calls_in_args(args: &[Arg], out: &mut BTreeSet<String>) {
    for a in args {
        calls_in_expr(&a.value, out);
    }
}

fn calls_in_expr(e: &Expr, out: &mut BTreeSet<String>) {
    match &e.kind {
        ExprKind::Call { callee, args, .. } => {
            // `f(x)`, or a method call `x.m()` (the last segment may name a user method).
            if let Some(last) = callee.last() {
                out.insert(last.clone());
            }
            calls_in_args(args, out);
        }
        // A name used as a value may still be a function passed around: count it.
        ExprKind::Name(p) if p.len() == 1 => {
            out.insert(p[0].clone());
        }
        ExprKind::MethodCall { recv, args, name } => {
            out.insert(name.clone());
            calls_in_expr(recv, out);
            calls_in_args(args, out);
        }
        ExprKind::Field { recv, .. } => calls_in_expr(recv, out),
        ExprKind::Index { base, offset } => {
            calls_in_expr(base, out);
            calls_in_expr(offset, out);
        }
        ExprKind::Unary { e, .. } => calls_in_expr(e, out),
        ExprKind::Binary { l, r, .. } => {
            calls_in_expr(l, out);
            calls_in_expr(r, out);
        }
        ExprKind::Ternary { c, a, b } => {
            calls_in_expr(c, out);
            calls_in_expr(a, out);
            calls_in_expr(b, out);
        }
        ExprKind::Tuple(items) => items.iter().for_each(|i| calls_in_expr(i, out)),
        ExprKind::If(i) => {
            calls_in_expr(&i.cond, out);
            calls_in_block(&i.then, out);
            for (c, b) in &i.elifs {
                calls_in_expr(c, out);
                calls_in_block(b, out);
            }
            if let Some(b) = &i.els {
                calls_in_block(b, out);
            }
        }
        ExprKind::Switch(s) => {
            if let Some(k) = &s.key {
                calls_in_expr(k, out);
            }
            for (p, b) in &s.arms {
                if let Some(p) = p {
                    calls_in_expr(p, out);
                }
                calls_in_block(b, out);
            }
        }
        ExprKind::For(f) => {
            calls_in_expr(&f.from, out);
            calls_in_expr(&f.to, out);
            if let Some(b) = &f.by {
                calls_in_expr(b, out);
            }
            calls_in_block(&f.body, out);
        }
        ExprKind::ForIn(f) => {
            calls_in_expr(&f.iter, out);
            calls_in_block(&f.body, out);
        }
        ExprKind::While(w) => {
            calls_in_expr(&w.cond, out);
            calls_in_block(&w.body, out);
        }
        _ => {}
    }
}

impl<'a> Checker<'a> {
    fn var_ty(&self, name: &str) -> Option<Ty> {
        self.scopes.iter().rev().find_map(|s| s.get(name).cloned())
    }

    fn declared(&self, name: &str) -> bool {
        self.var_ty(name).is_some()
    }

    fn declare(&mut self, name: &str, ty: Ty, span: Span) {
        if name == "_" {
            return; // tuple placeholder, may repeat
        }
        let scope = self.scopes.last_mut().expect("scope");
        if scope.insert(name.to_string(), ty).is_some() {
            self.diags.push(Diagnostic::error(DiagCode::Redeclared, span, vec![name.to_string()]));
        }
    }

    /// A declared type: its checker type, or an unsupported/visual feature report.
    fn check_type(&mut self, t: &TypeRef, span: Span) -> Ty {
        let name = t.name.as_str();
        if let Some(ty) = types::named(name) {
            return ty;
        }
        if DRAWING_TYPES.contains(&name) {
            return Ty::Drawing(name.to_string());
        }
        if name == "chart.point" {
            return Ty::Object(name.to_string());
        }
        if self.udts.contains_key(name) {
            return Ty::Object(name.to_string());
        }
        match name {
            "array" => {
                let elem = t.args.first().map(|a| self.check_type(a, span)).unwrap_or(Ty::Unknown);
                return Ty::Array(Box::new(elem));
            }
            "matrix" | "map" => self.diags.push(unsupported(name, 0, span)),
            _ => self.diags.push(unsupported("type", 4, span)),
        }
        Ty::Unknown
    }

    /// A builtin used by name: supported, not yet (visual or not), or unknown.
    fn builtin(&mut self, name: &str, found: Lookup, span: Span) -> Option<&'static Builtin> {
        match found {
            Lookup::Supported(b) => return Some(b),
            Lookup::NotYet { visual_only: true, .. } => self.visual_uses.push((name.to_string(), span, None)),
            Lookup::NotYet { phase, .. } => self.diags.push(unsupported(name, phase, span)),
            Lookup::Unknown => self.diags.push(Diagnostic::error(DiagCode::UnknownName, span, vec![name.to_string()])),
        }
        None
    }

    /// Statements in a new scope; the type of the last one (the block's value).
    fn block(&mut self, b: &'a Block) -> Ty {
        self.scopes.push(HashMap::new());
        let mut last = Ty::Void;
        for s in &b.stmts {
            last = self.stmt(s);
        }
        self.scopes.pop();
        last
    }

    /// Check one statement and tag the diagnostics it raised (not already tagged by a nested
    /// statement) with its key.
    fn stmt(&mut self, s: &'a Stmt) -> Ty {
        let (d0, v0) = (self.diags.len(), self.visual_uses.len());
        let t = self.stmt_inner(s);
        self.diag_stmts.resize(self.diags.len(), None);
        for tag in &mut self.diag_stmts[d0..] {
            tag.get_or_insert(key_of(s));
        }
        for v in &mut self.visual_uses[v0..] {
            v.2.get_or_insert(key_of(s));
        }
        t
    }

    fn stmt_inner(&mut self, s: &'a Stmt) -> Ty {
        match &s.kind {
            StmtKind::Decl { mode, ty, name, value } => {
                if *mode == DeclMode::Varip {
                    self.diags.push(unsupported("varip", 0, s.span));
                }
                let declared = ty.as_ref().map(|t| self.check_type(t, s.span));
                let vt = self.expr(value);
                let t = match declared {
                    Some(d) => {
                        if !types::assignable(&d, &vt, self.version >= 6) {
                            self.diags.push(Diagnostic::error(DiagCode::TypeMismatch, s.span, vec![format!("{d:?}"), format!("{vt:?}")]));
                        }
                        d
                    }
                    None if vt == Ty::Na => Ty::Unknown,
                    None => vt,
                };
                self.declare(name, t.clone(), s.span);
                t
            }
            StmtKind::TupleDecl { names, value } => {
                let vt = self.expr(value);
                let items = match &vt {
                    Ty::Tuple(items) => {
                        if items.len() != names.len() {
                            self.diags.push(Diagnostic::error(DiagCode::TypeMismatch, s.span, vec![format!("{} values", items.len()), format!("{} names", names.len())]));
                        }
                        items.clone()
                    }
                    _ => vec![],
                };
                for (i, n) in names.iter().enumerate() {
                    self.declare(n, items.get(i).cloned().unwrap_or(Ty::Unknown), s.span);
                }
                Ty::Void
            }
            StmtKind::Assign { target, value, .. } => {
                let vt = self.expr(value);
                if target.len() > 1 && self.declared(&target[0]) {
                    // A field of an object: the object is changed, not the variable.
                    let d = self.path_ty(target, s.span);
                    if !types::assignable(&d, &vt, self.version >= 6) {
                        self.diags.push(Diagnostic::error(DiagCode::TypeMismatch, s.span, vec![format!("{d:?}"), format!("{vt:?}")]));
                    }
                } else if target.len() > 1 || !self.declared(&target[0]) {
                    self.diags.push(Diagnostic::error(DiagCode::AssignUndeclared, s.span, vec![target.join(".")]));
                } else if self.fn_scope.is_some_and(|f| self.scopes.iter().rposition(|sc| sc.contains_key(&target[0])).is_some_and(|i| i < f)) {
                    // Pine: a function cannot modify a variable outside its own scope.
                    self.diags.push(Diagnostic::error(DiagCode::AssignGlobal, s.span, vec![target[0].clone()]));
                } else if let Some(d) = self.var_ty(&target[0]) {
                    if !types::assignable(&d, &vt, self.version >= 6) && !matches!(d, Ty::Na) {
                        self.diags.push(Diagnostic::error(DiagCode::TypeMismatch, s.span, vec![format!("{d:?}"), format!("{vt:?}")]));
                    }
                }
                vt
            }
            StmtKind::Expr(e) => self.expr(e),
            StmtKind::Break | StmtKind::Continue => Ty::Void,
            StmtKind::FuncDef { name, params, body, .. } => {
                self.defined.insert(name.clone());
                if self.reachable.contains(name) {
                    self.scopes.push(HashMap::new());
                    self.fn_scope = Some(self.scopes.len() - 1);
                    for p in params {
                        let t = p.ty.as_ref().map(|t| self.check_type(t, s.span)).unwrap_or(Ty::Unknown);
                        if let Some(d) = &p.default {
                            self.expr(d);
                        }
                        self.declare(&p.name, t, s.span);
                    }
                    self.block(body);
                    self.scopes.pop();
                    self.fn_scope = None;
                } else {
                    self.diags.push(Diagnostic::warning(DiagCode::SkippedUnused, s.span, vec![name.clone()]));
                }
                Ty::Void
            }
            StmtKind::MethodDef { name, params, body, .. } => {
                self.defined.insert(name.clone());
                if self.reachable.contains(name) {
                    self.scopes.push(HashMap::new());
                    self.fn_scope = Some(self.scopes.len() - 1);
                    for p in params {
                        let t = p.ty.as_ref().map(|t| self.check_type(t, s.span)).unwrap_or(Ty::Unknown);
                        if let Some(d) = &p.default {
                            self.expr(d);
                        }
                        self.declare(&p.name, t, s.span);
                    }
                    self.block(body);
                    self.scopes.pop();
                    self.fn_scope = None;
                } else {
                    self.diags.push(Diagnostic::warning(DiagCode::SkippedUnused, s.span, vec![name.clone()]));
                }
                Ty::Void
            }
            StmtKind::TypeDef { fields, .. } => {
                for f in fields {
                    self.check_type(&f.ty, f.span);
                    if let Some(d) = &f.default {
                        self.expr(d);
                    }
                }
                Ty::Void
            }
            StmtKind::EnumDef { .. } => {
                self.diags.push(unsupported("enum", 0, s.span));
                Ty::Void
            }
            StmtKind::Import { .. } => {
                self.diags.push(unsupported("import", 0, s.span));
                Ty::Void
            }
        }
    }

    /// The type of field `field` of a value of type `t`.
    fn field_ty(&mut self, t: &Ty, field: &str, span: Span) -> Ty {
        let Ty::Object(name) = t else { return Ty::Unknown };
        if name == "chart.point" {
            return match field {
                "time" | "index" => Ty::Int,
                "price" => Ty::Float,
                _ => {
                    self.diags.push(Diagnostic::error(DiagCode::UnknownName, span, vec![format!("chart.point.{field}")]));
                    Ty::Unknown
                }
            };
        }
        let Some(fields) = self.udts.get(name.as_str()).copied() else { return Ty::Unknown };
        match fields.iter().find(|f| f.name == field) {
            Some(f) => {
                let ty = f.ty.clone();
                self.check_type(&ty, span)
            }
            None => {
                self.diags.push(Diagnostic::error(DiagCode::UnknownName, span, vec![format!("{name}.{field}")]));
                Ty::Unknown
            }
        }
    }

    /// Type of `path[0].path[1]…` where `path[0]` is a variable.
    fn path_ty(&mut self, path: &[String], span: Span) -> Ty {
        let mut t = self.var_ty(&path[0]).unwrap_or(Ty::Unknown);
        for f in &path[1..] {
            t = self.field_ty(&t, f, span);
        }
        t
    }

    /// `T.new(…)`: fields by position or name, every one optional.
    fn udt_new(&mut self, name: &str, args: &[Arg], span: Span) -> Ty {
        let fields = self.udts[name];
        let positional = args.iter().filter(|a| a.name.is_none()).count();
        if positional > fields.len() {
            self.arg_error((DiagCode::ArgCount, format!("{positional} > {}", fields.len())), span);
        }
        for a in args {
            if let Some(n) = &a.name {
                if !fields.iter().any(|f| &f.name == n) {
                    self.arg_error((DiagCode::ArgName, n.clone()), span);
                }
            }
        }
        Ty::Object(name.to_string())
    }

    /// Is `recv.name(…)` the user method `name`? Pine picks a method by its first parameter's
    /// type: an array, a drawing or another type's value keeps the builtin method of the same
    /// name (`a.set(…)` on an array beside a user `method set(Obj this, …)`).
    fn user_method_fits(&mut self, name: &str, recv: &Ty) -> bool {
        let Some(defs) = self.funcs.get(name) else { return false };
        let firsts: Vec<Option<TypeRef>> = defs.iter().filter(|f| f.is_method).map(|f| f.params.first().and_then(|p| p.ty.clone())).collect();
        if firsts.is_empty() {
            return false;
        }
        if matches!(recv, Ty::Unknown | Ty::Na) {
            return true;
        }
        let fits = firsts.iter().any(|t| match t {
            None => true,
            Some(t) => {
                let declared = self.check_type(t, Span::default());
                declared == Ty::Unknown || types::assignable(&declared, recv, true) || matches!((&declared, recv), (Ty::Array(_), Ty::Array(_)))
            }
        });
        fits
    }

    /// `recv.m(args)` where `m` is a user method (or function): its result.
    fn user_method(&mut self, name: &str) -> Ty {
        self.funcs[name].first().and_then(|f| f.ret_arity).map(|n| Ty::Tuple(vec![Ty::Unknown; n])).unwrap_or(Ty::Unknown)
    }

    /// `recv.name(args)` on a builtin type: an array method is `array.name(recv, args)`; a
    /// drawing's method is a visual use.
    fn method(&mut self, recv_ty: Ty, recv: Expr, name: &str, args: &[Arg], arg_tys: Vec<Ty>, span: Span) -> Ty {
        // `o.copy()` on an object.
        if let (Ty::Object(t), "copy") = (&recv_ty, name) {
            return Ty::Object(t.clone());
        }
        let at = (span.line, span.col, span.len);
        // A drawing's method: `l.set_x(…)` is `label.set_x(l, …)`.
        if let Ty::Drawing(kind) = &recv_ty {
            let full = format!("{kind}.{name}");
            if let Lookup::Supported(_) = lookup_call(&full) {
                self.resolved.insert(at, Resolved::Builtin(full.clone()));
                return self.method_call(&full, recv, recv_ty, args, arg_tys, span);
            }
            self.diags.push(Diagnostic::error(DiagCode::UnknownName, span, vec![full]));
            return Ty::Unknown;
        }
        let full = format!("array.{name}");
        // On a value of unknown type that is no array method: a drawing method, picked by the
        // drawing's kind at run time when several kinds have it.
        if recv_ty == Ty::Unknown && !matches!(lookup_call(&full), Lookup::Supported(_)) {
            let kinds: Vec<&str> = DRAWING_TYPES.iter().copied().filter(|ns| matches!(lookup_call(&format!("{ns}.{name}")), Lookup::Supported(_))).collect();
            if let Some(first) = kinds.first() {
                let b = format!("{first}.{name}");
                let arity = |k: &str| match lookup_call(&format!("{k}.{name}")) {
                    Lookup::Supported(b) => b.params.len(),
                    _ => 0,
                };
                // Kinds whose arguments differ cannot share one binding: the first one's wins.
                let shared = kinds.iter().all(|k| arity(k) == arity(first));
                self.resolved.insert(at, if kinds.len() > 1 && shared { Resolved::Dispatch(b.clone()) } else { Resolved::Builtin(b.clone()) });
                return self.method_call(&b, recv, recv_ty, args, arg_tys, span);
            }
        }
        if matches!(recv_ty, Ty::Array(_) | Ty::Unknown) {
            if let Lookup::Supported(b) = lookup_call(&full) {
                // A user method of the same name is for another type: the compiler must not
                // take it by its name.
                if self.funcs.contains_key(name) {
                    self.resolved.insert(at, Resolved::Builtin(full.clone()));
                }
                let mut all = vec![Arg { name: None, value: recv }];
                all.extend(args.iter().cloned());
                let mut tys = vec![recv_ty];
                tys.extend(arg_tys);
                return match types::bind_args(b.params, b.variadic, &all) {
                    Ok(bound) => types::ret_of(b, &bound, &tys),
                    Err(err) => {
                        self.arg_error(err, span);
                        Ty::Unknown
                    }
                };
            }
        }
        self.diags.push(unsupported("method", 4, span));
        Ty::Unknown
    }

    /// `name(recv, args…)`: its result type, or the binding error.
    fn method_call(&mut self, name: &str, recv: Expr, recv_ty: Ty, args: &[Arg], arg_tys: Vec<Ty>, span: Span) -> Ty {
        let Lookup::Supported(b) = lookup_call(name) else { return Ty::Unknown };
        let mut all = vec![Arg { name: None, value: recv }];
        all.extend(args.iter().cloned());
        let mut tys = vec![recv_ty];
        tys.extend(arg_tys);
        match types::bind_args(b.params, b.variadic, &all) {
            Ok(bound) => types::ret_of(b, &bound, &tys),
            Err(err) => {
                self.arg_error(err, span);
                Ty::Unknown
            }
        }
    }

    fn args(&mut self, args: &'a [Arg]) -> Vec<Ty> {
        args.iter().map(|a| self.expr(&a.value)).collect()
    }

    fn arg_error(&mut self, (code, what): (DiagCode, String), span: Span) {
        self.diags.push(Diagnostic::error(code, span, vec![what]));
    }

    fn expr(&mut self, e: &'a Expr) -> Ty {
        match &e.kind {
            ExprKind::Num(_) => Ty::Float,
            ExprKind::Int(_) => Ty::Int,
            ExprKind::Str(_) => Ty::Str,
            ExprKind::Bool(_) => Ty::Bool,
            ExprKind::Color(_) => Ty::Color,
            ExprKind::Na => Ty::Na,
            ExprKind::Name(path) if path.len() > 1 && self.enums.contains(&path[0]) => {
                self.diags.push(unsupported("enum", 0, e.span));
                Ty::Unknown
            }
            ExprKind::Name(path) if path.len() > 1 && self.udts.contains_key(&path[0]) && !self.declared(&path[0]) => {
                self.diags.push(Diagnostic::error(DiagCode::UnknownName, e.span, vec![path.join(".")]));
                Ty::Unknown
            }
            ExprKind::Name(path) if path.len() > 1 && self.libs.contains(&path[0]) => {
                self.diags.push(unsupported("import", 0, e.span));
                Ty::Unknown
            }
            ExprKind::Name(path) => {
                let builtin = path.len() > 1 && matches!(lookup(&path.join(".")), Lookup::Supported(_));
                if let Some(t) = self.var_ty(&path[0]).filter(|_| !builtin) {
                    if path.len() > 1 {
                        return self.path_ty(path, e.span);
                    }
                    t
                } else if path.len() == 1 && self.funcs.contains_key(&path[0]) {
                    Ty::Unknown
                } else {
                    let name = path.join(".");
                    let found = lookup(&name);
                    match self.builtin(&name, found, e.span) {
                        Some(b) => types::ret_of(b, &types::Bound { slots: vec![], extra: vec![] }, &[]),
                        None => Ty::Unknown,
                    }
                }
            }
            // `na(float)`: a typed na (v6).
            ExprKind::Call { callee, args, .. }
                if callee.len() == 1
                    && callee[0] == "na"
                    && args.len() == 1
                    && matches!(&args[0].value.kind, ExprKind::Name(p) if p.len() == 1 && types::named(&p[0]).is_some()) =>
            {
                match &args[0].value.kind {
                    ExprKind::Name(p) => types::named(&p[0]).unwrap_or(Ty::Unknown),
                    _ => Ty::Unknown,
                }
            }
            ExprKind::Call { callee, args, generic } => {
                let arg_tys = self.args(args);
                if callee.len() == 2 && self.udts.contains_key(&callee[0]) && !self.declared(&callee[0]) {
                    match callee[1].as_str() {
                        "new" => self.udt_new(&callee[0], args, e.span),
                        "copy" => Ty::Object(callee[0].clone()),
                        _ => {
                            self.diags.push(Diagnostic::error(DiagCode::UnknownName, e.span, vec![callee.join(".")]));
                            Ty::Unknown
                        }
                    }
                } else if callee.len() > 1 && self.enums.contains(&callee[0]) {
                    self.diags.push(unsupported("enum", 0, e.span));
                    Ty::Unknown
                } else if callee.len() > 1 && self.libs.contains(&callee[0]) {
                    self.diags.push(unsupported("import", 0, e.span));
                    Ty::Unknown
                } else if callee.len() == 1 && self.funcs.contains_key(&callee[0]) && !self.defined.contains(&callee[0]) {
                    self.diags.push(Diagnostic::error(DiagCode::UnknownName, e.span, vec![callee[0].clone()]));
                    Ty::Unknown
                } else if callee.len() == 1 && self.funcs.contains_key(&callee[0]) {
                    // Positional arguments fill parameters in order; named ones by name. Any
                    // overload that accepts the call will do.
                    let mut err = None;
                    let mut ret = Ty::Unknown;
                    for f in &self.funcs[&callee[0]] {
                        let names: Vec<&str> = f.params.iter().map(|p| p.name.as_str()).collect();
                        let required: Vec<usize> = (0..names.len()).filter(|&i| f.params[i].default.is_none()).collect();
                        let positional = args.iter().filter(|a| a.name.is_none()).count();
                        let mut filled: Vec<usize> = (0..positional.min(names.len())).collect();
                        let mut this_err = None;
                        for n in args.iter().filter_map(|a| a.name.as_deref()) {
                            match names.iter().position(|m| *m == n) {
                                Some(i) => filled.push(i),
                                None => this_err = Some((DiagCode::ArgName, n.to_string())),
                            }
                        }
                        if this_err.is_none() && (positional > names.len() || required.iter().any(|i| !filled.contains(i))) {
                            this_err = Some((DiagCode::ArgCount, callee[0].clone()));
                        }
                        match this_err {
                            None => {
                                err = None;
                                ret = f.ret_arity.map(|n| Ty::Tuple(vec![Ty::Unknown; n])).unwrap_or(Ty::Unknown);
                                break;
                            }
                            Some(e) => err = err.or(Some(e)),
                        }
                    }
                    if let Some(err) = err {
                        self.arg_error(err, e.span);
                    }
                    ret
                } else if callee.len() >= 2 && self.declared(&callee[0]) && !matches!(lookup_call(&callee.join(".")), Lookup::Supported(_)) {
                    let (path, m) = callee.split_at(callee.len() - 1);
                    let recv_ty = self.path_ty(path, e.span);
                    if self.user_method_fits(&m[0], &recv_ty) {
                        self.user_method(&m[0])
                    } else {
                        let recv = Expr { kind: ExprKind::Name(path.to_vec()), span: e.span };
                        self.method(recv_ty, recv, &m[0], args, arg_tys, e.span)
                    }
                } else {
                    let name = callee.join(".");
                    if self.scopes.len() > 1 && crate::stdlib::output(&name).is_some() {
                        // Pine: outputs are declared at the global scope only.
                        self.diags.push(Diagnostic::error(DiagCode::Syntax, e.span, vec![format!("{name} in local scope")]));
                    }
                    let mut name = name;
                    // `label.new(point, …)`, `line.new(p1, p2, …)`, `box.new(p1, p2, …)`.
                    if matches!(name.as_str(), "label.new" | "line.new" | "box.new") {
                        let first = args.iter().position(|a| a.name.is_none());
                        if first.is_some_and(|i| arg_tys[i] == Ty::Object("chart.point".into())) {
                            name = format!("{name}#point");
                            self.resolved.insert((e.span.line, e.span.col, e.span.len), Resolved::Builtin(name.clone()));
                        }
                    }
                    let found = lookup_call(&name);
                    match self.builtin(&name, found, e.span) {
                        // `array.new<T>`: the element type is the type argument.
                        Some(b) if name == "array.new" => {
                            if let Err(err) = types::bind_args(b.params, b.variadic, args) {
                                self.arg_error(err, e.span);
                            }
                            let elem = generic.as_ref().and_then(|g| g.args.first()).map(|t| self.check_type(t, e.span));
                            Ty::Array(Box::new(elem.unwrap_or(Ty::Unknown)))
                        }
                        Some(b) => match types::bind_args(b.params, b.variadic, args) {
                            Ok(bound) => types::ret_of(b, &bound, &arg_tys),
                            Err(err) => {
                                self.arg_error(err, e.span);
                                Ty::Unknown
                            }
                        },
                        None => Ty::Unknown,
                    }
                }
            }
            ExprKind::MethodCall { recv, name, args } => {
                let recv_ty = self.expr(recv);
                let arg_tys = self.args(args);
                if self.user_method_fits(name, &recv_ty) {
                    self.user_method(name)
                } else {
                    self.method(recv_ty, (**recv).clone(), name, args, arg_tys, e.span)
                }
            }
            ExprKind::Field { recv, name } => {
                let t = self.expr(recv);
                self.field_ty(&t, name, e.span)
            }
            ExprKind::Index { base, offset } => {
                let t = self.expr(base);
                self.expr(offset);
                t
            }
            ExprKind::Unary { op, e: inner } => {
                let t = self.expr(inner);
                if *op == UnOp::Not {
                    Ty::Bool
                } else {
                    t
                }
            }
            ExprKind::Binary { op, l, r } => {
                let (lt, rt) = (self.expr(l), self.expr(r));
                match op {
                    BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge | BinOp::And | BinOp::Or => Ty::Bool,
                    BinOp::Add if lt == Ty::Str || rt == Ty::Str => Ty::Str,
                    // `int / int` is int or float depending on version and constness: left open.
                    BinOp::Div if lt == Ty::Int && rt == Ty::Int => Ty::Unknown,
                    BinOp::Div => Ty::Float,
                    _ => match (&lt, &rt) {
                        (Ty::Int, Ty::Int) => Ty::Int,
                        (Ty::Unknown, _) | (_, Ty::Unknown) => Ty::Unknown,
                        _ => Ty::Float,
                    },
                }
            }
            ExprKind::Ternary { c, a, b } => {
                self.expr(c);
                let (at, bt) = (self.expr(a), self.expr(b));
                match types::unify(&at, &bt) {
                    Some(t) => t,
                    None => {
                        self.diags.push(Diagnostic::error(DiagCode::TypeMismatch, e.span, vec![format!("{at:?}"), format!("{bt:?}")]));
                        Ty::Unknown
                    }
                }
            }
            ExprKind::Tuple(items) => Ty::Tuple(items.iter().map(|i| self.expr(i)).collect()),
            ExprKind::If(i) => {
                self.expr(&i.cond);
                self.block(&i.then);
                for (c, b) in &i.elifs {
                    self.expr(c);
                    self.block(b);
                }
                if let Some(b) = &i.els {
                    self.block(b);
                }
                Ty::Unknown
            }
            ExprKind::Switch(s) => {
                if let Some(k) = &s.key {
                    self.expr(k);
                }
                for (p, b) in &s.arms {
                    if let Some(p) = p {
                        self.expr(p);
                    }
                    self.block(b);
                }
                Ty::Unknown
            }
            ExprKind::For(f) => {
                self.expr(&f.from);
                self.expr(&f.to);
                if let Some(b) = &f.by {
                    self.expr(b);
                }
                self.scopes.push(HashMap::new());
                self.declare(&f.var, Ty::Int, e.span);
                self.block(&f.body);
                self.scopes.pop();
                Ty::Unknown
            }
            ExprKind::ForIn(f) => {
                let item = match self.expr(&f.iter) {
                    Ty::Array(t) => *t,
                    _ => Ty::Unknown,
                };
                self.scopes.push(HashMap::new());
                if let Some(i) = &f.idx {
                    self.declare(i, Ty::Int, e.span);
                }
                self.declare(&f.item, item, e.span);
                self.block(&f.body);
                self.scopes.pop();
                Ty::Unknown
            }
            ExprKind::While(w) => {
                self.expr(&w.cond);
                self.block(&w.body);
                Ty::Unknown
            }
        }
    }
}

/// Is `name` called at the top level of the script (outside function bodies)?
fn top_level_call<'s>(script: &'s Script, name: &str) -> Option<&'s Stmt> {
    script.body.iter().find(|s| matches!(&s.kind, StmtKind::Expr(Expr { kind: ExprKind::Call { callee, .. }, .. }) if callee.len() == 1 && callee[0] == name))
}

pub fn check(script: &Script) -> Checked {
    let mut c = Checker {
        diags: Vec::new(),
        scopes: vec![HashMap::new()],
        version: script.version,
        funcs: HashMap::new(),
        reachable: BTreeSet::new(),
        visual_uses: Vec::new(),
        diag_stmts: Vec::new(),
        udts: HashMap::new(),
        enums: BTreeSet::new(),
        libs: BTreeSet::new(),
        defined: BTreeSet::new(),
        fn_scope: None,
        resolved: HashMap::new(),
    };

    for s in &script.body {
        match &s.kind {
            StmtKind::FuncDef { name, params, body, .. } | StmtKind::MethodDef { name, params, body, .. } => {
                let ret_arity = match body.stmts.last().map(|s| &s.kind) {
                    Some(StmtKind::Expr(Expr { kind: ExprKind::Tuple(items), .. })) => Some(items.len()),
                    _ => None,
                };
                let is_method = matches!(s.kind, StmtKind::MethodDef { .. });
                c.funcs.entry(name.clone()).or_default().push(FnDef { params, body, ret_arity, is_method });
            }
            StmtKind::TypeDef { name, fields, .. } => {
                c.udts.insert(name.clone(), fields);
            }
            StmtKind::EnumDef { name, .. } => {
                c.enums.insert(name.clone());
            }
            StmtKind::Import { path, alias } => {
                let lib = alias.clone().unwrap_or_else(|| path.split('/').nth(1).unwrap_or_default().to_string());
                c.libs.insert(lib);
            }
            _ => {}
        }
    }
    // Reachability: calls from top-level statements, then transitively through function bodies.
    let top: Vec<Stmt> = script.body.iter().filter(|s| !matches!(s.kind, StmtKind::FuncDef { .. } | StmtKind::MethodDef { .. })).cloned().collect();
    let mut called = BTreeSet::new();
    calls_in_stmts(&top, &mut called);
    let mut work: Vec<String> = called.iter().filter(|n| c.funcs.contains_key(*n)).cloned().collect();
    while let Some(n) = work.pop() {
        if !c.reachable.insert(n.clone()) {
            continue;
        }
        let mut inner = BTreeSet::new();
        for f in c.funcs.get(&n).into_iter().flatten() {
            calls_in_block(f.body, &mut inner);
            for p in f.params {
                if let Some(d) = &p.default {
                    calls_in_expr(d, &mut inner);
                }
            }
        }
        work.extend(inner.into_iter().filter(|m| c.funcs.contains_key(m) && !c.reachable.contains(m)));
    }

    for name in ["strategy", "library"] {
        if let Some(s) = top_level_call(script, name) {
            c.diags.push(Diagnostic::error(DiagCode::NotIndicator, s.span, vec![name.into()]));
        }
    }
    if top_level_call(script, "indicator").is_none() {
        c.diags.push(Diagnostic::error(DiagCode::NotIndicator, Span { line: 1, col: 1, len: 0 }, vec!["indicator".into()]));
    }

    for s in &script.body {
        c.stmt(s);
    }

    let meta = ScriptMeta {
        version: script.version,
        title: String::new(),
        shorttitle: None,
        overlay: false,
        precision: None,
        max_bars_back: None,
        inputs: Vec::new(),
        plots: Vec::new(),
    };
    let mut diag_stmts = c.diag_stmts;
    diag_stmts.resize(c.diags.len(), None);
    let mut checked = Checked {
        diags: c.diags,
        reachable_fns: c.reachable,
        meta,
        visual_uses: c.visual_uses,
        diag_stmts,
        dropped: BTreeSet::new(),
        resolved: c.resolved,
        draw_limits: [50; 4],
    };
    flow::apply(script, &mut checked);
    checked
}
