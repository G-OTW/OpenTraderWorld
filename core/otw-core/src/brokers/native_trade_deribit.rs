//! Deribit orders through the shared trader: the account read as before (every kind at once,
//! on the host of the account's `env`), orders on its futures and perpetuals, one book
//! (`futures`): linear USDC contracts sized in the coin, inverse ones sized in USD (the ticket
//! offers the size in the coin or in USD there, [`super::Broker::quote_sized`]). Options, combos
//! and spot are refused by name. Orders follow `futures:SYMBOL:venue id`; a stop that fired as a
//! new order is followed to it by the trader. An account set to `env` `test` reads and trades on
//! test.deribit.com (the sandbox: orders at any level); a live one takes orders at E2D only once
//! its owner acknowledged that the broker was never tested against the venue.

#![deny(clippy::float_arithmetic)]

use std::sync::{Arc, OnceLock};

use otw_connector_engine::trading::Trader;
use otw_connector_providers::deribit::Deribit;

use super::native::NativeBroker;
use super::native_trade::{NativeTradingBroker, TradeBook};

static DERIBIT_FUTURES: OnceLock<Arc<Trader<Deribit>>> = OnceLock::new();

/// Deribit: read as before; orders on its futures and perpetuals.
pub fn deribit() -> Option<NativeTradingBroker<Deribit, NativeBroker<Deribit>>> {
    let read = super::native::deribit()?;
    let d = crate::histdata::native::deribit_driver()?;
    let trader = DERIBIT_FUTURES.get_or_init(|| Arc::new(Trader::new())).clone();
    Some(NativeTradingBroker::new(read, "books", vec![TradeBook { name: "futures", broker: NativeBroker::new(d, &super::caps::DERIBIT), trader }]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brokers::Broker;

    #[test]
    fn deribit_registers_with_its_futures_book_that_takes_orders() {
        let b = deribit().expect("deribit broker");
        let caps = b.order_caps().expect("order caps");
        assert_eq!(caps.level, otw_connector_model::ExecLevel::E2D);
        assert!(caps.reduce_only && caps.post_only && caps.amend && caps.cancel_all_native && !caps.brackets && caps.client_id, "{caps:?}");
        assert_eq!(b.capability().broker, "deribit");
        assert!(b.capability().config_fields.iter().any(|f| f.name == "env"));
        let mut bad = std::collections::HashMap::new();
        bad.insert("env".to_string(), "demo".to_string());
        assert!(b.validate_config(&bad).is_err());
        bad.insert("env".to_string(), "test".to_string());
        assert!(b.validate_config(&bad).is_ok());
    }

    #[tokio::test]
    async fn the_test_environment_is_the_sandbox_and_a_live_account_needs_the_acknowledgement() {
        let b = deribit().expect("deribit broker");
        let mut s = std::collections::HashMap::new();
        s.insert("api_key".to_string(), "k".to_string());
        s.insert("api_secret".to_string(), "s".to_string());
        assert!(b.is_live(&s).await.unwrap());
        s.insert("env".to_string(), "test".to_string());
        assert!(!b.is_live(&s).await.unwrap());
    }
}
