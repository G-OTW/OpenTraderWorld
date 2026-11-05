//! Chart alerts, evaluated server-side.
//!
//! The whole point is that an alert works with no browser open, so this is a background loop
//! over `histviz_alerts` and never a timer in the page.
//!
//! An alert compares two series, each a backtest `Operand` (price, built-in indicator, custom
//! indicator or script plot, metric, or a fixed value), through the same `resolve` a backtest
//! runs, so "SMA 9 crosses above SMA 21" and "close crosses 64,000" are the same row.
//!
//! Three rules decide everything here:
//!
//! - **Closed bars only.** A forming candle's high is not a fact yet: it can be revised by
//!   the next tick, and an alert that fired on it would be reporting something that never
//!   happened. The evaluator drops any bar whose period has not ended.
//! - **A cross, not a state.** `above` means the series *crossed* the other one going up (the
//!   previous closed bar was at or below it), not "is currently above". A level alert placed
//!   under the current price would otherwise fire the instant it is created, which is the one
//!   thing nobody wants from an alert.
//! - **Judged when the bar closes.** The loop wakes a few seconds after each minute boundary,
//!   and an alert is read once the bar after the one it last judged has closed, so a 1m alert
//!   fires seconds after the close and a daily one costs one read a day. A bar the provider
//!   has not published yet is asked for again at the next wake-up. A stored instrument costs
//!   nothing to read; an unstored one costs one small request per read.

use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use otw_store::backtest as bt_store;
use otw_store::crypto::SecretCipher;
use otw_store::histdata::{self as hist_store, Bar};
use otw_store::histviz::{self as store, Alert};
use otw_store::market_sessions::{self as sessions_store, Session};
use otw_store::reminders::FiredNotification;

use crate::backtest::custom::CustomIndicatorDef;
use crate::backtest::script::{self as bt_script, RawInput, ScriptDef, ScriptDefs, ScriptHistory, WarmBars};
use crate::backtest::{self, Bars, Defs, IndicatorDefs, Operand, PriceField};
use crate::market_sessions_api;
use crate::notif_send;
use otw_store::connectors as conn_store;

/// The loop wakes on every minute boundary, this many seconds past it: bars close on the
/// minute, and a provider needs a moment to publish the bar that just closed.
const SETTLE_SECS: i64 = 3;
/// Wait before the first pass, so a restart does not race the migrations and the connectors.
const WARMUP: Duration = Duration::from_secs(20);
/// An alert is never read twice within this, whatever the wake-ups (the store's coarse filter).
const MIN_GAP: i64 = 55;
/// An alert that has judged no bar yet, or failed, is retried after its bar length, bounded:
/// at least a minute, at most a quarter hour, so a refused key does not cost a request a
/// minute and a fixed connector is picked up within the quarter hour.
const MIN_RETRY: i64 = 60;
const MAX_RETRY: i64 = 900;
/// Bars asked for beyond an indicator's own lookback, so the series is warm and the previous
/// closed bar (the one a cross is measured against) is always there.
const EXTRA_BARS: i64 = 5;
/// Ceiling on the window one evaluation may ask a provider for. A script reads this whole
/// window: it has no declared lookback, and its own `na` bars gate what it plots.
pub(crate) const MAX_BARS: i64 = 400;
/// Run cache one alert's script history may hold.
const SCRIPT_CACHE_BYTES: usize = 4 << 20;

pub struct AlertCx {
    pub cipher: SecretCipher,
    pub http: Client,
}

pub fn spawn(pool: PgPool, cx: AlertCx) {
    tokio::spawn(async move {
        tokio::time::sleep(WARMUP).await;
        loop {
            tokio::time::sleep(until_next_wake(OffsetDateTime::now_utc())).await;
            if let Err(e) = pass(&pool, &cx).await {
                tracing::error!("chart alerts: pass failed: {e:#}");
            }
        }
    });
}

/// Time until the next wake-up: `SETTLE_SECS` past the next minute boundary.
fn until_next_wake(now: OffsetDateTime) -> Duration {
    let ms = (now.unix_timestamp_nanos() / 1_000_000) as i64;
    let next = (ms.div_euclid(60_000) + 1) * 60_000 + SETTLE_SECS * 1000;
    // Still before this minute's settle point (woken early): wait for it, not the next one.
    let this = ms.div_euclid(60_000) * 60_000 + SETTLE_SECS * 1000;
    let at = if this > ms { this } else { next };
    Duration::from_millis((at - ms) as u64)
}

/// One sweep: every enabled alert with a bar to judge.
async fn pass(pool: &PgPool, cx: &AlertCx) -> Result<()> {
    let expired = store::expire_alerts(pool).await?;
    if expired > 0 {
        tracing::info!("chart alerts: {expired} expired");
    }
    let now = OffsetDateTime::now_utc();
    // The store filter is the coarse one; whether a bar has closed is decided here, where the
    // timeframe is known.
    let stale = now - time::Duration::seconds(MIN_GAP);
    let due = store::due_alerts(pool, stale).await?;
    for a in due {
        // A time alert reads no bars: the clock is the whole condition.
        if a.op == "at_time" {
            if a.fire_at.is_some_and(|t| t <= now) {
                if let Err(e) = fire_time(pool, cx, &a).await {
                    let msg = short(&format!("{e:#}"));
                    tracing::warn!("chart alert {} ({}): {msg}", a.id, a.ticker);
                    let _ = store::mark_error(pool, a.id, &msg).await;
                }
            }
            continue;
        }
        if !is_due(&a, now) {
            continue;
        }
        if let Err(e) = evaluate(pool, cx, &a).await {
            let msg = short(&format!("{e:#}"));
            tracing::warn!("chart alert {} ({}): {msg}", a.id, a.ticker);
            let _ = store::mark_error(pool, a.id, &msg).await;
        }
    }
    Ok(())
}

