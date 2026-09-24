//! Broker accounts: what *I* did, as opposed to what the market did.
//!
//! The data broker ([`crate::histdata`]) reads prices; this one reads an account:
//! executions, positions and open orders, **read only**. No connector here places, cancels
//! or modifies anything, and none ever will: the credentials the user is asked for are the
//! read-only kind, and a broker that cannot issue a read-only key says so in `key_note`.
//!
//! Each broker declares a [`BrokerCapability`] (which credentials it needs, whether its
//! execution history takes an arbitrary window, whether it must be asked one instrument at
//! a time, how far back it answers) and the consumers, journal import first, drive it
//! generically from that.
//!
//! **A cash account has no shorts.** On a spot venue a sell with nothing open is the sale of
//! a holding bought before the window, not the opening of a short. `spot_only` says so, and
//! the importer reports such a fill as an error naming the fix (widen the period) rather
//! than inventing a short position nobody took.

use std::collections::HashMap;

use anyhow::{anyhow, Result};
use serde::Serialize;
use time::OffsetDateTime;

pub use crate::histdata::ConfigField;

mod alpaca;
mod binance;
mod binance_futures;
mod bitget;
pub(crate) mod capitalcom;
mod coinbase;
pub(crate) mod forexcom;
mod ibkr;
mod kraken;
mod ninjatrader;
mod oanda;
mod okx;
mod tokens;
pub(crate) mod tradestation;

#[cfg(test)]
mod mock_tests;
#[cfg(test)]
mod net_tests;

/// Static description of what a broker's API can answer. Drives the account form, the
/// import modal's controls, and is enforced again server-side before a pull is attempted.
#[derive(Debug, Clone, Serialize)]
pub struct BrokerCapability {
    pub broker: &'static str,
    pub label: &'static str,
    pub website: &'static str,
    /// Link to the broker's API documentation (shown as an info tooltip).
    pub docs_url: &'static str,
    /// Short human note on rate limits for this broker.
    pub rate_limit: &'static str,
    /// One line on what the key must be allowed to do, and what it must *not*.
    pub key_note: &'static str,
    /// Secret names the broker needs. Surfaced on the settings page, write-only.
    pub required_secrets: &'static [&'static str],
    /// Non-secret settings (a Flex query id, a sub-account label).
    pub config_fields: &'static [ConfigField],
    /// Asset classes this account can report, in journal vocabulary.
    pub asset_classes: &'static [&'static str],
    /// Executions (fills) can be pulled.
    pub executions: bool,
    /// The API answers one instrument at a time: the caller must name the symbols.
    pub needs_symbols: bool,
    /// The window is the broker's, not ours: the pull returns whatever its own saved query
    /// covers and the requested period only filters the answer (IBKR Flex).
    pub window_from_broker: bool,
    /// Furthest back the API answers, in days. 0 = no published limit.
    pub history_days: u32,
    /// A cash account: a sell with nothing open is a sale, never a short.
    pub spot_only: bool,
    pub positions: bool,
    pub orders: bool,
    /// What the account owns can be listed (a balance sheet, not a trade history).
    pub holdings: bool,
    /// The venue can be asked what an asset trades at right now.
    pub quotes: bool,
    pub testable: bool,
}

/// One fill, as the broker reports it. Prices and quantities are the broker's own; the
/// symbol is its own spelling and is never rewritten into a guess.
#[derive(Debug, Clone, Serialize)]
pub struct Execution {
    /// The broker's execution id: the identity a re-sync deduplicates on.
    pub id: String,
    /// The order this fill belongs to (empty when the broker reports none).
    pub order_id: String,
    #[serde(with = "time::serde::rfc3339")]
    pub at: OffsetDateTime,
    /// The instrument, spelled as the broker spells it.
    pub symbol: String,
    /// "buy" | "sell".
    pub side: String,
    pub qty: f64,
    pub price: f64,
    /// Commission as a positive cost.
    pub fee: f64,
    /// Currency (or crypto asset) the fee was billed in.
    pub fee_currency: String,
    /// The instrument's quote/settlement currency.
    pub currency: String,
    /// Journal asset class: stock / etf / crypto / forex / future / option.
    pub asset_class: String,
    /// Point value, when the broker reports one (futures). 1 otherwise.
    pub multiplier: f64,
    /// Exchange/venue label, empty when the broker reports none.
    pub venue: String,
    /// Sub-account id, when the broker has several.
    pub account: String,
}

