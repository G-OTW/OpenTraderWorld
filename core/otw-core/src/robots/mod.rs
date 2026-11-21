//! Trading robots: a backtested strategy running forward, tick by tick, on a broker account
//! or simulated, with its own capital policy and guards.
//!
//! The module: this file is the engine tick (stored as a paper session), [`api`] the routes
//! under `/api/robots`, [`exec`] the execution on a broker account, [`live`] the levels watched
//! on the live price, [`job`] the tick loop, [`sizing`] the capital layer, [`guards`] the rules
//! over the robot's own trades and [`dashboard`] the aggregates its page reads.
//!
//! **There is no second engine.** A tick reloads the session's datasets over its moving
//! window and calls the ordinary [`backtest::run_portfolio_forced`] on the strategy's signals
//! with the robot's money, calendar and venue ([`own`]), then diffs the trades it produced
//! against the ones already stored. A signal robot runs forward on what it executed: no entry
//! before its last bar (`filters.trade_from`), and the positions it holds carried in as it
//! took them (`ForcedKind::Hold`). What the trailing window re-finds on an earlier bar is
//! history: never recorded, announced or taken. No serialized simulator state to drift, no
//! parallel order logic to keep in sync.
//!
//! The diff is the database's answer, not a second bookkeeping pass: `upsert_trade` reports
//! whether the row was new, and a key that was open and is no longer produced as open is a
//! close. A round trip's identity is `ticker|direction|entry_ts`, which the engine reproduces
//! identically as long as the bars before it do not change.
//!
//! Two dials decide what leaves the machine, in this order: `inputs_hash` (nothing new to
//! simulate, so no engine run at all) and `notify_when` (`always` | `on_change` | `never`).
//! A fill then leaves on the tick that produced it: the alert's clock is the strategy's.
//! `cadence` is a third dial over a *different* message, the periodic summary of the book.
//! What is *not* delivered is still recorded: `robot_events` with `notified_at` NULL is the
//! offline inbox the page reads on the next login.

pub mod api;
pub mod dashboard;
pub mod exclusions;
pub mod exec;
pub mod guards;
pub mod job;
pub mod live;
pub mod own;
pub mod sizing;

use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};

use serde_json::{json, Map, Value};
use time::OffsetDateTime;
use uuid::Uuid;

use otw_store::automator::Schedule;
use otw_store::connectors as conn_store;
use otw_store::histdata as hd;
use otw_store::robots as store;
use otw_store::reminders::FiredNotification;
use otw_connector_model::Decimal;

use crate::automator::expr;
use crate::backtest::{self, Trade};
use crate::backtest_api::load_run_assets;
use crate::{notif_send, AppState};

/// Events kept per session. A paper session is long-lived; its log is a tail, not an archive.
const EVENT_KEEP: i64 = 500;
/// Bars fed to the engine when the session pins neither a trailing count nor a start.
const DEFAULT_WINDOW_BARS: i32 = 5_000;
/// Hard cap on what one tick loads, mirroring the backtest endpoint's own clamp.
const MAX_BARS: i64 = 200_000;
/// Seconds after a period boundary at which a session runs: none. A tick reads the bar that
/// just closed, so it fires the second it closes. The publication delay is absorbed by
/// [`top_up_dataset`], which re-asks for a moment rather than planning late for everyone. A
/// provider may still revise the bar after its close (Binance up to 4 s, IBKR 0.1 s, measured
/// 2026-11-16): the tick decides on what it reads at the close, and the next top-up reads that
/// bar again (`histdata::last_bar_unsettled`).
const PUBLISH_LAG_SECS: i64 = 0;
/// How long a top-up waits for the bar that just closed, and how often it re-asks. A provider
/// writes it within a second or so; asking twice costs one request, planning the whole session
/// a few seconds late costs every tick.
const PUBLISH_WAIT: std::time::Duration = std::time::Duration::from_millis(700);
const PUBLISH_TRIES: usize = 4;

/// How far back a top-up reaches on a dataset that has never been downloaded, or one left
/// behind for days. A session is a live feed, not a backfill: the rest is a download job.
const TOP_UP_BARS: i64 = 300;
/// Lower-timeframe windows one tick may download. A tick reads one new bar; the cap only bites
/// when a session first meets a history the cache does not hold yet, which then fills over a
/// few ticks instead of in one.
const PAPER_FETCH_MAX: usize = 60;

/// The module id used for channel grants and for `dispatch`: the module the user sees in the
/// switcher, so the channel screen labels a grant from the registry.
pub const MODULE: &str = "robots";
/// The data module a robot reads its bars, its top-ups and its live price through: a connector
/// granted to Trading Robots in the data broker.
pub const DATA_MODULE: &str = "robots";
/// Where a robot read its data before Robots was a data module: still asked second, so an
/// existing robot never loses its feed.
pub const LEGACY_DATA_MODULE: &str = "backtest";

/// The connector a robot reads `provider` through: one granted to Robots, else one granted to
/// Backtest, else the provider's default.
pub async fn data_connector(pool: &sqlx::PgPool, provider: &str) -> anyhow::Result<Option<conn_store::ConnectorRow>> {
    for module in [DATA_MODULE, LEGACY_DATA_MODULE] {
        if let Some(c) = conn_store::default_for_module(pool, provider, module).await? {
            return Ok(Some(c));
        }
    }
    conn_store::default_for(pool, provider).await
}

/// The source `ds` is read through for this robot: the connector picked for it when it was
/// created or edited, else its provider's default ([`data_connector`]). A picked connector
/// that was deleted or serves another provider falls back to the default too.
pub async fn data_connector_for(pool: &sqlx::PgPool, session: &store::Session, ds: &hd::Dataset) -> anyhow::Result<Option<conn_store::ConnectorRow>> {
    if let Some(id) = session.data_connector_of(ds.id) {
        if let Some(c) = conn_store::get(pool, id).await?.filter(|c| c.provider == ds.provider) {
            return Ok(Some(c));
        }
    }
    data_connector(pool, &ds.provider).await
}

/// Whether a connector may serve a robot's live price stream: granted to Robots (or, for a
/// robot from before, to Backtest).
pub fn streams_for_robots(c: &conn_store::ConnectorRow) -> bool {
    c.allows(DATA_MODULE) || c.allows(LEGACY_DATA_MODULE)
}

// ── Recurrence ────────────────────────────────────────────────────────────────

/// Adapt a session to the automator recurrence resolver. The two share a vocabulary on
/// purpose: one cron in the app, not two. `workflow_id` is nil here because the resolver
/// only ever reads the rule fields.
pub fn rule(s: &store::Session) -> Schedule {
    Schedule {
        id: s.id,
        workflow_id: Uuid::nil(),
        version_id: None,
        kind: s.kind.clone(),
        timezone: s.timezone.clone(),
        every_minutes: s.every_minutes,
        at_hour: s.at_hour,
        at_minute: s.at_minute,
        weekdays: s.weekdays,
        day_of_month: s.day_of_month,
        run_at: s.run_at,
        catch_up: false,
        active: s.status == "active",
        next_run_at: s.next_run_at,
        last_run_at: s.last_run_at,
        created_at: s.created_at,
        updated_at: s.updated_at,
    }
}

/// The status report's next occurrence after now: the recurrence rule, when the robot sends
/// one (`cadence = scheduled`) and is active. None for a rule that is exhausted.
pub fn next_report(s: &store::Session) -> Option<OffsetDateTime> {
    if s.cadence != "scheduled" || s.status != "active" {
        return None;
    }
    crate::automator::schedule::next_from_now(&rule(s)).map(|at| on_bar_grid(s, at))
}

/// The robot's timeframe: its datasets' (a basket shares one).
pub async fn timeframe_of(state: &AppState, dataset_ids: &[Uuid]) -> anyhow::Result<crate::timeframe::Timeframe> {
    let first = dataset_ids.first().ok_or_else(|| anyhow::anyhow!("the bot reads no dataset"))?;
    let ds = hd::get_dataset(&state.pool, *first).await?.ok_or_else(|| anyhow::anyhow!("the bot's dataset no longer exists"))?;
    crate::timeframe::Timeframe::parse(&ds.timeframe)
}

/// The next tick: the close of the bar in progress on the strategy's timeframe. A robot ticks
/// at every close of its own bars, never on a clock of its own.
pub fn next_close(tf: crate::timeframe::Timeframe, now: OffsetDateTime) -> OffsetDateTime {
    tf.closes_at(tf.bucket(now)) + time::Duration::seconds(PUBLISH_LAG_SECS)
}

/// The next tick of an active robot, None when paused.
pub async fn next_tick(state: &AppState, s: &store::Session) -> Option<OffsetDateTime> {
    if s.status != "active" {
        return None;
    }
    match timeframe_of(state, &s.dataset_ids).await {
        Ok(tf) => Some(next_close(tf, OffsetDateTime::now_utc())),
        Err(e) => {
            tracing::warn!("robots: \"{}\" has no timeframe to tick on: {e:#}", s.name);
            None
        }
    }
}

/// Why the robot takes no new entry now because of its exclusion periods, if it does not.
pub fn excluded(s: &store::Session, now: OffsetDateTime) -> Option<String> {
    let tz = crate::automator::schedule::timezone(&s.timezone).unwrap_or(time_tz::timezones::db::UTC);
    exclusions::Exclusions::of(&s.exclusions).holds(now, tz)
}

/// Whether the robot holds a position it took.
pub fn holds_position(book: &Book) -> bool {
    book.open.iter().any(|t| t.robot_qty.is_none_or(|q| !q.is_zero()))
}

/// Snap an interval occurrence onto the bar grid. The resolver counts from the last planned
/// instant, which is the second the session happened to be created: a one-minute session born
/// at 14:37:38 would read the market at :38 forever, a third of a bar past the close it is
/// waiting for. Bars close on the period boundary, so that is where a tick belongs: every
/// minute at :00, every 15 minutes at :00/:15/:30/:45, hourly on the hour. Daily and weekly
/// rules already carry a time of day and are left alone.
fn on_bar_grid(s: &store::Session, at: OffsetDateTime) -> OffsetDateTime {
    if s.kind != "interval" {
        return at;
    }
    let step = s.every_minutes.unwrap_or(0).max(1) as i64 * 60;
    let secs = at.unix_timestamp() - PUBLISH_LAG_SECS;
    let mut slot = secs.div_euclid(step) * step;
    if slot < secs {
        slot += step; // the first grid instant at or after the planned one
    }
    OffsetDateTime::from_unix_timestamp(slot + PUBLISH_LAG_SECS).unwrap_or(at)
}

// ── Tick ──────────────────────────────────────────────────────────────────────

/// What one tick did. `skipped` means the inputs were unchanged, so the engine never ran.
pub struct Tick {
    pub skipped: bool,
    pub bars: usize,
    pub inputs_hash: String,
    pub stats: Option<Value>,
    pub opened: Vec<Value>,
    pub closed: Vec<Value>,
    pub error: String,
}

impl Tick {
    fn failed(error: String) -> Self {
        Self {
            skipped: false,
            bars: 0,
            inputs_hash: String::new(),
            stats: None,
            opened: Vec::new(),
            closed: Vec::new(),
            error,
        }
    }

    pub fn changed(&self) -> bool {
        !self.opened.is_empty() || !self.closed.is_empty()
    }
}

/// A round trip's identity across ticks. The engine reproduces it verbatim as long as the
/// bars before the entry do not change, which is exactly when the position is the same one.
fn trade_key(t: &Trade) -> String {
    format!("{}|{}|{}", t.ticker, t.direction, t.entry_ts)
}

/// What makes a re-run worth doing: the strategy, the instruments, the window, the engine
/// semantics and how fresh the stored bars are. Unchanged means the simulation would return
/// the same numbers, so the tick costs one query instead of a full run.
fn inputs_hash(s: &store::Session, venue: &own::Venue, freshness: &[(Uuid, OffsetDateTime, i64)]) -> String {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut h = DefaultHasher::new();
    s.settings.to_string().hash(&mut h);
    // The robot's money and the venue's contract size the engine too (`own`).
    s.capital.to_string().hash(&mut h);
    s.on_broker().hash(&mut h);
    venue.key().hash(&mut h);
    s.window_bars.hash(&mut h);
    s.start_ts.map(|t| t.unix_timestamp()).hash(&mut h);
    backtest::ENGINE_VERSION.hash(&mut h);
    for (id, updated, bars) in freshness {
        id.hash(&mut h);
        updated.unix_timestamp().hash(&mut h);
        bars.hash(&mut h);
    }
    format!("{:016x}", h.finish())
}

