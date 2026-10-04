//! How long a pull will take, said before it is asked for.
//!
//! The import modals ask first, and past a few seconds they offer to let the pull run in
//! the background while the user does something else. The number is a model of each
//! connector's paging (how many calls a window costs, what the venue makes it wait between
//! them), corrected by what the last pulls from the same broker actually took. It decides
//! which button to show, never whether a pull is allowed.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use time::OffsetDateTime;

/// Round trip of one signed call, network and venue included.
const CALL_SECS: f64 = 0.35;
/// Signing in, minting a token, reading the account: paid once per pull.
const SETUP_SECS: f64 = 1.0;
/// An IBKR Flex statement is generated on demand and polled until it is ready.
const FLEX_SECS: f64 = 20.0;
/// Past this, the modal offers to run the pull in the background.
pub const LONG_SECS: f64 = 8.0;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Executions,
    Holdings,
}

/// What the connector's paging alone predicts for a window of fills.
pub(crate) fn model_executions(broker: &str, from: OffsetDateTime, to: OffsetDateTime, symbols: usize) -> f64 {
    let days = ((to.min(OffsetDateTime::now_utc()) - from).as_seconds_f64() / 86_400.0).max(1.0);
    let n = symbols.max(1) as f64;
    // Windows the venue cuts the period into, each at least one call.
    let slices = |len: f64| (days / len).ceil().max(1.0);
    // An active account fills about one page every two months on a cursor-paged endpoint.
    let pages = 1.0 + days / 60.0;
    let calls = match broker {
        "ibkr_flex" => return FLEX_SECS,
        // Pages sleep 1.2 s apart to stay under the decaying call counter.
        "kraken" => return SETUP_SECS + (1.0 + days / 30.0) * (CALL_SECS + 1.2),
        // One quote-asset lookup, then each symbol paged on its own.
        "binance" => 1.0 + n * (1.0 + days / 180.0),
        "binance_futures" => n * slices(7.0),
        // Spot in 90-day slices, futures in weekly ones.
        "bitget" => slices(90.0) + slices(7.0),
        // Every instrument type asked in turn.
        "okx" => 5.0 * pages,
        // The activity log answers one day per call.
        "capitalcom" => slices(1.0),
        "oanda" => 2.0 * slices(365.0) + days / 90.0,
        _ => pages,
    };
    SETUP_SECS + calls * CALL_SECS
}

/// What reading the balance sheet costs: one call, except where it is a statement.
pub(crate) fn model_holdings(broker: &str) -> f64 {
    match broker {
        "ibkr_flex" => FLEX_SECS,
        _ => SETUP_SECS + 2.0 * CALL_SECS,
    }
}

/// How far each broker's pulls ran from the model lately (actual / predicted).
fn ratios() -> &'static Mutex<HashMap<(String, Kind), f64>> {
    static R: OnceLock<Mutex<HashMap<(String, Kind), f64>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(HashMap::new()))
}

fn ratio(broker: &str, kind: Kind) -> f64 {
    ratios()
        .lock()
        .ok()
        .and_then(|r| r.get(&(broker.to_string(), kind)).copied())
        .unwrap_or(1.0)
}

/// Fold a finished pull into the correction. Half old, half new: one slow answer from a
/// busy venue moves the next estimate without taking it over.
pub(crate) fn learn(broker: &str, kind: Kind, model: f64, took: Duration) {
    if model <= 0.0 {
        return;
    }
    let seen = (took.as_secs_f64() / model).clamp(0.2, 5.0);
    if let Ok(mut r) = ratios().lock() {
        let e = r.entry((broker.to_string(), kind)).or_insert(seen);
        *e = (*e + seen) / 2.0;
    }
}

pub fn executions(broker: &str, from: OffsetDateTime, to: OffsetDateTime, symbols: usize) -> f64 {
    model_executions(broker, from, to, symbols) * ratio(broker, Kind::Executions)
}

pub fn holdings(broker: &str) -> f64 {
    model_holdings(broker) * ratio(broker, Kind::Holdings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_weekly_sliced_venue_grows_with_the_window_and_the_symbols() {
        let to = OffsetDateTime::now_utc();
        let short = model_executions("binance_futures", to - time::Duration::days(7), to, 1);
        let long = model_executions("binance_futures", to - time::Duration::days(90), to, 10);
        assert!(short < LONG_SECS, "{short}");
        assert!(long > LONG_SECS, "{long}");
    }

    #[test]
    fn a_flex_statement_is_always_long() {
        let to = OffsetDateTime::now_utc();
        assert!(model_executions("ibkr_flex", to - time::Duration::days(1), to, 0) > LONG_SECS);
        assert!(model_holdings("ibkr_flex") > LONG_SECS);
        assert!(model_holdings("binance") < LONG_SECS);
    }
}
