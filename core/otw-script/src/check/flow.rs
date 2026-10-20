//! Which statements matter, and which plots a backtest may use.
//!
//! **Kept statements.** Outputs (`plot`, `hline`, `fill`, `bgcolor`…) and `runtime.error` are
//! roots. A kept statement keeps every variable it reads, every user function it calls and its
//! enclosing control statement; a kept variable keeps every statement that writes it; a kept
//! function keeps its return statement. Everything else only feeds drawings, alerts, logs or
//! nothing, and is dropped: unsupported features there become `SkippedVisual` warnings, while a
//! visual-only feature inside a kept statement refuses the script (its value would be needed).
//!
//! **Causality.** A value is non-causal when it reads a builtin that knows where the data ends
//! (`last_bar_index`, `barstate.islast`…), directly, through variables, through a function, or by
//! being assigned under such a condition. A plot reading one, or drawn with a negative offset,
//! gets `causal: false` (spec §4.4).

use std::collections::{BTreeMap, BTreeSet};

use super::{key_of, types, Checked, StmtKey};
use crate::ast::*;
use crate::diag::{DiagCode, Diagnostic, Span};
use crate::output::{PlotMeta, PlotStyle, Taint};
use crate::registry::{lookup, lookup_call, Lookup};
use super::Resolved;
use std::collections::HashMap;

/// Pine's ceiling for `max_bars_back`; more is refused, never clamped.
const MAX_BARS_BACK: usize = 5000;

const OUTPUTS: &[&str] = &["plot", "plotshape", "plotchar", "plotarrow", "plotcandle", "plotbar", "hline", "fill", "bgcolor", "barcolor"];
/// Named arguments the chart does not render yet: accepted, ignored, reported.
const COSMETIC: &[&str] = &["editable", "force_overlay", "trackprice", "join", "show_last", "format", "precision"];

#[derive(Default)]
struct Info {
    parent: Option<StmtKey>,
    reads: BTreeSet<String>,
    writes: BTreeSet<String>,
    calls: BTreeSet<String>,
    /// The non-causal builtin it reads directly, if any.
    noncausal: Option<Taint>,
    root: bool,
    /// Statement opening a block (if/for/while/switch, function): its reads control the block.
    control: bool,
    /// A function definition: scopes its body but is no condition over it.
    is_fn: bool,
    /// A `for`/`while` statement.
    is_loop: bool,
    /// `b = a`: the variable `a` (an array shared under two names).
    alias_of: Vec<String>,
    /// User function calls with the variable passed in each argument (`f(a, 1)`).
    fn_args: Vec<(String, Vec<Option<String>>)>,
    /// Names it declares (`x = …`, tuple names, a `for` variable).
    declares: BTreeSet<String>,
    /// `break` or `continue`.
    jump: bool,
    func: Option<String>,
    /// Creates, changes or deletes a drawing (directly, not through a user function).
    effect: bool,
}

#[derive(Default)]
struct Facts {
    stmts: BTreeMap<StmtKey, Info>,
    order: Vec<StmtKey>,
    /// Last statement of each user function body (its return value).
    fn_returns: BTreeMap<String, Vec<StmtKey>>,
    fn_defs: BTreeMap<String, Vec<StmtKey>>,
    embedded_children: BTreeMap<StmtKey, Vec<StmtKey>>,
    /// Last statement of each block of a control statement (its value when used as one).
    block_lasts: BTreeMap<StmtKey, Vec<StmtKey>>,
    /// Last statements of embedded blocks (`x = if …`): they are values.
    embedded_lasts: BTreeSet<StmtKey>,
    /// Parameter names of each user function (one list per overload).
    fn_params: BTreeMap<String, Vec<Vec<String>>>,
    /// Statements of each function that change an array of a parameter or of the caller:
    /// kept with the function, since the change is what a call does.
    fn_side_writers: BTreeMap<String, Vec<StmtKey>>,
    /// Calls the checker resolved by position (drawing methods).
    resolved: HashMap<(u32, u32, u32), Resolved>,
    /// Statements that draw, directly or through the functions they call, by function (None
    /// for the top level): drawing is what they are for.
    effects: BTreeMap<Option<String>, Vec<StmtKey>>,
}