/// Run one tick: simulate, diff, record. Writes events but sends nothing, so a manual run
/// from the page and a scheduled one behave identically. Delivery is [`deliver`].
pub async fn tick(state: &AppState, session: &store::Session, force: bool) -> Tick {
    if session.dataset_ids.is_empty() {
        return Tick::failed("this session has no dataset".into());
    }
    // The strategy's signals on the robot's money, calendar and venue (`own`).
    let (mut settings, venue) = match own::settings_of(state, session).await {
        Ok(s) => s,
        Err(e) => return Tick::failed(format!("stored settings are not runnable: {e}")),
    };
    if let Some(err) = settings.validate() {
        return Tick::failed(err);
    }

    // A forward-running session owns its data: pull whatever closed since the last tick
    // before deciding there is nothing new to simulate.
    top_up(state, session).await;

    // Freshness first: a session whose datasets have not moved has nothing to simulate.
    let mut freshness = Vec::with_capacity(session.dataset_ids.len());
    for &id in &session.dataset_ids {
        match hd::get_dataset(&state.pool, id).await {
            Ok(Some(ds)) => freshness.push((id, ds.last_updated, ds.bar_count)),
            Ok(None) => return Tick::failed("a dataset of this session no longer exists".into()),
            Err(e) => return Tick::failed(format!("reading the dataset: {e}")),
        }
    }
    let hash = inputs_hash(session, &venue, &freshness);
    if !force && hash == session.inputs_hash {
        return Tick {
            skipped: true,
            bars: session.bars.max(0) as usize,
            inputs_hash: hash,
            stats: None,
            opened: Vec::new(),
            closed: Vec::new(),
            error: String::new(),
        };
    }

    // The window trails the present unless the session pinned a start. `limit` applies to the
    // most recent bars, which is what a forward-running strategy wants.
    let limit = match session.window_bars {
        Some(n) if n > 0 => (n as i64).min(MAX_BARS),
        _ if session.start_ts.is_some() => MAX_BARS,
        _ => DEFAULT_WINDOW_BARS as i64,
    };
    let ex = settings.execution.clone();
    // The lower timeframe is read only under a bar that needs it, and downloaded then when the
    // strategy allows it, waiting a moment for a bar that just closed.
    let fetch = (ex.intrabar_fetch || ex.use_quotes).then(|| crate::intrabar::Fetch {
        max: PAPER_FETCH_MAX,
        progress: Default::default(),
    });
    let scripts = backtest::script::used(&settings);
    let assets = match load_run_assets(state, &session.dataset_ids, limit, session.start_ts, None, &ex, fetch, scripts)
        .await
    {
        Ok((a, _, _)) => a,
        Err(e) => return Tick::failed(e.message().to_string()),
    };
    // A hole in the window is filled without asking: the session owns its data, and the
    // top-up only reaches back a few hundred bars, so a pause or a downtime leaves one behind.
    // Until the download closes it, the engine trades around it.
    let mut data_gaps = Vec::new();
    for (a, &id) in assets.iter().zip(&session.dataset_ids) {
        let found = crate::data_gaps::of(&a.ts);
        let filled = match crate::data_gaps::statuses(&state.pool, id, &a.ticker, &found).await {
            Ok(g) => crate::data_gaps::fill(&state.pool, g).await,
            Err(e) => Err(e),
        };
        match filled {
            Ok(g) => data_gaps.extend(g),
            Err(e) => tracing::warn!("paper: filling the holes of {}: {e:#}", a.ticker),
        }
    }
    // Fills taken on the live price since they happened are replayed as given, on their bar,
    // and were alerted the moment they happened: the diff below does not alert them again.
    let live = store::list_live_fills(&state.pool, session.id).await.unwrap_or_default();
    let (mut forced, mut alerted, exits) = forced_fills(&assets, &live);
    // The last bar of each instrument. A running robot acts on it and on nothing before: an
    // entry or an exit the window shows on an earlier bar is history, re-found because the
    // window trails, never recorded, never news, never taken.
    let last_bar: HashMap<String, String> = assets.iter().filter_map(|a| a.ts.last().map(|t| (a.ticker.clone(), t.clone()))).collect();
    // A signal robot runs forward on what it executed: no entry before its last bar, and the
    // positions it holds carried in as they were taken (`carry`).
    let mut remap: HashMap<String, (String, String)> = HashMap::new();
    // The positions an announced reverse closes: said with the entry, not again at the close.
    let mut reversed: HashSet<(String, String)> = HashSet::new();
    // The entries the robot announced, recorded wherever they fill (a failed tick in between
    // leaves their open before the last bar).
    let mut announced: HashSet<String> = HashSet::new();
    if settings.kind == "signals" {
        settings.filters.trade_from = last_bar.values().max().cloned().unwrap_or_default();
        // Only the last close decides: what the robot decided at the closes before is replayed
        // as it decided it (`decided`), never read again from bars a provider may have revised.
        settings.filters.acts_from = settings.filters.trade_from.clone();
        let held = match store::held(&state.pool, session.id).await {
            Ok(h) => h,
            Err(e) => return Tick::failed(format!("reading the paper book: {e}")),
        };
        carry(&assets, &held, &mut forced, &mut remap);
        if !session.inputs_hash.is_empty() {
            (reversed, announced) = decided(&assets, session.stats.as_ref(), &live, &held, &mut forced);
        }
    }
    let mult = if settings.instrument.multiplier > 0.0 { settings.instrument.multiplier } else { 1.0 };
    // The engine is synchronous and CPU-bound: it runs on a blocking thread, never on a
    // runtime one, or a long session would stall every other request on this worker.
    let simulated = tokio::task::spawn_blocking(move || {
        let bars_total: usize = assets.iter().map(|a| a.ts.len()).sum();
        let bars: Vec<_> = assets.iter().map(|a| a.as_bars()).collect();
        let bar_refs: Vec<_> = bars.iter().collect();
        backtest::script::prepare(&settings, &bar_refs)?;
        Ok::<_, String>((backtest::run_portfolio_forced(&settings, &bar_refs, &forced), bars_total))
    })
    .await;
    let (result, bars_total) = match simulated {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => return Tick::failed(e),
        Err(e) => return Tick::failed(format!("the simulation did not finish: {e}")),
    };
    // A carried position the run had to start at its first bar is the robot's own trade.
    let ident = |t: &Trade| -> (String, String) {
        let k = trade_key(t);
        remap.get(&k).cloned().unwrap_or((k, t.entry_ts.clone()))
    };
    // A live exit was alerted when it happened: the trade it closed is the one the run closes
    // on that bar, for that reason.
    for t in &result.trades {
        if exits.contains(&(t.ticker.clone(), t.direction.clone(), t.exit_ts.clone(), t.exit_reason.clone())) {
            alerted.insert((ident(t).0, "exit".to_string()));
        }
    }
    // What the robot recorded: a trade it never met on a last bar is not its own.
    let known: HashSet<String> = match store::trade_keys(&state.pool, session.id).await {
        Ok(k) => k.into_iter().collect(),
        Err(e) => return Tick::failed(format!("reading the paper book: {e}")),
    };

    let previously_open: HashSet<String> = match store::open_keys(&state.pool, session.id).await {
        Ok(k) => k.into_iter().collect(),
        Err(e) => return Tick::failed(format!("reading the paper book: {e}")),
    };

    let mut opened = Vec::new();
    // Closes on the last bar, said once the robot's share is known: only what it took.
    let mut closing: Vec<(String, Value)> = Vec::new();
    // The round trips this run met for the first time on the last bar: the robot decides its
    // share of each.
    let mut fresh: Vec<(String, &Trade)> = Vec::new();
    // Every key this run produced: an open row the robot did not take and the run no longer
    // produces is pruned below. What it took stays.
    let mut seen: Vec<String> = Vec::with_capacity(result.trades.len());

    for t in &result.trades {
        // The engine closes whatever is still open at the last bar with reason "end". That
        // row is the live position, marked to that close, not a completed round trip.
        let is_open = t.exit_reason == "end";
        let (key, entry_ts) = ident(t);
        let on_last = |ts: &str| last_bar.get(&t.ticker).is_some_and(|l| ts >= l.as_str());
        let entered_now = on_last(&entry_ts);
        if !entered_now && !known.contains(&key) && !announced.contains(&key) {
            continue;
        }
        let mut payload = serde_json::to_value(t).unwrap_or_else(|_| json!({}));
        payload["entry_ts"] = json!(entry_ts);
        let was_open = previously_open.contains(&key);
        seen.push(key.clone());
        let inserted = match store::upsert_trade(
            &state.pool,
            session.id,
            &key,
            &t.ticker,
            &t.direction,
            &entry_ts,
            &t.exit_ts,
            is_open,
            t.pnl,
            t.fees,
            &payload,
        )
        .await
        {
            Ok(v) => v,
            Err(e) => return Tick::failed(format!("writing the paper book: {e}")),
        };
        if inserted {
            fresh.push((key.clone(), t));
        }
        // A brand-new row that is already closed is a bar that opened and closed a position at
        // once: it is reported as a close, since that is the event that happened.
        if inserted && is_open && entered_now && !alerted.contains(&(key.clone(), "entry".to_string())) {
            opened.push(payload.clone());
        }
        // Out at the last bar's open by a reverse announced with its entry: said then.
        let said_with_reverse = last_bar.get(&t.ticker) == Some(&t.exit_ts) && reversed.contains(&(t.ticker.clone(), t.direction.clone()));
        if !is_open && (was_open || inserted) && on_last(&t.exit_ts) && !said_with_reverse && !alerted.contains(&(key.clone(), "exit".to_string())) {
            closing.push((key.clone(), payload));
        }
    }

    // The market entries this close announces stay in the book until the run that fills them.
    for o in result.pending_orders.iter().filter(|o| o.kind == "entry" && o.order == "market") {
        if let Some(signal_ts) = o.signal_ts.as_deref() {
            seen.push(format!("{}|{}|pending:{signal_ts}", o.ticker, o.direction));
        }
    }
    if let Err(e) = store::prune_open_trades(&state.pool, session.id, &seen).await {
        tracing::error!("paper: pruning the book: {e:#}");
    }
    let cut: Vec<String> = result.excluded_trades.iter().map(|t| ident(t).0).collect();
    if let Err(e) = store::drop_trades(&state.pool, session.id, &cut).await {
        tracing::error!("paper: dropping excluded trades: {e:#}");
    }
    if let Err(e) = take(state, session, &fresh).await {
        return Tick::failed(format!("sizing the bot's trades: {e:#}"));
    }
    // A broker robot's entry its broker or its caps refused is not its own, whenever the
    // refusal was written (before this tick, or by the sync that follows it).
    if session.on_broker() {
        if let Err(e) = exec::untake_unsent(state, session.id).await {
            return Tick::failed(format!("writing the refused trades: {e:#}"));
        }
    }
    let keys: Vec<String> = closing.iter().map(|(k, _)| k.clone()).collect();
    let taken: HashSet<String> = match store::taken_of(&state.pool, session.id, &keys).await {
        Ok(k) => k.into_iter().collect(),
        Err(e) => return Tick::failed(format!("reading the paper book: {e}")),
    };
    let closed: Vec<Value> = closing.into_iter().filter(|(k, _)| taken.contains(k)).map(|(_, p)| p).collect();
    if let Err(e) = settle(state, session).await {
        return Tick::failed(format!("settling the bot's trades: {e:#}"));
    }

    // The entries this close decided: announced now, at the close's price for a market order
    // (the next open fills it and the next run puts that price in, without a second alert),
    // at its own price for a limit (its fill is confirmed when it happens).
    for o in result.pending_orders.iter().filter(|o| o.kind == "entry") {
        let Some(signal_ts) = o.signal_ts.as_deref() else { continue };
        let market = o.order == "market";
        let marker = store::LiveFill {
            id: Uuid::new_v4(),
            session_id: session.id,
            trade_key: format!("{}|{}|signal:{signal_ts}", o.ticker, o.direction),
            ticker: o.ticker.clone(),
            kind: "signal".into(),
            direction: o.direction.clone(),
            reason: o.order.clone(),
            bar_ts: signal_ts.to_string(),
            price: o.price,
            maker: !market,
            quoted: false,
            at: OffsetDateTime::now_utc(),
        };
        // Recorded once: a rerun of the same close announces nothing new.
        match store::add_live_fill(&state.pool, &marker).await {
            Ok(true) => {}
            Ok(false) => continue,
            Err(e) => return Tick::failed(format!("writing the paper book: {e}")),
        }
        let mut payload = json!({
            "ticker": o.ticker,
            "direction": o.direction,
            "order": o.order,
            "pending": true,
            "signal_ts": signal_ts,
            "entry_price": o.price,
            "qty": o.qty,
            "amount": o.amount,
            "bars_left": o.bars_left,
            "provisional": o.provisional,
        });
        // A market entry on the other side of the position held: one reverse, said once. The
        // position it closes leaves at the same open, about this close's price.
        if market {
            if let Some(p) = result.open_positions.iter().find(|p| p.ticker == o.ticker && p.direction != o.direction) {
                let sign = if p.direction == "long" { 1.0 } else { -1.0 };
                payload["reverses"] = json!({
                    "direction": p.direction,
                    "entry_ts": p.entry_ts,
                    "entry_price": p.avg_price,
                    "qty": p.qty,
                    "exit_price": o.price,
                    "pnl": sign * (o.price - p.avg_price) * p.qty * mult,
                });
            }
        }
        if market {
            // In the book until the next run replaces it with the filled trade.
            let key = format!("{}|{}|pending:{signal_ts}", o.ticker, o.direction);
            if let Err(e) = store::upsert_trade(
                &state.pool, session.id, &key, &o.ticker, &o.direction, signal_ts, "", true, 0.0, 0.0, &payload,
            )
            .await
            {
                return Tick::failed(format!("writing the paper book: {e}"));
            }
        }
        opened.push(payload);
    }

    Tick {
        skipped: false,
        bars: bars_total,
        inputs_hash: hash,
        stats: Some({
            let mut v = serde_json::to_value(&result.stats).unwrap_or_else(|_| json!({}));
            // The close this run decided at, per instrument: the next run replays what it decided.
            if let Some(m) = v.as_object_mut() {
                m.insert("closes".into(), json!(last_bar));
            }
            // Limit orders resting after the last bar, and the levels the open positions hold:
            // what the session has working now, and what the live watch triggers on.
            if let Some(m) = v.as_object_mut() {
                if !result.pending_orders.is_empty() {
                    m.insert("pending_orders".into(), json!(result.pending_orders));
                }
                if !result.open_positions.is_empty() {
                    m.insert("open_positions".into(), json!(result.open_positions));
                }
                if result.execution.forced_unmatched > 0 {
                    m.insert("live_unmatched".into(), json!(result.execution.forced_unmatched));
                }
                if !data_gaps.is_empty() {
                    m.insert("data_gaps".into(), json!(data_gaps));
                }
                if !result.excluded_trades.is_empty() {
                    m.insert("excluded_trades".into(), json!(result.excluded_trades));
                }
            }
            v
        }),
        opened,
        closed,
        error: String::new(),
    }
}

