//! What a robot owns over the strategy it runs.
//!
//! The strategy gives the signals: its entries and exits, its stop and its target, and the
//! order types it trades with. The robot gives the money and the calendar: its allocation is
//! the capital the engine starts from, its share in one position the size, its leverage and
//! its entries per position; its exclusions replace the strategy's day, session and date
//! filters. The instrument (point value, price tick, size step, minimum) is the venue's, and
//! the costs are what the broker charges: the strategy's own values for both are not read.
//!
//! [`apply`] is pure; [`venue_of`] reads the facts it needs.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use rust_decimal::prelude::ToPrimitive;
use uuid::Uuid;

use otw_store::histdata as hd;
use otw_store::robots as store;

use super::sizing::Capital;
use crate::backtest::{Fees, Funding, Instrument, Settings, Sizing, Slippage};
use crate::AppState;

/// The instrument as the venue states it. `None`: the venue says nothing of it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Venue {
    pub multiplier: Option<f64>,
    pub tick: Option<f64>,
    pub lot_step: Option<f64>,
    pub min_qty: Option<f64>,
}

impl Venue {
    /// What makes a re-run worth doing when it changes.
    pub fn key(&self) -> String {
        format!("{:?}", self)
    }
}

/// The settings the engine runs for this robot: the strategy's signals, the robot's money and
/// calendar, the venue's instrument, no cost of the strategy's own.
pub fn apply(s: &mut Settings, c: &Capital, on_broker: bool, venue: &Venue) {
    let f = |d: rust_decimal::Decimal| d.to_f64().unwrap_or_default();
    // An empty allocation is the whole account: the strategy's capital and size stand in for
    // it, the only figure there is to start from.
    if let Some(a) = c.allocation {
        let percent = c.percent_in.map(f).unwrap_or(100.0);
        s.starting_capital = f(a);
        s.sizing = Sizing::PercentEquity { percent };
        // A grid spreads the whole allocation over its levels: each level is one position.
        if let Some(g) = s.grid.as_mut() {
            g.total_budget = f(a);
            g.qty_per_level = 0.0;
        }
    }
    s.leverage = c.leverage.map(f).unwrap_or(1.0);
    // A broker robot sends one entry per position, whatever it is set to.
    s.pyramiding = if on_broker { 1 } else { c.pyramiding.unwrap_or(1) as usize };
    // The robot's exclusions say when it leaves the market; a window the strategy closes must
    // not flatten a position the robot holds.
    s.filters.on_window_end = "hold".into();
    s.filters.weekdays.clear();
    s.filters.sessions.clear();
    s.filters.include_dates.clear();
    s.filters.exclude_dates.clear();
    s.fees = Fees::default();
    s.slippage = Slippage::default();
    s.spread_pct = 0.0;
    s.funding = Funding::default();
    s.instrument = Instrument {
        multiplier: venue.multiplier.filter(|m| *m > 0.0).unwrap_or(1.0),
        lot_step: venue.lot_step.filter(|v| *v > 0.0).unwrap_or(0.0),
        min_qty: venue.min_qty.filter(|v| *v > 0.0).unwrap_or(0.0),
        tick_size: venue.tick.filter(|v| *v > 0.0).unwrap_or(0.0),
    };
}

/// The robot's settings as the engine runs them ([`apply`] over the frozen strategy), with the
/// venue's facts they were built on.
pub async fn settings_of(state: &AppState, session: &store::Session) -> anyhow::Result<(Settings, Venue)> {
    let mut s: Settings = serde_json::from_value(session.settings.clone())?;
    let venue = venue_of(state, session).await;
    apply(&mut s, &Capital::of(&session.capital), session.on_broker(), &venue);
    Ok((s, venue))
}

/// How long a broker's grid is trusted before it is asked again.
const GRID_TTL: Duration = Duration::from_secs(3600);
static GRIDS: LazyLock<Mutex<HashMap<(Uuid, String), (Instant, Option<crate::brokers::Grid>)>>> = LazyLock::new(Default::default);