impl Facts {
    /// Statements that draw: directly, or by calling a function that draws (transitively).
    fn find_effects(&mut self) {
        let mut fns: BTreeSet<String> = BTreeSet::new();
        loop {
            let before = fns.len();
            for info in self.stmts.values() {
                if info.effect || info.calls.iter().any(|c| fns.contains(c)) {
                    if let Some(f) = &info.func {
                        fns.insert(f.clone());
                    }
                }
            }
            if fns.len() == before {
                break;
            }
        }
        for k in &self.order {
            let info = &self.stmts[k];
            if info.effect || info.calls.iter().any(|c| fns.contains(c)) {
                self.effects.entry(info.func.clone()).or_default().push(*k);
            }
        }
    }

    /// A call `f(a, …)` to a function that changes the array of one of its parameters writes
    /// the variable passed there, transitively through nested calls.
    fn propagate_param_writes(&mut self) {
        // Names declared inside each function (its own locals).
        let mut locals: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for info in self.stmts.values() {
            if let Some(f) = &info.func {
                locals.entry(f.clone()).or_default().extend(info.declares.iter().cloned());
            }
        }
        loop {
            // Parameter positions each function writes (through any overload), and the
            // caller's names it writes.
            let mut params_w: BTreeMap<String, BTreeSet<usize>> = BTreeMap::new();
            let mut outer_w: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
            for info in self.stmts.values() {
                let Some(f) = &info.func else { continue };
                let all_params: BTreeSet<&String> = self.fn_params.get(f).into_iter().flatten().flatten().collect();
                for params in self.fn_params.get(f).into_iter().flatten() {
                    for (i, p) in params.iter().enumerate() {
                        if info.writes.contains(p) {
                            params_w.entry(f.clone()).or_default().insert(i);
                        }
                    }
                }
                let own = locals.get(f);
                for w in &info.writes {
                    if !all_params.contains(w) && !own.is_some_and(|l| l.contains(w)) {
                        outer_w.entry(f.clone()).or_default().insert(w.clone());
                    }
                }
            }
            let mut changed = false;
            for info in self.stmts.values_mut() {
                let mut add = Vec::new();
                for (f, args) in &info.fn_args {
                    for &i in params_w.get(f).into_iter().flatten() {
                        if let Some(Some(v)) = args.get(i) {
                            add.push(v.clone());
                        }
                    }
                    add.extend(outer_w.get(f).into_iter().flatten().cloned());
                }
                add.retain(|v| !info.writes.contains(v));
                changed |= !add.is_empty();
                info.writes.extend(add);
            }
            if !changed {
                // Side-writing statements of each function, kept when it is.
                for (k, info) in &self.stmts {
                    let Some(f) = &info.func else { continue };
                    let all_params: BTreeSet<&String> = self.fn_params.get(f).into_iter().flatten().flatten().collect();
                    let own = locals.get(f);
                    if info.writes.iter().any(|w| all_params.contains(w) || !own.is_some_and(|l| l.contains(w))) {
                        self.fn_side_writers.entry(f.clone()).or_default().push(*k);
                    }
                }
                return;
            }
        }
    }
}

/// Names read by an expression (first path segment for variables, full path for builtins);
/// user-function-looking calls; blocks embedded in it.
struct Scan<'a> {
    reads: BTreeSet<String>,
    calls: BTreeSet<String>,
    /// The first non-causal builtin met.
    noncausal: Option<Taint>,
    blocks: Vec<&'a Block>,
    /// Variables whose array a call changes in place.
    mutated: BTreeSet<String>,
    fn_args: Vec<(String, Vec<Option<String>>)>,
    /// A call to a drawing builtin that changes the chart.
    effect: bool,
    /// Positions of method calls (`x.m(…)`), resolved by the checker.
    method_sites: Vec<(u32, u32, u32)>,
}

/// A drawing method that changes its drawing (`l.set_x(…)`, `t.cell(…)`, `b.delete()`).
fn draw_setter(name: &str) -> bool {
    name.starts_with("set_") || name.starts_with("cell") || matches!(name, "delete" | "merge_cells")
}

/// A drawing builtin that creates, changes or deletes a drawing (not a getter).
fn draws(name: &str) -> bool {
    let Some((ns, member)) = name.split_once('.') else { return false };
    matches!(ns, "label" | "line" | "box" | "table" | "linefill" | "polyline") && !member.starts_with("get_") && member != "all"
}

/// Array functions that change their array.
const MUTATORS: &[&str] = &["push", "pop", "shift", "unshift", "insert", "remove", "set", "clear", "fill", "reverse", "sort", "concat"];