// ── The robot's share ────────────────────────────────────────────────────────

/// Size the round trips a tick met for the first time: the engine's size times the guard
/// scale in force, cut by the capital policy (`sizing`). A robot whose guards hold its entries,
/// or whose caps are reached, does not take the trade (`robot_qty` 0, the reason recorded and
/// said once on its channels). Only trades met on the last bar come here: what the window
/// shows earlier is history and is not recorded at all.
///
/// A robot on a broker account records the same share for its book, but the order it sends is
/// sized, refused and counted by [`exec`], against the venue's own step. An entry it did not
/// send (it was not armed when the entry's bar opened) is in its book as not taken: its guards
/// and its figures read what the robot did at the broker, not the engine alone.
pub(crate) async fn take(state: &AppState, session: &store::Session, fresh: &[(String, &Trade)]) -> anyhow::Result<()> {
    if fresh.is_empty() {
        return Ok(());
    }
    let now = OffsetDateTime::now_utc();
    let seeding = session.inputs_hash.is_empty();
    let on_broker = session.on_broker();
    let capital = sizing::Capital::of(&session.capital);
    let held = guards::State::of(&session.guard_state);
    let max_qty = exec::Execution::of(session).and_then(|e| e.max_position_qty);
    // A simulated robot takes what the venue would fill: its step and minimum (a broker robot's
    // are applied where the order is sized, `exec::plan`).
    let (step, min_qty) = if on_broker {
        (None, None)
    } else {
        let v = own::venue_of(state, session).await;
        let dec = |x: Option<f64>| x.filter(|x| *x > 0.0).and_then(|x| Decimal::try_from(x).ok()).map(|d| d.normalize());
        (dec(v.lot_step), dec(v.min_qty))
    };
    let fresh_open = fresh.iter().filter(|(_, t)| t.exit_reason == "end").count() as i64;
    let mut open_positions = (store::open_taken(&state.pool, session.id).await? - fresh_open).max(0);
    let mut orders_today = otw_store::robots_guards::day(&state.pool, session.id, now.date()).await?.orders_sent as i64;
    let mut ordered: Vec<&(String, &Trade)> = fresh.iter().collect();
    ordered.sort_by(|a, b| a.1.entry_ts.cmp(&b.1.entry_ts));
    for (key, t) in ordered {
        let scale = if seeding { Decimal::ONE } else { held.scale() };
        let excluded_now = if seeding { None } else { excluded(session, now) };
        let hold = if seeding || on_broker { None } else { held.holds(now).or_else(|| excluded_now.clone()) };
        let ask = sizing::Ask {
            ticker: t.ticker.clone(),
            engine_qty: t.qty,
            scale,
            step,
            min_qty,
            open_positions: if seeding || on_broker { 0 } else { open_positions },
            orders_today: if seeding || on_broker { 0 } else { orders_today },
            max_qty: if on_broker { None } else { max_qty },
            ..Default::default()
        };
        let unarmed = on_broker
            && !seeding
            && session.armed_at.is_none_or(|armed| {
                OffsetDateTime::parse(&t.entry_ts, &time::format_description::well_known::Rfc3339).is_ok_and(|entry| entry < armed)
            });
        if unarmed {
            let note = "not sent: the bot was not armed when this entry was decided".to_string();
            store::set_trade_size(&state.pool, session.id, key, Decimal::ZERO, scale, &note).await?;
            continue;
        }
        let (qty, note) = match hold {
            Some(why) => (Decimal::ZERO, format!("not taken: the bot is {why}")),
            None => match sizing::size(&capital, &ask) {
                sizing::Sized::Take { qty, .. } => (qty, String::new()),
                sizing::Sized::Refuse(_, why) => (Decimal::ZERO, why),
            },
        };
        store::set_trade_size(&state.pool, session.id, key, qty, scale, &note).await?;
        if seeding || on_broker {
            continue;
        }
        if qty > Decimal::ZERO {
            orders_today += 1;
            if t.exit_reason == "end" {
                open_positions += 1;
            }
            otw_store::robots_guards::bump_orders(&state.pool, session.id, now.date()).await?;
        } else if excluded_now.is_none() {
            // An exclusion is the user's own calendar: an entry it skips is not news.
            let title = format!("{}: entry not taken", session.name);
            let payload = json!({ "ticker": t.ticker, "direction": t.direction, "entry_ts": t.entry_ts, "refused": note });
            store::add_event(&state.pool, session.id, "refused", &payload, &title, &note, true).await?;
        }
    }
    Ok(())
}

/// What the guards hold, given the trades closed since they last looked and the time. Run
/// after every tick and on the guard cycle; writes the state, the timeline and the log.
/// Answers whether the robot's entries are held now.
pub async fn run_guards(state: &AppState, session: &store::Session) -> anyhow::Result<bool> {
    let now = OffsetDateTime::now_utc();
    let rules = guards::validate(&session.guards).unwrap_or_default();
    let before = guards::State::of(&session.guard_state);
    if rules.is_empty() && before == guards::State::default() {
        return Ok(false);
    }
    // The robot's own trades only: what it took, closed after it learned its book.
    let since = session.seeded_at.unwrap_or(session.created_at);
    let closed: Vec<guards::Closed> = store::list_trades(&state.pool, session.id, false)
        .await?
        .iter()
        .filter(|t| !t.open && t.robot_qty.is_none_or(|q| !q.is_zero()))
        .filter_map(|t| {
            let at = t.closed_at?;
            (at > since).then(|| guards::Closed { key: t.trade_key.clone(), closed_at: at, pnl: t.robot_pnl() })
        })
        .collect();
    let tz = crate::automator::schedule::timezone(&session.timezone);
    let (after, fired) = guards::evaluate(&rules, &closed, start_equity(session), &before, now, tz);
    if after != before {
        store::set_guard_state(&state.pool, session.id, &after.to_value()).await?;
    }
    for f in &fired {
        let kind = f.action.get("kind").and_then(Value::as_str).unwrap_or("");
        if matches!(kind, "restore" | "resume") {
            let what: &[&str] = match f.action.get("what").and_then(Value::as_str) {
                Some("pause") => &["pause"],
                Some("scale") => &["scale"],
                Some("halt") => &["halt"],
                _ => &["pause", "halt", "scale"],
            };
            otw_store::robots_guards::clear_open(&state.pool, session.id, what, f.at).await?;
        }
        otw_store::robots_guards::add_event(&state.pool, session.id, f.at, &f.rule, &f.action, &f.trigger).await?;
        let title = format!("{}: guard", session.name);
        let payload = json!({ "guard": f.action, "trigger": f.trigger });
        store::add_event(&state.pool, session.id, "guard", &payload, &title, &f.message, true).await?;
    }
    if !fired.is_empty() {
        let fresh = store::get_session(&state.pool, session.id).await?.unwrap_or_else(|| session.clone());
        deliver_events(state, &fresh).await;
        live::poke();
        if session.on_broker() {
            exec::poke();
        }
    }
    Ok(after.holds(now).is_some())
}

/// The equity the robot started from: its allocation when it has one, else the strategy's
/// starting capital (read from its own text, never through a float).
pub fn start_equity(session: &store::Session) -> Decimal {
    sizing::Capital::of(&session.capital).allocation.unwrap_or_else(|| {
        session.settings.get("starting_capital").map(|v| v.to_string().trim_matches('"').parse().unwrap_or_default()).unwrap_or_default()
    })
}

/// Crypto quote currencies: their money is kept at 8 decimals.
const CRYPTO_QUOTES: &[&str] = &["USDT", "USDC", "BUSD", "FDUSD", "TUSD", "DAI", "USDE", "BTC", "ETH", "BNB", "SOL", "XRP", "TRY"];
/// What a crypto pair's symbol can end with, longest first: `SOLUSDT` is quoted in USDT.
const PAIR_QUOTES: &[&str] = &["FDUSD", "USDT", "USDC", "BUSD", "TUSD", "USDE", "DAI", "USD", "EUR", "GBP", "TRY", "BTC", "ETH", "BNB"];

/// The currency a crypto pair is quoted in, read off its symbol, for a pair whose instrument
/// facts state none. `None` when no known quote ends it or nothing is left before it.
pub fn pair_quote(ticker: &str) -> Option<&'static str> {
    let t = ticker.trim().to_uppercase();
    PAIR_QUOTES.iter().copied().find(|q| t.len() > q.len() && t.ends_with(q))
}

/// Fiat currencies with no minor unit.
const NO_MINOR: &[&str] = &["JPY", "KRW", "VND", "CLP", "ISK", "HUF"];

/// How many decimals a robot's money is kept in on an instrument, from the currency its facts
/// state: 8 for a crypto quote, 0 for a currency with no minor unit, 2 for any other currency.
/// With no fact, 8 for a crypto instrument and 2 for the rest. `TRY` is listed as a quote
/// because Binance quotes in it to 8 decimals too.
pub fn money_dp(currency: Option<&str>, asset_type: &str) -> u32 {
    match currency.map(|c| c.trim().to_uppercase()).filter(|c| !c.is_empty()) {
        Some(c) if CRYPTO_QUOTES.contains(&c.as_str()) => 8,
        Some(c) if NO_MINOR.contains(&c.as_str()) => 0,
        Some(c) if c.len() == 3 && c.chars().all(|ch| ch.is_ascii_alphabetic()) => 2,
        Some(_) => 8,
        None if asset_type == "crypto" => 8,
        None => 2,
    }
}

/// The boundary where the engine's f64 becomes the robot's money: each closed trade not settled
/// yet gets its P&L and fees at the robot's size, in Decimal, rounded to the instrument
/// currency's precision. Run after every tick; a re-priced trade was unsettled by its upsert.
async fn settle(state: &AppState, session: &store::Session) -> anyhow::Result<()> {
    let rows = store::unsettled(&state.pool, session.id).await?;
    if rows.is_empty() {
        return Ok(());
    }
    let mut dps: HashMap<String, u32> = HashMap::new();
    for &id in &session.dataset_ids {
        let Some(ds) = hd::get_dataset(&state.pool, id).await? else { continue };
        let facts = crate::instrument_meta_api::resolved(&state.pool, &ds.provider, &ds.asset_type, &ds.ticker).await.ok().flatten();
        dps.insert(ds.ticker.clone(), money_dp(facts.as_ref().and_then(|m| m.currency.as_deref()), &ds.asset_type));
    }
    for t in rows {
        let dp = dps.get(&t.ticker).copied().unwrap_or(8);
        store::set_robot_money(&state.pool, session.id, &t.trade_key, t.money_at(t.pnl, dp), t.money_at(t.fees, dp)).await?;
    }
    Ok(())
}

/// Live exits as (ticker, direction, bar, reason): the trade the replay closes there.
type LiveExits = HashSet<(String, String, String, String)>;

/// The live fills of a session as the engine replays them: per asset (input order), on the bar
/// that was forming when they happened. Also the entries they already alerted, and the exits
/// to match against the run. A fill on an instrument or a time the window no longer holds is
/// dropped: history the replay does not cover.
/// The positions the robot holds, carried into the run as it took them: on their entry bar, or
/// on the run's first bar when the window no longer reaches back to it (that trade keeps its
/// own key and entry time through `remap`).
fn carry(
    assets: &[backtest::OwnedBars],
    held: &[store::Trade],
    forced: &mut [Vec<backtest::ForcedFill>],
    remap: &mut HashMap<String, (String, String)>,
) {
    for t in held {
        let Some(ai) = assets.iter().position(|a| a.ticker == t.ticker) else { continue };
        let a = &assets[ai];
        let Some(first) = a.ts.first() else { continue };
        let bar = match a.ts.iter().position(|ts| *ts == t.entry_ts) {
            Some(b) => b,
            None if t.entry_ts.as_str() < first.as_str() => {
                remap.insert(format!("{}|{}|{}", t.ticker, t.direction, first), (t.trade_key.clone(), t.entry_ts.clone()));
                0
            }
            None => continue,
        };
        let px = t.trade.get("entry_price").and_then(Value::as_f64).unwrap_or(0.0);
        let kind = backtest::ForcedKind::Hold { long: t.direction == "long", px, qty: t.engine_qty() };
        if let Some(f) = forced.get_mut(ai) {
            f.push(backtest::ForcedFill { bar, kind });
        }
    }
}