/// One open position as the broker reports it now. Read-only: nothing here can be closed.
#[derive(Debug, Clone, Serialize)]
pub struct Position {
    pub symbol: String,
    /// "long" | "short".
    pub side: String,
    pub qty: f64,
    /// Weighted average cost, when the broker reports one.
    pub avg_price: Option<f64>,
    pub currency: String,
    pub asset_class: String,
    pub multiplier: f64,
    pub venue: String,
    pub account: String,
}

/// One live working order.
#[derive(Debug, Clone, Serialize)]
pub struct OpenOrder {
    pub id: String,
    pub symbol: String,
    /// "buy" | "sell".
    pub side: String,
    /// The broker's own order type, verbatim ("limit", "stop-loss", "LMT"…).
    pub order_type: String,
    pub qty: f64,
    pub filled_qty: f64,
    pub limit_price: Option<f64>,
    pub stop_price: Option<f64>,
    pub currency: String,
    pub asset_class: String,
    #[serde(with = "time::serde::rfc3339::option")]
    pub placed_at: Option<OffsetDateTime>,
    pub venue: String,
    pub account: String,
}

/// One line of what an account owns: an asset and how much of it.
///
/// Deliberately not a [`Position`]. A position has a side and a cost basis because it was
/// opened; a spot balance is a quantity of something the account holds, and the exchange
/// says nothing about how it was acquired. Anything the broker does not report stays
/// `None` rather than being filled in with a plausible number.
#[derive(Debug, Clone, Serialize)]
pub struct Holding {
    /// The asset, spelled as the broker spells it: "BTC", "AAPL", "ESZ4".
    pub symbol: String,
    /// The broker's own description, empty when it gives none.
    pub name: String,
    /// Units held. Always positive; a short is reported with `side` = "short".
    pub qty: f64,
    /// "long" | "short". A spot balance is always long: it is owned.
    pub side: String,
    /// Journal asset class: stock / etf / crypto / forex / future / option.
    pub asset_class: String,
    /// Weighted average cost, only when the broker reports one. A crypto exchange does
    /// not: a balance has no cost basis, and inventing one would misstate every gain.
    pub avg_price: Option<f64>,
    /// Currency `avg_price` is expressed in. Empty when there is no price.
    pub currency: String,
    pub venue: String,
    pub account: String,
}

/// What one asset trades at on this venue right now.
///
/// The price is the venue's, and so is the money it is quoted in: a crypto exchange
/// prices BTC in USDT, not in dollars, and `pair` says which market answered so the
/// number is never mistaken for a dollar price nobody asked for.
#[derive(Debug, Clone, Serialize)]
pub struct Quote {
    /// The asset, spelled as the holding spells it.
    pub symbol: String,
    /// Last traded price, per unit, in `currency`.
    pub price: f64,
    /// The market's quote currency ("USDT", "USD", "EUR").
    pub currency: String,
    /// The market that answered ("BTCUSDT", "XBT/USD"), for the line to show.
    pub pair: String,
}

/// What to pull, in broker-independent terms.
pub struct ExecQuery {
    pub from: OffsetDateTime,
    pub to: OffsetDateTime,
    /// Instruments to ask for, in the broker's spelling. Required when the capability says
    /// `needs_symbols`, ignored by brokers that answer the whole account at once.
    pub symbols: Vec<String>,
}

/// A broker account connector: capability metadata + read-only account queries.
#[async_trait::async_trait]
pub trait Broker: Send + Sync {
    fn capability(&self) -> &'static BrokerCapability;

    /// Reach the API and report what answered, or fail with a message naming what to fix.
    async fn test(
        &self,
        _client: &reqwest::Client,
        _secrets: &HashMap<String, String>,
    ) -> Result<String> {
        Err(anyhow!("{} has no connection test", self.capability().label))
    }

    /// Fills over `[from, to]`. Brokers page internally; the result is the whole window.
    async fn executions(
        &self,
        _client: &reqwest::Client,
        _secrets: &HashMap<String, String>,
        _q: &ExecQuery,
    ) -> Result<Vec<Execution>> {
        Err(anyhow!(
            "{} does not publish an execution history",
            self.capability().label
        ))
    }

