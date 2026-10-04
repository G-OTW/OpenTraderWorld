//! Paper sessions on the live price: stops, targets and limit orders triggered as the market
//! moves, not at the next candle close.
//!
//! A tick (see [`crate::paper`]) runs the engine at each close and leaves, in the session's
//! stats, the positions it holds with their levels (`open_positions`) and the limits resting
//! (`pending_orders`). This module watches those on the live hub, one task per session and
//! instrument, the way a chart alert does. Signals stay on closed candles; only the levels are
//! live. The trailing stop is one of those levels, ratcheted at each close.
//!
//! When a level is crossed, the fill is priced (on the live bid/ask when the strategy asked for
//! bid/ask, else on the live price with the spread and slippage of the settings), recorded in
//! `paper_live_fills` and alerted at once. The next tick replays it as given on its bar and does
//! not alert it again. A provider with no live price, a feed that cannot run, or the app being
//! down leaves the candle in charge, as before; the session is told once.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use time::OffsetDateTime;
use tokio::sync::{broadcast::error::RecvError, Notify};
use tokio::task::JoinHandle;
use uuid::Uuid;

use otw_store::connectors as conn_store;
use otw_store::histdata as hd;
use otw_store::paper as store;

use crate::backtest::{self, Settings};
use crate::live::hub::{LiveMsg, LiveTarget, Subscription};
use crate::{paper, AppState};

/// Reconciler period, on top of the poke every tick sends.
const RECONCILE: Duration = Duration::from_secs(10);
/// Wait before the first pass, so a restart does not race the migrations and the connectors.
const WARMUP: Duration = Duration::from_secs(20);

static POKE: Notify = Notify::const_new();

/// A tick finished: its levels changed, look again now.
pub fn poke() {
    POKE.notify_one();
}

/// One level to watch on one instrument.
#[derive(Clone, Debug, PartialEq)]
enum Watch {
    /// An open position: its stop and its resting sells above a long (buys below a short).
    Position {
        key: String,
        long: bool,
        avg: f64,
        qty: f64,
        fees: f64,
        entry_ts: String,
        stop: Option<(f64, String)>,
        limits: Vec<(f64, String)>,
    },
    /// A resting entry limit, and the size it is expected to fill with. An `anchor` limit has
    /// no price yet: it takes it from the first live price after its alert.
    Entry { long: bool, px: f64, qty: Option<f64>, anchor: Option<Anchor> },
    /// A market entry alerted at its signal's close: it fills on the first live price after
    /// that alert (the stream bar from `after` on), silently, then is watched as a position.
    Market { long: bool, after: OffsetDateTime, qty: f64, atr: Option<f64> },
}

/// A limit priced off the open that follows its alert: `offset` from it, a fraction when `pct`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Anchor {
    after: OffsetDateTime,
    pct: bool,
    offset: f64,
}

/// The position a live market fill opened, watched on the levels the engine gives that price.
fn filled_position(s: &Settings, f: &store::LiveFill, qty: f64, atr: Option<f64>) -> Watch {
    let long = f.direction == "long";
    let mult = if s.instrument.multiplier > 0.0 { s.instrument.multiplier } else { 1.0 };
    let (stop, tp) = backtest::fresh_levels(s, long, f.price, atr);
    Watch::Position {
        key: f.trade_key.clone(),
        long,
        avg: f.price,
        qty,
        fees: backtest::fee_for(&s.fees, qty, f.price, mult),
        entry_ts: f.at.format(&time::format_description::well_known::Rfc3339).unwrap_or_default(),
        stop: stop.map(|(x, r)| (x, r.to_string())),
        limits: tp.map(|x| (x, "take_profit".to_string())).into_iter().collect(),
    }
}

struct Running {
    signature: String,
    task: JoinHandle<()>,
}

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        tokio::time::sleep(WARMUP).await;
        let mut running: HashMap<(Uuid, String), Running> = HashMap::new();
        // Sessions told that their live price is not available, so it is said once.
        let told: Arc<Mutex<HashSet<(Uuid, String)>>> = Default::default();
        loop {
            if let Err(e) = reconcile(&state, &mut running, &told).await {
                tracing::warn!("paper live: {e:#}");
            }
            tokio::select! {
                _ = tokio::time::sleep(RECONCILE) => {}
                _ = POKE.notified() => {}
            }
        }
    });
}