/// The variables an array-changing call (or a drawing setter) changes: `a.push(x)`, `o.items.push(x)` or
/// `array.push(a, x)`.
fn mutated_vars(callee: &[String], args: &[Arg]) -> Vec<String> {
    let last = callee[callee.len() - 1].as_str();
    if callee.len() < 2 || !(MUTATORS.contains(&last) || draw_setter(last)) {
        return Vec::new();
    }
    if !matches!(callee[0].as_str(), "array" | "label" | "line" | "box" | "table" | "linefill" | "polyline") {
        return vec![callee[0].clone()];
    }
    args.first().map(|a| ref_sources(&a.value)).unwrap_or_default()
}

/// Array functions whose result shares objects with their array (`array.slice` is a view).
const GETTERS: &[&str] = &["get", "first", "last", "pop", "shift", "remove", "slice"];

/// The variable whose objects a value may share: `a`, `o.items`, `array.get(a, 0)`,
/// `a.get(0)`, `o.items.first()`. Writing a field or changing an array through such a value
/// changes that variable's objects.
/// Every branch of `c ? a : b`, `if` and `switch`.
fn ref_sources(e: &Expr) -> Vec<String> {
    match &e.kind {
        ExprKind::Name(p) => vec![p[0].clone()],
        ExprKind::Field { recv, .. } => ref_sources(recv),
        ExprKind::Call { callee, args, .. } if callee.len() >= 2 && GETTERS.contains(&callee[callee.len() - 1].as_str()) => {
            if callee[0] == "array" {
                args.first().map(|a| ref_sources(&a.value)).unwrap_or_default()
            } else {
                vec![callee[0].clone()]
            }
        }
        ExprKind::MethodCall { recv, name, .. } if GETTERS.contains(&name.as_str()) => ref_sources(recv),
        ExprKind::Ternary { a, b, .. } => [ref_sources(a), ref_sources(b)].concat(),
        ExprKind::If(i) => std::iter::once(&i.then).chain(i.elifs.iter().map(|(_, b)| b)).chain(&i.els).flat_map(block_sources).collect(),
        ExprKind::Switch(sw) => sw.arms.iter().flat_map(|(_, b)| block_sources(b)).collect(),
        _ => Vec::new(),
    }
}

/// What a block's value (its last statement) may share.
fn block_sources(b: &Block) -> Vec<String> {
    match b.stmts.last().map(|s| &s.kind) {
        Some(StmtKind::Expr(e)) => ref_sources(e),
        _ => Vec::new(),
    }
}

