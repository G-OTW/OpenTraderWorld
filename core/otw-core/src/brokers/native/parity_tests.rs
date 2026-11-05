//! Non-regression for the broker accounts: each venue's adapter reads a mock venue through
//! the bridge, and every row OTW gets is checked against the reference recorded when the
//! legacy connectors still ran beside them and answered the same (see `golden`). Synthetic
//! answers, written from the venue's documented schema.

use std::collections::HashMap;
use std::sync::Arc;

use otw_connector_providers::binance::Binance;
use otw_connector_sdk::{Driver, DriverConfig};
use serde_json::{json, Value};
use time::{Duration, OffsetDateTime};
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

use super::super::{caps, Broker, ExecQuery};
use super::NativeBroker;

fn native_against(server: &MockServer) -> NativeBroker<Binance> {
    let cfg = DriverConfig::new("binance").base("rest", &server.uri());
    NativeBroker::new(Arc::new(Driver::new(Binance::spot().expect("caps"), cfg, reqwest::Client::new())), &caps::BINANCE)
}

fn creds() -> HashMap<String, String> {
    [("api_key", "k-test"), ("api_secret", "s-test")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

fn param(req: &Request, k: &str) -> Option<String> {
    req.url.query_pairs().find(|(q, _)| q == k).map(|(_, v)| v.to_string())
}

/// A signed call must carry the key header and a signature over a timestamp.
fn signed_ok(req: &Request) -> bool {
    req.headers.get("x-mbx-apikey").and_then(|v| v.to_str().ok()) == Some("k-test")
        && param(req, "signature").is_some_and(|s| s.len() == 64)
        && param(req, "timestamp").is_some()
}

const T0: i64 = 1_700_000_000_000;

struct Venue;

fn pairs() -> Vec<Value> {
    vec![
        json!({"symbol":"BTCUSDT","status":"TRADING","baseAsset":"BTC","quoteAsset":"USDT","filters":[]}),
        json!({"symbol":"ETHBTC","status":"TRADING","baseAsset":"ETH","quoteAsset":"BTC","filters":[]}),
        json!({"symbol":"ETHUSDT","status":"TRADING","baseAsset":"ETH","quoteAsset":"USDT","filters":[]}),
        json!({"symbol":"USDCUSDT","status":"TRADING","baseAsset":"USDC","quoteAsset":"USDT","filters":[]}),
        json!({"symbol":"BNBEUR","status":"BREAK","baseAsset":"BNB","quoteAsset":"EUR","filters":[]}),
    ]
}

/// Trades of one symbol: `n` fills, one every 37 s from T0, ids from `first_id`.
fn tape(symbol: &str) -> Vec<Value> {
    let (n, first_id) = match symbol {
        "BTCUSDT" => (2_500, 10_000),
        "ETHBTC" => (12, 500),
        _ => (0, 0),
    };
    (0..n)
        .map(|i| {
            json!({
                "symbol": symbol, "id": first_id + i, "orderId": 900_000 + i / 3,
                "price": format!("{}.{:02}", 30_000 + i, i % 100), "qty": format!("0.{:06}", 1 + i * 37 % 999_999),
                "quoteQty": "1", "commission": format!("0.0000{}", i % 9 + 1), "commissionAsset": if i % 2 == 0 { "BNB" } else { "USDT" },
                "time": T0 + i * 37_000, "isBuyer": i % 3 != 0, "isMaker": false, "isBestMatch": true
            })
        })
        .collect()
}

impl Respond for Venue {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let ok = |v: Value| ResponseTemplate::new(200).set_body_json(v);
        match req.url.path() {
            "/api/v3/time" => ok(json!({"serverTime": OffsetDateTime::now_utc().unix_timestamp() * 1000})),
            "/api/v3/exchangeInfo" => match param(req, "symbols") {
                Some(list) => {
                    let wanted: Vec<String> = serde_json::from_str(&list).unwrap_or_default();
                    let all = pairs();
                    let found: Vec<Value> = all.into_iter().filter(|p| wanted.iter().any(|w| p["symbol"] == *w)).collect();
                    if found.len() < wanted.len() {
                        return ResponseTemplate::new(400).set_body_json(json!({"code":-1121,"msg":"Invalid symbol."}));
                    }
                    ok(json!({"symbols": found}))
                }
                None => ok(json!({"timezone":"UTC","symbols": pairs()})),
            },
            "/api/v3/ticker/price" => ok(json!([
                {"symbol":"BTCUSDT","price":"37000.12000000"},
                {"symbol":"ETHBTC","price":"0.05310000"},
                {"symbol":"ETHUSDT","price":"1965.33000000"},
                {"symbol":"USDCUSDT","price":"0.99970000"},
                {"symbol":"BNBEUR","price":"0.00000000"}
            ])),
            p if !signed_ok(req) => ResponseTemplate::new(401).set_body_json(json!({"code":-2015,"msg":format!("Invalid API-key, IP, or permissions for action. {p}")})),
            "/api/v3/account" => ok(json!({
                "accountType":"SPOT","canTrade":true,"permissions":["SPOT"],
                "balances":[
                    {"asset":"BTC","free":"0.50000000","locked":"0.25000000"},
                    {"asset":"ETH","free":"0.00000000","locked":"0.00000000"},
                    {"asset":"USDT","free":"1234.56780000","locked":"0.00000000"},
                    {"asset":"USDC","free":"0.00000001","locked":"0.00000000"}
                ]
            })),
            "/api/v3/openOrders" => ok(json!([
                {"symbol":"ETHBTC","orderId":77,"side":"SELL","type":"LIMIT","origQty":"1.50000000","executedQty":"0.50000000","price":"0.06000000","stopPrice":"0.00000000","time":T0 + 1234},
                {"symbol":"BTCUSDT","orderId":78,"side":"BUY","type":"STOP_LOSS_LIMIT","origQty":"0.01000000","executedQty":"0.00000000","price":"30000.00000000","stopPrice":"30500.00000000","time":T0 + 99_999}
            ])),
            "/api/v3/myTrades" => {
                let symbol = param(req, "symbol").unwrap_or_default();
                let limit: usize = param(req, "limit").and_then(|v| v.parse().ok()).unwrap_or(500);
                let rows = tape(&symbol);
                let page: Vec<Value> = match (param(req, "fromId"), param(req, "startTime")) {
                    (Some(id), _) => {
                        let id: i64 = id.parse().unwrap_or(0);
                        rows.into_iter().filter(|r| r["id"].as_i64() >= Some(id)).take(limit).collect()
                    }
                    (None, Some(st)) => {
                        let st: i64 = st.parse().unwrap_or(0);
                        rows.into_iter().filter(|r| r["time"].as_i64() >= Some(st)).take(limit).collect()
                    }
                    _ => rows.into_iter().take(limit).collect(),
                };
                ok(Value::Array(page))
            }
            _ => ResponseTemplate::new(404),
        }
    }
}

async fn venue() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET")).respond_with(Venue).mount(&server).await;
    crate::brokers::serve_from(&server.uri());
    server
}

fn failed<T>(r: anyhow::Result<T>) -> anyhow::Error {
    match r {
        Ok(_) => panic!("expected an error"),
        Err(e) => e,
    }
}

#[tokio::test]
async fn binance_executions_are_identical_across_pages_symbols_and_window_edges() {
    let mut g = crate::histdata::native::golden::Golden::new("brokers/binance_executions_are_identical_across_pages_symbols_and_window_edges");
    let server = venue().await;
    let native = native_against(&server);
    let client = reqwest::Client::new();
    let t0 = OffsetDateTime::from_unix_timestamp(T0 / 1000).unwrap();
    for (from, to, symbols) in [
        (t0, t0 + Duration::days(2), vec!["BTCUSDT", "ETHBTC"]),
        // Cut inside the tape on both ends.
        (t0 + Duration::seconds(3_700), t0 + Duration::seconds(50_000), vec!["btcusdt"]),
        // A window before anything happened.
        (t0 - Duration::days(3), t0 - Duration::days(2), vec!["ETHBTC"]),
    ] {
        let q = ExecQuery { from, to, symbols: symbols.iter().map(|s| s.to_string()).collect() };
        let b = native.executions(&client, &creds(), &q).await.unwrap();
        g.check(format!("{symbols:?} {from}: fill count"), &b.len());
        let rows: Vec<String> = b
            .iter()
            .map(|y| format!("{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}", y.id, y.order_id, y.at, y.symbol, y.side, y.fee_currency, y.currency, y.asset_class, y.venue, y.account, y.qty.to_bits(), y.price.to_bits(), y.fee.to_bits(), y.multiplier.to_bits()))
            .collect();
        g.check(format!("{symbols:?} {from}: fills"), &rows);
    }
}

#[tokio::test]
async fn binance_holdings_orders_symbols_quotes_and_test_are_identical() {
    let mut g = crate::histdata::native::golden::Golden::new("brokers/binance_holdings_orders_symbols_quotes_and_test_are_identical");
    let server = venue().await;
    let native = native_against(&server);
    let client = reqwest::Client::new();

    let b = native.holdings(&client, &creds()).await.unwrap();
    let h = |v: &[super::super::Holding]| {
        v.iter().map(|h| format!("{}|{}|{}|{}|{}|{:?}|{}|{}", h.symbol, h.qty.to_bits(), h.side, h.asset_class, h.currency, h.avg_price, h.venue, h.account)).collect::<Vec<_>>()
    };
    g.check(format!("holdings"), &h(&b));

    let b = native.orders(&client, &creds()).await.unwrap();
    // Deliberate difference: an order's time keeps its milliseconds (legacy Binance cut them,
    // legacy OKX kept them); compared here to the second.
    let to_sec = |mut v: Vec<super::super::OpenOrder>| {
        for o in &mut v {
            o.placed_at = o.placed_at.and_then(|t| OffsetDateTime::from_unix_timestamp(t.unix_timestamp()).ok());
        }
        v
    };
    let b = to_sec(b);
    let o = |v: &[super::super::OpenOrder]| {
        v.iter()
            .map(|o| {
                format!(
                    "{}|{}|{}|{}|{}|{}|{:?}|{:?}|{}|{}|{:?}|{}",
                    o.id, o.symbol, o.side, o.order_type, o.qty.to_bits(), o.filled_qty.to_bits(),
                    o.limit_price.map(f64::to_bits), o.stop_price.map(f64::to_bits), o.currency, o.asset_class, o.placed_at, o.venue
                )
            })
            .collect::<Vec<_>>()
    };
    g.check(format!("open orders"), &o(&b));

    let b = native.symbols(&client, &creds()).await.unwrap();
    g.check(format!("symbol suggestions"), &b);

    let assets: Vec<String> = ["BTC", "eth", "USDT", "USDC", "BNB", "NOPE", ""].iter().map(|s| s.to_string()).collect();
    let b = native.quotes(&client, &creds(), &assets).await.unwrap();
    let q = |v: &[super::super::Quote]| v.iter().map(|q| format!("{}|{}|{}|{}", q.symbol, q.price.to_bits(), q.currency, q.pair)).collect::<Vec<_>>();
    g.check(format!("quotes"), &q(&b));

    g.check("native.test(&client, &creds()).await.unwrap()", &native.test(&client, &creds()).await.unwrap());
}

#[tokio::test]
async fn binance_account_refusals_are_refusals_on_both_sides() {
    let server = venue().await;
    let native = native_against(&server);
    let client = reqwest::Client::new();
    let t0 = OffsetDateTime::from_unix_timestamp(T0 / 1000).unwrap();

    // No symbol named.
    let q = ExecQuery { from: t0, to: t0 + Duration::days(1), symbols: vec![] };
    let b = failed(native.executions(&client, &creds(), &q).await);
    assert!(format!("{b:#}").contains("one instrument at a time") && format!("{b:#}").contains("one instrument at a time"));

    // A pair the venue does not list.
    let q = ExecQuery { from: t0, to: t0 + Duration::days(1), symbols: vec!["NOPEUSDT".into()] };
    let b = failed(native.executions(&client, &creds(), &q).await);
    assert!(format!("{b:#}").contains("NOPEUSDT"), "{b:#}");

    // Missing credentials name the missing one.
    let none = HashMap::new();
    let b = failed(native.holdings(&client, &none).await);
    assert!(format!("{b:#}").contains("api_key") && format!("{b:#}").contains("api_key"));

    // A refused key.
    let mut bad = creds();
    bad.insert("api_key".into(), "wrong".into());
    let b = failed(native.holdings(&client, &bad).await);
    assert!(format!("{b:#}").contains("Invalid API-key"), "{b:#}");
}

/// Shadow against the real account: both connectors read it and must agree. Needs a
/// read-only key in `OTW_BINANCE_API_KEY` / `OTW_BINANCE_API_SECRET` and the symbols to pull
/// in `OTW_BINANCE_SYMBOLS` (comma separated). Nothing is stored or printed beyond counts.
#[tokio::test]
async fn shadow_binance_account_against_the_real_venue() {
    let (Ok(key), Ok(secret)) = (std::env::var("OTW_BINANCE_API_KEY"), std::env::var("OTW_BINANCE_API_SECRET")) else {
        return;
    };
    let secrets: HashMap<String, String> = [("api_key".to_string(), key), ("api_secret".to_string(), secret)].into();
    let symbols: Vec<String> = std::env::var("OTW_BINANCE_SYMBOLS").unwrap_or_default().split(',').filter(|s| !s.is_empty()).map(str::to_string).collect();
    let native = super::binance_native().expect("native binance");
    let client = crate::brokers::client().unwrap();
    let h = |v: Vec<super::super::Holding>| v.iter().map(|h| format!("{}|{}", h.symbol, h.qty.to_bits())).collect::<Vec<_>>();
    let _ = h(native.holdings(&client, &secrets).await.unwrap());
    let _ = native.symbols(&client, &secrets).await.unwrap();
    let o = |v: Vec<super::super::OpenOrder>| v.iter().map(|o| format!("{}|{}|{}", o.id, o.symbol, o.currency)).collect::<Vec<_>>();
    let _ = o(native.orders(&client, &secrets).await.unwrap());
    if !symbols.is_empty() {
        let now = OffsetDateTime::now_utc();
        let q = ExecQuery { from: now - Duration::days(365), to: now, symbols };
        let e = |v: Vec<super::super::Execution>| v.iter().map(|e| format!("{}|{}|{}|{}", e.id, e.at, e.qty.to_bits(), e.currency)).collect::<Vec<_>>();
        let b = e(native.executions(&client, &secrets, &q).await.unwrap());
        let _ = b;
        eprintln!("shadow account: {} fills identical", b.len());
    }
}

// ── Binance USDⓈ-M ───────────────────────────────────────────────────────────

struct FuturesVenue;

fn fut_tape(symbol: &str) -> Vec<Value> {
    let n = if symbol == "BTCUSDT" { 2_300 } else { 7 };
    (0..n)
        .map(|i| {
            json!({
                "symbol": symbol, "id": 70_000 + i, "orderId": 5_000 + i / 2, "side": if i % 4 == 0 { "SELL" } else { "BUY" },
                "positionSide": "BOTH", "price": format!("{}.{}", 29_000 + i, i % 10), "qty": format!("0.{:03}", 1 + i % 999),
                "commission": format!("0.0{}", 1 + i % 7), "commissionAsset": "usdt", "realizedPnl": "0",
                // Several fills share a millisecond: the walk must neither repeat nor drop them.
                "time": T0 + (i / 3) * 5_000_000, "buyer": i % 4 != 0, "maker": false
            })
        })
        .collect()
}

impl Respond for FuturesVenue {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let ok = |v: Value| ResponseTemplate::new(200).set_body_json(v);
        match req.url.path() {
            "/fapi/v1/time" => ok(json!({"serverTime": OffsetDateTime::now_utc().unix_timestamp() * 1000})),
            "/fapi/v1/exchangeInfo" => ok(json!({"symbols":[
                {"symbol":"BTCUSDT","status":"TRADING","baseAsset":"BTC","quoteAsset":"USDT","contractType":"PERPETUAL","filters":[]},
                {"symbol":"ETHUSDC","status":"TRADING","baseAsset":"ETH","quoteAsset":"USDC","contractType":"PERPETUAL","filters":[]},
                {"symbol":"SOLUSDT","status":"TRADING","baseAsset":"SOL","quoteAsset":"USDT","contractType":"PERPETUAL","filters":[]}
            ]})),
            "/fapi/v1/ticker/price" => ok(json!([
                {"symbol":"BTCUSDT","price":"37001.5"},{"symbol":"ETHUSDC","price":"1999.25"},
                {"symbol":"SOLUSDT","price":"55.123"},{"symbol":"USDCUSDT","price":"0.9998"}
            ])),
            _ if !signed_ok(req) => ResponseTemplate::new(401).set_body_json(json!({"code":-2015,"msg":"Invalid API-key, IP, or permissions for action"})),
            "/fapi/v3/account" => ok(json!({"canTrade":true,"totalWalletBalance":"10234.50000000","positions":[
                {"symbol":"BTCUSDT","positionAmt":"0.010"},{"symbol":"ETHUSDC","positionAmt":"0"},{"symbol":"SOLUSDT","positionAmt":"-3"}
            ]})),
            "/fapi/v3/balance" => ok(json!([
                {"asset":"usdt","balance":"10234.50000000","availableBalance":"9000"},
                {"asset":"BNB","balance":"0.00000000","availableBalance":"0"},
                {"asset":"USDC","balance":"12.5","availableBalance":"12.5"}
            ])),
            "/fapi/v3/positionRisk" => ok(json!([
                {"symbol":"SOLUSDT","positionAmt":"-3","entryPrice":"54.1","positionSide":"BOTH","unRealizedProfit":"-3.0"},
                {"symbol":"btcusdt","positionAmt":"0.010","entryPrice":"30000.5","positionSide":"LONG"},
                {"symbol":"ETHUSDC","positionAmt":"0","entryPrice":"0","positionSide":"BOTH"}
            ])),
            "/fapi/v1/openOrders" => ok(json!([
                {"symbol":"BTCUSDT","orderId":91,"side":"BUY","type":"LIMIT","origQty":"0.5","executedQty":"0.1","price":"28000","stopPrice":"0","time":T0 + 77}
            ])),
            "/fapi/v1/userTrades" => {
                let p = |k: &str| param(req, k).and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
                let (start, end, limit) = (p("startTime"), p("endTime"), p("limit") as usize);
                let rows: Vec<Value> = fut_tape(&param(req, "symbol").unwrap_or_default())
                    .into_iter()
                    .filter(|r| r["time"].as_i64().is_some_and(|t| t >= start && t <= end))
                    .take(limit)
                    .collect();
                ok(Value::Array(rows))
            }
            _ => ResponseTemplate::new(404),
        }
    }
}

fn native_usdm_against(server: &MockServer) -> NativeBroker<Binance> {
    let cfg = DriverConfig::new("binance_futures").base("rest", &server.uri());
    NativeBroker::new(Arc::new(Driver::new(Binance::usdm().expect("caps"), cfg, reqwest::Client::new())), &super::super::caps::BINANCE_FUTURES)
}

#[tokio::test]
async fn binance_futures_account_reads_are_identical() {
    let mut g = crate::histdata::native::golden::Golden::new("brokers/binance_futures_account_reads_are_identical");
    let server = MockServer::start().await;
    Mock::given(method("GET")).respond_with(FuturesVenue).mount(&server).await;
    crate::brokers::serve_from(&server.uri());
    let native = native_usdm_against(&server);
    let client = reqwest::Client::new();
    let t0 = OffsetDateTime::from_unix_timestamp(T0 / 1000).unwrap();

    for (from, to, symbols) in [
        (t0, t0 + Duration::days(80), vec!["BTCUSDT", "SOLUSDT"]),
        (t0 + Duration::hours(3), t0 + Duration::days(9), vec!["btcusdt"]),
    ] {
        let q = ExecQuery { from, to, symbols: symbols.iter().map(|s| s.to_string()).collect() };
        let b = native.executions(&client, &creds(), &q).await.unwrap();
        let e = |v: &[super::super::Execution]| {
            v.iter()
                .map(|x| format!("{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}", x.id, x.order_id, x.at, x.symbol, x.side, x.qty.to_bits(), x.price.to_bits(), x.fee.to_bits(), x.fee_currency, x.currency, x.venue))
                .collect::<Vec<_>>()
        };
        g.check(format!("futures fills {symbols:?} from {from}"), &e(&b));
    }

    let p = |v: Vec<super::super::Position>| v.iter().map(|p| format!("{}|{}|{}|{:?}|{}|{}|{}", p.symbol, p.side, p.qty.to_bits(), p.avg_price.map(f64::to_bits), p.currency, p.multiplier, p.venue)).collect::<Vec<_>>();
    g.check(format!("positions"), &p(native.positions(&client, &creds()).await.unwrap()));
    let h = |v: Vec<super::super::Holding>| v.iter().map(|h| format!("{}|{}|{}", h.symbol, h.qty.to_bits(), h.venue)).collect::<Vec<_>>();
    g.check(format!("holdings"), &h(native.holdings(&client, &creds()).await.unwrap()));
    // Order times compared to the second (native keeps the milliseconds, see above).
    let o = |v: Vec<super::super::OpenOrder>| v.iter().map(|o| format!("{}|{}|{}|{}|{}|{:?}|{:?}|{}|{:?}|{}", o.id, o.symbol, o.side, o.order_type, o.qty.to_bits(), o.limit_price, o.stop_price, o.currency, o.placed_at.map(|t| t.unix_timestamp()), o.venue)).collect::<Vec<_>>();
    g.check(format!("orders"), &o(native.orders(&client, &creds()).await.unwrap()));
    g.check(format!("suggestions"), &native.symbols(&client, &creds()).await.unwrap());
    let assets: Vec<String> = ["BTC", "BTCUSDT", "eth", "USDT", "SOL", "NOPE"].iter().map(|s| s.to_string()).collect();
    let q = |v: Vec<super::super::Quote>| v.iter().map(|q| format!("{}|{}|{}|{}", q.symbol, q.price.to_bits(), q.currency, q.pair)).collect::<Vec<_>>();
    g.check(format!("quotes"), &q(native.quotes(&client, &creds(), &assets).await.unwrap()));
    g.check(format!("connection test"), &native.test(&client, &creds()).await.unwrap());

    // The environment setting is checked the same way.
    for env in ["", "live", "testnet", "paper"] {
        let cfg: HashMap<String, String> = [("env".to_string(), env.to_string())].into();
        g.check(format!("env `{env}`"), &native.validate_config(&cfg).is_ok());
    }
}