/// Start what is wanted, stop what is not, restart a watch whose levels changed.
async fn reconcile(
    state: &AppState,
    running: &mut HashMap<(Uuid, String), Running>,
    told: &Arc<Mutex<HashSet<(Uuid, String)>>>,
) -> Result<()> {
    running.retain(|_, r| !r.task.is_finished());
    let mut wanted: HashSet<(Uuid, String)> = HashSet::new();
    for session in store::list_sessions(&state.pool).await? {
        if session.status != "active" {
            continue;
        }
        let Some(stats) = session.stats.as_ref() else { continue };
        let live = store::list_live_fills(&state.pool, session.id).await.unwrap_or_default();
        for &id in &session.dataset_ids {
            let Ok(Some(ds)) = hd::get_dataset(&state.pool, id).await else { continue };
            let Ok(settings) = serde_json::from_value::<Settings>(session.settings.clone()) else { continue };
            let stream_secs = stream_timeframe(&ds).and_then(|t| crate::histdata::timeframe_secs(&t).ok()).unwrap_or(60);
            let watches = watches_for(stats, &ds.ticker, &live, session.last_run_at, &settings, stream_secs);
            if watches.is_empty() {
                continue;
            }
            let key = (session.id, ds.ticker.clone());
            if let Some(why) = live_refusal(&ds) {
                let first = told.lock().map(|mut t| t.insert(key.clone())).unwrap_or(false);
                if first {
                    say(state, &session, &format!(
                        "{}: no live price ({why}). Stops, targets and limits are checked at candle close.",
                        ds.ticker
                    ))
                    .await;
                }
                continue;
            }
            wanted.insert(key.clone());
            let signature = format!("{:?}|{:?}", session.updated_at, watches);
            if running.get(&key).is_some_and(|r| r.signature == signature) {
                continue;
            }
            if let Some(old) = running.remove(&key) {
                old.task.abort();
            }
            let (st, sess, told2) = (state.clone(), session.clone(), told.clone());
            let task = tokio::spawn(async move {
                if let Err(e) = watch(&st, &sess, &ds, watches).await {
                    tracing::warn!("paper live {} {}: {e:#}", sess.name, ds.ticker);
                    let first = told2.lock().map(|mut t| t.insert((sess.id, ds.ticker.clone()))).unwrap_or(false);
                    if first {
                        say(&st, &sess, &format!(
                            "{}: the live price stopped ({e}). Stops, targets and limits are checked at candle close until it is back.",
                            ds.ticker
                        ))
                        .await;
                    }
                }
            });
            running.insert(key, Running { signature, task });
        }
    }
    running.retain(|k, r| {
        let keep = wanted.contains(k);
        if !keep {
            r.task.abort();
        }
        keep
    });
    Ok(())
}

/// Why this dataset's instrument cannot be watched live, if it cannot.
fn live_refusal(ds: &hd::Dataset) -> Option<String> {
    let tf = stream_timeframe(ds)?;
    crate::live::stream_refusal(&ds.provider, &ds.asset_type, &tf)
}

/// The timeframe to stream: only the price matters, so the finest the provider serves.
fn stream_timeframe(ds: &hd::Dataset) -> Option<String> {
    let cap = crate::histdata::capabilities().into_iter().find(|c| c.provider == ds.provider)?;
    Some(if cap.stream_timeframes.contains(&"1m") { "1m".into() } else { ds.timeframe.clone() })
}