    /// Instruments worth offering in a symbol picker: the ones this account actually
    /// holds or has worked. A suggestion list, never a filter, so the user may type another.
    async fn symbols(
        &self,
        _client: &reqwest::Client,
        _secrets: &HashMap<String, String>,
    ) -> Result<Vec<String>> {
        Ok(vec![])
    }

    async fn positions(
        &self,
        _client: &reqwest::Client,
        _secrets: &HashMap<String, String>,
    ) -> Result<Vec<Position>> {
        Err(anyhow!("{} does not report positions", self.capability().label))
    }

    async fn orders(
        &self,
        _client: &reqwest::Client,
        _secrets: &HashMap<String, String>,
    ) -> Result<Vec<OpenOrder>> {
        Err(anyhow!("{} does not report open orders", self.capability().label))
    }

    /// Everything the account owns right now, one line per asset.
    async fn holdings(
        &self,
        _client: &reqwest::Client,
        _secrets: &HashMap<String, String>,
    ) -> Result<Vec<Holding>> {
        Err(anyhow!(
            "{} does not list what the account holds",
            self.capability().label
        ))
    }

    /// What these assets trade at right now, one line per asset that has a market. An
    /// asset the venue does not price is left out rather than priced from somewhere else:
    /// the point of asking the exchange is that the answer comes from the exchange.
    async fn quotes(
        &self,
        _client: &reqwest::Client,
        _secrets: &HashMap<String, String>,
        _symbols: &[String],
    ) -> Result<Vec<Quote>> {
        Err(anyhow!(
            "{} does not publish a live price",
            self.capability().label
        ))
    }

    /// Check an account's non-secret settings before they are stored.
    fn validate_config(&self, _config: &HashMap<String, String>) -> Result<()> {
        Ok(())
    }
}

fn all_brokers() -> Vec<Box<dyn Broker>> {
    vec![
        Box::new(alpaca::Alpaca),
        Box::new(binance::Binance),
        Box::new(binance_futures::BinanceFutures),
        Box::new(bitget::Bitget),
        Box::new(coinbase::Coinbase),
        Box::new(kraken::Kraken),
        Box::new(ibkr::IbkrFlex),
        Box::new(oanda::Oanda),
        Box::new(okx::Okx),
        Box::new(tradestation::TradeStation),
        Box::new(forexcom::ForexCom),
        Box::new(capitalcom::CapitalCom),
        Box::new(ninjatrader::NinjaTrader),
    ]
}

/// Every broker's capability, for the settings page and the import modal.
pub fn capabilities() -> Vec<&'static BrokerCapability> {
    all_brokers().iter().map(|b| b.capability()).collect()
}

/// Look a broker up by id.
pub fn broker_for(broker: &str) -> Result<Box<dyn Broker>> {
    all_brokers()
        .into_iter()
        .find(|b| b.capability().broker == broker)
        .ok_or_else(|| anyhow!("unknown broker: {broker}"))
}

/// The host a connector's requests go to. Every URL a broker builds passes through here, so
/// a test can point the whole connector at a local mock server: a paging loop, a cursor, a
/// rate-limit answer and a refusal wrapped in a 200 are all reachable without an account,
/// and none of them can be reached with one either unless the account happens to hold the
/// right history. Outside tests it is the constant the connector declares, inlined away.
#[cfg(not(test))]
#[inline]
pub(crate) fn at(base: &str) -> String {
    base.to_string()
}

#[cfg(test)]
thread_local! {
    static BASE_OVERRIDE: std::cell::RefCell<Option<String>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn at(base: &str) -> String {
    BASE_OVERRIDE
        .with(|b| b.borrow().clone())
        .unwrap_or_else(|| base.to_string())
}

/// Send every connector call on this thread to `url` instead of the real venue. One
/// `#[tokio::test]` is one thread, so tests do not leak into each other.
#[cfg(test)]
pub(crate) fn serve_from(url: &str) {
    BASE_OVERRIDE.with(|b| *b.borrow_mut() = Some(url.trim_end_matches('/').to_string()));
}

/// HTTP client for account calls. Longer than the market-data one: a Flex statement is
/// generated on demand and a crypto history call pages server-side.
pub fn client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(45))
        .user_agent("OpenTraderWorld/brokers")
        .build()
        .map_err(|e| anyhow!("building broker http client: {e}"))
}