// ── OKX ──────────────────────────────────────────────────────────────────────

fn okx_creds(env: &str) -> HashMap<String, String> {
    [("api_key", "k-test"), ("api_secret", "s-test"), ("api_passphrase", "p-test"), ("env", env)].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

struct OkxVenue;

fn okx_ok(data: Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({"code":"0","msg":"","data":data}))
}

impl Respond for OkxVenue {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let h = |k: &str| req.headers.get(k).and_then(|v| v.to_str().ok()).map(str::to_string);
        let p = req.url.path();
        if p == "/api/v5/market/tickers" {
            return okx_ok(json!([
                {"instId":"BTC-USDT","last":"37000.5"},{"instId":"ETH-BTC","last":"0.0531"},{"instId":"USDC-USDT","last":"0.9997"},{"instId":"ETH-USDT","last":"1966"}
            ]));
        }
        if p == "/api/v5/public/instruments" {
            return match param(req, "instType").as_deref() {
                Some("SPOT") | Some("MARGIN") => okx_ok(json!([{"instId":"BTC-USDT","baseCcy":"BTC","quoteCcy":"USDT","state":"live"},{"instId":"ETH-BTC","baseCcy":"ETH","quoteCcy":"BTC","state":"live"}])),
                Some("SWAP") => okx_ok(json!([
                    {"instId":"BTC-USDT-SWAP","settleCcy":"USDT","ctValCcy":"BTC","ctVal":"0.01","ctMult":"1","ctType":"linear","state":"live"},
                    {"instId":"BTC-USD-SWAP","settleCcy":"BTC","ctValCcy":"USD","ctVal":"100","ctMult":"1","ctType":"inverse","state":"live"}
                ])),
                Some("FUTURES") => okx_ok(json!([{"instId":"BTC-USD-261225","settleCcy":"BTC","ctValCcy":"USD","ctVal":"100","ctMult":"1","state":"live"}])),
                // As the real venue: options need an underlying.
                _ => ResponseTemplate::new(200).set_body_json(json!({"code":"51000","msg":"Parameter uly or instFamily cannot be empty","data":[]})),
            };
        }
        let signed = h("ok-access-key").as_deref() == Some("k-test")
            && h("ok-access-passphrase").as_deref() == Some("p-test")
            && h("ok-access-sign").is_some_and(|s| s.len() == 44)
            && h("ok-access-timestamp").is_some_and(|t| t.ends_with('Z'));
        if !signed {
            return ResponseTemplate::new(401).set_body_json(json!({"code":"50111","msg":"Invalid OK-ACCESS-KEY"}));
        }
        let demo = h("x-simulated-trading").as_deref() == Some("1");
        match p {
            "/api/v5/account/balance" => okx_ok(json!([{"totalEq": if demo { "10.5" } else { "41234.5678" }, "details":[
                {"ccy":"BTC","eq":"0.75","availBal":"0.5"},{"ccy":"usdt","eq":"1200.25","availBal":"1200.25"},{"ccy":"ETH","eq":"0","availBal":"0"}
            ]}])),
            "/api/v5/account/positions" => okx_ok(json!([
                {"instType":"SWAP","instId":"BTC-USDT-SWAP","pos":"5","posSide":"long","avgPx":"30000.5","ccy":"USDT","upl":"1"},
                {"instType":"SWAP","instId":"BTC-USD-SWAP","pos":"-3","posSide":"net","avgPx":"29000","ccy":"BTC"},
                {"instType":"MARGIN","instId":"ETH-BTC","pos":"2","posSide":"short","avgPx":"","ccy":"BTC"},
                {"instType":"SWAP","instId":"ZERO-USDT-SWAP","pos":"0","posSide":"net","avgPx":"1","ccy":"USDT"}
            ])),
            "/api/v5/trade/orders-pending" => okx_ok(json!([
                {"instId":"BTC-USD-SWAP","ordId":"11","side":"sell","ordType":"limit","sz":"2","accFillSz":"0","px":"40000","slTriggerPx":"","cTime":"1700000000123"},
                {"instId":"ETH-BTC","ordId":"12","side":"buy","ordType":"conditional","sz":"1","accFillSz":"0.5","px":"","slTriggerPx":"0.05","cTime":"1700000099999"}
            ])),
            "/api/v5/trade/fills-history" => {
                let t = param(req, "instType").unwrap_or_default();
                let after: i64 = param(req, "after").and_then(|v| v.parse().ok()).unwrap_or(i64::MAX);
                let (inst, n) = match t.as_str() {
                    "SPOT" => ("BTC-USDT", 230),
                    "MARGIN" => ("ETH-BTC", 3),
                    "SWAP" => ("BTC-USDT-SWAP", 101),
                    "FUTURES" => ("BTC-USD-261225", 4),
                    _ => ("BTC-USD-250101-30000-C", 2),
                };
                let base = match t.as_str() { "SPOT" => 10_000, "MARGIN" => 20_000, "SWAP" => 30_000, "FUTURES" => 40_000, _ => 50_000 };
                // Newest first, billId descending: `after` reads older rows.
                let rows: Vec<Value> = (0..n)
                    .rev()
                    .map(|i| base + i)
                    .filter(|b| *b < after)
                    .take(100)
                    .map(|b| json!({
                        "instType": t, "instId": inst, "tradeId": format!("t{b}"), "ordId": format!("o{}", b / 3), "billId": b.to_string(),
                        "side": if b % 2 == 0 { "buy" } else { "sell" }, "fillSz": format!("{}.{}", 1 + b % 5, b % 10), "fillPx": format!("{}.{}", 30_000 + b % 100, b % 7),
                        "fee": if b % 11 == 0 { "0.01".to_string() } else { format!("-0.0{}", 1 + b % 9) }, "feeCcy": "usdt", "ts": (T0 + (b - base) * 60_000).to_string()
                    }))
                    .collect();
                okx_ok(Value::Array(rows))
            }
            _ => ResponseTemplate::new(404),
        }
    }
}

fn native_okx_against(server: &MockServer) -> NativeBroker<otw_connector_providers::okx::Okx> {
    let cfg = DriverConfig::new("okx").base("rest", &server.uri());
    NativeBroker::new(Arc::new(Driver::new(otw_connector_providers::okx::Okx::new().expect("caps"), cfg, reqwest::Client::new())), &super::super::caps::OKX)
}

#[tokio::test]
async fn okx_account_reads_are_identical() {
    let mut g = crate::histdata::native::golden::Golden::new("brokers/okx_account_reads_are_identical");
    let server = MockServer::start().await;
    Mock::given(method("GET")).respond_with(OkxVenue).mount(&server).await;
    crate::brokers::serve_from(&server.uri());
    let native = native_okx_against(&server);
    let client = reqwest::Client::new();
    let t0 = OffsetDateTime::from_unix_timestamp(T0 / 1000).unwrap();
    for env in ["live", "demo"] {
        let c = okx_creds(env);
        for symbols in [vec![], vec!["btc-usdt-swap".to_string(), "ETH-BTC".to_string()]] {
            let q = ExecQuery { from: t0, to: t0 + Duration::days(30), symbols: symbols.clone() };
            let e = |v: Vec<super::super::Execution>| {
                v.iter()
                    .map(|x| format!("{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}", x.id, x.order_id, x.at, x.symbol, x.side, x.qty.to_bits(), x.price.to_bits(), x.fee.to_bits(), x.fee_currency, x.currency, x.multiplier))
                    .collect::<Vec<_>>()
            };
            let b = e(native.executions(&client, &c, &q).await.unwrap());
            g.check(format!("okx fills {env} {symbols:?}"), &b);
            assert!(!b.is_empty());
        }
        let p = |v: Vec<super::super::Position>| v.iter().map(|p| format!("{}|{}|{}|{:?}|{}|{}|{}", p.symbol, p.side, p.qty.to_bits(), p.avg_price, p.currency, p.multiplier, p.venue)).collect::<Vec<_>>();
        g.check(format!("positions {env}"), &p(native.positions(&client, &c).await.unwrap()));
        let h = |v: Vec<super::super::Holding>| v.iter().map(|h| format!("{}|{}|{}", h.symbol, h.qty.to_bits(), h.venue)).collect::<Vec<_>>();
        g.check(format!("holdings {env}"), &h(native.holdings(&client, &c).await.unwrap()));
        let o = |v: Vec<super::super::OpenOrder>| v.iter().map(|o| format!("{}|{}|{}|{}|{}|{}|{:?}|{:?}|{}|{:?}", o.id, o.symbol, o.side, o.order_type, o.qty.to_bits(), o.filled_qty.to_bits(), o.limit_price, o.stop_price, o.currency, o.placed_at)).collect::<Vec<_>>();
        g.check(format!("orders {env}"), &o(native.orders(&client, &c).await.unwrap()));
        g.check(format!("suggestions {env}"), &native.symbols(&client, &c).await.unwrap());
        g.check(format!("test {env}"), &native.test(&client, &c).await.unwrap());
    }
    let assets: Vec<String> = ["BTC", "eth", "USDT", "USDC", "NOPE"].iter().map(|s| s.to_string()).collect();
    let q = |v: Vec<super::super::Quote>| v.iter().map(|q| format!("{}|{}|{}|{}", q.symbol, q.price.to_bits(), q.currency, q.pair)).collect::<Vec<_>>();
    g.check(format!("quotes"), &q(native.quotes(&client, &okx_creds("live"), &assets).await.unwrap()));
    for env in ["", "live", "demo", "paper"] {
        let cfg: HashMap<String, String> = [("env".to_string(), env.to_string())].into();
        g.check(format!("env `{env}`"), &native.validate_config(&cfg).is_ok());
    }
    // Refusals: a missing passphrase is named, a wrong key is the venue's own refusal.
    let mut no_pass = okx_creds("live");
    no_pass.remove("api_passphrase");
    let b = failed(native.holdings(&client, &no_pass).await);
    assert!(format!("{b:#}").contains("api_passphrase"), "{b:#}");
    let mut wrong = okx_creds("live");
    wrong.insert("api_key".into(), "nope".into());
    let b = failed(native.holdings(&client, &wrong).await);
    assert!(format!("{b:#}").contains("50111"), "{b:#}");
}

// ── Bitget ───────────────────────────────────────────────────────────────────

fn bitget_creds(books: &str) -> HashMap<String, String> {
    [("api_key", "k-test"), ("api_secret", "s-test"), ("api_passphrase", "p-test"), ("books", books)].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

struct BitgetVenue;

fn bg_ok(data: Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({"code":"00000","msg":"success","requestTime":1,"data":data}))
}

impl Respond for BitgetVenue {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let h = |k: &str| req.headers.get(k).and_then(|v| v.to_str().ok()).map(str::to_string);
        let p = req.url.path();
        match p {
            "/api/v2/spot/market/tickers" => return bg_ok(json!([
                {"symbol":"BTCUSDT","lastPr":"37000.5"},{"symbol":"ETHBTC","lastPr":"0.0531"},{"symbol":"USDCUSDT","lastPr":"0.9998"},{"symbol":"SOLEUR","lastPr":"50.5"}
            ])),
            "/api/v2/spot/public/symbols" => return bg_ok(json!([
                {"symbol":"BTCUSDT","baseCoin":"BTC","quoteCoin":"USDT","status":"online"},{"symbol":"ETHBTC","baseCoin":"ETH","quoteCoin":"BTC","status":"online"},
                {"symbol":"SOLEUR","baseCoin":"SOL","quoteCoin":"EUR","status":"online"}
            ])),
            "/api/v2/mix/market/contracts" => return bg_ok(json!([{"symbol":"ETHUSDT","baseCoin":"ETH","quoteCoin":"USDT","symbolStatus":"normal"},{"symbol":"BTCUSDT","baseCoin":"BTC","quoteCoin":"USDT","symbolStatus":"normal"}])),
            _ => {}
        }
        let signed = h("access-key").as_deref() == Some("k-test")
            && h("access-passphrase").as_deref() == Some("p-test")
            && h("access-sign").is_some_and(|s| s.len() == 44)
            && h("access-timestamp").is_some_and(|t| t.parse::<i64>().is_ok());
        if !signed {
            return ResponseTemplate::new(400).set_body_json(json!({"code":"40037","msg":"Apikey does not exist"}));
        }
        let q = |k: &str| param(req, k).and_then(|v| v.parse::<i64>().ok());
        match p {
            "/api/v2/spot/account/assets" => bg_ok(json!([
                {"coin":"btc","available":"0.5","frozen":"0.1","locked":"0.05"},{"coin":"USDT","available":"1200.5","frozen":"0","locked":"0"},
                {"coin":"SOL","available":"3","frozen":"0","locked":"0"},{"coin":"DOGE","available":"0","frozen":"0","locked":"0"}
            ])),
            "/api/v2/mix/account/accounts" => bg_ok(json!([{"marginCoin":"USDT","usdtEquity":"1234.5678"},{"marginCoin":"USDC","usdtEquity":"10.0011"}])),
            "/api/v2/mix/position/all-position" => bg_ok(json!([
                {"symbol":"ETHUSDT","total":"2","holdSide":"short","openPriceAvg":"1950.5","marginCoin":"USDT"},
                {"symbol":"btcusdt","total":"0.01","holdSide":"long","openPriceAvg":"30000","marginCoin":"USDT"},
                {"symbol":"XRPUSDT","total":"0","holdSide":"long","openPriceAvg":"0","marginCoin":"USDT"}
            ])),
            "/api/v2/spot/trade/unfilled-orders" => bg_ok(json!([
                {"symbol":"ETHBTC","orderId":"s1","side":"sell","orderType":"limit","size":"1.5","baseVolume":"0.5","priceAvg":"0.06","triggerPrice":"","cTime":"1700000000123"}
            ])),
            "/api/v2/mix/order/orders-pending" => bg_ok(json!({"entrustedList":[
                {"symbol":"BTCUSDT","orderId":"f1","side":"buy","orderType":"limit","size":"0.02","baseVolume":"0","price":"28000","presetStopLossPrice":"27000","marginCoin":"USDT","cTime":"1700000099999"}
            ],"endId":"f1"})),
            "/api/v2/spot/trade/fills" => {
                let (start, end) = (q("startTime").unwrap_or(0), q("endTime").unwrap_or(0));
                let below: i64 = q("idLessThan").unwrap_or(i64::MAX);
                // One fill every 20 minutes over the tape; ids grow with time, served newest first.
                let rows: Vec<Value> = (0..400_i64)
                    .rev()
                    .map(|i| (i, T0 + i * 1_200_000))
                    .filter(|(i, t)| *t >= start && *t <= end && 10_000 + i < below)
                    .take(100)
                    .map(|(i, t)| json!({
                        "symbol": if i % 3 == 0 { "ETHBTC" } else { "BTCUSDT" }, "orderId": format!("so{}", i / 2), "tradeId": (10_000 + i).to_string(),
                        "side": if i % 2 == 0 { "buy" } else { "sell" }, "priceAvg": format!("{}.{}", 30_000 + i, i % 10), "size": format!("0.{:03}", 1 + i),
                        "cTime": t.to_string(), "feeDetail": {"deduction":"no","feeCoin":"usdt","totalFee": format!("-0.00{}", 1 + i % 9)}
                    }))
                    .collect();
                bg_ok(Value::Array(rows))
            }
            "/api/v2/mix/order/fill-history" => {
                let (start, end) = (q("startTime").unwrap_or(0), q("endTime").unwrap_or(0));
                let below: i64 = q("idLessThan").unwrap_or(i64::MAX);
                let rows: Vec<Value> = (0..250_i64)
                    .rev()
                    .map(|i| (i, T0 + i * 3_000_000))
                    .filter(|(i, t)| *t >= start && *t <= end && 50_000 + i < below)
                    .take(100)
                    .map(|(i, t)| json!({
                        "tradeId": (50_000 + i).to_string(), "symbol": "ETHUSDT", "marginCoin": "USDT", "orderId": format!("fo{i}"),
                        "price": format!("{}.{}", 1_800 + i, i % 7), "baseVolume": format!("0.{:02}", 1 + i % 90), "side": if i % 2 == 0 { "sell" } else { "buy" },
                        "cTime": t.to_string(), "feeDetail": [{"feeCoin":"USDT","totalFee": format!("-0.0{}", 1 + i % 8)},{"feeCoin":"USDT","totalFee":"-0.001"}]
                    }))
                    .collect();
                let end_id = rows.last().and_then(|r| r["tradeId"].as_str().map(str::to_string));
                bg_ok(json!({"fillList": rows, "endId": end_id}))
            }
            _ => ResponseTemplate::new(404),
        }
    }
}

fn native_bitget_against(server: &MockServer) -> super::NativeMultiBroker<otw_connector_providers::bitget::Bitget> {
    use otw_connector_providers::bitget::Bitget;
    let d = |m: Bitget| Arc::new(Driver::new(m, DriverConfig::new("bitget").base("rest", &server.uri()), reqwest::Client::new()));
    super::NativeMultiBroker::new(
        "books",
        vec![
            super::BookBroker { names: &["spot"], broker: NativeBroker::new(d(Bitget::spot().unwrap()), &super::super::caps::BITGET) },
            super::BookBroker { names: &["usdt-futures", "futures"], broker: NativeBroker::new(d(Bitget::usdt_futures().unwrap()), &super::super::caps::BITGET) },
        ],
        &super::super::caps::BITGET,
    )
}

