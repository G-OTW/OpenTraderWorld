//! Kraken spot orders through the shared trader ([`NativeTradingBroker`]): the account read as
//! before, orders on its one book (`spot`). Orders follow `spot:SYMBOL:txid`. Kraken has no spot
//! sandbox: every account is live, so orders go out at E2D only on an account whose owner
//! acknowledged that the broker was never tested against the venue, the venue's validate call
//! (`AddOrder` with `validate=true`) running before the first order of each session. The key's
//! signed calls take turns (its nonce must rise call after call), the read path's included.

#![deny(clippy::float_arithmetic)]

use std::sync::{Arc, OnceLock};

use otw_connector_engine::trading::Trader;
use otw_connector_providers::kraken::Kraken;

use super::native::NativeBroker;
use super::native_trade::{NativeTradingBroker, TradeBook};

static KRAKEN_SPOT: OnceLock<Arc<Trader<Kraken>>> = OnceLock::new();

/// Kraken: read as before; orders on its spot book.
pub fn kraken() -> Option<NativeTradingBroker<Kraken, NativeBroker<Kraken>>> {
    let read = super::native::kraken()?;
    let d = crate::histdata::native::kraken_driver()?;
    let trader = KRAKEN_SPOT.get_or_init(|| Arc::new(Trader::new())).clone();
    Some(NativeTradingBroker::new(read, "books", vec![TradeBook { name: "spot", broker: NativeBroker::new(d, &super::caps::KRAKEN), trader }]))
}