fn scan_expr<'a>(e: &'a Expr, out: &mut Scan<'a>) {
    match &e.kind {
        ExprKind::Name(path) => {
            out.reads.insert(path[0].clone());
            let name = path.join(".");
            if let Lookup::Supported(b) = lookup(&name) {
                if !b.causal && out.noncausal.is_none() {
                    out.noncausal = Some(Taint { feature: name, line: e.span.line });
                }
            }
        }
        ExprKind::Call { callee, args, .. } => {
            let arg_names = || -> Vec<Option<String>> {
                args.iter()
                    .map(|a| match &a.value.kind {
                        ExprKind::Name(p) if a.name.is_none() => Some(p[0].clone()),
                        _ => None,
                    })
                    .collect()
            };
            if callee.len() == 1 {
                out.calls.insert(callee[0].clone());
                out.fn_args.push((callee[0].clone(), arg_names()));
            } else if matches!(lookup_call(&callee.join(".")), Lookup::Supported(_)) {
                // A builtin (`ta.sma`), even when a variable shares its namespace's name.
                out.reads.insert(callee[0].clone());
                out.effect |= draws(&callee.join("."));
            } else {
                out.method_sites.push((e.span.line, e.span.col, e.span.len));
                // `x.m(…)`: a user method gets `x` (or the object holding the field) first.
                let mut names = vec![Some(callee[0].clone())];
                names.extend(arg_names());
                out.fn_args.push((callee[callee.len() - 1].clone(), names));
                out.calls.insert(callee[callee.len() - 1].clone());
                out.reads.insert(callee[0].clone());
            }
            let name = callee.join(".");
            if let Lookup::Supported(b) = lookup_call(&name) {
                if !b.causal && out.noncausal.is_none() {
                    out.noncausal = Some(Taint { feature: name, line: e.span.line });
                }
            }
            out.mutated.extend(mutated_vars(callee, args));
            args.iter().for_each(|a| scan_expr(&a.value, out));
        }
        ExprKind::MethodCall { recv, args, name } => {
            out.method_sites.push((e.span.line, e.span.col, e.span.len));
            if MUTATORS.contains(&name.as_str()) || draw_setter(name) {
                out.mutated.extend(ref_sources(recv));
            }
            let mut names = vec![match &recv.kind {
                ExprKind::Name(p) => Some(p[0].clone()),
                _ => None,
            }];
            names.extend(args.iter().map(|a| match &a.value.kind {
                ExprKind::Name(p) if a.name.is_none() => Some(p[0].clone()),
                _ => None,
            }));
            out.fn_args.push((name.clone(), names));
            out.calls.insert(name.clone());
            scan_expr(recv, out);
            args.iter().for_each(|a| scan_expr(&a.value, out));
        }
        ExprKind::Field { recv, .. } => scan_expr(recv, out),
        ExprKind::Index { base, offset } => {
            scan_expr(base, out);
            scan_expr(offset, out);
        }
        ExprKind::Unary { e, .. } => scan_expr(e, out),
        ExprKind::Binary { l, r, .. } => {
            scan_expr(l, out);
            scan_expr(r, out);
        }
        ExprKind::Ternary { c, a, b } => {
            scan_expr(c, out);
            scan_expr(a, out);
            scan_expr(b, out);
        }
        ExprKind::Tuple(items) => items.iter().for_each(|i| scan_expr(i, out)),
        ExprKind::If(i) => {
            scan_expr(&i.cond, out);
            out.blocks.push(&i.then);
            for (c, b) in &i.elifs {
                scan_expr(c, out);
                out.blocks.push(b);
            }
            if let Some(b) = &i.els {
                out.blocks.push(b);
            }
        }
        ExprKind::Switch(s) => {
            if let Some(k) = &s.key {
                scan_expr(k, out);
            }
            for (p, b) in &s.arms {
                if let Some(p) = p {
                    scan_expr(p, out);
                }
                out.blocks.push(b);
            }
        }
        ExprKind::For(f) => {
            scan_expr(&f.from, out);
            scan_expr(&f.to, out);
            if let Some(b) = &f.by {
                scan_expr(b, out);
            }
            out.blocks.push(&f.body);
        }
        ExprKind::ForIn(f) => {
            scan_expr(&f.iter, out);
            out.blocks.push(&f.body);
        }
        ExprKind::While(w) => {
            scan_expr(&w.cond, out);
            out.blocks.push(&w.body);
        }
        _ => {}
    }
}

fn is_control(e: &Expr) -> bool {
    matches!(e.kind, ExprKind::If(_) | ExprKind::For(_) | ExprKind::ForIn(_) | ExprKind::While(_) | ExprKind::Switch(_))
}

impl Facts {
    fn walk_block(&mut self, b: &Block, parent: StmtKey, func: Option<&str>) {
        for s in &b.stmts {
            self.walk(s, Some(parent), func);
        }
    }