/// What the last tick left to watch on `ticker`, minus what a live fill already took since.
/// An order alerted at a close is watched from the first live price after that alert
/// (`stream_secs`: the stream's bar, which that alert falls in).
fn watches_for(
    stats: &Value,
    ticker: &str,
    live: &[store::LiveFill],
    since: Option<OffsetDateTime>,
    settings: &Settings,
    stream_secs: i64,
) -> Vec<Watch> {
    let num = |v: &Value, k: &str| v.get(k).and_then(Value::as_f64);
    let text = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    let fresh = |f: &&store::LiveFill| since.is_none_or(|s| f.at >= s);
    let exited = |key: &str| live.iter().any(|f| f.kind == "exit" && f.trade_key == key);
    let mut out = Vec::new();
    for p in stats.get("open_positions").and_then(Value::as_array).into_iter().flatten() {
        if p.get("ticker").and_then(Value::as_str).is_some_and(|t| t != ticker) {
            continue;
        }
        let direction = text(p, "direction");
        let entry_ts = text(p, "entry_ts");
        let key = format!("{ticker}|{direction}|{entry_ts}");
        if exited(&key) {
            continue;
        }
        let mut limits = Vec::new();
        if let Some(tp) = num(p, "take_profit") {
            limits.push((tp, "take_profit".to_string()));
        }
        if let Some(x) = num(p, "exit_limit") {
            limits.push((x, text(p, "exit_limit_reason")));
        }
        let stop = num(p, "stop").map(|s| (s, text(p, "stop_reason")));
        if stop.is_none() && limits.is_empty() {
            continue;
        }
        out.push(Watch::Position {
            key,
            long: direction == "long",
            avg: num(p, "avg_price").unwrap_or(0.0),
            qty: num(p, "qty").unwrap_or(0.0),
            fees: num(p, "fees").unwrap_or(0.0),
            entry_ts,
            stop,
            limits,
        });
    }
    let filled_entry = live.iter().filter(fresh).any(|f| f.kind == "entry" && f.ticker == ticker);
    for o in stats.get("pending_orders").and_then(Value::as_array).into_iter().flatten() {
        if text(o, "kind") != "entry" || o.get("ticker").and_then(Value::as_str).is_some_and(|t| t != ticker) {
            continue;
        }
        let direction = text(o, "direction");
        let long = direction == "long";
        let signal = text(o, "signal_ts");
        // When the order was alerted, floored to the stream bar that alert falls in.
        let after = live
            .iter()
            .find(|f| f.kind == "signal" && f.ticker == ticker && f.direction == direction && f.bar_ts == signal)
            .and_then(|m| OffsetDateTime::from_unix_timestamp(m.at.unix_timestamp().div_euclid(stream_secs) * stream_secs).ok());
        if text(o, "order") == "market" {
            let Some(after) = after else { continue };
            let (qty, atr) = (num(o, "qty").unwrap_or(0.0), num(o, "atr"));
            let fill = live.iter().find(|f| {
                f.kind == "entry" && f.reason == "market" && f.ticker == ticker && f.direction == direction && f.at >= after
            });
            match fill {
                Some(f) if exited(&f.trade_key) => {}
                Some(f) => out.push(filled_position(settings, f, qty, atr)),
                None => out.push(Watch::Market { long, after, qty, atr }),
            }
            continue;
        }
        if filled_entry {
            continue;
        }
        let anchor = match num(o, "anchor_offset") {
            Some(offset) => {
                let Some(after) = after else { continue };
                Some(Anchor { after, pct: o.get("anchor_pct").and_then(Value::as_bool) == Some(true), offset })
            }
            None => None,
        };
        if let Some(px) = num(o, "price") {
            out.push(Watch::Entry { long, px, qty: num(o, "qty"), anchor });
        }
    }
    out
}

/// Record a line in the session's log, sent like any other event.
async fn say(state: &AppState, session: &store::Session, message: &str) {
    let title = format!("{}: live price", session.name);
    if let Err(e) = store::add_event(&state.pool, session.id, "run", &json!({}), &title, message, true).await {
        tracing::error!("paper live: recording a notice: {e:#}");
    }
}

