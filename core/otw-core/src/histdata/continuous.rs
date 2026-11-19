//! A dated futures contract read further back than the day it became the traded month: the
//! bars before it come from the product's continuous series, shifted to sit on the contract's
//! level.
//!
//! A contract trades thinly until the one before it rolls off (a print or two a day, bars with
//! open = high = low = close), so its own bars only take over at the **roll**: the first day
//! its daily volume reaches the continuous series' (which is the front month's). A contract
//! that has not led yet (a far month) keeps its own bars from its listing, thin as they are,
//! since that is all it has.
//!
//! Provider-neutral. A connector says which continuous series a dated contract belongs to and
//! when the contract's own bars start ([`super::Connector::continuous_of`]); everything else is
//! here. The shift is a **difference** (Panama): the median of `contract close − continuous
//! close` over the first daily bars both hold from the junction, so a point move stays a point
//! move and the curve does not step there. Applied on read, never stored: the continuous bars
//! stay under their own ticker.

use std::collections::HashMap;
use std::sync::Mutex;

use anyhow::{anyhow, Result};
use time::OffsetDateTime;

use otw_store::histdata::Bar;

use super::Connector;

/// The continuous series behind a dated contract, as a connector answers it.
#[derive(Debug, Clone)]
pub struct Continuous {
    /// The continuous series, as a ticker this same connector reads (`MNQ@CME` at IB).
    pub ticker: String,
    /// The instant the dated contract's own first bar stands at.
    pub listed: OffsetDateTime,
}

/// Where a stitched chart changes source, and by how much the continuous bars are moved.
#[derive(Debug, Clone, PartialEq)]
pub struct Junction {
    pub continuous: String,
    /// Bars before this instant come from the continuous series: the roll, or the listing for
    /// a contract that has not led yet.
    pub until: OffsetDateTime,
    /// Added to the continuous bars' prices.
    pub offset: f64,
}

/// Daily bars the shift is measured over, from the junction.
const MEASURE_BARS: usize = 5;

/// Junctions by `provider|connector|ticker`. A listing does not move and the shift is
/// measured on past bars, so one answer holds for the life of the process. `None` is cached
/// too: a contract that is not a dated future stays one.
static CACHE: Mutex<Option<HashMap<String, Option<Junction>>>> = Mutex::new(None);

fn cached(key: &str) -> Option<Option<Junction>> {
    CACHE.lock().ok()?.as_ref()?.get(key).cloned()
}

fn remember(key: &str, j: &Option<Junction>) {
    if let Ok(mut g) = CACHE.lock() {
        g.get_or_insert_with(HashMap::new).insert(key.to_string(), j.clone());
    }
}

/// The junction of a dated contract with its continuous series, or `None` when the connector
/// serves no continuous series for it. An error when it does but the two series share no
/// daily bar to measure the shift on: the bars are not placed by guess.
pub async fn junction(
    connector: &dyn Connector,
    http: &reqwest::Client,
    secrets: &HashMap<String, String>,
    cache_key: &str,
    ticker: &str,
    asset_type: &str,
) -> Result<Option<Junction>> {
    if let Some(hit) = cached(cache_key) {
        return Ok(hit);
    }
    let Some(c) = connector.continuous_of(http, secrets, ticker, asset_type).await? else {
        remember(cache_key, &None);
        return Ok(None);
    };
    let tf = if connector.capability().timeframes.contains(&"1d") { "1d" } else {
        return Err(anyhow!("{} serves no daily bars to place {} on {ticker}", connector.capability().label, c.ticker));
    };
    // The contract's whole life so far, daily: where it took the lead, and the gap there.
    let from = c.listed - time::Duration::days(1);
    let to = OffsetDateTime::now_utc();
    let dated = connector.fetch_chunk(http, secrets, ticker, asset_type, tf, from, to).await?.bars;
    let cont = connector.fetch_chunk(http, secrets, &c.ticker, asset_type, tf, from, to).await?.bars;
    let (until, offset) = place(&dated, &cont, c.listed).ok_or_else(|| {
        anyhow!("{ticker} and {} share no daily bar, so the continuous bars cannot be placed on it", c.ticker)
    })?;
    let j = Some(Junction { continuous: c.ticker, until, offset });
    remember(cache_key, &j);
    Ok(j)
}

/// Where the contract's own bars take over, and the shift there, from both series' daily bars:
/// the roll when the contract has led, else its listing.
pub fn place(dated: &[Bar], cont: &[Bar], listed: OffsetDateTime) -> Option<(OffsetDateTime, f64)> {
    match roll(dated, cont) {
        Some(at) => Some((at, measure(dated, cont, at)?)),
        None => Some((listed, measure(dated, cont, listed)?)),
    }
}

/// The first day the contract's volume reaches the continuous series': the day the series
/// switches to it, its volume becoming the contract's own. At IB (MNQ, 2025-12 to 2026-09)
/// that is the Monday after CME's roll Thursday, the same day as the real crossover with the
/// expiring month or one day before it. A day either side reports no volume for (IB writes -1)
/// proves nothing.
pub fn roll(dated: &[Bar], cont: &[Bar]) -> Option<OffsetDateTime> {
    let volume: HashMap<i64, f64> = cont.iter().filter(|b| b.volume > 0.0).map(|b| (b.ts.unix_timestamp(), b.volume)).collect();
    let mut sorted: Vec<&Bar> = dated.iter().filter(|b| b.volume > 0.0).collect();
    sorted.sort_by_key(|b| b.ts);
    sorted.into_iter().find(|b| volume.get(&b.ts.unix_timestamp()).is_some_and(|v| b.volume >= *v)).map(|b| b.ts)
}

