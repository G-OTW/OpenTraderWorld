//! MEXC orders through the shared trader: the account read as before (its spot and USDT
//! perpetual books), each order on its instrument's book (`spot`, `futures`), each book followed
//! on its own private socket (the mapper is `mexc/trade.rs`). MEXC has no sandbox: orders go
//! out at E2D only on an account whose owner acknowledged that the broker was never tested
//! against the venue; the spot order test runs before the first spot order of each session.

#![deny(clippy::float_arithmetic)]

use std::sync::{Arc, OnceLock};

use otw_connector_engine::trading::Trader;
use otw_connector_providers::mexc::Mexc;

use super::native::{NativeBroker, NativeMultiBroker};
use super::native_trade::{NativeTradingBroker, TradeBook};

static MEXC_SPOT: OnceLock<Arc<Trader<Mexc>>> = OnceLock::new();
static MEXC_FUTURES: OnceLock<Arc<Trader<Mexc>>> = OnceLock::new();

fn shared(cell: &'static OnceLock<Arc<Trader<Mexc>>>) -> Arc<Trader<Mexc>> {
    cell.get_or_init(|| Arc::new(Trader::new())).clone()
}

/// MEXC: read over its two books as before; orders on spot and the USDT perpetuals.
pub fn mexc() -> Option<NativeTradingBroker<Mexc, NativeMultiBroker<Mexc>>> {
    let read = super::native::mexc()?;
    let (spot, futures) = crate::histdata::native::mexc_drivers()?;
    Some(NativeTradingBroker::new(
        read,
        "books",
        vec![
            TradeBook { name: "spot", broker: NativeBroker::new(spot, &super::caps::MEXC), trader: shared(&MEXC_SPOT) },
            TradeBook { name: "futures", broker: NativeBroker::new(futures, &super::caps::MEXC), trader: shared(&MEXC_FUTURES) },
        ],
    ))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::super::Broker;
    use otw_connector_model::{ExecLevel, OrderType, PositionModeCap};

    /// Both books take orders at E2D, each with its own caps: the futures book in hedge mode
    /// only, the spot book with the venue's order test.
    #[test]
    fn mexc_trades_both_books_at_e2d() {
        let b = super::mexc().unwrap();
        let caps = b.order_caps().unwrap();
        assert_eq!(caps.level, ExecLevel::E2D);
        assert!(caps.types.contains(&OrderType::Limit) && caps.types.contains(&OrderType::Market) && caps.client_id && caps.cancel_all_native);
        assert!(!caps.types.contains(&OrderType::Stop), "no stop on either book");
        assert_eq!(b.capability().broker, "mexc");
        let fut = otw_connector_providers::mexc::Mexc::futures().unwrap();
        assert_eq!(otw_connector_sdk::Mapper::capabilities(&fut).orders.position_mode, Some(PositionModeCap::Hedge));
    }
}