/// Whether this alert has something to judge. Once it has judged a bar, it is due when the
/// next bar has closed (that bar opens `tf` after the judged one and closes `tf` later), and
/// stays due until the provider has it. Before its first bar, or after a failure, it is
/// retried after its bar length, bounded.
fn is_due(a: &Alert, now: OffsetDateTime) -> bool {
    let Some(checked) = a.checked_at else { return true };
    let since = (now - checked).whole_seconds();
    if since < MIN_GAP {
        return false;
    }
    let tf = crate::histdata::timeframe_secs(&a.timeframe).unwrap_or(3600);
    match a.last_bar_ts {
        Some(judged) if a.last_error.is_empty() => now >= judged + time::Duration::seconds(2 * tf),
        // A wake-up lands a fraction of a second short of the full length after the last
        // read (stamped once the read finished): the gap `MIN_GAP` allows absorbs that.
        _ => since + (60 - MIN_GAP) >= retry_after(tf),
    }
}

/// How long an alert without a judged bar, or a failed one, waits before the next try.
fn retry_after(tf: i64) -> i64 {
    tf.clamp(MIN_RETRY, MAX_RETRY)
}

/// Evaluate one alert: read its bars, build its series, judge the newest closed bar.
async fn evaluate(pool: &PgPool, cx: &AlertCx, a: &Alert) -> Result<()> {
    let tf = crate::histdata::timeframe_secs(&a.timeframe)
        .map_err(|e| anyhow!("{e}"))?;
    let probe = Probe::load(pool, a).await?;
    let bars = load_bars(pool, cx, a, tf, probe.bars_wanted()).await?;
    let closed = closed_bars(&bars, tf, OffsetDateTime::now_utc());
    if closed.len() < 2 {
        // Not enough history to say anything about a crossing yet.
        store::mark_checked(pool, a.id, None, None).await?;
        return Ok(());
    }
    let (lhs, rhs) = probe.pair_watched(&a.ticker, &closed).await;
    let n = lhs.len();
    let bar_ts = closed[n - 1].ts;
    let (Some(pl), Some(pr), Some(cur), Some(cr)) = (lhs[n - 2], rhs[n - 2], lhs[n - 1], rhs[n - 1]) else {
        store::mark_checked(pool, a.id, Some(bar_ts), None).await?;
        return Ok(());
    };

    // The same bar is judged once: the loop wakes more often than a bar closes.
    if a.last_bar_ts.is_some_and(|t| t >= bar_ts) {
        store::mark_checked(pool, a.id, None, Some(cur)).await?;
        return Ok(());
    }
    // The frequency is judged at the bar's close, which is when the crossing became a fact.
    let session = session_of(pool, a).await?;
    let at = bar_ts + time::Duration::seconds(tf);
    if !crossed(&a.op, pl - pr, cur - cr, 0.0) || !allows(a, a.last_fired_at, session.as_ref(), at)? {
        store::mark_checked(pool, a.id, Some(bar_ts), Some(cur)).await?;
        return Ok(());
    }
    // A trade on alert is priced from the instrument's close, whatever series crossed.
    let trigger = crate::alert_trade::Trigger { price: closed[n - 1].close, at };
    fire(pool, cx, a, &probe, (cur, cr), bar_ts, false, Some(trigger)).await
}

/// Bars for the window this alert needs: the catalog first, the provider only when the
/// catalog cannot reach the present.
pub(crate) async fn load_bars(
    pool: &PgPool,
    cx: &AlertCx,
    a: &Alert,
    tf: i64,
    want: i64,
) -> Result<Vec<Bar>> {
    let now = OffsetDateTime::now_utc();
    let from = now - time::Duration::seconds(tf * (want + 1));
    let dataset =
        hist_store::find_dataset(pool, &a.provider, &a.asset_type, &a.ticker, &a.timeframe).await?;
    let mut rows = match &dataset {
        Some(d) => hist_store::read_bars(pool, d.id, Some(from), Some(now), want + 2).await?,
        None => Vec::new(),
    };
    // Fresh enough to answer on its own: the stored series already covers the last closed bar.
    let fresh = rows
        .last()
        .is_some_and(|b| (now - b.ts).whole_seconds() < tf * 2);
    if !fresh {
        let connector = resolve_connector(pool, a).await?;
        let secrets = conn_store::load_creds(pool, &cx.cipher, connector).await?;
        let rest = crate::histdata::connector_for(&a.provider)?;
        let start = rows
            .last()
            .map(|b| b.ts)
            .unwrap_or(from)
            .max(now - time::Duration::seconds(tf * (MAX_BARS + 1)));
        let chunk = rest
            .fetch_chunk(&cx.http, &secrets, &a.ticker, &a.asset_type, &a.timeframe, start, now)
            .await
            .context("reading bars for the alert")?;
        rows.extend(chunk.bars);
        // Watching is not downloading: an alert stores nothing, exactly like the chart it was
        // drawn on.
        rows.sort_by_key(|b| b.ts);
        rows.dedup_by_key(|b| b.ts);
    }
    Ok(rows)
}

/// The connector this alert reads through: its own, else the oldest one the chart is granted
/// for that provider. Resolved at evaluation time, so a connector deleted or re-granted
/// yesterday does not orphan an alert.
pub(crate) async fn resolve_connector(pool: &PgPool, a: &Alert) -> Result<Uuid> {
    if let Some(id) = a.connector_id {
        if let Some(c) = conn_store::get(pool, id).await? {
            if c.allows("histviz") {
                return Ok(c.id);
            }
        }
    }
    conn_store::default_for_module(pool, &a.provider, "histviz")
        .await?
        .map(|c| c.id)
        .ok_or_else(|| {
            anyhow!(
                "no {} connector is granted to the chart, so this alert cannot read its bars",
                a.provider
            )
        })
}

/// Bars whose period has ended. A forming candle is not evidence.
pub(crate) fn closed_bars(bars: &[Bar], tf: i64, now: OffsetDateTime) -> Vec<Bar> {
    bars.iter()
        .filter(|b| b.ts.unix_timestamp() + tf <= now.unix_timestamp())
        .cloned()
        .collect()
}

/// What one alert compares, resolved: its two operands, the library rows they name, and the
/// history a script runs on. Built once per evaluation (background) or per watch (live).
pub(crate) struct Probe {
    pub lhs: Operand,
    pub rhs: Operand,
    dag: IndicatorDefs,
    scripts: ScriptDefs,
    history: Option<ScriptHistory>,
    /// Library names of the custom indicators read, for the message.
    names: HashMap<String, String>,
    lookback: usize,
}