/// Watch one instrument of one session until every level it holds has filled.
async fn watch(state: &AppState, session: &store::Session, ds: &hd::Dataset, mut watches: Vec<Watch>) -> Result<()> {
    let settings: Settings = serde_json::from_value(session.settings.clone())?;
    let conn = match conn_store::default_for_module(&state.pool, &ds.provider, "backtest").await? {
        Some(c) => c,
        None => conn_store::default_for(&state.pool, &ds.provider)
            .await?
            .ok_or_else(|| anyhow!("no {} connector to read the live price through", ds.provider))?,
    };
    let secrets = conn_store::load_creds(&state.pool, &state.cipher, conn.id).await?;
    let timeframe = stream_timeframe(ds).ok_or_else(|| anyhow!("{} has no live feed", ds.provider))?;
    let Subscription { mut rx, snapshot, status, guard: _guard } = state.live.subscribe(LiveTarget {
        // Watching is not recording.
        dataset_id: None,
        connector_id: conn.id,
        connector: conn.name.clone(),
        provider: ds.provider.clone(),
        asset_type: ds.asset_type.clone(),
        ticker: ds.ticker.clone(),
        timeframe,
    })?;
    if status.state == "stopped" {
        bail!(status.message.unwrap_or_else(|| "the live feed stopped".into()));
    }
    let pricer = Pricer::new(&settings, ds, secrets);
    // The extremes already seen on the forming bar: only what prints after the watch starts
    // can trigger, so the first event counts its last price alone.
    let mut seen: Option<(OffsetDateTime, f64, f64)> = snapshot.map(|e| (e.bar_ts, e.high, e.low));
    loop {
        let e = match rx.recv().await {
            Ok(LiveMsg::Bar(e)) => e,
            Ok(LiveMsg::Status(s)) if s.state == "stopped" => {
                bail!(s.message.unwrap_or_else(|| "the live feed stopped".into()))
            }
            Ok(LiveMsg::Status(_)) | Err(RecvError::Lagged(_)) => continue,
            Err(RecvError::Closed) => bail!("the live feed closed"),
        };
        // What traded since the last event: a new extreme of the same bar, or the last price.
        let (lo, hi) = match seen {
            Some((ts, h, l)) if ts == e.bar_ts => (
                if e.low < l { e.low } else { e.close },
                if e.high > h { e.high } else { e.close },
            ),
            Some(_) => (e.low, e.high),
            None => (e.close, e.close),
        };
        seen = Some((e.bar_ts, e.high, e.low));
        // A limit priced off the open takes its price from the first bar after its alert.
        for w in watches.iter_mut() {
            if let Watch::Entry { long, px, anchor, .. } = w {
                if let Some(a) = anchor.filter(|a| e.bar_ts >= a.after) {
                    let d = if a.pct { e.open * a.offset } else { a.offset };
                    *px = if *long { e.open - d } else { e.open + d };
                    *anchor = None;
                }
            }
        }
        let mut done = Vec::new();
        let mut filled = Vec::new();
        for (i, w) in watches.iter().enumerate() {
            if let Watch::Market { long, after, qty, atr } = w {
                // The first price after the alert: the open of the bar it came in.
                if e.bar_ts < *after {
                    continue;
                }
                let (px, quoted) = pricer.market(state, *long, e.open).await;
                match record_market(state, session, ds, *long, px, quoted).await? {
                    Some(f) => filled.push((i, filled_position(&settings, &f, *qty, *atr))),
                    None => done.push(i),
                }
                continue;
            }
            if let Some(fill) = pricer.check(state, w, lo, hi, e.close).await {
                if record(state, session, ds, w, fill).await? {
                    done.push(i);
                }
            }
        }
        for (i, w) in filled {
            watches[i] = w;
        }
        for i in done.into_iter().rev() {
            watches.remove(i);
        }
        if watches.is_empty() {
            return Ok(());
        }
    }
}

/// A live fill about to be recorded.
struct LiveFillPx {
    px: f64,
    reason: String,
    maker: bool,
    quoted: bool,
}

/// The settings' fill rules, applied to a live price.
struct Pricer {
    half_spread: f64,
    slippage: backtest::Slippage,
    use_quotes: bool,
    ticker: String,
    asset_type: String,
    provider: String,
    secrets: HashMap<String, String>,
}

impl Pricer {
    fn new(s: &Settings, ds: &hd::Dataset, secrets: HashMap<String, String>) -> Pricer {
        Pricer {
            half_spread: s.spread_pct / 2.0,
            slippage: s.slippage.clone(),
            use_quotes: s.execution.use_quotes,
            ticker: ds.ticker.clone(),
            asset_type: ds.asset_type.clone(),
            provider: ds.provider.clone(),
            secrets,
        }
    }