/// The median of `dated close − continuous close` over the first [`MEASURE_BARS`] stamps both
/// series hold from `from` on. A median, so one odd session does not set the level of years of
/// bars.
pub fn measure(dated: &[Bar], cont: &[Bar], from: OffsetDateTime) -> Option<f64> {
    let by_ts: HashMap<i64, f64> = cont.iter().filter(|b| b.close > 0.0).map(|b| (b.ts.unix_timestamp(), b.close)).collect();
    let mut sorted: Vec<&Bar> = dated.iter().filter(|b| b.close > 0.0 && b.ts >= from).collect();
    sorted.sort_by_key(|b| b.ts);
    let mut diffs: Vec<f64> = sorted
        .iter()
        .filter_map(|b| by_ts.get(&b.ts.unix_timestamp()).map(|c| b.close - c))
        .take(MEASURE_BARS)
        .collect();
    if diffs.is_empty() {
        return None;
    }
    diffs.sort_by(f64::total_cmp);
    let n = diffs.len();
    Some(if n % 2 == 1 { diffs[n / 2] } else { (diffs[n / 2 - 1] + diffs[n / 2]) / 2.0 })
}

/// Continuous bars moved onto the contract: before the junction only, prices shifted, volume
/// as it was (it is the series' own).
pub fn shift(bars: Vec<Bar>, j: &Junction) -> Vec<Bar> {
    bars.into_iter()
        .filter(|b| b.ts < j.until)
        .map(|b| Bar {
            open: b.open + j.offset,
            high: b.high + j.offset,
            low: b.low + j.offset,
            close: b.close + j.offset,
            adj_open: None,
            adj_high: None,
            adj_low: None,
            adj_close: None,
            ..b
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(day: i64) -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(1_758_412_800 + day * 86_400).unwrap()
    }

    fn bar(day: i64, close: f64) -> Bar {
        traded(day, close, 10.0)
    }

    fn traded(day: i64, close: f64, volume: f64) -> Bar {
        Bar { ts: at(day), open: close, high: close + 1.0, low: close - 1.0, close, volume, adj_open: None, adj_high: None, adj_low: None, adj_close: None }
    }

    #[test]
    fn the_shift_is_the_median_gap_over_the_first_shared_bars() {
        // An odd first session (gap 900) does not set the level: the median of 5 does.
        let dated = vec![bar(0, 1900.0), bar(1, 1325.0), bar(2, 1330.0), bar(3, 1320.0), bar(4, 1335.0), bar(9, 9999.0)];
        let cont = vec![bar(-1, 500.0), bar(0, 1000.0), bar(1, 1000.0), bar(2, 1000.0), bar(3, 1000.0), bar(4, 1000.0), bar(9, 1000.0)];
        assert_eq!(measure(&dated, &cont, at(0)), Some(330.0));
        // From day 1 on, the odd session is not in the sample.
        assert_eq!(measure(&dated, &cont, at(1)), Some(330.0));
    }

    #[test]
    fn no_shared_bar_is_no_shift() {
        assert_eq!(measure(&[bar(0, 10.0)], &[bar(1, 10.0)], at(0)), None);
        assert_eq!(measure(&[], &[bar(1, 10.0)], at(0)), None);
        assert_eq!(place(&[bar(0, 10.0)], &[bar(1, 10.0)], at(0)), None);
    }

    #[test]
    fn the_contract_takes_over_on_the_day_its_volume_reaches_the_front() {
        // Thin for days 0-3 (a print a day), the lead on day 4, the front's level 300 lower.
        let dated: Vec<Bar> = (0..8).map(|d| traded(d, 1300.0 + d as f64, if d < 4 { 3.0 } else { 900.0 + d as f64 })).collect();
        let cont: Vec<Bar> = (0..8).map(|d| traded(d, 1000.0 + d as f64, 900.0)).collect();
        assert_eq!(roll(&dated, &cont), Some(at(4)));
        assert_eq!(place(&dated, &cont, at(0)), Some((at(4), 300.0)));
    }

    #[test]
    fn a_month_that_never_led_keeps_its_listing() {
        let dated: Vec<Bar> = (0..5).map(|d| traded(d, 1300.0, 3.0)).collect();
        let cont: Vec<Bar> = (0..5).map(|d| traded(d, 1000.0, 900.0)).collect();
        assert_eq!(roll(&dated, &cont), None);
        assert_eq!(place(&dated, &cont, at(0)), Some((at(0), 300.0)));
    }

    #[test]
    fn a_day_with_no_reported_volume_is_not_a_roll() {
        let dated = vec![traded(0, 10.0, -1.0), traded(1, 10.0, 50.0)];
        let cont = vec![traded(0, 9.0, -1.0), traded(1, 9.0, 40.0)];
        assert_eq!(roll(&dated, &cont), Some(at(1)));
    }

    #[test]
    fn only_bars_before_the_junction_are_shifted_in() {
        let j = Junction { continuous: "MNQ@CME".into(), until: at(2), offset: 325.5 };
        let out = shift(vec![bar(0, 100.0), bar(1, 101.0), bar(2, 102.0)], &j);
        assert_eq!(out.len(), 2);
        assert_eq!((out[0].open, out[0].high, out[0].low, out[0].close, out[0].volume), (425.5, 426.5, 424.5, 425.5, 10.0));
        assert_eq!(out[1].close, 426.5);
    }
}
