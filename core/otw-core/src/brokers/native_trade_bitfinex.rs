//! Bitfinex orders through the shared trader: the account read as before (one key, both books,
//! read by the spot adapter), orders on the instrument's book (the mapper is
//! `bitfinex/trade.rs`): `spot` (the exchange wallet's pairs) and `derivatives` (the
//! perpetuals), the names the data connector's `market` setting uses. Bitfinex has no test
//! host: every account is live, so orders go out at E2D only on an account whose owner
//! acknowledged that the broker was never tested against the venue. Its client ids are minted
//! by OTW and stored with each leg ([`super::Broker::mint_client_ids`]).

#![deny(clippy::float_arithmetic)]

use std::sync::{Arc, OnceLock};

use otw_connector_engine::trading::Trader;
use otw_connector_providers::bitfinex::Bitfinex;

use super::native::NativeBroker;
use super::native_trade::{NativeTradingBroker, TradeBook};

static BITFINEX_SPOT: OnceLock<Arc<Trader<Bitfinex>>> = OnceLock::new();
static BITFINEX_DERIVATIVES: OnceLock<Arc<Trader<Bitfinex>>> = OnceLock::new();

fn shared(cell: &'static OnceLock<Arc<Trader<Bitfinex>>>) -> Arc<Trader<Bitfinex>> {
    cell.get_or_init(|| Arc::new(Trader::new())).clone()
}

/// Bitfinex: read as before; orders on `spot` and `derivatives`, each on its instrument's book.
pub fn bitfinex() -> Option<NativeTradingBroker<Bitfinex, NativeBroker<Bitfinex>>> {
    let read = super::native::bitfinex()?;
    let variants = crate::histdata::native::bitfinex_variants()?;
    let driver = |name: &str| variants.iter().find(|v| v.names.first() == Some(&name)).map(|v| v.driver.clone());
    let (spot, derivatives) = (driver("spot")?, driver("derivatives")?);
    Some(
        NativeTradingBroker::new(
            read,
            "books",
            vec![
                TradeBook { name: "spot", broker: NativeBroker::new(spot, &super::caps::BITFINEX), trader: shared(&BITFINEX_SPOT) },
                TradeBook { name: "derivatives", broker: NativeBroker::new(derivatives, &super::caps::BITFINEX), trader: shared(&BITFINEX_DERIVATIVES) },
            ],
        )
        // The data connector's `market` setting also takes these.
        .aliases(&[("perp", "derivatives"), ("futures", "derivatives")]),
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::super::Broker;
    use otw_connector_model::{ExecLevel, OrderType, PositionModeCap};

    /// Both books take orders at E2D with their own caps: a net position and reduce-only on
    /// the perpetuals only; client ids carried (minted); no native cancel all (the kill switch
    /// names its orders).
    #[test]
    fn bitfinex_trades_both_books_at_e2d() {
        let b = super::bitfinex().unwrap();
        let caps = b.order_caps().unwrap();
        assert_eq!(caps.level, ExecLevel::E2D);
        assert!(caps.types.contains(&OrderType::StopLimit) && caps.client_id && caps.amend && !caps.cancel_all_native);
        assert_eq!(caps.position_mode, Some(PositionModeCap::Net));
        assert_eq!(b.capability().broker, "bitfinex");
    }
}
