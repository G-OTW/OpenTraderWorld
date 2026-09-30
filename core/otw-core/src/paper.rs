//! Paper trading: running a backtested strategy forward, tick by tick.
//!
//! **There is no second engine.** A tick reloads the session's datasets over its moving
//! window and calls the ordinary [`backtest::run_portfolio`], then diffs the trades it
//! produced against the ones already stored. A paper fill is therefore, by construction, the
//! fill the backtest would have shown for the same bars: no serialized simulator state to
//! drift, no parallel order logic to keep in sync, and a session opened from a result the
//! user is looking at reproduces that result exactly.
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
//! What is *not* delivered is still recorded: `paper_events` with `notified_at` NULL is the
//! offline inbox the page reads on the next login.

use std::collections::HashSet;

use serde_json::{json, Map, Value};
use time::OffsetDateTime;
use uuid::Uuid;

use otw_store::automator::Schedule;
use otw_store::connectors as conn_store;
use otw_store::histdata as hd;
use otw_store::paper as store;
use otw_store::reminders::FiredNotification;

use crate::automator::expr;
use crate::backtest::{self, Settings, Trade};
use crate::backtest_api::load_assets;
use crate::{notif_send, AppState};

/// Events kept per session. A paper session is long-lived; its log is a tail, not an archive.
const EVENT_KEEP: i64 = 500;
/// Bars fed to the engine when the session pins neither a trailing count nor a start.
const DEFAULT_WINDOW_BARS: i32 = 5_000;
/// Hard cap on what one tick loads, mirroring the backtest endpoint's own clamp.
const MAX_BARS: i64 = 200_000;
/// Seconds after a period boundary at which a session runs: none. A tick reads the bar that
/// just closed, so it fires the second it closes. The publication delay is absorbed by
/// [`top_up_dataset`], which re-asks for a moment rather than planning late for everyone.
const PUBLISH_LAG_SECS: i64 = 0;
/// How long a top-up waits for the bar that just closed, and how often it re-asks. A provider
/// writes it within a second or so; asking twice costs one request, planning the whole session
/// a few seconds late costs every tick.
const PUBLISH_WAIT: std::time::Duration = std::time::Duration::from_millis(700);
const PUBLISH_TRIES: usize = 4;
/// How far back a top-up reaches on a dataset that has never been downloaded, or one left
/// behind for days. A session is a live feed, not a backfill: the rest is a download job.
const TOP_UP_BARS: i64 = 300;

/// The module id used for channel grants and for `dispatch`. Paper sessions are part of
/// Backtest, and the channel screen labels a grant from the module registry, so the
/// producer id is the module the user sees, not the feature's own name.
pub const MODULE: &str = "backtest";

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

