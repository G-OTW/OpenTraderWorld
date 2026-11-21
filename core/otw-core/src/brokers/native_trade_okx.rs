//! OKX orders: the account read as before (one read over every book, [`super::native::okx`]),
//! orders on three books through the shared trader, each order on its instrument's book:
//! `spot` (SPOT), `swap` (perpetual SWAP) and `futures` (expiry FUTURES), linear contracts only.
//! The read rows name no book, so the orders panel and the positions are given the ids orders
//! are followed by (`BOOK:SYMBOL:ordId`, `BOOK:SYMBOL:side`) here, the book told from OKX's own
//! instrument id (`BTC-USDT` spot, `BTC-USDT-SWAP` swap, `BTC-USDT-250808` futures: the
//! overview's naming table). Everything else is [`super::native_trade::NativeTradingBroker`]'s.

#![deny(clippy::float_arithmetic)]

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use anyhow::Result;
use otw_connector_engine::trading::Trader;
use otw_connector_model::Decimal;
use otw_connector_providers::okx::Okx;
use otw_connector_sdk::Driver;

use super::native::NativeBroker;
use super::native_trade::{NativeTradingBroker, TradeBook};
use super::{AccountSummary, Broker, BrokerCapability, ExecQuery, Execution, Grid, Holding, OpenOrder, Position, Quote};

static SPOT_DRIVER: OnceLock<Option<Arc<Driver<Okx>>>> = OnceLock::new();
static SWAP_DRIVER: OnceLock<Option<Arc<Driver<Okx>>>> = OnceLock::new();
static FUTURES_DRIVER: OnceLock<Option<Arc<Driver<Okx>>>> = OnceLock::new();
static SPOT: OnceLock<Arc<Trader<Okx>>> = OnceLock::new();
static SWAP: OnceLock<Arc<Trader<Okx>>> = OnceLock::new();
static FUTURES: OnceLock<Arc<Trader<Okx>>> = OnceLock::new();

fn trader(cell: &'static OnceLock<Arc<Trader<Okx>>>) -> Arc<Trader<Okx>> {
    cell.get_or_init(|| Arc::new(Trader::new())).clone()
}

/// The book of an OKX instrument id, by the doc's naming: `-SWAP` a perpetual, a dated third
/// part an expiry future, two parts a spot pair. Anything else (an option) is on no book.
pub(crate) fn book_of(symbol: &str) -> Option<&'static str> {
    let parts: Vec<&str> = symbol.split('-').collect();
    match parts.as_slice() {
        [_, _] => Some("spot"),
        [_, _, "SWAP"] => Some("swap"),
        [_, _, date] if !date.is_empty() && date.chars().all(|c| c.is_ascii_digit()) => Some("futures"),
        _ => None,
    }
}

/// The read orders under the ids the trading books follow them by (`BOOK:SYMBOL:id`). An algo
/// order is read as `cond:<algoId>`: given its book like a plain one.
fn booked(rows: &mut [OpenOrder]) {
    for o in rows.iter_mut().filter(|o| !o.id.contains(':') || o.id.starts_with("cond:")) {
        if let Some(b) = book_of(&o.symbol) {
            o.id = format!("{b}:{}:{}", o.symbol, o.id);
        }
    }
}

/// OKX's read broker, its rows given the ids the trading books follow them by.
pub struct OkxRead(NativeBroker<Okx>);

