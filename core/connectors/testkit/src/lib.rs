//! Test support shared by every adapter: a clock that only moves when told, a mock venue the
//! driver is pointed at, and the generic conformance checks.
//!
//! Fixtures are synthetic: written by hand from the venue's documented schema, never copied
//! from a live answer. Recordings of real traffic stay on the machine that made them
//! (`cassettes/` is gitignored) and are never committed.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use bytes::Bytes;
use otw_connector_engine::validate;
use otw_connector_model::{
    AccountKind, Capabilities, Command, ConnectorError, CorrId, DataType, Envelope, ErrorKind, Event, HistoryItems,
    InstrumentId, LiveMode, TimeRange, Timeframe, Timestamp,
};
use otw_connector_sdk::{Clock, Driver, DriverConfig, InstrumentCache, MapCtx, Mapper, RawFrame};

pub use wiremock;

/// A clock that stands still until a test moves it.
#[derive(Debug, Default)]
pub struct ManualClock(AtomicI64);

impl ManualClock {
    pub fn at(t: Timestamp) -> Arc<Self> {
        Arc::new(ManualClock(AtomicI64::new(t.nanos())))
    }
    pub fn set(&self, t: Timestamp) {
        self.0.store(t.nanos(), Ordering::SeqCst);
    }
    pub fn advance_secs(&self, s: i64) {
        self.0.fetch_add(s.saturating_mul(1_000_000_000), Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Timestamp {
        Timestamp::from_nanos(self.0.load(Ordering::SeqCst))
    }
}

pub fn ts(secs: i64) -> Timestamp {
    Timestamp::from_secs(secs).expect("test timestamp")
}

/// An HTTP answer as the transport would hand it to `decode`.
pub fn http_frame(corr: u64, endpoint_id: &'static str, status: u16, body: &str) -> RawFrame {
    RawFrame::Http {
        corr_id: CorrId(corr),
        endpoint_id,
        status,
        headers: vec![("content-type".into(), "application/json".into())],
        body: Bytes::from(body.to_string()),
    }
}

/// Run `decode` once, outside any driver: the purest test there is.
pub fn decode_once<M: Mapper>(
    m: &M,
    session: &mut M::Session,
    instruments: &InstrumentCache,
    frame: &RawFrame,
    now: Timestamp,
) -> Result<Vec<Envelope>, ConnectorError> {
    let connector = otw_connector_model::ConnectorId("test".into());
    let ctx = MapCtx { received_ts: now, instruments, connector: &connector };
    m.decode(frame, session, &ctx)
}

pub fn encode_once<M: Mapper>(
    m: &M,
    session: &mut M::Session,
    instruments: &InstrumentCache,
    cmd: &Command,
    now: Timestamp,
) -> Result<Vec<otw_connector_sdk::NativeRequest>, ConnectorError> {
    let connector = otw_connector_model::ConnectorId("test".into());
    let ctx = MapCtx { received_ts: now, instruments, connector: &connector };
    m.encode(cmd, session, &ctx)
}

/// A driver whose every base URL points at `server`, with a frozen clock.
pub fn driver_against<M: Mapper>(mapper: M, server: &wiremock::MockServer, clock: Arc<ManualClock>) -> Driver<M> {
    let mut cfg = DriverConfig::new("test");
    for (k, _) in mapper.base_urls() {
        cfg = cfg.base(k, &server.uri());
    }
    cfg.timeout = std::time::Duration::from_secs(5);
    Driver::new(mapper, cfg, reqwest::Client::new()).with_clock(clock)
}

/// Every candle sound, sorted, and no candle repeated across chunks.
pub fn assert_history_sound(chunks: &[HistoryItems]) {
    let mut all = Vec::new();
    for c in chunks {
        let v = validate::history(c);
        assert!(v.is_empty(), "invariants broken: {v:?}");
        if let HistoryItems::Candles(cs) = c {
            all.extend(cs.iter().map(|c| c.open_ts));
        }
    }
    let mut sorted = all.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), all.len(), "a candle is repeated across pages");
    assert_eq!(sorted, all, "pages are not in time order");
}

