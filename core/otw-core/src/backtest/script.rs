//! Script indicators (Pine v5/v6 compatible) as backtest operands. A script is embedded in the
//! settings like a DAG indicator, checked without bars by [`check`], then run once per asset
//! by [`prepare`] over the asset's stored history plus its bars. `resolve` reads the cached
//! plot columns, so optimizer trials sharing an asset and input values share one run.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use otw_script::host::{close_times, input_values};
use otw_script::input::{Bars as ScriptBars, Limits, RunInput, SymInfo, Timeframe};
use otw_script::output::ScriptMeta;
use serde::{Deserialize, Serialize};

use super::{Bars, Defs, Operand, Settings, SignalGroup};
use crate::script::{COMPILE_STACK, MAX_SOURCE_BYTES};

/// A raw input value as the form holds it; converted by `otw_script::host::input_values`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum RawInput {
    Bool(bool),
    Num(f64),
    Str(String),
}

/// A script embedded in the settings: its source as of the run, and the input values every
/// operand on it reads.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ScriptDef {
    pub name: String,
    pub source: String,
    #[serde(default)]
    pub inputs: BTreeMap<String, RawInput>,
}

pub type ScriptDefs = BTreeMap<String, ScriptDef>;

/// The stored bars before an asset's first traded bar, oldest first.
#[derive(Debug, Clone, Default)]
pub struct WarmBars {
    pub time_ms: Vec<i64>,
    pub open: Vec<f64>,
    pub high: Vec<f64>,
    pub low: Vec<f64>,
    pub close: Vec<f64>,
    pub volume: Vec<f64>,
}

/// Plot columns of one run, by plot index, sliced to the asset's bars, offsets applied, `NaN`
/// for na. Only the plots a condition reads are kept (`None` for the others).
type PlotCols = Vec<Option<Vec<f64>>>;
type Run = Arc<Result<PlotCols, String>>;
type RunKey = (u64, String, String, usize);
/// A run being made or made: trials asking for it meanwhile wait on the same slot.
type Slot = Arc<OnceLock<Run>>;

#[derive(Default)]
struct RunCache {
    slots: HashMap<RunKey, Slot>,
    /// Oldest first, for eviction.
    order: VecDeque<RunKey>,
    /// Bytes of each finished run kept.
    sizes: HashMap<RunKey, usize>,
    bytes: usize,
}

impl RunCache {
    fn drop_key(&mut self, key: &RunKey) {
        self.slots.remove(key);
        self.order.retain(|k| k != key);
        self.bytes -= self.sizes.remove(key).unwrap_or(0);
    }
}

/// Bytes the run caches of one load may hold in all, shared out between its assets. An
/// optimizer varying script inputs makes one run per value set; a trial whose run was
/// dropped runs it again.
pub const CACHE_BYTES: usize = 512 << 20;
/// Runs kept per asset at most, whatever their size (failed runs weigh nothing).
const RUNS_KEPT: usize = 64;

/// One asset's script history: warmup bars, instrument facts and the run cache.
pub struct ScriptHistory {
    warm: WarmBars,
    syminfo: SymInfo,
    timeframe: String,
    cache: Mutex<RunCache>,
    budget: usize,
    runs: AtomicUsize,
}

impl ScriptHistory {
    pub fn new(warm: WarmBars, syminfo: SymInfo, timeframe: String) -> Self {
        Self { warm, syminfo, timeframe, cache: Mutex::default(), budget: CACHE_BYTES, runs: AtomicUsize::new(0) }
    }

    /// The same history with a run cache of at most `bytes`.
    pub fn with_budget(mut self, bytes: usize) -> Self {
        self.budget = bytes;
        self
    }

    /// Script runs made so far (cache misses).
    #[cfg(test)]
    pub fn runs(&self) -> usize {
        self.runs.load(Ordering::Relaxed)
    }

