//! The range each plot can take, known at compile time: a chart frames a bounded plot (an RSI,
//! a stochastic, a correlation) whole instead of zooming on the window's values.
//!
//! Interval analysis over the syntax tree. Builtins with a known range seed it (`ta.rsi` is
//! 0..100, `ta.wpr` -100..0), averages and extremes keep their source's range, and arithmetic,
//! `math.min`/`math.max`, ternaries, `if`/`switch` values and user functions carry it on.
//! Anything else (a price, a sum, a value the analysis does not follow) is unbounded, and a
//! plot is only bounded when both ends are finite: a range is never guessed.
//!
//! Path-insensitive: every value a variable is assigned anywhere joins its range, conditions
//! refine nothing. Variables are keyed by name across scopes and a user function's parameters
//! take the join of every call's arguments, so a name collision only widens. A variable still
//! moving after `WIDEN_AFTER` rounds (`x := x + 1`) is unbounded.

use std::collections::HashMap;

use super::types;
use crate::ast::*;
use crate::registry::{lookup, lookup_call, Lookup};

/// Rounds of plain joins before a variable still changing is widened to unbounded.
const WIDEN_AFTER: usize = 8;
/// Safety cap: past it the analysis gives up and no plot is bounded.
const MAX_ROUNDS: usize = 64;

/// A closed interval over the extended reals. `lo > hi` is the empty one (`na` only).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Iv {
    pub lo: f64,
    pub hi: f64,
}

const INF: f64 = f64::INFINITY;

impl Iv {
    pub const EMPTY: Iv = Iv { lo: INF, hi: -INF };
    pub const TOP: Iv = Iv { lo: -INF, hi: INF };
    const BOOL: Iv = Iv { lo: 0.0, hi: 1.0 };
    const NONNEG: Iv = Iv { lo: 0.0, hi: INF };

