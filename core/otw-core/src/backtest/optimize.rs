//! Grid optimizer: one loaded bar set, many simulations.
//!
//! This is not the agent's `/api/backtest/sweep` (64 trials, one blocking response). A grid the
//! user builds in the UI is five or six figures of simulations and minutes of CPU, so it runs as
//! a **job**: started, polled while it fills, stopped at will, and readable from whatever it
//! finished. Nothing is persisted: a sweep is an exploration, and the run history stays the
//! place where a result is kept.
//!
//! Three decisions carry the whole design:
//!
//! - **Combinations are never materialised.** Trial `i` decodes from `i` in mixed radix over the
//!   axes, so a 130k-trial grid costs 130k patches and 130k simulations, and no 130k × axes
//!   array of tuples.
//! - **A row keeps the digits, not the values.** 8 × u16 + 11 × f32 per trial is what makes
//!   keeping *every* trial affordable, and keeping every trial is what makes the ranking
//!   honest, since the number of variants tried is exactly what decides whether the best one
//!   means anything.
//! - **The bars are loaded once** and shared by every worker thread, so the window can't drift
//!   between trials and the DB is read once per job.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

use super::{Bars, OwnedBars, Settings};

/// Axes per grid. Past this a sweep is a fishing expedition, and the multiple-testing haircut
/// eats whatever it finds.
pub const MAX_AXES: usize = 8;
/// Values per axis.
pub const MAX_AXIS_VALUES: usize = 2_000;
/// Trials per job. Rows are ~40 bytes, so the ceiling is about 10 MB of results.
pub const MAX_TRIALS: u64 = 250_000;
/// Ceiling on the simulated work, `trials × bars`. A grid that clears the trial cap can still
/// ask for a century of CPU; this is the line where the answer is "narrow the window".
pub const MAX_POINTS: u64 = 3_000_000_000;
/// Worker threads a job may use, whatever the host has.
const MAX_WORKERS: usize = 12;
/// Jobs kept in memory. The oldest finished one is dropped past this.
const MAX_JOBS: usize = 6;
/// Rows a worker buffers before it takes the shared lock.
const FLUSH_EVERY: usize = 64;

/// One swept parameter: where it lives in the settings JSON, and the values to try.
///
/// The axis is deliberately dumb: a path and a list of JSON values. Ranges, steps and weekday
/// subsets are the UI's arithmetic, so the engine never has to know what a parameter *means*.
#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct Axis {
    /// Dot-path into `settings`; a numeric segment indexes an array (same grammar as the agent
    /// sweep's grid keys), which is how an indicator period inside a conditions array is reached.
    #[serde(default)]
    pub path: String,
    /// Several paths written from the *same* value. One parameter of a mirrored strategy lives in
    /// two places (the long side and the short side derived from it): sweeping only one of them
    /// would leave the other at its old value and measure a strategy nobody configured.
    #[serde(default)]
    pub paths: Vec<String>,
    /// What the UI calls this axis. Echoed back so a reloaded page can label its columns.
    #[serde(default)]
    pub label: String,
    /// Display metadata, opaque to the engine and echoed back untouched: a stored `0.02` is a
    /// stop-loss of `2%`, and the ranking table has no other way to know that. `display = value /
    /// scale`, then `unit` after it.
    #[serde(default = "one")]
    pub scale: f64,
    #[serde(default)]
    pub unit: String,
    /// The values to try, in the order they are displayed.
    pub values: Vec<Value>,
}

fn one() -> f64 {
    1.0
}

/// Set a dot-path inside a settings JSON, creating intermediate objects.
///
/// A numeric segment indexes an array: `long.entry.conditions.0.left.period` is the ordinary
/// shape of a signal rule, so a grid that cannot reach into arrays cannot sweep an indicator
/// period, which is the parameter anyone actually wants to sweep. Indices must already exist;
/// a grid does not invent conditions.
pub fn set_path(root: &mut Value, path: &str, val: Value) -> Result<(), String> {
    let parts: Vec<&str> = path.split('.').filter(|s| !s.is_empty()).collect();
    if parts.is_empty() {
        return Err("empty grid path".into());
    }
    let mut cur = root;
    for seg in &parts[..parts.len() - 1] {
        cur = step(cur, seg, path)?;
    }
    let last = parts[parts.len() - 1];
    match cur {
        Value::Object(o) => {
            o.insert(last.to_string(), val);
            Ok(())
        }
        Value::Array(a) => {
            let i: usize = last
                .parse()
                .map_err(|_| format!("\"{path}\": \"{last}\" is not an array index"))?;
            let slot = a
                .get_mut(i)
                .ok_or_else(|| format!("\"{path}\": index {i} is past the end of the array"))?;
            *slot = val;
            Ok(())
        }
        _ => Err(format!("\"{path}\" does not name a field")),
    }
}

/// Descend one path segment, creating a missing object key on the way.
fn step<'a>(cur: &'a mut Value, seg: &str, path: &str) -> Result<&'a mut Value, String> {
    match cur {
        Value::Array(a) => {
            let i: usize = seg
                .parse()
                .map_err(|_| format!("\"{path}\": \"{seg}\" is not an array index"))?;
            let n = a.len();
            a.get_mut(i)
                .ok_or_else(|| format!("\"{path}\": index {i} is past the end ({n} items)"))
        }
        Value::Object(o) => Ok(o.entry(seg.to_string()).or_insert_with(|| Value::Object(Default::default()))),
        _ => Err(format!("\"{path}\": \"{seg}\" is not an object in settings")),
    }
}

/// Trials a set of axes describes: the product of their value counts, saturating.
pub fn total_trials(axes: &[Axis]) -> u64 {
    axes.iter()
        .map(|a| a.values.len().max(1) as u64)
        .try_fold(1u64, |acc, n| acc.checked_mul(n))
        .unwrap_or(u64::MAX)
}

/// Structural check on a grid, before anything is loaded or run.
/// Whether a dot-path names a field of the engine's `Settings` (an array index for a list, any
/// key for a map). Read off the settings' JSON schema, so a field added to the engine is known
/// here without a list to maintain. A misspelled path used to be written in as a dead key and
/// the grid ran the base strategy N times.
pub fn path_known(path: &str) -> bool {
    static SCHEMA: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    let root = SCHEMA.get_or_init(|| serde_json::to_value(schemars::schema_for!(Settings)).unwrap_or(Value::Null));
    let segs: Vec<&str> = path.split('.').collect();
    schema_walk(root, root, &segs, 0)
}