    /// Bytes the run cache holds.
    #[cfg(test)]
    pub fn cached_bytes(&self) -> usize {
        self.cache.lock().unwrap_or_else(|e| e.into_inner()).bytes
    }

    /// The plot columns of `def` over `b`, holding at least the plots `wanted`, run on a miss.
    fn columns(&self, def: &ScriptDef, b: &Bars, wanted: &BTreeSet<usize>) -> Run {
        let inputs = serde_json::to_string(&def.inputs).unwrap_or_default();
        let key = (source_hash(&def.source), inputs, b.ts.first().cloned().unwrap_or_default(), b.ts.len());
        let mut keep = wanted.clone();
        let slot = {
            let mut c = self.cache.lock().unwrap_or_else(|e| e.into_inner());
            let lacking = c.slots.get(&key).and_then(|s| s.get()).and_then(|r| r.as_ref().as_ref().ok()).map(|cols| {
                keep.extend((0..cols.len()).filter(|&i| cols[i].is_some()));
                wanted.iter().any(|&i| cols.get(i).is_some_and(Option::is_none))
            });
            // A run kept without a plot now read is made again with every plot either reads.
            if lacking == Some(true) {
                c.drop_key(&key);
            }
            match c.slots.get(&key) {
                Some(s) => s.clone(),
                None => {
                    let s = Slot::default();
                    c.slots.insert(key.clone(), s.clone());
                    c.order.push_back(key.clone());
                    s
                }
            }
        };
        let run = slot
            .get_or_init(|| {
                self.runs.fetch_add(1, Ordering::Relaxed);
                Arc::new(self.run(def, b, &keep))
            })
            .clone();
        // Waited on a run started for other plots: make it again with these too.
        if run.as_ref().as_ref().is_ok_and(|cols| wanted.iter().any(|&i| cols.get(i).is_some_and(Option::is_none))) {
            return self.columns(def, b, wanted);
        }
        let mut c = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        if c.slots.get(&key).is_some_and(|s| Arc::ptr_eq(s, &slot)) && !c.sizes.contains_key(&key) {
            let size = run.as_ref().as_ref().map_or(0, |cols| cols.iter().flatten().map(|v| v.len() * 8).sum());
            if size > self.budget {
                c.drop_key(&key);
            } else {
                c.sizes.insert(key.clone(), size);
                c.bytes += size;
                while c.bytes > self.budget || c.order.len() > RUNS_KEPT {
                    let Some(old) = c.order.front().cloned() else { break };
                    c.drop_key(&old);
                }
            }
        }
        run
    }

    fn run(&self, def: &ScriptDef, b: &Bars, keep: &BTreeSet<usize>) -> Result<PlotCols, String> {
        let Some(timeframe) = Timeframe::parse(&self.timeframe) else {
            return Err(format!("script \"{}\": unknown timeframe {}", def.name, self.timeframe));
        };
        let w = &self.warm;
        let mut time = w.time_ms.clone();
        for t in b.ts {
            time.push(epoch_ms(t).ok_or_else(|| format!("bar time {t} is not a timestamp"))?);
        }
        let cat = |a: &[f64], c: &[f64]| [a, c].concat();
        let bars = ScriptBars {
            time_close: close_times(&time, timeframe),
            time,
            open: cat(&w.open, b.open),
            high: cat(&w.high, b.high),
            low: cat(&w.low, b.low),
            close: cat(&w.close, b.close),
            volume: cat(&w.volume, b.volume),
        };
        let syminfo = self.syminfo.clone();
        let source = def.source.clone();
        let raw: serde_json::Map<String, serde_json::Value> =
            def.inputs.iter().map(|(k, v)| (k.clone(), serde_json::to_value(v).unwrap_or_default())).collect();
        let out = on_big_stack(move || {
            let program = otw_script::compile(&source).map_err(|_| None)?;
            let (inputs, _) = input_values(&program.meta().inputs, &raw);
            program.run(&RunInput { bars, syminfo, timeframe, inputs, limits: Limits::default() }).map_err(Some)
        });
        let warm = w.close.len();
        match out {
            Ok(Ok(o)) => Ok(o
                .plots
                .iter()
                .enumerate()
                .map(|(n, p)| {
                    let k = p.offset.max(0) as usize;
                    keep.contains(&n).then(|| {
                        (warm..warm + b.close.len()).map(|i| i.checked_sub(k).and_then(|j| p.values[j]).unwrap_or(f64::NAN)).collect()
                    })
                })
                .collect()),
            Ok(Err(Some(e))) => {
                let at = if e.bar < warm {
                    time::OffsetDateTime::from_unix_timestamp_nanos(w.time_ms[e.bar] as i128 * 1_000_000)
                        .ok()
                        .and_then(|d| d.format(&time::format_description::well_known::Rfc3339).ok())
                        .unwrap_or_default()
                } else {
                    b.ts.get(e.bar - warm).cloned().unwrap_or_default()
                };
                let args = if e.args.is_empty() { String::new() } else { format!(" {}", e.args.join(" ")) };
                Err(format!("script \"{}\" stopped on {} at {at}, line {}: {:?}{args}", def.name, b.ticker, e.span.line, e.code))
            }
            Ok(Err(None)) => Err(format!("script \"{}\" does not compile", def.name)),
            Err(()) => Err(format!("script \"{}\": the script engine failed", def.name)),
        }
    }
}

