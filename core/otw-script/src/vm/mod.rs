//! Bar-by-bar interpreter.
//!
//! Every block and function body runs in an *instance* of its template: its variables are
//! series that advance once per execution of that instance, its stateful builtin calls keep
//! their own state. The global instance executes once per bar; a block's instance once per run
//! of the block; a function call site has its own instance (Pine's call-site semantics).
//! Instances live in an arena and are addressed by index; `stack` holds the lexical frames.

pub mod heap;
pub mod series;
pub mod value;

use std::rc::Rc;

use crate::ast::{AssignOp, BinOp};
use crate::compile::{BlockRef, CExpr, CStmt, Program, VarRef};
use crate::diag::{RtCode, RuntimeError, Span};
use crate::input::RunInput;
use crate::output::RunOutput;
use crate::stdlib::{self, State};
use series::Series;
use value::Value;

/// How the bar being executed stands (spec §4.2, Pine's realtime model).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarMode {
    /// A confirmed bar run before any realtime update (a full run, a session's open).
    History,
    /// The forming bar; `first` on its first execution.
    Forming { first: bool },
    /// A bar committed by a session update; `was_forming` when it ran as the forming bar before.
    Commit { was_forming: bool },
}

/// What a native function sees of the run.
pub struct Ctx<'r> {
    pub input: &'r RunInput,
    pub bar: usize,
    pub version: u8,
    pub mode: BarMode,
    /// Index of the last bar run as `History` (`barstate.islastconfirmedhistory`).
    pub last_history: Option<usize>,
    pub out: RunOutput,
    /// Arrays and other reference objects.
    pub heap: heap::Heap,
    /// Deepest history a script may read (`max_bars_back` or the limit).
    pub max_history: usize,
    /// Live labels, lines, boxes and polylines at most (`indicator(max_*_count)`).
    pub draw_limits: [usize; 4],
    ops: u64,
    loop_iters: u32,
    #[cfg(not(target_arch = "wasm32"))]
    started: std::time::Instant,
}

struct Instance {
    tmpl: usize,
    vars: Vec<Series>,
    var_init: Vec<bool>,
    states: Vec<Option<State>>,
    hists: Vec<Series>,
    children: Vec<Option<usize>>,
}

impl Clone for Instance {
    fn clone(&self) -> Self {
        Self {
            tmpl: self.tmpl,
            vars: self.vars.clone(),
            var_init: self.var_init.clone(),
            states: self.states.iter().map(|s| s.as_ref().map(|b| (**b).clone_box())).collect(),
            hists: self.hists.clone(),
            children: self.children.clone(),
        }
    }
}

/// Everything a run carries from one bar to the next, outputs aside: the instances and the
/// `Once` values. Cloned to snapshot a session after its last confirmed bar.
#[derive(Clone, Default)]
pub struct Machine {
    arena: Vec<Instance>,
    once: Vec<Option<Value>>,
    global: Option<usize>,
    heap: heap::Heap,
}

impl Machine {
    pub fn new(program: &Program) -> Self {
        Self { arena: Vec::new(), once: vec![None; program.once_slots], global: None, heap: heap::Heap::default() }
    }