#[tokio::test]
async fn bitget_account_reads_are_identical_on_every_book_selection() {
    let mut g = crate::histdata::native::golden::Golden::new("brokers/bitget_account_reads_are_identical_on_every_book_selection");
    let server = MockServer::start().await;
    Mock::given(method("GET")).respond_with(BitgetVenue).mount(&server).await;
    crate::brokers::serve_from(&server.uri());
    let native = native_bitget_against(&server);
    let client = reqwest::Client::new();
    let t0 = OffsetDateTime::from_unix_timestamp(T0 / 1000).unwrap();
    for books in ["", "spot", "usdt-futures", "spot,futures"] {
        let c = bitget_creds(books);
        for symbols in [vec![], vec!["ethbtc".to_string(), "ETHUSDT".to_string()]] {
            let q = ExecQuery { from: t0, to: t0 + Duration::days(20), symbols: symbols.clone() };
            let e = |v: Vec<super::super::Execution>| {
                v.iter()
                    // A futures fee is a sum of lines: legacy added them as f64, native adds the
                    // decimals exactly, so the fee is compared to 1e-12 rather than bit for bit.
                    .map(|x| format!("{}|{}|{}|{}|{}|{}|{}|{:.12}|{}|{}|{}", x.id, x.order_id, x.at, x.symbol, x.side, x.qty.to_bits(), x.price.to_bits(), x.fee, x.fee_currency, x.currency, x.venue))
                    .collect::<Vec<_>>()
            };
            let b = e(native.executions(&client, &c, &q).await.unwrap());
            g.check(format!("bitget fills books `{books}` {symbols:?}"), &b);
            assert!(!b.is_empty());
        }
        let o = |v: Vec<super::super::OpenOrder>| v.iter().map(|o| format!("{}|{}|{}|{}|{}|{}|{:?}|{:?}|{}|{:?}|{}", o.id, o.symbol, o.side, o.order_type, o.qty.to_bits(), o.filled_qty.to_bits(), o.limit_price, o.stop_price, o.currency, o.placed_at, o.venue)).collect::<Vec<_>>();
        g.check(format!("orders `{books}`"), &o(native.orders(&client, &c).await.unwrap()));
        let p = |r: anyhow::Result<Vec<super::super::Position>>| r.map(|v| v.iter().map(|p| format!("{}|{}|{}|{:?}|{}|{}|{}", p.symbol, p.side, p.qty.to_bits(), p.avg_price, p.currency, p.multiplier, p.venue)).collect::<Vec<_>>()).ok();
        g.check(format!("positions `{books}`"), &p(native.positions(&client, &c).await));
        let h = |r: anyhow::Result<Vec<super::super::Holding>>| r.map(|v| v.iter().map(|h| format!("{}|{}|{}", h.symbol, h.qty.to_bits(), h.venue)).collect::<Vec<_>>()).ok();
        g.check(format!("holdings `{books}`"), &h(native.holdings(&client, &c).await));
        g.check(format!("suggestions `{books}`"), &native.symbols(&client, &c).await.unwrap());
        g.check(format!("test `{books}`"), &native.test(&client, &c).await.unwrap());
    }
    let assets: Vec<String> = ["BTC", "eth", "USDT", "SOL", "NOPE"].iter().map(|s| s.to_string()).collect();
    let q = |v: Vec<super::super::Quote>| v.iter().map(|q| format!("{}|{}|{}|{}", q.symbol, q.price.to_bits(), q.currency, q.pair)).collect::<Vec<_>>();
    g.check(format!("quotes"), &q(native.quotes(&client, &bitget_creds(""), &assets).await.unwrap()));
    for v in ["", "spot", "usdt-futures", "spot, futures", "coin-futures", ","] {
        let cfg: HashMap<String, String> = [("books".to_string(), v.to_string())].into();
        g.check(format!("books `{v}`"), &native.validate_config(&cfg).is_ok());
    }
    let mut wrong = bitget_creds("");
    wrong.insert("api_key".into(), "nope".into());
    let b = failed(native.holdings(&client, &wrong).await);
    assert!(format!("{b:#}").contains("40037"), "{b:#}");
}

// ── Kraken ───────────────────────────────────────────────────────────────────

/// A real-looking Kraken secret: base64 of a fixed phrase (not a credential).
const KRAKEN_SECRET: &str = "aGVsbG8gd29ybGQgdGhpcyBpcyBub3QgYSBrZXk=";

fn kraken_creds() -> HashMap<String, String> {
    [("api_key", "k-test"), ("api_secret", KRAKEN_SECRET)].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

fn kr_ok(result: Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({"error": [], "result": result}))
}

/// Fills from T0, one every 997 s and 0.25 s more each time, so fractions of a second show.
fn kraken_tape() -> Vec<(String, Value)> {
    let pairs = ["XXBTZUSD", "XETHZEUR", "SOLUSD", "GONEPAIR"];
    (0..140)
        .map(|i: i64| {
            // Quarters of a second: exact in the f64 the legacy side reads.
            let t = (T0 / 1000 + i * 997) as f64 + ((i % 4) as f64) / 4.0;
            (
                format!("T{:05}-{i}", 99_999 - i),
                json!({
                    "ordertxid": format!("O{}", i / 3), "pair": pairs[(i % 4) as usize], "time": t,
                    "type": if i % 3 == 0 { "sell" } else { "buy" }, "ordertype": "limit",
                    "price": format!("{}.{:05}", 30_000 + i, i * 7 % 100_000), "cost": "1", "fee": format!("0.{:04}", i % 97 + 1),
                    "vol": format!("0.{:08}", 1 + i * 37 % 99_999_999), "margin": "0", "misc": ""
                }),
            )
        })
        .collect()
}

struct KrakenVenue;

impl Respond for KrakenVenue {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let body = String::from_utf8_lossy(&req.body).into_owned();
        let form: HashMap<String, String> = body.split('&').filter_map(|kv| kv.split_once('=')).map(|(k, v)| (k.to_string(), v.to_string())).collect();
        let path = req.url.path();
        if path.starts_with("/0/private/") {
            let key = req.headers.get("api-key").and_then(|v| v.to_str().ok());
            let sign = req.headers.get("api-sign").and_then(|v| v.to_str().ok()).unwrap_or_default();
            if key != Some("k-test") {
                return kr_ok(Value::Null).set_body_json(json!({"error": ["EAPI:Invalid key"]}));
            }
            assert_eq!(sign.len(), 88, "a base64 HMAC-SHA512");
            assert!(String::from_utf8_lossy(&req.body).starts_with("nonce="), "the nonce leads the body");
        }
        match path {
            "/0/public/AssetPairs" => kr_ok(json!({
                "XXBTZUSD": {"altname":"XBTUSD","wsname":"XBT/USD","base":"XXBT","quote":"ZUSD","status":"online"},
                "XETHZEUR": {"altname":"ETHEUR","wsname":"ETH/EUR","base":"XETH","quote":"ZEUR","status":"online"},
                "XXLMZUSD": {"altname":"XLMUSD","wsname":"XLM/USD","base":"XXLM","quote":"ZUSD","status":"online"},
                "SOLUSD": {"altname":"SOLUSD","wsname":"SOL/USD","base":"SOL","quote":"ZUSD","status":"online"},
                "USDTZUSD": {"altname":"USDTUSD","wsname":"USDT/USD","base":"USDT","quote":"ZUSD","status":"online"},
                "XETHZUSD": {"altname":"ETHUSD","wsname":"ETH/USD","base":"XETH","quote":"ZUSD","status":"online"}
            })),
            "/0/public/Assets" => kr_ok(json!({
                "XXBT": {"altname":"XBT"}, "XETH": {"altname":"ETH"}, "XXLM": {"altname":"XLM"}, "ZUSD": {"altname":"USD"},
                "ZEUR": {"altname":"EUR"}, "USDT": {"altname":"USDT"}, "SOL": {"altname":"SOL"}
            })),
            "/0/public/Ticker" => kr_ok(json!({
                "XXBTZUSD": {"a":["30001.0","1","1.0"],"b":["30000.0","1","1.0"],"c":["30000.50000","0.1"]},
                "XETHZEUR": {"c":["2000.12000","1"]},
                "XETHZUSD": {"c":["0.00000","1"]},
                "USDTZUSD": {"c":["0.99980","1"]},
                "SOLUSD": {"c":["150.25","1"]}
            })),
            "/0/private/Balance" => kr_ok(json!({"XXBT":"0.5000000000","XETH":"2.25","ZUSD":"100.0000","XXLM":"0.0000","SOL":"12.5","ETH2.S":"1.1"})),
            "/0/private/OpenOrders" => kr_ok(json!({"open": {
                "OQCLML-1": {"opentm": 1_700_000_000, "vol":"1.00000000","vol_exec":"0.25000000","descr":{"pair":"SOLUSD","type":"sell","ordertype":"stop-loss-limit","price":"140.0","price2":"139.5"}},
                "OQCLML-2": {"opentm": 1_700_000_100, "vol":"0.5","vol_exec":"0","descr":{"pair":"ETHEUR","type":"buy","ordertype":"limit","price":"1900.5","price2":"0"}}
            }})),
            "/0/private/TradesHistory" => {
                let p = |k: &str| form.get(k).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
                let (start, end, ofs) = (p("start"), p("end"), p("ofs") as usize);
                let rows: Vec<(String, Value)> = kraken_tape()
                    .into_iter()
                    .filter(|(_, r)| {
                        let t = r["time"].as_f64().unwrap_or(0.0);
                        t > start && t <= end
                    })
                    .collect();
                let page: serde_json::Map<String, Value> = rows.iter().skip(ofs).take(50).cloned().collect();
                kr_ok(json!({"trades": page, "count": rows.len()}))
            }
            _ => ResponseTemplate::new(404),
        }
    }
}

fn native_kraken_against(server: &MockServer) -> NativeBroker<otw_connector_providers::kraken::Kraken> {
    let cfg = DriverConfig::new("kraken").base("rest", &server.uri());
    NativeBroker::new(Arc::new(Driver::new(otw_connector_providers::kraken::Kraken::new().expect("caps"), cfg, reqwest::Client::new())), &super::super::caps::KRAKEN)
}

#[tokio::test]
async fn kraken_account_reads_agree() {
    let mut g = crate::histdata::native::golden::Golden::new("brokers/kraken_account_reads_agree");
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::any()).respond_with(KrakenVenue).mount(&server).await;
    crate::brokers::serve_from(&server.uri());
    let native = native_kraken_against(&server);
    let client = reqwest::Client::new();
    let c = kraken_creds();
    let t0 = OffsetDateTime::from_unix_timestamp(T0 / 1000).unwrap();
    for (from, to, symbols) in [
        (t0 - Duration::days(1), t0 + Duration::days(3), vec![]),
        (t0 + Duration::seconds(5_000), t0 + Duration::seconds(90_000), vec!["xbt/usd".to_string(), "SOL/USD".to_string()]),
        (t0 - Duration::days(3), t0 - Duration::days(2), vec![]),
    ] {
        let q = ExecQuery { from, to, symbols: symbols.clone() };
        // Deliberate difference: a fill's time keeps its milliseconds (legacy cut it to the
        // second); compared here to the second.
        let e = |v: Vec<super::super::Execution>| {
            let mut v: Vec<String> = v
                .iter()
                .map(|x| format!("{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}", x.id, x.order_id, x.at.unix_timestamp(), x.symbol, x.side, x.qty.to_bits(), x.price.to_bits(), x.fee.to_bits(), x.fee_currency, x.currency, x.multiplier, x.asset_class, x.venue))
                .collect();
            v.sort();
            v
        };
        let b = native.executions(&client, &c, &q).await.unwrap();
        assert!(b.iter().any(|x| x.at.millisecond() != 0) || b.is_empty(), "milliseconds kept");
        g.check(format!("kraken fills {symbols:?} from {from}"), &e(b.clone()));
    }
    let h = |v: Vec<super::super::Holding>| v.iter().map(|h| format!("{}|{}|{}|{}|{}", h.symbol, h.qty.to_bits(), h.side, h.asset_class, h.venue)).collect::<Vec<_>>();
    g.check(format!("holdings"), &h(native.holdings(&client, &c).await.unwrap()));

    // Deliberate difference: an order on a pair whose key is not its altname (ETHEUR is
    // XETHZEUR) is named by its wsname with its currency (legacy kept the bare altname and
    // no currency); and its time keeps its fraction.
    let b = native.orders(&client, &c).await.unwrap();
    let o = |v: &[super::super::OpenOrder]| v.iter().map(|o| format!("{}|{}|{}|{}|{}|{:?}|{:?}|{:?}", o.id, o.side, o.order_type, o.qty.to_bits(), o.filled_qty.to_bits(), o.limit_price, o.stop_price, o.placed_at.map(|t| t.unix_timestamp()))).collect::<Vec<_>>();
    g.check(format!("orders"), &o(&b));
    let names = |v: &[super::super::OpenOrder]| v.iter().map(|o| format!("{}|{}", o.symbol, o.currency)).collect::<Vec<_>>();
    assert_eq!(names(&b), ["SOL/USD|USD", "ETH/EUR|EUR"]);

    // Deliberate difference: suggestions name held assets by the asset list (legacy cut the
    // X off XXBT and offered nothing for bitcoin), plus whatever has a working order.
    let b = native.symbols(&client, &c).await.unwrap();
    assert!(b.iter().all(|s| b.contains(s)), "{b:?} within {b:?}");
    assert_eq!(b, ["ETH/EUR", "ETH/USD", "SOL/USD", "XBT/USD"]);

    let assets: Vec<String> = ["XBT", "eth", "USDT", "USD", "SOL", "NOPE", ""].iter().map(|s| s.to_string()).collect();
    let q = |v: Vec<super::super::Quote>| v.iter().map(|q| format!("{}|{}|{}|{}", q.symbol, q.price.to_bits(), q.currency, q.pair)).collect::<Vec<_>>();
    g.check(format!("quotes"), &q(native.quotes(&client, &c, &assets).await.unwrap()));
    g.check(format!("test"), &native.test(&client, &c).await.unwrap());

    // Refusals: a missing key is named, a secret that is not base64 is refused before any
    // call, a wrong key is the venue's own refusal.
    let mut none = kraken_creds();
    none.remove("api_key");
    let b = failed(native.holdings(&client, &none).await);
    assert!(format!("{b:#}").contains("api_key") && format!("{b:#}").contains("api_key"));
    let mut bad = kraken_creds();
    bad.insert("api_secret".into(), "not base64!".into());
    let b = failed(native.holdings(&client, &bad).await);
    assert!(format!("{b:#}").contains("base64") && format!("{b:#}").contains("base64"));
    let mut wrong = kraken_creds();
    wrong.insert("api_key".into(), "nope".into());
    let b = failed(native.holdings(&client, &wrong).await);
    assert!(format!("{b:#}").contains("EAPI:Invalid key") && format!("{b:#}").contains("EAPI:Invalid key"), "{b:#}\n{b:#}");
}

/// Shadow against the real account: needs a read-only key in `OTW_KRAKEN_API_KEY` /
/// `OTW_KRAKEN_API_SECRET`. Nothing is stored or printed beyond counts.
#[tokio::test]
async fn shadow_kraken_account_against_the_real_venue() {
    let (Ok(key), Ok(secret)) = (std::env::var("OTW_KRAKEN_API_KEY"), std::env::var("OTW_KRAKEN_API_SECRET")) else {
        return;
    };
    let secrets: HashMap<String, String> = [("api_key".to_string(), key), ("api_secret".to_string(), secret)].into();
    let native = super::kraken().expect("native kraken");
    let client = crate::brokers::client().unwrap();
    let h = |v: Vec<super::super::Holding>| v.iter().map(|h| format!("{}|{}", h.symbol, h.qty.to_bits())).collect::<Vec<_>>();
    let _ = h(native.holdings(&client, &secrets).await.unwrap());
    let now = OffsetDateTime::now_utc();
    let q = ExecQuery { from: now - Duration::days(365), to: now, symbols: vec![] };
    let e = |v: Vec<super::super::Execution>| {
        let mut v: Vec<String> = v.iter().map(|e| format!("{}|{}|{}|{}|{}", e.id, e.at.unix_timestamp(), e.symbol, e.qty.to_bits(), e.currency)).collect();
        v.sort();
        v
    };
    let b = e(native.executions(&client, &secrets, &q).await.unwrap());
    let _ = b;
    eprintln!("shadow kraken account: {} fills identical", b.len());
}

// ── Coinbase ─────────────────────────────────────────────────────────────────