impl Probe {
    /// Parse the alert's operands and load what they name from the library.
    pub(crate) async fn load(pool: &PgPool, a: &Alert) -> Result<Probe> {
        let lhs = operand(&a.lhs).context("reading the watched series")?;
        let rhs = operand(&a.rhs).context("reading what it is compared with")?;
        let mut p = Probe::resolve(pool, lhs, rhs, &a.inputs).await.map_err(|e| anyhow!(e))?;
        if !p.scripts.is_empty() {
            let meta = crate::instrument_meta_api::resolved_or_stored(pool, &a.provider, &a.asset_type, &a.ticker).await?;
            let syminfo = crate::script::syminfo_for(&a.provider, &a.asset_type, &a.ticker, meta.as_ref());
            p.history = Some(
                ScriptHistory::new(WarmBars::default(), syminfo, a.timeframe.clone()).with_budget(SCRIPT_CACHE_BYTES),
            );
        }
        Ok(p)
    }

    /// Check both operands and load the library rows they name. `Err` is the sentence the API
    /// answers and the alert list shows: an unknown indicator, a deleted row, a script plot
    /// that cannot drive an alert.
    pub(crate) async fn resolve(
        pool: &PgPool,
        lhs: Operand,
        rhs: Operand,
        inputs: &serde_json::Value,
    ) -> Result<Probe, String> {
        check_pair(&lhs, &rhs)?;
        let mut p = Probe::bare(lhs, rhs);
        for op in [p.lhs.clone(), p.rhs.clone()] {
            let Operand::CustomIndicator { id, plot } = &op else { continue };
            let uuid = Uuid::parse_str(id).map_err(|_| format!("'{id}' is not an indicator id"))?;
            let row = bt_store::get_indicator(pool, uuid)
                .await
                .map_err(|e| format!("{e:#}"))?
                .ok_or_else(|| "the indicator this alert watches no longer exists".to_string())?;
            p.names.insert(id.clone(), row.name.clone());
            if row.kind == "script" {
                let def = ScriptDef {
                    name: row.name.clone(),
                    source: row.source.unwrap_or_default(),
                    inputs: script_inputs(inputs, id)?,
                };
                if let Some(why) = bt_script::check_plot(&def, plot.as_deref()) {
                    return Err(why);
                }
                p.scripts.insert(id.clone(), def);
            } else {
                p.dag.insert(id.clone(), dag_def(row).map_err(|e| format!("{e:#}"))?);
            }
        }
        p.measure();
        Ok(p)
    }

    /// Two operands with nothing loaded from the library yet.
    fn bare(lhs: Operand, rhs: Operand) -> Probe {
        let mut p = Probe {
            lhs,
            rhs,
            dag: IndicatorDefs::new(),
            scripts: ScriptDefs::new(),
            history: None,
            names: HashMap::new(),
            lookback: 0,
        };
        p.measure();
        p
    }

    fn measure(&mut self) {
        let defs = Defs { dag: &self.dag, scripts: &self.scripts };
        self.lookback = backtest::op_lookback(&self.lhs, &defs).max(backtest::op_lookback(&self.rhs, &defs));
    }

    /// A script either side: judged on closed bars only, and over the whole window.
    pub(crate) fn reads_script(&self) -> bool {
        !self.scripts.is_empty()
    }

    /// A plain price against a fixed value: the live feed's last trade answers it on its own,
    /// with no bars kept.
    pub(crate) fn is_level(&self) -> bool {
        matches!(&self.lhs, Operand::Price { field } if *field != PriceField::Volume)
            && matches!(self.rhs, Operand::Const { .. })
    }

    /// Closed bars the series need: the lookback plus the bar a crossing is measured against.
    pub(crate) fn bars_wanted(&self) -> i64 {
        if self.reads_script() {
            return MAX_BARS;
        }
        (self.lookback as i64 + EXTRA_BARS).clamp(EXTRA_BARS, MAX_BARS)
    }

    /// Both series over `bars`, bar for bar.
    pub(crate) fn pair(&self, ticker: &str, bars: &[Bar]) -> (Vec<Option<f64>>, Vec<Option<f64>>) {
        let c = Cols::of(bars);
        let b = Bars {
            ticker,
            ts: &c.ts,
            open: &c.open,
            high: &c.high,
            low: &c.low,
            close: &c.close,
            volume: &c.volume,
            quotes: None,
            sub: None,
            script: self.history.as_ref(),
        };
        let defs = Defs { dag: &self.dag, scripts: &self.scripts };
        (backtest::resolve(&self.lhs, &b, &defs), backtest::resolve(&self.rhs, &b, &defs))
    }

    /// [`Probe::pair`], a script run queued on a watcher permit.
    pub(crate) async fn pair_watched(&self, ticker: &str, bars: &[Bar]) -> (Vec<Option<f64>>, Vec<Option<f64>>) {
        if self.reads_script() {
            crate::script::watched(|| self.pair(ticker, bars)).await
        } else {
            self.pair(ticker, bars)
        }
    }

    /// [`Probe::last`], a script run queued on a watcher permit.
    pub(crate) async fn last_watched(&self, ticker: &str, bars: &[Bar]) -> Option<(f64, f64)> {
        if self.reads_script() {
            crate::script::watched(|| self.last(ticker, bars)).await
        } else {
            self.last(ticker, bars)
        }
    }

    /// The newest value of each series, when both have one.
    pub(crate) fn last(&self, ticker: &str, bars: &[Bar]) -> Option<(f64, f64)> {
        if bars.is_empty() {
            return None;
        }
        let (l, r) = self.pair(ticker, bars);
        match (l.last().copied().flatten(), r.last().copied().flatten()) {
            (Some(l), Some(r)) if l.is_finite() && r.is_finite() => Some((l, r)),
            _ => None,
        }
    }