    fn new(lo: f64, hi: f64) -> Iv {
        if lo.is_nan() || hi.is_nan() {
            Iv::TOP
        } else {
            Iv { lo, hi }
        }
    }
    fn point(v: f64) -> Iv {
        if v.is_finite() {
            Iv { lo: v, hi: v }
        } else {
            Iv::TOP
        }
    }
    fn is_empty(self) -> bool {
        self.lo > self.hi
    }
    pub fn join(self, o: Iv) -> Iv {
        Iv { lo: self.lo.min(o.lo), hi: self.hi.max(o.hi) }
    }
    /// Both ends finite and apart: what a chart can frame.
    pub fn bounded(self) -> Option<[f64; 2]> {
        (self.lo.is_finite() && self.hi.is_finite() && self.lo < self.hi).then_some([self.lo, self.hi])
    }
    fn neg(self) -> Iv {
        if self.is_empty() {
            return self;
        }
        Iv::new(-self.hi, -self.lo)
    }
    fn add(self, o: Iv) -> Iv {
        if self.is_empty() || o.is_empty() {
            return Iv::EMPTY;
        }
        Iv::new(self.lo + o.lo, self.hi + o.hi)
    }
    fn sub(self, o: Iv) -> Iv {
        self.add(o.neg())
    }
    fn mul(self, o: Iv) -> Iv {
        if self.is_empty() || o.is_empty() {
            return Iv::EMPTY;
        }
        // 0 × ∞ is 0: the operands are finite values, an infinite end only says "unbounded".
        let m = |a: f64, b: f64| if a == 0.0 || b == 0.0 { 0.0 } else { a * b };
        let p = [m(self.lo, o.lo), m(self.lo, o.hi), m(self.hi, o.lo), m(self.hi, o.hi)];
        Iv::new(p.iter().copied().fold(INF, f64::min), p.iter().copied().fold(-INF, f64::max))
    }
    /// Division by zero is `na` in Pine, so a divisor touching 0 at one end only loses that end.
    fn div(self, o: Iv) -> Iv {
        if self.is_empty() || o.is_empty() {
            return Iv::EMPTY;
        }
        if o.lo == 0.0 && o.hi == 0.0 {
            return Iv::EMPTY;
        }
        if o.lo < 0.0 && o.hi > 0.0 {
            return Iv::TOP;
        }
        let recip = |d: f64, neg: bool| {
            if d == 0.0 {
                if neg {
                    -INF
                } else {
                    INF
                }
            } else {
                1.0 / d
            }
        };
        let neg = o.hi <= 0.0;
        self.mul(Iv::new(recip(o.hi, neg), recip(o.lo, neg)))
    }
    fn abs(self) -> Iv {
        if self.is_empty() || self.lo >= 0.0 {
            self
        } else if self.hi <= 0.0 {
            self.neg()
        } else {
            Iv::new(0.0, (-self.lo).max(self.hi))
        }
    }
    /// `a - a[n]`: as far as the range spans, either way.
    fn change(self) -> Iv {
        self.sub(self)
    }
    /// A spread (`ta.stdev`, `ta.range`): 0 up to the range's width.
    fn spread(self) -> Iv {
        if self.is_empty() {
            return self;
        }
        Iv::new(0.0, self.hi - self.lo)
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Abs {
    One(Iv),
    Many(Vec<Iv>),
}

impl Abs {
    fn iv(&self) -> Iv {
        match self {
            Abs::One(i) => *i,
            Abs::Many(v) => v.iter().fold(Iv::EMPTY, |a, b| a.join(*b)),
        }
    }
    fn at(&self, k: usize) -> Iv {
        match self {
            Abs::One(i) => *i,
            Abs::Many(v) => v.get(k).copied().unwrap_or(Iv::TOP),
        }
    }
    fn join(&self, o: &Abs) -> Abs {
        match (self, o) {
            (Abs::One(e), x) | (x, Abs::One(e)) if e.is_empty() => x.clone(),
            (Abs::Many(a), Abs::Many(b)) if a.len() == b.len() => Abs::Many(a.iter().zip(b).map(|(x, y)| x.join(*y)).collect()),
            (Abs::One(a), Abs::One(b)) => Abs::One(a.join(*b)),
            _ => Abs::One(Iv::TOP),
        }
    }
}

/// A builtin's own range, whatever its inputs.
fn fixed(name: &str) -> Option<Iv> {
    Some(match name {
        "ta.rsi" | "ta.stoch" | "ta.mfi" | "ta.percentrank" => Iv::new(0.0, 100.0),
        "ta.wpr" => Iv::new(-100.0, 0.0),
        "ta.correlation" | "ta.tsi" | "math.sign" => Iv::new(-1.0, 1.0),
        "ta.atr" | "ta.tr" | "ta.barssince" | "ta.variance" => Iv::NONNEG,
        "ta.crossover" | "ta.crossunder" | "ta.cross" | "ta.rising" | "ta.falling" | "na" | "bool" => Iv::BOOL,
        _ => return None,
    })
}

/// Builtins whose value stays within their `source`: averages, extremes, picks.
const KEEPS_SOURCE: &[&str] = &[
    "ta.sma",
    "ta.ema",
    "ta.rma",
    "ta.wma",
    "ta.vwma",
    "ta.swma",
    "ta.alma",
    "ta.highest",
    "ta.lowest",
    "ta.median",
    "ta.mode",
    "ta.max",
    "ta.min",
    "ta.percentile_linear_interpolation",
    "ta.percentile_nearest_rank",
    "ta.valuewhen",
    "ta.pivothigh",
    "ta.pivotlow",
    "fixnan",
    "int",
    "float",
];

#[derive(Default)]
struct Env {
    vars: HashMap<String, Iv>,
    /// A user function's return value.
    rets: HashMap<String, Abs>,
}

pub struct Ranges<'s> {
    env: Env,
    fns: HashMap<&'s str, &'s [Param]>,
    /// Names widened to unbounded for good.
    widened: Vec<String>,
    failed: bool,
}

impl<'s> Ranges<'s> {
    /// Run the analysis over the whole script.
    pub fn of(script: &'s Script) -> Ranges<'s> {
        let mut r = Ranges { env: Env::default(), fns: HashMap::new(), widened: Vec::new(), failed: false };
        for s in &script.body {
            if let StmtKind::FuncDef { name, params, .. } = &s.kind {
                r.fns.insert(name.as_str(), params.as_slice());
            }
        }
        for round in 0.. {
            if round >= MAX_ROUNDS {
                r.failed = true;
                break;
            }
            let before_vars = r.env.vars.clone();
            let before_rets = r.env.rets.clone();
            r.block(&script.body);
            if r.env.vars == before_vars && r.env.rets == before_rets {
                break;
            }
            if round + 1 >= WIDEN_AFTER {
                let moved: Vec<String> = r.env.vars.iter().filter(|(k, v)| before_vars.get(*k) != Some(v)).map(|(k, _)| k.clone()).collect();
                for k in moved {
                    r.env.vars.insert(k.clone(), Iv::TOP);
                    r.widened.push(k);
                }
                let moved: Vec<String> = r.env.rets.iter().filter(|(k, v)| before_rets.get(*k) != Some(v)).map(|(k, _)| k.clone()).collect();
                for k in moved {
                    r.env.rets.insert(k, Abs::One(Iv::TOP));
                }
            }
        }
        r
    }

    /// The range of a plot's `series` argument, when both ends are known.
    pub fn plot(&mut self, series: Option<&'s Expr>) -> Option<[f64; 2]> {
        if self.failed {
            return None;
        }
        self.expr(series?).iv().bounded()
    }

    fn set(&mut self, name: &str, v: Iv) {
        if self.widened.iter().any(|w| w == name) {
            return;
        }
        let e = self.env.vars.entry(name.to_string()).or_insert(Iv::EMPTY);
        *e = e.join(v);
    }

    fn var(&self, name: &str) -> Iv {
        self.env.vars.get(name).copied().unwrap_or(Iv::EMPTY)
    }

    /// Every statement of a block; its value is the last one's.
    fn block(&mut self, stmts: &'s [Stmt]) -> Abs {
        let mut last = Abs::One(Iv::EMPTY);
        for s in stmts {
            last = self.stmt(s);
        }
        last
    }

    fn stmt(&mut self, s: &'s Stmt) -> Abs {
        match &s.kind {
            StmtKind::Decl { name, value, .. } => {
                let v = self.expr(value);
                self.set(name, v.iv());
                v
            }
            StmtKind::TupleDecl { names, value } => {
                let v = self.expr(value);
                for (k, n) in names.iter().enumerate() {
                    self.set(n, v.at(k));
                }
                v
            }
            StmtKind::Assign { target, op, value } => {
                let v = self.expr(value).iv();
                if let [name] = target.as_slice() {
                    let cur = self.var(name);
                    let next = match op {
                        AssignOp::Reassign => v,
                        AssignOp::Add => cur.add(v),
                        AssignOp::Sub => cur.sub(v),
                        AssignOp::Mul => cur.mul(v),
                        AssignOp::Div => cur.div(v),
                        AssignOp::Mod => Iv::TOP,
                    };
                    self.set(name, next);
                    return Abs::One(next);
                }
                Abs::One(v)
            }
            StmtKind::Expr(e) => self.expr(e),
            StmtKind::FuncDef { name, body, .. } => {
                let v = self.block(&body.stmts);
                let e = self.env.rets.entry(name.clone()).or_insert(Abs::One(Iv::EMPTY));
                *e = e.join(&v);
                Abs::One(Iv::EMPTY)
            }
            StmtKind::MethodDef { body, .. } => {
                self.block(&body.stmts);
                Abs::One(Iv::EMPTY)
            }
            _ => Abs::One(Iv::EMPTY),
        }
    }

    fn expr(&mut self, e: &'s Expr) -> Abs {
        let one = Abs::One;
        match &e.kind {
            ExprKind::Num(v) => one(Iv::point(*v)),
            ExprKind::Int(v) => one(Iv::point(*v as f64)),
            ExprKind::Bool(_) => one(Iv::BOOL),
            ExprKind::Na => one(Iv::EMPTY),
            ExprKind::Str(_) | ExprKind::Color(_) => one(Iv::TOP),
            ExprKind::Name(path) => one(self.name(path)),
            ExprKind::Index { base, offset } => {
                self.expr(offset);
                self.expr(base)
            }
            ExprKind::Unary { op, e } => {
                let v = self.expr(e).iv();
                one(match op {
                    UnOp::Neg => v.neg(),
                    UnOp::Plus => v,
                    UnOp::Not => Iv::BOOL,
                })
            }
            ExprKind::Binary { op, l, r } => {
                let (a, b) = (self.expr(l).iv(), self.expr(r).iv());
                one(match op {
                    BinOp::Add => a.add(b),
                    BinOp::Sub => a.sub(b),
                    BinOp::Mul => a.mul(b),
                    BinOp::Div => a.div(b),
                    BinOp::Mod => Iv::TOP,
                    _ => Iv::BOOL,
                })
            }
            ExprKind::Ternary { c, a, b } => {
                self.expr(c);
                let a = self.expr(a);
                let b = self.expr(b);
                a.join(&b)
            }
            ExprKind::Tuple(items) => Abs::Many(items.iter().map(|i| self.expr(i).iv()).collect()),
            ExprKind::If(f) => {
                self.expr(&f.cond);
                let mut v = self.block(&f.then.stmts);
                for (c, b) in &f.elifs {
                    self.expr(c);
                    v = v.join(&self.block(&b.stmts));
                }
                match &f.els {
                    Some(b) => v.join(&self.block(&b.stmts)),
                    // No `else`: `na` when no branch runs, which adds nothing.
                    None => v,
                }
            }
            ExprKind::Switch(s) => {
                if let Some(k) = &s.key {
                    self.expr(k);
                }
                let mut v: Option<Abs> = None;
                for (p, b) in &s.arms {
                    if let Some(p) = p {
                        self.expr(p);
                    }
                    let bv = self.block(&b.stmts);
                    v = Some(match v {
                        Some(x) => x.join(&bv),
                        None => bv,
                    });
                }
                v.unwrap_or(one(Iv::EMPTY))
            }
            ExprKind::For(f) => {
                let (a, b) = (self.expr(&f.from).iv(), self.expr(&f.to).iv());
                if let Some(by) = &f.by {
                    self.expr(by);
                }
                self.set(&f.var, a.join(b));
                self.block(&f.body.stmts);
                one(Iv::TOP)
            }
            ExprKind::ForIn(f) => {
                self.expr(&f.iter);
                if let Some(i) = &f.idx {
                    self.set(i, Iv::NONNEG);
                }
                self.set(&f.item, Iv::TOP);
                self.block(&f.body.stmts);
                one(Iv::TOP)
            }
            ExprKind::While(w) => {
                self.expr(&w.cond);
                self.block(&w.body.stmts);
                one(Iv::TOP)
            }
            ExprKind::Call { callee, args, .. } => self.call(callee, args),
            ExprKind::MethodCall { recv, args, .. } => {
                self.expr(recv);
                for a in args {
                    self.expr(&a.value);
                }
                one(Iv::TOP)
            }
            ExprKind::Field { recv, .. } => {
                self.expr(recv);
                one(Iv::TOP)
            }
        }
    }

    fn name(&self, path: &[String]) -> Iv {
        if let [n] = path {
            if let Some(v) = self.env.vars.get(n) {
                return *v;
            }
            // A variable of the script not assigned yet in this round: nothing so far.
            if matches!(lookup(n), Lookup::Unknown) {
                return Iv::EMPTY;
            }
        }
        match path.join(".").as_str() {
            "bar_index" | "last_bar_index" | "volume" | "ta.tr" | "ta.obv" => Iv::NONNEG,
            "barstate.isfirst" | "barstate.islast" | "barstate.isconfirmed" | "barstate.isnew" | "barstate.isrealtime" | "barstate.ishistory" => Iv::BOOL,
            _ => Iv::TOP,
        }
    }

    fn call(&mut self, callee: &'s [String], args: &'s [Arg]) -> Abs {
        let vals: Vec<Abs> = args.iter().map(|a| self.expr(&a.value)).collect();
        if let [n] = callee {
            if let Some(params) = self.fns.get(n.as_str()).copied() {
                for (k, a) in args.iter().enumerate() {
                    let pi = match &a.name {
                        Some(name) => params.iter().position(|p| &p.name == name),
                        None => Some(k),
                    };
                    if let Some(p) = pi.and_then(|i| params.get(i)) {
                        self.set(&p.name, vals[k].iv());
                    }
                }
                // A parameter left to its default takes the default's range.
                for (i, p) in params.iter().enumerate() {
                    let passed = args.iter().enumerate().any(|(k, a)| a.name.as_deref() == Some(&p.name) || (a.name.is_none() && k == i));
                    if !passed {
                        let v = p.default.as_ref().map_or(Iv::TOP, |d| self.expr(d).iv());
                        self.set(&p.name, v);
                    }
                }
                return self.env.rets.get(n.as_str()).cloned().unwrap_or(Abs::One(Iv::EMPTY));
            }
        }
        let name = callee.join(".");
        // An argument by parameter name, as the builtin's signature binds it.
        let arg = |param: &str| -> Option<Iv> {
            let Lookup::Supported(b) = lookup_call(&name) else { return None };
            let bound = types::bind_args(b.params, b.variadic, args).ok()?;
            let pi = b.params.iter().position(|p| p.name == param)?;
            bound.slots[pi].map(|ai| vals[ai].iv())
        };
        let all = || vals.iter().fold(Iv::EMPTY, |a, v| a.join(v.iv()));
        if let Some(v) = fixed(&name) {
            return Abs::One(v);
        }
        if KEEPS_SOURCE.contains(&name.as_str()) {
            let src = arg("source").or_else(|| vals.first().map(Abs::iv));
            // `ta.highest(10)` / `ta.pivothigh(2, 2)`: the source defaults to a price.
            let defaulted = matches!(name.as_str(), "ta.highest" | "ta.lowest" | "ta.pivothigh" | "ta.pivotlow") && arg("source").is_none();
            return Abs::One(if defaulted { Iv::TOP } else { src.unwrap_or(Iv::TOP) });
        }
        Abs::One(match name.as_str() {
            "nz" => arg("source").unwrap_or(Iv::TOP).join(arg("replacement").unwrap_or(Iv::point(0.0))),
            "ta.change" | "ta.mom" => arg("source").map_or(Iv::TOP, Iv::change),
            "ta.stdev" | "ta.dev" | "ta.range" => arg("source").map_or(Iv::NONNEG, Iv::spread),
            "math.abs" => arg("number").map_or(Iv::TOP, Iv::abs),
            "math.max" => vals.iter().map(Abs::iv).reduce(|a, b| Iv::new(a.lo.max(b.lo), a.hi.max(b.hi))).unwrap_or(Iv::TOP),
            "math.min" => vals.iter().map(Abs::iv).reduce(|a, b| Iv::new(a.lo.min(b.lo), a.hi.min(b.hi))).unwrap_or(Iv::TOP),
            "math.avg" => all(),
            "math.round" | "math.floor" | "math.ceil" => arg("number").map_or(Iv::TOP, |v| Iv::new(v.lo.floor(), v.hi.ceil())),
            "math.sqrt" => arg("number").map_or(Iv::TOP, |v| Iv::new(libm::sqrt(v.lo.max(0.0)), libm::sqrt(v.hi))),
            // An input the user may set anywhere within its declared bounds.
            "input.int" | "input.float" if arg("options").is_none() => {
                let lo = arg("minval").map_or(-INF, |v| v.lo);
                let hi = arg("maxval").map_or(INF, |v| v.hi);
                Iv::new(lo, hi)
            }
            "ta.dmi" => return Abs::Many(vec![Iv::new(0.0, 100.0); 3]),
            "ta.supertrend" => return Abs::Many(vec![Iv::TOP, Iv::new(-1.0, 1.0)]),
            _ => Iv::TOP,
        })
    }
}

#[cfg(test)]
mod tests {
    fn ranges(src: &str) -> Vec<Option<[f64; 2]>> {
        crate::compile(src).expect("compiles").meta.plots.iter().map(|p| p.range).collect()
    }

