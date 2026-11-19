//! Kraken Futures orders through the shared trader ([`NativeTradingBroker`]): the account read
//! as before ([`super::native::kraken_futures`], live or the demo host by the account's `env`),
//! orders on its one book `futures`, the multi-collateral contracts (`PF_`, `FF_`); the mapper
//! is `kraken_futures/trade.rs`. Orders follow `futures:SYMBOL:order_id`. No private stream:
//! polled only. Live accounts take orders at E2D only once their owner acknowledged that the
//! broker was never tested against the venue; a demo account takes them as a sandbox.

#![deny(clippy::float_arithmetic)]

use std::sync::{Arc, OnceLock};

use otw_connector_engine::trading::Trader;
use otw_connector_providers::kraken_futures::KrakenFutures;

use super::native::NativeBroker;
use super::native_trade::{NativeTradingBroker, TradeBook};

static KRAKEN_FUTURES: OnceLock<Arc<Trader<KrakenFutures>>> = OnceLock::new();

/// Kraken Futures: read as before; orders on its multi-collateral contracts. The book's broker
/// is the read broker's twin (the same live and demo drivers, shared), so an order goes to the
/// host the account reads.
pub fn kraken_futures() -> Option<NativeTradingBroker<KrakenFutures, NativeBroker<KrakenFutures>>> {
    let read = super::native::kraken_futures()?;
    let book = super::native::kraken_futures()?;
    let trader = KRAKEN_FUTURES.get_or_init(|| Arc::new(Trader::new())).clone();
    Some(NativeTradingBroker::new(read, "books", vec![TradeBook { name: "futures", broker: book, trader }]))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use std::collections::HashMap;

    use otw_connector_model::{Decimal, ExecLevel, OrderType, PositionModeCap, Side, TifKind};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::super::{broker_for, serve_from, OrderLeg, OrderTicket, LIVE_UNTESTED};

    const INSTRUMENTS: &str = include_str!("../../../connectors/providers/src/kraken_futures/fixtures/doc_instruments.json");
    const PLACED: &str = include_str!("../../../connectors/providers/src/kraken_futures/fixtures/trade/doc_sendorder_placed.json");

    fn creds(env: Option<&str>, untested: bool) -> HashMap<String, String> {
        let mut m: HashMap<String, String> = [
            ("api_key".to_string(), format!("mock-kf-{}", uuid::Uuid::new_v4().simple())),
            ("api_secret".to_string(), "aGVsbG8gd29ybGQgdGhpcyBpcyBub3QgYSBrZXkgYXQgYWxs".to_string()), // gitleaks:allow
        ]
        .into_iter()
        .collect();
        if let Some(e) = env {
            m.insert("env".into(), e.into());
        }
        if untested {
            m.insert(LIVE_UNTESTED.into(), "1".into());
        }
        m
    }

    fn limit(ticker: &str, key: &str) -> OrderTicket {
        let mut leg = OrderLeg::new(key, Side::Buy, OrderType::Limit, Decimal::new(1, 2));
        leg.limit = Some(Decimal::from(30_000));
        leg.tif = Some(TifKind::Gtc);
        OrderTicket { ticker: ticker.into(), asset_type: "crypto".into(), legs: vec![leg], provider: None, connector: None, reference: None, reference_at: None, confirm_duplicate: false }
    }

    fn ok(body: &str) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_string(body.to_string())
    }

    #[test]
    fn kraken_futures_takes_orders_at_e2d_on_one_book() {
        let b = broker_for("kraken_futures").unwrap();
        let caps = b.order_caps().unwrap();
        assert_eq!(caps.level, ExecLevel::E2D);
        assert!(caps.types.contains(&OrderType::StopLimit) && caps.types.contains(&OrderType::Lit) && caps.reduce_only && caps.post_only && caps.amend && caps.client_id, "{caps:?}");
        assert!(!caps.brackets && !caps.oca && caps.trail.is_empty() && !b.has_dry_run(), "{caps:?}");
        assert_eq!(caps.position_mode, Some(PositionModeCap::Net));
        assert_eq!(b.capability().broker, "kraken_futures");
        assert_eq!(super::super::live::level(b.as_ref()), ExecLevel::E2D);
    }

    /// Live: refused without the owner's acknowledgement; with it, the order goes out one-way
    /// (the mode read once at account level) and is followed as `futures:SYMBOL:order_id`. An
    /// inverse contract never leaves. A demo account is a sandbox.
    #[tokio::test]
    async fn a_kraken_futures_order_is_gated_live_and_followed_by_its_book() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/derivatives/api/v3/instruments")).respond_with(ok(INSTRUMENTS)).mount(&server).await;
        Mock::given(method("GET")).and(path("/derivatives/api/v3/openpositions")).respond_with(ok(r#"{"result":"success","openPositions":[]}"#)).mount(&server).await;
        Mock::given(method("POST")).and(path("/derivatives/api/v3/sendorder")).respond_with(ok(PLACED)).mount(&server).await;
        serve_from(&server.uri());
        let b = broker_for("kraken_futures").unwrap();
        let e = b.place(&creds(None, false), &limit("PF_XBTUSD", "kf0")).await.unwrap_err().to_string();
        assert!(e.contains("never tested against the venue"), "{e}");
        let s = creds(None, true);
        assert!(b.is_live(&s).await.unwrap());
        let placed = b.place(&s, &limit("PF_XBTUSD", "kf1")).await.unwrap();
        assert_eq!(placed[0].id, "futures:PF_XBTUSD:179f9af8-e45e-469d-b3e9-2fd4675cb7d0");
        let e = b.place(&s, &limit("PI_XBTUSD", "kf2")).await.unwrap_err().to_string();
        assert!(e.contains("PF_, FF_"), "{e}");
        let reqs = server.received_requests().await.unwrap();
        assert_eq!(reqs.iter().filter(|r| r.url.path() == "/derivatives/api/v3/sendorder").count(), 1);
        let body = reqs.iter().find(|r| r.url.path() == "/derivatives/api/v3/sendorder").map(|r| String::from_utf8_lossy(&r.body).to_string()).unwrap();
        assert_eq!(body, "orderType=lmt&symbol=PF_XBTUSD&side=buy&size=0.01&limitPrice=30000&cliOrdId=kf1");
        assert!(!b.is_live(&creds(Some("demo"), false)).await.unwrap(), "a demo account is a sandbox");
    }
}