    /// An operand the way a person reads it: `close`, `SMA(9)`, `My script: Signal`, `5,000`.
    pub(crate) fn describe(&self, op: &Operand) -> String {
        match op {
            Operand::Price { field } => field.as_str().to_string(),
            Operand::Const { value } => fmt(*value),
            Operand::Indicator { indicator, period, fast, slow, mult, signal_period } => {
                let mut args: Vec<String> =
                    [*period, *fast, *slow, *signal_period].iter().filter(|v| **v > 0).map(|v| v.to_string()).collect();
                if *mult > 0.0 {
                    args.push(fmt(*mult));
                }
                let name = indicator.to_uppercase();
                if args.is_empty() { name } else { format!("{name}({})", args.join(", ")) }
            }
            Operand::CustomIndicator { id, plot } => {
                let name = self.names.get(id).cloned().unwrap_or_else(|| "indicator".into());
                match plot {
                    Some(p) => format!("{name}: {p}"),
                    None => name,
                }
            }
            Operand::Metric { metric, period } => match metric.as_str() {
                "change_pct" => format!("change over {} (%)", bars(*period)),
                "rel_volume" => format!("relative volume ({})", bars(*period)),
                "dd_from_high" => "drawdown from high (%)".into(),
                "up_from_low" => "rise from low (%)".into(),
                _ => metric.clone(),
            },
            Operand::Position { field } => field.clone(),
        }
    }
}

fn bars(n: usize) -> String {
    if n == 1 { "1 bar".into() } else { format!("{n} bars") }
}

/// Owned columns for one `Bars` view. Stamps are RFC 3339, what a script and the DAG read.
struct Cols {
    ts: Vec<String>,
    open: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    close: Vec<f64>,
    volume: Vec<f64>,
}

impl Cols {
    fn of(bars: &[Bar]) -> Cols {
        use time::format_description::well_known::Rfc3339;
        Cols {
            ts: bars.iter().map(|b| b.ts.format(&Rfc3339).unwrap_or_default()).collect(),
            open: bars.iter().map(|b| b.open).collect(),
            high: bars.iter().map(|b| b.high).collect(),
            low: bars.iter().map(|b| b.low).collect(),
            close: bars.iter().map(|b| b.close).collect(),
            volume: bars.iter().map(|b| b.volume).collect(),
        }
    }
}

pub(crate) fn operand(v: &serde_json::Value) -> Result<Operand> {
    serde_json::from_value(v.clone()).map_err(|e| anyhow!("not a series: {e}"))
}

/// Why this pair cannot be an alert, without the library.
pub(crate) fn check_pair(lhs: &Operand, rhs: &Operand) -> Result<(), String> {
    check_operand(lhs)?;
    check_operand(rhs)?;
    if matches!(lhs, Operand::Const { .. }) {
        return Err("the watched series cannot be a fixed value: put the value on the other side".into());
    }
    Ok(())
}

/// Why an operand cannot be watched, without the library: unknown names, nonsense periods,
/// and portfolio state, which only a DCA run has. Shared with the scanner and the journal lab.
pub(crate) use crate::backtest::operand_check::check_operand;

/// The input values the alert gives script `id`; none = the script's defaults.
fn script_inputs(inputs: &serde_json::Value, id: &str) -> Result<BTreeMap<String, RawInput>, String> {
    match inputs.get(id) {
        None | Some(serde_json::Value::Null) => Ok(BTreeMap::new()),
        Some(v) => serde_json::from_value(v.clone()).map_err(|e| format!("script inputs: {e}")),
    }
}

/// Did the series cross the level between these two closed bars? Two series are judged on
/// their difference against 0 (IEEE subtraction keeps the sign and is 0 only on equality, so
/// this is exactly "lhs crossed rhs").
///
/// A crossing, never a state: `above` needs the previous bar at or below the level. An alert
/// placed under the price would otherwise fire on the very next evaluation, which is a
/// notification about the past.
///
/// Except `close_above` and `close_below`, the two states the user asks for by name: the bar
/// closed past the level, whatever the bar before it did. They are judged on closed bars only
/// ([`at_close`]), so every mode fires them once per bar at most.
pub fn crossed(op: &str, prev: f64, cur: f64, level: f64) -> bool {
    let up = prev <= level && cur > level;
    let down = prev >= level && cur < level;
    match op {
        "above" => up,
        "below" => down,
        "close_above" => cur > level,
        "close_below" => cur < level,
        _ => up || down,
    }
}

/// A state of the closed bar, never judged on a forming one, live or not.
pub fn at_close(op: &str) -> bool {
    matches!(op, "close_above" | "close_below")
}

/// Whether the alert's frequency lets it fire at `at`, given when it last fired. `once`,
/// `every` and `bar_close` always may: the first disables itself on firing, and the last is
/// decided by *what* is evaluated, not by when.
pub(crate) fn allows(
    a: &Alert,
    last_fired: Option<OffsetDateTime>,
    session: Option<&Session>,
    at: OffsetDateTime,
) -> Result<bool> {
    Ok(match a.frequency.as_str() {
        "cooldown" => last_fired
            .map_or(true, |t| (at - t).whole_seconds() >= a.cooldown_secs as i64),
        "session" => {
            let s = session.ok_or_else(|| anyhow!("the session this alert fires in was deleted"))?;
            // Once per occurrence, and only inside one: "once per US session" says nothing
            // about the night.
            match market_sessions_api::occurrence(s, at) {
                Some((start, _)) => last_fired.map_or(true, |t| t < start),
                None => false,
            }
        }
        _ => true,
    })
}

/// The session a `session` alert fires in; `None` for every other frequency.
pub(crate) async fn session_of(pool: &PgPool, a: &Alert) -> Result<Option<Session>> {
    match (a.frequency.as_str(), a.session_id) {
        ("session", Some(id)) => Ok(sessions_store::get(pool, id).await?),
        _ => Ok(None),
    }
}

fn dag_def(row: bt_store::Indicator) -> Result<CustomIndicatorDef> {
    let Some(definition) = row.definition else {
        return Err(anyhow!("{} has no node graph", row.name));
    };
    let def: CustomIndicatorDef = serde_json::from_value(definition)
        .with_context(|| format!("reading the definition of {}", row.name))?;
    if let Some(why) = def.validate() {
        return Err(anyhow!("{} is not a valid indicator: {why}", row.name));
    }
    Ok(def)
}

