//! Chart alerts on a live feed: the `app` and `background_live` modes.
//!
//! The `background` mode polls closed bars (`histviz_alerts`). These two ride the live hub
//! instead, so a crossing is seen on the tick rather than at the next poll:
//!
//! - **`background_live`** holds its feed for as long as the alert is armed, browser or not.
//! - **`app`** holds it only while the app is open somewhere: the page pings
//!   `/api/histviz/alerts/presence`, and once the pings stop for [`PRESENCE_TTL`] the feed is
//!   released like a chart pane that was closed.
//!
//! One task per alert, each holding its own hub subscription. The hub ref-counts them, so ten
//! alerts and a chart pane on BTCUSDT 1m still share one socket and one upstream stream. A
//! reconciler owns the tasks: it starts what is wanted, stops what is not, and restarts an
//! alert whose settings changed. A task that failed (a refused key, a plan without streaming)
//! is retried after [`RETRY_AFTER`], or at once when the user edits the alert.
//!
//! What a live alert judges depends on its frequency: `bar_close` waits for the bar to close
//! (the feed says so, no polling), every other frequency judges the price as it moves, which
//! is the point of choosing live. A price alert judges the last trade whatever its `source`;
//! `high` and `low` only differ from it on a closed bar.

use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Result};
use sqlx::PgPool;
use time::OffsetDateTime;
use tokio::sync::{broadcast::error::RecvError, Notify};
use tokio::task::JoinHandle;
use uuid::Uuid;

use otw_store::connectors as conn_store;
use otw_store::histdata::Bar;
use otw_store::histviz::{self as store, Alert};
use otw_store::market_sessions::Session;

use crate::backtest::custom::CustomIndicatorDef;
use crate::histviz_alerts::{self as alerts, AlertCx};
use crate::live::{
    self,
    hub::{LiveHub, LiveMsg, LiveTarget, Subscription},
    LiveEvent,
};

/// How long an `app` alert keeps its feed after the last ping. The page pings every 30 s; a
/// background tab can be throttled to one timer a minute, so this leaves room for two misses.
const PRESENCE_TTL: i64 = 150;
/// Reconciler period, on top of the pokes a create/update/delete sends.
const RECONCILE: Duration = Duration::from_secs(15);
/// Wait before the first pass, so a restart does not race the migrations and the connectors.
const WARMUP: Duration = Duration::from_secs(20);
/// A failed alert is tried again after this, unless the user edits it first.
const RETRY_AFTER: Duration = Duration::from_secs(300);
/// What the alert last saw is written at most this often: a tick feed must not become a
/// database write per trade.
const CHECKPOINT: Duration = Duration::from_secs(30);
/// An indicator is re-evaluated on the forming bar at most this often.
const INDICATOR_THROTTLE: Duration = Duration::from_secs(1);
/// Closed bars kept in memory for an indicator, past its own lookback.
const EXTRA_BARS: usize = 5;
const MAX_BARS: i64 = 400;

/// Handle kept in `AppState`: the page's presence and the reconciler's wake-up.
#[derive(Clone)]
pub struct LiveAlerts(Arc<Inner>);

struct Inner {
    /// Unix seconds of the last presence ping.
    presence: AtomicI64,
    poke: Notify,
}

impl LiveAlerts {
    /// The app is open. The first ping after a gap wakes the reconciler, so `app` alerts do
    /// not wait a whole period to start.
    pub fn touch(&self) {
        let now = OffsetDateTime::now_utc().unix_timestamp();
        let before = self.0.presence.swap(now, Ordering::Relaxed);
        if now - before >= PRESENCE_TTL {
            self.poke();
        }
    }

    /// Alerts changed: reconcile now rather than at the next period.
    pub fn poke(&self) {
        self.0.poke.notify_one();
    }

    fn present(&self) -> bool {
        let now = OffsetDateTime::now_utc().unix_timestamp();
        now - self.0.presence.load(Ordering::Relaxed) < PRESENCE_TTL
    }
}

struct Running {
    handle: JoinHandle<bool>,
    /// The settings it was started with; a different `updated_at` means restart.
    updated_at: OffsetDateTime,
}

