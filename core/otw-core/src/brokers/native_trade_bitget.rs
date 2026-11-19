//! Bitget orders through the shared trader: the account read as before (its spot and USDT-M
//! futures books), orders on the instrument's book (the mapper is `bitget/trade.rs`).

#![deny(clippy::float_arithmetic)]

use std::sync::{Arc, OnceLock};

use otw_connector_engine::trading::Trader;
use otw_connector_providers::bitget::Bitget;

use super::native::{NativeBroker, NativeMultiBroker};
use super::native_trade::{NativeTradingBroker, TradeBook};

static BITGET_SPOT: OnceLock<Arc<Trader<Bitget>>> = OnceLock::new();
static BITGET_FUTURES: OnceLock<Arc<Trader<Bitget>>> = OnceLock::new();

fn shared(cell: &'static OnceLock<Arc<Trader<Bitget>>>) -> Arc<Trader<Bitget>> {
    cell.get_or_init(|| Arc::new(Trader::new())).clone()
}

/// Bitget: read over its two books as before; orders on `spot` and `usdt-futures` (the names
/// the account's `books` setting and the data connector's `market` setting use).
pub fn bitget() -> Option<NativeTradingBroker<Bitget, NativeMultiBroker<Bitget>>> {
    let read = super::native::bitget()?;
    let (spot, futures) = crate::histdata::native::bitget_drivers()?;
    Some(NativeTradingBroker::new(
        read,
        "books",
        vec![
            TradeBook { name: "spot", broker: NativeBroker::new(spot, &super::caps::BITGET), trader: shared(&BITGET_SPOT) },
            TradeBook { name: "usdt-futures", broker: NativeBroker::new(futures, &super::caps::BITGET), trader: shared(&BITGET_FUTURES) },
        ],
    )
    // The data connector's `market` and the `books` setting also take `futures`.
    .aliases(&[("futures", "usdt-futures")]))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::super::Broker;
    use otw_connector_model::{ExecLevel, OrderType};

    /// Both books take orders at E2D, each with its own caps: no stop-limit, reduce-only or
    /// position on spot; futures with both position modes.
    #[test]
    fn bitget_trades_both_books_at_e2d() {
        let b = super::bitget().unwrap();
        let caps = b.order_caps().unwrap();
        assert_eq!(caps.level, ExecLevel::E2D);
        assert!(caps.types.contains(&OrderType::Stop) && caps.client_id && caps.cancel_all_native);
        assert_eq!(b.capability().broker, "bitget");
    }
}
