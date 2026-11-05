//! The paper broker's matching engine: one watch per instrument a paper account has working
//! orders or open positions on, on the live hub, the way a chart alert rides it.
//!
//! A watch follows the instrument's bars (the provider's finest live timeframe) and, when the
//! feed publishes one, its book. Each print updates the instrument's mark (what positions are
//! valued at) and is matched against the instrument's working orders ([`super::matching`]).
//! A fill is checked against the account (shorts, buying power) at the price it meets, written
//! with its result, and told to whoever follows the account.
//!
//! The engine runs with the app: while it is down nothing fills, and orders resume on the first
//! print after it is back, the way a venue's book resumes after a halt. A print older than
//! [`STALE`] when a watch starts is not a price to deal on, so a market order sent while the
//! market is closed waits for it to open.
//!
//! A print comes off the hub as a float and is read once as the decimal it shows; matching,
//! the fill and the ledger are decimal from there.

#![deny(clippy::float_arithmetic)]

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Result};
use otw_connector_model::Decimal;
use time::OffsetDateTime;
use tokio::sync::{broadcast::error::RecvError, Notify};
use tokio::task::JoinHandle;
use uuid::Uuid;

use otw_store::brokers as accounts;
use otw_store::connectors as conn_store;
use otw_store::paper_broker::{self as store, Order};

use super::matching::{decide, Resting, Tick, Trail};
use super::{exact, native_id, publish, set_mark, shown, Settings, BOOK_FRESH, ID};
use crate::brokers::AccountEvent;
use crate::live::hub::{LiveMsg, LiveTarget};
use crate::AppState;

/// Reconciler period, on top of the poke every order change sends.
const RECONCILE: Duration = Duration::from_secs(10);
/// Wait before the first pass, so a restart does not race the migrations and the connectors.
const WARMUP: Duration = Duration::from_secs(15);
/// A bar this old when a watch starts is the last session's, not the market now.
const STALE: Duration = Duration::from_secs(120);
/// Wait before joining a feed the hub restarted.
const REJOIN: Duration = Duration::from_millis(250);
const FEED_CLOSED: &str = "the live feed closed";

static POKE: Notify = Notify::const_new();

/// The orders changed: look at what to watch now.
pub fn poke() {
    POKE.notify_one();
}

/// The finest timeframe a provider streams, the one a watch follows.
fn stream_timeframe(provider: &str) -> Option<String> {
    let cap = crate::histdata::capabilities().into_iter().find(|c| c.provider == provider)?;
    if cap.stream_timeframes.contains(&"1m") {
        return Some("1m".into());
    }
    cap.stream_timeframes.first().map(|t| t.to_string())
}

/// Why an instrument has no live price to fill on, if it has none.
pub fn live_refusal(provider: &str, asset_type: &str) -> Option<String> {
    let Some(tf) = stream_timeframe(provider) else {
        return Some(format!("{provider} has no live feed"));
    };
    crate::live::stream_refusal(provider, asset_type, &tf)
}

/// One instrument on one connector: what a watch follows.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Feed {
    connector_id: Uuid,
    provider: String,
    asset_type: String,
    ticker: String,
}

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        tokio::time::sleep(WARMUP).await;
        let mut running: HashMap<Feed, JoinHandle<()>> = HashMap::new();
        // Feeds whose failure was already logged, so a dead connector is said once.
        let mut told: HashSet<Feed> = HashSet::new();
        loop {
            if let Err(e) = reconcile(&state, &mut running, &mut told).await {
                tracing::warn!("paper broker: {e:#}");
            }
            tokio::select! {
                _ = tokio::time::sleep(RECONCILE) => {}
                _ = POKE.notified() => {}
            }
        }
    });
}