/// The next occurrence after now, or None for a rule that is exhausted (a `once` in the past).
pub fn next_occurrence(s: &store::Session) -> Option<OffsetDateTime> {
    crate::automator::schedule::next_from_now(&rule(s)).map(|at| on_bar_grid(s, at))
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
fn inputs_hash(s: &store::Session, freshness: &[(Uuid, OffsetDateTime, i64)]) -> String {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut h = DefaultHasher::new();
    s.settings.to_string().hash(&mut h);
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
    let settings: Settings = match serde_json::from_value(session.settings.clone()) {
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
    let hash = inputs_hash(session, &freshness);
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
    let assets = match load_assets(state, &session.dataset_ids, limit, session.start_ts, None).await
    {
        Ok(a) => a,
        Err(e) => return Tick::failed(e.message().to_string()),
    };
    // The engine is synchronous and CPU-bound: it runs on a blocking thread, never on a
    // runtime one, or a long session would stall every other request on this worker.
    let simulated = tokio::task::spawn_blocking(move || {
        let bars_total: usize = assets.iter().map(|a| a.ts.len()).sum();
        let bars: Vec<_> = assets.iter().map(|a| a.as_bars()).collect();
        let bar_refs: Vec<_> = bars.iter().collect();
        (backtest::run_portfolio(&settings, &bar_refs), bars_total)
    })
    .await;
    let (result, bars_total) = match simulated {
        Ok(v) => v,
        Err(e) => return Tick::failed(format!("the simulation did not finish: {e}")),
    };

    let previously_open: HashSet<String> = match store::open_keys(&state.pool, session.id).await {
        Ok(k) => k.into_iter().collect(),
        Err(e) => return Tick::failed(format!("reading the paper book: {e}")),
    };

    let mut opened = Vec::new();
    let mut closed = Vec::new();
    // Every key this run produced. The book is a projection of the current simulation, so
    // whatever is not in here has left the window and is pruned below.
    let mut seen: Vec<String> = Vec::with_capacity(result.trades.len());

    for t in &result.trades {
        // The engine closes whatever is still open at the last bar with reason "end". That
        // row is the live position, marked to that close, not a completed round trip.
        let is_open = t.exit_reason == "end";
        let key = trade_key(t);
        let payload = serde_json::to_value(t).unwrap_or_else(|_| json!({}));
        let was_open = previously_open.contains(&key);
        seen.push(key.clone());
        let inserted = match store::upsert_trade(
            &state.pool,
            session.id,
            &key,
            &t.ticker,
            &t.direction,
            &t.entry_ts,
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
        // A brand-new row that is already closed is a round trip the session never saw open
        // (the first tick over history, or a bar that opened and closed a position at once):
        // it is reported as a close, since that is the event that happened.
        if inserted && is_open {
            opened.push(payload.clone());
        }
        if !is_open && (was_open || inserted) {
            closed.push(payload);
        }
    }

    if let Err(e) = store::prune_open_trades(&state.pool, session.id, &seen).await {
        tracing::error!("paper: pruning the book: {e:#}");
    }

    Tick {
        skipped: false,
        bars: bars_total,
        inputs_hash: hash,
        stats: Some(serde_json::to_value(&result.stats).unwrap_or_else(|_| json!({}))),
        opened,
        closed,
        error: String::new(),
    }
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
        if let Err(e) = top_up_dataset(state, &ds, now).await {
            tracing::warn!("paper: topping up {} {}: {e:#}", ds.ticker, ds.timeframe);
        }
    }
}

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
    ds: &hd::Dataset,
    now: OffsetDateTime,
) -> anyhow::Result<()> {
    let tf = crate::histdata::timeframe_secs(&ds.timeframe)?;
    let running = OffsetDateTime::from_unix_timestamp(now.unix_timestamp().div_euclid(tf) * tf)?;
    let last_closed = running - time::Duration::seconds(tf);
    let floor = running - time::Duration::seconds(tf * TOP_UP_BARS);
    let from = match ds.range_to {
        Some(last) => (last + time::Duration::seconds(1)).max(floor),
        None => floor,
    };
    if from >= running {
        return Ok(());
    }
    // Only an instrument that was still printing bars one period ago is worth waiting for.
    let live = ds.range_to.is_some_and(|last| last >= last_closed - time::Duration::seconds(tf));
    let tries = if live { PUBLISH_TRIES } else { 1 };

    let connector = crate::histdata::connector_for(&ds.provider)?;
    let conn = conn_store::default_for(&state.pool, &ds.provider).await?;
    let secrets = match &conn {
        Some(c) => conn_store::load_creds(&state.pool, &state.cipher, c.id).await?,
        None => Default::default(),
    };
    let mut bars = Vec::new();
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
        bars = chunk.bars.into_iter().filter(|b| b.ts >= from && b.ts < running).collect();
        // The bar the tick came for is in: nothing to wait for.
        if bars.last().is_some_and(|b| b.ts >= last_closed) {
            break;
        }
    }
    if bars.is_empty() {
        return Ok(());
    }
    hd::write_bars(&state.pool, ds.id, &bars).await?;
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
    let capital = session.settings.get("starting_capital").and_then(Value::as_f64).unwrap_or(0.0);
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
    match event {
        "open" => (
            format!("{}: {} {}", session.name, s("direction"), ticker),
            format!(
                "Entry {:.4} for {:.4} units at {}.",
                f("entry_price"),
                f("qty"),
                s("entry_ts")
            ),
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
    let pending = match store::undelivered(&state.pool, session.id).await {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("paper: reading undelivered events: {e:#}");
            return;
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

    // The summary is the user's own dial and reports the period, so it is sent on its
    // schedule whether or not a fill just went out. `each` means the fills *are* the report.
    if session.cadence == "each" || !due_for_digest(session, now) {
        return;
    }
    // A period with nothing in it is a message worth sending only to someone who asked to
    // hear from the session on every run.
    if book.since.trades == 0 && session.notify_when != "always" {
        return;
    }
    let (title, body) = render_digest(state, session, book, false).await;
    if push(state, session, title, body).await {
        if let Err(e) = store::set_digest_mark(&state.pool, session.id, now).await {
            tracing::error!("paper: advancing the digest watermark: {e:#}");
        }
    }
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

/// Whether the summary window has elapsed. It gates the period report alone: fills are never
/// held back, so a one-minute rule stays a one-minute alert with an hourly recap over it.
fn due_for_digest(session: &store::Session, now: OffsetDateTime) -> bool {
    let span = match session.cadence.as_str() {
        "hourly" => time::Duration::hours(1),
        "daily" => time::Duration::days(1),
        "weekly" => time::Duration::weeks(1),
        _ => return true,
    };
    match session.last_digest_at {
        Some(last) => now - last >= span,
        None => true,
    }
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
    if session.notify_when == "always" && !t.changed() && t.error.is_empty() {
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
    let next = if session.status == "active" { next_occurrence(session) } else { None };
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

    // The book is re-read for delivery: the seed just stamped its boundary, and the events
    // this tick recorded are part of what the digest reports.
    let book = self::book(state, session).await;
    deliver(state, session, now, &book).await;
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

    fn session() -> store::Session {
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
        }
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
    fn inputs_hash_follows_the_data() {
        let s = session();
        let id = Uuid::from_u128(7);
        let t0 = OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap();
        let base = inputs_hash(&s, &[(id, t0, 1000)]);
        assert_eq!(base, inputs_hash(&s, &[(id, t0, 1000)]));
        // A download landed: same dataset, later stamp, more bars.
        let t1 = OffsetDateTime::from_unix_timestamp(1_700_000_060).unwrap();
        assert_ne!(base, inputs_hash(&s, &[(id, t1, 1060)]));
        // The strategy was edited.
        let mut edited = session();
        edited.settings = json!({ "kind": "grid" });
        assert_ne!(base, inputs_hash(&edited, &[(id, t0, 1000)]));
    }

    /// The watermark paces the recap alone: a fill is never waiting behind it.
    #[test]
    fn the_summary_window_paces_the_recap() {
        let now = OffsetDateTime::now_utc();
        let mut s = session();
        assert!(due_for_digest(&s, now), "cadence `each` always sends");

        s.cadence = "hourly".into();
        s.last_digest_at = Some(now - time::Duration::minutes(20));
        assert!(!due_for_digest(&s, now));
        s.last_digest_at = Some(now - time::Duration::minutes(61));
        assert!(due_for_digest(&s, now));

        s.cadence = "daily".into();
        s.last_digest_at = None;
        assert!(due_for_digest(&s, now), "never sent yet ⇒ due");
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
}