    fn walk(&mut self, s: &Stmt, parent: Option<StmtKey>, func: Option<&str>) {
        let key = key_of(s);
        let mut info = Info { parent, func: func.map(str::to_string), ..Default::default() };
        let mut scan = Scan {
            reads: BTreeSet::new(),
            calls: BTreeSet::new(),
            noncausal: None,
            blocks: Vec::new(),
            mutated: BTreeSet::new(),
            fn_args: Vec::new(),
            effect: false,
            method_sites: Vec::new(),
        };
        let mut child_blocks: Vec<&Block> = Vec::new();
        match &s.kind {
            StmtKind::Decl { name, value, .. } => {
                info.writes.insert(name.clone());
                info.declares.insert(name.clone());
                info.alias_of = ref_sources(value);
                scan_expr(value, &mut scan);
            }
            StmtKind::TupleDecl { names, value } => {
                info.writes.extend(names.iter().cloned());
                info.declares.extend(names.iter().cloned());
                scan_expr(value, &mut scan);
            }
            StmtKind::Assign { target, op, value } => {
                info.writes.insert(target[0].clone());
                if *op == AssignOp::Reassign {
                    info.alias_of = ref_sources(value);
                }
                if *op != AssignOp::Reassign {
                    scan.reads.insert(target[0].clone());
                }
                scan_expr(value, &mut scan);
            }
            StmtKind::Expr(e) if is_control(e) => {
                info.control = true;
                info.is_loop = matches!(e.kind, ExprKind::For(_) | ExprKind::ForIn(_) | ExprKind::While(_));
                if let ExprKind::For(f) = &e.kind {
                    info.writes.insert(f.var.clone());
                    info.declares.insert(f.var.clone());
                }
                if let ExprKind::ForIn(f) = &e.kind {
                    // The item is each element in turn: a field written through it is written
                    // in the array.
                    info.writes.insert(f.item.clone());
                    info.alias_of = ref_sources(&f.iter);
                    info.declares.insert(f.item.clone());
                    info.declares.extend(f.idx.clone());
                }
                scan_expr(e, &mut scan);
                child_blocks = std::mem::take(&mut scan.blocks);
            }
            StmtKind::Expr(e) => {
                scan_expr(e, &mut scan);
                if let ExprKind::Call { callee, .. } = &e.kind {
                    let name = callee.join(".");
                    if OUTPUTS.contains(&name.as_str()) || name == "runtime.error" || name == "indicator" || name == "max_bars_back" {
                        info.root = true;
                    }
                }
            }
            StmtKind::Break | StmtKind::Continue => info.jump = true,
            StmtKind::FuncDef { name, body, params, .. } | StmtKind::MethodDef { name, body, params, .. } => {
                info.control = true;
                info.is_fn = true;
                self.fn_params.entry(name.clone()).or_default().push(params.iter().map(|p| p.name.clone()).collect());
                self.fn_defs.entry(name.clone()).or_default().push(key);
                if let Some(last) = body.stmts.last() {
                    self.fn_returns.entry(name.clone()).or_default().push(key_of(last));
                }
                child_blocks.push(body);
            }
            _ => {}
        }
        let embedded_blocks: Vec<&Block> = scan.blocks.drain(..).collect();
        // Changing an array in place writes its variable: kept whenever the variable is.
        info.writes.extend(scan.mutated);
        info.effect = scan.effect
            || scan.method_sites.iter().any(|at| matches!(self.resolved.get(at), Some(Resolved::Builtin(b) | Resolved::Dispatch(b)) if draws(b)));
        info.fn_args = scan.fn_args;
        info.reads = scan.reads;
        info.calls = scan.calls;
        info.noncausal = scan.noncausal;
        self.stmts.insert(key, info);
        self.order.push(key);
        let fname = match &s.kind {
            StmtKind::FuncDef { name, .. } | StmtKind::MethodDef { name, .. } => Some(name.as_str()),
            _ => func,
        };
        let is_fn = matches!(s.kind, StmtKind::FuncDef { .. } | StmtKind::MethodDef { .. });
        for b in child_blocks {
            if let (false, Some(last)) = (is_fn, b.stmts.last()) {
                self.block_lasts.entry(key).or_default().push(key_of(last));
            }
            self.walk_block(b, key, fname);
        }
        for b in embedded_blocks {
            for c in &b.stmts {
                self.embedded_children.entry(key).or_default().push(key_of(c));
            }
            if let Some(last) = b.stmts.last() {
                self.embedded_lasts.insert(key_of(last));
            }
            self.walk_block(b, key, fname);
        }
    }
}

#[derive(Clone)]
struct Keep<'f> {
    facts: &'f Facts,
    stmts: BTreeSet<StmtKey>,
    /// Statements kept as values: a control statement among them keeps its arms' results.
    valued: BTreeSet<StmtKey>,
    vars: BTreeSet<String>,
    fns: BTreeSet<String>,
    writers: BTreeMap<String, Vec<StmtKey>>,
    /// Names that may share an array (`b = a`), both ways.
    aliases: BTreeMap<String, BTreeSet<String>>,
}