/// Expire what is due, then start the watches wanted and stop the others.
async fn reconcile(state: &AppState, running: &mut HashMap<Feed, JoinHandle<()>>, told: &mut HashSet<Feed>) -> Result<()> {
    let expired = store::expire_due(&state.pool, OffsetDateTime::now_utc()).await?;
    if !expired.is_empty() {
        for (account, ids) in &expired {
            super::tell_done(*account, ids, "expired");
        }
        super::changed();
    }
    let mut wanted: HashSet<Feed> = HashSet::new();
    for o in store::working(&state.pool).await? {
        wanted.insert(Feed { connector_id: o.connector_id, provider: o.provider, asset_type: o.asset_type, ticker: o.ticker });
    }
    // Open positions are watched too: their mark is what the account is worth.
    for a in accounts::list(&state.pool).await?.into_iter().filter(|a| a.broker == ID) {
        for h in super::holdings(&store::all_fills(&state.pool, a.id).await?) {
            wanted.insert(Feed { connector_id: h.connector_id, provider: h.provider, asset_type: h.asset_type, ticker: h.ticker });
        }
    }
    running.retain(|feed, task| {
        let keep = wanted.contains(feed) && !task.is_finished();
        if !keep {
            task.abort();
        }
        keep
    });
    for feed in wanted {
        if running.contains_key(&feed) {
            continue;
        }
        let first = told.insert(feed.clone());
        let (st, f) = (state.clone(), feed.clone());
        running.insert(
            feed,
            tokio::spawn(async move {
                let mut said = !first;
                loop {
                    let Err(e) = watch(&st, &f).await else { return };
                    if !said {
                        tracing::warn!("paper broker: {} {}: {e:#}", f.provider, f.ticker);
                        said = true;
                    }
                    // A feed the hub restarted (a chart began recording it) is back at once:
                    // join it again now, orders are waiting. Anything else is retried slower.
                    let closed = e.to_string() == FEED_CLOSED;
                    tokio::time::sleep(if closed { REJOIN } else { RECONCILE }).await;
                }
            }),
        );
    }
    told.retain(|f| running.contains_key(f));
    Ok(())
}

/// The working orders of one feed, with the settings of their accounts.
struct Book {
    version: u64,
    orders: Vec<Order>,
    accounts: HashMap<Uuid, (accounts::BrokerRow, Settings)>,
}

async fn read_book(state: &AppState, feed: &Feed) -> Result<Book> {
    let version = super::version();
    let orders: Vec<Order> = store::working(&state.pool)
        .await?
        .into_iter()
        .filter(|o| o.connector_id == feed.connector_id && o.provider == feed.provider && o.asset_type == feed.asset_type && o.ticker == feed.ticker)
        .collect();
    let mut accts = HashMap::new();
    for id in orders.iter().map(|o| o.account_id).collect::<HashSet<_>>() {
        if let Some(a) = accounts::get(&state.pool, id).await? {
            let s = Settings::of(&a)?;
            accts.insert(id, (a, s));
        }
    }
    Ok(Book { version, orders, accounts: accts })
}