fn epoch_ms(t: &str) -> Option<i64> {
    use time::{format_description::well_known::Rfc3339, OffsetDateTime};
    OffsetDateTime::parse(t, &Rfc3339).ok().map(|d| (d.unix_timestamp_nanos() / 1_000_000) as i64)
}

fn source_hash(source: &str) -> u64 {
    let mut h = DefaultHasher::new();
    source.hash(&mut h);
    h.finish()
}

/// `f` on a thread with the compiler's stack; `Err` when the thread could not run or panicked.
fn on_big_stack<T: Send>(f: impl FnOnce() -> T + Send) -> Result<T, ()> {
    std::thread::scope(|sc| {
        std::thread::Builder::new()
            .name("script-run".into())
            .stack_size(COMPILE_STACK)
            .spawn_scoped(sc, f)
            .map_err(|_| ())?
            .join()
            .map_err(|_| ())
    })
}

/// Compiled metadata by source hash, so optimizer trials never recompile.
const METAS_KEPT: usize = 64;
type MetaCache = (HashMap<u64, Result<Arc<ScriptMeta>, String>>, VecDeque<u64>);
static METAS: OnceLock<Mutex<MetaCache>> = OnceLock::new();

/// The script's metadata, or the first compile error (line and code).
fn meta(def: &ScriptDef) -> Result<Arc<ScriptMeta>, String> {
    let h = source_hash(&def.source);
    let cache = METAS.get_or_init(Mutex::default);
    if let Some(hit) = cache.lock().unwrap_or_else(|e| e.into_inner()).0.get(&h) {
        return hit.clone();
    }
    let source = def.source.clone();
    let compiled = on_big_stack(move || otw_script::compile(&source).map(|p| p.meta().clone()));
    let r = match compiled {
        Ok(Ok(m)) => Ok(Arc::new(m)),
        Ok(Err(diags)) => Err(match diags.iter().find(|d| d.is_error()) {
            Some(d) => {
                let args = if d.args.is_empty() { String::new() } else { format!(" {}", d.args.join(" ")) };
                format!("script \"{}\" does not compile: line {}: {:?}{args}", def.name, d.span.line, d.code)
            }
            None => format!("script \"{}\" does not compile", def.name),
        }),
        // A panic is not the script's fault: report it, never cache it.
        Err(()) => return Err(format!("script \"{}\": the script engine failed", def.name)),
    };
    let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
    let (map, order) = &mut *guard;
    if map.insert(h, r.clone()).is_none() {
        order.push_back(h);
        while order.len() > METAS_KEPT {
            if let Some(old) = order.pop_front() {
                map.remove(&old);
            }
        }
    }
    r
}