pub fn spawn(pool: PgPool, hub: LiveHub, cx: AlertCx) -> LiveAlerts {
    let ctl = LiveAlerts(Arc::new(Inner { presence: AtomicI64::new(0), poke: Notify::new() }));
    let this = ctl.clone();
    let cx = Arc::new(cx);
    tokio::spawn(async move {
        tokio::time::sleep(WARMUP).await;
        let mut running: HashMap<Uuid, Running> = HashMap::new();
        let mut failed: HashMap<Uuid, (OffsetDateTime, Instant)> = HashMap::new();
        loop {
            if let Err(e) = reconcile(&pool, &hub, &cx, &this, &mut running, &mut failed).await {
                tracing::error!("live chart alerts: reconcile failed: {e:#}");
            }
            tokio::select! {
                _ = this.0.poke.notified() => {}
                _ = tokio::time::sleep(RECONCILE) => {}
            }
        }
    });
    ctl
}

async fn reconcile(
    pool: &PgPool,
    hub: &LiveHub,
    cx: &Arc<AlertCx>,
    ctl: &LiveAlerts,
    running: &mut HashMap<Uuid, Running>,
    failed: &mut HashMap<Uuid, (OffsetDateTime, Instant)>,
) -> Result<()> {
    let present = ctl.present();
    let wanted: HashMap<Uuid, Alert> = store::live_alerts(pool)
        .await?
        .into_iter()
        .filter(|a| a.mode == "background_live" || present)
        .map(|a| (a.id, a))
        .collect();

    // Reap the tasks that ended on their own: a one-shot that fired, or a failure.
    let done: Vec<Uuid> =
        running.iter().filter(|(_, r)| r.handle.is_finished()).map(|(id, _)| *id).collect();
    for id in done {
        let r = running.remove(&id).expect("listed above");
        if r.handle.await.unwrap_or(true) {
            failed.insert(id, (r.updated_at, Instant::now()));
        }
    }

    // Stop what is no longer wanted, or wanted with other settings.
    running.retain(|id, r| {
        let keep = wanted.get(id).is_some_and(|a| a.updated_at == r.updated_at);
        if !keep {
            r.handle.abort();
        }
        keep
    });
    // An edit clears a failure; so does time.
    failed.retain(|id, (updated_at, at)| {
        wanted.get(id).is_some_and(|a| a.updated_at == *updated_at) && at.elapsed() < RETRY_AFTER
    });

    for (id, a) in wanted {
        if running.contains_key(&id) || failed.contains_key(&id) {
            continue;
        }
        let updated_at = a.updated_at;
        let (pool, hub, cx) = (pool.clone(), hub.clone(), cx.clone());
        let handle = tokio::spawn(async move { watch(&pool, &hub, &cx, a).await });
        running.insert(id, Running { handle, updated_at });
    }
    Ok(())
}

/// Run one alert until it is stopped, fires its only shot, or fails. True = failed.
async fn watch(pool: &PgPool, hub: &LiveHub, cx: &AlertCx, a: Alert) -> bool {
    match run(pool, hub, cx, &a).await {
        Ok(()) => false,
        Err(e) => {
            let msg = alerts::short(&format!("{e:#}"));
            tracing::warn!("live chart alert {} ({}): {msg}", a.id, a.ticker);
            let _ = store::mark_error(pool, a.id, &msg).await;
            true
        }
    }
}