fn schema_walk(root: &Value, node: &Value, segs: &[&str], depth: usize) -> bool {
    if depth > 64 {
        return false;
    }
    let Some((head, rest)) = segs.split_first() else { return true };
    if let Some(name) = node.get("$ref").and_then(Value::as_str).and_then(|r| r.rsplit('/').next()) {
        let defs = root.get("$defs").or_else(|| root.get("definitions"));
        if defs.and_then(|d| d.get(name)).is_some_and(|t| schema_walk(root, t, segs, depth + 1)) {
            return true;
        }
    }
    for k in ["anyOf", "oneOf", "allOf"] {
        if node.get(k).and_then(Value::as_array).is_some_and(|a| a.iter().any(|sub| schema_walk(root, sub, segs, depth + 1))) {
            return true;
        }
    }
    if head.parse::<usize>().is_ok() && node.get("items").is_some_and(|i| schema_walk(root, i, rest, depth + 1)) {
        return true;
    }
    if node.get("properties").and_then(|p| p.get(*head)).is_some_and(|p| schema_walk(root, p, rest, depth + 1)) {
        return true;
    }
    node.get("additionalProperties").filter(|v| v.is_object()).is_some_and(|ap| schema_walk(root, ap, rest, depth + 1))
}

pub fn validate(axes: &[Axis]) -> Result<u64, String> {
    if axes.is_empty() {
        return Err("an optimization needs at least one parameter to vary".into());
    }
    if axes.len() > MAX_AXES {
        return Err(format!("at most {MAX_AXES} parameters per optimization"));
    }
    for a in axes {
        if a.targets().next().is_none() {
            return Err("a parameter with no path cannot be varied".into());
        }
        if a.values.is_empty() {
            return Err(format!("\"{}\" has no values to try", a.label_or_path()));
        }
        if let Some(bad) = a.targets().find(|p| !path_known(p)) {
            return Err(format!(
                "\"{bad}\" is not a setting the engine reads: /api/backtest/optimize/params lists the ones this strategy has"
            ));
        }
        if a.values.len() > MAX_AXIS_VALUES {
            return Err(format!(
                "\"{}\" asks for {} values; at most {MAX_AXIS_VALUES} per parameter",
                a.label_or_path(),
                a.values.len()
            ));
        }
    }
    let total = total_trials(axes);
    if total > MAX_TRIALS {
        return Err(format!(
            "that grid is {total} variants; the cap is {MAX_TRIALS}. Narrow a range or drop a \
             parameter"
        ));
    }
    Ok(total)
}

impl Axis {
    /// Every settings path this axis writes: `paths` when given, else the single `path`.
    fn targets(&self) -> impl Iterator<Item = &str> {
        let multi = (!self.paths.is_empty()).then_some(&self.paths);
        multi
            .into_iter()
            .flatten()
            .map(String::as_str)
            .chain(multi.is_none().then_some(self.path.as_str()))
            .filter(|p| !p.trim().is_empty())
    }

    fn label_or_path(&self) -> &str {
        for candidate in [self.label.as_str(), self.path.as_str()] {
            if !candidate.trim().is_empty() {
                return candidate;
            }
        }
        self.paths.first().map(String::as_str).unwrap_or("parameter")
    }
}

/// Digits of trial `i` in mixed radix over the axes (last axis varies fastest), so consecutive
/// trials differ in one parameter and a worker never needs the combination list.
pub fn digits(i: u64, axes: &[Axis]) -> Vec<u16> {
    let mut out = vec![0u16; axes.len()];
    let mut rest = i;
    for (k, a) in axes.iter().enumerate().rev() {
        let radix = a.values.len().max(1) as u64;
        out[k] = (rest % radix) as u16;
        rest /= radix;
    }
    out
}

/// The values trial `i` stands for, in axis order.
pub fn params_at(i: u64, axes: &[Axis]) -> Vec<Value> {
    digits(i, axes)
        .into_iter()
        .zip(axes)
        .map(|(d, a)| a.values.get(d as usize).cloned().unwrap_or(Value::Null))
        .collect()
}

/// Base settings with trial `i`'s values patched in.
pub fn patched(base: &Value, axes: &[Axis], i: u64) -> Result<Value, String> {
    let mut out = base.clone();
    for (d, a) in digits(i, axes).into_iter().zip(axes) {
        let v = a
            .values
            .get(d as usize)
            .cloned()
            .ok_or_else(|| format!("\"{}\": missing value {d}", a.label_or_path()))?;
        for path in a.targets().map(str::to_string).collect::<Vec<_>>() {
            set_path(&mut out, &path, v.clone())?;
        }
    }
    Ok(out)
}

/// One trial's result. `f32` throughout and NaN for "not computable". A quarter of a million of
/// these live in memory at once, and the ranking only ever needs display precision.
#[derive(Debug, Clone, Copy)]
pub struct Row {
    /// Trial index; the parameter values decode from it.
    pub i: u32,
    pub trades: u32,
    pub return_pct: f32,
    pub net_pnl: f32,
    pub sharpe: f32,
    pub sortino: f32,
    pub max_drawdown_pct: f32,
    pub profit_factor: f32,
    pub win_rate: f32,
    pub expectancy_pct: f32,
    pub avg_trade: f32,
    /// Out-of-sample return, when the settings carry a split. NaN otherwise.
    pub oos_return_pct: f32,
}

/// f32 → JSON, mapping NaN/±inf to null so the client never has to guess.
fn num(v: f32) -> Value {
    if v.is_finite() {
        serde_json::Number::from_f64(v as f64).map(Value::Number).unwrap_or(Value::Null)
    } else {
        Value::Null
    }
}

impl Row {
    /// The row as the ranking table reads it, with its rank and its parameter values.
    fn to_json(&self, rank: usize, axes: &[Axis]) -> Value {
        serde_json::json!({
            "rank": rank,
            "i": self.i,
            "params": params_at(self.i as u64, axes),
            "trades": self.trades,
            "return_pct": num(self.return_pct),
            "net_pnl": num(self.net_pnl),
            "sharpe": num(self.sharpe),
            "sortino": num(self.sortino),
            "max_drawdown_pct": num(self.max_drawdown_pct),
            "profit_factor": num(self.profit_factor),
            "win_rate": num(self.win_rate),
            "expectancy_pct": num(self.expectancy_pct),
            "avg_trade": num(self.avg_trade),
            "oos_return_pct": num(self.oos_return_pct),
        })
    }
}