/// Follow one instrument until it has nothing left to fill or value.
async fn watch(state: &AppState, feed: &Feed) -> Result<()> {
    let conn = conn_store::get(&state.pool, feed.connector_id)
        .await?
        .ok_or_else(|| anyhow!("its data connector was deleted: the orders on it cannot fill"))?;
    let timeframe = stream_timeframe(&feed.provider).ok_or_else(|| anyhow!("{} has no live feed", feed.provider))?;
    let target = LiveTarget {
        // Watching is not recording.
        dataset_id: None,
        connector_id: conn.id,
        connector: conn.name.clone(),
        provider: feed.provider.clone(),
        asset_type: feed.asset_type.clone(),
        ticker: feed.ticker.clone(),
        timeframe,
    };
    let quote_target = crate::live::quote_source(&feed.provider, &feed.asset_type, &feed.ticker)
        .map(|_| LiveTarget { dataset_id: None, timeframe: crate::live::QUOTE_TF.to_string(), ..target.clone() });
    let bars = state.live.subscribe(target)?;
    if bars.status.state == "stopped" {
        bail!(bars.status.message.unwrap_or_else(|| "the live feed stopped".into()));
    }
    let (mut rx, _guard) = (bars.rx, bars.guard);
    let (mut qrx, _qguard, first_quote) = match quote_target.map(|t| state.live.subscribe(t)).transpose()? {
        Some(q) => (Some(q.rx), Some(q.guard), q.quote),
        None => (None, None, None),
    };
    let native = native_id(&feed.provider, &feed.asset_type, &feed.ticker);
    let mut book = read_book(state, feed).await?;
    let mut quote: Option<(Decimal, Decimal, Instant)> = first_quote.and_then(|q| Some((exact(q.bid)?, exact(q.ask)?, Instant::now())));
    if let Some((bid, ask, at)) = quote {
        set_mark(&native, |m| super::Mark { last: m.map(|m| m.last).unwrap_or((bid + ask) / Decimal::TWO), at: m.map(|m| m.at).unwrap_or(at), book: Some((bid, ask, at)) });
    }
    // The forming bar's extremes already seen: only what prints after the watch starts can
    // fill, so the first event counts its last price alone. A stale bar is not a price.
    let mut seen: Option<(OffsetDateTime, Decimal, Decimal)> = None;
    let mut bests: HashMap<Uuid, Decimal> = HashMap::new();
    let mut first = bars.snapshot.filter(|e| {
        let age = OffsetDateTime::now_utc() - e.provider_ts.max(e.bar_ts);
        age.whole_seconds() >= 0 && age.unsigned_abs() < STALE
    });
    loop {
        let tick = if let Some(e) = first.take() {
            let (Some(close), Some(high), Some(low)) = (exact(e.close), exact(e.high), exact(e.low)) else { continue };
            seen = Some((e.bar_ts, high, low));
            set_mark(&native, |m| super::Mark { last: close, at: Instant::now(), book: m.and_then(|m| m.book) });
            Tick { lo: close, hi: close, last: close, book: None }
        } else {
            let msg = match &mut qrx {
                Some(q) => tokio::select! {
                    m = rx.recv() => m,
                    m = q.recv() => m,
                },
                None => rx.recv().await,
            };
            match msg {
                Ok(LiveMsg::Bar(e)) => {
                    let (Some(close), Some(high), Some(low)) = (exact(e.close), exact(e.high), exact(e.low)) else { continue };
                    let (lo, hi) = match seen {
                        Some((ts, h, l)) if ts == e.bar_ts => (if low < l { low } else { close }, if high > h { high } else { close }),
                        Some(_) => (low, high),
                        None => (close, close),
                    };
                    seen = Some((e.bar_ts, high, low));
                    set_mark(&native, |m| super::Mark { last: close, at: Instant::now(), book: m.and_then(|m| m.book) });
                    Tick { lo, hi, last: close, book: None }
                }
                Ok(LiveMsg::Quote(q)) => {
                    let (Some(bid), Some(ask)) = (exact(q.bid), exact(q.ask)) else { continue };
                    let now = Instant::now();
                    quote = Some((bid, ask, now));
                    let mid = (bid + ask) / Decimal::TWO;
                    set_mark(&native, |m| super::Mark { last: m.map(|m| m.last).unwrap_or(mid), at: m.map(|m| m.at).unwrap_or(now), book: Some((bid, ask, now)) });
                    let last = super::mark(&native).map(|m| m.last).unwrap_or(mid);
                    Tick { lo: last, hi: last, last, book: None }
                }
                Ok(LiveMsg::Status(s)) if s.state == "stopped" => {
                    bail!(s.message.unwrap_or_else(|| "the live feed stopped".into()))
                }
                Ok(LiveMsg::Status(_)) | Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => bail!(FEED_CLOSED),
            }
        };
        if book.version != super::version() {
            book = read_book(state, feed).await?;
        }
        let fresh = quote.filter(|(_, _, at)| at.elapsed() < BOOK_FRESH).map(|(b, a, _)| (b, a));
        let mut touched = false;
        for o in &book.orders {
            let Some((account, settings)) = book.accounts.get(&o.account_id) else { continue };
            let t = Tick { book: if settings.quotes { fresh } else { None }, ..tick };
            let trail = match (o.trail_amount, o.trail_pct) {
                (Some(a), _) => Some(Trail::Amount(a)),
                (None, Some(p)) => Some(Trail::Percent(p)),
                _ => None,
            };
            let best = bests.get(&o.id).copied().or(o.trail_best);
            let resting = Resting { buy: o.buy(), kind: &o.order_type, limit: o.limit_price, stop: o.stop_price, triggered: o.triggered, trail, best };
            let d = decide(&resting, &t, settings.slippage_bps);
            if d.trigger {
                store::trigger(&state.pool, o.id).await?;
                touched = true;
            }
            // Kept here and in the store, so a restart trails on from where it was, without
            // reading the book again on every tick.
            if let Some(b) = d.best.filter(|b| Some(*b) != best) {
                store::set_trail_best(&state.pool, o.id, b).await?;
                bests.insert(o.id, b);
            }
            match d.fill {
                Some(px) => {
                    execute(state, account, settings, o, px).await?;
                    touched = true;
                }
                // Paper fills an order whole, so fill-or-kill and immediate-or-cancel end alike.
                None if o.tif == "ioc" || o.tif == "fok" => {
                    let why = "nothing to fill it at once";
                    let ids = store::stop(&state.pool, o.account_id, o.id, "cancelled", Some(why)).await?;
                    super::tell_done_why(o.account_id, &ids, "cancelled", Some(why));
                    touched = true;
                }
                None => {}
            }
        }
        if touched {
            super::changed();
            book = read_book(state, feed).await?;
        }
    }
}