/// What the robot decided at its last close, sent at the open that follows: the market entries
/// it announced there (`signal` markers) and the market exits its last run planned
/// (`leaves_at_open`, or the reverse into an entry announced on the other side). The run replays
/// them as given ([`backtest::ForcedKind::Open`], [`backtest::ForcedKind::Leave`]) and reads no
/// signal from that close again: a provider may have revised the bar since, and the robot does
/// what it decided, never what a corrected bar would have decided. Returns the positions an
/// announced reverse closes, as (ticker, direction): their close was said with the entry; and
/// the keys of the entries replayed, which are the robot's own wherever they fill.
fn decided(
    assets: &[backtest::OwnedBars],
    stats: Option<&Value>,
    live: &[store::LiveFill],
    held: &[store::Trade],
    forced: &mut [Vec<backtest::ForcedFill>],
) -> (HashSet<(String, String)>, HashSet<String>) {
    let mut reversed = HashSet::new();
    let mut keys = HashSet::new();
    let text = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    let list = |k: &str| stats.and_then(|s| s.get(k)).and_then(Value::as_array).cloned().unwrap_or_default();
    let (pending, open) = (list("pending_orders"), list("open_positions"));
    let mine = |v: &Value, ticker: &str| v.get("ticker").and_then(Value::as_str).is_none_or(|t| t == ticker);
    for (ai, a) in assets.iter().enumerate() {
        // The close the last run decided at: stamped with it, else the bar before the last.
        let close = stats
            .and_then(|s| s.get("closes"))
            .and_then(|c| c.get(&a.ticker))
            .and_then(Value::as_str)
            .map(String::from)
            .or_else(|| a.ts.len().checked_sub(2).map(|i| a.ts[i].clone()));
        let Some(close) = close else { continue };
        let Some(k) = a.ts.iter().position(|t| *t == close) else { continue };
        let bar = k + 1;
        if bar >= a.ts.len() {
            continue;
        }
        let Some(out) = forced.get_mut(ai) else { continue };
        // The market entries announced at that close.
        let mut opened: Vec<bool> = Vec::new();
        for m in live.iter().filter(|f| f.kind == "signal" && f.reason == "market" && f.ticker == a.ticker && f.bar_ts == close) {
            let long = m.direction == "long";
            if opened.contains(&long) {
                continue;
            }
            let qty = pending
                .iter()
                .find(|o| mine(o, &a.ticker) && text(o, "direction") == m.direction && text(o, "order") == "market" && text(o, "signal_ts") == close)
                .and_then(|o| o.get("qty"))
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            opened.push(long);
            keys.insert(format!("{}|{}|{}", a.ticker, m.direction, a.ts[bar]));
            out.push(backtest::ForcedFill { bar, kind: backtest::ForcedKind::Open { long, qty } });
        }
        // A position held through that close leaves at this open when the run planned it, or
        // when the entry announced there is on the other side (a reverse).
        for t in held.iter().filter(|t| t.ticker == a.ticker && t.entry_ts <= close) {
            let long = t.direction == "long";
            let planned = open
                .iter()
                .find(|p| mine(p, &a.ticker) && text(p, "direction") == t.direction && p.get("leaves_at_open").and_then(Value::as_bool) == Some(true))
                .map(|p| p.get("leaves_reason").and_then(Value::as_str).unwrap_or("exit_signal").to_string());
            let reverse = opened.contains(&!long);
            let Some(reason) = planned.or_else(|| reverse.then(|| "reverse".to_string())) else { continue };
            if reverse {
                reversed.insert((t.ticker.clone(), t.direction.clone()));
            }
            out.push(backtest::ForcedFill { bar, kind: backtest::ForcedKind::Leave { reason } });
        }
    }
    (reversed, keys)
}

fn forced_fills(
    assets: &[backtest::OwnedBars],
    live: &[store::LiveFill],
) -> (Vec<Vec<backtest::ForcedFill>>, HashSet<(String, String)>, LiveExits) {
    use time::format_description::well_known::Rfc3339;
    let mut alerted = HashSet::new();
    let mut exits = LiveExits::new();
    let per_asset = assets
        .iter()
        .map(|a| {
            let stamps: Vec<Option<OffsetDateTime>> =
                a.ts.iter().map(|t| OffsetDateTime::parse(t, &Rfc3339).ok()).collect();
            // Where the last stored bar ends: a fill after it belongs to a bar not stored yet,
            // which the next run replays once it is.
            let end = match stamps.as_slice() {
                [.., Some(p), Some(l)] => Some(*l + (*l - *p)),
                _ => None,
            };
            live.iter()
                .filter(|f| f.ticker == a.ticker)
                .filter_map(|f| {
                    if f.kind == "signal" {
                        // An entry announced at its signal's close: the trade the next bar
                        // opens was alerted then (a market order; a limit confirms its fill).
                        if f.reason == "market" {
                            if let Some(i) = a.ts.iter().position(|t| *t == f.bar_ts) {
                                if let Some(next) = a.ts.get(i + 1) {
                                    alerted.insert((format!("{}|{}|{}", f.ticker, f.direction, next), "entry".to_string()));
                                }
                            }
                        }
                        return None;
                    }
                    if end.is_some_and(|e| f.at >= e) {
                        return None;
                    }
                    let bar = stamps.iter().rposition(|t| t.is_some_and(|t| t <= f.at))?;
                    let kind = match f.kind.as_str() {
                        "entry" => {
                            alerted.insert((format!("{}|{}|{}", f.ticker, f.direction, a.ts[bar]), "entry".to_string()));
                            backtest::ForcedKind::Entry { long: f.direction == "long", px: f.price }
                        }
                        _ => {
                            exits.insert((f.ticker.clone(), f.direction.clone(), a.ts[bar].clone(), f.reason.clone()));
                            backtest::ForcedKind::Exit { px: f.price, reason: f.reason.clone(), maker: f.maker }
                        }
                    };
                    Some(backtest::ForcedFill { bar, kind })
                })
                .collect()
        })
        .collect();
    (per_asset, alerted, exits)
}

// ── Feeding the datasets ──────────────────────────────────────────────────────

/// Pull the bars that closed since the last tick into the session's own datasets. Best
/// effort by design: a provider that is down or a symbol that has stopped trading is not a
/// broken session, so a failure is logged and the tick runs on what is stored.
async fn top_up(state: &AppState, session: &store::Session) {
    if session.feed != "hist" {
        return;
    }
    let now = OffsetDateTime::now_utc();
    for &id in &session.dataset_ids {
        let ds = match hd::get_dataset(&state.pool, id).await {
            Ok(Some(d)) => d,
            Ok(None) => continue,
            Err(e) => {
                tracing::warn!("paper: reading a dataset to top up: {e:#}");
                continue;
            }
        };
        if let Err(e) = top_up_dataset(state, session, &ds, now).await {
            tracing::warn!("paper: topping up {} {}: {e:#}", ds.ticker, ds.timeframe);
        }
    }
}

/// The period each dataset was last topped up for: a robot due on the same dataset in the same
/// period reads what the first one fetched instead of asking the provider again.
static TOPPED: LazyLock<Mutex<HashMap<Uuid, OffsetDateTime>>> = LazyLock::new(Default::default);
/// One top-up at a time per dataset: robots tick side by side, and a second robot on the same
/// dataset waits for the first one's fetch to be written instead of reading before it.
static TOPPING: LazyLock<Mutex<HashMap<Uuid, std::sync::Arc<tokio::sync::Mutex<()>>>>> = LazyLock::new(Default::default);

/// One dataset, one fetch: from the bar after the last stored one to the start of the period
/// in progress.
///
/// **The running period is never stored.** Providers stamp a bar at its *open*, so the answer
/// to a request made at 16:04:00 carries the 16:03 bar (whole) and, often, the 16:04 one that
/// opened a millisecond ago. The window ends at the boundary and anything stamped on or after
/// it is dropped: what is kept is the last *closed* bar, never the one still moving.
///
/// **The bar that just closed is waited for, not skipped.** A tick fires the second the period
/// ends, which is sometimes a moment before the provider has written that bar. Rather than
/// planning every session seconds late, the fetch re-asks a few times over ~2 s, and only
/// while the dataset was actually producing bars until then: a closed market has nothing to
/// wait for and must not pay for it.
///
/// **A missing bar is not an error.** No trade in the minute means some providers publish
/// nothing at all: the fetch comes back empty, nothing is written, the dataset's stamp does
/// not move and the tick that follows finds its inputs unchanged and skips. Nothing is
/// invented, and the next top-up asks for that window again.
///
/// Duplicates cost nothing either: the window starts *after* the last stored bar, and
/// `write_bars` upserts on `(dataset_id, ts)`, so a bar sent twice is rewritten, never
/// doubled.
async fn top_up_dataset(
    state: &AppState,
    session: &store::Session,
    ds: &hd::Dataset,
    now: OffsetDateTime,
) -> anyhow::Result<()> {
    let tf = crate::histdata::timeframe_secs(&ds.timeframe)?;
    // The period in progress starts on the timeframe's own grid: the ISO Monday for a week, not
    // the epoch's Thursday. Its bar is not closed until `closes_at` has passed.
    let grid = crate::timeframe::Timeframe::parse(&ds.timeframe)?;
    let running = grid.bucket(now);
    let last_closed = grid.bucket(running - time::Duration::seconds(1));
    let floor = running - time::Duration::seconds(tf * TOP_UP_BARS);
    let from = match ds.range_to {
        // Written before its period ended: a fragment, read again.
        Some(last) if crate::histdata::last_bar_unsettled(&ds.timeframe, last, ds.last_updated) => last.max(floor),
        Some(last) => (last + time::Duration::seconds(1)).max(floor),
        None => floor,
    };
    if from >= running {
        return Ok(());
    }
    let lock = match TOPPING.lock() {
        Ok(mut m) => m.entry(ds.id).or_default().clone(),
        Err(_) => return Ok(()),
    };
    let _one = lock.lock().await;
    // Shared per pass: two robots on one dataset cost one fetch of the period that closed.
    {
        let Ok(mut done) = TOPPED.lock() else { return Ok(()) };
        if done.get(&ds.id) == Some(&running) {
            return Ok(());
        }
        done.insert(ds.id, running);
    }
    // Only an instrument that was still printing bars one period ago is worth waiting for.
    let live = ds.range_to.is_some_and(|last| last >= last_closed - time::Duration::seconds(tf));
    let tries = if live { PUBLISH_TRIES } else { 1 };

    let connector = crate::histdata::connector_for(&ds.provider)?;
    let conn = data_connector_for(&state.pool, session, ds).await?;
    let secrets = match &conn {
        Some(c) => conn_store::load_creds(&state.pool, &state.cipher, c.id).await?,
        None => Default::default(),
    };
    let mut bars = Vec::new();
    let mut quotes = Vec::new();
    for attempt in 0..tries {
        if attempt > 0 {
            tokio::time::sleep(PUBLISH_WAIT).await;
        }
        if let Some(c) = &conn {
            // Display-only counter; a failed write must not block the fetch.
            let _ = otw_store::api_quota::bump(
                &state.pool,
                &crate::connectors_api::quota_scope(c.id),
            )
            .await;
        }
        let chunk = connector
            .fetch_chunk(
                &state.http,
                &secrets,
                &ds.ticker,
                &ds.asset_type,
                &ds.timeframe,
                from,
                running,
            )
            .await?;
        let chunk = crate::histdata::closed_only(chunk, &ds.timeframe, now);
        bars = chunk.bars.into_iter().filter(|b| b.ts >= from && b.ts < running).collect();
        quotes = chunk.quotes.into_iter().filter(|q| q.ts >= from && q.ts < running).collect();
        // The bar the tick came for is in: nothing to wait for.
        if bars.last().is_some_and(|b| b.ts >= last_closed) {
            break;
        }
    }
    if bars.is_empty() {
        return Ok(());
    }
    hd::write_bars(&state.pool, ds.id, &bars).await?;
    if !quotes.is_empty() {
        hd::write_quotes(&state.pool, ds.id, &quotes).await?;
    }
    Ok(())
}

/// Record what a tick produced as events. Returns them oldest first.
pub async fn record(
    state: &AppState,
    session: &store::Session,
    t: &Tick,
    book: &Book,
) -> anyhow::Result<Vec<store::Event>> {
    let mut out = Vec::new();
    for (kind, trades) in [("open", &t.opened), ("close", &t.closed)] {
        for trade in trades.iter() {
            let (title, message) = render(state, session, kind, trade, t, book, false).await;
            let notify = deliverable(session, kind, trade);
            out.push(
                store::add_event(
                    &state.pool,
                    session.id,
                    kind,
                    trade,
                    &title,
                    &message,
                    notify,
                )
                .await?,
            );
        }
    }
    if !t.error.is_empty() {
        let payload = json!({ "error": t.error });
        let title = format!("{}: paper session failed", session.name);
        out.push(
            store::add_event(
                &state.pool,
                session.id,
                "error",
                &payload,
                &title,
                &t.error,
                true,
            )
            .await?,
        );
    }
    if !out.is_empty() {
        let _ = store::trim_events(&state.pool, session.id, EVENT_KEEP).await;
    }
    Ok(out)
}

/// Whether this event is worth pushing to a channel. A failure and a heartbeat always are:
/// they are about the session itself, not about an instrument, so no instrument filter
/// applies to them. The event is recorded either way.
fn deliverable(session: &store::Session, kind: &str, trade: &Value) -> bool {
    match kind {
        // A reverse is the close and the entry in one message: said when either is asked for.
        "open" if trade.get("reverses").is_some_and(Value::is_object) => (session.notify_open || session.notify_close) && ticker_allowed(session, trade),
        "open" => session.notify_open && ticker_allowed(session, trade),
        "close" => session.notify_close && ticker_allowed(session, trade),
        _ => true,
    }
}

/// An empty list means every instrument. A named list is matched case-insensitively against
/// the trade's own ticker; a trade with no ticker (a single-asset run that did not stamp one)
/// never matches a named list, since there is nothing to match it on.
fn ticker_allowed(session: &store::Session, trade: &Value) -> bool {
    if session.notify_tickers.is_empty() {
        return true;
    }
    let ticker = trade.get("ticker").and_then(Value::as_str).unwrap_or("");
    !ticker.is_empty()
        && session.notify_tickers.iter().any(|t| t.eq_ignore_ascii_case(ticker))
}

// ── What a message may talk about ─────────────────────────────────────────────

/// The two periods a message can report on, plus what is held right now.
///
/// Both aggregates come from one indexed query over the closed round trips, so a session
/// that has been running for a year costs the same at send time as one started this morning.
/// They are read at delivery, never per trade.
pub struct Book {
    pub since: store::Agg,
    pub total: store::Agg,
    pub since_from: OffsetDateTime,
    pub total_from: OffsetDateTime,
    pub open: Vec<store::Trade>,
}