/// Metrics a ranking can sort on. `higher_better` is false for the one metric where it isn't.
pub const METRICS: &[(&str, bool)] = &[
    ("sharpe", true),
    ("sortino", true),
    ("return_pct", true),
    ("net_pnl", true),
    ("profit_factor", true),
    ("win_rate", true),
    ("expectancy_pct", true),
    ("avg_trade", true),
    ("trades", true),
    ("max_drawdown_pct", false),
];

pub fn is_metric(m: &str) -> bool {
    METRICS.iter().any(|(id, _)| *id == m)
}

pub fn higher_better(m: &str) -> bool {
    METRICS.iter().find(|(id, _)| *id == m).map(|(_, hb)| *hb).unwrap_or(true)
}

/// A row's value for a metric. NaN when the metric didn't compute for that trial: such a row
/// sorts last whichever way the column points.
fn metric_of(r: &Row, m: &str) -> f32 {
    match m {
        "sortino" => r.sortino,
        "return_pct" => r.return_pct,
        "net_pnl" => r.net_pnl,
        "profit_factor" => r.profit_factor,
        "win_rate" => r.win_rate,
        "expectancy_pct" => r.expectancy_pct,
        "avg_trade" => r.avg_trade,
        "trades" => r.trades as f32,
        "oos_return_pct" => r.oos_return_pct,
        "max_drawdown_pct" => r.max_drawdown_pct,
        _ => r.sharpe,
    }
}

/// Order two rows on a metric, best first. Non-finite values sink to the bottom.
fn cmp_metric(a: &Row, b: &Row, m: &str, desc: bool) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (x, y) = (metric_of(a, m), metric_of(b, m));
    match (x.is_finite(), y.is_finite()) {
        (false, false) => a.i.cmp(&b.i),
        (false, true) => Ordering::Greater,
        (true, false) => Ordering::Less,
        (true, true) => {
            let ord = x.partial_cmp(&y).unwrap_or(Ordering::Equal);
            let ord = if desc { ord.reverse() } else { ord };
            ord.then_with(|| a.i.cmp(&b.i))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Running,
    Done,
    /// Stopped by the user. Whatever finished is kept and readable.
    Cancelled,
    Failed,
}

struct State {
    status: Status,
    /// First trial error, kept so a grid that patches nothing legal says why.
    error: Option<String>,
    /// Wall time of the finished job, frozen at the end.
    elapsed_ms: Option<u64>,
}

/// A running or finished optimization. Lives in memory only: `Jobs` holds the last few.
pub struct Job {
    pub id: Uuid,
    pub created_at: OffsetDateTime,
    /// What was simulated, echoed so a reloaded page needs nothing but the job id.
    pub dataset_ids: Vec<Uuid>,
    pub ticker: String,
    pub timeframe: String,
    pub bars: usize,
    /// Rows per year of the clock the trials walk, as the engine measured it for their Sharpe,
    /// and the rows the ranked Sharpe is computed over (the in-sample head under a split).
    pub ppy: Option<f64>,
    pub observations: usize,
    pub from: Option<OffsetDateTime>,
    pub to: Option<OffsetDateTime>,
    /// Base settings; every trial is this with one combination patched in.
    pub base: Value,
    pub axes: Vec<Axis>,
    pub total: u64,
    pub workers: usize,
    /// Metric the grid was launched to rank on (the table can still sort on any other).
    pub metric: String,
    next: AtomicU64,
    done: AtomicU64,
    failed: AtomicU64,
    cancel: AtomicBool,
    started: Instant,
    state: Mutex<State>,
    rows: Mutex<Vec<Row>>,
}

/// Everything needed to start a job except the bars.
pub struct NewJob {
    pub dataset_ids: Vec<Uuid>,
    pub ticker: String,
    pub timeframe: String,
    pub bars: usize,
    pub ppy: Option<f64>,
    pub observations: usize,
    pub from: Option<OffsetDateTime>,
    pub to: Option<OffsetDateTime>,
    pub base: Value,
    pub axes: Vec<Axis>,
    pub metric: String,
}

/// Worker threads to use: every core but one, so the box stays answerable while it grinds.
pub fn worker_count() -> usize {
    let n = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2);
    n.saturating_sub(1).clamp(1, MAX_WORKERS)
}

impl Job {
    fn new(n: NewJob, total: u64) -> Self {
        Job {
            id: Uuid::new_v4(),
            created_at: OffsetDateTime::now_utc(),
            dataset_ids: n.dataset_ids,
            ticker: n.ticker,
            timeframe: n.timeframe,
            bars: n.bars,
            ppy: n.ppy,
            observations: n.observations,
            from: n.from,
            to: n.to,
            base: n.base,
            axes: n.axes,
            total,
            workers: worker_count(),
            metric: n.metric,
            next: AtomicU64::new(0),
            done: AtomicU64::new(0),
            failed: AtomicU64::new(0),
            cancel: AtomicBool::new(false),
            started: Instant::now(),
            state: Mutex::new(State { status: Status::Running, error: None, elapsed_ms: None }),
            rows: Mutex::new(Vec::new()),
        }
    }

    pub fn status(&self) -> Status {
        self.state.lock().map(|s| s.status).unwrap_or(Status::Failed)
    }

    pub fn is_running(&self) -> bool {
        self.status() == Status::Running
    }