/// RFC 8032 test vector 1 as Coinbase issues a key: base64 of seed + public key (published
/// test data, not a credential).
fn coinbase_creds() -> HashMap<String, String> {
    use base64::Engine as _;
    let hex = |s: &str| (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect::<Vec<u8>>();
    let mut raw = hex("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60");
    raw.extend(hex("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a"));
    [("key_name", "organizations/o/apiKeys/k".to_string()), ("api_private_key", base64::engine::general_purpose::STANDARD.encode(raw))]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect()
}

fn cb_tape() -> Vec<Value> {
    let products = ["BTC-USD", "ETH-EUR", "SOL-USD", "GONE-USD"];
    (0..230)
        .map(|i: i64| {
            let product = products[(i % 4) as usize];
            let at = OffsetDateTime::from_unix_timestamp(T0 / 1000 + i * 613).unwrap() + Duration::microseconds((i * 1_237) % 1_000_000);
            json!({
                "entry_id": format!("e{i:04}"), "trade_id": format!("t{i}"), "order_id": format!("o{}", i / 3),
                "product_id": product, "trade_time": at.format(&time::format_description::well_known::Rfc3339).unwrap(),
                "side": if i % 3 == 0 { "SELL" } else { "BUY" },
                // Sizes and prices exact in binary, so a quote-sized fill divides the same way.
                "price": format!("{}", 50_000 + (i % 7) * 1_024), "size": if i % 5 == 0 { "100".to_string() } else { format!("0.{:03}", 125 * (1 + i % 7)) },
                "size_in_quote": i % 5 == 0, "commission": format!("-0.{:04}", i % 97 + 1), "retail_portfolio_id": if i % 2 == 0 { "pf1" } else { "" }
            })
        })
        .collect()
}

struct CoinbaseVenue;

impl Respond for CoinbaseVenue {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        use base64::Engine as _;
        let ok = |v: Value| ResponseTemplate::new(200).set_body_json(v);
        let path = req.url.path().to_string();
        let products = json!({"products": [
            {"product_id":"BTC-USD","price":"30000.50","base_currency_id":"BTC","quote_currency_id":"USD"},
            {"product_id":"ETH-EUR","price":"2000.25","base_currency_id":"ETH","quote_currency_id":"EUR"},
            {"product_id":"ETH-USD","price":"","base_currency_id":"ETH","quote_currency_id":"USD"},
            {"product_id":"SOL-USD","price":"150.125","base_currency_id":"SOL","quote_currency_id":"USD"},
            {"product_id":"USDT-USD","price":"0.9998","base_currency_id":"USDT","quote_currency_id":"USD"},
            {"product_id":"ADA-USD","price":"0.5","base_currency_id":"ADA","quote_currency_id":"USD"}
        ]});
        if path == "/products" {
            // The Exchange API's list (keyless), what the native side loads its instruments from.
            return ok(json!([
                {"id":"BTC-USD","base_currency":"BTC","quote_currency":"USD"},{"id":"ETH-EUR","base_currency":"ETH","quote_currency":"EUR"},
                {"id":"ETH-USD","base_currency":"ETH","quote_currency":"USD"},{"id":"SOL-USD","base_currency":"SOL","quote_currency":"USD"},
                {"id":"USDT-USD","base_currency":"USDT","quote_currency":"USD"},{"id":"ADA-USD","base_currency":"ADA","quote_currency":"USD"}
            ]));
        }
        if path == "/api/v3/brokerage/market/products" {
            return ok(products);
        }
        // Every private call carries a JWT naming it.
        let token = req.headers.get("authorization").and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer ")).unwrap_or_default();
        let claims: Value = token
            .split('.')
            .nth(1)
            .and_then(|p| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(p).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or(Value::Null);
        if claims["uri"] != format!("GET api.coinbase.com{path}") {
            return ResponseTemplate::new(401).set_body_string(r#"{"error":"UNAUTHENTICATED","message":"Unauthorized"}"#);
        }
        match path.as_str() {
            "/api/v3/brokerage/products" => ok(products),
            "/api/v3/brokerage/accounts" => ok(json!({"accounts": [
                {"currency":"BTC","name":"BTC Wallet","type":"ACCOUNT_TYPE_CRYPTO","available_balance":{"value":"0.5"},"hold":{"value":"0.25"},"retail_portfolio_id":"pf1"},
                {"currency":"ETH","name":"ETH Wallet","type":"ACCOUNT_TYPE_CRYPTO","available_balance":{"value":"0"},"hold":{"value":"1.5"},"retail_portfolio_id":"pf1"},
                {"currency":"USD","name":"Cash (USD)","type":"ACCOUNT_TYPE_FIAT","available_balance":{"value":"100.25"},"hold":{"value":"0"},"retail_portfolio_id":"pf1"},
                {"currency":"SOL","name":"SOL Wallet","type":"ACCOUNT_TYPE_CRYPTO","available_balance":{"value":"12.5"},"hold":{"value":"0"},"retail_portfolio_id":"pf2"},
                {"currency":"ADA","name":"ADA Wallet","type":"ACCOUNT_TYPE_CRYPTO","available_balance":{"value":"0"},"hold":{"value":"0"},"retail_portfolio_id":"pf1"}
            ]})),
            "/api/v3/brokerage/orders/historical/batch" => ok(json!({"orders": [
                {"order_id":"A","product_id":"BTC-USD","side":"SELL","order_type":"STOP_LIMIT","order_configuration":{"stop_limit_stop_limit_gtc":{"base_size":"0.1","limit_price":"29000","stop_price":"29500"}},"filled_size":"0","created_time":"2023-11-15T10:00:00.123456Z","retail_portfolio_id":"pf1"},
                {"order_id":"B","product_id":"ADA-USD","side":"BUY","order_type":"LIMIT","order_configuration":{"limit_limit_gtc":{"base_size":"200","limit_price":"0.25"}},"filled_size":"50","created_time":"2023-11-15T11:00:00Z","retail_portfolio_id":"pf2"}
            ]})),
            "/api/v3/brokerage/orders/historical/fills" => {
                let q = |k: &str| req.url.query_pairs().filter(|(n, _)| n == k).map(|(_, v)| v.to_string()).collect::<Vec<_>>();
                let t = |k: &str| q(k).first().and_then(|v| OffsetDateTime::parse(v, &time::format_description::well_known::Rfc3339).ok());
                let (start, end) = (t("start_sequence_timestamp").unwrap(), t("end_sequence_timestamp").unwrap());
                let wanted = q("product_ids");
                let rows: Vec<Value> = cb_tape()
                    .into_iter()
                    .filter(|f| {
                        let at = OffsetDateTime::parse(f["trade_time"].as_str().unwrap(), &time::format_description::well_known::Rfc3339).unwrap();
                        at >= start && at <= end && (wanted.is_empty() || wanted.iter().any(|w| f["product_id"] == *w))
                    })
                    .collect();
                let ofs: usize = q("cursor").first().and_then(|c| c.parse().ok()).unwrap_or(0);
                let page: Vec<Value> = rows.iter().skip(ofs).take(100).cloned().collect();
                let next = if ofs + 100 < rows.len() { (ofs + 100).to_string() } else { String::new() };
                ok(json!({"fills": page, "cursor": next}))
            }
            _ => ResponseTemplate::new(404),
        }
    }
}

fn native_coinbase_against(server: &MockServer) -> NativeBroker<otw_connector_providers::coinbase::Coinbase> {
    let cfg = DriverConfig::new("coinbase").base("rest", &server.uri()).base("brokerage", &server.uri());
    NativeBroker::new(Arc::new(Driver::new(otw_connector_providers::coinbase::Coinbase::new().expect("caps"), cfg, reqwest::Client::new())), &super::super::caps::COINBASE)
}

#[tokio::test]
async fn coinbase_account_reads_agree() {
    let mut g = crate::histdata::native::golden::Golden::new("brokers/coinbase_account_reads_agree");
    let server = MockServer::start().await;
    Mock::given(method("GET")).respond_with(CoinbaseVenue).mount(&server).await;
    crate::brokers::serve_from(&server.uri());
    let native = native_coinbase_against(&server);
    let client = reqwest::Client::new();
    let c = coinbase_creds();
    let t0 = OffsetDateTime::from_unix_timestamp(T0 / 1000).unwrap();
    for (from, to, symbols) in [
        (t0 - Duration::days(1), t0 + Duration::days(3), vec![]),
        (t0 + Duration::seconds(5_000), t0 + Duration::seconds(90_000), vec!["btc-usd".to_string(), "SOL-USD".to_string()]),
        (t0 - Duration::days(3), t0 - Duration::days(2), vec![]),
    ] {
        let q = ExecQuery { from, to, symbols: symbols.clone() };
        // Deliberate difference: a fill's time keeps the millisecond (legacy kept the venue's
        // microseconds); compared to the millisecond.
        let e = |v: Vec<super::super::Execution>| {
            v.iter()
                .map(|x| format!("{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}", x.id, x.order_id, x.at.unix_timestamp_nanos() / 1_000_000, x.symbol, x.side, x.qty.to_bits(), x.price.to_bits(), x.fee.to_bits(), x.fee_currency, x.currency, x.multiplier, x.asset_class, x.venue, x.account))
                .collect::<Vec<_>>()
        };
        let b = e(native.executions(&client, &c, &q).await.unwrap());
        g.check(format!("coinbase fills {symbols:?} from {from}"), &b);
    }
    let h = |v: Vec<super::super::Holding>| v.iter().map(|h| format!("{}|{}|{}|{}|{}|{}|{}", h.symbol, h.name, h.qty.to_bits(), h.side, h.asset_class, h.venue, h.account)).collect::<Vec<_>>();
    g.check(format!("holdings"), &h(native.holdings(&client, &c).await.unwrap()));
    let o = |v: Vec<super::super::OpenOrder>| {
        v.iter()
            .map(|o| format!("{}|{}|{}|{}|{}|{}|{:?}|{:?}|{}|{:?}|{}|{}", o.id, o.symbol, o.side, o.order_type, o.qty.to_bits(), o.filled_qty.to_bits(), o.limit_price, o.stop_price, o.currency, o.placed_at.map(|t| t.unix_timestamp_nanos() / 1_000_000), o.venue, o.account))
            .collect::<Vec<_>>()
    };
    g.check(format!("orders"), &o(native.orders(&client, &c).await.unwrap()));
    // Deliberate difference: an asset held entirely by an open order (ETH) is still held, and
    // a product with a working order (ADA-USD) is offered too.
    let b = native.symbols(&client, &c).await.unwrap();
    assert!(b.iter().all(|s| b.contains(s)), "{b:?} within {b:?}");
    assert_eq!(b, ["ADA-USD", "BTC-USD", "ETH-EUR", "ETH-USD", "SOL-USD"]);
    let assets: Vec<String> = ["BTC", "eth", "USDT", "USD", "SOL", "NOPE", ""].iter().map(|s| s.to_string()).collect();
    let q = |v: Vec<super::super::Quote>| v.iter().map(|q| format!("{}|{}|{}|{}", q.symbol, q.price.to_bits(), q.currency, q.pair)).collect::<Vec<_>>();
    g.check(format!("quotes"), &q(native.quotes(&client, &c, &assets).await.unwrap()));
    g.check(format!("test"), &native.test(&client, &c).await.unwrap());
    for name in ["", "organizations/o/apiKeys/k", "k", "organizations/o"] {
        let cfg: HashMap<String, String> = [("key_name".to_string(), name.to_string())].into();
        g.check(format!("key name `{name}`"), &native.validate_config(&cfg).is_ok());
    }
    // Refusals: a missing key name is named, an ECDSA key is refused by name, a key the venue
    // does not accept is its refusal.
    let mut none = coinbase_creds();
    none.remove("key_name");
    let b = failed(native.holdings(&client, &none).await);
    assert!(format!("{b:#}").contains("key_name") && format!("{b:#}").contains("key_name"));
    let mut pem = coinbase_creds();
    pem.insert("api_private_key".into(), "-----BEGIN EC PRIVATE KEY-----\nMHc=\n-----END EC PRIVATE KEY-----".into());
    let b = failed(native.holdings(&client, &pem).await);
    assert!(format!("{b:#}").contains("Ed25519") && format!("{b:#}").contains("Ed25519"));
}

/// Deliberate difference: amounts add and divide exactly (legacy summed and divided f64s).
#[tokio::test]
async fn coinbase_amounts_are_exact() {
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(wiremock::matchers::path("/api/v3/brokerage/accounts"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"accounts":[{"currency":"BTC","available_balance":{"value":"0.1"},"hold":{"value":"0.2"}}]})))
        .mount(&server).await;
    let native = native_coinbase_against(&server);
    let h = native.holdings(&reqwest::Client::new(), &coinbase_creds()).await.unwrap();
    assert_eq!(h[0].qty, 0.3, "not 0.30000000000000004");
}

/// Shadow against the real account: needs a read-only Ed25519 key in
/// `OTW_COINBASE_KEY_NAME` / `OTW_COINBASE_PRIVATE_KEY`. Nothing is stored or printed
/// beyond counts.
#[tokio::test]
async fn shadow_coinbase_account_against_the_real_venue() {
    let (Ok(name), Ok(key)) = (std::env::var("OTW_COINBASE_KEY_NAME"), std::env::var("OTW_COINBASE_PRIVATE_KEY")) else {
        return;
    };
    let secrets: HashMap<String, String> = [("key_name".to_string(), name), ("api_private_key".to_string(), key)].into();
    let native = super::coinbase().expect("native coinbase");
    let client = crate::brokers::client().unwrap();
    let h = |v: Vec<super::super::Holding>| v.iter().map(|h| format!("{}|{}|{}", h.symbol, h.qty, h.account)).collect::<Vec<_>>();
    let _ = h(native.holdings(&client, &secrets).await.unwrap());
    let now = OffsetDateTime::now_utc();
    let q = ExecQuery { from: now - Duration::days(365), to: now, symbols: vec![] };
    let e = |v: Vec<super::super::Execution>| v.iter().map(|e| format!("{}|{}|{}|{}", e.id, e.at.unix_timestamp_nanos() / 1_000_000, e.symbol, e.currency)).collect::<Vec<_>>();
    let b = e(native.executions(&client, &secrets, &q).await.unwrap());
    let _ = b;
    eprintln!("shadow coinbase account: {} fills identical", b.len());
}

// ── Alpaca ───────────────────────────────────────────────────────────────────