    /// The bid and ask now, when the strategy prices on them and the provider says.
    async fn quote(&self, state: &AppState) -> Option<(f64, f64)> {
        if !self.use_quotes {
            return None;
        }
        let c = crate::histdata::connector_for(&self.provider).ok()?;
        match c.best_quote(&state.http, &self.secrets, &self.ticker, &self.asset_type).await {
            Ok(q) => Some(q),
            Err(e) => {
                tracing::warn!("paper live: bid/ask for {}: {e:#}", self.ticker);
                None
            }
        }
    }

    /// A market buy (long) or sell at `open`: the live ask or bid when the strategy prices on
    /// them, else the open with the spread; slippage either way.
    async fn market(&self, state: &AppState, long: bool, open: f64) -> (f64, bool) {
        let slip = |px: f64| self.slippage.amount(px);
        if let Some((bid, ask)) = self.quote(state).await {
            return (if long { ask + slip(ask) } else { bid - slip(bid) }, true);
        }
        let b = if long { open * (1.0 + self.half_spread) } else { open * (1.0 - self.half_spread) };
        (if long { b + slip(b) } else { b - slip(b) }, false)
    }

    /// Whether `w` fills on a move that reached `lo`..`hi` (last price `last`), and at what.
    /// A stop is a market order: it fills on the side that deals, slippage included. A limit
    /// fills at its price once the side that deals reaches it, without slippage.
    async fn check(&self, state: &AppState, w: &Watch, lo: f64, hi: f64, last: f64) -> Option<LiveFillPx> {
        let slip = |px: f64| self.slippage.amount(px);
        match w {
            Watch::Position { long, stop, limits, .. } => {
                let long = *long;
                let stop_hit = stop.as_ref().filter(|(s, _)| if long { lo <= *s } else { hi >= *s });
                if let Some((s, reason)) = stop_hit {
                    // Through a gap, the price that traded is the best the stop can do.
                    let base = if long { s.min(last) } else { s.max(last) };
                    let (px, quoted) = match self.quote(state).await {
                        Some((bid, ask)) => (if long { bid - slip(bid) } else { ask + slip(ask) }, true),
                        None => {
                            let b = if long { base * (1.0 - self.half_spread) } else { base * (1.0 + self.half_spread) };
                            (if long { b - slip(b) } else { b + slip(b) }, false)
                        }
                    };
                    return Some(LiveFillPx { px, reason: reason.clone(), maker: false, quoted });
                }
                // The nearest resting limit the move reached, on the side that deals.
                let side = |x: f64| if long { x * (1.0 - self.half_spread) } else { x * (1.0 + self.half_spread) };
                let reached = |l: f64, ext: f64| if long { ext >= l } else { ext <= l };
                let mut hits: Vec<&(f64, String)> =
                    limits.iter().filter(|(l, _)| reached(*l, side(if long { hi } else { lo }))).collect();
                hits.sort_by(|a, b| if long { a.0.total_cmp(&b.0) } else { b.0.total_cmp(&a.0) });
                let (l, reason) = hits.first()?;
                if self.use_quotes {
                    if let Some((bid, ask)) = self.quote(state).await {
                        if !reached(*l, if long { bid } else { ask }) {
                            return None;
                        }
                        return Some(LiveFillPx { px: *l, reason: reason.clone(), maker: true, quoted: true });
                    }
                }
                Some(LiveFillPx { px: *l, reason: reason.clone(), maker: true, quoted: false })
            }
            Watch::Market { .. } | Watch::Entry { anchor: Some(_), .. } => None,
            Watch::Entry { long, px, .. } => {
                let long = *long;
                // A buy limit fills when the ask comes down to it, a sell limit when the bid
                // comes up to it.
                let side = if long { lo * (1.0 + self.half_spread) } else { hi * (1.0 - self.half_spread) };
                if !(if long { side <= *px } else { side >= *px }) {
                    return None;
                }
                let mut quoted = false;
                if self.use_quotes {
                    if let Some((bid, ask)) = self.quote(state).await {
                        if !(if long { ask <= *px } else { bid >= *px }) {
                            return None;
                        }
                        quoted = true;
                    }
                }
                Some(LiveFillPx { px: *px, reason: "limit".into(), maker: true, quoted })
            }
        }
    }
}