/// Index of the plot an operand reads: its title, or the first plot when none is given.
fn plot_index(def: &ScriptDef, meta: &ScriptMeta, plot: Option<&str>) -> Result<usize, String> {
    let Some(title) = plot else {
        return if meta.plots.is_empty() { Err(format!("script \"{}\" has no plot", def.name)) } else { Ok(0) };
    };
    let mut hits = meta.plots.iter().enumerate().filter(|(_, p)| p.title == title);
    match (hits.next(), hits.next()) {
        (Some((i, _)), None) => Ok(i),
        (Some(_), Some(_)) => Err(format!("script \"{}\" has several plots titled \"{title}\": give each its own title", def.name)),
        (None, _) => Err(format!("script \"{}\" has no plot \"{title}\"", def.name)),
    }
}

/// Every signal group of the settings, with whether it reads a synthetic DCA basket.
fn groups(s: &Settings) -> Vec<(SignalGroup, bool)> {
    let mut out = Vec::new();
    for side in [s.long.as_ref(), s.short.as_ref()].into_iter().flatten() {
        out.extend(side.entry_group().map(|g| (g, false)));
        out.extend(side.exit.clone().map(|g| (g, false)));
    }
    if let Some(cfg) = &s.dca {
        out.extend(cfg.buys.iter().map(|r| (r.condition.clone(), !r.per_asset)));
        out.extend(cfg.sells.iter().map(|r| (r.condition.clone(), !r.per_asset)));
    }
    out
}

fn operands(g: &SignalGroup) -> impl Iterator<Item = &Operand> {
    g.conditions.iter().flat_map(|c| std::iter::once(&c.left).chain(c.right.as_ref()))
}

/// Why the settings' scripts cannot run, without bars: unknown ids, sources that do not
/// compile, plots that are missing, ambiguous or read the future, input values refused.
pub fn check(s: &Settings) -> Option<String> {
    for def in s.scripts.values() {
        if let Some(e) = check_def(def) {
            return Some(e);
        }
    }
    for (g, basket) in groups(s) {
        for op in operands(&g) {
            let Operand::CustomIndicator { id, plot } = op else { continue };
            if s.indicators.contains_key(id) {
                continue;
            }
            let Some(def) = s.scripts.get(id) else {
                return Some(format!("custom indicator '{id}' is not embedded in the settings"));
            };
            if basket {
                return Some(format!("a DCA rule over the basket cannot read script \"{}\" (a basket has no instrument history): set the rule to per asset", def.name));
            }
            let m = match meta(def) {
                Ok(m) => m,
                Err(e) => return Some(e),
            };
            let i = match plot_index(def, &m, plot.as_deref()) {
                Ok(i) => i,
                Err(e) => return Some(e),
            };
            let p = &m.plots[i];
            if !p.causal {
                let why = p.taint.as_ref().map(|t| format!("{}, line {}", t.feature, t.line)).unwrap_or_else(|| "unknown".into());
                return Some(format!("plot \"{}\" of script \"{}\" reads data a backtest cannot know yet ({why})", p.title, def.name));
            }
        }
    }
    None
}

/// Why one script cannot run at all: too large, does not compile, or an input value refused.
fn check_def(def: &ScriptDef) -> Option<String> {
    if def.source.len() > MAX_SOURCE_BYTES {
        return Some(format!("script \"{}\" is over {MAX_SOURCE_BYTES} bytes", def.name));
    }
    let m = match meta(def) {
        Ok(m) => m,
        Err(e) => return Some(e),
    };
    if let Some(key) = def.inputs.keys().find(|k| !m.inputs.iter().any(|i| &i.key == *k)) {
        return Some(format!("script \"{}\" has no input \"{key}\"", def.name));
    }
    let raw = def.inputs.iter().map(|(k, v)| (k.clone(), serde_json::to_value(v).unwrap_or_default())).collect();
    if let Some(e) = input_values(&m.inputs, &raw).1.first() {
        let why = match e.reason {
            "range" => "is out of range",
            "option" => "is not one of its options",
            _ => "has the wrong type",
        };
        return Some(format!("script \"{}\": input \"{}\" {why}", def.name, e.key));
    }
    None
}