impl Keep<'_> {
    fn stmt(&mut self, k: StmtKey) {
        if !self.stmts.insert(k) {
            return;
        }
        let Some(info) = self.facts.stmts.get(&k) else { return };
        for r in &info.reads {
            self.var(r);
        }
        // A kept write needs the variable's declaration (and its other writers).
        for w in &info.writes {
            self.var(w);
        }
        for f in &info.calls {
            self.func(f);
        }
        if let Some(p) = info.parent {
            self.stmt(p);
        }
        for c in self.facts.embedded_children.get(&k).cloned().unwrap_or_default() {
            if self.facts.embedded_lasts.contains(&c) {
                self.value(c);
            } else {
                self.stmt(c);
            }
        }
    }

    /// Keep a statement whose value is used: for `if`/`switch`/`for`/`while` that is the last
    /// statement of every arm, recursively.
    fn value(&mut self, k: StmtKey) {
        self.stmt(k);
        if !self.valued.insert(k) {
            return;
        }
        for last in self.facts.block_lasts.get(&k).cloned().unwrap_or_default() {
            self.value(last);
        }
    }

    fn var(&mut self, v: &str) {
        if !self.vars.insert(v.to_string()) {
            return;
        }
        for k in self.writers.get(v).cloned().unwrap_or_default() {
            self.stmt(k);
        }
        // An array changed under another name changes this one.
        for a in self.aliases.get(v).cloned().unwrap_or_default() {
            self.var(&a);
        }
    }

    fn func(&mut self, f: &str) {
        if !self.fns.insert(f.to_string()) {
            return;
        }
        for k in self.facts.fn_returns.get(f).cloned().unwrap_or_default() {
            self.value(k);
        }
        for k in self.facts.fn_side_writers.get(f).cloned().unwrap_or_default() {
            self.stmt(k);
        }
        // A function that draws is called for its drawings.
        for k in self.facts.effects.get(&Some(f.to_string())).cloned().unwrap_or_default() {
            self.stmt(k);
        }
    }
}

fn plot_style(e: &Expr) -> PlotStyle {
    let name = match &e.kind {
        ExprKind::Name(p) => p.join("."),
        _ => return PlotStyle::Line,
    };
    match name.as_str() {
        "plot.style_linebr" => PlotStyle::LineBr,
        "plot.style_stepline" | "plot.style_stepline_diamond" | "plot.style_steplinebr" => PlotStyle::StepLine,
        "plot.style_histogram" => PlotStyle::Histogram,
        "plot.style_columns" => PlotStyle::Columns,
        "plot.style_area" => PlotStyle::Area,
        "plot.style_areabr" => PlotStyle::AreaBr,
        "plot.style_circles" => PlotStyle::Circles,
        "plot.style_cross" => PlotStyle::Cross,
        _ => PlotStyle::Line,
    }
}

/// Causal only without an offset or with a non-negative literal one (an input or an
/// expression may be negative).
fn offset_causal(e: Option<&Expr>) -> bool {
    match e.map(|e| &e.kind) {
        None => true,
        Some(ExprKind::Int(v)) => *v >= 0,
        Some(ExprKind::Num(v)) => *v >= 0.0,
        Some(_) => false,
    }
}

/// Top-level `plot`/`indicator` calls, in source order.
fn top_calls<'s>(script: &'s Script, name: &str) -> Vec<&'s Stmt> {
    script
        .body
        .iter()
        .filter(|s| matches!(&s.kind, StmtKind::Expr(Expr { kind: ExprKind::Call { callee, .. }, .. }) if callee.len() == 1 && callee[0] == name))
        .collect()
}

fn bound_arg<'e>(name: &str, call: &'e Expr, param: &str) -> Option<&'e Expr> {
    let ExprKind::Call { args, .. } = &call.kind else { return None };
    let Lookup::Supported(b) = lookup_call(name) else { return None };
    let bound = types::bind_args(b.params, b.variadic, args).ok()?;
    let pi = b.params.iter().position(|p| p.name == param)?;
    bound.slots[pi].map(|ai| &args[ai].value)
}