/// Refuse a period the broker's own archive does not reach.
///
/// `history_days` is not decoration: a venue that keeps three months of fills answers an
/// older window with an empty page and no error at all, which reads as "you traded nothing
/// then". The pull is stopped here instead, with the date it *can* answer from, so the
/// user narrows the period rather than filing a quiet hole in their book.
///
/// A broker whose window is its own ([`BrokerCapability::window_from_broker`]) is exempt:
/// the dates only filter a statement it has already built.
pub fn check_window(cap: &BrokerCapability, from: OffsetDateTime) -> Result<()> {
    if cap.history_days == 0 || cap.window_from_broker {
        return Ok(());
    }
    let earliest = OffsetDateTime::now_utc() - time::Duration::days(cap.history_days as i64);
    if from >= earliest {
        return Ok(());
    }
    Err(anyhow!(
        "{} serves {} days of execution history over its API, so it cannot answer for {}. \
         Start the period on {} or later, and take anything older from the broker's own \
         statement export.",
        cap.label,
        cap.history_days,
        from.date(),
        earliest.date(),
    ))
}

/// A required credential or setting that is missing, named the way the user sees it.
pub fn require<'a>(secrets: &'a HashMap<String, String>, name: &str) -> Result<&'a str> {
    secrets
        .get(name)
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("{name} is not set on this broker account"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A broker id is stored on `broker_accounts.broker`, so two connectors answering to one
    /// id would read each other's accounts. It is also never renamed after a release, which
    /// is why this test exists rather than a comment.
    #[test]
    fn every_broker_id_is_its_own_and_its_flags_agree_with_its_methods() {
        let mut seen: Vec<&str> = Vec::new();
        for cap in capabilities() {
            assert!(!cap.broker.is_empty(), "{}: a broker needs an id", cap.label);
            assert!(
                !seen.contains(&cap.broker),
                "two connectors answer to the broker id {}",
                cap.broker
            );
            assert!(!cap.key_note.is_empty(), "{}: owes a note on what its key may do", cap.broker);
            assert!(
                !cap.asset_classes.is_empty(),
                "{}: says nothing about what it can report",
                cap.broker
            );
            // A broker that must be asked one instrument at a time has to be able to
            // suggest some, or the import modal asks for a list nobody can produce.
            assert!(
                !cap.needs_symbols || cap.executions,
                "{}: needs symbols for a history it does not publish",
                cap.broker
            );
            seen.push(cap.broker);
        }
    }

    fn cap(history_days: u32, window_from_broker: bool) -> BrokerCapability {
        BrokerCapability {
            broker: "test",
            label: "Test",
            website: "",
            docs_url: "",
            rate_limit: "",
            key_note: "",
            required_secrets: &[],
            config_fields: &[],
            asset_classes: &[],
            executions: true,
            needs_symbols: false,
            window_from_broker,
            history_days,
            spot_only: false,
            positions: false,
            orders: false,
            holdings: false,
            quotes: false,
            testable: false,
        }
    }

    #[test]
    fn a_bounded_archive_refuses_a_period_it_cannot_answer() {
        let now = OffsetDateTime::now_utc();
        let c = cap(90, false);
        // Inside the window: nothing to say.
        assert!(check_window(&c, now - time::Duration::days(89)).is_ok());
        // Past it, the refusal names the date to start from rather than answering nothing.
        let e = check_window(&c, now - time::Duration::days(200)).unwrap_err();
        let msg = format!("{e}");
        assert!(msg.contains("90 days"), "{msg}");
        assert!(msg.contains(&(now - time::Duration::days(90)).date().to_string()), "{msg}");
    }

    #[test]
    fn a_broker_without_a_published_limit_is_left_alone() {
        let old = OffsetDateTime::now_utc() - time::Duration::days(4000);
        // No published limit at all.
        assert!(check_window(&cap(0, false), old).is_ok());
        // A window that is the broker's own: the dates only filter what it already built,
        // so refusing them would refuse a statement that does cover the period.
        assert!(check_window(&cap(365, true), old).is_ok());
    }
}