/// Store the live fill of a market entry, without an alert: the entry was announced at its
/// signal's close, and the next run puts this price in. False when it was already taken.
async fn record_market(
    state: &AppState,
    session: &store::Session,
    ds: &hd::Dataset,
    long: bool,
    px: f64,
    quoted: bool,
) -> Result<Option<store::LiveFill>> {
    let direction = if long { "long" } else { "short" };
    let at = OffsetDateTime::now_utc();
    let stamp = at.format(&time::format_description::well_known::Rfc3339).unwrap_or_default();
    let fill = store::LiveFill {
        id: Uuid::new_v4(),
        session_id: session.id,
        trade_key: format!("{}|{direction}|live:{stamp}", ds.ticker),
        ticker: ds.ticker.clone(),
        kind: "entry".into(),
        direction: direction.into(),
        reason: "market".into(),
        bar_ts: String::new(),
        price: px,
        maker: false,
        quoted,
        at,
    };
    Ok(store::add_live_fill(&state.pool, &fill).await?.then_some(fill))
}

/// Store a live fill and alert it now. False when that fill was already taken.
async fn record(state: &AppState, session: &store::Session, ds: &hd::Dataset, w: &Watch, f: LiveFillPx) -> Result<bool> {
    let now = OffsetDateTime::now_utc();
    let stamp = now.format(&time::format_description::well_known::Rfc3339).unwrap_or_default();
    let settings: Settings = serde_json::from_value(session.settings.clone())?;
    let mult = if settings.instrument.multiplier > 0.0 { settings.instrument.multiplier } else { 1.0 };
    let (fill, opened, closed) = match w {
        // Recorded by `record_market`, never priced through `check`.
        Watch::Market { .. } => return Ok(false),
        Watch::Position { key, long, avg, qty, fees, entry_ts, .. } => {
            let fee_rate = if f.maker {
                backtest::Fees { amount: settings.execution.maker_fee.unwrap_or(settings.fees.amount), ..settings.fees.clone() }
            } else {
                settings.fees.clone()
            };
            let exit_fee = backtest::fee_for(&fee_rate, *qty, f.px, mult);
            let dir = if *long { 1.0 } else { -1.0 };
            let pnl = dir * qty * (f.px - avg) * mult - fees - exit_fee;
            let notional = avg * qty.abs() * mult;
            let trade = json!({
                "ticker": ds.ticker,
                "entry_ts": entry_ts,
                "exit_ts": stamp,
                "entry_price": avg,
                "exit_price": f.px,
                "qty": qty,
                "direction": if *long { "long" } else { "short" },
                "exit_reason": f.reason,
                "pnl": pnl,
                "fees": fees + exit_fee,
                "return_pct": if notional != 0.0 { pnl / notional * 100.0 } else { 0.0 },
                "live": true,
            });
            let fill = store::LiveFill {
                id: Uuid::new_v4(),
                session_id: session.id,
                trade_key: key.clone(),
                ticker: ds.ticker.clone(),
                kind: "exit".into(),
                direction: if *long { "long".into() } else { "short".into() },
                reason: f.reason.clone(),
                bar_ts: String::new(),
                price: f.px,
                maker: f.maker,
                quoted: f.quoted,
                at: now,
            };
            (fill, Vec::new(), vec![trade])
        }
        Watch::Entry { long, qty, .. } => {
            let direction = if *long { "long" } else { "short" };
            let trade = json!({
                "ticker": ds.ticker,
                "entry_ts": stamp,
                "entry_price": f.px,
                "qty": qty,
                "direction": direction,
                "live": true,
            });
            let fill = store::LiveFill {
                id: Uuid::new_v4(),
                session_id: session.id,
                trade_key: format!("{}|{direction}|live:{stamp}", ds.ticker),
                ticker: ds.ticker.clone(),
                kind: "entry".into(),
                direction: direction.into(),
                reason: f.reason.clone(),
                bar_ts: String::new(),
                price: f.px,
                maker: true,
                quoted: f.quoted,
                at: now,
            };
            (fill, vec![trade], Vec::new())
        }
    };
    if !store::add_live_fill(&state.pool, &fill).await? {
        return Ok(false);
    }
    let tick = paper::Tick {
        skipped: false,
        bars: 0,
        inputs_hash: String::new(),
        stats: None,
        opened,
        closed,
        error: String::new(),
    };
    let book = paper::book(state, session).await;
    paper::record(state, session, &tick, &book).await?;
    paper::deliver(state, session, now, &book).await;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(t: &str) -> OffsetDateTime {
        OffsetDateTime::parse(t, &time::format_description::well_known::Rfc3339).unwrap()
    }

    fn fill(kind: &str, key: &str, direction: &str, reason: &str, bar_ts: &str, t: &str) -> store::LiveFill {
        store::LiveFill {
            id: Uuid::nil(),
            session_id: Uuid::nil(),
            trade_key: key.into(),
            ticker: "ETHUSDT".into(),
            kind: kind.into(),
            direction: direction.into(),
            reason: reason.into(),
            bar_ts: bar_ts.into(),
            price: 100.0,
            maker: false,
            quoted: false,
            at: at(t),
        }
    }

    /// Orders alerted at a close are watched from the first live price after the alert; a
    /// market one filled live is then a position on the levels of its fill, until it exits.
    #[test]
    fn orders_alerted_at_a_close_are_watched_from_the_alert() {
        let mut settings: Settings = serde_json::from_value(json!({
            "mode": "long",
            "long": { "stop_loss_pct": 0.01, "take_profit_pct": 0.02 },
            "sizing": { "mode": "fixed_qty", "qty": 1 }
        }))
        .unwrap();
        settings.spread_pct = 0.0;
        let stats = json!({ "pending_orders": [
            { "ticker": "ETHUSDT", "kind": "entry", "direction": "long", "order": "market", "price": 99.0,
              "bars_left": 1, "signal_ts": "2026-10-03T12:02:00Z", "qty": 2.0 },
            { "ticker": "ETHUSDT", "kind": "entry", "direction": "short", "order": "limit", "price": 101.0,
              "bars_left": 3, "signal_ts": "2026-10-03T12:02:00Z", "provisional": true,
              "anchor_offset": 0.001, "anchor_pct": true },
            { "ticker": "ETHUSDT", "kind": "entry", "direction": "short", "order": "limit", "price": 102.0, "bars_left": 2 }
        ]});
        let alerts = vec![
            fill("signal", "ETHUSDT|long|signal:2026-10-03T12:02:00Z", "long", "market", "2026-10-03T12:02:00Z", "2026-10-03T12:03:00.400Z"),
            fill("signal", "ETHUSDT|short|signal:2026-10-03T12:02:00Z", "short", "limit", "2026-10-03T12:02:00Z", "2026-10-03T12:03:00.400Z"),
        ];
        let after = at("2026-10-03T12:03:00Z");
        assert_eq!(
            watches_for(&stats, "ETHUSDT", &alerts, None, &settings, 60),
            vec![
                Watch::Market { long: true, after, qty: 2.0, atr: None },
                Watch::Entry { long: false, px: 101.0, qty: None, anchor: Some(Anchor { after, pct: true, offset: 0.001 }) },
                Watch::Entry { long: false, px: 102.0, qty: None, anchor: None },
            ]
        );

        let key = "ETHUSDT|long|live:2026-10-03T12:03:00.9Z";
        let mut live = alerts.clone();
        live.push(fill("entry", key, "long", "market", "", "2026-10-03T12:03:00.900Z"));
        let w = watches_for(&stats, "ETHUSDT", &live, None, &settings, 60);
        assert!(matches!(&w[0], Watch::Position { key: k, avg, stop: Some((s, _)), .. }
            if k == key && *avg == 100.0 && (s - 99.0).abs() < 1e-9));

        live.push(fill("exit", key, "long", "stop_loss", "", "2026-10-03T12:03:30Z"));
        let w = watches_for(&stats, "ETHUSDT", &live, None, &settings, 60);
        assert!(!w.iter().any(|w| matches!(w, Watch::Position { .. } | Watch::Market { .. })));
    }
}