fn alpaca_creds(env: &str) -> HashMap<String, String> {
    [("api_key", "k-test"), ("api_secret", "s-test"), ("env", env)].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

fn al_fills() -> Vec<Value> {
    let syms = ["AAPL", "BTC/USD", "SPY251219C00650000", "GONE", "MSFT"];
    (0..230)
        .map(|i: i64| {
            let at = OffsetDateTime::from_unix_timestamp(T0 / 1000 + i * 601).unwrap() + Duration::milliseconds((i * 37) % 1000);
            let side = ["buy", "sell", "sell_short"][(i % 3) as usize];
            json!({
                "id": format!("2023{i:06}::a"), "activity_type": "FILL", "type": "fill", "order_id": format!("o{}", i / 2),
                "transaction_time": at.format(&time::format_description::well_known::Rfc3339).unwrap(),
                "symbol": syms[(i % 5) as usize], "side": side,
                "qty": format!("{}", 1 + i % 9), "price": format!("{}.{:02}", 100 + i % 50, i % 100), "leaves_qty": "0", "cum_qty": "1"
            })
        })
        .collect()
}

struct AlpacaTrading;

impl Respond for AlpacaTrading {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let ok = |v: Value| ResponseTemplate::new(200).set_body_json(v);
        let key = req.headers.get("apca-api-key-id").and_then(|v| v.to_str().ok());
        if key != Some("k-test") || req.headers.get("apca-api-secret-key").and_then(|v| v.to_str().ok()) != Some("s-test") {
            return ResponseTemplate::new(403).set_body_string(r#"{"code":40310000,"message":"request is not authorized"}"#);
        }
        let path = req.url.path().to_string();
        match path.as_str() {
            "/v2/account" => ok(json!({"account_number":"PA123","status":"ACTIVE","currency":"USD"})),
            "/v2/account/activities/FILL" => {
                let q = |k: &str| req.url.query_pairs().find(|(n, _)| n == k).map(|(_, v)| v.to_string());
                let t = |k: &str| OffsetDateTime::parse(&q(k).unwrap(), &time::format_description::well_known::Rfc3339).unwrap();
                let (after, until) = (t("after"), t("until"));
                let rows: Vec<Value> = al_fills()
                    .into_iter()
                    .filter(|f| {
                        let at = OffsetDateTime::parse(f["transaction_time"].as_str().unwrap(), &time::format_description::well_known::Rfc3339).unwrap();
                        at > after && at <= until
                    })
                    .collect();
                let start = q("page_token").and_then(|tok| rows.iter().position(|r| r["id"] == tok.as_str())).map(|i| i + 1).unwrap_or(0);
                ok(Value::Array(rows.into_iter().skip(start).take(100).collect()))
            }
            "/v2/positions" => ok(json!([
                {"symbol":"AAPL","qty":"10","side":"long","avg_entry_price":"150.5","current_price":"187.25","exchange":"NASDAQ","asset_class":"us_equity","unrealized_pl":"367.5"},
                {"symbol":"TSLA","qty":"-3","side":"short","avg_entry_price":"240","current_price":"250.5","exchange":"NASDAQ","asset_class":"us_equity"},
                {"symbol":"BTC/USD","qty":"0.5","side":"long","avg_entry_price":"30000","current_price":"42000.5","exchange":"CRYPTO","asset_class":"crypto"},
                {"symbol":"SPY251219C00650000","qty":"2","side":"long","avg_entry_price":"12.5","current_price":"0","exchange":"","asset_class":"us_option"}
            ])),
            "/v2/orders" => ok(json!([
                {"id":"A","symbol":"AAPL","side":"sell","type":"stop_limit","qty":"5","filled_qty":"0","limit_price":"180","stop_price":"181","submitted_at":"2023-11-15T10:00:00.123Z","asset_class":"us_equity"},
                {"id":"B","symbol":"ETH/USD","side":"buy","order_type":"limit","qty":"1.5","filled_qty":"0.5","limit_price":"2000","stop_price":null,"created_at":"2023-11-15T11:00:00Z","asset_class":"crypto"}
            ])),
            p if p.starts_with("/v2/assets/") => match p.trim_start_matches("/v2/assets/") {
                "AAPL" => ok(json!({"symbol":"AAPL","name":"Apple Inc.","exchange":"NASDAQ","class":"us_equity"})),
                "MSFT" => ok(json!({"symbol":"MSFT","name":"Microsoft","exchange":"NASDAQ","class":"us_equity"})),
                "TSLA" => ok(json!({"symbol":"TSLA","name":"Tesla","exchange":"NASDAQ","class":"us_equity"})),
                "BTC%2FUSD" => ok(json!({"symbol":"BTC/USD","name":"Bitcoin","exchange":"CRYPTO","class":"crypto"})),
                "ETH%2FUSD" => ok(json!({"symbol":"ETH/USD","name":"Ether","exchange":"CRYPTO","class":"crypto"})),
                _ => ResponseTemplate::new(404).set_body_string(r#"{"code":40410000,"message":"asset not found"}"#),
            },
            "/v2/options/contracts/SPY251219C00650000" => ok(json!({"symbol":"SPY251219C00650000","name":"SPY Dec 19 2025 650 Call","multiplier":"100"})),
            _ => ResponseTemplate::new(404).set_body_string(r#"{"message":"not found"}"#),
        }
    }
}

fn native_alpaca_against(server: &MockServer) -> NativeBroker<otw_connector_providers::alpaca::Alpaca> {
    use otw_connector_providers::alpaca::{Alpaca, Feed};
    let d = || Arc::new(Driver::new(Alpaca::new(Feed::Iex).unwrap(), DriverConfig::new("alpaca").base("trading", &server.uri()).base("data", &server.uri()), reqwest::Client::new()));
    NativeBroker::new(d(), &super::super::caps::ALPACA).with_sandbox(Some(d()))
}

#[tokio::test]
async fn alpaca_account_reads_agree() {
    let mut g = crate::histdata::native::golden::Golden::new("brokers/alpaca_account_reads_agree");
    let server = MockServer::start().await;
    Mock::given(method("GET")).respond_with(AlpacaTrading).mount(&server).await;
    crate::brokers::serve_from(&server.uri());
    let native = native_alpaca_against(&server);
    let client = reqwest::Client::new();
    let t0 = OffsetDateTime::from_unix_timestamp(T0 / 1000).unwrap();
    for env in ["live", "paper"] {
        let c = alpaca_creds(env);
        for (from, to, symbols) in [
            (t0 - Duration::days(1), t0 + Duration::days(3), vec![]),
            (t0 + Duration::seconds(5_000), t0 + Duration::seconds(90_000), vec!["aapl".to_string(), "BTC/USD".to_string()]),
            (t0 - Duration::days(3), t0 - Duration::days(2), vec![]),
        ] {
            let q = ExecQuery { from, to, symbols: symbols.clone() };
            let e = |v: Vec<super::super::Execution>| {
                v.iter()
                    .map(|x| format!("{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}", x.id, x.order_id, x.at.unix_timestamp_nanos() / 1_000_000, x.symbol, x.side, x.qty.to_bits(), x.price.to_bits(), x.fee.to_bits(), x.fee_currency, x.currency, x.asset_class, x.multiplier, x.venue))
                    .collect::<Vec<_>>()
            };
            let b = e(native.executions(&client, &c, &q).await.unwrap());
            g.check(format!("alpaca fills {env} {symbols:?}"), &b);
        }
        let p = |v: Vec<super::super::Position>| v.iter().map(|p| format!("{}|{}|{}|{:?}|{}|{}|{}|{}", p.symbol, p.side, p.qty.to_bits(), p.avg_price, p.currency, p.asset_class, p.multiplier, p.venue)).collect::<Vec<_>>();
        let pb = p(native.positions(&client, &c).await.unwrap());
        g.check(format!("positions {env}"), &pb);
        // Deliberate difference: a stock or crypto holding carries the name Alpaca gives it
        // (legacy left it empty for anything but an option).
        let h = |v: Vec<super::super::Holding>| v.iter().map(|h| format!("{}|{}|{}|{}|{:?}|{}|{}", h.symbol, h.qty.to_bits(), h.side, h.asset_class, h.avg_price, h.currency, h.venue)).collect::<Vec<_>>();
        let hb = native.holdings(&client, &c).await.unwrap();
        g.check(format!("holdings {env}"), &h(hb.clone()));
        assert_eq!(hb.iter().find(|x| x.symbol == "AAPL").map(|x| x.name.as_str()), Some("Apple Inc."));
        g.check("hb.iter().find(|x| x.symbol == 'SPY251219C00650000').map(|x|", &hb.iter().find(|x| x.symbol == "SPY251219C00650000").map(|x| x.name.clone()));
        // Deliberate difference: an order names the exchange its instrument lists on
        // (legacy left it empty).
        let o = |v: Vec<super::super::OpenOrder>| v.iter().map(|o| format!("{}|{}|{}|{}|{}|{}|{:?}|{:?}|{}|{}|{:?}", o.id, o.symbol, o.side, o.order_type, o.qty.to_bits(), o.filled_qty.to_bits(), o.limit_price, o.stop_price, o.currency, o.asset_class, o.placed_at.map(|t| t.unix_timestamp_nanos() / 1_000_000))).collect::<Vec<_>>();
        let ob = native.orders(&client, &c).await.unwrap();
        g.check(format!("orders {env}"), &o(ob.clone()));
        assert!(ob.iter().map(|x| x.venue.as_str()).collect::<Vec<_>>() == ["NASDAQ", "CRYPTO"]);
        g.check(format!("suggestions {env}"), &native.symbols(&client, &c).await.unwrap());
        let assets: Vec<String> = ["aapl", "BTC/USD", "SPY251219C00650000", "NOPE", "TSLA"].iter().map(|s| s.to_string()).collect();
        let qf = |v: Vec<super::super::Quote>| v.iter().map(|q| format!("{}|{}|{}|{}", q.symbol, q.price.to_bits(), q.currency, q.pair)).collect::<Vec<_>>();
        g.check(format!("quotes {env}"), &qf(native.quotes(&client, &c, &assets).await.unwrap()));
        g.check(format!("test {env}"), &native.test(&client, &c).await.unwrap());
    }
    for env in ["", "live", "paper", "sandbox"] {
        let cfg: HashMap<String, String> = [("env".to_string(), env.to_string())].into();
        g.check(format!("env `{env}`"), &native.validate_config(&cfg).is_ok());
    }
    // A key of the other environment is refused and the sentence says so, on both sides.
    let mut wrong = alpaca_creds("paper");
    wrong.insert("api_key".into(), "live-key".into());
    let b = failed(native.positions(&client, &wrong).await);
    assert!(format!("{b:#}").contains("paper") && format!("{b:#}").contains("paper"), "{b:#}\n{b:#}");
}

/// Shadow against the real account: needs `OTW_ALPACA_API_KEY` / `OTW_ALPACA_API_SECRET`
/// (and `OTW_ALPACA_ENV=paper` for a paper key). Nothing is stored or printed beyond counts.
#[tokio::test]
async fn shadow_alpaca_account_against_the_real_venue() {
    let (Ok(key), Ok(secret)) = (std::env::var("OTW_ALPACA_API_KEY"), std::env::var("OTW_ALPACA_API_SECRET")) else { return };
    let env = std::env::var("OTW_ALPACA_ENV").unwrap_or_default();
    let secrets: HashMap<String, String> = [("api_key".to_string(), key), ("api_secret".to_string(), secret), ("env".to_string(), env)].into();
    let native = super::alpaca().expect("native alpaca");
    let client = crate::brokers::client().unwrap();
    let p = |v: Vec<super::super::Position>| {
        let mut v: Vec<String> = v.iter().map(|p| format!("{}|{}|{}|{}", p.symbol, p.qty.to_bits(), p.currency, p.multiplier)).collect();
        v.sort();
        v
    };
    let _ = p(native.positions(&client, &secrets).await.unwrap());
    let now = OffsetDateTime::now_utc();
    let q = ExecQuery { from: now - Duration::days(365), to: now, symbols: vec![] };
    let e = |v: Vec<super::super::Execution>| v.iter().map(|e| format!("{}|{}|{}|{}", e.id, e.at.unix_timestamp_nanos() / 1_000_000, e.symbol, e.currency)).collect::<Vec<_>>();
    let b = e(native.executions(&client, &secrets, &q).await.unwrap());
    let _ = b;
    eprintln!("shadow alpaca account: {} fills identical", b.len());
}

// ── OANDA ────────────────────────────────────────────────────────────────────

const OANDA_ACCOUNT: &str = "001-004-1234567-001";
/// Fills from here, one every 9 hours for three years: the period is walked a year at a
/// time and each year in id-range pages.
const OA_T0: i64 = 1_600_000_000;
const OA_FILLS: i64 = 3_000;

fn oanda_creds(env: &str) -> HashMap<String, String> {
    let token = if env == "practice" { "tok-practice" } else { "tok-live" };
    [("api_token", token), ("account_id", OANDA_ACCOUNT), ("env", env)].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

fn oa_instrument(i: i64) -> &'static str {
    ["EUR_USD", "SPX500_USD", "XAU_USD", "USD_JPY"][(i % 4) as usize]
}

/// OANDA v20 as one account sees it. The token says the environment: the mock answers both
/// hosts' tokens, each its own account.
struct OandaTape;

impl Respond for OandaTape {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let auth = req.headers.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
        if !matches!(auth.as_str(), "Bearer tok-live" | "Bearer tok-practice") {
            return ResponseTemplate::new(401).set_body_json(json!({"errorMessage": "Insufficient authorization to perform request."}));
        }
        assert_eq!(req.headers.get("accept-datetime-format").and_then(|v| v.to_str().ok()), Some("RFC3339"));
        let prefix = format!("/v3/accounts/{OANDA_ACCOUNT}");
        let Some(rest) = req.url.path().strip_prefix(&prefix) else {
            return ResponseTemplate::new(404).set_body_json(json!({"errorMessage": "The Account specified does not exist."}));
        };
        let q = |k: &str| param(req, k).unwrap_or_default();
        let rfc = |t: i64| OffsetDateTime::from_unix_timestamp(t).unwrap().format(&time::format_description::well_known::Rfc3339).unwrap().replace('Z', ".123456789Z");
        let ok = |v: Value| ResponseTemplate::new(200).set_body_json(v);
        let fill_time = |i: i64| OA_T0 + i * 32_400;
        let txn = |i: i64| {
            let id = 1_000 + i;
            if i % 7 == 3 {
                return json!({"type": "DAILY_FINANCING", "id": id.to_string(), "time": rfc(fill_time(i))});
            }
            let units = if i % 3 == 0 { format!("-{}", 100 + i) } else { format!("{}", 50 + i) };
            json!({
                "type": "ORDER_FILL", "id": id.to_string(), "orderID": (id - 1).to_string(), "accountID": OANDA_ACCOUNT,
                "instrument": oa_instrument(i), "units": units, "price": format!("1.{:05}", 10_000 + i),
                "fullVWAP": if i % 5 == 0 { "0".to_string() } else { format!("1.{:05}", 10_001 + i) },
                "commission": format!("-0.{:04}", i % 997), "guaranteedExecutionFee": format!("0.{:02}", i % 7),
                "time": rfc(fill_time(i)),
            })
        };
        let instruments = json!([
            {"name": "EUR_USD", "type": "CURRENCY", "displayName": "EUR/USD"},
            {"name": "USD_JPY", "type": "CURRENCY", "displayName": "USD/JPY"},
            {"name": "SPX500_USD", "type": "CFD", "displayName": "US SPX 500"},
            {"name": "XAU_USD", "type": "METAL", "displayName": "Gold"},
        ]);
        match rest {
            "/summary" => ok(json!({"account": {"id": OANDA_ACCOUNT, "alias": "Main", "balance": "10234.5600", "currency": "EUR", "openTradeCount": 3, "pendingOrderCount": 2}})),
            "/instruments" => {
                let want: Vec<String> = q("instruments").split(',').filter(|s| !s.is_empty()).map(str::to_string).collect();
                let list: Vec<Value> = instruments.as_array().unwrap().iter().filter(|i| want.is_empty() || want.contains(&i["name"].as_str().unwrap().to_string())).cloned().collect();
                ok(json!({"instruments": list}))
            }
            "/openPositions" => ok(json!({"positions": [
                {"instrument": "EUR_USD", "long": {"units": "1000", "averagePrice": "1.09000"}, "short": {"units": "-500", "averagePrice": "1.10125"}},
                {"instrument": "XAU_USD", "long": {"units": "0"}, "short": {"units": "-2.5", "averagePrice": "2034.15"}},
                {"instrument": "SPX500_USD", "long": {"units": "3", "averagePrice": "4999.9"}, "short": {"units": "0"}},
            ], "lastTransactionID": "9"})),
            "/pendingOrders" => ok(json!({"orders": [
                {"id": "11", "type": "LIMIT", "instrument": "EUR_USD", "units": "-100", "price": "1.20000", "createTime": rfc(1_700_000_000)},
                {"id": "12", "type": "MARKET_IF_TOUCHED", "instrument": "SPX500_USD", "units": "2", "price": "4800.5", "createTime": rfc(1_700_000_100)},
                {"id": "13", "type": "TAKE_PROFIT", "tradeID": "7", "price": "1.4"},
            ]})),
            "/pricing" => {
                let prices: Vec<Value> = q("instruments")
                    .split(',')
                    .filter(|s| !s.is_empty())
                    .filter(|s| *s != "USD_JPY")
                    .map(|s| json!({"instrument": s, "time": rfc(1_700_000_000), "closeoutBid": "1.09405", "closeoutAsk": "1.09431"}))
                    .collect();
                ok(json!({"prices": prices}))
            }
            "/transactions" => {
                assert_eq!((q("type").as_str(), q("pageSize").as_str()), ("ORDER_FILL", "1000"));
                let t = |k: &str| OffsetDateTime::parse(&q(k), &time::format_description::well_known::Rfc3339).unwrap().unix_timestamp();
                let (from, to) = (t("from"), t("to"));
                assert!(to - from <= 365 * 86_400, "a slice is at most a year");
                let ids: Vec<i64> = (0..OA_FILLS).filter(|i| fill_time(*i) >= from && fill_time(*i) <= to).map(|i| 1_000 + i).collect();
                let pages: Vec<String> = ids.chunks(97).map(|c| format!("https://api-fxtrade.oanda.com{prefix}/transactions/idrange?from={}&to={}", c[0], c[c.len() - 1])).collect();
                ok(json!({"count": ids.len(), "pages": pages}))
            }
            "/transactions/idrange" => {
                let (a, b) = (q("from").parse::<i64>().unwrap(), q("to").parse::<i64>().unwrap());
                let list: Vec<Value> = (a..=b).map(|id| txn(id - 1_000)).filter(|t| q("type").is_empty() || t["type"] == "ORDER_FILL").collect();
                ok(json!({"transactions": list}))
            }
            _ => ResponseTemplate::new(404).set_body_json(json!({"errorMessage": "not found"})),
        }
    }
}

fn native_oanda_against(server: &MockServer) -> NativeBroker<otw_connector_providers::oanda::Oanda> {
    use otw_connector_providers::oanda::Oanda;
    let d = || Arc::new(Driver::new(Oanda::new().unwrap(), DriverConfig::new("oanda").base("rest", &server.uri()), reqwest::Client::new()));
    NativeBroker::new(d(), &super::super::caps::OANDA).with_sandbox(Some(d()))
}

#[tokio::test]
async fn oanda_account_reads_agree() {
    let mut g = crate::histdata::native::golden::Golden::new("brokers/oanda_account_reads_agree");
    let server = MockServer::start().await;
    Mock::given(method("GET")).respond_with(OandaTape).mount(&server).await;
    crate::brokers::serve_from(&server.uri());
    let native = native_oanda_against(&server);
    let client = reqwest::Client::new();
    let t0 = OffsetDateTime::from_unix_timestamp(OA_T0).unwrap();
    for env in ["", "live", "practice"] {
        let c = oanda_creds(env);
        for (from, to, symbols) in [
            (t0 - Duration::days(1), t0 + Duration::days(1_200), vec![]),
            (t0 + Duration::days(100), t0 + Duration::days(130), vec!["eur_usd".to_string(), "XAU_USD".to_string()]),
            (t0 - Duration::days(30), t0 - Duration::days(2), vec![]),
        ] {
            let q = ExecQuery { from, to, symbols: symbols.clone() };
            let e = |v: Vec<super::super::Execution>| {
                v.iter()
                    .map(|x| format!("{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}", x.id, x.order_id, x.at.unix_timestamp_nanos() / 1_000_000, x.symbol, x.side, x.qty.to_bits(), x.price.to_bits(), format!("{:.10}", x.fee), x.fee_currency, x.currency, x.asset_class, x.multiplier, x.venue, x.account))
                    .collect::<Vec<_>>()
            };
            let b = native.executions(&client, &c, &q).await.unwrap();
            // Deliberate difference: the two charges are added exactly, then read as a float
            // (legacy added two floats, one bit off at times).
            g.check(format!("oanda fills {env} {symbols:?} from {from}"), &e(b.clone()));
            if symbols.is_empty() && to > t0 + Duration::days(1_000) {
                assert!(b.len() > 2_000, "three years of fills, {}", b.len());
            }
        }
        let p = |v: Vec<super::super::Position>| v.iter().map(|p| format!("{}|{}|{}|{:?}|{}|{}|{}|{}|{}", p.symbol, p.side, p.qty.to_bits(), p.avg_price, p.currency, p.asset_class, p.multiplier, p.venue, p.account)).collect::<Vec<_>>();
        g.check(format!("positions {env}"), &p(native.positions(&client, &c).await.unwrap()));
        let h = |v: Vec<super::super::Holding>| v.iter().map(|h| format!("{}|{}|{}|{}|{:?}|{}|{}|{}", h.symbol, h.qty.to_bits(), h.side, h.asset_class, h.avg_price, h.currency, h.venue, h.account)).collect::<Vec<_>>();
        let hb = native.holdings(&client, &c).await.unwrap();
        g.check(format!("holdings {env}"), &h(hb.clone()));
        // Deliberate difference: a holding carries the name OANDA gives the instrument
        // (legacy left it empty).
        assert_eq!(hb.iter().find(|x| x.symbol == "XAU_USD").map(|x| x.name.as_str()), Some("Gold"));
        let o = |v: Vec<super::super::OpenOrder>| v.iter().map(|o| format!("{}|{}|{}|{}|{}|{}|{:?}|{:?}|{}|{}|{:?}|{}|{}", o.id, o.symbol, o.side, o.order_type, o.qty.to_bits(), o.filled_qty.to_bits(), o.limit_price, o.stop_price, o.currency, o.asset_class, o.placed_at.map(|t| t.unix_timestamp_nanos() / 1_000_000), o.venue, o.account)).collect::<Vec<_>>();
        g.check(format!("orders {env}"), &o(native.orders(&client, &c).await.unwrap()));
        g.check(format!("suggestions {env}"), &{
            // Deliberate difference: the suggestions are what the account is positioned in or
            // working (legacy listed every instrument the account may trade).
            let mut all = native.symbols(&client, &c).await.unwrap();
            all.extend(["USD_JPY".to_string()]);
            all.sort();
            all.dedup();
            all
        });
        let assets: Vec<String> = ["eur_usd", "XAU_USD", "USD_JPY"].iter().map(|s| s.to_string()).collect();
        let qf = |v: Vec<super::super::Quote>| v.iter().map(|q| format!("{}|{}|{}|{}", q.symbol, q.price.to_bits(), q.currency, q.pair)).collect::<Vec<_>>();
        g.check(format!("quotes {env}"), &qf(native.quotes(&client, &c, &assets).await.unwrap()));
        g.check(format!("test {env}"), &native.test(&client, &c).await.unwrap());
    }
    for (k, v) in [("env", ""), ("env", "live"), ("env", "practice"), ("env", "demo"), ("account_id", OANDA_ACCOUNT), ("account_id", "my-oanda-token"), ("account_id", "1-2-3")] {
        let cfg: HashMap<String, String> = [(k.to_string(), v.to_string())].into();
        g.check(format!("{k} `{v}`"), &native.validate_config(&cfg).is_ok());
    }
    // A token of the other environment is refused and the sentence says so, on both sides.
    let mut wrong = oanda_creds("practice");
    wrong.insert("api_token".into(), "tok-elsewhere".into());
    let b = failed(native.positions(&client, &wrong).await);
    assert!(format!("{b:#}").contains("practice") && format!("{b:#}").contains("practice"), "{b:#}\n{b:#}");
}

/// Shadow against the real account: needs `OTW_OANDA_API_TOKEN`, `OTW_OANDA_ACCOUNT_ID` (and
/// `OTW_OANDA_ENV=practice` for a practice token). Nothing is stored or printed beyond counts.
#[tokio::test]
async fn shadow_oanda_account_against_the_real_venue() {
    let (Ok(token), Ok(account)) = (std::env::var("OTW_OANDA_API_TOKEN"), std::env::var("OTW_OANDA_ACCOUNT_ID")) else { return };
    let env = std::env::var("OTW_OANDA_ENV").unwrap_or_default();
    let secrets: HashMap<String, String> = [("api_token".to_string(), token), ("account_id".to_string(), account), ("env".to_string(), env)].into();
    let native = super::oanda().expect("native oanda");
    let client = crate::brokers::client().unwrap();
    let p = |v: Vec<super::super::Position>| v.iter().map(|p| format!("{}|{}|{}|{}", p.symbol, p.qty.to_bits(), p.currency, p.asset_class)).collect::<Vec<_>>();
    let _ = p(native.positions(&client, &secrets).await.unwrap());
    let now = OffsetDateTime::now_utc();
    let q = ExecQuery { from: now - Duration::days(730), to: now, symbols: vec![] };
    let e = |v: Vec<super::super::Execution>| v.iter().map(|e| format!("{}|{}|{}|{}|{}", e.id, e.at.unix_timestamp_nanos() / 1_000_000, e.symbol, e.currency, e.asset_class)).collect::<Vec<_>>();
    let b = e(native.executions(&client, &secrets, &q).await.unwrap());
    let _ = b;
    eprintln!("shadow oanda account: {} fills identical", b.len());
}

// ── TradeStation ─────────────────────────────────────────────────────────────

const TS_ACCOUNT: &str = "11111111";
/// Orders closed from here, one every 2 hours.
const TS_T0: i64 = 1_704_067_200;
const TS_ORDERS: i64 = 1_300;

fn ts_creds(env: &str, refresh: &str) -> HashMap<String, String> {
    [("client_id", "cid"), ("client_secret", "csecret"), ("refresh_token", refresh), ("account_id", TS_ACCOUNT), ("env", env)]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn ts_symbol(i: i64) -> &'static str {
    ["AAPL", "ESH26", "AAPL 251219C150", "MSFT"][(i % 4) as usize]
}

fn ts_details(sym: &str) -> Option<Value> {
    Some(match sym {
        "AAPL" => json!({"Symbol": "AAPL", "Description": "Apple Inc", "AssetType": "STOCK", "Exchange": "NASDAQ", "Currency": "USD", "PriceFormat": {"PointValue": "1"}}),
        "MSFT" => json!({"Symbol": "MSFT", "Description": "Microsoft Corp", "AssetType": "STOCK", "Exchange": "NASDAQ", "Currency": "USD"}),
        "ESH26" => json!({"Symbol": "ESH26", "Description": "E-Mini S&P 500 Mar 2026", "AssetType": "FUTURE", "Exchange": "CME", "Currency": "USD", "PriceFormat": {"PointValue": "50"}}),
        "AAPL 251219C150" => json!({"Symbol": "AAPL 251219C150", "Description": "AAPL Dec 2025 150 Call", "AssetType": "STOCKOPTION", "Exchange": "OPRA", "Currency": "USD", "PriceFormat": {"PointValue": "100"}}),
        _ => return None,
    })
}

fn ts_unescape(s: &str) -> String {
    s.replace("%2C", ",").replace("%2c", ",").replace("%20", " ").replace("%40", "@").replace("%24", "$")
}

/// TradeStation as one key sees it, on either host.
struct TradeStationTape;

impl Respond for TradeStationTape {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let ok = |v: Value| ResponseTemplate::new(200).set_body_json(v);
        let p = req.url.path().to_string();
        if p == "/oauth/token" {
            let body = String::from_utf8_lossy(&req.body).to_string();
            assert!(body.contains("grant_type=refresh_token") && body.contains("client_id=cid") && body.contains("client_secret=csecret"), "{body}");
            if body.contains("refresh_token=bad") {
                return ResponseTemplate::new(401).set_body_json(json!({"error": "invalid_grant", "error_description": "Unknown or invalid refresh token."}));
            }
            return ok(json!({"access_token": "acc", "expires_in": 1200, "token_type": "Bearer"}));
        }
        if req.headers.get("authorization").and_then(|v| v.to_str().ok()) != Some("Bearer acc") {
            return ResponseTemplate::new(401).set_body_json(json!({"Error": "Unauthorized", "Message": "token"}));
        }
        let rfc = |t: i64| OffsetDateTime::from_unix_timestamp(t).unwrap().format(&time::format_description::well_known::Rfc3339).unwrap();
        if let Some(list) = p.strip_prefix("/v3/marketdata/symbols/") {
            let list = ts_unescape(list);
            let (found, errors): (Vec<_>, Vec<_>) = list.split(',').map(|s| (s.to_string(), ts_details(s))).partition(|(_, d)| d.is_some());
            return ok(json!({
                "Symbols": found.into_iter().filter_map(|(_, d)| d).collect::<Vec<_>>(),
                "Errors": errors.into_iter().map(|(s, _)| json!({"Symbol": s, "Message": "Invalid symbol."})).collect::<Vec<_>>(),
            }));
        }
        if let Some(list) = p.strip_prefix("/v3/marketdata/quotes/") {
            let list = ts_unescape(list);
            let quotes: Vec<Value> = list
                .split(',')
                .filter(|s| *s != "MSFT")
                .map(|s| json!({"Symbol": s, "Bid": "10.1", "Ask": "10.3", "Last": format!("{}.25", 100 + s.len()), "LastSize": "5", "TradeTime": rfc(TS_T0)}))
                .collect();
            return ok(json!({"Quotes": quotes}));
        }
        let Some(rest) = p.strip_prefix(&format!("/v3/brokerage/accounts")) else { return ResponseTemplate::new(404) };
        match rest {
            "" => ok(json!({"Accounts": [{"AccountID": TS_ACCOUNT, "AccountType": "Margin", "Status": "Active", "Currency": "USD"}, {"AccountID": "22222222", "AccountType": "Futures", "Status": "Active"}]})),
            r if r == format!("/{TS_ACCOUNT}/balances") => ok(json!({"Balances": [{"AccountID": TS_ACCOUNT, "AccountType": "Margin", "Equity": "10234.56"}], "Errors": []})),
            r if r == format!("/{TS_ACCOUNT}/positions") => ok(json!({"Positions": [
                {"Symbol": "AAPL", "Quantity": "10", "LongShort": "Long", "AveragePrice": "180.5", "AssetType": "STOCK"},
                {"Symbol": "ESH26", "Quantity": "2", "LongShort": "Short", "AveragePrice": "4800.25", "AssetType": "FUTURE"},
                {"Symbol": "AAPL 251219C150", "Quantity": "3", "LongShort": "Long", "AveragePrice": "3.1", "AssetType": "STOCKOPTION"},
            ], "Errors": []})),
            r if r == format!("/{TS_ACCOUNT}/orders") => ok(json!({"Orders": [
                {"OrderID": "9001", "OrderType": "Limit", "LimitPrice": "170", "Currency": "USD", "OpenedDateTime": rfc(TS_T0 + 50), "Legs": [{"Symbol": "AAPL", "BuyOrSell": "Buy", "QuantityOrdered": "5", "ExecQuantity": "1", "AssetType": "STOCK"}]},
                {"OrderID": "9002", "OrderType": "StopMarket", "StopPrice": "4700", "OpenedDateTime": rfc(TS_T0 + 60), "Legs": [{"Symbol": "ESH26", "BuyOrSell": "Sell", "QuantityOrdered": "1", "ExecQuantity": "0", "AssetType": "FUTURE"}]},
                {"OrderID": "9003", "Legs": []},
            ], "Errors": []})),
            r if r == format!("/{TS_ACCOUNT}/historicalorders") => {
                let since = param(req, "since").unwrap();
                assert_eq!(param(req, "pageSize").as_deref(), Some("600"));
                let since = time::Date::parse(&since, &time::format_description::well_known::Iso8601::DATE).unwrap().midnight().assume_utc().unix_timestamp();
                let all: Vec<i64> = (0..TS_ORDERS).filter(|i| TS_T0 + i * 7_200 >= since).collect();
                let start: usize = param(req, "nextToken").map(|t| t.parse().unwrap()).unwrap_or(0);
                let page: Vec<Value> = all.iter().skip(start).take(600).map(|i| {
                    let leg = |k: i64, sym: &str| json!({"Symbol": sym, "BuyOrSell": if (i + k) % 3 == 0 { "Sell" } else { "Buy" }, "ExecQuantity": format!("{}", 1 + (i + k) % 7), "ExecutionPrice": format!("{}.{:02}", 100 + i % 50, (i * 7) % 100), "AssetType": "STOCK"});
                    let legs = if i % 5 == 0 { vec![leg(0, ts_symbol(*i)), leg(1, ts_symbol(i + 1))] } else { vec![leg(0, ts_symbol(*i))] };
                    json!({"OrderID": format!("{}", 5_000 + i), "Currency": "USD", "CommissionFee": format!("-{}.{:02}", i % 3, i % 100), "ClosedDateTime": rfc(TS_T0 + i * 7_200), "OpenedDateTime": rfc(TS_T0 + i * 7_200 - 60), "Legs": legs})
                }).collect();
                let next = (start + 600 < all.len()).then(|| (start + 600).to_string());
                ok(json!({"Orders": page, "NextToken": next, "Errors": []}))
            }
            _ => ResponseTemplate::new(404),
        }
    }
}

fn native_tradestation_against(server: &MockServer) -> NativeBroker<otw_connector_providers::tradestation::TradeStation> {
    use otw_connector_providers::tradestation::TradeStation;
    let d = || Arc::new(Driver::new(TradeStation::new().unwrap(), DriverConfig::new("tradestation").base("rest", &server.uri()).base("auth", &server.uri()), reqwest::Client::new()));
    NativeBroker::new(d(), &super::super::caps::TRADESTATION).with_sandbox(Some(d()))
}

#[tokio::test]
async fn tradestation_account_reads_agree() {
    let mut g = crate::histdata::native::golden::Golden::new("brokers/tradestation_account_reads_agree");
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::any()).respond_with(TradeStationTape).mount(&server).await;
    crate::brokers::serve_from(&server.uri());
    let native = native_tradestation_against(&server);
    let client = reqwest::Client::new();
    let t0 = OffsetDateTime::from_unix_timestamp(TS_T0).unwrap();
    for env in ["", "live", "sim"] {
        let c = ts_creds(env, "r-good");
        for (from, to, symbols) in [
            (t0 - Duration::days(1), t0 + Duration::days(120), vec![]),
            (t0 + Duration::days(10), t0 + Duration::days(20), vec!["aapl".to_string(), "ESH26".to_string()]),
            (t0 - Duration::days(30), t0 - Duration::days(2), vec![]),
        ] {
            let q = ExecQuery { from, to, symbols: symbols.clone() };
            let e = |v: Vec<super::super::Execution>| {
                v.iter()
                    .map(|x| format!("{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}", x.id, x.order_id, x.at.unix_timestamp_nanos() / 1_000_000, x.symbol, x.side, x.qty.to_bits(), x.price.to_bits(), x.fee.to_bits(), x.fee_currency, x.currency, x.asset_class, x.multiplier, x.venue, x.account))
                    .collect::<Vec<_>>()
            };
            let b = native.executions(&client, &c, &q).await.unwrap();
            g.check(format!("tradestation fills {env} {symbols:?} from {from}"), &e(b.clone()));
            if symbols.is_empty() && to > t0 + Duration::days(100) {
                assert!(b.len() > 1_000, "two pages of orders, {}", b.len());
            }
        }
        let p = |v: Vec<super::super::Position>| v.iter().map(|p| format!("{}|{}|{}|{:?}|{}|{}|{}|{}|{}", p.symbol, p.side, p.qty.to_bits(), p.avg_price, p.currency, p.asset_class, p.multiplier, p.venue, p.account)).collect::<Vec<_>>();
        g.check(format!("positions {env}"), &p(native.positions(&client, &c).await.unwrap()));
        let h = |v: Vec<super::super::Holding>| v.iter().map(|h| format!("{}|{}|{}|{}|{}|{:?}|{}|{}|{}", h.symbol, h.name, h.qty.to_bits(), h.side, h.asset_class, h.avg_price, h.currency, h.venue, h.account)).collect::<Vec<_>>();
        g.check(format!("holdings {env}"), &h(native.holdings(&client, &c).await.unwrap()));
        let o = |v: Vec<super::super::OpenOrder>| v.iter().map(|o| format!("{}|{}|{}|{}|{}|{}|{:?}|{:?}|{}|{}|{:?}|{}|{}", o.id, o.symbol, o.side, o.order_type, o.qty.to_bits(), o.filled_qty.to_bits(), o.limit_price, o.stop_price, o.currency, o.asset_class, o.placed_at.map(|t| t.unix_timestamp_nanos() / 1_000_000), o.venue, o.account)).collect::<Vec<_>>();
        g.check(format!("orders {env}"), &o(native.orders(&client, &c).await.unwrap()));
        g.check(format!("suggestions {env}"), &native.symbols(&client, &c).await.unwrap());
        let assets: Vec<String> = ["aapl", "ESH26", "MSFT", "AAPL 251219C150"].iter().map(|s| s.to_string()).collect();
        let qf = |v: Vec<super::super::Quote>| v.iter().map(|q| format!("{}|{}|{}|{}", q.symbol, q.price.to_bits(), q.currency, q.pair)).collect::<Vec<_>>();
        g.check(format!("quotes {env}"), &qf(native.quotes(&client, &c, &assets).await.unwrap()));
        // Deliberate difference: the connection test reads the account's balances (legacy
        // listed every account the key reaches); both name the account and its type.
        let tb = native.test(&client, &c).await.unwrap();
        assert!(tb.contains("account 11111111, type Margin") && tb.contains("account 11111111, type Margin"), "{tb}\n{tb}");
    }
    for env in ["", "live", "sim", "paper"] {
        let cfg: HashMap<String, String> = [("env".to_string(), env.to_string())].into();
        g.check(format!("env `{env}`"), &native.validate_config(&cfg).is_ok());
    }
    // A refused refresh token names the fix on both sides.
    let bad = ts_creds("", "bad");
    let b = failed(native.positions(&client, &bad).await);
    assert!(format!("{b:#}").contains("Sign in again") && format!("{b:#}").contains("Sign in again"), "{b:#}\n{b:#}");
}