impl Book {
    /// An empty book, for a preview or a session that has not run.
    pub fn empty(session: &store::Session) -> Self {
        let from = session.seeded_at.unwrap_or(session.created_at);
        Self {
            since: store::Agg::default(),
            total: store::Agg::default(),
            since_from: from,
            total_from: from,
            open: Vec::new(),
        }
    }
}

impl Book {
    /// A book with something in it, so the variable catalog can advertise the fields of an
    /// open position. A sample the editor can read beats a list that only fills in once the
    /// session happens to hold something.
    pub fn sample(session: &store::Session) -> Self {
        let now = OffsetDateTime::now_utc();
        let mut b = Self::empty(session);
        b.since = store::Agg { trades: 2, wins: 1, losses: 1, pnl: 21.4, fees: 1.2, best: 32.0, worst: -10.6 };
        b.total = store::Agg { trades: 9, wins: 6, losses: 3, pnl: 154.9, fees: 5.4, best: 61.2, worst: -22.8 };
        b.open = vec![store::Trade {
            session_id: session.id,
            trade_key: "AAPL|long|2026-09-04T12:21:23Z".into(),
            ticker: "AAPL".into(),
            direction: "long".into(),
            entry_ts: "2026-09-04T12:21:23Z".into(),
            exit_ts: String::new(),
            open: true,
            pnl: 12.5,
            fees: 0.4,
            trade: json!({ "entry_price": 432.23, "qty": 100.0 }),
            opened_at: now,
            closed_at: None,
            robot_qty: None,
            scale: Decimal::ONE,
            robot_note: String::new(),
            robot_pnl: None,
            robot_fees: None,
            money_at_broker: false,
        }];
        b
    }
}

/// Read both periods and the open positions. `since` starts at the last **delivered**
/// notification (that is what "since the last alert" means), falling back to the session's
/// own beginning when nothing has been sent yet.
pub async fn book(state: &AppState, session: &store::Session) -> Book {
    let total_from = session.seeded_at.unwrap_or(session.created_at);
    let since_from = session.last_digest_at.unwrap_or(total_from);
    let (since, total) =
        match store::aggregates(&state.pool, session.id, since_from, total_from).await {
            Ok(v) => v,
            Err(e) => {
                tracing::error!("paper: aggregating the book: {e:#}");
                (store::Agg::default(), store::Agg::default())
            }
        };
    let open = store::list_trades(&state.pool, session.id, true).await.unwrap_or_default();
    Book { since, total, since_from, total_from, open }
}

/// One period, as the template sees it. `pnl_pct` is on the money the session started with,
/// which is the only base that means anything for a plan that never deposits.
fn agg_json(a: &store::Agg, from: OffsetDateTime, capital: f64) -> Value {
    let win_rate = if a.trades > 0 { a.wins as f64 * 100.0 / a.trades as f64 } else { 0.0 };
    let pnl_pct = if capital > 0.0 { a.pnl * 100.0 / capital } else { 0.0 };
    json!({
        "from": rfc3339(from),
        "trades": a.trades,
        "wins": a.wins,
        "losses": a.losses,
        "win_rate": win_rate,
        "pnl": a.pnl,
        "pnl_pct": pnl_pct,
        "fees": a.fees,
        "best": a.best,
        "worst": a.worst,
    })
}

/// The live positions. `summary` is pre-rendered because the template language has no loop
/// on purpose: a list that has to be walked is built here, once, where it can be read.
fn open_json(open: &[store::Trade]) -> Value {
    let items: Vec<Value> = open
        .iter()
        .map(|t| {
            let f = |k: &str| t.trade.get(k).and_then(Value::as_f64).unwrap_or(0.0);
            json!({
                "ticker": t.ticker,
                "direction": t.direction,
                "entry_ts": t.entry_ts,
                "entry_price": f("entry_price"),
                "qty": f("qty"),
                "pnl": t.pnl,
            })
        })
        .collect();
    let summary = if items.is_empty() {
        "no open position".to_string()
    } else {
        items
            .iter()
            .map(|i| {
                format!(
                    "{} {} {} at {} since {}",
                    i["direction"].as_str().unwrap_or(""),
                    i["qty"].as_f64().unwrap_or(0.0),
                    i["ticker"].as_str().unwrap_or(""),
                    i["entry_price"].as_f64().unwrap_or(0.0),
                    i["entry_ts"].as_str().unwrap_or("")
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    json!({ "count": items.len(), "summary": summary, "items": items })
}

fn rfc3339(t: OffsetDateTime) -> String {
    t.format(&time::format_description::well_known::Rfc3339).unwrap_or_default()
}

// ── Messages ──────────────────────────────────────────────────────────────────

/// The namespaces a message template resolves against. Built from the session, the trade and
/// the last stat block, so the helper list the UI shows is generated from this same function
/// (see [`variables`]) and cannot drift from what the renderer actually resolves.
pub fn context(
    session: &store::Session,
    event: &str,
    trade: &Value,
    t: &Tick,
    book: &Book,
) -> expr::Ctx {
    let stats = t.stats.clone().or_else(|| session.stats.clone()).unwrap_or_else(|| json!({}));
    let capital = rust_decimal::prelude::ToPrimitive::to_f64(&start_equity(session)).unwrap_or(0.0);
    let mut extra = Map::new();
    extra.insert(
        "session".into(),
        json!({
            "id": session.id.to_string(),
            "name": session.name,
            "status": session.status,
            "bars": t.bars,
            "timezone": session.timezone,
        }),
    );
    extra.insert("event".into(), json!({ "kind": event, "at": now_rfc3339() }));
    // A summary is about a period, not about a trade: offering `trade.*` there would put
    // one arbitrary fill's numbers in a message that speaks for many.
    if event != "digest" {
        extra.insert("trade".into(), trade.clone());
    }
    extra.insert("stats".into(), stats);
    // The two periods a message may report on, and what is held right now.
    extra.insert("since".into(), agg_json(&book.since, book.since_from, capital));
    extra.insert("total".into(), agg_json(&book.total, book.total_from, capital));
    extra.insert("open".into(), open_json(&book.open));
    expr::Ctx { extra, ..Default::default() }
}

/// Render the user's template, falling back to the built-in wording. A template that fails to
/// resolve does not mean silence: the default message is sent with the reason appended, so a
/// typo costs a line of noise rather than a missed fill.
async fn render(
    state: &AppState,
    session: &store::Session,
    event: &str,
    trade: &Value,
    t: &Tick,
    book: &Book,
    preview: bool,
) -> (String, String) {
    let ctx = context(session, event, trade, t, book);
    let (mut title, mut body) = default_message(session, event, trade);
    let resolver = expr::Resolver::new(state, expr::Secrets::default());
    let mut complaint = String::new();
    // `allow_secrets: false`: a notification body is not a secret-bearing field, so `vault.*`
    // must not resolve into a message leaving the machine.
    // Entry and exit are two messages, not one worded twice: each reads its own pair.
    let (title_tpl, body_tpl) = if event == "open" {
        (&session.entry_title_template, &session.entry_template)
    } else {
        (&session.exit_title_template, &session.exit_template)
    };
    if !title_tpl.trim().is_empty() {
        match render_one(&resolver, title_tpl, &ctx, preview).await {
            Ok(text) => title = text,
            Err(e) => complaint.push_str(&format!("\n\n(title template: {e})")),
        }
    }
    if !body_tpl.trim().is_empty() {
        match render_one(&resolver, body_tpl, &ctx, preview).await {
            Ok(text) => body = text,
            Err(e) => complaint.push_str(&format!("\n\n(message template: {e})")),
        }
    }
    body.push_str(&complaint);
    (title, body)
}

/// One template, rendered the way its destination needs it. A message that leaves the machine
/// is all or nothing (a half-resolved alert is worse than the built-in wording), while the
/// editor's preview keeps the text and marks the paths it cannot read in place.
/// `allow_secrets: false` in both: a notification body may not carry a `vault.*` value.
async fn render_one(
    resolver: &expr::Resolver<'_>,
    template: &str,
    ctx: &expr::Ctx,
    preview: bool,
) -> Result<String, String> {
    if preview {
        resolver.render_preview(template, ctx).await
    } else {
        resolver.render_str(template, ctx, false).await
    }
}

/// The grouped message. Same renderer and same context as a per-fill message, minus the
/// trade: a digest is about the period, so `trade.*` resolves to nothing and a template
/// naming it is told so rather than silently printing a blank.
async fn render_digest(
    state: &AppState,
    session: &store::Session,
    book: &Book,
    preview: bool,
) -> (String, String) {
    let t = Tick::failed(String::new());
    let ctx = context(session, "digest", &json!({}), &t, book);
    let resolver = expr::Resolver::new(state, expr::Secrets::default());
    let mut title = format!("{}: {} trades since {}", session.name, book.since.trades, rfc3339(book.since_from));
    let mut body = format!(
        "{} trade(s), PnL {:+.2}, best {:+.2}, worst {:+.2}.\n{}",
        book.since.trades,
        book.since.pnl,
        book.since.best,
        book.since.worst,
        open_json(&book.open)["summary"].as_str().unwrap_or("")
    );
    let mut complaint = String::new();
    if !session.summary_title_template.trim().is_empty() {
        match render_one(&resolver, &session.summary_title_template, &ctx, preview).await {
            Ok(text) => title = text,
            Err(e) => complaint.push_str(&format!("\n\n(summary title: {e})")),
        }
    }
    if !session.summary_template.trim().is_empty() {
        match render_one(&resolver, &session.summary_template, &ctx, preview).await {
            Ok(text) => body = text,
            Err(e) => complaint.push_str(&format!("\n\n(summary template: {e})")),
        }
    }
    body.push_str(&complaint);
    (title, body)
}

/// The wording used when the user wrote no template.
fn default_message(session: &store::Session, event: &str, trade: &Value) -> (String, String) {
    let f = |k: &str| trade.get(k).and_then(|v| v.as_f64()).unwrap_or(0.0);
    let s = |k: &str| trade.get(k).and_then(|v| v.as_str()).unwrap_or("");
    let ticker = if s("ticker").is_empty() { session.name.as_str() } else { s("ticker") };
    let pending = trade.get("pending").and_then(Value::as_bool).unwrap_or(false);
    match event {
        "open" if pending && s("order") == "limit" => (
            format!("{}: {} {} limit order", session.name, s("direction"), ticker),
            format!(
                "Triggering limit order at {:.4} for {:.4} units, valid {} bar(s), on the signal of {}.",
                f("entry_price"),
                f("qty"),
                f("bars_left"),
                s("signal_ts")
            ),
        ),
        "open" if pending && trade.get("reverses").is_some_and(Value::is_object) => {
            let r = &trade["reverses"];
            let rf = |k: &str| r.get(k).and_then(Value::as_f64).unwrap_or(0.0);
            let was = r.get("direction").and_then(Value::as_str).unwrap_or("");
            (
                format!("{}: {ticker} reversed {was} to {}", session.name, s("direction")),
                format!(
                    "The {was} position ({:.4} units from {:.4}) closes at market, about {:.4}, PnL about {:+.2}; {} for {:.4} units at market. Both fill at the next open (close of {}).",
                    rf("qty"),
                    rf("entry_price"),
                    rf("exit_price"),
                    rf("pnl"),
                    s("direction"),
                    f("qty"),
                    s("signal_ts")
                ),
            )
        }
        "open" if pending => (
            format!("{}: {} {}", session.name, s("direction"), ticker),
            format!(
                "Entry at market for {:.4} units, about {:.4} (close of {}); filled at the next open.",
                f("qty"),
                f("entry_price"),
                s("signal_ts")
            ),
        ),
        "open" => (
            format!("{}: {} {}", session.name, s("direction"), ticker),
            if f("qty") > 0.0 {
                format!("Entry {:.4} for {:.4} units at {}.", f("entry_price"), f("qty"), s("entry_ts"))
            } else {
                format!("Entry {:.4} at {}.", f("entry_price"), s("entry_ts"))
            },
        ),
        _ => (
            format!("{}: {} {} closed", session.name, s("direction"), ticker),
            format!(
                "Exit {:.4} at {} ({}). PnL {:+.2} ({:+.2}%).",
                f("exit_price"),
                s("exit_ts"),
                s("exit_reason"),
                f("pnl"),
                f("return_pct")
            ),
        ),
    }
}

/// Every variable a template can read, with a sample value. Generated from [`context`] over a
/// representative trade, so the helper list in the editor is the resolver's own vocabulary.
pub fn variables(session: &store::Session, kind: &str) -> Vec<Value> {
    let trade = match kind {
        "open" => entry_sample_trade(),
        _ => sample_trade(),
    };
    let tick = Tick {
        skipped: false,
        bars: session.bars.max(0) as usize,
        inputs_hash: String::new(),
        stats: session.stats.clone(),
        opened: Vec::new(),
        closed: Vec::new(),
        error: String::new(),
    };
    let ctx = context(session, kind, &trade, &tick, &Book::sample(session));
    let mut out = Vec::new();
    for (root, value) in &ctx.extra {
        flatten(root, value, &mut out);
    }
    out.sort_by(|a, b| a["path"].as_str().unwrap_or("").cmp(b["path"].as_str().unwrap_or("")));
    out
}

/// Leaf paths of a JSON value, as `{path, type, sample}`. Objects recurse; an array is
/// offered whole plus its first element, which is how `[0]` indexing is discovered.
fn flatten(path: &str, value: &Value, out: &mut Vec<Value>) {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                flatten(&format!("{path}.{k}"), v, out);
            }
        }
        Value::Array(items) => {
            out.push(json!({ "path": path, "type": "list", "sample": items.len() }));
            if let Some(first) = items.first() {
                flatten(&format!("{path}[0]"), first, out);
            }
        }
        other => {
            let kind = match other {
                Value::Number(_) => "number",
                Value::Bool(_) => "boolean",
                Value::Null => "empty",
                _ => "text",
            };
            out.push(json!({ "path": path, "type": kind, "sample": other }));
        }
    }
}

/// Filters the renderer accepts, written the way a template must spell them: an argument is
/// `name:arg`, not `name(arg)`. The panel shows this list verbatim, so the one thing a user
/// cannot get wrong is the syntax.
pub const FILTERS: &[&str] =
    &["round:2", "date:YYYY-MM-DD", "default:-", "upper", "lower", "trim", "json"];

// ── Delivery ──────────────────────────────────────────────────────────────────

/// Push what is due to the session's channels. Nothing here decides *what happened*, only
/// whether it leaves the machine now: an event that is not sent keeps `notified_at` NULL and
/// is read in the app as the offline inbox.
///
/// **A fill leaves on the tick that produced it.** The alert's clock is the strategy's own,
/// so an entry on a one-minute rule is a one-minute alert; `cadence` never holds one back.
/// What it does govern is the *summary*, a message about the period rather than about a
/// trade, which is the one thing the user asked to receive hourly, daily or not at all.
pub async fn deliver(
    state: &AppState,
    session: &store::Session,
    now: OffsetDateTime,
    book: &Book,
) {
    if session.notify_when == "never" {
        return;
    }
    // Excluded and flat: nothing to say until the robot trades again.
    if excluded(session, now).is_some() && !holds_position(book) {
        return;
    }
    deliver_events(state, session).await;
}

/// The status report, sent on its own schedule (`next_report_at`), whatever the ticks did:
/// the robot's state, then the period's figures and what it holds. Not sent while the robot is
/// excluded and flat. Plans the next one either way.
pub async fn report(state: &AppState, session: &store::Session) {
    let now = OffsetDateTime::now_utc();
    let book = book(state, session).await;
    let quiet = session.notify_when == "never" || (excluded(session, now).is_some() && !holds_position(&book));
    if !quiet {
        let (title, body) = render_digest(state, session, &book, false).await;
        let body = format!("{}\n{body}", status_line(session, now));
        if push(state, session, title, body).await {
            if let Err(e) = store::set_digest_mark(&state.pool, session.id, now).await {
                tracing::error!("robots: advancing the report watermark: {e:#}");
            }
        }
    }
    if let Err(e) = store::set_next_report(&state.pool, session.id, next_report(session)).await {
        tracing::error!("robots: planning the next report of \"{}\": {e:#}", session.name);
    }
}

/// One line on what the robot is doing now.
fn status_line(session: &store::Session, now: OffsetDateTime) -> String {
    let mut parts = vec![format!("Status: {}", session.status)];
    if session.on_broker() {
        parts.push(if session.armed_at.is_some() { "armed".into() } else { "not armed".into() });
    }
    if let Some(why) = guards::State::of(&session.guard_state).holds(now) {
        parts.push(why);
    }
    if let Some(why) = excluded(session, now) {
        parts.push(why);
    }
    if !session.last_error.is_empty() {
        parts.push(format!("last error: {}", session.last_error));
    }
    parts.join(", ")
}

/// Push the events not delivered yet, grouped into one message. False when they could not be
/// read (nothing else is sent then either). Also how a broker execution notice leaves the
/// moment it is recorded, between two ticks.
pub async fn deliver_events(state: &AppState, session: &store::Session) -> bool {
    if session.notify_when == "never" {
        return true;
    }
    let pending = match store::undelivered(&state.pool, session.id).await {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("paper: reading undelivered events: {e:#}");
            return false;
        }
    };

    // Several fills can land in one tick (a window that jumped, a seed catching up): they are
    // one message, not a burst of pings, because they happened at the same instant as far as
    // the session is concerned.
    if !pending.is_empty() {
        let (title, body) = if pending.len() == 1 {
            (pending[0].title.clone(), pending[0].message.clone())
        } else {
            let lines: Vec<String> =
                pending.iter().map(|e| format!("• {} : {}", e.title, e.message)).collect();
            (format!("{}: {} paper events", session.name, pending.len()), lines.join("\n"))
        };
        // Nothing left the machine (no channel granted, none enabled, every send failed): the
        // events stay undelivered so the next tick tries again and the inbox still shows them
        // as never sent. Marking them here would turn "nowhere to send" into "sent".
        if push(state, session, title, body).await {
            let ids: Vec<Uuid> = pending.iter().map(|e| e.id).collect();
            if let Err(e) = store::mark_notified(&state.pool, &ids).await {
                tracing::error!("paper: marking events delivered: {e:#}");
            }
        }
    }
    true
}