/// The venue's facts for this robot's instrument. One instrument only: a basket shares one
/// instrument setting, so it runs on the defaults (point value 1, any size).
///
/// The point value and the price tick are the instrument's facts. The size step and the
/// minimum are the broker account's, for a robot on one; a simulated robot on an exchange
/// future trades whole contracts, and elsewhere the venue's public terms when the provider is
/// also a broker that serves them without a key (Binance), else any size.
pub async fn venue_of(state: &AppState, session: &store::Session) -> Venue {
    let [id] = session.dataset_ids.as_slice() else { return Venue::default() };
    let Ok(Some(ds)) = hd::get_dataset(&state.pool, *id).await else { return Venue::default() };
    let meta = crate::instrument_meta_api::resolved_or_stored(&state.pool, &ds.provider, &ds.asset_type, &ds.ticker).await.ok().flatten();
    let mut v = Venue {
        multiplier: meta.as_ref().and_then(|m| m.pointvalue),
        tick: meta.as_ref().and_then(|m| m.mintick),
        ..Default::default()
    };
    match session.exec_account_id.filter(|_| session.on_broker()) {
        Some(account) => {
            if let Some(g) = grid(state, account, &ds).await {
                v.lot_step = g.market_step.or(g.lot_step).and_then(|d| d.to_f64());
                v.min_qty = g.market_min_qty.or(g.min_qty).and_then(|d| d.to_f64());
                if v.tick.is_none() {
                    v.tick = g.tick.and_then(|d| d.to_f64());
                }
            }
        }
        None if ds.asset_type == "future" => {
            v.lot_step = Some(1.0);
            v.min_qty = Some(1.0);
        }
        None => {
            if let Some(g) = public_grid(&ds).await {
                v.lot_step = g.market_step.or(g.lot_step).and_then(|d| d.to_f64());
                v.min_qty = g.market_min_qty.or(g.min_qty).and_then(|d| d.to_f64());
                if v.tick.is_none() {
                    v.tick = g.tick.and_then(|d| d.to_f64());
                }
            }
        }
    }
    v
}

/// The broker's grid for one instrument, kept an hour. A failed read is not kept: the next
/// tick asks again.
async fn grid(state: &AppState, account: Uuid, ds: &hd::Dataset) -> Option<crate::brokers::Grid> {
    let key = (account, format!("{}:{}", ds.provider, ds.ticker));
    let kept = GRIDS.lock().ok().and_then(|m| m.get(&key).filter(|(at, _)| at.elapsed() < GRID_TTL).map(|(_, g)| g.clone()));
    if let Some(g) = kept {
        return g;
    }
    let (connector, settings, _) = crate::brokers_api::resolve_for_job(&state.pool, &state.cipher, account, super::MODULE).await.ok()?;
    let g = connector.grid(&settings, Some(&ds.provider), &ds.ticker, &ds.asset_type).await.ok()?;
    if let Ok(mut m) = GRIDS.lock() {
        m.insert(key, (Instant::now(), g.clone()));
    }
    g
}

/// Brokers that serve an instrument's terms without a key, under the data provider's own id.
const PUBLIC_TERMS: [&str; 2] = ["binance", "binance_futures"];