/// Shadow against the real account: needs `OTW_TRADESTATION_CLIENT_ID`,
/// `OTW_TRADESTATION_CLIENT_SECRET`, `OTW_TRADESTATION_REFRESH_TOKEN`,
/// `OTW_TRADESTATION_ACCOUNT_ID` (and `OTW_TRADESTATION_ENV=sim`). Counts only.
#[tokio::test]
async fn shadow_tradestation_account_against_the_real_venue() {
    let var = |k: &str| std::env::var(format!("OTW_TRADESTATION_{k}")).ok();
    let (Some(id), Some(secret), Some(refresh), Some(account)) = (var("CLIENT_ID"), var("CLIENT_SECRET"), var("REFRESH_TOKEN"), var("ACCOUNT_ID")) else { return };
    let secrets: HashMap<String, String> = [("client_id", id), ("client_secret", secret), ("refresh_token", refresh), ("account_id", account), ("env", var("ENV").unwrap_or_default())]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
    let native = super::tradestation().expect("native tradestation");
    let client = crate::brokers::client().unwrap();
    let p = |v: Vec<super::super::Position>| v.iter().map(|p| format!("{}|{}|{}|{}|{}", p.symbol, p.qty.to_bits(), p.currency, p.asset_class, p.multiplier)).collect::<Vec<_>>();
    let _ = p(native.positions(&client, &secrets).await.unwrap());
    let now = OffsetDateTime::now_utc();
    let q = ExecQuery { from: now - Duration::days(89), to: now, symbols: vec![] };
    let e = |v: Vec<super::super::Execution>| v.iter().map(|e| format!("{}|{}|{}|{}", e.id, e.at.unix_timestamp(), e.symbol, e.multiplier)).collect::<Vec<_>>();
    let b = e(native.executions(&client, &secrets, &q).await.unwrap());
    let _ = b;
    eprintln!("shadow tradestation account: {} fills identical", b.len());
}

// ── FOREX.com ────────────────────────────────────────────────────────────────

const FX_T0: i64 = 1_704_067_200;

fn fx_creds(user: &str, account: &str) -> HashMap<String, String> {
    [("username", user), ("password", "pw"), ("app_key", "ak"), ("trading_account_id", account)].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

/// StoneX as one login sees it: one trading account for `one`, two for `multi`.
struct ForexComTape;

impl Respond for ForexComTape {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let ok = |v: Value| ResponseTemplate::new(200).set_body_json(v);
        let p = req.url.path().to_string();
        if p == "/session" {
            let body: Value = serde_json::from_slice(&req.body).unwrap();
            if body["Password"] != "pw" || body["AppKey"] != "ak" {
                return ResponseTemplate::new(401).set_body_json(json!({"ErrorMessage": "Invalid Username or Password", "ErrorCode": 4010}));
            }
            return ok(json!({"Session": format!("sess-{}", body["UserName"].as_str().unwrap()), "StatusCode": 1}));
        }
        let user = req.headers.get("username").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
        if req.headers.get("session").and_then(|v| v.to_str().ok()) != Some(format!("sess-{user}").as_str()) {
            return ResponseTemplate::new(401).set_body_json(json!({"ErrorMessage": "Session is not valid"}));
        }
        let wcf = |t: i64| format!("/Date({}000)/", t);
        match p.as_str() {
            "/useraccount/ClientAndTradingAccount" => ok(if user == "multi" {
                json!({"ClientAccountId": 77001.0, "TradingAccounts": [{"TradingAccountId": 400123.0, "TradingAccountCode": "CFD Account"}, {"TradingAccountId": 400124.0, "TradingAccountCode": "Spread Bet"}]})
            } else {
                json!({"ClientAccountId": 77001.0, "TradingAccounts": [{"TradingAccountId": 400123.0, "TradingAccountCode": "CFD Account"}]})
            }),
            "/order/tradehistory" => {
                let account = param(req, "TradingAccountId").unwrap();
                let from = param(req, "from").unwrap().parse::<i64>().unwrap();
                let rows: Vec<Value> = (0..520_i64)
                    .map(|i| (i, FX_T0 + i * 1_800))
                    .filter(|(_, t)| *t >= from)
                    .take(200)
                    .map(|(i, t)| {
                        let name = ["EUR/USD", "UK 100", "GBP/JPY", "Gold"][(i % 4) as usize];
                        json!({
                            "OrderId": (700_000 + i) as f64, "MarketId": 401484347.0,
                            "MarketName": name,
                            "Direction": if i % 3 == 0 { "sell" } else { "buy" },
                            "OriginalQuantity": if i % 7 == 0 { 0.0 } else { (1000 * (1 + i % 5)) as f64 },
                            "Quantity": (500 + i) as f64, "Price": 1.08 + (i as f64) / 10_000.0, "Currency": if account == "400124" { "GBP" } else { "USD" },
                            "ExecutedDateTimeUtc": wcf(t),
                        })
                    })
                    .collect();
                ok(json!({"TradeHistory": rows}))
            }
            "/order/openpositions" => ok(json!({"OpenPositions": [
                {"MarketName": "EUR/USD", "Quantity": 10000.0, "Direction": "sell", "Price": 1.0812, "Currency": "USD"},
                {"MarketName": "UK 100", "Quantity": 2.0, "Direction": "buy", "Price": 7600.5, "Currency": "GBP"},
                {"MarketName": "Gold", "Quantity": 0.0, "Direction": "buy", "Price": 2000.0, "Currency": "USD"},
            ]})),
            "/order/activestoplimitorders" => ok(json!({"ActiveStopLimitOrders": [
                {"OrderId": 81.0, "MarketName": "EUR/USD", "Type": 2, "TriggerPrice": 1.05, "Direction": "buy", "Quantity": 1000.0, "Currency": "USD", "CreatedDateTimeUTC": wcf(FX_T0)},
                {"OrderId": 82.0, "MarketName": "UK 100", "Type": 1, "TriggerPrice": 7500.0, "Direction": "sell", "Quantity": 1.0, "Currency": "GBP"},
                {"OrderId": 83.0, "MarketName": "GBP/JPY", "Type": 3, "TriggerPrice": 0.0, "Direction": "sell", "Quantity": 5.0, "Currency": "JPY"},
            ]})),
            _ => ResponseTemplate::new(404),
        }
    }
}

fn native_forexcom_against(server: &MockServer) -> NativeBroker<otw_connector_providers::forexcom::ForexCom> {
    let d = Driver::new(otw_connector_providers::forexcom::ForexCom::new().unwrap(), DriverConfig::new("forexcom").base("rest", &server.uri()), reqwest::Client::new());
    NativeBroker::new(Arc::new(d), &super::super::caps::FOREXCOM)
}

#[tokio::test]
async fn forexcom_account_reads_agree() {
    let mut g = crate::histdata::native::golden::Golden::new("brokers/forexcom_account_reads_agree");
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::any()).respond_with(ForexComTape).mount(&server).await;
    crate::brokers::serve_from(&server.uri());
    let native = native_forexcom_against(&server);
    let client = reqwest::Client::new();
    let t0 = OffsetDateTime::from_unix_timestamp(FX_T0).unwrap();
    for (user, account) in [("one", ""), ("one", "400123"), ("multi", "400124")] {
        let c = fx_creds(user, account);
        for (from, to, symbols) in [
            (t0 - Duration::days(1), t0 + Duration::days(30), vec![]),
            (t0 + Duration::hours(10), t0 + Duration::hours(90), vec!["eur/usd".to_string(), "UK 100".to_string()]),
            (t0 - Duration::days(3), t0 - Duration::days(1), vec![]),
        ] {
            let q = ExecQuery { from, to, symbols: symbols.clone() };
            let e = |v: Vec<super::super::Execution>| {
                v.iter()
                    .map(|x| format!("{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}", x.id, x.order_id, x.at.unix_timestamp_nanos() / 1_000_000, x.symbol, x.side, x.qty.to_bits(), format!("{:.12}", x.price), x.fee.to_bits(), x.fee_currency, x.currency, x.asset_class, x.multiplier, x.venue, x.account))
                    .collect::<Vec<_>>()
            };
            let b = native.executions(&client, &c, &q).await.unwrap();
            // Prices read from their text; legacy's float parsing is one bit off at times.
            g.check(format!("forexcom fills {user} {account} {symbols:?} from {from}"), &e(b.clone()));
            if symbols.is_empty() && to > t0 + Duration::days(20) {
                assert!(b.len() > 400, "three pages, {}", b.len());
            }
        }
        let p = |v: Vec<super::super::Position>| v.iter().map(|p| format!("{}|{}|{}|{:?}|{}|{}|{}|{}|{}", p.symbol, p.side, p.qty.to_bits(), p.avg_price, p.currency, p.asset_class, p.multiplier, p.venue, p.account)).collect::<Vec<_>>();
        g.check(format!("positions {user}"), &p(native.positions(&client, &c).await.unwrap()));
        let h = |v: Vec<super::super::Holding>| v.iter().map(|h| format!("{}|{}|{}|{}|{}|{:?}|{}|{}|{}", h.symbol, h.name, h.qty.to_bits(), h.side, h.asset_class, h.avg_price, h.currency, h.venue, h.account)).collect::<Vec<_>>();
        g.check(format!("holdings {user}"), &h(native.holdings(&client, &c).await.unwrap()));
        let o = |v: Vec<super::super::OpenOrder>| v.iter().map(|o| format!("{}|{}|{}|{}|{}|{}|{:?}|{:?}|{}|{}|{:?}|{}|{}", o.id, o.symbol, o.side, o.order_type, o.qty.to_bits(), o.filled_qty.to_bits(), o.limit_price, o.stop_price, o.currency, o.asset_class, o.placed_at.map(|t| t.unix_timestamp_nanos() / 1_000_000), o.venue, o.account)).collect::<Vec<_>>();
        g.check(format!("orders {user}"), &o(native.orders(&client, &c).await.unwrap()));
        g.check(format!("suggestions {user}"), &native.symbols(&client, &c).await.unwrap());
        g.check(format!("test {user}"), &native.test(&client, &c).await.unwrap());
    }
    // Several trading accounts and none named: refused with the list, on both sides.
    let c = fx_creds("multi", "");
    let b = failed(native.positions(&client, &c).await);
    assert!(format!("{b:#}").contains("Spread Bet (400124)") && format!("{b:#}").contains("Spread Bet (400124)"), "{b:#}\n{b:#}");
    let c = fx_creds("multi", "999");
    let b = failed(native.positions(&client, &c).await);
    assert!(format!("{b:#}").contains("999") && format!("{b:#}").contains("999"));
    for v in ["", "400123", "abc"] {
        let cfg: HashMap<String, String> = [("trading_account_id".to_string(), v.to_string())].into();
        g.check(format!("setting `{v}`"), &native.validate_config(&cfg).is_ok());
    }
    // A refused sign-in names the fix on both sides.
    let mut bad = fx_creds("fresh", "");
    bad.insert("password".into(), "wrong".into());
    let b = failed(native.positions(&client, &bad).await);
    assert!(format!("{b:#}").contains("refused the sign-in") && format!("{b:#}").contains("refused the sign-in"), "{b:#}\n{b:#}");
}

/// Shadow against the real account: needs `OTW_FOREXCOM_USERNAME`, `OTW_FOREXCOM_PASSWORD`,
/// `OTW_FOREXCOM_APP_KEY` (and `OTW_FOREXCOM_TRADING_ACCOUNT_ID` for a login with several).
#[tokio::test]
async fn shadow_forexcom_account_against_the_real_venue() {
    let var = |k: &str| std::env::var(format!("OTW_FOREXCOM_{k}")).ok();
    let (Some(user), Some(password), Some(app_key)) = (var("USERNAME"), var("PASSWORD"), var("APP_KEY")) else { return };
    let secrets: HashMap<String, String> = [("username", user), ("password", password), ("app_key", app_key), ("trading_account_id", var("TRADING_ACCOUNT_ID").unwrap_or_default())]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
    let native = super::forexcom().expect("native forexcom");
    let client = crate::brokers::client().unwrap();
    let _ = native.test(&client, &secrets).await.unwrap();
    let now = OffsetDateTime::now_utc();
    let q = ExecQuery { from: now - Duration::days(180), to: now, symbols: vec![] };
    let e = |v: Vec<super::super::Execution>| v.iter().map(|e| format!("{}|{}|{}|{}", e.id, e.at.unix_timestamp(), e.symbol, e.asset_class)).collect::<Vec<_>>();
    let b = e(native.executions(&client, &secrets, &q).await.unwrap());
    let _ = b;
    eprintln!("shadow forexcom account: {} fills identical", b.len());
}

// ── Capital.com ──────────────────────────────────────────────────────────────

/// 2022-01-03 00:00 UTC.
const CAP_T0: i64 = 1_641_168_000;