fn str_lit(e: Option<&Expr>) -> Option<String> {
    match e.map(|e| &e.kind) {
        Some(ExprKind::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

pub fn apply(script: &Script, checked: &mut Checked) {
    let mut facts = Facts { resolved: checked.resolved.clone(), ..Default::default() };
    for s in &script.body {
        facts.walk(s, None, None);
    }
    facts.propagate_param_writes();
    facts.find_effects();

    // ── Kept statements ──
    let mut writers: BTreeMap<String, Vec<StmtKey>> = BTreeMap::new();
    for (k, info) in &facts.stmts {
        for w in &info.writes {
            writers.entry(w.clone()).or_default().push(*k);
        }
    }
    let mut aliases: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for info in facts.stmts.values() {
        for a in &info.alias_of {
            for w in &info.writes {
                aliases.entry(w.clone()).or_default().insert(a.clone());
                aliases.entry(a.clone()).or_default().insert(w.clone());
            }
        }
    }
    let mut keep = Keep { facts: &facts, stmts: BTreeSet::new(), valued: BTreeSet::new(), vars: BTreeSet::new(), fns: BTreeSet::new(), writers, aliases };
    let roots: Vec<StmtKey> = facts.order.iter().copied().filter(|k| facts.stmts[k].root).collect();
    for k in roots {
        keep.stmt(k);
    }
    // A drawing is kept unless what it needs is not supported yet (`request.security`): then
    // it is skipped with that feature named, and the script still runs.
    let blocked: BTreeSet<StmtKey> = checked
        .diags
        .iter()
        .zip(&checked.diag_stmts)
        .filter(|(d, _)| d.code == DiagCode::Unsupported)
        .filter_map(|(_, t)| *t)
        .chain(checked.visual_uses.iter().filter_map(|v| v.2))
        .collect();
    for k in facts.effects.get(&None).cloned().unwrap_or_default() {
        if keep.stmts.contains(&k) {
            continue;
        }
        let mut trial = keep.clone();
        trial.stmt(k);
        if !trial.stmts.difference(&keep.stmts).any(|s| blocked.contains(s)) {
            keep = trial;
        }
    }
    // A `break`/`continue` shapes its loop: kept with the loop.
    loop {
        let jumps: Vec<StmtKey> = facts
            .order
            .iter()
            .copied()
            .filter(|k| facts.stmts[k].jump && !keep.stmts.contains(k))
            .filter(|k| {
                let mut p = facts.stmts[k].parent;
                while let Some(pk) = p {
                    if facts.stmts[&pk].is_loop {
                        return keep.stmts.contains(&pk);
                    }
                    p = facts.stmts[&pk].parent;
                }
                false
            })
            .collect();
        if jumps.is_empty() {
            break;
        }
        for k in jumps {
            keep.stmt(k);
        }
    }
    let kept = keep.stmts;
    checked.dropped = facts.order.iter().copied().filter(|k| !kept.contains(k)).collect();

    // ── Rewrite diagnostics: unsupported in dropped code → warning; visual in kept → error ──
    let mut skipped: BTreeMap<String, (Span, usize)> = BTreeMap::new();
    let mut diags = Vec::new();
    for (d, tag) in checked.diags.drain(..).zip(checked.diag_stmts.drain(..)) {
        let dropped = tag.is_some_and(|t| checked.dropped.contains(&t));
        if dropped && d.code == DiagCode::Unsupported {
            let e = skipped.entry(d.args[0].clone()).or_insert((d.span, 0));
            e.1 += 1;
        } else {
            diags.push(d);
        }
    }
    for (name, span, tag) in &checked.visual_uses {
        if tag.is_some_and(|t| checked.dropped.contains(&t)) {
            let e = skipped.entry(name.clone()).or_insert((*span, 0));
            e.1 += 1;
        } else {
            let phase = match lookup(name) {
                Lookup::NotYet { phase, .. } => phase,
                _ => 5,
            };
            diags.push(Diagnostic::error(DiagCode::Unsupported, *span, vec![name.clone(), phase.to_string()]));
        }
    }

    // ── Causality ──
    // Each tainted statement, variable, function and loop keeps the root that reached it
    // first: a direct read, then a read variable, then a called function, then an enclosing
    // condition or loop.
    let mut tainted: BTreeMap<String, Taint> = BTreeMap::new();
    let mut tainted_fns: BTreeMap<String, Taint> = BTreeMap::new();
    let mut tainted_stmts: BTreeMap<StmtKey, Taint> = BTreeMap::new();
    let mut tainted_loops: BTreeMap<StmtKey, Taint> = BTreeMap::new();
    loop {
        let before = (tainted.len(), tainted_fns.len(), tainted_stmts.len(), tainted_loops.len());
        for k in &facts.order {
            let info = &facts.stmts[k];
            let mut t = info
                .noncausal
                .clone()
                .or_else(|| info.reads.iter().find_map(|r| tainted.get(r).cloned()))
                .or_else(|| info.calls.iter().find_map(|c| tainted_fns.get(c).cloned()));
            // Control dependence: under a non-causal condition, or inside a loop a jump under
            // such a condition can cut short.
            let mut p = info.parent;
            while let (None, Some(pk)) = (&t, p) {
                let pi = &facts.stmts[&pk];
                if pi.control && !pi.is_fn {
                    t = tainted_stmts.get(&pk).cloned();
                }
                if t.is_none() {
                    t = tainted_loops.get(&pk).cloned();
                }
                p = pi.parent;
            }
            let Some(t) = t else { continue };
            if info.jump {
                let mut p = info.parent;
                while let Some(pk) = p {
                    if facts.stmts[&pk].is_loop {
                        tainted_loops.entry(pk).or_insert_with(|| t.clone());
                        break;
                    }
                    p = facts.stmts[&pk].parent;
                }
            }
            tainted_stmts.entry(*k).or_insert_with(|| t.clone());
            for w in &info.writes {
                tainted.entry(w.clone()).or_insert_with(|| t.clone());
            }
            if let Some(f) = &info.func {
                tainted_fns.entry(f.clone()).or_insert_with(|| t.clone());
            }
        }
        if before == (tainted.len(), tainted_fns.len(), tainted_stmts.len(), tainted_loops.len()) {
            break;
        }
    }

    // ── Plots and indicator() metadata ──
    for (index, s) in top_calls(script, "plot").into_iter().enumerate() {
        let StmtKind::Expr(call) = &s.kind else { continue };
        let negative = !offset_causal(bound_arg("plot", call, "offset"));
        let taint = tainted_stmts
            .get(&key_of(s))
            .cloned()
            .or_else(|| negative.then(|| Taint { feature: "offset".into(), line: s.span.line }));
        checked.meta.plots.push(PlotMeta {
            index,
            title: str_lit(bound_arg("plot", call, "title")).unwrap_or_else(|| "Plot".into()),
            style: bound_arg("plot", call, "style").map(plot_style).unwrap_or(PlotStyle::Line),
            causal: taint.is_none(),
            taint,
        });
    }
    for out in OUTPUTS {
        for s in top_calls(script, out) {
            if let StmtKind::Expr(Expr { kind: ExprKind::Call { args, .. }, .. }) = &s.kind {
                for a in args {
                    if let Some(n) = a.name.as_deref().filter(|n| COSMETIC.contains(n)) {
                        let e = skipped.entry(n.to_string()).or_insert((s.span, 0));
                        e.1 += 1;
                    }
                }
            }
        }
    }
    if let Some(s) = top_calls(script, "indicator").first() {
        if let StmtKind::Expr(call) = &s.kind {
            checked.meta.title = str_lit(bound_arg("indicator", call, "title")).unwrap_or_default();
            checked.meta.shorttitle = str_lit(bound_arg("indicator", call, "shorttitle"));
            checked.meta.overlay = matches!(bound_arg("indicator", call, "overlay").map(|e| &e.kind), Some(ExprKind::Bool(true)));
            checked.meta.precision = match bound_arg("indicator", call, "precision").map(|e| &e.kind) {
                Some(ExprKind::Int(v)) => u8::try_from(*v).ok(),
                _ => None,
            };
            if let Some(ExprKind::Int(v)) = bound_arg("indicator", call, "max_bars_back").map(|e| &e.kind) {
                if *v > MAX_BARS_BACK as i64 {
                    diags.push(Diagnostic::error(DiagCode::OutOfRange, s.span, vec!["max_bars_back".into(), MAX_BARS_BACK.to_string()]));
                }
            }
            checked.meta.max_bars_back = match bound_arg("indicator", call, "max_bars_back").map(|e| &e.kind) {
                Some(ExprKind::Int(v)) => usize::try_from(*v).ok(),
                _ => None,
            };
            // Pine's drawing counts: 50 by default, 500 at most (more is refused, never clamped).
            for (i, arg) in ["max_labels_count", "max_lines_count", "max_boxes_count", "max_polylines_count"].into_iter().enumerate() {
                if let Some(ExprKind::Int(v)) = bound_arg("indicator", call, arg).map(|e| &e.kind) {
                    if !(1..=500).contains(v) {
                        diags.push(Diagnostic::error(DiagCode::OutOfRange, s.span, vec![arg.into(), "500".into()]));
                    }
                    checked.draw_limits[i] = (*v).clamp(1, 500) as usize;
                }
            }
        }
    }

    for (name, (span, count)) in skipped {
        diags.push(Diagnostic::warning(DiagCode::SkippedVisual, span, vec![name, count.to_string()]));
    }
    checked.diags = diags;
    checked.diag_stmts = vec![None; checked.diags.len()];
}