pub fn assert_events_sound(evs: &[Envelope]) {
    for e in evs {
        let v = validate::envelope(e);
        assert!(v.is_empty(), "invariants broken by {:?}: {v:?}", e.event);
    }
}

/// The command must be refused as `Unsupported` without sending anything.
fn refused<M: Mapper>(m: &M, cmd: &Command) -> bool {
    let mut s = M::Session::default();
    matches!(
        encode_once(m, &mut s, &InstrumentCache::new(), cmd, ts(0)),
        Err(ConnectorError { kind: ErrorKind::Unsupported, .. })
    )
}

/// Conformance: whatever the capabilities do not declare is refused at encode time.
pub fn assert_undeclared_is_unsupported<M: Mapper>(m: &M, instrument: &InstrumentId) {
    let caps: Capabilities = m.capabilities().clone();
    let sub = |dt: DataType| Command::Subscribe { sub_id: CorrId(1), instrument: instrument.clone(), data_type: dt };
    let live = [
        (DataType::Trades, caps.live.trades),
        (DataType::Quote, caps.live.quote),
        (DataType::BookSnapshot { depth: 5 }, caps.live.book),
        (DataType::FundingRate, LiveMode::None),
        (DataType::OpenInterest, LiveMode::None),
        (DataType::News, LiveMode::None),
    ];
    for (dt, mode) in live {
        if !matches!(mode, LiveMode::Push) {
            assert!(refused(m, &sub(dt.clone())), "{} accepts an undeclared live {dt:?}", caps.provider);
        }
    }
    let odd: Timeframe = "7m".parse().unwrap_or(Timeframe::M1);
    if !caps.live_timeframes().contains(&odd) {
        assert!(refused(m, &sub(DataType::Candles { tf: odd })), "{} streams an undeclared 7m", caps.provider);
    }
    let range = TimeRange::new(ts(0), ts(60));
    let hist = |dt: DataType| Command::RequestHistory {
        req_id: CorrId(2),
        instrument: instrument.clone(),
        data_type: dt,
        range,
        cursor: None,
    };
    if !caps.history_timeframes().contains(&odd) {
        assert!(refused(m, &hist(DataType::Candles { tf: odd })), "{} serves an undeclared 7m history", caps.provider);
    }
    if caps.history.trades.is_none() {
        assert!(refused(m, &hist(DataType::Trades)), "{} serves an undeclared trade history", caps.provider);
    }
    for (kind, declared) in [
        (AccountKind::Balances, caps.account.balances),
        (AccountKind::Positions, caps.account.positions),
        (AccountKind::Fills, caps.account.fills_history),
        (AccountKind::OpenOrders, caps.account.open_orders),
        (AccountKind::Summary, caps.account.summary),
    ] {
        if !declared {
            let q = Command::QueryAccount { req_id: CorrId(3), kind, symbols: vec![instrument.clone()], range: Some(range), cursor: None, account: None };
            assert!(
                refused(m, &q),
                "{} accepts an undeclared account query {kind:?}",
                caps.provider
            );
        }
    }
    if !caps.history.board {
        assert!(refused(m, &Command::RequestBoard { req_id: CorrId(4) }), "{} serves an undeclared price board", caps.provider);
    }
}

/// Pull the chunks out of a decode result, failing on an `Error` event.
pub fn chunks(evs: &[Envelope]) -> Vec<(HistoryItems, Option<String>, bool)> {
    evs.iter()
        .filter_map(|e| match &e.event {
            Event::HistoryChunk { items, next_cursor, done } => Some((items.clone(), next_cursor.clone(), *done)),
            Event::Error(err) => panic!("error event: {err}"),
            _ => None,
        })
        .collect()
}

/// The JSON body a test writes inline, checked to parse so a typo fails loudly here.
pub fn json(s: &str) -> String {
    let _: serde_json::Value = serde_json::from_str(s).expect("fixture is valid JSON");
    s.to_string()
}