    /// Ask the workers to stop. They finish the trial in flight and leave the rows behind.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    fn elapsed_ms(&self) -> u64 {
        if let Ok(s) = self.state.lock() {
            if let Some(ms) = s.elapsed_ms {
                return ms;
            }
        }
        self.started.elapsed().as_millis() as u64
    }

    fn finish(&self, status: Status) {
        if let Ok(mut s) = self.state.lock() {
            s.status = status;
            s.elapsed_ms = Some(self.started.elapsed().as_millis() as u64);
        }
    }

    fn note_error(&self, msg: String) {
        if let Ok(mut s) = self.state.lock() {
            if s.error.is_none() {
                s.error = Some(msg);
            }
        }
    }

    /// Live progress. `eta_ms` is the measured pace of *this* job applied to what is left. No
    /// model, no guess: before any trial lands there is simply no estimate.
    pub fn progress(&self) -> Value {
        let done = self.done.load(Ordering::Relaxed);
        let elapsed = self.elapsed_ms();
        let per_trial = (done > 0).then(|| elapsed as f64 / done as f64);
        let eta = match (per_trial, self.is_running()) {
            (Some(p), true) => Some(((self.total.saturating_sub(done)) as f64 * p) as u64),
            _ => None,
        };
        let (status, error) = self
            .state
            .lock()
            .map(|s| (s.status, s.error.clone()))
            .unwrap_or((Status::Failed, None));
        serde_json::json!({
            "status": status,
            "done": done,
            "failed": self.failed.load(Ordering::Relaxed),
            "total": self.total,
            "elapsed_ms": elapsed,
            "eta_ms": eta,
            "per_trial_ms": per_trial,
            "error": error,
        })
    }

    /// Job identity + shape, for the page header and the job list.
    pub fn head(&self) -> Value {
        serde_json::json!({
            "id": self.id,
            "created_at": self.created_at
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_default(),
            "dataset_ids": self.dataset_ids,
            "ticker": self.ticker,
            "timeframe": self.timeframe,
            "bars": self.bars,
            // The window every trial ran over: a replayed variant has to use the same one.
            "from": self.from.and_then(|t| t.format(&time::format_description::well_known::Rfc3339).ok()),
            "to": self.to.and_then(|t| t.format(&time::format_description::well_known::Rfc3339).ok()),
            "workers": self.workers,
            "metric": self.metric,
            "axes": self.axes,
            // The base settings ride along: replaying a picked variant as a full backtest needs
            // them, and after a page reload the job id is all the client has.
            "settings": self.base,
        })
    }

    pub fn row_count(&self) -> usize {
        self.rows.lock().map(|r| r.len()).unwrap_or(0)
    }

    /// One page of the ranking, sorted on `metric`. Sorting a snapshot each time is what keeps
    /// the collection side lock-free-ish: workers only ever append.
    pub fn ranking(&self, metric: &str, desc: bool, offset: usize, limit: usize) -> Vec<Value> {
        let Ok(rows) = self.rows.lock() else { return Vec::new() };
        if offset >= rows.len() {
            return Vec::new();
        }
        let mut snap: Vec<Row> = rows.clone();
        drop(rows);
        let end = (offset + limit).min(snap.len());
        // Only the requested page needs to be in order: partial-sort the window, so a 250k-row
        // ranking costs a scan per poll and not a full sort.
        snap.select_nth_unstable_by(end - 1, |a, b| cmp_metric(a, b, metric, desc));
        let mut page: Vec<Row> = snap[..end].to_vec();
        page.sort_by(|a, b| cmp_metric(a, b, metric, desc));
        page[offset..end]
            .iter()
            .enumerate()
            .map(|(k, r)| r.to_json(offset + k + 1, &self.axes))
            .collect()
    }

    /// Per-axis effect of each value on the metric: how many trials used it, their average, and
    /// the best one. The question a grid is really asked ("does this parameter matter at all?")
    /// is answered here, not by the winning row.
    pub fn sensitivity(&self, metric: &str) -> Value {
        let Ok(rows) = self.rows.lock() else { return Value::Array(Vec::new()) };
        let desc = higher_better(metric);
        let mut acc: Vec<Vec<(u64, f64, f32)>> = self
            .axes
            .iter()
            .map(|a| vec![(0u64, 0.0f64, f32::NAN); a.values.len()])
            .collect();
        for r in rows.iter() {
            let v = metric_of(r, metric);
            if !v.is_finite() {
                continue;
            }
            for (k, d) in digits(r.i as u64, &self.axes).into_iter().enumerate() {
                let Some(slot) = acc[k].get_mut(d as usize) else { continue };
                slot.0 += 1;
                slot.1 += v as f64;
                let better = !slot.2.is_finite() || if desc { v > slot.2 } else { v < slot.2 };
                if better {
                    slot.2 = v;
                }
            }
        }
        drop(rows);
        let out: Vec<Value> = self
            .axes
            .iter()
            .zip(acc)
            .map(|(a, stats)| {
                serde_json::json!({
                    "path": a.path,
                    "label": a.label,
                    "values": a.values.iter().zip(stats).map(|(v, (n, sum, best))| {
                        serde_json::json!({
                            "value": v,
                            "n": n,
                            "avg": (n > 0).then(|| sum / n as f64),
                            "best": num(best),
                        })
                    }).collect::<Vec<_>>(),
                })
            })
            .collect();
        Value::Array(out)
    }

    /// Distribution of the metric across every finished trial, plus the trial Sharpes the
    /// deflated-Sharpe haircut needs. Buckets are uniform over the observed range.
    pub fn distribution(&self, metric: &str, buckets: usize) -> (Value, Vec<f64>) {
        let buckets = buckets.clamp(4, 60);
        let Ok(rows) = self.rows.lock() else { return (Value::Null, Vec::new()) };
        let sharpes: Vec<f64> = rows.iter().filter(|r| r.sharpe.is_finite()).map(|r| r.sharpe as f64).collect();
        let vals: Vec<f32> = rows.iter().map(|r| metric_of(r, metric)).filter(|v| v.is_finite()).collect();
        drop(rows);
        if vals.is_empty() {
            return (Value::Null, sharpes);
        }
        let min = vals.iter().cloned().fold(f32::INFINITY, f32::min);
        let max = vals.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let span = (max - min).max(f32::MIN_POSITIVE);
        let mut hist = vec![0u64; buckets];
        for v in &vals {
            let k = (((v - min) / span) * buckets as f32) as usize;
            hist[k.min(buckets - 1)] += 1;
        }
        let mean = vals.iter().map(|v| *v as f64).sum::<f64>() / vals.len() as f64;
        let positive = vals.iter().filter(|v| **v > 0.0).count();
        (
            serde_json::json!({
                "metric": metric,
                "n": vals.len(),
                "min": num(min),
                "max": num(max),
                "mean": mean,
                "positive": positive,
                "buckets": hist,
            }),
            sharpes,
        )
    }

    /// Resolve a cloud dimension id: a metric (`sharpe`) or a swept parameter (`p0`, `p1`…).
    pub fn cloud_dim(&self, id: &str) -> Option<CloudDim> {
        if let Some(k) = id.strip_prefix('p').and_then(|k| k.parse::<usize>().ok()) {
            return (k < self.axes.len()).then_some(CloudDim::Param(k));
        }
        is_metric(id).then(|| CloudDim::Metric(id.to_string()))
    }

    /// A row's coordinate on one dimension. A parameter reads its own value when every value of
    /// its axis is a number, else the position of the value in the axis (weekday subsets), which
    /// the client labels back from `axes[k].values`.
    fn dim_value(&self, r: &Row, d: &CloudDim, numeric: &[bool]) -> f32 {
        match d {
            CloudDim::Metric(m) => metric_of(r, m),
            CloudDim::Param(k) => {
                let d = digits(r.i as u64, &self.axes)[*k] as usize;
                if numeric[*k] {
                    self.axes[*k].values.get(d).and_then(Value::as_f64).map_or(f32::NAN, |v| v as f32)
                } else {
                    d as f32
                }
            }
        }
    }

    /// The 3D cloud: every finished trial on three dimensions, narrowed by min/max filters and
    /// thinned to `max` points. Bounds are over the whole grid, not the filtered part, so a
    /// slider keeps its track while it is dragged.
    ///
    /// Thinning keeps the best tenth by `best` (the frontier is what the reader looks for) and
    /// fills the rest with an even stride over what matched, so the shape of the cloud survives.
    pub fn cloud(&self, dims: &[CloudDim; 3], filters: &[(CloudDim, f32, f32)], max: usize, best: (&str, bool)) -> Value {
        let numeric: Vec<bool> = self
            .axes
            .iter()
            .map(|a| !a.values.is_empty() && a.values.iter().all(Value::is_number))
            .collect();
        let Ok(rows) = self.rows.lock() else { return Value::Null };
        let mut bounds = [(f32::INFINITY, f32::NEG_INFINITY); 3];
        let mut matched: Vec<(usize, [f32; 3])> = Vec::new();
        let mut skipped = 0usize;
        'rows: for (idx, r) in rows.iter().enumerate() {
            let xyz = [0, 1, 2].map(|k| self.dim_value(r, &dims[k], &numeric));
            if xyz.iter().any(|v| !v.is_finite()) {
                skipped += 1;
                continue;
            }
            for (b, v) in bounds.iter_mut().zip(xyz) {
                b.0 = b.0.min(v);
                b.1 = b.1.max(v);
            }
            for (d, lo, hi) in filters {
                let v = self.dim_value(r, d, &numeric);
                if !(v >= *lo && v <= *hi) {
                    continue 'rows;
                }
            }
            matched.push((idx, xyz));
        }
        let n = matched.len();
        let keep: Vec<(usize, [f32; 3])> = if n <= max {
            matched
        } else {
            let top = (max / 10).max(1);
            let (metric, desc) = best;
            matched.select_nth_unstable_by(top - 1, |a, b| cmp_metric(&rows[a.0], &rows[b.0], metric, desc));
            let (head, rest) = matched.split_at_mut(top);
            // Stride over the rest in trial order, so the same filter always shows the same points.
            rest.sort_unstable_by_key(|m| m.0);
            let room = max - top;
            let step = rest.len() as f64 / room as f64;
            let mut out = head.to_vec();
            out.extend((0..room).map(|j| rest[((j as f64 * step) as usize).min(rest.len() - 1)]));
            out
        };
        let points: Vec<Value> = keep
            .iter()
            .map(|(idx, [x, y, z])| {
                let r = &rows[*idx];
                serde_json::json!([num(*x), num(*y), num(*z), r.i, num(r.return_pct)])
            })
            .collect();
        let total = rows.len();
        drop(rows);
        let bounds: Vec<Value> = bounds
            .iter()
            .map(|(lo, hi)| if lo <= hi { serde_json::json!([num(*lo), num(*hi)]) } else { Value::Null })
            .collect();
        serde_json::json!({
            "total": total,
            "skipped": skipped,
            "matched": n,
            "bounds": bounds,
            "points": points,
        })
    }
}