#[async_trait::async_trait]
impl Broker for OkxRead {
    fn capability(&self) -> &'static BrokerCapability {
        self.0.capability()
    }

    fn validate_config(&self, config: &HashMap<String, String>) -> Result<()> {
        self.0.validate_config(config)
    }

    async fn test(&self, client: &reqwest::Client, s: &HashMap<String, String>) -> Result<String> {
        self.0.test(client, s).await
    }

    fn chart_fills(&self) -> bool {
        self.0.chart_fills()
    }

    async fn executions(&self, client: &reqwest::Client, s: &HashMap<String, String>, q: &ExecQuery) -> Result<Vec<Execution>> {
        self.0.executions(client, s, q).await
    }

    async fn symbols(&self, client: &reqwest::Client, s: &HashMap<String, String>) -> Result<Vec<String>> {
        self.0.symbols(client, s).await
    }

    async fn positions(&self, client: &reqwest::Client, s: &HashMap<String, String>) -> Result<Vec<Position>> {
        let mut rows = self.0.positions(client, s).await?;
        for p in rows.iter_mut() {
            if let Some(b) = book_of(&p.symbol).filter(|b| *b != "spot") {
                p.native_id = Some(format!("{b}:{}:{}", p.symbol, p.side));
            }
        }
        Ok(rows)
    }

    async fn held_asset(&self, s: &HashMap<String, String>, ticker: &str) -> Result<Option<Decimal>> {
        self.0.held_asset(s, ticker).await
    }

    async fn orders(&self, client: &reqwest::Client, s: &HashMap<String, String>) -> Result<Vec<OpenOrder>> {
        let mut rows = self.0.orders(client, s).await?;
        booked(&mut rows);
        Ok(rows)
    }

    async fn marks(&self, s: &HashMap<String, String>, ids: &[String]) -> Result<HashMap<String, f64>> {
        self.0.marks(s, ids).await
    }

    async fn account(&self, s: &HashMap<String, String>, symbol: Option<&str>) -> Result<Vec<AccountSummary>> {
        self.0.account(s, symbol).await
    }

    async fn events(&self, s: &HashMap<String, String>) -> Result<Option<futures::stream::BoxStream<'static, super::AccountEvent>>> {
        self.0.events(s).await
    }

    async fn holdings(&self, client: &reqwest::Client, s: &HashMap<String, String>) -> Result<Vec<Holding>> {
        self.0.holdings(client, s).await
    }

    async fn quotes(&self, client: &reqwest::Client, s: &HashMap<String, String>, symbols: &[String]) -> Result<Vec<Quote>> {
        self.0.quotes(client, s, symbols).await
    }

    async fn symbol_for(&self, s: &HashMap<String, String>, ticker: &str, asset_type: &str) -> Result<Option<String>> {
        self.0.symbol_for(s, ticker, asset_type).await
    }

    async fn grid(&self, s: &HashMap<String, String>, provider: Option<&str>, ticker: &str, asset_type: &str) -> Result<Option<Grid>> {
        self.0.grid(s, provider, ticker, asset_type).await
    }
}

/// OKX: read over every book as before; orders on spot, swap and futures, each on its
/// instrument's book.
pub fn okx() -> Option<NativeTradingBroker<Okx, OkxRead>> {
    let read = OkxRead(super::native::okx()?);
    use crate::histdata::native::shared_driver;
    let spot = shared_driver(&SPOT_DRIVER, "okx", || Okx::spot().ok())?;
    let swap = shared_driver(&SWAP_DRIVER, "okx", || Okx::swap().ok())?;
    let futures = shared_driver(&FUTURES_DRIVER, "okx", || Okx::futures().ok())?;
    Some(NativeTradingBroker::new(
        read,
        "books",
        vec![
            TradeBook { name: "spot", broker: NativeBroker::new(spot, &super::caps::OKX), trader: trader(&SPOT) },
            TradeBook { name: "swap", broker: NativeBroker::new(swap, &super::caps::OKX), trader: trader(&SWAP) },
            TradeBook { name: "futures", broker: NativeBroker::new(futures, &super::caps::OKX), trader: trader(&FUTURES) },
        ],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_book_is_told_from_the_instrument_id() {
        assert_eq!(book_of("BTC-USDT"), Some("spot"));
        assert_eq!(book_of("BTC-USDT-SWAP"), Some("swap"));
        assert_eq!(book_of("BTC-USDT-250808"), Some("futures"));
        assert_eq!(book_of("BTC-USD-241227-50000-C"), None);
        assert_eq!(book_of("BTC"), None);
    }

    #[test]
    fn plain_and_algo_orders_are_given_their_book() {
        let row = |id: &str, symbol: &str| OpenOrder {
            id: id.into(),
            symbol: symbol.into(),
            side: "buy".into(),
            order_type: "limit".into(),
            qty: 1.0,
            filled_qty: 0.0,
            limit_price: None,
            stop_price: None,
            trail_start: None,
            trail_rate: None,
            currency: String::new(),
            asset_class: String::new(),
            placed_at: None,
            venue: String::new(),
            account: String::new(),
            parent_id: None,
            reduce_only: false,
        };
        let mut rows = vec![row("11", "BTC-USDT"), row("cond:42", "BTC-USDT-SWAP"), row("swap:BTC-USDT-SWAP:9", "BTC-USDT-SWAP")];
        booked(&mut rows);
        let ids: Vec<&str> = rows.iter().map(|o| o.id.as_str()).collect();
        assert_eq!(ids, ["spot:BTC-USDT:11", "swap:BTC-USDT-SWAP:cond:42", "swap:BTC-USDT-SWAP:9"]);
    }

    #[test]
    fn okx_registers_with_three_books_that_take_orders() {
        let b = okx().expect("okx broker");
        let caps = b.order_caps().expect("order caps");
        assert_eq!(caps.level, otw_connector_model::ExecLevel::E2D);
        assert!(caps.reduce_only && caps.post_only && caps.amend && !caps.brackets, "{caps:?}");
        assert_eq!(b.capability().broker, "okx");
    }
}