/// Fill `o` at `px` for its account, or reject it with the reason the account refuses it.
async fn execute(state: &AppState, account: &accounts::BrokerRow, s: &Settings, o: &Order, px: Decimal) -> Result<()> {
    let px = match o.tick {
        // A fill lands on a price the instrument trades at: the nearest tick, half a tick up.
        Some(t) if t > Decimal::ZERO => {
            let n = (px / t + Decimal::new(5, 1)).floor();
            let snapped = n * t;
            if snapped > Decimal::ZERO {
                snapped
            } else {
                px
            }
        }
        _ => px,
    };
    // Snapping to the nearest tick must never fill a limit worse than its limit: a buy never
    // pays above it, a sell never receives below it.
    let px = match o.limit_price {
        Some(l) if o.buy() && px > l => l,
        Some(l) if !o.buy() && px < l => l,
        _ => px,
    };
    let fee = s.commission.fee(o.qty, px, o.multiplier);
    let fx = match super::fx(&state.pool, &o.currency, &s.currency).await {
        Ok(r) => r,
        Err(e) => return reject(state, o, &format!("{e}")).await,
    };
    if let Err(why) = super::allows(&state.pool, account, o, px, fee).await? {
        return reject(state, o, &why).await;
    }
    let done = store::fill(&state.pool, o.id, px, fee, fx, OffsetDateTime::now_utc()).await?;
    if let Some(why) = done.cancelled_why {
        super::tell_done_why(o.account_id, &done.cancelled, "cancelled", Some(why));
        return Ok(());
    }
    let Some(fill) = done.fill else { return Ok(()) };
    publish(o.account_id, AccountEvent::Filled { id: o.id.to_string(), filled_qty: shown(fill.qty) });
    super::tell_done(o.account_id, &[o.id], "filled");
    for child in &done.activated {
        publish(o.account_id, AccountEvent::Order { order: super::open_order(child), key: Some(child.key.clone()) });
    }
    super::tell_done(o.account_id, &done.cancelled, "cancelled");
    let native = native_id(&o.provider, &o.asset_type, &o.ticker);
    let held = super::holdings(&store::all_fills(&state.pool, o.account_id).await?);
    let position = match held.iter().find(|h| h.native == native) {
        Some(h) => h.position(),
        // Flat: a zero quantity drops the row.
        None => crate::brokers::Position {
            symbol: o.ticker.clone(),
            side: "long".into(),
            qty: 0.0,
            avg_price: None,
            currency: o.currency.clone(),
            asset_class: String::new(),
            multiplier: shown(o.multiplier),
            venue: o.provider.clone(),
            account: String::new(),
            native_id: Some(native),
            market_price: None,
            unrealized_pnl: None,
            opened_at: None,
            entry_fee: None,
            fee_currency: o.currency.clone(),
            leverage: None,
            margin_mode: None,
        },
    };
    publish(o.account_id, AccountEvent::Position { position });
    Ok(())
}

async fn reject(state: &AppState, o: &Order, why: &str) -> Result<()> {
    let ids = store::stop(&state.pool, o.account_id, o.id, "rejected", Some(why)).await?;
    super::tell_done_why(o.account_id, &ids, "rejected", Some(why));
    tracing::info!("paper broker: order {} on {} rejected: {why}", o.id, o.ticker);
    Ok(())
}