/// One dimension of the 3D cloud.
pub enum CloudDim {
    Metric(String),
    /// Index into the job's axes.
    Param(usize),
}

/// Simulate one trial and reduce it to a row. Errors are per-trial: an illegal combination is
/// counted, never fatal: one impossible corner of a grid must not throw the other 130k away.
fn trial(job: &Job, refs: &[&Bars], i: u64) -> Result<Row, String> {
    let patched = patched(&job.base, &job.axes, i)?;
    let settings: Settings =
        serde_json::from_value(patched).map_err(|e| format!("variant settings are invalid: {e}"))?;
    if let Some(err) = settings.validate() {
        return Err(err);
    }
    let res = super::run_portfolio(&settings, refs);
    // Under an in/out-of-sample split every ranked figure is the in-sample one: ranking on the
    // whole window would pick the winner on the very bars its out-of-sample block then "tests".
    let s = res.oos.as_ref().map_or(&res.stats, |o| &o.in_sample);
    Ok(Row {
        i: i as u32,
        trades: s.trades as u32,
        return_pct: s.return_pct as f32,
        net_pnl: s.net_pnl as f32,
        sharpe: s.sharpe.unwrap_or(f64::NAN) as f32,
        sortino: s.sortino.unwrap_or(f64::NAN) as f32,
        max_drawdown_pct: s.max_drawdown_pct as f32,
        profit_factor: s.profit_factor as f32,
        win_rate: s.win_rate as f32,
        expectancy_pct: s.expectancy_pct as f32,
        avg_trade: s.avg_trade as f32,
        oos_return_pct: res
            .oos
            .as_ref()
            .map(|o| o.out_sample.return_pct as f32)
            .unwrap_or(f32::NAN),
    })
}

fn flush(job: &Job, buf: &mut Vec<Row>) {
    if buf.is_empty() {
        return;
    }
    if let Ok(mut rows) = job.rows.lock() {
        rows.append(buf);
    }
    buf.clear();
}

/// One worker: pull the next trial index, simulate, buffer the row. The index counter is the
/// only coordination, since trials are independent, so there is nothing else to share.
fn worker_loop(job: &Job, refs: &[&Bars]) {
    let mut buf: Vec<Row> = Vec::with_capacity(FLUSH_EVERY);
    loop {
        if job.cancel.load(Ordering::Relaxed) {
            break;
        }
        let i = job.next.fetch_add(1, Ordering::Relaxed);
        if i >= job.total {
            break;
        }
        match trial(job, refs, i) {
            Ok(row) => buf.push(row),
            Err(e) => {
                job.failed.fetch_add(1, Ordering::Relaxed);
                job.note_error(e);
            }
        }
        job.done.fetch_add(1, Ordering::Relaxed);
        if buf.len() >= FLUSH_EVERY {
            flush(job, &mut buf);
        }
    }
    flush(job, &mut buf);
}