    /// Execute bars `bars` of `input`, writing into `out` (sized for every bar), each in the
    /// mode `mode(bar)` gives. The operation budget and the clock are this call's own.
    pub fn steps(
        &mut self,
        program: &Program,
        input: &RunInput,
        out: &mut RunOutput,
        bars: std::ops::Range<usize>,
        last_history: Option<usize>,
        mode: impl Fn(usize) -> BarMode,
    ) -> Result<(), RuntimeError> {
        let max_history = program.meta.max_bars_back.unwrap_or(input.limits.max_history).min(input.limits.max_history);
        let ctx = Ctx {
            input,
            bar: bars.start,
            version: program.version,
            mode: BarMode::History,
            last_history,
            out: std::mem::take(out),
            heap: std::mem::take(&mut self.heap),
            max_history,
            draw_limits: program.draw_limits,
            ops: 0,
            loop_iters: 0,
            #[cfg(not(target_arch = "wasm32"))]
            started: std::time::Instant::now(),
        };
        let mut vm = Vm {
            program,
            arena: std::mem::take(&mut self.arena),
            stack: Vec::new(),
            ctx,
            flow: Flow::Normal,
            args: Vec::with_capacity(64),
            once: std::mem::take(&mut self.once),
        };
        let r = vm.run_bars(program, &mut self.global, bars, mode);
        self.arena = vm.arena;
        self.once = vm.once;
        self.heap = std::mem::take(&mut vm.ctx.heap);
        *out = vm.ctx.out;
        if r.is_ok() {
            out.drawings = self.heap.export();
        }
        r
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Flow {
    Normal,
    Break,
    Continue,
}

type RtResult<T> = Result<T, RuntimeError>;

struct Vm<'p, 'r> {
    program: &'p Program,
    arena: Vec<Instance>,
    stack: Vec<usize>,
    ctx: Ctx<'r>,
    flow: Flow,
    /// Argument values of native and output calls, reused across calls (no allocation).
    args: Vec<Value>,
    /// Values of `CExpr::Once` nodes.
    once: Vec<Option<Value>>,
}

impl<'p, 'r> Vm<'p, 'r> {
    fn fail(&self, code: RtCode, span: Span, args: Vec<String>) -> RuntimeError {
        RuntimeError { code, span, bar: self.ctx.bar, args }
    }

    fn instance(&mut self, tmpl: usize) -> RtResult<usize> {
        if self.arena.len() >= Self::MAX_INSTANCES {
            return Err(self.fail(RtCode::MemoryLimit, Span::default(), vec![Self::MAX_INSTANCES.to_string()]));
        }
        let t = &self.program.templates[tmpl];
        let max = self.ctx.max_history + 1;
        self.arena.push(Instance {
            tmpl,
            vars: t.vars.iter().map(|v| Series::new(v.cap.unwrap_or(max).min(max))).collect(),
            var_init: vec![false; t.vars.len()],
            states: (0..t.n_states).map(|_| None).collect(),
            hists: t.hists.iter().map(|c| Series::new(c.unwrap_or(max).min(max))).collect(),
            children: vec![None; t.n_children],
        });
        Ok(self.arena.len() - 1)
    }

    /// The child instance in slot `child` of the current frame, created on first use.
    fn child(&mut self, child: usize, tmpl: usize) -> RtResult<usize> {
        let cur = *self.stack.last().expect("frame");
        if let Some(id) = self.arena[cur].children[child] {
            return Ok(id);
        }
        let id = self.instance(tmpl)?;
        self.arena[cur].children[child] = Some(id);
        Ok(id)
    }

    /// A new execution of an instance: plain variables start `na`, `var` ones carry over.
    fn begin(&mut self, inst: usize) {
        let i = &mut self.arena[inst];
        for (s, init) in i.vars.iter_mut().zip(&i.var_init) {
            // Without history the slot already holds the carried value (or is rewritten by
            // its declaration before any read): nothing to shift.
            if s.is_scalar() && *init {
                continue;
            }
            let carry = if *init { s.current() } else { Value::Na };
            s.push(carry);
        }
    }

    fn frame(&self, up: usize) -> usize {
        self.stack[self.stack.len() - 1 - up]
    }

    fn tick(&mut self, span: Span) -> RtResult<()> {
        self.ctx.ops += 1;
        if self.ctx.ops > self.ctx.input.limits.max_ops {
            return Err(self.fail(RtCode::OpsBudget, span, vec![]));
        }
        #[cfg(not(target_arch = "wasm32"))]
        if self.ctx.ops & 0xFFFF == 0 {
            if let Some(ms) = self.ctx.input.limits.timeout_ms {
                if self.ctx.started.elapsed().as_millis() as u64 > ms {
                    return Err(self.fail(RtCode::Timeout, span, vec![]));
                }
            }
        }
        Ok(())
    }

    /// Run a block's statements in the current frame; its value is the last statement's.
    fn run_stmts(&mut self, body: &'p [CStmt]) -> RtResult<Value> {
        let mut last = Value::Na;
        for s in body {
            last = self.stmt(s)?;
            if self.flow != Flow::Normal {
                break;
            }
        }
        Ok(last)
    }

    /// Execute a nested block in its own instance.
    fn run_block(&mut self, b: BlockRef) -> RtResult<Value> {
        let inst = self.child(b.child, b.tmpl)?;
        self.begin(inst);
        self.stack.push(inst);
        let program = self.program;
        let r = self.run_stmts(&program.templates[b.tmpl].body);
        self.stack.pop();
        r
    }

    fn set_var(&mut self, var: VarRef, v: Value) {
        let f = self.frame(var.up);
        self.arena[f].vars[var.slot].set(v);
        self.arena[f].var_init[var.slot] = true;
    }

    fn stmt(&mut self, s: &'p CStmt) -> RtResult<Value> {
        self.tick(Span::default())?;
        match s {
            CStmt::Decl { slot, persistent, value } => {
                let f = self.frame(0);
                if *persistent && self.arena[f].var_init[*slot] {
                    return Ok(self.arena[f].vars[*slot].current());
                }
                let v = self.expr(value)?;
                self.set_var(VarRef { up: 0, slot: *slot }, v.clone());
                Ok(v)
            }
            CStmt::TupleDecl { slots, value, .. } => {
                let v = self.expr(value)?;
                let items: Vec<Value> = match &v {
                    Value::Tuple(items) => items.to_vec(),
                    _ => vec![Value::Na; slots.len()],
                };
                for (slot, item) in slots.iter().zip(items) {
                    if let Some(slot) = slot {
                        self.set_var(VarRef { up: 0, slot: *slot }, item);
                    }
                }
                Ok(v)
            }
            CStmt::Assign { var, op, value } => {
                let v = self.expr(value)?;
                let v = match op {
                    AssignOp::Reassign => v,
                    _ => {
                        let f = self.frame(var.up);
                        let cur = self.arena[f].vars[var.slot].current();
                        let bop = match op {
                            AssignOp::Add => BinOp::Add,
                            AssignOp::Sub => BinOp::Sub,
                            AssignOp::Mul => BinOp::Mul,
                            AssignOp::Div => BinOp::Div,
                            _ => BinOp::Mod,
                        };
                        self.check_len(binary(bop, &cur, &v, false))?
                    }
                };
                self.set_var(*var, v.clone());
                Ok(v)
            }
            CStmt::AssignField { obj, field, op, value, span } => {
                let o = self.expr(obj)?;
                let v = self.expr(value)?;
                let (id, i) = self.field_slot(&o, field, *span)?;
                let v = match op {
                    AssignOp::Reassign => v,
                    _ => {
                        let cur = self.ctx.heap.object(id).map(|o| o.fields[i].clone()).unwrap_or(Value::Na);
                        let bop = match op {
                            AssignOp::Add => BinOp::Add,
                            AssignOp::Sub => BinOp::Sub,
                            AssignOp::Mul => BinOp::Mul,
                            AssignOp::Div => BinOp::Div,
                            _ => BinOp::Mod,
                        };
                        self.check_len(binary(bop, &cur, &v, false))?
                    }
                };
                if let Some(o) = self.ctx.heap.object_mut(id) {
                    o.fields[i] = v.clone();
                }
                Ok(v)
            }
            CStmt::Expr(e) => self.expr(e),
            CStmt::Break => {
                self.flow = Flow::Break;
                Ok(Value::Na)
            }
            CStmt::Continue => {
                self.flow = Flow::Continue;
                Ok(Value::Na)
            }
        }
    }

    fn offset(&mut self, off: &'p CExpr, span: Span) -> RtResult<Option<usize>> {
        let k = self.expr(off)?.int();
        match k {
            Some(k) if k < 0 => Ok(None),
            Some(k) if k as usize > self.ctx.max_history => Err(self.fail(RtCode::HistoryTooDeep, span, vec![k.to_string()])),
            Some(k) => Ok(Some(k as usize)),
            None => Ok(None),
        }
    }

    fn expr(&mut self, e: &'p CExpr) -> RtResult<Value> {
        match e {
            CExpr::Lit(v) => Ok(v.clone()),
            CExpr::Var(var) => {
                let f = self.frame(var.up);
                Ok(self.arena[f].vars[var.slot].current())
            }
            CExpr::VarIndex { var, off, span } => {
                let Some(k) = self.offset(off, *span)? else { return Ok(Value::Na) };
                let f = self.frame(var.up);
                Ok(self.arena[f].vars[var.slot].get(k))
            }
            CExpr::Bar { var, span } => stdlib::bar_value(&self.ctx, *var, 0).map_err(|(c, a)| self.fail(c, *span, a)),
            CExpr::BarIndex { var, off, span } => {
                let Some(k) = self.offset(off, *span)? else { return Ok(Value::Na) };
                stdlib::bar_value(&self.ctx, *var, k).map_err(|(c, a)| self.fail(c, *span, a))
            }
            CExpr::Neg(x) => Ok(match self.expr(x)? {
                Value::Int(i) => Value::Int(i.wrapping_neg()),
                Value::Float(f) => Value::float(-f),
                _ => Value::Na,
            }),
            CExpr::Not(x) => Ok(Value::Bool(!self.expr(x)?.truthy())),
            CExpr::Bin { op, l, r, int_div } => {
                let (a, b) = (self.expr(l)?, self.expr(r)?);
                self.check_len(binary(*op, &a, &b, *int_div))
            }
            CExpr::And { l, r, lazy } => {
                let a = self.expr(l)?.truthy();
                if *lazy && !a {
                    return Ok(Value::Bool(false));
                }
                let b = self.expr(r)?.truthy();
                Ok(Value::Bool(a && b))
            }
            CExpr::Or { l, r, lazy } => {
                let a = self.expr(l)?.truthy();
                if *lazy && a {
                    return Ok(Value::Bool(true));
                }
                let b = self.expr(r)?.truthy();
                Ok(Value::Bool(a || b))
            }
            CExpr::Ternary { c, a, b } => {
                if self.expr(c)?.truthy() {
                    self.expr(a)
                } else {
                    self.expr(b)
                }
            }
            _ => self.expr_cold(e),
        }
    }

    /// Arms off the hot path, kept out of `expr` so its stack frame stays small.
    #[inline(never)]
    fn expr_cold(&mut self, e: &'p CExpr) -> RtResult<Value> {
        match e {
            CExpr::Index { base, off, hist, span } => {
                let v = self.expr(base)?;
                let f = self.frame(0);
                self.arena[f].hists[*hist].push(v);
                let Some(k) = self.offset(off, *span)? else { return Ok(Value::Na) };
                Ok(self.arena[f].hists[*hist].get(k))
            }
            CExpr::Tuple(items) => {
                let vals = items.iter().map(|i| self.expr(i)).collect::<RtResult<Vec<_>>>()?;
                Ok(Value::Tuple(Rc::from(vals)))
            }
            CExpr::Native { f, args, state, span } => {
                let base = self.args.len();
                for a in args {
                    let v = self.expr(a)?;
                    self.args.push(v);
                }
                let inst = self.frame(0);
                let st = match state {
                    Some(slot) => {
                        let tmpl = self.arena[inst].tmpl;
                        let cell = &mut self.arena[inst].states[*slot];
                        if cell.is_none() {
                            *cell = Some((self.program.templates[tmpl].state_init[*slot].expect("state init"))());
                        }
                        cell.as_mut()
                    }
                    None => None,
                };
                let r = (f.f)(&mut self.ctx, &self.args[base..], st);
                self.args.truncate(base);
                r.map_err(|(c, a)| self.fail(c, *span, a))
            }
            CExpr::Once { slot, e } => {
                if let Some(v) = &self.once[*slot] {
                    return Ok(v.clone());
                }
                let v = self.expr(e)?;
                self.once[*slot] = Some(v.clone());
                Ok(v)
            }
            CExpr::Output { kind, index, args, span } => {
                self.tick(*span)?;
                let base = self.args.len();
                for a in args {
                    let v = match a {
                        Some(a) => self.expr(a)?,
                        None => Value::Na,
                    };
                    self.args.push(v);
                }
                stdlib::plot::record(&mut self.ctx, *kind, *index, &self.args[base..]);
                self.args.truncate(base);
                Ok(match kind {
                    stdlib::OutKind::HLine => Value::HLineRef(*index),
                    _ => Value::PlotRef(*index),
                })
            }
            CExpr::Call { func, child, args, span } => {
                self.tick(*span)?;
                let vals = args.iter().map(|a| self.expr(a)).collect::<RtResult<Vec<_>>>()?;
                let program = self.program;
                let def = &program.fns[*func];
                let inst = self.child(*child, def.tmpl)?;
                let global = self.stack[0];
                let saved = std::mem::replace(&mut self.stack, vec![global, inst]);
                self.begin(inst);
                for (slot, v) in def.params.iter().zip(vals) {
                    self.set_var(VarRef { up: 0, slot: *slot }, v);
                }
                let program = self.program;
                let r = self.run_stmts(&program.templates[def.tmpl].body);
                self.stack = saved;
                r
            }
            CExpr::If { branches } => {
                for (cond, block) in branches {
                    let take = match cond {
                        Some(c) => self.expr(c)?.truthy(),
                        None => true,
                    };
                    if take {
                        return self.run_block(*block);
                    }
                }
                Ok(Value::Na)
            }
            CExpr::Switch { key, arms } => {
                let key = match key {
                    Some(k) => Some(self.expr(k)?),
                    None => None,
                };
                for (pat, block) in arms {
                    let take = match (pat, &key) {
                        (None, _) => true,
                        (Some(p), Some(k)) => {
                            let pv = self.expr(p)?;
                            binary(BinOp::Eq, k, &pv, false).truthy()
                        }
                        (Some(p), None) => self.expr(p)?.truthy(),
                    };
                    if take {
                        return self.run_block(*block);
                    }
                }
                Ok(Value::Na)
            }
            CExpr::For { var, from, to, by, body, span } => {
                let (from, to) = (self.expr(from)?, self.expr(to)?);
                let step = match by {
                    Some(b) => self.expr(b)?,
                    None => Value::Int(1),
                };
                let ints = matches!((&from, &to, &step), (Value::Int(_), Value::Int(_), Value::Int(_)));
                let (Some(a), Some(z), Some(st)) = (from.num(), to.num(), step.num()) else { return Ok(Value::Na) };
                let st = st.abs();
                if st == 0.0 {
                    return Ok(Value::Na);
                }
                let dir = if z >= a { 1.0 } else { -1.0 };
                let inst = self.child(body.child, body.tmpl)?;
                let mut i = a;
                let mut last = Value::Na;
                while (dir > 0.0 && i <= z) || (dir < 0.0 && i >= z) {
                    self.loop_tick(*span)?;
                    self.begin(inst);
                    self.stack.push(inst);
                    let v = if ints { Value::Int(i as i64) } else { Value::float(i) };
                    self.set_var(VarRef { up: 0, slot: *var }, v);
                    let program = self.program;
                    let r = self.run_stmts(&program.templates[body.tmpl].body);
                    self.stack.pop();
                    last = r?;
                    let flow = std::mem::replace(&mut self.flow, Flow::Normal);
                    if flow == Flow::Break {
                        break;
                    }
                    i += dir * st;
                }
                Ok(last)
            }
            CExpr::While { cond, body, span } => {
                let mut last = Value::Na;
                while self.expr(cond)?.truthy() {
                    self.loop_tick(*span)?;
                    last = self.run_block(*body)?;
                    let flow = std::mem::replace(&mut self.flow, Flow::Normal);
                    if flow == Flow::Break {
                        break;
                    }
                }
                Ok(last)
            }
            CExpr::ForIn { idx, item, iter, body, span } => {
                let arr = self.expr(iter)?;
                let Value::Ref(id) = arr else { return Ok(Value::Na) };
                let inst = self.child(body.child, body.tmpl)?;
                let mut last = Value::Na;
                let mut i = 0;
                // The size is read at each step: the body may push or remove.
                while let Some(v) = self.ctx.heap.array(id).and_then(|a| a.items.get(i).cloned()) {
                    self.loop_tick(*span)?;
                    self.begin(inst);
                    self.stack.push(inst);
                    if let Some(slot) = idx {
                        self.set_var(VarRef { up: 0, slot: *slot }, Value::Int(i as i64));
                    }
                    self.set_var(VarRef { up: 0, slot: *item }, v);
                    let program = self.program;
                    let r = self.run_stmts(&program.templates[body.tmpl].body);
                    self.stack.pop();
                    last = r?;
                    let flow = std::mem::replace(&mut self.flow, Flow::Normal);
                    if flow == Flow::Break {
                        break;
                    }
                    i += 1;
                }
                Ok(last)
            }
            CExpr::New { ty, args } => {
                let fields = args.iter().map(|a| self.expr(a)).collect::<RtResult<Vec<_>>>()?;
                let obj = heap::Obj::Object(heap::Object { ty: *ty, fields });
                self.ctx.heap.alloc(obj).ok_or_else(|| self.fail(RtCode::MemoryLimit, Span::default(), vec![heap::MAX_OBJECTS.to_string()]))
            }
            CExpr::CopyObj(e) => {
                let Value::Ref(id) = self.expr(e)? else { return Ok(Value::Na) };
                let Some(o) = self.ctx.heap.object(id).cloned() else { return Ok(Value::Na) };
                self.ctx.heap.alloc(heap::Obj::Object(o)).ok_or_else(|| self.fail(RtCode::MemoryLimit, Span::default(), vec![heap::MAX_OBJECTS.to_string()]))
            }
            CExpr::Field { obj, name, span } => {
                let o = self.expr(obj)?;
                let (id, i) = self.field_slot(&o, name, *span)?;
                Ok(self.ctx.heap.object(id).map(|o| o.fields[i].clone()).unwrap_or(Value::Na))
            }
            _ => unreachable!("hot arm"),
        }
    }

    /// The object `o` refers to and the index of its field `name`; an `na` object (or a value
    /// that is not one) stops the run naming the field.
    fn field_slot(&self, o: &Value, name: &str, span: Span) -> RtResult<(u32, usize)> {
        let na = || self.fail(RtCode::ObjectNa, span, vec![name.to_string()]);
        let Value::Ref(id) = o else { return Err(na()) };
        let obj = self.ctx.heap.object(*id).ok_or_else(na)?;
        let i = self.program.types[obj.ty].fields.iter().position(|f| &**f == name).ok_or_else(na)?;
        Ok((*id, i))
    }

    /// Refuse strings past `Limits::max_string_len` (repeated `s := s + s` exhausts memory).
    fn check_len(&self, v: Value) -> RtResult<Value> {
        match &v {
            Value::Str(s) if s.len() > self.ctx.input.limits.max_string_len => Err(self.fail(RtCode::StringTooLong, Span::default(), vec![s.len().to_string()])),
            _ => Ok(v),
        }
    }

    /// The bar loop of `Machine::steps`; creates the global instance on first use.
    fn run_bars(&mut self, program: &'p Program, global: &mut Option<usize>, bars: std::ops::Range<usize>, mode: impl Fn(usize) -> BarMode) -> RtResult<()> {
        let g = match *global {
            Some(g) => g,
            None => {
                let g = self.instance(program.global)?;
                *global = Some(g);
                g
            }
        };
        self.stack.push(g);
        let body = &program.templates[program.global].body;
        for bar in bars {
            self.ctx.bar = bar;
            self.ctx.mode = mode(bar);
            self.ctx.loop_iters = 0;
            self.begin(g);
            self.run_stmts(body)?;
            if self.ctx.heap.wants_collect() {
                self.collect();
            }
        }
        Ok(())
    }

    /// Drop the objects no variable, history slot or cached value refers to any more.
    fn collect(&mut self) {
        let vars = self.arena.iter().flat_map(|i| i.vars.iter().chain(&i.hists)).flat_map(|s| s.values());
        let once = self.once.iter().flatten();
        self.ctx.heap.collect(vars.chain(once));
    }

    /// Cap on call-site and block instances in one run (function fan-out exhausts memory).
    const MAX_INSTANCES: usize = 100_000;

    fn loop_tick(&mut self, span: Span) -> RtResult<()> {
        self.ctx.loop_iters += 1;
        if self.ctx.loop_iters > self.ctx.input.limits.max_loop_iters_per_bar {
            return Err(self.fail(RtCode::LoopLimit, span, vec![]));
        }
        self.tick(span)
    }

}

/// Arithmetic, comparison and concatenation with Pine's `na` rules: arithmetic with `na` is
/// `na`, a comparison with `na` is false. `int_div`: v5 division of two constant ints.
pub fn binary(op: BinOp, a: &Value, b: &Value, int_div: bool) -> Value {
    use Value::*;
    // Fast path: two floats (the common case in indicator math).
    if let (Float(x), Float(y)) = (a, b) {
        let (x, y) = (*x, *y);
        return match op {
            BinOp::Add => Value::float(x + y),
            BinOp::Sub => Value::float(x - y),
            BinOp::Mul => Value::float(x * y),
            BinOp::Div if y == 0.0 => Na,
            BinOp::Div => Value::float(x / y),
            BinOp::Mod if y == 0.0 => Na,
            BinOp::Mod => Value::float(x % y),
            BinOp::Eq => Bool(x == y),
            BinOp::Ne => Bool(x != y),
            BinOp::Lt => Bool(x < y),
            BinOp::Gt => Bool(x > y),
            BinOp::Le => Bool(x <= y),
            BinOp::Ge => Bool(x >= y),
            BinOp::And | BinOp::Or => Na,
        };
    }
    let cmp = matches!(op, BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge);
    if a.is_na() || b.is_na() {
        return if cmp { Bool(false) } else { Na };
    }
    if cmp {
        let ord = match (a, b) {
            (Str(x), Str(y)) => x.partial_cmp(y),
            (Bool(x), Bool(y)) => x.partial_cmp(y),
            (Color(x), Color(y)) => return Bool(match op {
                BinOp::Eq => x == y,
                BinOp::Ne => x != y,
                _ => false,
            }),
            _ => match (a.num(), b.num()) {
                (Some(x), Some(y)) => x.partial_cmp(&y),
                _ => None,
            },
        };
        let Some(ord) = ord else { return Bool(false) };
        use std::cmp::Ordering::*;
        return Bool(match op {
            BinOp::Eq => ord == Equal,
            BinOp::Ne => ord != Equal,
            BinOp::Lt => ord == Less,
            BinOp::Gt => ord == Greater,
            BinOp::Le => ord != Greater,
            _ => ord != Less,
        });
    }
    match (a, b) {
        (Str(x), Str(y)) if op == BinOp::Add => Value::str(&format!("{x}{y}")),
        (Int(x), Int(y)) => match op {
            BinOp::Add => Int(x.wrapping_add(*y)),
            BinOp::Sub => Int(x.wrapping_sub(*y)),
            BinOp::Mul => Int(x.wrapping_mul(*y)),
            BinOp::Div if *y == 0 => Na,
            BinOp::Div if int_div => Int(x.wrapping_div(*y)),
            BinOp::Div => Value::float(*x as f64 / *y as f64),
            BinOp::Mod if *y == 0 => Na,
            BinOp::Mod => Int(x.wrapping_rem(*y)),
            _ => Na,
        },
        _ => {
            let (Some(x), Some(y)) = (a.num(), b.num()) else { return Na };
            match op {
                BinOp::Add => Value::float(x + y),
                BinOp::Sub => Value::float(x - y),
                BinOp::Mul => Value::float(x * y),
                BinOp::Div if y == 0.0 => Na,
                BinOp::Div => Value::float(x / y),
                BinOp::Mod if y == 0.0 => Na,
                BinOp::Mod => Value::float(x % y),
                _ => Na,
            }
        }
    }
}

/// Every column as long as `close`; `volume` may also be empty (unknown).
pub(crate) fn check_bars(b: &crate::input::Bars) -> Result<(), RuntimeError> {
    let n = b.close.len();
    let cols: [(&str, usize); 6] =
        [("time", b.time.len()), ("time_close", b.time_close.len()), ("open", b.open.len()), ("high", b.high.len()), ("low", b.low.len()), ("volume", b.volume.len())];
    match cols.iter().find(|(name, len)| *len != n && !(*name == "volume" && *len == 0)) {
        Some((name, _)) => Err(RuntimeError { code: RtCode::BadBars, span: Span::default(), bar: 0, args: vec![name.to_string()] }),
        None => Ok(()),
    }
}

pub fn run(program: &Program, input: &RunInput) -> Result<RunOutput, RuntimeError> {
    let n = input.bars.len();
    check_bars(&input.bars)?;
    let mut out = stdlib::plot::empty(&program.out_counts, n);
    Machine::new(program).steps(program, input, &mut out, 0..n, n.checked_sub(1), |_| BarMode::History)?;
    Ok(out)
}