/// One send to the session's channels. `true` means at least one channel took it; the caller
/// decides what that makes deliverable, since a message nobody received must not be recorded
/// as read.
async fn push(state: &AppState, session: &store::Session, title: String, body: String) -> bool {
    let only: Option<Vec<Uuid>> =
        if session.channel_ids.is_empty() { None } else { Some(session.channel_ids.clone()) };
    let notif = FiredNotification { name: title, details: body, url: String::new() };
    notif_send::dispatch(
        &state.pool,
        &state.cipher,
        &state.http,
        &notif,
        MODULE,
        only.as_deref(),
    )
    .await
        > 0
}

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

// ── One complete tick ─────────────────────────────────────────────────────────

/// Simulate, record, schedule the next occurrence and deliver. The one entry point the
/// worker and the "run now" button share, so a manual tick and a scheduled one differ in
/// nothing but who asked.
pub async fn run_once(state: &AppState, session: &store::Session, force: bool) -> Tick {
    let t = tick(state, session, force).await;
    let now = OffsetDateTime::now_utc();
    // Read once per tick and handed to every renderer: the aggregates are one indexed query,
    // and a message that mentions no period must not pay for one either.
    let book = book(state, session).await;

    // The first successful tick is the session *learning its own book*: every round trip in
    // the window already happened, and reporting them one by one would open a new session
    // with a burst of alerts about history. It is summarized in a single event instead, and
    // only what happens after it is a fill worth a message.
    let seeding = session.inputs_hash.is_empty();
    if !t.skipped {
        if seeding {
            // The boundary of "since the beginning": the session exists from here, and the
            // round trips written by this very tick are the history it inherited.
            if let Err(e) = store::set_seeded_at(&state.pool, session.id, now).await {
                tracing::error!("paper: stamping the seed: {e:#}");
            }
            let payload = json!({ "trades": t.opened.len() + t.closed.len(), "bars": t.bars });
            let title = format!("{}: started", session.name);
            let message = format!(
                "Book seeded from {} bars: {} past round trip(s) recorded, {} position(s) \
                 open. From here on, only new fills are reported.",
                t.bars,
                t.closed.len(),
                t.opened.len()
            );
            if let Err(e) =
                store::add_event(&state.pool, session.id, "run", &payload, &title, &message, true)
                    .await
            {
                tracing::error!("paper: recording the seed: {e:#}");
            }
        } else if let Err(e) = record(state, session, &t, &book).await {
            tracing::error!("paper: recording events: {e:#}");
        }
    }
    // `always` reports every tick, the quiet ones included, and a tick short-circuited by the
    // freshness check is the quietest of all: that is the difference between "nothing
    // happened" and "the engine stopped running", which a heartbeat is the only way to tell.
    if session.notify_when == "always" && !t.changed() && t.error.is_empty() && !(excluded(session, now).is_some() && !holds_position(&book)) {
        let stats = t.stats.clone().or_else(|| session.stats.clone()).unwrap_or_else(|| json!({}));
        let title = format!("{}: no change", session.name);
        let message = if t.skipped {
            format!("No new bar since the last run ({} in the window).", t.bars)
        } else {
            format!("{} bars simulated, no new fill.", t.bars)
        };
        if let Err(e) =
            store::add_event(&state.pool, session.id, "run", &stats, &title, &message, true).await
        {
            tracing::error!("paper: recording the heartbeat: {e:#}");
        }
    }

    // A failed tick keeps its schedule: the next occurrence is planned from the rule, and the
    // session is flipped to `error` by the store so the page says why it stopped producing.
    let next = next_tick(state, session).await;
    if let Err(e) = store::finish_tick(
        &state.pool,
        session.id,
        next,
        t.stats.as_ref(),
        t.bars as i32,
        &t.inputs_hash,
        backtest::ENGINE_VERSION as i32,
        &t.error,
    )
    .await
    {
        tracing::error!("paper: closing the tick: {e:#}");
    }

    // The robot's layers: the day's figures, then the guards over what just closed.
    if !t.skipped && t.error.is_empty() {
        if let Err(e) = otw_store::robots_guards::refresh_realized(&state.pool, session.id, now.date()).await {
            tracing::error!("robots: the day's figures: {e:#}");
        }
        if !seeding {
            if let Some(fresh) = store::get_session(&state.pool, session.id).await.ok().flatten() {
                if let Err(e) = run_guards(state, &fresh).await {
                    tracing::error!("robots: guards of \"{}\": {e:#}", session.name);
                }
            }
        }
    }
    // The book is re-read for delivery: the seed just stamped its boundary, and the events
    // this tick recorded are part of what the digest reports.
    let book = self::book(state, session).await;
    deliver(state, session, now, &book).await;
    // The levels the live watch triggers on may have moved with this tick.
    live::poke();
    // A session executing on a broker converges on what this tick decided. A simulated one is
    // left alone by the sync.
    if session.on_broker() {
        exec::poke();
    }
    t
}

/// Render the session's templates against a representative trade, for the editor's preview.
/// A draft carries no stats, so a `stats.*` path the real message resolves comes back missing
/// here: the complaint is rewritten to say it is the preview that cannot read it.
pub async fn preview(state: &AppState, session: &store::Session, event: &str) -> (String, String) {
    let (title, body) = if event == "digest" {
        render_digest(state, session, &Book::sample(session), true).await
    } else {
        let trade = if event == "open" { entry_sample_trade() } else { sample_trade() };
        let t = Tick {
            skipped: false,
            bars: session.bars.max(0) as usize,
            inputs_hash: String::new(),
            stats: session.stats.clone(),
            opened: Vec::new(),
            closed: Vec::new(),
            error: String::new(),
        };
        render(state, session, event, &trade, &t, &Book::sample(session), true).await
    };
    (as_preview(&title), as_preview(&body))
}

/// The resolver speaks for a live run ("not available at this point in the run"), which reads
/// as a broken template when it is only the preview that has no such value yet.
fn as_preview(text: &str) -> String {
    text.replace("is not available at this point in the run", "is not available in the preview")
}

/// The trade every helper list and preview is built on. One definition, so the paths the
/// editor offers are exactly the paths the preview resolves.
/// The entry half of a trade: what a position knows the moment it is opened. Offering the
/// exit paths there would put an empty number in every entry message.
fn entry_sample_trade() -> Value {
    let mut t = sample_trade();
    if let Some(map) = t.as_object_mut() {
        for k in ["exit_ts", "exit_price", "exit_reason", "pnl", "return_pct", "bars_held", "mae", "mfe"] {
            map.remove(k);
        }
    }
    t
}