/// Run the grid to completion (or to a cancel). Blocking and CPU-bound: call it from
/// `spawn_blocking`, never on a runtime thread.
pub fn work(job: &Job, assets: &[OwnedBars]) {
    let bars: Vec<Bars> = assets.iter().map(|a| a.as_bars()).collect();
    let refs: Vec<&Bars> = bars.iter().collect();
    std::thread::scope(|scope| {
        for _ in 0..job.workers {
            let refs = &refs;
            scope.spawn(move || worker_loop(job, refs));
        }
    });
    let status = if job.cancel.load(Ordering::Relaxed) {
        Status::Cancelled
    } else if job.failed.load(Ordering::Relaxed) >= job.total {
        Status::Failed
    } else {
        Status::Done
    };
    job.finish(status);
}

/// The in-memory job registry. Single-user install, one optimization at a time: a second start
/// while one runs is refused rather than queued, because two grids on the same cores make both
/// slower and neither honest about its ETA.
pub struct Jobs {
    inner: Mutex<Vec<Arc<Job>>>,
}

impl Default for Jobs {
    fn default() -> Self {
        Self::new()
    }
}

impl Jobs {
    pub fn new() -> Self {
        Jobs { inner: Mutex::new(Vec::new()) }
    }

    /// The running job, if any.
    pub fn running(&self) -> Option<Arc<Job>> {
        let jobs = self.inner.lock().ok()?;
        jobs.iter().find(|j| j.is_running()).cloned()
    }

    /// Register a new job (newest first) and drop the oldest finished ones past the cap.
    pub fn add(&self, n: NewJob, total: u64) -> Arc<Job> {
        let job = Arc::new(Job::new(n, total));
        if let Ok(mut jobs) = self.inner.lock() {
            jobs.insert(0, job.clone());
            // Keep the running one whatever its age; only finished jobs are prunable.
            while jobs.len() > MAX_JOBS {
                let Some(pos) = jobs.iter().rposition(|j| !j.is_running()) else { break };
                jobs.remove(pos);
            }
        }
        job
    }

    pub fn get(&self, id: Uuid) -> Option<Arc<Job>> {
        let jobs = self.inner.lock().ok()?;
        jobs.iter().find(|j| j.id == id).cloned()
    }

    pub fn list(&self) -> Vec<Arc<Job>> {
        self.inner.lock().map(|j| j.clone()).unwrap_or_default()
    }