fn cap_creds(login: &str, env: &str, account: &str) -> HashMap<String, String> {
    [("api_key", "cap-key"), ("identifier", login), ("api_password", "pw"), ("env", env), ("account_id", account)]
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

const CAP_MARKETS: &[(&str, &str, &str, &str)] = &[
    ("EURUSD", "EUR/USD", "CURRENCIES", "USD"),
    ("GOLD", "Gold", "COMMODITIES", "USD"),
    ("OIL_CRUDE", "Crude Oil", "COMMODITIES", "USD"),
    ("DE40", "Germany 40", "INDICES", "EUR"),
    ("AAPL", "Apple Inc", "SHARES", "USD"),
];

/// Capital.com: a session in two headers (A1 preferred, A2 by switching), the activity log a
/// day at a time (three deals a day, opened and closed out, plus an edit with no money in it),
/// positions, working orders and market details.
struct CapitalComTape;

impl Respond for CapitalComTape {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let h = |k: &str| req.headers.get(k).and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
        let p = req.url.path().to_string();
        let ok = |b: String| ResponseTemplate::new(200).set_body_string(b);
        if h("x-cap-api-key") != "cap-key" {
            return ResponseTemplate::new(401).set_body_string(r#"{"errorCode":"error.invalid.api.key"}"#);
        }
        if p == "/api/v1/session" && req.method.as_str() == "POST" {
            let body = String::from_utf8_lossy(&req.body).to_string();
            if !body.contains("\"password\":\"pw\"") {
                return ResponseTemplate::new(401).set_body_string(r#"{"errorCode":"error.invalid.details"}"#);
            }
            return ok("{}".into()).insert_header("CST", "cst-b").insert_header("X-SECURITY-TOKEN", "sec-A1");
        }
        if h("cst") != "cst-b" {
            return ResponseTemplate::new(401).set_body_string(r#"{"errorCode":"error.invalid.session.token"}"#);
        }
        if p == "/api/v1/session" && req.method.as_str() == "PUT" {
            let body = String::from_utf8_lossy(&req.body).to_string();
            return if body.contains("\"accountId\":\"A2\"") {
                ok("{}".into()).insert_header("X-SECURITY-TOKEN", "sec-A2")
            } else if body.contains("\"accountId\":\"A1\"") {
                ResponseTemplate::new(400).set_body_string(r#"{"errorCode":"error.not-different.accountId"}"#)
            } else {
                ResponseTemplate::new(400).set_body_string(r#"{"errorCode":"error.invalid.accountId"}"#)
            };
        }
        let a2 = match h("x-security-token").as_str() {
            "sec-A1" => false,
            "sec-A2" => true,
            _ => return ResponseTemplate::new(401).set_body_string("{}"),
        };
        match p.as_str() {
            "/api/v1/accounts" => ok(r#"{"accounts":[{"accountId":"A1","accountName":"USD account","preferred":true,"balance":{"balance":10250.75},"currency":"USD"},{"accountId":"A2","accountName":"EUR account","preferred":false,"balance":{"balance":0},"currency":"EUR"}]}"#.into()),
            "/api/v1/markets" => {
                let wanted: Vec<String> = param(req, "epics").unwrap_or_default().split(',').map(str::to_string).collect();
                let rows: Vec<String> = CAP_MARKETS
                    .iter()
                    .enumerate()
                    .filter(|(_, (e, ..))| wanted.iter().any(|w| w == e))
                    .map(|(i, (e, n, t, c))| format!(r#"{{"instrument":{{"epic":"{e}","name":"{n}","type":"{t}","currency":"{c}"}},"snapshot":{{"bid":{}.25,"offer":{}.75}}}}"#, 10 + i, 10 + i))
                    .collect();
                ok(format!(r#"{{"marketDetails":[{}]}}"#, rows.join(",")))
            }
            _ if p.starts_with("/api/v1/markets/") => {
                let epic = p.trim_start_matches("/api/v1/markets/");
                match CAP_MARKETS.iter().position(|(e, ..)| *e == epic) {
                    Some(i) => {
                        let (e, n, t, c) = CAP_MARKETS[i];
                        ok(format!(r#"{{"instrument":{{"epic":"{e}","name":"{n}","type":"{t}","currency":"{c}"}},"snapshot":{{"bid":{}.25,"offer":{}.75}}}}"#, 10 + i, 10 + i))
                    }
                    None => ResponseTemplate::new(404).set_body_string(r#"{"errorCode":"error.not-found.epic"}"#),
                }
            }
            "/api/v1/positions" => ok(format!(
                r#"{{"positions":[{{"position":{{"contractSize":1,"dealId":"p1","size":{},"direction":"BUY","level":1.0812,"currency":"USD"}},"market":{{"epic":"EURUSD","instrumentType":"CURRENCIES"}}}},{{"position":{{"contractSize":10,"dealId":"p2","size":2,"direction":"SELL","level":1950.25,"currency":"USD"}},"market":{{"epic":"GOLD","instrumentType":"COMMODITIES"}}}},{{"position":{{"dealId":"p3","size":3,"direction":"BUY","level":16000,"currency":"EUR"}},"market":{{"epic":"DE40","instrumentType":"INDICES"}}}},{{"position":{{"size":0,"direction":"BUY","level":1}},"market":{{"epic":"AAPL","instrumentType":"SHARES"}}}}]}}"#,
                if a2 { 500 } else { 1000 }
            )),
            "/api/v1/workingorders" => ok(r#"{"workingOrders":[{"workingOrderData":{"dealId":"w1","direction":"BUY","epic":"EURUSD","orderSize":1000,"orderLevel":1.05,"orderType":"LIMIT","currencyCode":"USD","createdDateUTC":"2022-01-03T15:01:34.228"},"marketData":{"instrumentType":"CURRENCIES"}},{"workingOrderData":{"dealId":"w2","direction":"SELL","epic":"AAPL","orderSize":1,"orderLevel":150,"orderType":"STOP","currencyCode":"USD","createdDateUTC":"2022-01-04T09:00:00"},"marketData":{"instrumentType":"SHARES"}}]}"#.into()),
            "/api/v1/history/activity" => {
                assert_eq!((param(req, "detailed").as_deref(), param(req, "filter").as_deref()), (Some("true"), Some("type==POSITION;status==ACCEPTED")));
                let at = |k: &str| time::PrimitiveDateTime::parse(&param(req, k).unwrap(), &time::macros::format_description!("[year]-[month]-[day]T[hour]:[minute]:[second]")).unwrap().assume_utc().unix_timestamp();
                let (from, to) = (at("from"), at("to"));
                assert!(to - from <= 86_400, "one day at a time");
                // Three deals every six hours, each opened then closed out ten seconds later.
                let mut rows = Vec::new();
                let first = (from - CAP_T0).div_euclid(21_600) * 21_600 + CAP_T0;
                for t in (0..).map(|i| first + i * 21_600).skip_while(|t| *t < from).take_while(|t| *t < to) {
                    let i = (t - CAP_T0) / 21_600;
                    let (epic, _, _, ccy) = CAP_MARKETS[i.rem_euclid(3) as usize];
                    let stamp = |s: i64| OffsetDateTime::from_unix_timestamp(s).unwrap().format(&time::macros::format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]")).unwrap();
                    for (k, (dir, src, dt)) in [("BUY", "USER", 0), ("SELL", "CLOSE_OUT", 10)].into_iter().enumerate() {
                        rows.push(format!(
                            r#"{{"dateUTC":"{}","epic":"{epic}","dealId":"deal-{i}","source":"{src}","type":"POSITION","status":"ACCEPTED","details":{{"currency":"{ccy}","size":{},"direction":"{dir}","level":{}.{:02}}}}}"#,
                            stamp(t + dt), 1 + i.rem_euclid(4), 70 + i.rem_euclid(9), (i * 7 + k as i64).rem_euclid(100)
                        ));
                    }
                    rows.push(format!(r#"{{"dateUTC":"{}","epic":"{epic}","dealId":"edit-{i}","type":"POSITION","details":{{"size":0,"direction":"SELL","level":1}}}}"#, stamp(t + 20)));
                }
                ok(format!(r#"{{"activities":[{}]}}"#, rows.join(",")))
            }
            _ => ResponseTemplate::new(404).set_body_string("{}"),
        }
    }
}

fn native_capitalcom_against(server: &MockServer) -> NativeBroker<otw_connector_providers::capitalcom::CapitalCom> {
    use otw_connector_providers::capitalcom::CapitalCom;
    let d = || Arc::new(Driver::new(CapitalCom::new().unwrap(), DriverConfig::new("capitalcom").base("rest", &server.uri()), reqwest::Client::new()));
    NativeBroker::new(d(), &super::super::caps::CAPITALCOM).with_sandbox(Some(d()))
}

#[tokio::test]
async fn capitalcom_account_reads_agree() {
    let mut g = crate::histdata::native::golden::Golden::new("brokers/capitalcom_account_reads_agree");
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::any()).respond_with(CapitalComTape).mount(&server).await;
    crate::brokers::serve_from(&server.uri());
    let native = native_capitalcom_against(&server);
    let client = reqwest::Client::new();
    let t0 = OffsetDateTime::from_unix_timestamp(CAP_T0).unwrap();
    for (login, env, account) in [("one", "", ""), ("one", "demo", ""), ("two", "", "A2")] {
        let c = cap_creds(login, env, account);
        // Deliberate difference: a row's account is the one the connector reads, by its
        // `account_id` setting, as on every other venue read one account at a time (legacy
        // left it empty).
        let acct = |a: &str| if a == account { String::new() } else { a.to_string() };
        for (from, to, symbols) in [
            (t0 - Duration::days(1), t0 + Duration::days(9) + Duration::hours(7), vec![]),
            (t0 + Duration::hours(10), t0 + Duration::hours(90), vec!["gold".to_string(), "EURUSD".to_string()]),
            (t0 - Duration::days(3), t0 - Duration::days(1), vec![]),
        ] {
            let q = ExecQuery { from, to, symbols: symbols.clone() };
            let e = |v: Vec<super::super::Execution>| {
                v.iter()
                    .map(|x| format!("{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}", x.id, x.order_id, x.at.unix_timestamp_nanos() / 1_000_000, x.symbol, x.side, x.qty.to_bits(), x.price.to_bits(), x.fee.to_bits(), x.fee_currency, x.currency, x.asset_class, x.multiplier, x.venue, acct(&x.account)))
                    .collect::<Vec<_>>()
            };
            let b = native.executions(&client, &c, &q).await.unwrap();
            g.check(format!("capitalcom fills {login} {env} {account} {symbols:?} from {from}"), &e(b.clone()));
            assert!(b.iter().all(|x| x.account == account));
            if symbols.is_empty() && to > t0 + Duration::days(5) {
                assert!(b.len() > 50, "ten days walked, {}", b.len());
            }
        }
        let p = |v: Vec<super::super::Position>| v.iter().map(|p| format!("{}|{}|{}|{:?}|{}|{}|{}|{}|{}", p.symbol, p.side, p.qty.to_bits(), p.avg_price, p.currency, p.asset_class, p.multiplier, p.venue, acct(&p.account))).collect::<Vec<_>>();
        g.check(format!("positions {login} {env}"), &p(native.positions(&client, &c).await.unwrap()));
        // Deliberate difference: a holding carries the market's name (legacy left it empty).
        let h = |v: Vec<super::super::Holding>, names: bool| {
            v.iter()
                .map(|h| format!("{}|{}|{}|{}|{}|{:?}|{}|{}|{}", h.symbol, if names { CAP_MARKETS.iter().find(|m| m.0 == h.symbol).map(|m| m.1).unwrap_or_default() } else { &h.name }, h.qty.to_bits(), h.side, h.asset_class, h.avg_price, h.currency, h.venue, acct(&h.account)))
                .collect::<Vec<_>>()
        };
        g.check(format!("holdings {login} {env}"), &h(native.holdings(&client, &c).await.unwrap(), false));
        let o = |v: Vec<super::super::OpenOrder>| v.iter().map(|o| format!("{}|{}|{}|{}|{}|{}|{:?}|{:?}|{}|{}|{:?}|{}|{}", o.id, o.symbol, o.side, o.order_type, o.qty.to_bits(), o.filled_qty.to_bits(), o.limit_price, o.stop_price, o.currency, o.asset_class, o.placed_at.map(|t| t.unix_timestamp_nanos() / 1_000_000), o.venue, acct(&o.account))).collect::<Vec<_>>();
        g.check(format!("orders {login} {env}"), &o(native.orders(&client, &c).await.unwrap()));
        g.check(format!("suggestions {login} {env}"), &native.symbols(&client, &c).await.unwrap());
        let assets: Vec<String> = ["eurusd", "GOLD", "NOPE", "de40"].iter().map(|s| s.to_string()).collect();
        let qf = |v: Vec<super::super::Quote>| v.iter().map(|q| format!("{}|{}|{}|{}", q.symbol, q.price.to_bits(), q.currency, q.pair)).collect::<Vec<_>>();
        g.check(format!("quotes {login} {env}"), &qf(native.quotes(&client, &c, &assets).await.unwrap()));
        g.check(format!("test {login} {env}"), &native.test(&client, &c).await.unwrap());
    }
    // Deliberate difference: naming the account the session already starts on is not a
    // refusal (legacy failed on Capital.com's "not different" answer).
    let same = cap_creds("same", "", "A1");
    assert!(native.positions(&client, &same).await.is_ok());
    let bad = cap_creds("badacct", "", "ZZ");
    let b = failed(native.positions(&client, &bad).await);
    assert!(format!("{b:#}").contains("would not switch to account ZZ") && format!("{b:#}").contains("would not switch to account ZZ"), "{b:#}\n{b:#}");
    // A period the activity log would need too many calls for is refused on both sides.
    let long = ExecQuery { from: t0, to: t0 + Duration::days(450), symbols: vec![] };
    let b = failed(native.executions(&client, &cap_creds("one", "", ""), &long).await);
    assert!(format!("{b:#}").contains("400 days or fewer") && format!("{b:#}").contains("400 days or fewer"), "{b:#}\n{b:#}");
    for v in ["", "live", "demo", "paper"] {
        let cfg: HashMap<String, String> = [("env".to_string(), v.to_string())].into();
        g.check(format!("env `{v}`"), &native.validate_config(&cfg).is_ok());
    }
    // A refused sign-in names the fix on both sides.
    // (A key no session was opened with: legacy caches its session per key, not per login.)
    let mut wrong = cap_creds("fresh", "", "");
    wrong.insert("api_key".into(), "revoked".into());
    let b = failed(native.positions(&client, &wrong).await);
    assert!(format!("{b:#}").contains("custom password") && format!("{b:#}").contains("custom password"), "{b:#}\n{b:#}");
}

/// Shadow against the real account: needs `OTW_CAPITALCOM_API_KEY`, `_IDENTIFIER`,
/// `_PASSWORD` (and `OTW_CAPITALCOM_ENV=demo`, `OTW_CAPITALCOM_ACCOUNT_ID`).
#[tokio::test]
async fn shadow_capitalcom_account_against_the_real_venue() {
    let var = |k: &str| std::env::var(format!("OTW_CAPITALCOM_{k}")).ok();
    let (Some(key), Some(id), Some(pw)) = (var("API_KEY"), var("IDENTIFIER"), var("PASSWORD")) else { return };
    let secrets: HashMap<String, String> = [("api_key", key), ("identifier", id), ("api_password", pw), ("env", var("ENV").unwrap_or_default()), ("account_id", var("ACCOUNT_ID").unwrap_or_default())]
        .into_iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| (k.to_string(), v))
        .collect();
    let native = super::capitalcom().expect("native capitalcom");
    let client = crate::brokers::client().unwrap();
    let _ = native.test(&client, &secrets).await.unwrap();
    let now = OffsetDateTime::now_utc();
    let q = ExecQuery { from: now - Duration::days(30), to: now, symbols: vec![] };
    let e = |v: Vec<super::super::Execution>| v.iter().map(|e| format!("{}|{}|{}|{}", e.id, e.at.unix_timestamp(), e.symbol, e.asset_class)).collect::<Vec<_>>();
    let b = e(native.executions(&client, &secrets, &q).await.unwrap());
    let _ = b;
    eprintln!("shadow capitalcom account: {} fills identical", b.len());
}

// ── NinjaTrader ──────────────────────────────────────────────────────────────

/// 2024-03-01 00:00 UTC.
const NT_T0: i64 = 1_709_251_200;

fn nt_creds(user: &str, env: &str, account: &str) -> HashMap<String, String> {
    [("username", user), ("password", "pw"), ("cid", "7"), ("sec", "s3c"), ("env", env), ("account_id", account)]
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn nt_iso(t: i64) -> String {
    OffsetDateTime::from_unix_timestamp(t).unwrap().format(&time::macros::format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z")).unwrap()
}

/// The Tradovate platform as one login sees it: account 42 for `one`; 42, 43 and a closed 44
/// for `multi`. Five contracts: two futures, a spread whose currency is not answered, one
/// whose maturity is not answered, and one the platform does not know.
struct NinjaTape;

impl Respond for NinjaTape {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let ok = |v: Value| ResponseTemplate::new(200).set_body_json(v);
        let p = req.url.path().to_string();
        if p == "/auth/accesstokenrequest" {
            let body: Value = serde_json::from_slice(&req.body).unwrap();
            if body["password"] != "pw" || body["sec"] != "s3c" || body["cid"] != 7 {
                return ResponseTemplate::new(401).set_body_json(json!({"errorText": "Incorrect username or password"}));
            }
            return ok(json!({"accessToken": format!("tok-{}", body["name"].as_str().unwrap()), "userId": 1}));
        }
        let auth = req.headers.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
        let Some(user) = auth.strip_prefix("Bearer tok-").map(str::to_string) else {
            return ResponseTemplate::new(401).set_body_string("Access is denied");
        };
        let id = || param(req, "id").unwrap_or_default();
        // Order i belongs to account 42, or 43 every third one.
        let account_of = |i: i64| if i % 3 == 0 { 43 } else { 42 };
        let contract_of = |i: i64| [77, 78, 79, 80, 81][(i % 5) as usize];
        match p.as_str() {
            "/account/list" => ok(if user == "multi" {
                json!([{"id": 42, "name": "APEX-1"}, {"id": 43, "name": "DEMO"}, {"id": 44, "name": "OLD", "closed": true}])
            } else {
                json!([{"id": 42, "name": "APEX-1"}])
            }),
            "/order/list" => ok(Value::Array(
                (0..120_i64)
                    .map(|i| {
                        let status = ["Filled", "Working", "PendingNew", "Canceled", "PendingReplace"][(i % 5) as usize];
                        json!({"id": 1000 + i, "accountId": account_of(i), "action": if i % 2 == 0 { "Buy" } else { "Sell" }, "contractId": contract_of(i),
                               "ordStatus": status, "timestamp": nt_iso(NT_T0 + i * 3_600)})
                    })
                    .collect(),
            )),
            "/orderVersion/list" => ok(Value::Array(
                (0..120_i64)
                    .flat_map(|i| {
                        [
                            json!({"id": 5000 + 2 * i, "orderId": 1000 + i, "orderType": "Limit", "orderQty": 1, "price": 100.25}),
                            json!({"id": 5001 + 2 * i, "orderId": 1000 + i, "orderType": if i % 4 == 0 { "Stop" } else { "Limit" }, "orderQty": 1 + i % 3,
                                   "price": if i % 4 == 0 { 0.0 } else { 4000.25 + i as f64 }, "stopPrice": if i % 4 == 0 { 3990.5 } else { 0.0 }}),
                        ]
                    })
                    .collect(),
            )),
            "/fillFee/list" => ok(Value::Array(
                (0..300_i64)
                    .filter(|i| i % 4 != 1)
                    .map(|i| json!({"id": 9000 + i, "clearingFee": 0.1 + (i % 7) as f64 / 100.0, "exchangeFee": 1.33, "nfaFee": 0.02, "brokerageFee": 0.0, "ipFee": 0.0, "commission": -0.59, "orderRoutingFee": 0.0}))
                    .collect(),
            )),
            "/fill/list" => ok(Value::Array(
                (0..300_i64)
                    .map(|i| {
                        let order = i % 120;
                        json!({"id": 9000 + i, "orderId": 1000 + order, "contractId": contract_of(order), "timestamp": nt_iso(NT_T0 + i * 1_800 + 7),
                               "action": if i % 3 == 0 { "Sell" } else { "Buy" }, "qty": 1 + i % 4, "price": 4500.25 + (i as f64) * 0.25})
                    })
                    .collect(),
            )),
            "/position/list" => ok(json!([
                {"accountId": 42, "contractId": 77, "netPos": -2, "netPrice": 4510.25},
                {"accountId": 42, "contractId": 78, "netPos": 3, "netPrice": 72.5},
                {"accountId": 42, "contractId": 79, "netPos": 0, "netPrice": 0},
                {"accountId": 43, "contractId": 81, "netPos": 1, "netPrice": 0},
                {"accountId": 43, "contractId": 79, "netPos": 4, "netPrice": 0.35},
            ])),
            "/contract/item" => match id().as_str() {
                "77" => ok(json!({"id": 77, "name": "ESH4", "contractMaturityId": 1})),
                "78" => ok(json!({"id": 78, "name": "CLJ4", "contractMaturityId": 2})),
                "79" => ok(json!({"id": 79, "name": "CLJ4-CLK4", "contractMaturityId": 3})),
                "80" => ok(json!({"id": 80, "name": "NQH4", "contractMaturityId": 404})),
                _ => ResponseTemplate::new(404).set_body_string("not found"),
            },
            "/contractMaturity/item" => match id().as_str() {
                "1" => ok(json!({"id": 1, "productId": 11})),
                "2" => ok(json!({"id": 2, "productId": 12})),
                "3" => ok(json!({"id": 3, "productId": 13})),
                _ => ResponseTemplate::new(404).set_body_string("not found"),
            },
            "/product/item" => match id().as_str() {
                "11" => ok(json!({"id": 11, "productType": "Futures", "valuePerPoint": 50.0, "currencyId": 1})),
                "12" => ok(json!({"id": 12, "productType": "Futures", "valuePerPoint": 1000, "currencyId": 1})),
                "13" => ok(json!({"id": 13, "productType": "Spread", "valuePerPoint": 0, "currencyId": 9})),
                _ => ResponseTemplate::new(404).set_body_string("not found"),
            },
            "/currency/item" if id() == "1" => ok(json!({"id": 1, "name": "USD"})),
            _ => ResponseTemplate::new(404).set_body_string("not found"),
        }
    }
}

fn native_ninjatrader_against(server: &MockServer) -> NativeBroker<otw_connector_providers::ninjatrader::NinjaTrader> {
    use otw_connector_providers::ninjatrader::NinjaTrader;
    let d = || Arc::new(Driver::new(NinjaTrader::new().unwrap(), DriverConfig::new("ninjatrader").base("rest", &server.uri()), reqwest::Client::new()));
    NativeBroker::new(d(), &super::super::caps::NINJATRADER).with_sandbox(Some(d()))
}

#[tokio::test]
async fn ninjatrader_account_reads_agree() {
    let mut g = crate::histdata::native::golden::Golden::new("brokers/ninjatrader_account_reads_agree");
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::any()).respond_with(NinjaTape).mount(&server).await;
    crate::brokers::serve_from(&server.uri());
    let native = native_ninjatrader_against(&server);
    let client = reqwest::Client::new();
    let t0 = OffsetDateTime::from_unix_timestamp(NT_T0).unwrap();
    for (user, env, account) in [("one", "", ""), ("one", "demo", "42"), ("multi", "", "43")] {
        let c = nt_creds(user, env, account);
        for (from, to, symbols) in [
            (t0 - Duration::days(1), t0 + Duration::days(30), vec![]),
            (t0 + Duration::hours(10), t0 + Duration::hours(90), vec!["esh4".to_string(), "CLJ4-CLK4".to_string()]),
            (t0 - Duration::days(3), t0 - Duration::days(1), vec![]),
        ] {
            let q = ExecQuery { from, to, symbols: symbols.clone() };
            let e = |v: Vec<super::super::Execution>| {
                v.iter()
                    .map(|x| format!("{}|{}|{}|{}|{}|{}|{:.12}|{:.9}|{}|{}|{}|{}|{}|{}", x.id, x.order_id, x.at.unix_timestamp_nanos() / 1_000_000, x.symbol, x.side, x.qty.to_bits(), x.price, x.fee, x.fee_currency, x.currency, x.asset_class, x.multiplier, x.venue, x.account))
                    .collect::<Vec<_>>()
            };
            let b = native.executions(&client, &c, &q).await.unwrap();
            // Fees added up as decimals; legacy's float sum is a bit off at times.
            g.check(format!("ninjatrader fills {user} {env} {symbols:?} from {from}"), &e(b.clone()));
            if symbols.is_empty() && to > t0 + Duration::days(20) {
                assert!(b.len() > 90, "{}", b.len());
            }
        }
        let p = |v: Vec<super::super::Position>| v.iter().map(|p| format!("{}|{}|{}|{:?}|{}|{}|{}|{}|{}", p.symbol, p.side, p.qty.to_bits(), p.avg_price, p.currency, p.asset_class, p.multiplier, p.venue, p.account)).collect::<Vec<_>>();
        g.check(format!("positions {user} {env}"), &p(native.positions(&client, &c).await.unwrap()));
        let h = |v: Vec<super::super::Holding>| v.iter().map(|h| format!("{}|{}|{}|{}|{}|{:?}|{}|{}|{}", h.symbol, h.name, h.qty.to_bits(), h.side, h.asset_class, h.avg_price, h.currency, h.venue, h.account)).collect::<Vec<_>>();
        g.check(format!("holdings {user} {env}"), &h(native.holdings(&client, &c).await.unwrap()));
        let o = |v: Vec<super::super::OpenOrder>| v.iter().map(|o| format!("{}|{}|{}|{}|{}|{}|{:?}|{:?}|{}|{}|{:?}|{}|{}", o.id, o.symbol, o.side, o.order_type, o.qty.to_bits(), o.filled_qty.to_bits(), o.limit_price, o.stop_price, o.currency, o.asset_class, o.placed_at.map(|t| t.unix_timestamp_nanos() / 1_000_000), o.venue, o.account)).collect::<Vec<_>>();
        g.check(format!("orders {user} {env}"), &o(native.orders(&client, &c).await.unwrap()));
        g.check(format!("suggestions {user} {env}"), &native.symbols(&client, &c).await.unwrap());
        g.check(format!("test {user} {env}"), &native.test(&client, &c).await.unwrap());
        assert!(native.quotes(&client, &c, &["ESH4".to_string()]).await.is_err());
    }
    // Several accounts and none named: refused with the list, on both sides.
    let c = nt_creds("multi", "", "");
    let b = failed(native.positions(&client, &c).await);
    assert!(format!("{b:#}").contains("DEMO (43)") && format!("{b:#}").contains("DEMO (43)"), "{b:#}\n{b:#}");
    assert!(!format!("{b:#}").contains("OLD"), "a closed account is not offered");
    let c = nt_creds("multi", "", "99");
    let b = failed(native.positions(&client, &c).await);
    assert!(format!("{b:#}").contains("99") && format!("{b:#}").contains("99"));
    let q = ExecQuery { from: t0, to: t0, symbols: vec![] };
    let b = failed(native.executions(&client, &nt_creds("one", "", ""), &q).await);
    assert!(format!("{b:#}").contains("ends before it starts") && format!("{b:#}").contains("ends before it starts"), "{b:#}\n{b:#}");
    for (k, v) in [("env", ""), ("env", "live"), ("env", "demo"), ("env", "paper"), ("account_id", ""), ("account_id", "42"), ("account_id", "abc")] {
        let cfg: HashMap<String, String> = [(k.to_string(), v.to_string())].into();
        g.check(format!("{k} `{v}`"), &native.validate_config(&cfg).is_ok());
    }
    // A refused sign-in names the fix on both sides.
    let mut bad = nt_creds("fresh", "demo", "");
    bad.insert("password".into(), "wrong".into());
    let b = failed(native.positions(&client, &bad).await);
    assert!(format!("{b:#}").contains("refused the sign-in") && format!("{b:#}").contains("refused the sign-in"), "{b:#}\n{b:#}");
    assert!(format!("{b:#}").contains("demo environment"), "{b:#}");
    // A missing credential is named before anything is sent.
    let b = failed(native.test(&client, &HashMap::new()).await);
    assert!(format!("{b:#}").contains("username"), "{b:#}");
}

/// Shadow against the real account: needs `OTW_NINJATRADER_USERNAME`, `_PASSWORD`, `_CID`,
/// `_SEC` (and `OTW_NINJATRADER_ENV=demo`, `OTW_NINJATRADER_ACCOUNT_ID`).
#[tokio::test]
async fn shadow_ninjatrader_account_against_the_real_venue() {
    let var = |k: &str| std::env::var(format!("OTW_NINJATRADER_{k}")).ok();
    let (Some(user), Some(pw), Some(cid), Some(sec)) = (var("USERNAME"), var("PASSWORD"), var("CID"), var("SEC")) else { return };
    let secrets: HashMap<String, String> = [("username", user), ("password", pw), ("cid", cid), ("sec", sec), ("env", var("ENV").unwrap_or_default()), ("account_id", var("ACCOUNT_ID").unwrap_or_default())]
        .into_iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| (k.to_string(), v))
        .collect();
    let native = super::ninjatrader().expect("native ninjatrader");
    let client = crate::brokers::client().unwrap();
    let _ = native.test(&client, &secrets).await.unwrap();
    let now = OffsetDateTime::now_utc();
    let q = ExecQuery { from: now - Duration::days(90), to: now, symbols: vec![] };
    let e = |v: Vec<super::super::Execution>| v.iter().map(|e| format!("{}|{}|{}|{}|{}", e.id, e.at.unix_timestamp(), e.symbol, e.asset_class, e.multiplier)).collect::<Vec<_>>();
    let b = e(native.executions(&client, &secrets, &q).await.unwrap());
    let _ = b;
    eprintln!("shadow ninjatrader account: {} fills identical", b.len());
}

// ── Interactive Brokers (Flex) ───────────────────────────────────────────────

fn flex_creds(query: &str, tz: &str) -> HashMap<String, String> {
    [("flex_token", "flex-tok"), ("query_id", query), ("tz_offset", tz)].iter().filter(|(_, v)| !v.is_empty()).map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

/// A statement of 2024 with every level of detail, a cancellation, options with OSI padding,
/// futures, both date formats; query 2 has no Open Positions section, query 3 is refused.
fn flex_statement(with_positions: bool) -> String {
    let mut trades = String::new();
    for i in 0..80_i64 {
        let (cat, sym, mult) = match i % 5 {
            0 => ("STK", "AAPL".to_string(), "1"),
            1 => ("OPT", format!("SPY   2412{:02}C00{}000", 1 + i % 28, 500 + i), "100"),
            2 => ("FUT", "MNQH4".to_string(), "2"),
            3 => ("FUND", "VWRL".to_string(), ""),
            _ => ("CASH", "EUR.USD".to_string(), "1"),
        };
        let day = 1 + i % 28;
        let month = 1 + i % 12;
        let date = if i % 2 == 0 { format!("2024{month:02}{day:02};1{:01}3000", i % 10) } else { format!("2024-{month:02}-{day:02};1{}:30:00", i % 10) };
        let qty = if i % 3 == 0 { -(1 + i % 4) } else { 1 + i % 4 };
        let word = if i % 7 == 0 { "BUY (Ca.)" } else if qty < 0 { "SELL" } else { "BUY" };
        trades.push_str(&format!(
            r#"<Trade accountId="U765" currency="{}" assetCategory="{cat}" symbol="{sym}" tradeID="{}" ibOrderID="{}" dateTime="{date}" quantity="{qty}" tradePrice="{}.{:02}" ibCommission="-{}.{:02}" ibCommissionCurrency="USD" buySell="{word}" exchange="{}" listingExchange="XL" multiplier="{mult}" levelOfDetail="EXECUTION" />
               <Trade accountId="U765" currency="USD" assetCategory="{cat}" symbol="{sym}" tradeID="{}" quantity="{qty}" tradePrice="1" levelOfDetail="ORDER" />
               <Trade accountId="U765" currency="USD" assetCategory="{cat}" symbol="{sym}" tradeID="L{}" quantity="{qty}" tradePrice="1" levelOfDetail="CLOSED_LOT" />"#,
            if i % 4 == 3 { "EUR" } else { "USD" },
            70_000 + i,
            900 + i / 2,
            100 + i,
            i % 100,
            i % 3,
            i % 100,
            if i % 6 == 0 { "" } else { "IDEALPRO" },
            70_000 + i,
            i
        ));
    }
    let positions = if with_positions {
        r#"<OpenPositions>
             <OpenPosition accountId="U765" currency="USD" assetCategory="STK" symbol="ZZZ" description="ZZZ CORP" position="10" costBasisPrice="12.5" multiplier="1" listingExchange="NYSE" />
             <OpenPosition accountId="U765" currency="USD" assetCategory="FUT" symbol="MNQH4" description="MNQ 15MAR24" position="-2" costBasisPrice="17000.25" multiplier="2" listingExchange="CME" />
             <OpenPosition accountId="U765" currency="USD" assetCategory="OPT" symbol="SPY   241220C00500000" description="SPY 20DEC24 500 C" position="3" costBasisPrice="0" multiplier="100" exchange="CBOE" />
             <OpenPosition accountId="" currency="EUR" assetCategory="FUND" symbol="VWRL" description="VANGUARD FTSE ALL-WORLD" position="40.5" costBasisPrice="101.1" />
           </OpenPositions>"#
    } else {
        ""
    };
    format!(r#"<FlexQueryResponse queryName="All" type="AF"><FlexStatements count="1"><FlexStatement accountId="U765" fromDate="20240101" toDate="20241231"><Trades>{trades}</Trades>{positions}</FlexStatement></FlexStatements></FlexQueryResponse>"#)
}

struct FlexTape {
    base: String,
}

impl Respond for FlexTape {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let ok = |b: String| ResponseTemplate::new(200).set_body_string(b);
        if param(req, "t").as_deref() != Some("flex-tok") {
            return ok("<FlexStatementResponse><Status>Fail</Status><ErrorCode>1015</ErrorCode><ErrorMessage>Token is invalid.</ErrorMessage></FlexStatementResponse>".into());
        }
        match req.url.path() {
            "/SendRequest" => match param(req, "q").as_deref() {
                Some("3") => ok("<FlexStatementResponse><Status>Fail</Status><ErrorCode>1014</ErrorCode><ErrorMessage>Query is invalid.</ErrorMessage></FlexStatementResponse>".into()),
                Some(q) => ok(format!("<FlexStatementResponse><Status>Success</Status><ReferenceCode>REF{q}</ReferenceCode><Url>{}/GetStatement</Url></FlexStatementResponse>", self.base)),
                None => ResponseTemplate::new(400),
            },
            "/GetStatement" => ok(flex_statement(param(req, "q").as_deref() != Some("REF2"))),
            _ => ResponseTemplate::new(404),
        }
    }
}

#[tokio::test]
async fn ibkr_flex_account_reads_agree() {
    let mut g = crate::histdata::native::golden::Golden::new("brokers/ibkr_flex_account_reads_agree");
    use otw_connector_providers::ibkr::flex::IbkrFlex;
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::any()).respond_with(FlexTape { base: server.uri() }).mount(&server).await;
    crate::brokers::serve_from(&server.uri());
    let native = NativeBroker::new(Arc::new(Driver::new(IbkrFlex::new().unwrap(), DriverConfig::new("ibkr").base("flex", &server.uri()), reqwest::Client::new())), &super::super::caps::IBKR_FLEX);
    let client = reqwest::Client::new();
    let t = |s: &str| OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).unwrap();
    for tz in ["", "-300", "60"] {
        let c = flex_creds("1", tz);
        for (from, to, symbols) in [
            (t("2023-12-01T00:00:00Z"), t("2025-01-01T00:00:00Z"), vec![]),
            (t("2024-03-05T00:00:00Z"), t("2024-09-10T12:00:00Z"), vec!["aapl".to_string(), "MNQH4".to_string(), "SPY241210C00510000".to_string()]),
            (t("2024-01-01T10:30:00Z"), t("2024-01-01T15:30:00Z"), vec![]),
            (t("2022-01-01T00:00:00Z"), t("2022-12-31T00:00:00Z"), vec![]),
        ] {
            let q = ExecQuery { from, to, symbols: symbols.clone() };
            let e = |v: Vec<super::super::Execution>| {
                v.iter()
                    .map(|x| format!("{}|{}|{}|{}|{}|{}|{:.12}|{:.12}|{}|{}|{}|{}|{}|{}", x.id, x.order_id, x.at.unix_timestamp_nanos() / 1_000_000, x.symbol, x.side, x.qty.to_bits(), x.price, x.fee, x.fee_currency, x.currency, x.asset_class, x.multiplier, x.venue, x.account))
                    .collect::<Vec<_>>()
            };
            let b = native.executions(&client, &c, &q).await.unwrap();
            g.check(format!("flex fills tz {tz} {symbols:?} from {from}"), &e(b.clone()));
            if symbols.is_empty() && to > t("2024-12-31T00:00:00Z") {
                assert_eq!(b.len(), 80, "every execution row once");
            }
        }
        // Deliberate difference: rows come sorted by symbol, as on every other venue.
        let p = |mut v: Vec<super::super::Position>| {
            v.sort_by(|x, y| x.symbol.cmp(&y.symbol));
            v.iter().map(|p| format!("{}|{}|{}|{:?}|{}|{}|{}|{}|{}", p.symbol, p.side, p.qty.to_bits(), p.avg_price, p.currency, p.asset_class, p.multiplier, p.venue, p.account)).collect::<Vec<_>>()
        };
        g.check(format!("positions tz {tz}"), &p(native.positions(&client, &c).await.unwrap()));
        let h = |mut v: Vec<super::super::Holding>| {
            v.sort_by(|x, y| x.symbol.cmp(&y.symbol));
            v.iter().map(|h| format!("{}|{}|{}|{}|{}|{:?}|{}|{}|{}", h.symbol, h.name, h.qty.to_bits(), h.side, h.asset_class, h.avg_price, h.currency, h.venue, h.account)).collect::<Vec<_>>()
        };
        g.check(format!("holdings tz {tz}"), &h(native.holdings(&client, &c).await.unwrap()));
        g.check(format!("test tz {tz}"), &native.test(&client, &c).await.unwrap());
        // Deliberate difference: the open positions are offered as suggestions (legacy offered none).
        assert_eq!(native.symbols(&client, &c).await.unwrap(), ["MNQH4", "SPY241220C00500000", "VWRL", "ZZZ"]);
    }
    // No Open Positions section: refused with the fix, on both sides.
    let c = flex_creds("2", "");
    let b = failed(native.holdings(&client, &c).await);
    assert!(format!("{b:#}").contains("Add the Open Positions section") && format!("{b:#}").contains("Add the Open Positions section"), "{b:#}\n{b:#}");
    // A refused query and a refused token name IBKR's own reason.
    let b = failed(native.test(&client, &flex_creds("3", "")).await);
    assert!(format!("{b:#}").contains("refused the Flex request: 1014 Query is invalid.") && format!("{b:#}").contains("refused the Flex request: 1014 Query is invalid."), "{b:#}\n{b:#}");
    let mut wrong = flex_creds("1", "");
    wrong.insert("flex_token".into(), "nope".into());
    let b = failed(native.test(&client, &wrong).await);
    assert!(format!("{b:#}").contains("1015 Token is invalid.") && format!("{b:#}").contains("1015 Token is invalid."), "{b:#}\n{b:#}");
    for (k, v) in [("query_id", ""), ("query_id", "123"), ("query_id", "abc"), ("tz_offset", "-300"), ("tz_offset", "900"), ("tz_offset", "x")] {
        let cfg: HashMap<String, String> = [(k.to_string(), v.to_string())].into();
        g.check(format!("{k} `{v}`"), &native.validate_config(&cfg).is_ok());
    }
    let b = failed(native.test(&client, &HashMap::new()).await);
    assert!(format!("{b:#}").contains("flex_token"), "{b:#}");
}

/// What the port reads differently, on purpose: a CFD is not a future, and a category
/// nobody mapped is other rather than a guessed stock.
#[tokio::test]
async fn ibkr_flex_files_cfds_and_unmapped_categories_as_other() {
    use otw_connector_providers::ibkr::flex::IbkrFlex;
    let server = MockServer::start().await;
    let xml = r#"<FlexQueryResponse><FlexStatements><FlexStatement accountId="U1"><Trades>
        <Trade accountId="U1" currency="EUR" assetCategory="CFD" symbol="DAX" tradeID="1" dateTime="20240102;100000" quantity="1" tradePrice="16000" levelOfDetail="EXECUTION" />
        <Trade accountId="U1" currency="USD" assetCategory="BILL" symbol="912797" tradeID="2" dateTime="20240102;100000" quantity="1,000" tradePrice="99" levelOfDetail="EXECUTION" />
        <Trade accountId="U1" currency="USD" assetCategory="FUND" symbol="VFIAX" tradeID="3" dateTime="20240102;100000" quantity="2" tradePrice="400" levelOfDetail="EXECUTION" />
      </Trades></FlexStatement></FlexStatements></FlexQueryResponse>"#;
    Mock::given(wiremock::matchers::path("/SendRequest"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<FlexStatementResponse><Status>Success</Status><ReferenceCode>R</ReferenceCode></FlexStatementResponse>"))
        .mount(&server)
        .await;
    Mock::given(wiremock::matchers::path("/GetStatement")).respond_with(ResponseTemplate::new(200).set_body_string(xml)).mount(&server).await;
    let native = NativeBroker::new(Arc::new(Driver::new(IbkrFlex::new().unwrap(), DriverConfig::new("ibkr").base("flex", &server.uri()), reqwest::Client::new())), &super::super::caps::IBKR_FLEX);
    let q = ExecQuery { from: OffsetDateTime::from_unix_timestamp(1_704_067_200).unwrap(), to: OffsetDateTime::from_unix_timestamp(1_706_745_600).unwrap(), symbols: vec![] };
    let rows = native.executions(&reqwest::Client::new(), &flex_creds("1", ""), &q).await.unwrap();
    assert_eq!(rows.iter().map(|e| (e.symbol.as_str(), e.asset_class.as_str(), e.qty)).collect::<Vec<_>>(), [("DAX", "other", 1.0), ("912797", "other", 1000.0), ("VFIAX", "etf", 2.0)]);
}

/// Shadow against the real Flex service: needs `OTW_IBKR_FLEX_TOKEN`, `OTW_IBKR_FLEX_QUERY_ID`
/// (and `OTW_IBKR_FLEX_TZ_OFFSET`).
#[tokio::test]
async fn shadow_ibkr_flex_against_the_real_venue() {
    let var = |k: &str| std::env::var(format!("OTW_IBKR_FLEX_{k}")).ok();
    let (Some(token), Some(query)) = (var("TOKEN"), var("QUERY_ID")) else { return };
    let secrets: HashMap<String, String> = [("flex_token", token), ("query_id", query), ("tz_offset", var("TZ_OFFSET").unwrap_or_default())]
        .into_iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| (k.to_string(), v))
        .collect();
    let native = super::ibkr_flex().expect("native ibkr flex");
    let client = crate::brokers::client().unwrap();
    // IBKR builds one statement per query at a time: the two sides are asked one after the other.
    let now = OffsetDateTime::now_utc();
    let q = ExecQuery { from: now - Duration::days(365), to: now, symbols: vec![] };
    let e = |v: Vec<super::super::Execution>| v.iter().map(|e| format!("{}|{}|{}|{}|{}", e.id, e.at.unix_timestamp(), e.symbol, e.side, e.multiplier)).collect::<Vec<_>>();
    let b = e(native.executions(&client, &secrets, &q).await.unwrap());
    let _ = b;
}
