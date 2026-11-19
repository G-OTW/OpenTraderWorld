//! Gate.io orders through the shared trader: the account read as before (spot and USDT
//! futures books), each order on its instrument's book (`spot`, `futures`), each book followed
//! on its own private socket.

#![deny(clippy::float_arithmetic)]

use std::sync::{Arc, OnceLock};

use otw_connector_engine::trading::Trader;
use otw_connector_providers::gateio::Gateio;

use super::native::{NativeBroker, NativeMultiBroker};
use super::native_trade::{NativeTradingBroker, TradeBook};

static GATEIO_SPOT: OnceLock<Arc<Trader<Gateio>>> = OnceLock::new();
static GATEIO_FUTURES: OnceLock<Arc<Trader<Gateio>>> = OnceLock::new();

fn shared(cell: &'static OnceLock<Arc<Trader<Gateio>>>) -> Arc<Trader<Gateio>> {
    cell.get_or_init(|| Arc::new(Trader::new())).clone()
}

/// Gate.io: read over its two books as before; orders on spot and the USDT perpetuals.
pub fn gateio() -> Option<NativeTradingBroker<Gateio, NativeMultiBroker<Gateio>>> {
    let read = super::native::gateio()?;
    let (spot, futures) = crate::histdata::native::gateio_drivers()?;
    Some(NativeTradingBroker::new(
        read,
        "books",
        vec![
            TradeBook { name: "spot", broker: NativeBroker::new(spot, &super::caps::GATEIO), trader: shared(&GATEIO_SPOT) },
            TradeBook { name: "futures", broker: NativeBroker::new(futures, &super::caps::GATEIO), trader: shared(&GATEIO_FUTURES) },
        ],
    )
    // The data connector's `market` and the `books` setting also take `usdt-futures`.
    .aliases(&[("usdt-futures", "futures")]))
}