/// The venue's public terms for a simulated robot's instrument ([`PUBLIC_TERMS`]), kept an hour
/// like an account's. Any other provider: nothing, the robot runs on any size.
async fn public_grid(ds: &hd::Dataset) -> Option<crate::brokers::Grid> {
    if !PUBLIC_TERMS.contains(&ds.provider.as_str()) {
        return None;
    }
    let key = (Uuid::nil(), format!("{}:{}", ds.provider, ds.ticker));
    let kept = GRIDS.lock().ok().and_then(|m| m.get(&key).filter(|(at, _)| at.elapsed() < GRID_TTL).map(|(_, g)| g.clone()));
    if let Some(g) = kept {
        return g;
    }
    let connector = crate::brokers::broker_for(&ds.provider).ok()?;
    let g = connector.grid(&HashMap::new(), Some(&ds.provider), &ds.ticker, &ds.asset_type).await.ok()?;
    if let Ok(mut m) = GRIDS.lock() {
        m.insert(key, (Instant::now(), g.clone()));
    }
    g
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn strategy() -> Settings {
        serde_json::from_value(json!({
            "kind": "signals",
            "mode": "both",
            "long": { "entry": { "logic": "all", "conditions": [] }, "exit": { "logic": "all", "conditions": [] }, "stop_loss_pct": 0.02 },
            "short": { "entry": { "logic": "all", "conditions": [] }, "exit": { "logic": "all", "conditions": [] } },
            "pyramiding": 3,
            "sizing": { "mode": "percent_equity", "percent": 10 },
            "starting_capital": 1000,
            "leverage": 4,
            "spread_pct": 0.05,
            "fees": { "per": "trade", "amount": 0.1, "amount_kind": "pct" },
            "slippage": { "kind": "pct", "value": 0.2, "tick_size": 0 },
            "instrument": { "multiplier": 1, "lot_step": 0, "min_qty": 0, "tick_size": 0 },
            "filters": { "weekdays": [1, 2], "sessions": [], "include_dates": [], "exclude_dates": [], "on_window_end": "hold", "block_adds": true, "tz_offset_min": 0 }
        }))
        .unwrap()
    }

    fn mnq() -> Venue {
        Venue { multiplier: Some(2.0), tick: Some(0.25), lot_step: Some(1.0), min_qty: Some(1.0) }
    }

    /// IBKRPaper: allocation 100 000 all in one position on MNQ. The strategy's 1000 at 10%
    /// made 0.0035 of a contract; the robot's money and the contract make whole contracts.
    #[test]
    fn the_robot_sizes_on_its_allocation_and_the_venue_contract() {
        let mut s = strategy();
        let c = Capital::validate(&json!({ "allocation": "100000", "percent_in": "100" })).unwrap();
        apply(&mut s, &c, true, &mnq());
        assert_eq!(s.starting_capital, 100_000.0);
        assert!(matches!(s.sizing, Sizing::PercentEquity { percent } if percent == 100.0));
        assert_eq!((s.instrument.multiplier, s.instrument.lot_step, s.instrument.min_qty, s.instrument.tick_size), (2.0, 1.0, 1.0, 0.25));
        // 100 000 of notional at 28 316.5 x 2 per point: one whole contract.
        let notional_per_contract = 28_316.5 * s.instrument.multiplier;
        assert_eq!((s.starting_capital / notional_per_contract).floor(), 1.0);
    }

    #[test]
    fn leverage_and_entries_are_the_robot_s_and_one_entry_on_a_broker() {
        let mut s = strategy();
        apply(&mut s, &Capital::default(), false, &Venue::default());
        assert_eq!((s.leverage, s.pyramiding), (1.0, 1));
        let c = Capital::validate(&json!({ "leverage": "3", "pyramiding": 2 })).unwrap();
        let mut s = strategy();
        apply(&mut s, &c, false, &Venue::default());
        assert_eq!((s.leverage, s.pyramiding), (3.0, 2));
        let mut s = strategy();
        apply(&mut s, &c, true, &Venue::default());
        assert_eq!((s.leverage, s.pyramiding), (3.0, 1));
    }

    #[test]
    fn the_strategy_s_calendar_costs_and_instrument_are_not_read() {
        let mut s = strategy();
        apply(&mut s, &Capital::default(), false, &Venue::default());
        assert!(s.filters.weekdays.is_empty());
        assert_eq!((s.fees.amount, s.slippage.value, s.spread_pct), (0.0, 0.0, 0.0));
        assert_eq!((s.instrument.multiplier, s.instrument.lot_step), (1.0, 0.0));
        // The signals and the stop stay the strategy's.
        assert_eq!(s.long.as_ref().map(|l| l.stop_loss_pct), Some(0.02));
        // No allocation: the strategy's capital and size stand in for the whole account.
        assert_eq!(s.starting_capital, 1000.0);
        assert!(matches!(s.sizing, Sizing::PercentEquity { percent } if percent == 10.0));
    }

    #[test]
    fn a_grid_spreads_the_allocation_over_its_levels() {
        let mut s = strategy();
        s.grid = serde_json::from_value(json!({ "lower": 0, "upper": 0, "levels": 5, "qty_per_level": 0.5, "total_budget": 1000 })).ok();
        let c = Capital::validate(&json!({ "allocation": "200", "percent_in": "6" })).unwrap();
        apply(&mut s, &c, false, &Venue::default());
        let g = s.grid.unwrap();
        assert_eq!((g.total_budget, g.qty_per_level), (200.0, 0.0));
    }
}