    #[test]
    fn builtin_oscillators_are_bounded() {
        let r = ranges("//@version=5\nindicator(\"t\")\nplot(ta.rsi(close, 14))\nplot(ta.wpr(14))\nplot(ta.correlation(close, open, 20))\nplot(ta.cci(close, 20))\n");
        assert_eq!(r, vec![Some([0.0, 100.0]), Some([-100.0, 0.0]), Some([-1.0, 1.0]), None]);
    }

    #[test]
    fn ranges_flow_through_variables_and_arithmetic() {
        let src = "//@version=5\nindicator(\"t\")\nlen = input.int(14, minval=1)\nr = ta.rsi(close, len)\nsig = ta.sma(r, 9)\nplot(sig)\nplot(r - 50)\nplot((ta.stoch(close, high, low, 14) - 50) * 2)\nplot(math.max(r, 30))\nplot(r > 50 ? r : na)\n";
        assert_eq!(ranges(src), vec![Some([0.0, 100.0]), Some([-50.0, 50.0]), Some([-100.0, 100.0]), Some([30.0, 100.0]), Some([0.0, 100.0])]);
    }

    #[test]
    fn hand_written_rsi_is_bounded() {
        let src = "//@version=5\nindicator(\"t\")\nsrc = close\nup = ta.rma(math.max(ta.change(src), 0), 14)\ndown = ta.rma(-math.min(ta.change(src), 0), 14)\nrsi = down == 0 ? 100 : up == 0 ? 0 : 100 - (100 / (1 + up / down))\nplot(rsi)\n";
        assert_eq!(ranges(src), vec![Some([0.0, 100.0])]);
    }

    #[test]
    fn user_functions_and_tuples() {
        let src = "//@version=5\nindicator(\"t\")\nf(x) => ta.ema(x, 3)\ng() => [ta.rsi(close, 14), close]\n[a, b] = g()\n[p, m, adx] = ta.dmi(14, 14)\nplot(f(ta.mfi(hlc3, 14)))\nplot(a)\nplot(b)\nplot(adx)\n";
        assert_eq!(ranges(src), vec![Some([0.0, 100.0]), Some([0.0, 100.0]), None, Some([0.0, 100.0])]);
    }

    #[test]
    fn prices_and_growing_state_stay_unbounded() {
        let src = "//@version=5\nindicator(\"t\")\nvar float n = 0\nn := n + 1\nplot(n)\nplot(close)\nplot(ta.sma(close, 20))\nplot(ta.rsi(close, 14) + close)\n";
        assert_eq!(ranges(src), vec![None, None, None, None]);
    }
}
