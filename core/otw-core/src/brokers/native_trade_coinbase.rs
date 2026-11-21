//! Coinbase Advanced Trade orders through the shared trader ([`NativeTradingBroker`]): the
//! account read as before ([`super::native::coinbase`]), orders on its one book (`spot`, the
//! mapper is `coinbase/trade.rs`). Orders follow `spot:SYMBOL:venue id`. Coinbase has no test
//! account (its sandbox answers canned responses), so every account is live: orders go out at
//! E2D only on an account whose owner acknowledged that the broker was never tested against the
//! venue, the venue's preview running before the first order of each session.

#![deny(clippy::float_arithmetic)]

use std::sync::{Arc, OnceLock};

use otw_connector_engine::trading::Trader;
use otw_connector_providers::coinbase::Coinbase;

use super::native::NativeBroker;
use super::native_trade::{NativeTradingBroker, TradeBook};

static COINBASE_SPOT: OnceLock<Arc<Trader<Coinbase>>> = OnceLock::new();

fn shared(cell: &'static OnceLock<Arc<Trader<Coinbase>>>) -> Arc<Trader<Coinbase>> {
    cell.get_or_init(|| Arc::new(Trader::new())).clone()
}

/// Coinbase: read as before; orders on its spot book.
pub fn coinbase() -> Option<NativeTradingBroker<Coinbase, NativeBroker<Coinbase>>> {
    let read = super::native::coinbase()?;
    let d = crate::histdata::native::coinbase_driver()?;
    Some(NativeTradingBroker::new(read, "books", vec![TradeBook { name: "spot", broker: NativeBroker::new(d, &super::caps::COINBASE), trader: shared(&COINBASE_SPOT) }]))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use std::collections::HashMap;

    use otw_connector_model::{Decimal, ExecLevel, OrderType, Side, TifKind};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::super::{broker_for, serve_from, OrderLeg, OrderTicket, LIVE_UNTESTED};

    const PRODUCTS: &str = include_str!("../../../connectors/providers/src/coinbase/fixtures/trade/live_market_products_btc.json");
    const PREVIEW: &str = include_str!("../../../connectors/providers/src/coinbase/fixtures/trade/sandbox_preview.json");
    const CREATE: &str = include_str!("../../../connectors/providers/src/coinbase/fixtures/trade/sandbox_create.json");

    /// A key made for tests (RFC 8032 vector 1's seed), and a key name Coinbase never issued.
    fn creds(untested: bool) -> HashMap<String, String> {
        let mut m: HashMap<String, String> = [
            ("key_name", "organizations/00000000-0000-0000-0000-000000000000/apiKeys/00000000-0000-0000-0000-000000000000"),
            ("api_private_key", "nWGxne/9WmC6hEr0kuwsxERJxWl7MmkZcDusAxyuf2A="),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        if untested {
            m.insert(LIVE_UNTESTED.into(), "1".into());
        }
        m
    }

    fn limit(key: &str) -> OrderTicket {
        let mut leg = OrderLeg::new(key, Side::Buy, OrderType::Limit, Decimal::new(1, 3));
        leg.limit = Some(Decimal::from(30_000));
        leg.tif = Some(TifKind::Gtc);
        OrderTicket { ticker: "BTC-USD".into(), asset_type: "crypto".into(), legs: vec![leg], provider: None, connector: None, reference: None, reference_at: None, confirm_duplicate: false }
    }

    #[test]
    fn coinbase_takes_spot_orders_at_e2d() {
        let b = broker_for("coinbase").unwrap();
        let caps = b.order_caps().unwrap();
        assert_eq!(caps.level, ExecLevel::E2D);
        assert!(caps.types.contains(&OrderType::StopLimit) && caps.types.contains(&OrderType::Lit) && !caps.types.contains(&OrderType::Stop), "{caps:?}");
        assert!(caps.amend && caps.post_only && caps.client_id && !caps.reduce_only && !caps.brackets && caps.position_mode.is_none(), "{caps:?}");
        assert!(b.has_dry_run());
        assert_eq!(super::super::live::level(b.as_ref()), ExecLevel::E2D);
    }

    /// Refused live without the owner's acknowledgement (no test account); with it, the venue's
    /// preview first, once per session, then the order, followed as `spot:SYMBOL:order_id`.
    #[tokio::test]
    async fn a_coinbase_order_runs_the_preview_first_and_is_followed_by_its_id() {
        let server = MockServer::start().await;
        let ok = |b: &str| ResponseTemplate::new(200).set_body_string(b.to_string());
        Mock::given(method("GET")).and(path("/api/v3/brokerage/market/products")).respond_with(ok(PRODUCTS)).mount(&server).await;
        Mock::given(method("POST")).and(path("/api/v3/brokerage/orders/preview")).respond_with(ok(PREVIEW)).mount(&server).await;
        Mock::given(method("POST")).and(path("/api/v3/brokerage/orders")).respond_with(ok(CREATE)).mount(&server).await;
        serve_from(&server.uri());
        let b = broker_for("coinbase").unwrap();
        let e = b.place(&creds(false), &limit("cb0")).await.unwrap_err().to_string();
        assert!(e.contains("never tested against the venue"), "{e}");
        let s = creds(true);
        assert!(b.is_live(&s).await.unwrap(), "no test account: every account is live");
        let placed = b.place(&s, &limit("cb1")).await.unwrap();
        assert_eq!(placed[0].id, "spot:BTC-USD:f898eaf4-6ffc-47be-a159-7ff292e5cdcf");
        b.place(&s, &limit("cb2")).await.unwrap();
        let reqs = server.received_requests().await.unwrap();
        assert_eq!(reqs.iter().filter(|r| r.url.path() == "/api/v3/brokerage/orders/preview").count(), 1, "checked once per session");
        let sent: Vec<String> = reqs.iter().filter(|r| r.url.path() == "/api/v3/brokerage/orders").map(|r| String::from_utf8_lossy(&r.body).to_string()).collect();
        assert_eq!(sent.len(), 2);
        assert!(sent[0].contains("\"client_order_id\":\"cb1\"") && sent[0].contains("\"limit_limit_gtc\"") && sent[0].contains("\"base_size\":\"0.001\""), "{}", sent[0]);
        assert!(reqs.iter().filter(|r| r.method.as_str() == "POST").all(|r| r.headers.get("authorization").is_some_and(|v| v.to_str().unwrap_or("").starts_with("Bearer "))));
        // Spot holds no positions: nothing to close.
        let e = b.close_position(&s, "BTC-USD", "", "c").await.unwrap_err().to_string();
        assert!(e.contains("not positions"), "{e}");
    }
}
