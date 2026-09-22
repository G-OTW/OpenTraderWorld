//! Live checks against the venues that publish candles **without a key**.
//!
//! OKX, Bitget and Binance USDⓈ-M answer their whole candle archive to anybody, so the
//! download path for those three can be exercised end to end with no account at all: the
//! request the connector builds, the page the venue returns, and the bars it decodes.
//!
//! What this catches that an offline test cannot:
//!   - a timeframe token the venue no longer accepts, or accepts with another meaning;
//!   - the **anchoring** claim in each connector's doc comment (a day opens at midnight UTC,
//!     a week on Monday), which is exactly what the `utc` variants were asked for;
//!   - a column read out of the wrong slot in the candle array, caught by comparing the same
//!     minute across three independent venues rather than against our own expectation.
//!
//! These hit the network, so they are skipped unless `OTW_NET_TEST=1`, like [`crate::live`].
//! Run with: `OTW_NET_TEST=1 cargo test -p otw-core histdata::net_tests -- --nocapture`.

use std::collections::HashMap;

use otw_store::histdata::Bar;
use time::{Duration, OffsetDateTime, Weekday};

use super::{connector_for, Chunk};

fn enabled() -> bool {
    std::env::var("OTW_NET_TEST").as_deref() == Ok("1")
}

/// Config a connector needs to know which market it reads. Empty for the ones with a single
/// board; Bitget serves the same ticker on two.
fn cfg(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

async fn fetch(
    provider: &str,
    config: &HashMap<String, String>,
    ticker: &str,
    timeframe: &str,
    from: OffsetDateTime,
    to: OffsetDateTime,
) -> Chunk {
    let c = connector_for(provider).unwrap();
    let http = super::client().unwrap();
    c.fetch_chunk(&http, config, ticker, "crypto", timeframe, from, to)
        .await
        .unwrap_or_else(|e| panic!("{provider} {ticker} {timeframe}: {e:#}"))
}

/// Every invariant a stored bar owes, whoever served it.
fn assert_sane(provider: &str, bars: &[Bar], step: i64, from: OffsetDateTime, to: OffsetDateTime) {
    assert!(!bars.is_empty(), "{provider}: answered no bars at all");
    let mut prev: Option<OffsetDateTime> = None;
    for b in bars {
        assert!(b.open > 0.0 && b.high > 0.0 && b.low > 0.0 && b.close > 0.0, "{provider}: {b:?}");
        assert!(b.high >= b.low, "{provider}: high below low: {b:?}");
        assert!(b.high >= b.open && b.high >= b.close, "{provider}: high is not the max: {b:?}");
        assert!(b.low <= b.open && b.low <= b.close, "{provider}: low is not the min: {b:?}");
        assert!(b.volume >= 0.0, "{provider}: negative volume: {b:?}");
        // A row is a period: its stamp is the open, aligned on the timeframe.
        assert_eq!(
            b.ts.unix_timestamp() % step,
            0,
            "{provider}: {} is not aligned on {step}s",
            b.ts
        );
        // The window is ours, not the venue's: nothing outside it may be stored.
        assert!(b.ts >= from && b.ts < to, "{provider}: {} is outside [{from}, {to})", b.ts);
        if let Some(p) = prev {
            assert!(b.ts > p, "{provider}: bars are not strictly ascending ({p} then {})", b.ts);
        }
        prev = Some(b.ts);
    }
}

// ── The window the connector was asked for ───────────────────────────────────

/// A minute window, on each keyless venue: the bars come back aligned, ascending, inside
/// the window, and dense enough that the page covered it.
#[tokio::test]
async fn a_minute_window_comes_back_whole_from_every_keyless_venue() {
    if !enabled() {
        return;
    }
    // End on a closed minute: the one in progress is not a period yet.
    let to = OffsetDateTime::from_unix_timestamp(
        OffsetDateTime::now_utc().unix_timestamp() / 60 * 60,
    )
    .unwrap();
    let from = to - Duration::minutes(90);
    for (provider, ticker, config) in [
        ("okx", "BTC-USDT", cfg(&[])),
        ("bitget", "BTCUSDT", cfg(&[])),
        ("bitget", "BTCUSDT", cfg(&[("market", "usdt-futures")])),
        ("binance_futures", "BTCUSDT", cfg(&[])),
    ] {
        let chunk = fetch(provider, &config, ticker, "1m", from, to).await;
        assert_sane(provider, &chunk.bars, 60, from, to);
        // 90 minutes asked for: a liquid pair trades every one of them. Allow a couple of
        // missing prints rather than demanding a perfect board.
        assert!(
            chunk.bars.len() >= 85,
            "{provider}: {} bars for a 90 minute window",
            chunk.bars.len()
        );
    }
}

/// The reason the UTC-anchored tokens are asked for. OKX and Bitget open their *default*
/// daily candle at 16:00 UTC; if the connector ever loses the `utc` variant, this fails.
#[tokio::test]
async fn a_day_opens_on_midnight_utc_everywhere() {
    if !enabled() {
        return;
    }
    let today = OffsetDateTime::now_utc().date().midnight().assume_utc();
    let from = today - Duration::days(12);
    for (provider, ticker, config) in [
        ("okx", "BTC-USDT", cfg(&[])),
        ("bitget", "BTCUSDT", cfg(&[])),
        ("bitget", "BTCUSDT", cfg(&[("market", "usdt-futures")])),
        ("binance_futures", "BTCUSDT", cfg(&[])),
    ] {
        let chunk = fetch(provider, &config, ticker, "1d", from, today).await;
        assert_sane(provider, &chunk.bars, 86_400, from, today);
        assert!(chunk.bars.len() >= 10, "{provider}: {} daily bars", chunk.bars.len());
        for b in &chunk.bars {
            assert_eq!(b.ts.time(), time::Time::MIDNIGHT, "{provider}: {} is not a UTC day", b.ts);
        }
    }
}

/// The weekly candle is the Monday-anchored one, for the same reason.
#[tokio::test]
async fn a_week_opens_on_monday() {
    if !enabled() {
        return;
    }
    let today = OffsetDateTime::now_utc().date().midnight().assume_utc();
    let from = today - Duration::weeks(10);
    for (provider, ticker, config) in [
        ("okx", "BTC-USDT", cfg(&[])),
        ("bitget", "BTCUSDT", cfg(&[])),
        ("binance_futures", "BTCUSDT", cfg(&[])),
    ] {
        let chunk = fetch(provider, &config, ticker, "1w", from, today).await;
        assert!(chunk.bars.len() >= 5, "{provider}: {} weekly bars", chunk.bars.len());
        for b in &chunk.bars {
            assert_eq!(b.ts.weekday(), Weekday::Monday, "{provider}: {} is not a Monday", b.ts);
            assert_eq!(b.ts.time(), time::Time::MIDNIGHT, "{provider}: {} is not midnight", b.ts);
        }
    }
}

/// Three venues, one minute, one instrument: BTC is BTC. A field read out of the wrong slot
/// (close where the open is, a volume in the price column) shows up here and nowhere else,
/// because the reference is the other two exchanges rather than our own sample.
#[tokio::test]
async fn the_same_minute_agrees_across_the_three_venues() {
    if !enabled() {
        return;
    }
    // Two minutes back: closed everywhere, and still recent enough to be one price.
    let to = OffsetDateTime::from_unix_timestamp(
        OffsetDateTime::now_utc().unix_timestamp() / 60 * 60,
    )
    .unwrap()
        - Duration::minutes(1);
    let from = to - Duration::minutes(30);
    let mut boards = Vec::new();
    for (provider, ticker, config) in [
        ("okx", "BTC-USDT", cfg(&[])),
        ("bitget", "BTCUSDT", cfg(&[])),
        ("binance_futures", "BTCUSDT", cfg(&[])),
    ] {
        let chunk = fetch(provider, &config, ticker, "1m", from, to).await;
        boards.push((provider, chunk.bars));
    }
    // Compare the minutes all three answered, so this test reports on prices and not on
    // whichever venue served one bar more or less.
    let common: Vec<OffsetDateTime> = boards[0]
        .1
        .iter()
        .map(|b| b.ts)
        .filter(|ts| boards.iter().all(|(_, bars)| bars.iter().any(|b| b.ts == *ts)))
        .collect();
    assert!(common.len() >= 20, "only {} minutes are shared by the three venues", common.len());
    for ts in common {
        let row: Vec<(&str, &Bar)> = boards
            .iter()
            .map(|(p, bars)| (*p, bars.iter().find(|b| b.ts == ts).unwrap()))
            .collect();
        // Every OHLC field, not just the close: a column read out of the wrong slot moves
        // one of the four and leaves the others right.
        for (label, pick) in [
            ("open", (|b: &Bar| b.open) as fn(&Bar) -> f64),
            ("high", |b: &Bar| b.high),
            ("low", |b: &Bar| b.low),
            ("close", |b: &Bar| b.close),
        ] {
            let hi = row.iter().map(|(_, b)| pick(b)).fold(f64::MIN, f64::max);
            let lo = row.iter().map(|(_, b)| pick(b)).fold(f64::MAX, f64::min);
            // Spot and perp on three exchanges: tenths of a percent apart, never more.
            assert!(
                (hi - lo) / lo < 0.01,
                "{ts} {label}: {lo} to {hi} across venues ({:?}): a field is misread",
                row.iter().map(|(p, b)| (*p, pick(b))).collect::<Vec<_>>()
            );
        }
    }
}

/// The symbol picker is keyless too, and its answer has to be spelled the way the download
/// endpoint accepts: the hit is fed straight back into a fetch.
#[tokio::test]
async fn a_symbol_hit_is_a_ticker_the_same_venue_will_serve() {
    if !enabled() {
        return;
    }
    let to = OffsetDateTime::from_unix_timestamp(
        OffsetDateTime::now_utc().unix_timestamp() / 60 * 60,
    )
    .unwrap();
    let from = to - Duration::minutes(10);
    for (provider, query, config) in [
        ("okx", "BTC-USDT", cfg(&[])),
        ("bitget", "BTCUSDT", cfg(&[])),
        ("binance_futures", "BTCUSDT", cfg(&[])),
    ] {
        let c = connector_for(provider).unwrap();
        let http = super::client().unwrap();
        let hits = c
            .search_symbols(&http, &config, query, "crypto", 10)
            .await
            .unwrap_or_else(|e| panic!("{provider} search: {e:#}"));
        assert!(!hits.is_empty(), "{provider}: found nothing for {query}");
        let top = &hits[0];
        assert_eq!(top.symbol.to_uppercase(), query, "{provider}: exact match is not first");
        let chunk = fetch(provider, &config, &top.symbol, "1m", from, to).await;
        assert!(!chunk.bars.is_empty(), "{provider}: its own hit downloads nothing");
    }
}

/// A pair that does not exist is an error naming what to fix, never an empty series that
/// reads as "this instrument did not trade".
#[tokio::test]
async fn an_unknown_ticker_is_refused_rather_than_answered_empty() {
    if !enabled() {
        return;
    }
    let to = OffsetDateTime::now_utc();
    let from = to - Duration::hours(2);
    for (provider, ticker, config) in [
        ("okx", "ZZZZ-NOPE", cfg(&[])),
        ("bitget", "ZZZZNOPE", cfg(&[])),
        ("binance_futures", "ZZZZNOPE", cfg(&[])),
    ] {
        let c = connector_for(provider).unwrap();
        let http = super::client().unwrap();
        let r = c.fetch_chunk(&http, &config, ticker, "crypto", "1m", from, to).await;
        match r {
            Err(e) => {
                let msg = format!("{e:#}");
                println!("{provider} unknown ticker: {msg}");
                assert!(
                    msg.to_lowercase().contains(&ticker.to_lowercase())
                        || msg.to_lowercase().contains("symbol")
                        || msg.to_lowercase().contains("pair")
                        || msg.to_lowercase().contains("instrument"),
                    "{provider}: the refusal does not name the instrument: {msg}"
                );
            }
            Ok(c) => assert!(
                c.bars.is_empty(),
                "{provider}: invented {} bars for a pair that does not exist",
                c.bars.len()
            ),
        }
    }
}

/// **A row is a period.** The candle the venue is still building is not one, and storing it
/// files a partial high, low, close and volume that nothing later comes back to correct.
/// Each connector states how it keeps that bar out (OKX reads `confirm`, TradeStation reads
/// `BarStatus`, OANDA reads `complete`); this asks the venue itself, mid-minute, whether the
/// bar in progress came back anyway.
#[tokio::test]
async fn a_forming_candle_is_not_stored() {
    if !enabled() {
        return;
    }
    let now = OffsetDateTime::now_utc();
    let minute =
        OffsetDateTime::from_unix_timestamp(now.unix_timestamp() / 60 * 60).unwrap();
    // The download worker asks for everything up to now, which is mid-period by definition.
    let from = minute - Duration::minutes(30);
    for (provider, ticker, config) in [
        ("okx", "BTC-USDT", cfg(&[])),
        ("bitget", "BTCUSDT", cfg(&[])),
        ("bitget", "BTCUSDT", cfg(&[("market", "usdt-futures")])),
        ("binance_futures", "BTCUSDT", cfg(&[])),
    ] {
        let chunk = fetch(provider, &config, ticker, "1m", from, now).await;
        let forming = chunk.bars.iter().find(|b| b.ts >= minute);
        assert!(
            forming.is_none(),
            "{provider}: stored the minute still forming ({:?}), asked at {now}",
            forming.unwrap()
        );
    }
}

/// The same claim, proved without reading a clock: a stored bar is final, so asking twice
/// for one window has to answer the same values both times. A venue whose archive is a
/// minute behind its socket slips past the check above and is caught here.
#[tokio::test]
async fn a_bar_does_not_change_after_it_is_answered() {
    if !enabled() {
        return;
    }
    let venues = [
        ("okx", "BTC-USDT", cfg(&[])),
        ("bitget", "BTCUSDT", cfg(&[])),
        ("bitget", "BTCUSDT", cfg(&[("market", "usdt-futures")])),
        ("binance_futures", "BTCUSDT", cfg(&[])),
    ];
    let mut first = Vec::new();
    for (provider, ticker, config) in &venues {
        let now = OffsetDateTime::now_utc();
        first.push(fetch(provider, config, ticker, "1m", now - Duration::minutes(20), now).await);
    }
    tokio::time::sleep(std::time::Duration::from_secs(40)).await;
    // Every venue is read before anything is asserted, so one that leaks does not hide the
    // next one behind it.
    let mut changed = Vec::new();
    for (i, (provider, ticker, config)) in venues.iter().enumerate() {
        let now = OffsetDateTime::now_utc();
        let again = fetch(provider, config, ticker, "1m", now - Duration::minutes(20), now).await;
        for a in &first[i].bars {
            let Some(b) = again.bars.iter().find(|b| b.ts == a.ts) else {
                continue;
            };
            if (a.open, a.high, a.low, a.close, a.volume)
                != (b.open, b.high, b.low, b.close, b.volume)
            {
                changed.push(format!(
                    "{provider} {}: {:?} then {:?}",
                    a.ts,
                    (a.open, a.high, a.low, a.close, a.volume),
                    (b.open, b.high, b.low, b.close, b.volume)
                ));
            }
        }
    }
    assert!(changed.is_empty(), "these bars were still forming when they were stored: {changed:#?}");
}