    /// Forget a job, stopping it first if it was still going.
    pub fn remove(&self, id: Uuid) -> bool {
        let Ok(mut jobs) = self.inner.lock() else { return false };
        let Some(pos) = jobs.iter().position(|j| j.id == id) else { return false };
        jobs[pos].cancel();
        jobs.remove(pos);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn axis(path: &str, values: Vec<Value>) -> Axis {
        Axis { path: path.into(), paths: Vec::new(), label: path.into(), scale: 1.0, unit: String::new(), values }
    }

    /// A mirrored strategy keeps the same parameter in two places; one axis has to write both,
    /// or the sweep measures a long side that moved and a short side that didn't.
    #[test]
    fn a_multi_path_axis_writes_every_target() {
        let base = json!({ "long": { "stop_loss_pct": 0.02 }, "short": { "stop_loss_pct": 0.02 } });
        let axes = vec![Axis {
            path: String::new(),
            paths: vec!["long.stop_loss_pct".into(), "short.stop_loss_pct".into()],
            label: "Stop-loss".into(),
            scale: 0.01,
            unit: "%".into(),
            values: vec![json!(0.01), json!(0.03)],
        }];
        let out = patched(&base, &axes, 1).unwrap();
        assert_eq!(out["long"]["stop_loss_pct"], json!(0.03));
        assert_eq!(out["short"]["stop_loss_pct"], json!(0.03));
        assert_eq!(validate(&axes).unwrap(), 2);
    }

    #[test]
    fn counts_the_product_of_the_axes() {
        let axes = vec![axis("a", vec![json!(1), json!(2)]), axis("b", vec![json!(1), json!(2), json!(3)])];
        assert_eq!(total_trials(&axes), 6);
        // An empty axis counts as one so a degenerate grid still has a trial 0 rather than none.
        assert_eq!(total_trials(&[axis("a", vec![])]), 1);
    }

    /// The whole point of mixed radix: every trial index maps to a distinct combination, and no
    /// list of combinations is ever built.
    #[test]
    fn digits_enumerate_every_combination_exactly_once() {
        let axes = vec![
            axis("a", vec![json!(1), json!(2)]),
            axis("b", vec![json!(10), json!(20), json!(30)]),
        ];
        let total = total_trials(&axes);
        let mut seen: Vec<Vec<u16>> = (0..total).map(|i| digits(i, &axes)).collect();
        assert_eq!(seen.len(), 6);
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), 6, "combinations must be distinct");
        // Last axis varies fastest: consecutive trials differ in one parameter.
        assert_eq!(digits(0, &axes), vec![0, 0]);
        assert_eq!(digits(1, &axes), vec![0, 1]);
        assert_eq!(digits(3, &axes), vec![1, 0]);
        assert_eq!(params_at(4, &axes), vec![json!(2), json!(20)]);
    }

    #[test]
    fn patches_indicator_periods_inside_arrays() {
        let base = json!({
            "long": { "entry": { "conditions": [
                { "left": { "indicator": "sma", "period": 20 }, "right": { "indicator": "sma", "period": 50 } }
            ]}}
        });
        let axes = vec![
            axis("long.entry.conditions.0.left.period", vec![json!(5), json!(10)]),
            axis("long.entry.conditions.0.right.period", vec![json!(30), json!(40)]),
        ];
        let out = patched(&base, &axes, 3).unwrap();
        assert_eq!(out["long"]["entry"]["conditions"][0]["left"]["period"], json!(10));
        assert_eq!(out["long"]["entry"]["conditions"][0]["right"]["period"], json!(40));
        // The base is untouched: every worker patches its own copy.
        assert_eq!(base["long"]["entry"]["conditions"][0]["left"]["period"], json!(20));
    }

    #[test]
    fn refuses_a_grid_it_cannot_run() {
        assert!(validate(&[]).is_err());
        assert!(validate(&[axis("leverage", vec![])]).is_err());
        let wide: Vec<Axis> = (0..MAX_AXES + 1).map(|k| axis(&format!("long.entry.conditions.{k}.left.period"), vec![json!(1)])).collect();
        assert!(validate(&wide).is_err());
        // 4 axes of 40 values = 2.56M trials, past the cap.
        let big: Vec<Axis> = (0..4)
            .map(|k| axis(&format!("long.entry.conditions.{k}.left.period"), (0..40).map(|v| json!(v)).collect()))
            .collect();
        let e = validate(&big).unwrap_err();
        assert!(e.contains("cap"), "{e}");
        assert_eq!(validate(&[axis("leverage", vec![json!(1), json!(2)])]).unwrap(), 2);
        let e = validate(&[axis("long.stop_los_pct", vec![json!(1)])]).unwrap_err();
        assert!(e.contains("not a setting"), "{e}");
    }

    fn row(i: u32, sharpe: f32, dd: f32) -> Row {
        Row {
            i,
            trades: 10,
            return_pct: 0.0,
            net_pnl: 0.0,
            sharpe,
            sortino: f32::NAN,
            max_drawdown_pct: dd,
            profit_factor: 0.0,
            win_rate: 0.0,
            expectancy_pct: 0.0,
            avg_trade: 0.0,
            oos_return_pct: f32::NAN,
        }
    }

    /// Synthetic bars: a slow wave, so a moving-average crossover actually trades.
    fn wave(n: usize) -> OwnedBars {
        let mut ts = Vec::with_capacity(n);
        let mut close = Vec::with_capacity(n);
        let start = OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap();
        for i in 0..n {
            let t = start + time::Duration::hours(i as i64);
            ts.push(t.format(&time::format_description::well_known::Rfc3339).unwrap());
            close.push(100.0 + 10.0 * (i as f64 / 12.0).sin() + (i as f64 / 40.0));
        }
        OwnedBars {
            ticker: "TEST".into(),
            timeframe: "1h".into(),
            open: close.clone(),
            high: close.iter().map(|c| c * 1.002).collect(),
            low: close.iter().map(|c| c * 0.998).collect(),
            volume: vec![1000.0; n],
            ts,
            close,
            quotes: None,
            sub: None,
        }
    }

    fn crossover_settings() -> Value {
        json!({
            "mode": "long",
            "long": {
                "entry": { "logic": "all", "conditions": [{
                    "left": { "kind": "indicator", "indicator": "sma", "period": 5 },
                    "op": "crosses_above",
                    "right": { "kind": "indicator", "indicator": "sma", "period": 20 }
                }]},
                "exit": { "logic": "all", "conditions": [] },
                "stop_loss_pct": 0.02,
                "take_profit_pct": 0.04,
                "exit_on_reverse": true
            },
            "sizing": { "mode": "percent_equity", "percent": 100 },
            "starting_capital": 10000
        })
    }

    /// #3 of the lookahead audit: with an in/out-of-sample split, a trial's ranked figures are
    /// its in-sample block, never the whole window its out-of-sample block is part of.
    #[test]
    fn a_split_grid_ranks_on_its_in_sample_block() {
        let jobs = Jobs::new();
        let mut base = crossover_settings();
        base["oos_split_pct"] = json!(0.5);
        let axes = vec![axis("long.entry.conditions.0.left.period", vec![json!(5)])];
        let total = validate(&axes).unwrap();
        let job = jobs.add(
            NewJob {
                dataset_ids: vec![Uuid::new_v4()],
                ticker: "TEST".into(),
                timeframe: "1h".into(),
                bars: 400,
                ppy: None,
                observations: 200,
                from: None,
                to: None,
                base: base.clone(),
                axes,
                metric: "return_pct".into(),
            },
            total,
        );
        let bars = wave(400);
        work(&job, std::slice::from_ref(&bars));
        let settings: Settings = serde_json::from_value(base).unwrap();
        let direct = super::super::run_portfolio(&settings, &[&bars.as_bars()]);
        let oos = direct.oos.as_ref().unwrap();
        assert!((oos.in_sample.return_pct - direct.stats.return_pct).abs() > 1e-6, "the blocks must differ for this to test anything");
        let row = &job.ranking("return_pct", true, 0, 1)[0];
        let ranked = row["return_pct"].as_f64().unwrap();
        assert!((ranked - oos.in_sample.return_pct).abs() < 1e-3, "{ranked} vs in-sample {}", oos.in_sample.return_pct);
    }

    fn two_axis_job(cancelled: bool) -> Arc<Job> {
        let jobs = Jobs::new();
        let axes = vec![
            axis("long.entry.conditions.0.left.period", vec![json!(5), json!(8)]),
            axis("long.entry.conditions.0.right.period", vec![json!(20), json!(30)]),
        ];
        let total = validate(&axes).unwrap();
        let job = jobs.add(
            NewJob {
                dataset_ids: vec![Uuid::new_v4()],
                ticker: "TEST".into(),
                timeframe: "1h".into(),
                bars: 400,
                ppy: None,
                observations: 400,
                from: None,
                to: None,
                base: crossover_settings(),
                axes,
                metric: "sharpe".into(),
            },
            total,
        );
        if cancelled {
            job.cancel();
        }
        work(&job, &[wave(400)]);
        job
    }

    /// The whole loop, for real: four variants simulated across the worker threads, then read
    /// back as a ranking and as a per-parameter effect table.
    #[test]
    fn runs_a_grid_and_reads_it_back() {
        let job = two_axis_job(false);
        assert_eq!(job.status(), Status::Done);
        let p = job.progress();
        assert_eq!(p["done"], json!(4));
        assert_eq!(p["failed"], json!(0));
        assert_eq!(job.row_count(), 4, "every variant must land exactly one row");

        let page = job.ranking("return_pct", true, 0, 10);
        assert_eq!(page.len(), 4);
        // Ranks are 1-based and the page is ordered on the metric asked for.
        let ranks: Vec<u64> = page.iter().map(|r| r["rank"].as_u64().unwrap()).collect();
        assert_eq!(ranks, vec![1, 2, 3, 4]);
        let rets: Vec<f64> = page.iter().filter_map(|r| r["return_pct"].as_f64()).collect();
        assert!(rets.windows(2).all(|w| w[0] >= w[1]), "descending: {rets:?}");
        // Each row carries the parameter values it stands for, in axis order.
        for row in &page {
            let params = row["params"].as_array().unwrap();
            assert!([json!(5), json!(8)].contains(&params[0]));
            assert!([json!(20), json!(30)].contains(&params[1]));
        }
        // A second page past the end is empty rather than an error.
        assert!(job.ranking("return_pct", true, 10, 10).is_empty());

        // Two axes of two values over four trials: every value was used exactly twice.
        let sens = job.sensitivity("return_pct");
        let sens = sens.as_array().unwrap();
        assert_eq!(sens.len(), 2);
        for axis in sens {
            for v in axis["values"].as_array().unwrap() {
                assert_eq!(v["n"], json!(2), "{axis:?}");
            }
        }
        let (dist, sharpes) = job.distribution("return_pct", 8);
        assert_eq!(dist["n"], json!(4));
        assert!(sharpes.len() <= 4);
    }

    /// The 3D cloud reads parameters as their own values, filters on any dimension, and thins
    /// to the cap while keeping the best trial.
    #[test]
    fn the_cloud_filters_and_thins() {
        let job = two_axis_job(false);
        let dims = || [job.cloud_dim("p0").unwrap(), job.cloud_dim("p1").unwrap(), job.cloud_dim("trades").unwrap()];
        assert!(job.cloud_dim("p2").is_none() && job.cloud_dim("nope").is_none());

        let all = job.cloud(&dims(), &[], 100, ("return_pct", true));
        assert_eq!(all["matched"], json!(4));
        assert_eq!(all["bounds"][0], json!([5.0, 8.0]));
        assert_eq!(all["bounds"][1], json!([20.0, 30.0]));

        let only_fast = job.cloud(&dims(), &[(job.cloud_dim("p0").unwrap(), 5.0, 6.0)], 100, ("return_pct", true));
        assert_eq!(only_fast["matched"], json!(2));
        assert!(only_fast["points"].as_array().unwrap().iter().all(|p| p[0] == json!(5.0)));
        // Bounds stay those of the whole grid, so a slider keeps its track.
        assert_eq!(only_fast["bounds"][0], json!([5.0, 8.0]));

        let best = job.ranking("return_pct", true, 0, 1)[0]["i"].clone();
        let one = job.cloud(&dims(), &[], 1, ("return_pct", true));
        let pts = one["points"].as_array().unwrap();
        assert_eq!(pts.len(), 1);
        assert_eq!(pts[0][3], best);
    }

    /// Stopping is not discarding: the job ends as cancelled and whatever finished stays readable.
    #[test]
    fn a_cancelled_grid_keeps_what_it_had() {
        let job = two_axis_job(true);
        assert_eq!(job.status(), Status::Cancelled);
        assert!(job.row_count() <= 4);
        assert_eq!(job.progress()["status"], json!("cancelled"));
    }

    /// A trial whose metric didn't compute must not win the ranking by being NaN.
    #[test]
    fn ranks_best_first_and_sinks_the_incomputable() {
        let mut rows = vec![row(0, 1.0, 10.0), row(1, f32::NAN, 5.0), row(2, 2.0, 20.0)];
        rows.sort_by(|a, b| cmp_metric(a, b, "sharpe", true));
        assert_eq!(rows.iter().map(|r| r.i).collect::<Vec<_>>(), vec![2, 0, 1]);
        // Drawdown is the metric where less is better, so its column points the other way.
        rows.sort_by(|a, b| cmp_metric(a, b, "max_drawdown_pct", false));
        assert_eq!(rows.iter().map(|r| r.i).collect::<Vec<_>>(), vec![1, 0, 2]);
        assert!(higher_better("sharpe"));
        assert!(!higher_better("max_drawdown_pct"));
    }
}