fn sample_trade() -> Value {
    json!({
        "ticker": "BTCUSDT", "direction": "long", "entry_ts": "2026-09-04T10:00:00Z",
        "exit_ts": "2026-09-04T14:00:00Z", "entry_price": 61250.5, "exit_price": 61890.0,
        "qty": 0.25, "entries": 1, "exit_reason": "take_profit", "pnl": 159.87,
        "fees": 3.06, "return_pct": 1.04, "bars_held": 4, "mae": 42.1, "mfe": 210.4
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tick is planned on the bar grid, not on the second the session was created: a
    /// one-minute rule born at 14:37:38 fires at 14:38:00, 14:39:00, and so on.
    #[test]
    fn an_interval_fires_on_the_period_boundary() {
        let mut s = session();
        s.every_minutes = Some(1);
        let born = OffsetDateTime::from_unix_timestamp(1_757_000_258).unwrap(); // ...:37:38
        let at = on_bar_grid(&s, born + time::Duration::minutes(1));
        assert_eq!(at.unix_timestamp() % 60, PUBLISH_LAG_SECS);
        assert!(at > born);
        // Already on the grid: the plan is kept as it is.
        assert_eq!(on_bar_grid(&s, at), at);

        s.every_minutes = Some(15);
        let at = on_bar_grid(&s, born);
        assert_eq!(at.unix_timestamp() % (15 * 60), PUBLISH_LAG_SECS);
        assert!(at >= born);
    }

    /// A broker robot shows as positions only the trades its orders filled at the broker: one
    /// whose entry never left, was refused, did not fill, or that the user closed there is in
    /// the log, not in the book. A simulated robot shows its whole book.
    #[test]
    fn a_broker_robot_shows_only_what_filled_at_the_broker() {
        use otw_store::robots_exec::Order;
        let trade = |entry: &str| store::Trade {
            session_id: Uuid::nil(),
            trade_key: format!("BTCUSDT|long|{entry}"),
            ticker: "BTCUSDT".into(),
            direction: "long".into(),
            entry_ts: entry.into(),
            exit_ts: String::new(),
            open: true,
            pnl: 0.0,
            fees: 0.0,
            trade: json!({ "qty": 1.0 }),
            opened_at: OffsetDateTime::UNIX_EPOCH,
            closed_at: None,
            robot_qty: Some(Decimal::ONE),
            scale: Decimal::ONE,
            robot_note: String::new(),
            robot_pnl: None,
            robot_fees: None,
            money_at_broker: false,
        };
        let at = |t: &str| OffsetDateTime::parse(t, &time::format_description::well_known::Rfc3339).unwrap();
        let order = |signal: &str, outcome: &str, filled: &str| Order {
            session_id: Uuid::nil(),
            account_id: Uuid::nil(),
            client_id: format!("e{signal}"),
            intent: format!("entry|BTCUSDT|long|{signal}"),
            role: "entry".into(),
            ticker: "BTCUSDT".into(),
            symbol: "BTCUSDT".into(),
            side: "buy".into(),
            qty: Decimal::ONE,
            limit_price: None,
            stop_price: None,
            engine_qty: None,
            engine_price: None,
            position: String::new(),
            outcome: outcome.into(),
            reason: None,
            venue_id: None,
            fills_as: None,
            status: Some("filled".into()),
            filled_qty: filled.parse().unwrap(),
            created_at: at(signal),
            updated_at: at(signal),
        };
        let trades = [
            trade("2026-11-17T10:01:00Z"),
            trade("2026-11-17T11:01:00Z"),
            trade("2026-11-17T12:01:00Z"),
            trade("2026-11-17T13:01:00Z"),
            trade("2026-11-17T14:01:00Z"),
        ];
        let mut outside = order("2026-11-17T14:00:00Z", "sent", "1");
        outside.client_id = "x".into();
        outside.role = "close".into();
        outside.position = "e2026-11-17T14:00:00Z".into();
        outside.reason = Some(exec::CLOSED_OUTSIDE.into());
        let orders = [
            order("2026-11-17T10:00:00Z", "sent", "1"),
            order("2026-11-17T11:00:00Z", "refused", "0"),
            order("2026-11-17T12:00:00Z", "sent", "0"),
            order("2026-11-17T14:00:00Z", "sent", "1"),
            outside,
        ];
        let mut s = session();
        assert!(dashboard::not_at_broker(&s, &trades, &orders).is_empty(), "a simulated bot shows its book");
        s.execution = Some(json!({ "target": "broker" }));
        let mut hidden: Vec<String> = dashboard::not_at_broker(&s, &trades, &orders).into_iter().collect();
        hidden.sort();
        // Refused, not filled, never sent (13:00), closed by the user: hidden. Filled: shown.
        assert_eq!(hidden, trades[1..].iter().map(|t| t.trade_key.clone()).collect::<Vec<_>>());
    }

    pub(crate) fn session() -> store::Session {
        let now = OffsetDateTime::now_utc();
        store::Session {
            id: Uuid::nil(),
            name: "TEST".into(),
            strategy_id: None,
            settings: json!({ "kind": "signals" }),
            dataset_ids: vec![Uuid::nil()],
            window_bars: Some(400),
            start_ts: None,
            feed: "hist".into(),
            kind: "interval".into(),
            timezone: "UTC".into(),
            every_minutes: Some(1),
            at_hour: None,
            at_minute: None,
            weekdays: None,
            day_of_month: None,
            run_at: None,
            next_run_at: None,
            last_run_at: None,
            notify_when: "on_change".into(),
            cadence: "each".into(),
            channel_ids: Vec::new(),
            entry_title_template: String::new(),
            entry_template: String::new(),
            exit_title_template: String::new(),
            exit_template: String::new(),
            summary_title_template: String::new(),
            summary_template: String::new(),
            last_digest_at: None,
            seeded_at: None,
            notify_open: true,
            notify_close: true,
            notify_tickers: Vec::new(),
            status: "active".into(),
            inputs_hash: String::new(),
            engine_version: 0,
            stats: None,
            bars: 0,
            last_error: String::new(),
            running: false,
            created_at: now,
            updated_at: now,
            execution: None,
            exec_account_id: None,
            armed_at: None,
            armed_by: None,
            exec_state: "idle".into(),
            exec_note: String::new(),
            exec_synced_at: None,
            exec_book: None,
            exec_baseline: json!({}),
            priority: 100,
            capital: json!({}),
            guards: json!([]),
            guard_state: json!({}),
            strategy_version: None,
            strategy_name: String::new(),
            exclusions: json!({}),
            excluded_at: None,
            next_report_at: None,
            data_connectors: json!({}),
        }
    }

    fn fill(kind: &str, reason: &str, key: &str, bar_ts: &str, at: &str) -> store::LiveFill {
        store::LiveFill {
            id: Uuid::new_v4(),
            session_id: Uuid::nil(),
            trade_key: key.into(),
            ticker: "ETHUSDT".into(),
            kind: kind.into(),
            direction: "long".into(),
            reason: reason.into(),
            bar_ts: bar_ts.into(),
            price: 100.0,
            maker: false,
            quoted: false,
            at: OffsetDateTime::parse(at, &time::format_description::well_known::Rfc3339).unwrap(),
        }
    }

    /// A market entry announced at its signal's close is not alerted again when the next bar
    /// opens it; a signal is no fill; a live fill after the last stored bar waits for that bar.
    #[test]
    fn announced_entries_and_fills_past_the_data() {
        let ts: Vec<String> = ["12:01", "12:02", "12:03"].iter().map(|t| format!("2026-10-03T{t}:00Z")).collect();
        let n = ts.len();
        let asset = backtest::OwnedBars {
            ticker: "ETHUSDT".into(),
            timeframe: "1m".into(),
            ts: ts.clone(),
            open: vec![100.0; n],
            high: vec![100.0; n],
            low: vec![100.0; n],
            close: vec![100.0; n],
            volume: vec![0.0; n],
            quotes: None,
            sub: None,
            script: None,
        };
        let live = vec![
            fill("signal", "market", "ETHUSDT|long|signal:2026-10-03T12:02:00Z", "2026-10-03T12:02:00Z", "2026-10-03T12:03:00Z"),
            fill("entry", "market", "ETHUSDT|long|2026-10-03T12:03:00Z", "2026-10-03T12:03:00Z", "2026-10-03T12:03:01Z"),
            fill("exit", "stop_loss", "ETHUSDT|long|2026-10-03T12:03:00Z", "", "2026-10-03T12:04:30Z"),
        ];
        let (forced, alerted, exits) = forced_fills(&[asset], &live);
        assert_eq!(forced[0].len(), 1, "only the entry fill replays; the exit is past 12:03's end");
        assert_eq!(forced[0][0].bar, 2);
        assert!(alerted.contains(&("ETHUSDT|long|2026-10-03T12:03:00Z".to_string(), "entry".to_string())));
        assert!(exits.is_empty());
    }

    fn trade(ticker: &str, entry: &str, reason: &str) -> Trade {
        Trade {
            ticker: ticker.into(),
            entry_ts: entry.into(),
            exit_ts: "2026-09-04T14:00:00Z".into(),
            entry_price: 100.0,
            exit_price: 101.0,
            qty: 1.0,
            entries: 1,
            direction: "long".into(),
            exit_reason: reason.into(),
            pnl: 1.0,
            fees: 0.0,
            return_pct: 1.0,
            bars_held: 4,
            mae: 0.0,
            mfe: 1.0,
        }
    }

    /// The key is what makes a fill the *same* fill across ticks. It must not move when
    /// anything downstream of the entry does (the exit, the price, the reason).
    #[test]
    fn trade_key_is_the_entry_identity() {
        let a = trade("AAPL", "2026-09-04T10:00:00Z", "end");
        let mut b = trade("AAPL", "2026-09-04T10:00:00Z", "take_profit");
        b.exit_price = 999.0;
        b.pnl = -50.0;
        assert_eq!(trade_key(&a), trade_key(&b));
        let other = trade("AAPL", "2026-09-04T10:01:00Z", "end");
        assert_ne!(trade_key(&a), trade_key(&other));
    }

    /// Freshness is the whole point of the short-circuit: the same bars must hash the same,
    /// and a landed download must change it.
    #[test]
    fn carry_puts_the_held_positions_back_on_their_bar_or_the_first() {
        let ts: Vec<String> = (0..4).map(|i| format!("2026-11-13T10:0{i}:00Z")).collect();
        let bars = backtest::OwnedBars {
            ticker: "MNQ".into(),
            timeframe: "1m".into(),
            ts: ts.clone(),
            open: vec![1.0; 4],
            high: vec![1.0; 4],
            low: vec![1.0; 4],
            close: vec![1.0; 4],
            volume: vec![1.0; 4],
            quotes: None,
            sub: None,
            script: None,
        };
        let row = |entry: &str| store::Trade {
            session_id: Uuid::nil(),
            trade_key: format!("MNQ|short|{entry}"),
            ticker: "MNQ".into(),
            direction: "short".into(),
            entry_ts: entry.into(),
            exit_ts: String::new(),
            open: true,
            pnl: 0.0,
            fees: 0.0,
            trade: json!({ "entry_price": 28316.5, "qty": 1.0 }),
            opened_at: OffsetDateTime::UNIX_EPOCH,
            closed_at: None,
            robot_qty: Some(Decimal::ONE),
            scale: Decimal::ONE,
            robot_note: String::new(),
            robot_pnl: None,
            robot_fees: None,
            money_at_broker: false,
        };
        let held = [row(&ts[2]), row("2026-11-13T09:00:00Z"), row("2026-11-13T11:00:00Z")];
        let mut forced = vec![Vec::new()];
        let mut remap = HashMap::new();
        carry(std::slice::from_ref(&bars), &held, &mut forced, &mut remap);
        // In the window: on its own bar. Before it: on the first bar, under its own key. After
        // the last stored bar: not carried (the next run has its bar).
        let bars_of: Vec<usize> = forced[0].iter().map(|f| f.bar).collect();
        assert_eq!(bars_of, vec![2, 0]);
        assert!(matches!(forced[0][0].kind, backtest::ForcedKind::Hold { long: false, qty, .. } if qty == 1.0));
        assert_eq!(
            remap.get(&format!("MNQ|short|{}", ts[0])),
            Some(&("MNQ|short|2026-11-13T09:00:00Z".to_string(), "2026-11-13T09:00:00Z".to_string()))
        );
    }

    #[test]
    fn inputs_hash_follows_the_data() {
        let s = session();
        let id = Uuid::from_u128(7);
        let t0 = OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap();
        let v = own::Venue::default();
        let base = inputs_hash(&s, &v, &[(id, t0, 1000)]);
        assert_eq!(base, inputs_hash(&s, &v, &[(id, t0, 1000)]));
        // A download landed: same dataset, later stamp, more bars.
        let t1 = OffsetDateTime::from_unix_timestamp(1_700_000_060).unwrap();
        assert_ne!(base, inputs_hash(&s, &v, &[(id, t1, 1060)]));
        // The strategy was edited.
        let mut edited = session();
        edited.settings = json!({ "kind": "grid" });
        assert_ne!(base, inputs_hash(&edited, &v, &[(id, t0, 1000)]));
        // The robot's capital or the venue's contract changed.
        let mut funded = session();
        funded.capital = json!({ "allocation": "100000" });
        assert_ne!(base, inputs_hash(&funded, &v, &[(id, t0, 1000)]));
        let whole = own::Venue { lot_step: Some(1.0), ..Default::default() };
        assert_ne!(base, inputs_hash(&s, &whole, &[(id, t0, 1000)]));
    }

    /// A robot ticks at the close of the bar in progress on its own timeframe, to the second.
    #[test]
    fn a_robot_ticks_at_each_close_of_its_timeframe() {
        use crate::timeframe::Timeframe;
        use time::macros::datetime;
        let tf = |s: &str| Timeframe::parse(s).unwrap();
        assert_eq!(next_close(tf("1m"), datetime!(2026-11-13 10:15:42.3 UTC)), datetime!(2026-11-13 10:16 UTC));
        // On the boundary: the bar that opens now closes a period later.
        assert_eq!(next_close(tf("1m"), datetime!(2026-11-13 10:16 UTC)), datetime!(2026-11-13 10:17 UTC));
        assert_eq!(next_close(tf("15m"), datetime!(2026-11-13 10:16 UTC)), datetime!(2026-11-13 10:30 UTC));
        assert_eq!(next_close(tf("4h"), datetime!(2026-11-13 10:16 UTC)), datetime!(2026-11-13 12:00 UTC));
        assert_eq!(next_close(tf("1d"), datetime!(2026-11-13 10:16 UTC)), datetime!(2026-11-14 00:00 UTC));
        assert_eq!(next_close(tf("1w"), datetime!(2026-11-13 10:16 UTC)), datetime!(2026-11-16 00:00 UTC));
        assert_eq!(next_close(tf("1M"), datetime!(2026-11-13 10:16 UTC)), datetime!(2026-12-01 00:00 UTC));
    }

    /// The status report follows the recurrence rule, only when the robot sends one and runs.
    #[test]
    fn the_report_is_planned_on_its_rule() {
        let mut s = session();
        s.kind = "interval".into();
        s.every_minutes = Some(60);
        s.status = "active".into();
        s.cadence = "each".into();
        assert!(next_report(&s).is_none(), "no report asked");
        s.cadence = "scheduled".into();
        let at = next_report(&s).unwrap();
        assert!(at > OffsetDateTime::now_utc());
        assert_eq!(at.unix_timestamp() % 3600, 0, "hourly, on the hour");
        s.status = "paused".into();
        assert!(next_report(&s).is_none());
    }

    /// Each message reads its own vocabulary: an entry knows nothing of the exit, and a
    /// summary knows nothing of any single trade.
    #[test]
    fn each_kind_offers_only_what_it_can_resolve() {
        let paths = |kind: &str| -> Vec<String> {
            variables(&session(), kind)
                .iter()
                .map(|v| v["path"].as_str().unwrap_or("").to_string())
                .collect()
        };
        let entry = paths("open");
        assert!(entry.contains(&"trade.entry_price".to_string()));
        assert!(!entry.iter().any(|p| p == "trade.pnl" || p == "trade.exit_price"));

        let summary = paths("digest");
        assert!(!summary.iter().any(|p| p.starts_with("trade.")));
        assert!(summary.contains(&"since.pnl".to_string()));
    }

    /// The helper list the editor shows is generated from the renderer's own context. If a
    /// namespace stops resolving, this is what notices.
    #[test]
    fn variable_catalog_covers_the_namespaces() {
        let paths: Vec<String> = variables(&session(), "close")
            .iter()
            .map(|v| v["path"].as_str().unwrap_or("").to_string())
            .collect();
        for wanted in [
            "trade.pnl",
            "trade.ticker",
            "session.name",
            "event.kind",
            "since.trades",
            "since.pnl_pct",
            "since.from",
            "total.pnl",
            "open.count",
            "open.summary",
            "open.items[0].ticker",
            "open.items[0].entry_price",
        ] {
            assert!(paths.contains(&wanted.to_string()), "missing {wanted} in {paths:?}");
        }
        // Every advertised path must actually resolve against the same context.
        let s = session();
        let ctx =
            context(&s, "close", &sample_trade(), &Tick::failed(String::new()), &Book::sample(&s));
        for p in &paths {
            assert!(expr::lookup(p, &ctx).is_some(), "{p} is advertised but does not resolve");
        }
    }

    /// The filter decides what reaches a channel, never what is recorded: the in-app log is
    /// the trader's own record and must stay complete.
    #[test]
    fn the_channel_filter_narrows_by_side_and_by_instrument() {
        let mut s = session();
        let aapl = json!({ "ticker": "AAPL" });
        let msft = json!({ "ticker": "MSFT" });

        assert!(deliverable(&s, "open", &aapl) && deliverable(&s, "close", &aapl));

        s.notify_open = false;
        assert!(!deliverable(&s, "open", &aapl), "entries were switched off");
        assert!(deliverable(&s, "close", &aapl), "exits were not");

        s.notify_open = true;
        s.notify_tickers = vec!["aapl".into()];
        assert!(deliverable(&s, "close", &aapl), "matched case-insensitively");
        assert!(!deliverable(&s, "close", &msft), "another instrument is filtered out");

        // A failure is about the session, not about an instrument: no filter applies.
        assert!(deliverable(&s, "error", &msft));
        assert!(deliverable(&s, "run", &msft));

        // A trade with no ticker cannot satisfy a named list.
        assert!(!deliverable(&s, "close", &json!({})));
    }

    /// Both periods come from one query and mean two different things. The catalog exposing
    /// them is worth nothing if `since` is not narrower than `total`.
    #[test]
    fn since_is_a_window_inside_total() {
        let s = session();
        let b = Book::sample(&s);
        let ctx = context(&s, "digest", &json!({}), &Tick::failed(String::new()), &b);
        let since = expr::lookup("since.trades", &ctx).unwrap();
        let total = expr::lookup("total.trades", &ctx).unwrap();
        assert!(since.as_i64() < total.as_i64());
        // A win rate is a percentage of that window, not of the whole book.
        let wr = expr::lookup("since.win_rate", &ctx).unwrap().as_f64().unwrap();
        assert!((wr - 50.0).abs() < 1e-9, "1 win of 2 trades is 50%, got {wr}");
        // The open position is readable both as a list and as a ready-made line.
        let summary = expr::lookup("open.summary", &ctx).unwrap();
        assert!(summary.as_str().unwrap().contains("AAPL"), "{summary}");
    }

    /// An empty template is not silence: the built-in wording carries the fill.
    #[test]
    fn default_wording_states_the_fill() {
        let s = session();
        let t = serde_json::to_value(trade("AAPL", "2026-09-04T10:00:00Z", "take_profit")).unwrap();
        let (title, body) = default_message(&s, "close", &t);
        assert!(title.contains("AAPL"), "{title}");
        assert!(body.contains("take_profit"), "{body}");
        let (title, body) = default_message(&s, "open", &t);
        assert!(title.contains("long"), "{title}");
        assert!(body.contains("100"), "{body}");
    }

    /// A crypto pair with no stated currency is quoted in what its symbol ends with.
    #[test]
    fn a_pair_is_quoted_in_what_its_symbol_ends_with() {
        assert_eq!(pair_quote("SOLUSDT"), Some("USDT"));
        assert_eq!(pair_quote("btcfdusd"), Some("FDUSD"));
        assert_eq!(pair_quote("ETHBTC"), Some("BTC"));
        assert_eq!(pair_quote("BTCUSD"), Some("USD"));
        assert_eq!(pair_quote("USDT"), None);
        assert_eq!(pair_quote("MNQ.20261218@CME"), None);
    }

    /// One reverse, one message: the close and the entry said together.
    #[test]
    fn a_reverse_is_one_message() {
        let mut s = session();
        let t = json!({
            "ticker": "SOLUSDT", "direction": "short", "order": "market", "pending": true,
            "signal_ts": "2026-11-17T08:44:00Z", "entry_price": 72.43, "qty": 8.17,
            "reverses": { "direction": "long", "entry_price": 72.87, "qty": 8.23, "exit_price": 72.43, "pnl": -3.62 },
        });
        let (title, body) = default_message(&s, "open", &t);
        assert!(title.contains("reversed long to short"), "{title}");
        assert!(body.contains("8.2300") && body.contains("-3.62") && body.contains("8.1700"), "{body}");
        // Said when the closes are asked for, even with the entries off.
        s.notify_open = false;
        s.notify_close = true;
        assert!(deliverable(&s, "open", &t));
        s.notify_close = false;
        assert!(!deliverable(&s, "open", &t));
    }

    fn sol_bars(close: &[f64]) -> backtest::OwnedBars {
        let n = close.len();
        backtest::OwnedBars {
            ticker: "SOLUSDT".into(),
            timeframe: "1m".into(),
            ts: (0..n).map(|i| format!("2026-11-17T08:0{i}:00Z")).collect(),
            open: vec![100.0; n],
            high: close.iter().map(|c| c.max(100.0) + 0.1).collect(),
            low: close.iter().map(|c| c.min(100.0) - 0.1).collect(),
            close: close.to_vec(),
            volume: vec![1.0; n],
            quotes: None,
            sub: None,
            script: None,
        }
    }

    fn held_row(direction: &str, entry: &str) -> store::Trade {
        store::Trade {
            session_id: Uuid::nil(),
            trade_key: format!("SOLUSDT|{direction}|{entry}"),
            ticker: "SOLUSDT".into(),
            direction: direction.into(),
            entry_ts: entry.into(),
            exit_ts: String::new(),
            open: true,
            pnl: 0.0,
            fees: 0.0,
            trade: json!({ "entry_price": 100.0, "qty": 1.0 }),
            opened_at: OffsetDateTime::UNIX_EPOCH,
            closed_at: None,
            robot_qty: Some(Decimal::ONE),
            scale: Decimal::ONE,
            robot_note: String::new(),
            robot_pnl: None,
            robot_fees: None,
            money_at_broker: false,
        }
    }

    /// The robot's both-side strategy, as stored (CrossBothSide), on plain levels: long above
    /// 100.5, short below 99.5, stop and reverse.
    fn both_sides() -> backtest::Settings {
        let level = |op: &str, v: f64| json!({ "logic": "all", "conditions": [{ "op": op, "left": { "kind": "price", "field": "close" }, "right": { "kind": "const", "value": v } }] });
        serde_json::from_value(json!({
            "kind": "signals", "mode": "both", "stop_and_reverse": true, "reverse_side": false,
            "starting_capital": 1000, "sizing": { "mode": "fixed_qty", "qty": 1 },
            "long": { "entry": level("above", 100.5), "exit": { "logic": "all", "conditions": [] }, "stop_loss_pct": 0, "take_profit_pct": 0, "exit_on_reverse": false },
            "short": { "entry": level("below", 99.5), "exit": { "logic": "all", "conditions": [] }, "stop_loss_pct": 0, "take_profit_pct": 0, "exit_on_reverse": false },
        }))
        .unwrap()
    }

    /// One tick the way `tick` runs the engine: the held positions carried, the last close's
    /// decisions replayed, only the last close deciding. Returns the run, the stats it stores
    /// and the markers of the entries it announced.
    fn robot_tick(bars: &backtest::OwnedBars, held: &[store::Trade], stats: Option<&Value>, live: &[store::LiveFill]) -> (backtest::RunResult, Value, Vec<store::LiveFill>, HashSet<(String, String)>) {
        let mut s = both_sides();
        let last = bars.ts.last().unwrap().clone();
        s.filters.trade_from = last.clone();
        s.filters.acts_from = last.clone();
        let assets = std::slice::from_ref(bars);
        let mut forced = vec![Vec::new()];
        let mut remap = HashMap::new();
        carry(assets, held, &mut forced, &mut remap);
        let (reversed, _) = decided(assets, stats, live, held, &mut forced);
        let b = bars.as_bars();
        let r = backtest::run_portfolio_forced(&s, &[&b], &forced);
        let mut v = serde_json::to_value(&r.stats).unwrap();
        v["pending_orders"] = json!(r.pending_orders);
        v["open_positions"] = json!(r.open_positions);
        v["closes"] = json!({ "SOLUSDT": last });
        let markers = r
            .pending_orders
            .iter()
            .filter(|o| o.kind == "entry" && o.order == "market")
            .map(|o| store::LiveFill {
                id: Uuid::new_v4(),
                session_id: Uuid::nil(),
                trade_key: format!("SOLUSDT|{}|signal:{last}", o.direction),
                ticker: "SOLUSDT".into(),
                kind: "signal".into(),
                direction: o.direction.clone(),
                reason: "market".into(),
                bar_ts: last.clone(),
                price: o.price,
                maker: false,
                quoted: false,
                at: OffsetDateTime::UNIX_EPOCH,
            })
            .collect();
        (r, v, markers, reversed)
    }

    fn trades_of(r: &backtest::RunResult) -> Vec<(String, String, String, String)> {
        r.trades.iter().map(|t| (t.direction.clone(), t.entry_ts.clone(), t.exit_ts.clone(), t.exit_reason.clone())).collect()
    }

    /// 2026-11-17 08:02 and 09:09: the provider revised the bar after the tick read it, the next
    /// run found a cross on it the robot never saw, and the book took a trade the broker never
    /// had. Two ticks on the same bars, the second one reading the revised close: the robot
    /// does what it decided at the close as it read it, never what the revision says.
    #[test]
    fn a_revised_bar_never_makes_the_robot_trade_what_it_did_not_decide() {
        let held = [held_row("long", "2026-11-17T08:01:00Z")];
        let t = |i: usize| format!("2026-11-17T08:0{i}:00Z");

        // Tick at the close of 08:04: nothing decided. The bar is then revised to a short signal.
        let (_, stats, markers, _) = robot_tick(&sol_bars(&[100.0, 100.0, 100.0, 100.0, 100.0]), &held, None, &[]);
        assert!(markers.is_empty());
        let (r, _, _, reversed) = robot_tick(&sol_bars(&[100.0, 100.0, 100.0, 100.0, 99.0, 100.0]), &held, Some(&stats), &markers);
        assert_eq!(trades_of(&r), vec![("long".into(), t(1), t(5), "end".into())], "the long is still held, never reversed");
        assert!(reversed.is_empty());

        // Tick at the close of 08:04: a reverse decided and announced. The bar is then revised
        // and no longer shows it: the reverse is done as decided, and its close was said with it.
        let (r, stats, markers, _) = robot_tick(&sol_bars(&[100.0, 100.0, 100.0, 100.0, 99.0]), &held, None, &[]);
        assert_eq!(r.pending_orders.iter().map(|o| (o.direction.as_str(), o.order.as_str())).collect::<Vec<_>>(), vec![("short", "market")]);
        assert_eq!(markers.len(), 1);
        let (r, _, _, reversed) = robot_tick(&sol_bars(&[100.0, 100.0, 100.0, 100.0, 100.0, 100.0]), &held, Some(&stats), &markers);
        assert_eq!(
            trades_of(&r),
            vec![("long".into(), t(1), t(5), "reverse".into()), ("short".into(), t(5), t(5), "end".into())]
        );
        assert!(reversed.contains(&("SOLUSDT".to_string(), "long".to_string())));
        assert_eq!(r.execution.forced_unmatched, 0);
    }

    /// Only the last close's decisions come back: an older announcement is history.
    #[test]
    fn only_the_last_close_is_replayed() {
        let bars = sol_bars(&[100.0; 6]);
        let marker = |bar_ts: &str| store::LiveFill {
            id: Uuid::new_v4(),
            session_id: Uuid::nil(),
            trade_key: format!("SOLUSDT|long|signal:{bar_ts}"),
            ticker: "SOLUSDT".into(),
            kind: "signal".into(),
            direction: "long".into(),
            reason: "market".into(),
            bar_ts: bar_ts.into(),
            price: 100.0,
            maker: false,
            quoted: false,
            at: OffsetDateTime::UNIX_EPOCH,
        };
        let stats = json!({ "closes": { "SOLUSDT": bars.ts[4] }, "pending_orders": [{ "ticker": "SOLUSDT", "kind": "entry", "direction": "long", "order": "market", "signal_ts": bars.ts[4], "qty": 2.5 }] });
        let mut forced = vec![Vec::new()];
        let (_, keys) = decided(std::slice::from_ref(&bars), Some(&stats), &[marker(&bars.ts[2]), marker(&bars.ts[4])], &[], &mut forced);
        assert_eq!(keys, HashSet::from([format!("SOLUSDT|long|{}", bars.ts[5])]));
        assert_eq!(forced[0].len(), 1);
        assert_eq!(forced[0][0].bar, 5);
        assert!(matches!(forced[0][0].kind, backtest::ForcedKind::Open { long: true, qty } if qty == 2.5));
        // Its next bar not stored yet: nothing to replay on.
        let short = sol_bars(&[100.0; 5]);
        let mut forced = vec![Vec::new()];
        decided(std::slice::from_ref(&short), Some(&stats), &[marker(&bars.ts[4])], &[], &mut forced);
        assert!(forced[0].is_empty());
    }
}
