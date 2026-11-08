//! A history answer for each HTTP market-data adapter: the first byte picks the status, the
//! rest is the body, decoded as the answer to a daily-candle request. Never panics.

#![no_main]

use bytes::Bytes;
use libfuzzer_sys::fuzz_target;
use otw_connector_model::{Command, CorrId, DataType, InstrumentId, TimeRange, Timeframe, Timestamp};
use otw_connector_providers as p;
use otw_connector_sdk::{InstrumentCache, Mapper, NativeRequest, RawFrame};
use otw_connector_testkit::{decode_once, encode_once};

const STATUSES: [u16; 8] = [200, 200, 200, 400, 401, 404, 429, 503];

fn run<M: Mapper>(m: &M, venue: &str, symbol: &str, status: u16, body: &[u8]) {
    let cache = InstrumentCache::new();
    let now = Timestamp::from_secs(1_774_915_200).unwrap_or_default();
    let range = TimeRange::new(Timestamp::from_secs(1_772_409_600).unwrap_or_default(), Timestamp::from_secs(1_773_014_400).unwrap_or_default());
    let cmd = Command::RequestHistory {
        req_id: CorrId(1),
        instrument: InstrumentId::new(venue, symbol),
        data_type: DataType::Candles { tf: Timeframe::D1 },
        range,
        cursor: None,
    };
    let mut s = M::Session::default();
    let Ok(reqs) = encode_once(m, &mut s, &cache, &cmd, now) else { return };
    let Some(NativeRequest::Http(h)) = reqs.into_iter().next() else { return };
    let frame = RawFrame::Http {
        corr_id: h.corr_id,
        endpoint_id: h.endpoint_id,
        status,
        headers: vec![("retry-after".into(), "7".into())],
        body: Bytes::copy_from_slice(body),
    };
    let _ = decode_once(m, &mut s, &cache, &frame, now);
}

fuzz_target!(|data: &[u8]| {
    let Some((&first, body)) = data.split_first() else { return };
    let status = STATUSES[usize::from(first) % STATUSES.len()];
    match first / 32 {
        0 => {
            if let Ok(m) = p::alpaca::Alpaca::new(p::alpaca::Feed::Iex) {
                run(&m, p::alpaca::VENUE, "AAPL", status, body);
                run(&m, p::alpaca::VENUE_CRYPTO, "BTC/USD", status, body);
            }
        }
        1 => {
            if let Ok(m) = p::alphavantage::AlphaVantage::new() {
                run(&m, p::alphavantage::VENUE, "IBM", status, body);
            }
        }
        2 => {
            if let Ok(m) = p::coinbase::Coinbase::new() {
                run(&m, p::coinbase::VENUE, "BTC-USD", status, body);
            }
        }
        3 => {
            if let Ok(m) = p::eodhd::Eodhd::new() {
                run(&m, p::eodhd::VENUE, "AAPL.US", status, body);
            }
        }
        4 => {
            if let Ok(m) = p::kraken::Kraken::new() {
                run(&m, p::kraken::VENUE, "XBTUSD", status, body);
            }
        }
        5 => {
            if let Ok(m) = p::massive::Massive::new() {
                run(&m, p::massive::VENUE, "AAPL", status, body);
            }
        }
        6 => {
            if let Ok(m) = p::oanda::Oanda::new() {
                run(&m, p::oanda::VENUE, "EUR_USD", status, body);
            }
        }
        _ => {
            if let Ok(m) = p::okx::Okx::new() {
                run(&m, p::okx::VENUE, "BTC-USDT", status, body);
            }
            if let Ok(m) = p::yahoo::Yahoo::new() {
                run(&m, p::yahoo::VENUE, "AAPL", status, body);
            }
        }
    }
});