/// Write the in-app notification, push it to the alert's channels, and record the fire.
/// `live` = judged on the forming bar, so the message does not claim the bar closed.
/// `trigger` = the instrument's price that fired, and when: what a trade on alert is priced
/// from, sent once the fire is recorded ([`crate::alert_trade::on_fire`]).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn fire(
    pool: &PgPool,
    cx: &AlertCx,
    a: &Alert,
    probe: &Probe,
    (value, other): (f64, f64),
    bar_ts: OffsetDateTime,
    live: bool,
    trigger: Option<crate::alert_trade::Trigger>,
) -> Result<()> {
    let subject = probe.describe(&probe.lhs);
    let target = probe.describe(&probe.rhs);
    let title = if a.name.trim().is_empty() {
        format!("{} {subject} {} {target}", a.ticker, verb(&a.op))
    } else {
        a.name.trim().to_string()
    };
    // A fixed value is its own reading; a series says where it was when it was crossed.
    let now = if matches!(probe.rhs, Operand::Const { .. }) {
        format!("Now {}.", fmt(value))
    } else {
        format!("Now {} against {}.", fmt(value), fmt(other))
    };
    let details = if live {
        format!(
            "{subject} {} {target} live, on the {} bar of {} opened at {}. {now}",
            verb(&a.op),
            a.timeframe,
            a.ticker,
            stamp(bar_ts),
        )
    } else {
        format!(
            "{subject} {} {target} on the {} bar of {} that closed at {}. {now}",
            verb(&a.op),
            a.timeframe,
            a.ticker,
            stamp(bar_ts),
        )
    };
    notify(pool, cx, a, title, details).await?;
    store::record_fire(pool, a.id, value, bar_ts, a.frequency != "once").await?;
    crate::alert_trade::on_fire(a, trigger).await;
    Ok(())
}

/// A time alert reached its instant: notify once, then disable it.
async fn fire_time(pool: &PgPool, cx: &AlertCx, a: &Alert) -> Result<()> {
    let at = a.fire_at.ok_or_else(|| anyhow!("this time alert has no time"))?;
    let title = if a.name.trim().is_empty() {
        format!("{} {}", a.ticker, stamp(at))
    } else {
        a.name.trim().to_string()
    };
    let details = format!("{} reached on {} ({}).", stamp(at), a.ticker, a.timeframe);
    notify(pool, cx, a, title, details).await?;
    store::record_time_fire(pool, a.id).await?;
    // No price fired a time alert: a trade on it reads the newest closed bar.
    crate::alert_trade::on_fire(a, None).await;
    Ok(())
}

/// The in-app notification, linked to the alert, pushed to the alert's channels.
pub(crate) async fn notify(pool: &PgPool, cx: &AlertCx, a: &Alert, title: String, details: String) -> Result<()> {
    let url = format!("/histviz?alert={}", a.id);

    sqlx::query(
        "INSERT INTO notifications (id, name, kind, linked_id, details, url, link_label) \
         VALUES ($1,$2,'chart',$3,$4,$5,$6)",
    )
    .bind(Uuid::new_v4())
    .bind(&title)
    .bind(a.id)
    .bind(&details)
    .bind(&url)
    .bind(&a.ticker)
    .execute(pool)
    .await?;

    // `"all"` is every channel the chart is granted; a list is the user's own narrowing, and
    // an empty one keeps the alert in the app.
    let channels: Option<Vec<Uuid>> = a
        .channels
        .as_array()
        .map(|v| v.iter().filter_map(|x| x.as_str()).filter_map(|s| Uuid::parse_str(s).ok()).collect());
    if channels.as_ref().is_some_and(|c| c.is_empty()) {
        return Ok(());
    }
    let notif = FiredNotification { name: title, details, url };
    notif_send::dispatch(pool, &cx.cipher, &cx.http, &notif, "histviz", channels.as_deref()).await;
    Ok(())
}

/// How an alert names itself in a notification: its own name, else what it watches.
pub(crate) fn title_of(a: &Alert) -> String {
    if a.name.trim().is_empty() {
        format!("{} {} alert", a.ticker, a.timeframe)
    } else {
        a.name.trim().to_string()
    }
}

/// The bar's own time, written the way a person reads it. RFC3339 would be exact and
/// unreadable in a push notification, which is where this line ends up.
fn stamp(ts: OffsetDateTime) -> String {
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02} UTC",
        ts.year(),
        ts.month() as u8,
        ts.day(),
        ts.hour(),
        ts.minute()
    )
}

fn verb(op: &str) -> &'static str {
    match op {
        "above" => "crossed above",
        "below" => "crossed below",
        "close_above" => "closed above",
        "close_below" => "closed below",
        _ => "crossed",
    }
}