async fn run(pool: &PgPool, hub: &LiveHub, cx: &AlertCx, a: &Alert) -> Result<()> {
    if let Some(why) = live::stream_refusal(&a.provider, &a.asset_type, &a.timeframe) {
        bail!(why);
    }
    let tf = crate::histdata::timeframe_secs(&a.timeframe).map_err(|e| anyhow!("{e}"))?;
    let connector_id = alerts::resolve_connector(pool, a).await?;
    let connector = conn_store::get(pool, connector_id)
        .await?
        .ok_or_else(|| anyhow!("the connector this alert reads through was deleted"))?;
    let def = match (a.kind.as_str(), a.indicator_id) {
        ("indicator", Some(id)) => Some(alerts::indicator_def(pool, id).await?),
        ("indicator", None) => bail!("this alert has no indicator"),
        _ => None,
    };
    let session = alerts::session_of(pool, a).await?;

    // The closed bars the series needs before the first live event can be judged: an
    // indicator's lookback, or the previous close a bar-close crossing is measured against.
    let keep = def.as_ref().map(|d| d.lookback()).unwrap_or(0) + EXTRA_BARS;
    let closed = if def.is_some() || a.frequency == "bar_close" {
        let want = (keep as i64).min(MAX_BARS);
        let bars = alerts::load_bars(pool, cx, a, tf, want).await?;
        alerts::closed_bars(&bars, tf, OffsetDateTime::now_utc())
    } else {
        Vec::new()
    };

    let Subscription { mut rx, snapshot, status, guard: _guard } = hub.subscribe(LiveTarget {
        // Watching is not recording, exactly like the chart the alert was drawn on.
        dataset_id: None,
        connector_id,
        connector: connector.name,
        provider: a.provider.clone(),
        asset_type: a.asset_type.clone(),
        ticker: a.ticker.clone(),
        timeframe: a.timeframe.clone(),
    })?;
    if status.state == "stopped" {
        bail!(status.message.unwrap_or_else(|| "the live feed stopped".into()));
    }

    let mut w = Watch {
        pool,
        cx,
        a,
        def: def.as_ref(),
        session: session.as_ref(),
        keep,
        prev: None,
        closed,
        last_fired: a.last_fired_at,
        last_eval: None,
        last_mark: Instant::now(),
    };
    if a.frequency == "bar_close" {
        w.prev = w.series_value(&w.closed);
    }
    store::mark_checked(pool, a.id, None, None).await?;
    if let Some(e) = snapshot {
        if w.on_bar(&e).await? {
            return Ok(());
        }
    }
    loop {
        match rx.recv().await {
            Ok(LiveMsg::Bar(e)) => {
                if w.on_bar(&e).await? {
                    return Ok(());
                }
            }
            Ok(LiveMsg::Status(s)) if s.state == "stopped" => {
                bail!(s.message.unwrap_or_else(|| "the live feed stopped".into()));
            }
            Ok(LiveMsg::Status(_)) => {}
            // A burst the task could not keep up with: the next event carries the price.
            Err(RecvError::Lagged(_)) => {}
            Err(RecvError::Closed) => bail!("the live feed closed"),
        }
    }
}

/// One alert's state while it watches a feed.
struct Watch<'a> {
    pool: &'a PgPool,
    cx: &'a AlertCx,
    a: &'a Alert,
    def: Option<&'a CustomIndicatorDef>,
    session: Option<&'a Session>,
    /// Closed bars kept for the series.
    keep: usize,
    /// The value the next one is compared against. `None` until the first reading, so an
    /// alert armed above the price does not fire on its first tick.
    prev: Option<f64>,
    closed: Vec<Bar>,
    last_fired: Option<OffsetDateTime>,
    last_eval: Option<Instant>,
    last_mark: Instant,
}

impl Watch<'_> {
    fn series_value(&self, bars: &[Bar]) -> Option<f64> {
        if bars.is_empty() {
            return None;
        }
        alerts::series_for(self.a, self.def, bars).last().copied().flatten()
    }

    /// Judge one event. True = the alert is done (a one-shot that fired).
    async fn on_bar(&mut self, e: &LiveEvent) -> Result<bool> {
        if e.closed {
            let bar = e.as_bar();
            match self.closed.last() {
                Some(last) if last.ts == bar.ts => *self.closed.last_mut().expect("some") = bar,
                Some(last) if last.ts > bar.ts => {}
                _ => self.closed.push(bar),
            }
            let excess = self.closed.len().saturating_sub(self.keep.max(2));
            self.closed.drain(..excess);
        }

        let cur = if self.a.frequency == "bar_close" {
            if !e.closed {
                return Ok(false);
            }
            self.series_value(&self.closed)
        } else if self.def.is_some() {
            if !e.closed && self.last_eval.is_some_and(|t| t.elapsed() < INDICATOR_THROTTLE) {
                return Ok(false);
            }
            self.last_eval = Some(Instant::now());
            if e.closed {
                self.series_value(&self.closed)
            } else {
                let mut bars = self.closed.clone();
                bars.push(e.as_bar());
                self.series_value(&bars)
            }
        } else {
            Some(e.close)
        };
        let Some(cur) = cur.filter(|v| v.is_finite()) else {
            return Ok(false);
        };

        let now = OffsetDateTime::now_utc();
        let hit = self.prev.is_some_and(|p| alerts::crossed(&self.a.op, p, cur, self.a.value));
        self.prev = Some(cur);
        if hit && alerts::allows(self.a, self.last_fired, self.session, now)? {
            alerts::fire(self.pool, self.cx, self.a, cur, e.bar_ts, self.def, !e.closed).await?;
            self.last_fired = Some(now);
            self.last_mark = Instant::now();
            return Ok(self.a.frequency == "once");
        }
        if self.last_mark.elapsed() >= CHECKPOINT {
            self.last_mark = Instant::now();
            store::mark_checked(self.pool, self.a.id, e.closed.then_some(e.bar_ts), Some(cur))
                .await?;
        }
        Ok(false)
    }
}