/// Why one plot of a script cannot drive a chart alert: the script cannot run, or the plot is
/// missing, ambiguous, or reads data the bar it is judged on cannot know yet (it would
/// repaint, so the alert would report a crossing that later disappears).
pub fn check_plot(def: &ScriptDef, plot: Option<&str>) -> Option<String> {
    if let Some(e) = check_def(def) {
        return Some(e);
    }
    let m = match meta(def) {
        Ok(m) => m,
        Err(e) => return Some(e),
    };
    let i = match plot_index(def, &m, plot) {
        Ok(i) => i,
        Err(e) => return Some(e),
    };
    let p = &m.plots[i];
    if !p.causal {
        let why = p.taint.as_ref().map(|t| format!("{}, line {}", t.feature, t.line)).unwrap_or_else(|| "unknown".into());
        return Some(format!("plot \"{}\" of script \"{}\" reads data an alert cannot know yet ({why})", p.title, def.name));
    }
    None
}

/// Whether the settings embed scripts, so the loader fetches their history.
pub fn used(s: &Settings) -> bool {
    !s.scripts.is_empty()
}

/// [`used`] on raw settings JSON (sweeps and optimizer jobs carry raw JSON).
pub fn used_json(v: &serde_json::Value) -> bool {
    v.get("scripts").and_then(|x| x.as_object()).is_some_and(|m| !m.is_empty())
}

/// The scripts an operand reads, each with the plots read (`check` has refused a plot that
/// cannot be had).
fn used_plots(s: &Settings) -> BTreeMap<&String, BTreeSet<usize>> {
    let mut out: BTreeMap<&String, BTreeSet<usize>> = BTreeMap::new();
    for (g, _) in groups(s) {
        for op in operands(&g) {
            let Operand::CustomIndicator { id, plot } = op else { continue };
            let Some((k, def)) = s.scripts.get_key_value(id) else { continue };
            if s.indicators.contains_key(id) {
                continue;
            }
            let plots = out.entry(k).or_default();
            if let Some(i) = meta(def).ok().and_then(|m| plot_index(def, &m, plot.as_deref()).ok()) {
                plots.insert(i);
            }
        }
    }
    out
}

/// Run every script an operand reads on every asset, once, and refuse the run on an asset
/// without history or on a runtime error. Called after `Settings::validate`.
pub fn prepare(s: &Settings, assets: &[&Bars]) -> Result<(), String> {
    let used = used_plots(s);
    if used.is_empty() {
        return Ok(());
    }
    for b in assets {
        let Some(h) = b.script else {
            return Err(format!("script history was not loaded for {}", b.ticker));
        };
        for (id, plots) in &used {
            if let Err(e) = &*h.columns(&s.scripts[*id], b, plots) {
                return Err(e.clone());
            }
        }
    }
    Ok(())
}

/// The column an operand on script `id` reads over `b`; all `None` when it cannot be had
/// (`prepare` has refused such a run already).
pub(super) fn resolve(id: &str, plot: Option<&str>, b: &Bars, defs: &Defs) -> Vec<Option<f64>> {
    let none = || vec![None; b.close.len()];
    let (Some(def), Some(h)) = (defs.scripts.get(id), b.script) else { return none() };
    let Ok(m) = meta(def) else { return none() };
    let Ok(i) = plot_index(def, &m, plot) else { return none() };
    match &*h.columns(def, b, &BTreeSet::from([i])) {
        Ok(cols) => match cols.get(i) {
            Some(Some(col)) => col.iter().map(|v| (!v.is_nan()).then_some(*v)).collect(),
            _ => none(),
        },
        Err(_) => none(),
    }
}