/// A price the way the chart writes it: enough digits for a micro-cap, none wasted on an index.
fn fmt(v: f64) -> String {
    let abs = v.abs();
    let digits = if abs >= 1000.0 {
        2
    } else if abs >= 1.0 {
        4
    } else {
        8
    };
    let s = format!("{v:.digits$}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// One line of the reason, for the row the user reads.
pub(crate) fn short(msg: &str) -> String {
    let first = msg.split('\n').next().unwrap_or(msg).trim();
    let mut out: String = first.chars().take(200).collect();
    if first.chars().count() > 200 {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    fn bar(ts: OffsetDateTime, close: f64) -> Bar {
        Bar {
            ts,
            open: close,
            high: close,
            low: close,
            close,
            volume: 1.0,
            adj_open: None,
            adj_high: None,
            adj_low: None,
            adj_close: None,
        }
    }

    #[test]
    fn a_level_fires_on_the_crossing_and_not_on_the_state() {
        // Already above when the alert is placed: nothing crossed, nothing fires.
        assert!(!crossed("above", 105.0, 106.0, 100.0));
        // The bar that actually crosses it does.
        assert!(crossed("above", 99.0, 101.0, 100.0));
        // Touching the level from below counts as still below, so the next bar can cross it.
        assert!(crossed("above", 100.0, 101.0, 100.0));
        assert!(!crossed("above", 101.0, 99.0, 100.0));
        assert!(crossed("below", 101.0, 99.0, 100.0));
        assert!(crossed("crosses", 101.0, 99.0, 100.0));
        assert!(crossed("crosses", 99.0, 101.0, 100.0));
        assert!(!crossed("crosses", 98.0, 99.0, 100.0));
    }

    #[test]
    fn a_close_alert_fires_on_the_state() {
        // Already above: every bar that closes above fires, crossing or not.
        assert!(crossed("close_above", 105.0, 106.0, 100.0));
        assert!(crossed("close_above", 99.0, 101.0, 100.0));
        // On the level is not past it.
        assert!(!crossed("close_above", 99.0, 100.0, 100.0));
        assert!(!crossed("close_above", 101.0, 99.0, 100.0));
        assert!(crossed("close_below", 98.0, 97.0, 100.0));
        assert!(!crossed("close_below", 98.0, 100.0, 100.0));
        assert!(at_close("close_above") && at_close("close_below"));
        assert!(!at_close("above") && !at_close("crosses"));
    }

    #[test]
    fn the_message_says_which_bar_closed() {
        assert_eq!(stamp(datetime!(2026-09-06 09:22:00 UTC)), "2026-09-06 09:22 UTC");
    }

    #[test]
    fn a_forming_bar_is_not_evidence() {
        let now = datetime!(2026-09-06 12:30:20 UTC);
        let bars = vec![
            bar(datetime!(2026-09-06 12:00:00 UTC), 10.0),
            bar(datetime!(2026-09-06 12:15:00 UTC), 11.0),
            // The 12:30 bar of a 15m series has 14 minutes left to run.
            bar(datetime!(2026-09-06 12:30:00 UTC), 99.0),
        ];
        let closed = closed_bars(&bars, 900, now);
        assert_eq!(closed.len(), 2);
        assert_eq!(closed.last().unwrap().close, 11.0);
    }

    #[test]
    fn the_loop_wakes_just_after_each_minute() {
        let wait = |t: OffsetDateTime| until_next_wake(t).as_millis();
        assert_eq!(wait(datetime!(2026-09-06 12:00:59.5 UTC)), 3_500);
        assert_eq!(wait(datetime!(2026-09-06 12:01:03 UTC)), 60_000);
        assert_eq!(wait(datetime!(2026-09-06 12:01:03.2 UTC)), 59_800);
        // Woken a little early: the same minute's settle point, not the next one.
        assert_eq!(wait(datetime!(2026-09-06 12:01:01 UTC)), 2_000);
    }

    #[test]
    fn an_alert_is_read_once_the_next_bar_has_closed() {
        let mut a = alert();
        a.timeframe = "1m".into();
        // Judged the 12:00 bar (closed 12:01), at the 12:01:03 wake-up.
        a.last_bar_ts = Some(datetime!(2026-09-06 12:00 UTC));
        a.checked_at = Some(datetime!(2026-09-06 12:01:03.4 UTC));
        // The next wake-up is 59.6 s later: the 12:01 bar has closed, so it is read.
        assert!(is_due(&a, datetime!(2026-09-06 12:02:03 UTC)));
        // Not before it closed.
        assert!(!is_due(&a, datetime!(2026-09-06 12:01:59 UTC)));
        // The provider had not published it: still due at the next wake-up.
        a.checked_at = Some(datetime!(2026-09-06 12:02:03.3 UTC));
        assert!(is_due(&a, datetime!(2026-09-06 12:03:03 UTC)));

        // A daily alert costs one read a day, not one a quarter hour.
        a.timeframe = "1d".into();
        a.last_bar_ts = Some(datetime!(2026-09-05 00:00 UTC));
        a.checked_at = Some(datetime!(2026-09-06 00:00:03 UTC));
        assert!(!is_due(&a, datetime!(2026-09-06 18:00:03 UTC)));
        assert!(is_due(&a, datetime!(2026-09-07 00:00:03 UTC)));
    }

    #[test]
    fn a_new_or_failing_alert_is_retried_after_its_bar_length_bounded() {
        let mut a = alert();
        a.timeframe = "1m".into();
        assert!(is_due(&a, datetime!(2026-09-06 12:00 UTC)));
        // Read at 12:00:03.4; the next wake-up is 59.6 s later and still retries.
        a.checked_at = Some(datetime!(2026-09-06 12:00:03.4 UTC));
        assert!(is_due(&a, datetime!(2026-09-06 12:01:03 UTC)));
        a.timeframe = "1d".into();
        a.last_bar_ts = Some(datetime!(2026-09-05 00:00 UTC));
        a.last_error = "quota".into();
        assert!(!is_due(&a, datetime!(2026-09-06 12:10 UTC)));
        assert!(is_due(&a, datetime!(2026-09-06 12:15:03 UTC)));
        assert_eq!(retry_after(60), MIN_RETRY);
        assert_eq!(retry_after(86_400), MAX_RETRY);
    }

    fn alert() -> Alert {
        Alert {
            id: Uuid::new_v4(),
            name: String::new(),
            provider: "binance".into(),
            asset_type: "crypto".into(),
            ticker: "BTCUSDT".into(),
            timeframe: "1h".into(),
            connector_id: None,
            lhs: serde_json::json!({ "kind": "price", "field": "high" }),
            rhs: serde_json::json!({ "kind": "const", "value": 10.0 }),
            inputs: serde_json::json!({}),
            op: "above".into(),
            channels: serde_json::json!("all"),
            mode: "background".into(),
            frequency: "once".into(),
            cooldown_secs: 0,
            session_id: None,
            enabled: true,
            expires_at: None,
            fire_at: None,
            last_bar_ts: None,
            last_value: None,
            last_fired_at: None,
            fire_count: 0,
            checked_at: None,
            last_error: String::new(),
            created_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
        }
    }

    fn probe(lhs: serde_json::Value, rhs: serde_json::Value) -> Probe {
        Probe::bare(operand(&lhs).unwrap(), operand(&rhs).unwrap())
    }

    /// Hourly closes from 00:00, volume 1 unless given.
    fn closes(px: &[f64]) -> Vec<Bar> {
        px.iter()
            .enumerate()
            .map(|(i, &c)| bar(datetime!(2026-09-06 00:00 UTC) + time::Duration::hours(i as i64), c))
            .collect()
    }

    /// The bars on which the alert would fire, judged like `evaluate` judges the newest one.
    fn fires(p: &Probe, op: &str, bars: &[Bar]) -> Vec<usize> {
        let (l, r) = p.pair("X", bars);
        (1..bars.len())
            .filter(|&i| match (l[i - 1], r[i - 1], l[i], r[i]) {
                (Some(pl), Some(pr), Some(cl), Some(cr)) => crossed(op, pl - pr, cl - cr, 0.0),
                _ => false,
            })
            .collect()
    }

    #[test]
    fn a_price_alert_reads_the_field_it_was_given() {
        let mut b = bar(datetime!(2026-09-06 12:00:00 UTC), 10.0);
        b.high = 12.0;
        b.low = 8.0;
        b.volume = 7.0;
        let level = serde_json::json!({ "kind": "const", "value": 1.0 });
        for (field, want) in [("high", 12.0), ("low", 8.0), ("close", 10.0), ("volume", 7.0)] {
            let p = probe(serde_json::json!({ "kind": "price", "field": field }), level.clone());
            assert_eq!(p.last("X", &[b.clone()]), Some((want, 1.0)), "{field}");
        }
    }

    #[test]
    fn a_stored_alert_reads_back_as_its_two_operands() {
        let a = alert();
        assert!(matches!(operand(&a.lhs).unwrap(), Operand::Price { field: PriceField::High }));
        assert!(matches!(operand(&a.rhs).unwrap(), Operand::Const { value } if value == 10.0));
    }

    #[test]
    fn a_fast_average_crossing_a_slow_one_fires_on_the_crossing_bar_only() {
        // Falls, then turns up: SMA(2) goes under SMA(4) and comes back through it once.
        let bars = closes(&[10.0, 9.0, 8.0, 7.0, 6.0, 7.0, 9.0, 11.0, 12.0, 13.0]);
        let sma = |n: usize| serde_json::json!({ "kind": "indicator", "indicator": "sma", "period": n });
        let p = probe(sma(2), sma(4));
        let (l, r) = p.pair("X", &bars);
        let up = fires(&p, "above", &bars);
        assert_eq!(up.len(), 1, "{l:?} {r:?}");
        let i = up[0];
        assert!(l[i - 1].unwrap() <= r[i - 1].unwrap() && l[i].unwrap() > r[i].unwrap());
        // Already above afterwards: a state, not a crossing, so nothing more.
        assert!(fires(&p, "below", &bars).is_empty());
        assert_eq!(fires(&p, "crosses", &bars), up);
    }

    #[test]
    fn the_price_crossing_its_average_fires() {
        let bars = closes(&[10.0, 10.0, 10.0, 10.0, 12.0, 8.0]);
        let p = probe(
            serde_json::json!({ "kind": "price", "field": "close" }),
            serde_json::json!({ "kind": "indicator", "indicator": "sma", "period": 3 }),
        );
        // Bar 4: close 12 over an SMA of 10.67. Bar 5: close 8 under 10.
        assert_eq!(fires(&p, "above", &bars), vec![4]);
        assert_eq!(fires(&p, "below", &bars), vec![5]);
    }

    #[test]
    fn a_close_above_a_level_fires_on_every_bar_closing_past_it() {
        let bars = closes(&[99.0, 101.0, 102.0, 100.0, 103.0]);
        let p = probe(
            serde_json::json!({ "kind": "price", "field": "close" }),
            serde_json::json!({ "kind": "const", "value": 100.0 }),
        );
        assert_eq!(fires(&p, "above", &bars), vec![1, 4]);
        assert_eq!(fires(&p, "close_above", &bars), vec![1, 2, 4]);
        assert_eq!(fires(&p, "close_below", &bars), Vec::<usize>::new());
    }

    #[test]
    fn a_change_alert_compares_percent_over_n_bars() {
        let bars = closes(&[100.0, 101.0, 102.0, 106.0, 104.0]);
        let p = probe(
            serde_json::json!({ "kind": "metric", "metric": "change_pct", "period": 2 }),
            serde_json::json!({ "kind": "const", "value": 3.0 }),
        );
        let (l, _) = p.pair("X", &bars);
        assert_eq!(l[..2], [None, None]);
        assert!((l[3].unwrap() - (106.0 / 101.0 - 1.0) * 100.0).abs() < 1e-9);
        // +2 % at bar 2, +4.95 % at bar 3: crossed 3 % there and only there.
        assert_eq!(fires(&p, "above", &bars), vec![3]);
        assert_eq!(p.bars_wanted(), 2 + EXTRA_BARS);
    }

    #[test]
    fn a_volume_spike_fires_when_it_crosses_the_ratio() {
        let mut bars = closes(&[10.0; 6]);
        for (b, v) in bars.iter_mut().zip([100.0, 100.0, 100.0, 120.0, 400.0, 100.0]) {
            b.volume = v;
        }
        let p = probe(
            serde_json::json!({ "kind": "metric", "metric": "rel_volume", "period": 3 }),
            serde_json::json!({ "kind": "const", "value": 2.0 }),
        );
        assert_eq!(fires(&p, "above", &bars), vec![4]);
        assert!(!p.is_level());
    }

    #[test]
    fn only_a_plain_price_level_is_answered_by_the_last_trade() {
        let level = serde_json::json!({ "kind": "const", "value": 1.0 });
        assert!(probe(serde_json::json!({ "kind": "price", "field": "close" }), level.clone()).is_level());
        assert!(!probe(serde_json::json!({ "kind": "price", "field": "volume" }), level).is_level());
        let p = probe(
            serde_json::json!({ "kind": "price", "field": "close" }),
            serde_json::json!({ "kind": "indicator", "indicator": "ema", "period": 50 }),
        );
        assert!(!p.is_level());
        assert!(p.bars_wanted() >= 50);
    }

    #[test]
    fn a_pair_that_cannot_be_watched_is_refused_with_the_reason() {
        let ok = |l: serde_json::Value, r: serde_json::Value| check_pair(&operand(&l).unwrap(), &operand(&r).unwrap());
        let close = serde_json::json!({ "kind": "price", "field": "close" });
        let one = serde_json::json!({ "kind": "const", "value": 1.0 });
        assert!(ok(close.clone(), one.clone()).is_ok());
        assert!(ok(one.clone(), close.clone()).unwrap_err().contains("fixed value"));
        assert!(ok(serde_json::json!({ "kind": "indicator", "indicator": "nope" }), one.clone())
            .unwrap_err()
            .contains("unknown indicator"));
        assert!(ok(serde_json::json!({ "kind": "indicator", "indicator": "sma", "period": 5000 }), one.clone()).is_err());
        assert!(ok(serde_json::json!({ "kind": "metric", "metric": "change_pct", "period": 0 }), one.clone()).is_err());
        assert!(ok(serde_json::json!({ "kind": "metric", "metric": "nope" }), one.clone()).is_err());
        // An unknown price field no longer reads as a series at all.
        assert!(operand(&serde_json::json!({ "kind": "price", "field": "vwap" })).unwrap_err().to_string().contains("unknown price field"));
        assert!(ok(serde_json::json!({ "kind": "position", "field": "pnl_pct" }), one).is_err());
    }

    #[test]
    fn the_message_names_both_sides() {
        let p = probe(
            serde_json::json!({ "kind": "indicator", "indicator": "sma", "period": 9 }),
            serde_json::json!({ "kind": "indicator", "indicator": "bb_upper", "period": 20, "mult": 2.0 }),
        );
        assert_eq!(p.describe(&p.lhs), "SMA(9)");
        assert_eq!(p.describe(&p.rhs), "BB_UPPER(20, 2)");
        let p = probe(
            serde_json::json!({ "kind": "metric", "metric": "rel_volume", "period": 20 }),
            serde_json::json!({ "kind": "const", "value": 64000.5 }),
        );
        assert_eq!(p.describe(&p.lhs), "relative volume (20 bars)");
        assert_eq!(p.describe(&operand(&serde_json::json!({ "kind": "metric", "metric": "change_pct", "period": 1 })).unwrap()), "change over 1 bar (%)");
        assert_eq!(p.describe(&p.rhs), "64000.5");
    }

    const CROSS: &str = "//@version=6\nindicator(\"Cross\")\nlen = input.int(5, minval = 2)\nplot(ta.sma(close, len), \"fast\")\nplot(ta.sma(close, 20), \"slow\")";

    #[test]
    fn a_script_plot_crossing_another_fires_like_the_same_averages_built_in() {
        let px: Vec<f64> = (0..80).map(|i| 100.0 + (i as f64 * 0.25).sin() * 5.0).collect();
        let bars = closes(&px);
        let side = |plot: &str| serde_json::json!({ "kind": "custom_indicator", "id": "s1", "plot": plot });
        let mut p = probe(side("fast"), side("slow"));
        p.scripts.insert(
            "s1".into(),
            ScriptDef {
                name: "Cross".into(),
                source: CROSS.into(),
                inputs: BTreeMap::from([("len".to_string(), RawInput::Num(3.0))]),
            },
        );
        let syminfo = otw_script::input::SymInfo { ticker: "X".into(), tickerid: "T:X".into(), ..Default::default() };
        p.history = Some(ScriptHistory::new(WarmBars::default(), syminfo, "1h".into()));
        assert!(p.reads_script());
        assert_eq!(p.bars_wanted(), MAX_BARS);

        // The input given to the alert is the one the script runs with: fast = SMA(3).
        let (l, _) = p.pair("X", &bars);
        let sma3 = (px[37] + px[38] + px[39]) / 3.0;
        assert!((l[39].unwrap() - sma3).abs() < 1e-9);

        let sma = |n: usize| serde_json::json!({ "kind": "indicator", "indicator": "sma", "period": n });
        let builtin = probe(sma(3), sma(20));
        let want = fires(&builtin, "crosses", &bars);
        assert!(!want.is_empty());
        assert_eq!(fires(&p, "crosses", &bars), want);
    }

    #[test]
    fn a_script_plot_that_repaints_or_is_missing_cannot_drive_an_alert() {
        let def = |src: &str| ScriptDef { name: "S".into(), source: src.into(), inputs: BTreeMap::new() };
        assert_eq!(bt_script::check_plot(&def(CROSS), Some("fast")), None);
        assert!(bt_script::check_plot(&def(CROSS), Some("nope")).unwrap().contains("no plot"));
        let repaint = "//@version=6\nindicator(\"T\")\nplot(barstate.islast ? close : na, \"x\")";
        assert!(bt_script::check_plot(&def(repaint), Some("x")).unwrap().contains("cannot know yet"));
        let mut bad = def(CROSS);
        bad.inputs.insert("len".into(), RawInput::Num(1.0));
        assert!(bt_script::check_plot(&bad, Some("fast")).unwrap().contains("out of range"));
    }

    #[test]
    fn script_inputs_are_read_per_indicator() {
        let inputs = serde_json::json!({ "abc": { "len": 14, "src": "close", "on": true } });
        let got = script_inputs(&inputs, "abc").unwrap();
        assert_eq!(got.get("len"), Some(&RawInput::Num(14.0)));
        assert_eq!(got.get("on"), Some(&RawInput::Bool(true)));
        assert!(script_inputs(&inputs, "other").unwrap().is_empty());
        assert!(script_inputs(&serde_json::json!({ "abc": 3 }), "abc").is_err());
    }

    #[test]
    fn a_cooldown_holds_the_next_fire() {
        let mut a = alert();
        a.frequency = "cooldown".into();
        a.cooldown_secs = 600;
        let fired = datetime!(2026-09-06 12:00 UTC);
        assert!(allows(&a, None, None, fired).unwrap());
        assert!(!allows(&a, Some(fired), None, datetime!(2026-09-06 12:09 UTC)).unwrap());
        assert!(allows(&a, Some(fired), None, datetime!(2026-09-06 12:10 UTC)).unwrap());
    }

    #[test]
    fn a_session_alert_fires_once_per_occurrence_and_only_inside_it() {
        let mut a = alert();
        a.frequency = "session".into();
        let us = Session {
            id: Uuid::new_v4(),
            name: "US".into(),
            timezone: "America/New_York".into(),
            start_minute: 570,
            end_minute: 960,
            weekdays: 31,
            position: 0,
            created_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
        };
        let open = datetime!(2026-07-08 15:00 UTC);
        assert!(allows(&a, None, Some(&us), open).unwrap());
        // Fired this session already.
        assert!(!allows(&a, Some(open), Some(&us), datetime!(2026-07-08 19:00 UTC)).unwrap());
        // The next day's session is a new occurrence.
        assert!(allows(&a, Some(open), Some(&us), datetime!(2026-07-09 15:00 UTC)).unwrap());
        // Outside the session, nothing fires.
        assert!(!allows(&a, None, Some(&us), datetime!(2026-07-08 22:00 UTC)).unwrap());
        // A deleted session is an error the alert list shows, not silence.
        assert!(allows(&a, None, None, open).is_err());
    }
}