#[cfg(test)]
mod verify_rank {
    use super::*;

    fn row(i: u32, pf: f32) -> Row {
        Row {
            i,
            trades: 10,
            return_pct: 1.0,
            net_pnl: 1.0,
            sharpe: 1.0,
            sortino: 1.0,
            max_drawdown_pct: 1.0,
            profit_factor: pf,
            win_rate: 50.0,
            expectancy_pct: 1.0,
            avg_trade: 1.0,
            oos_return_pct: f32::NAN,
        }
    }

    /// A trial with no losing trades carries the no-losses sentinel and must rank FIRST on a
    /// descending profit-factor sort. Before the fix it carried 0.0 and ranked last; with
    /// `f64::INFINITY` it would have ranked last too, since `cmp_metric` sinks non-finite.
    #[test]
    fn vr_no_loss_trial_ranks_first() {
        let mut rows = vec![
            row(0, 1.5),
            row(1, super::super::PROFIT_FACTOR_NO_LOSSES as f32),
            row(2, 3.0),
        ];
        rows.sort_by(|a, b| cmp_metric(a, b, "profit_factor", true));
        assert_eq!(rows[0].i, 1, "the no-losses trial must rank first, got trial {}", rows[0].i);
        assert_eq!(rows[1].i, 2);
        assert_eq!(rows[2].i, 0);
    }
}

#[cfg(test)]
mod path_check {
    use super::path_known;

    #[test]
    fn the_paths_the_screen_writes_are_known() {
        for p in [
            "starting_capital",
            "leverage",
            "pyramiding",
            "fees.amount",
            "spread_pct",
            "slippage.value",
            "sizing.percent",
            "sizing.qty",
            "sizing.window",
            "sizing.cap_pct",
            "risk.max_open_positions",
            "risk.max_drawdown_pct",
            "pyramid_steps.min_distance_pct",
            "long.stop_loss.value",
            "long.stop_loss_pct",
            "short.take_profit.period",
            "long.trailing_stop.value",
            "long.entry.conditions.0.left.period",
            "long.entry.conditions.3.right.value",
            "grid.levels",
            "grid.width_value",
            "dca.contribution.amount",
            "dca.buys.0.cooldown_bars",
            "dca.sells.1.target_gain_pct",
            "instrument.tick_size",
        ] {
            assert!(path_known(p), "{p} should be a known setting");
        }
    }

    #[test]
    fn a_misspelled_path_is_not() {
        for p in ["long.nope", "long.stop_los_pct", "grid.level", "sizing.percent.x", "foo"] {
            assert!(!path_known(p), "{p} should be refused");
        }
    }
}
