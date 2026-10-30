//! Request + capabilities -> plan. Pure: no I/O, no clock, same input same output.
//!
//! N adapters and M stages instead of N x M code paths. Rows are tried top to bottom and the
//! first feasible one wins, so the most native plan is always preferred.

use otw_connector_model::{Capabilities, DataType, DerivedFrom, Discovery, LiveMode, TimeRange, Timeframe};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    LiveCandles { tf: Timeframe },
    HistoryCandles { tf: Timeframe, range: Option<TimeRange> },
    LiveBook,
    LiveQuote,
    LiveTrades,
    InstrumentSearch,
    Positions,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prefs {
    /// Allow plans whose data is not what the venue sent as such.
    pub allow_derived: bool,
    /// Refuse a history plan that would cost more requests than this.
    pub max_history_requests: Option<u64>,
    /// Polling interval when live candles are emulated over history and the venue declares
    /// no interval of its own.
    pub history_poll_ms: u64,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs { allow_derived: true, max_history_requests: None, history_poll_ms: 60_000 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Subscribe to this data type natively.
    Live(DataType),
    /// Emulate live by polling this data type's REST endpoint.
    Poll { data_type: DataType, every_ms: u64 },
    /// Page through history of this data type.
    History(DataType),
    /// Fold smaller candles into larger ones.
    Aggregate { from: Timeframe, to: Timeframe },
    /// Fold trades into candles.
    CandlesFromTrades { tf: Timeframe },
    /// Fold the quote mid into candles (no tape).
    CandlesFromMid { tf: Timeframe },
    /// Keep a book from snapshot + deltas.
    BuildBook,
    /// Best bid/ask of a maintained book.
    TopOfBook,
    /// Drop repeats by trade id.
    DedupTrades,
    /// Load the full list and search it locally.
    LoadAndIndex,
    NativeSearch,
    ExactMatchOnly,
    PositionsFromFills,
    Native,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CostEstimate {
    pub requests: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub steps: Vec<Step>,
    pub derived: Option<DerivedFrom>,
    pub cost: CostEstimate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unsupported(pub String);

impl std::fmt::Display for Unsupported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn native(steps: Vec<Step>) -> Plan {
    Plan { steps, derived: None, cost: CostEstimate::default() }
}

fn derived(steps: Vec<Step>, d: DerivedFrom, prefs: &Prefs) -> Option<Plan> {
    prefs.allow_derived.then(|| Plan { steps, derived: Some(d), cost: CostEstimate::default() })
}

/// The largest timeframe in `available` that tiles `tf`, other than `tf` itself.
fn best_divisor(available: &[Timeframe], tf: Timeframe) -> Option<Timeframe> {
    available
        .iter()
        .copied()
        .filter(|t| *t != tf && t.divides(tf))
        .max_by_key(|t| t.fixed_secs().unwrap_or(i64::MAX))
}

/// Requests a candle history needs at `tf`, from the range and the page size.
fn history_cost(caps: &Capabilities, tf: Timeframe, range: Option<TimeRange>) -> CostEstimate {
    let (Some(h), Some(r)) = (&caps.history.candles, range) else {
        return CostEstimate::default();
    };
    // A month is costed as 31 days: an estimate, never used to place a bar.
    let secs = tf.fixed_secs().unwrap_or(31 * 86_400).max(1);
    let span = (r.to.secs() - r.from.secs()).max(0);
    let bars = (span + secs - 1) / secs;
    let per = i64::from(h.max_items_per_call.max(1));
    CostEstimate { requests: u64::try_from((bars + per - 1) / per).unwrap_or(u64::MAX) }
}

fn over_budget(cost: CostEstimate, prefs: &Prefs) -> bool {
    prefs.max_history_requests.is_some_and(|max| cost.requests > max)
}

pub fn plan(req: &Request, caps: &Capabilities, prefs: &Prefs) -> Result<Plan, Unsupported> {
    let found = match req {
        Request::LiveCandles { tf } => live_candles(*tf, caps, prefs),
        Request::HistoryCandles { tf, range } => return history_candles(*tf, *range, caps, prefs),
        Request::LiveBook => live_book(caps, prefs),
        Request::LiveQuote => live_quote(caps, prefs),
        Request::LiveTrades => live_trades(caps, prefs),
        Request::InstrumentSearch => match caps.instrument_discovery {
            Discovery::SearchOnly => Some(native(vec![Step::NativeSearch])),
            Discovery::Full => Some(native(vec![Step::LoadAndIndex])),
            Discovery::ByIdOnly => Some(native(vec![Step::ExactMatchOnly])),
            Discovery::None => None,
        },
        Request::Positions => {
            if caps.account.positions {
                Some(native(vec![Step::Native]))
            } else if caps.account.fills_history {
                derived(vec![Step::PositionsFromFills], DerivedFrom::FromFills, prefs)
            } else {
                None
            }
        }
    };
    found.ok_or_else(|| Unsupported(format!("{}: {req:?} is not available", caps.label)))
}

fn live_candles(tf: Timeframe, caps: &Capabilities, prefs: &Prefs) -> Option<Plan> {
    let live_tfs = caps.live_timeframes();
    if matches!(caps.live.candles, LiveMode::Push) {
        if live_tfs.contains(&tf) {
            return Some(native(vec![Step::Live(DataType::Candles { tf })]));
        }
        if let Some(t) = best_divisor(live_tfs, tf) {
            if let Some(p) = derived(
                vec![Step::Live(DataType::Candles { tf: t }), Step::Aggregate { from: t, to: tf }],
                DerivedFrom::CandlesFromCandles,
                prefs,
            ) {
                return Some(p);
            }
        }
    }
    if matches!(caps.live.trades, LiveMode::Push) {
        if let Some(p) = derived(
            vec![Step::Live(DataType::Trades), Step::CandlesFromTrades { tf }],
            DerivedFrom::CandlesFromTrades,
            prefs,
        ) {
            return Some(p);
        }
    }
    if matches!(caps.live.quote, LiveMode::Push) {
        if let Some(p) = derived(vec![Step::Live(DataType::Quote), Step::CandlesFromMid { tf }], DerivedFrom::Mid, prefs) {
            return Some(p);
        }
    }
    let hist_tfs = caps.history_timeframes();
    if hist_tfs.contains(&tf) {
        let every_ms = match caps.live.candles {
            LiveMode::Poll { min_interval_ms } => min_interval_ms,
            _ => prefs.history_poll_ms,
        };
        return derived(vec![Step::Poll { data_type: DataType::Candles { tf }, every_ms }], DerivedFrom::Polled, prefs);
    }
    None
}

fn history_candles(tf: Timeframe, range: Option<TimeRange>, caps: &Capabilities, prefs: &Prefs) -> Result<Plan, Unsupported> {
    let hist_tfs = caps.history_timeframes();
    if hist_tfs.contains(&tf) {
        let cost = history_cost(caps, tf, range);
        if over_budget(cost, prefs) {
            return Err(Unsupported(format!("{}: {} requests is over budget", caps.label, cost.requests)));
        }
        return Ok(Plan { steps: vec![Step::History(DataType::Candles { tf })], derived: None, cost });
    }
    if prefs.allow_derived {
        if let Some(t) = best_divisor(hist_tfs, tf) {
            let cost = history_cost(caps, t, range);
            if !over_budget(cost, prefs) {
                return Ok(Plan {
                    steps: vec![Step::History(DataType::Candles { tf: t }), Step::Aggregate { from: t, to: tf }],
                    derived: Some(DerivedFrom::CandlesFromCandles),
                    cost,
                });
            }
        }
        if caps.history.trades.is_some() {
            // A trade history has no page size in bars, so it is costed as unknown and
            // refused whenever a budget is set.
            if prefs.max_history_requests.is_none() {
                return Ok(Plan {
                    steps: vec![Step::History(DataType::Trades), Step::CandlesFromTrades { tf }],
                    derived: Some(DerivedFrom::CandlesFromTrades),
                    cost: CostEstimate::default(),
                });
            }
        }
    }
    Err(Unsupported(format!("{}: no {tf} candle history", caps.label)))
}

fn live_book(caps: &Capabilities, prefs: &Prefs) -> Option<Plan> {
    match caps.live.book {
        LiveMode::Push if caps.book.delta && caps.book.has_seq => {
            Some(native(vec![Step::Live(DataType::BookDelta), Step::BuildBook]))
        }
        LiveMode::Push => {
            let depth = caps.book.snapshot_depths.iter().copied().max().unwrap_or(0);
            Some(native(vec![Step::Live(DataType::BookSnapshot { depth })]))
        }
        LiveMode::Poll { min_interval_ms } => derived(
            vec![Step::Poll { data_type: DataType::BookSnapshot { depth: 0 }, every_ms: min_interval_ms }],
            DerivedFrom::Polled,
            prefs,
        ),
        LiveMode::None if caps.history.book_snapshot => derived(
            vec![Step::Poll { data_type: DataType::BookSnapshot { depth: 0 }, every_ms: prefs.history_poll_ms }],
            DerivedFrom::Polled,
            prefs,
        ),
        LiveMode::None => None,
    }
}

fn live_quote(caps: &Capabilities, prefs: &Prefs) -> Option<Plan> {
    match caps.live.quote {
        LiveMode::Push => return Some(native(vec![Step::Live(DataType::Quote)])),
        LiveMode::Poll { min_interval_ms } => {
            return derived(vec![Step::Poll { data_type: DataType::Quote, every_ms: min_interval_ms }], DerivedFrom::Polled, prefs)
        }
        LiveMode::None => {}
    }
    if !caps.live.book.is_none() {
        let mut steps = live_book(caps, prefs)?.steps;
        steps.push(Step::TopOfBook);
        return derived(steps, DerivedFrom::QuoteFromBook, prefs);
    }
    if caps.history.quote {
        return derived(vec![Step::Poll { data_type: DataType::Quote, every_ms: prefs.history_poll_ms }], DerivedFrom::Polled, prefs);
    }
    // Never a quote from trades: the last print is not a bid or an ask.
    None
}

fn live_trades(caps: &Capabilities, prefs: &Prefs) -> Option<Plan> {
    match caps.live.trades {
        LiveMode::Push => Some(native(vec![Step::Live(DataType::Trades)])),
        LiveMode::Poll { min_interval_ms } => derived(
            vec![Step::Poll { data_type: DataType::Trades, every_ms: min_interval_ms }, Step::DedupTrades],
            DerivedFrom::Polled,
            prefs,
        ),
        LiveMode::None => None,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use otw_connector_model::{AssetClass, AuthKind, HistoryCaps, LiveCaps, Maturity, Paging, Timestamp};
    use std::collections::BTreeSet;

    fn base() -> Capabilities {
        Capabilities {
            provider: "t".into(),
            label: "T".into(),
            venue: "T".into(),
            api_version: "v1".into(),
            maturity: Maturity::M0,
            asset_classes: BTreeSet::from([AssetClass::Spot]),
            instrument_discovery: Discovery::None,
            auth: AuthKind::None,
            native_timeframes: vec![],
            live: LiveCaps::default(),
            history: Default::default(),
            book: Default::default(),
            max_subs_per_connection: None,
            max_connections: None,
            rate_limits: vec![],
            account: Default::default(),
            orders: Default::default(),
            regions: Default::default(),
        }
    }

    const MODES: [LiveMode; 3] = [LiveMode::Push, LiveMode::Poll { min_interval_ms: 1000 }, LiveMode::None];

    /// Every combination of the flags that matter for these requests.
    fn profiles() -> Vec<Capabilities> {
        let tf_sets: [&[Timeframe]; 3] = [&[], &[Timeframe::M1], &[Timeframe::M1, Timeframe::H1, Timeframe::D1]];
        let mut out = Vec::new();
        for candles in MODES {
            for trades in MODES {
                for quote in MODES {
                    for book in MODES {
                        for tfs in tf_sets {
                            for hist in [false, true] {
                                for hist_trades in [false, true] {
                                    for delta in [false, true] {
                                        let mut c = base();
                                        c.native_timeframes = tfs.to_vec();
                                        c.live.candles = if tfs.is_empty() { LiveMode::None } else { candles };
                                        c.live.trades = trades;
                                        c.live.quote = quote;
                                        c.live.book = book;
                                        c.book.delta = delta && matches!(book, LiveMode::Push);
                                        c.book.has_seq = c.book.delta;
                                        let h = HistoryCaps { max_items_per_call: 1000, max_lookback_days: None, paging: Paging::Time, timeframes: None, final_flag: true, aggregate_stamp: Default::default(), sides: false };
                                        c.history.candles = (hist && !tfs.is_empty()).then_some(h.clone());
                                        c.history.trades = hist_trades.then_some(h);
                                        if c.validate().is_ok() {
                                            out.push(c);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        out
    }

    /// The data types a plan relies on natively.
    fn uses(p: &Plan) -> Vec<DataType> {
        p.steps
            .iter()
            .filter_map(|s| match s {
                Step::Live(dt) | Step::Poll { data_type: dt, .. } | Step::History(dt) => Some(dt.clone()),
                _ => None,
            })
            .collect()
    }

    fn declared(c: &Capabilities, step: &Step) -> bool {
        match step {
            Step::Live(dt) => matches!(c.live_mode(dt), LiveMode::Push) && match dt {
                DataType::Candles { tf } => c.live_timeframes().contains(tf),
                _ => true,
            },
            Step::Poll { data_type, .. } => match data_type {
                DataType::Candles { tf } => c.history_timeframes().contains(tf),
                DataType::BookSnapshot { .. } => !c.live.book.is_none() || c.history.book_snapshot,
                DataType::Quote => !c.live.quote.is_none() || c.history.quote,
                DataType::Trades => !c.live.trades.is_none(),
                _ => false,
            },
            Step::History(DataType::Candles { tf }) => c.history_timeframes().contains(tf),
            Step::History(DataType::Trades) => c.history.trades.is_some(),
            Step::History(_) => false,
            _ => true,
        }
    }

    #[test]
    fn every_profile_gets_a_sound_plan_or_a_reason() {
        let requests = [
            Request::LiveCandles { tf: Timeframe::M1 },
            Request::LiveCandles { tf: Timeframe::M5 },
            Request::LiveCandles { tf: Timeframe::H4 },
            Request::HistoryCandles { tf: Timeframe::M1, range: None },
            Request::HistoryCandles { tf: Timeframe::M15, range: None },
            Request::HistoryCandles { tf: Timeframe::W1, range: None },
            Request::LiveBook,
            Request::LiveQuote,
            Request::LiveTrades,
        ];
        let profiles = profiles();
        assert!(profiles.len() > 500, "enumeration shrank to {}", profiles.len());
        for c in &profiles {
            for r in &requests {
                for allow_derived in [true, false] {
                    let prefs = Prefs { allow_derived, ..Prefs::default() };
                    match plan(r, c, &prefs) {
                        Ok(p) => {
                            assert!(!p.steps.is_empty());
                            for s in &p.steps {
                                assert!(declared(c, s), "{r:?} uses undeclared {s:?} on {:?}", c.live);
                            }
                            let fully_native = p.steps.iter().all(|s| matches!(s, Step::Live(_) | Step::History(_) | Step::BuildBook | Step::Native));
                            assert_eq!(p.derived.is_none(), fully_native, "{r:?} -> {p:?}");
                            if !allow_derived {
                                assert!(p.derived.is_none(), "derived plan while derived is refused: {p:?}");
                            }
                            for s in &p.steps {
                                if let Step::Aggregate { from, to } = s {
                                    assert!(from.divides(*to), "aggregate {from} into {to}");
                                }
                            }
                            assert!(!uses(&p).is_empty());
                        }
                        Err(Unsupported(why)) => assert!(!why.is_empty()),
                    }
                }
            }
        }
    }

    #[test]
    fn never_derives_a_quote_from_trades() {
        let mut c = base();
        c.live.trades = LiveMode::Push;
        assert!(plan(&Request::LiveQuote, &c, &Prefs::default()).is_err());
    }

    #[test]
    fn prefers_native_then_divisor_then_trades() {
        let mut c = base();
        c.native_timeframes = vec![Timeframe::M5, Timeframe::H1];
        c.live.candles = LiveMode::Push;
        c.live.trades = LiveMode::Push;
        let p = plan(&Request::LiveCandles { tf: Timeframe::H1 }, &c, &Prefs::default()).unwrap();
        assert_eq!(p.steps, vec![Step::Live(DataType::Candles { tf: Timeframe::H1 })]);
        let p = plan(&Request::LiveCandles { tf: Timeframe::H4 }, &c, &Prefs::default()).unwrap();
        assert_eq!(p.steps[1], Step::Aggregate { from: Timeframe::H1, to: Timeframe::H4 });
        let m7: Timeframe = "7m".parse().unwrap();
        let p = plan(&Request::LiveCandles { tf: m7 }, &c, &Prefs::default()).unwrap();
        assert_eq!(p.derived, Some(DerivedFrom::CandlesFromTrades));
    }

    #[test]
    fn history_cost_is_counted_and_budgeted() {
        let mut c = base();
        c.native_timeframes = vec![Timeframe::M1];
        c.history.candles = Some(HistoryCaps { max_items_per_call: 1000, max_lookback_days: None, paging: Paging::Time, timeframes: None, final_flag: true, aggregate_stamp: Default::default(), sides: false });
        let day = TimeRange::new(Timestamp::from_secs(0).unwrap(), Timestamp::from_secs(86_400).unwrap());
        let p = plan(&Request::HistoryCandles { tf: Timeframe::M1, range: Some(day) }, &c, &Prefs::default()).unwrap();
        assert_eq!(p.cost.requests, 2, "1440 bars at 1000 per call");
        let tight = Prefs { max_history_requests: Some(1), ..Prefs::default() };
        assert!(plan(&Request::HistoryCandles { tf: Timeframe::M1, range: Some(day) }, &c, &tight).is_err());
    }
}
