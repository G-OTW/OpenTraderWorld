//! KuCoin and KuCoin Futures orders through the shared trader ([`NativeTradingBroker`]): each
//! platform is its own broker (its own host, symbols and key permission), read as before and
//! traded on its one book (`spot`, `futures`). Orders follow `BOOK:SYMBOL:venue id`. KuCoin has
//! no sandbox: every account is live, so orders go out at E2D only on an account whose owner
//! acknowledged that the broker was never tested against the venue, the venue's order test
//! running before the first order of each session.

#![deny(clippy::float_arithmetic)]

use std::sync::{Arc, OnceLock};

use otw_connector_engine::trading::Trader;
use otw_connector_providers::kucoin::Kucoin;

use super::native::NativeBroker;
use super::native_trade::{NativeTradingBroker, TradeBook};

static KUCOIN_SPOT: OnceLock<Arc<Trader<Kucoin>>> = OnceLock::new();
static KUCOIN_FUTURES: OnceLock<Arc<Trader<Kucoin>>> = OnceLock::new();

fn shared(cell: &'static OnceLock<Arc<Trader<Kucoin>>>) -> Arc<Trader<Kucoin>> {
    cell.get_or_init(|| Arc::new(Trader::new())).clone()
}

/// KuCoin spot: read as before; orders on its spot book (HF orders).
pub fn kucoin() -> Option<NativeTradingBroker<Kucoin, NativeBroker<Kucoin>>> {
    let read = super::native::kucoin()?;
    let d = crate::histdata::native::kucoin_driver()?;
    Some(NativeTradingBroker::new(read, "books", vec![TradeBook { name: "spot", broker: NativeBroker::new(d, &super::caps::KUCOIN), trader: shared(&KUCOIN_SPOT) }]))
}

/// KuCoin Futures: read as before; orders on its USDT- and USDC-margined contracts.
pub fn kucoin_futures() -> Option<NativeTradingBroker<Kucoin, NativeBroker<Kucoin>>> {
    let read = super::native::kucoin_futures()?;
    let d = crate::histdata::native::kucoin_futures_driver()?;
    Some(NativeTradingBroker::new(read, "books", vec![TradeBook { name: "futures", broker: NativeBroker::new(d, &super::caps::KUCOIN_FUTURES), trader: shared(&KUCOIN_FUTURES) }]))
}

#[cfg(test)]
#[path = "native_trade_kucoin_tests.rs"]
mod tests;
