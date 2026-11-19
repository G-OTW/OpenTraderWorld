//! Kraken Futures, offline. Fixtures under `fixtures/` are the OpenAPI examples of
//! https://docs.kraken.com/openapi/futures-rest.yaml (served as the API reference at
//! https://docs.kraken.com/api-reference/), converted from YAML to JSON, values untouched:
//!   - `doc_instruments.json`: `GET /instruments` example (api-reference/instrument-details/get-instruments)
//!   - `doc_accounts.json`: `GET /accounts` example (api-reference/account-information/get-wallets)
//!   - `doc_openpositions.json`: `GET /openpositions` example (api-reference/account-information/get-open-positions)
//!   - `doc_openorders.json`: `GET /openorders` example (api-reference/order-management/get-open-orders)
//!   - `doc_fills.json`, `doc_leveragepreferences.json`: kept for the order step.
//!
//! Inline bodies say where they come from: "doc" (quoted from a page) or "synthetic"
//! (written from the documented schema, no example exists).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use otw_connector_model::{AccountItems, AccountKind, AssetClass, Command, CorrId, ErrorKind, Event, InstrumentId, TimeRange};
use otw_connector_sdk::{InstrumentCache, Method, NativeRequest};
use otw_connector_testkit::{assert_undeclared_is_unsupported, decode_once, encode_once, http_frame, json, ts};

use super::*;

const INSTRUMENTS: &str = include_str!("fixtures/doc_instruments.json");
const ACCOUNTS: &str = include_str!("fixtures/doc_accounts.json");
const POSITIONS: &str = include_str!("fixtures/doc_openpositions.json");
const ORDERS: &str = include_str!("fixtures/doc_openorders.json");

/// Doc: the `GET /tickers` example (api-reference/market-data/get-tickers), trimmed to its
/// first two rows.
const TICKERS: &str = r#"{"result":"success","tickers":[
  {"tag":"perpetual","pair":"XBT:USD","symbol":"PI_XBTUSD","markPrice":30209.9,"bid":8634,"bidSize":1000,"ask":49289,"askSize":139984,"vol24h":15304,"volumeQuote":7305.2,"openInterest":149655,"open24h":49289,"indexPrice":21087.8,"last":49289,"lastTime":"2022-06-17T10:46:35.705Z","lastSize":100,"suspended":false,"fundingRate":1.18588737106e-07,"fundingRatePrediction":1.1852486794e-07,"postOnly":false,"change24h":1.9974017538161748},
  {"symbol":"in_xbtusd","last":21088,"lastTime":"2022-06-17T11:00:30.000Z"}],
  "serverTime":"2022-06-17T11:00:31.335Z"}"#;

/// Synthetic: the expired list, holding the two contracts the positions example names and
/// the current list lacks (shapes as the live `?expired=true` answer, 2026-11-12).
const EXPIRED: &str = r#"{"result":"success","instruments":[
  {"symbol":"FI_XBTUSD_201225","type":"futures_inverse","base":"BTC","quote":"USD","pair":"BTC:USD","contractSize":1,"tickSize":0.5,"contractValueTradePrecision":0,"lastTradingTime":"2020-12-25T16:00:00Z","isExpired":true,"suspended":true,"tradeable":true},
  {"symbol":"PF_DEFIUSD","type":"flexible_futures","base":"DEFI","quote":"USD","pair":"DEFI:USD","contractSize":1,"tickSize":0.0001,"contractValueTradePrecision":0,"isExpired":true,"suspended":true,"tradeable":true}],
  "serverTime":"2026-11-12T10:18:59.375Z"}"#;

fn m() -> KrakenFutures {
    KrakenFutures::new().unwrap()
}

fn query(req_id: u64, kind: AccountKind, cursor: Option<String>) -> Command {
    Command::QueryAccount {
        req_id: CorrId(req_id),
        kind,
        symbols: vec![],
        range: Some(TimeRange::new(ts(1_700_000_000), ts(1_700_086_400))),
        cursor,
        account: None,
    }
}

fn http(reqs: &[NativeRequest]) -> Vec<&otw_connector_sdk::HttpRequest> {
    reqs.iter()
        .map(|r| match r {
            NativeRequest::Http(h) => h,
            other => panic!("not http: {other:?}"),
        })
        .collect()
}

fn items(evs: &[otw_connector_model::Envelope]) -> (AccountItems, Option<String>) {
    match &evs[0].event {
        Event::AccountChunk { items, next_cursor, .. } => (items.clone(), next_cursor.clone()),
        other => panic!("not a chunk: {other:?}"),
    }
}

#[test]
fn capabilities_parse_and_undeclared_is_refused() {
    let m = m();
    assert_eq!(m.capabilities().account.sandbox_env.as_deref(), Some("demo"));
    assert_eq!(m.capabilities().regions.sandbox_url.as_deref(), Some("https://demo-futures.kraken.com"));
    let o = &m.capabilities().orders;
    assert_eq!(o.level, otw_connector_model::ExecLevel::E2D);
    assert!(o.reduce_only && o.post_only && o.amend && o.client_id && o.cancel_all_native && !o.brackets && !o.oca && o.trail.is_empty(), "{o:?}");
    assert_eq!(o.position_mode, Some(otw_connector_model::PositionModeCap::Net));
    assert_undeclared_is_unsupported(&m, &InstrumentId::new(VENUE, "PF_XBTUSD"));
}

#[test]
fn symbols_are_kept_as_kraken_writes_them() {
    let m = m();
    assert_eq!(m.instrument_id(" pf_xbtusd ").unwrap().native, "PF_XBTUSD");
    assert_eq!(m.instrument_id("FI_XBTUSD_260925").unwrap().native, "FI_XBTUSD_260925");
    assert_eq!(m.instrument_id("PF XBT").unwrap_err().kind, ErrorKind::NotFound);
}

#[test]
fn the_contract_list_says_linear_or_inverse_and_the_step() {
    let m = m();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let reqs = encode_once(&m, &mut s, &cache, &Command::LoadInstruments { req_id: CorrId(1) }, ts(0)).unwrap();
    let h = http(&reqs);
    assert_eq!((h[0].path.as_str(), h[0].signed), ("/derivatives/api/v3/instruments", false));
    let evs = decode_once(&m, &mut s, &cache, &http_frame(1, "instruments", 200, &json(INSTRUMENTS)), ts(0)).unwrap();
    let specs: Vec<_> = evs
        .iter()
        .filter_map(|e| match &e.event {
            Event::Instrument { spec } => Some(spec.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(specs.len(), 3);
    assert!(matches!(evs.last().unwrap().event, Event::InstrumentsEnd));
    let pi = specs.iter().find(|s| s.id.native == "PI_XBTUSD").unwrap();
    assert!(pi.inverse && pi.asset_class == AssetClass::Perp);
    assert_eq!(pi.note.as_deref(), Some("inverse: one contract is 1 USD, settled in XBT"));
    assert_eq!(pi.lot_size.unwrap().0.to_string(), "1");
    let fi = specs.iter().find(|s| s.id.native == "FI_XBTUSD_220930").unwrap();
    assert_eq!(fi.asset_class, AssetClass::Future);
    assert_eq!(fi.expiry.unwrap().millis(), 1_664_550_000_000);
    let pf = specs.iter().find(|s| s.id.native == "PF_XBTUSD").unwrap();
    assert!(!pf.inverse && pf.active);
    assert_eq!((pf.lot_size.unwrap().0.to_string(), pf.tick_size.unwrap().0.to_string(), pf.multiplier.to_string()), ("0.0001".into(), "1".into(), "1".into()));
    assert_eq!((pf.base.as_deref(), pf.quote.as_str()), (Some("XBT"), "USD"));
    // The live list writes `0E-20` and a negative precision is a step above one
    // (synthetic, shapes measured 2026-11-12).
    let body = r#"{"result":"success","instruments":[{"symbol":"PF_PEPEUSD","type":"flexible_futures","base":"PEPE","quote":"USD","tickSize":1E-10,"contractSize":1,"contractValueTradePrecision":-3,"maxPositionSize":1200.00000000000,"marginLevels":[{"numNonContractUnits":0E-20,"initialMargin":0.01,"maintenanceMargin":0.005}],"suspended":true,"tradeable":true,"isExpired":false}]}"#;
    let evs = decode_once(&m, &mut s, &cache, &http_frame(2, "instruments", 200, body), ts(0)).unwrap_err();
    assert_eq!(evs.kind, otw_connector_model::ErrorKind::Decode, "an answer nobody asked for is refused");
    encode_once(&m, &mut s, &cache, &Command::LoadInstruments { req_id: CorrId(2) }, ts(0)).unwrap();
    let evs = decode_once(&m, &mut s, &cache, &http_frame(2, "instruments", 200, body), ts(0)).unwrap();
    let Event::Instrument { spec } = &evs[0].event else { panic!() };
    assert_eq!((spec.lot_size.unwrap().0.to_string(), spec.active), ("1000".into(), false));
}

#[test]
fn balances_read_every_wallet_coin_by_coin_and_leave_contracts_out() {
    let m = m();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let reqs = encode_once(&m, &mut s, &cache, &query(3, AccountKind::Balances, None), ts(0)).unwrap();
    let h = http(&reqs);
    assert_eq!((h[0].method, h[0].path.as_str(), h[0].signed), (Method::Get, "/derivatives/api/v3/accounts", true));
    let evs = decode_once(&m, &mut s, &cache, &http_frame(3, "accounts", 200, &json(ACCOUNTS)), ts(0)).unwrap();
    let (AccountItems::Balances(b), None) = items(&evs) else { panic!() };
    let rows: Vec<(String, String, String, bool)> =
        b.iter().map(|b| (b.asset.clone(), b.total.0.to_string(), b.name.clone().unwrap_or_default(), b.fiat)).collect();
    assert_eq!(
        rows,
        [
            ("XBT".into(), "141.31756797".into(), "holding wallet".into(), false),
            ("XRP".into(), "52465.1254".into(), "holding wallet".into(), false),
            // `FI_XBTUSD_171215` and the zero `xrp` of the margin account are left out.
            ("XBT".into(), "141.31756797".into(), "fi_xbtusd margin account".into(), false),
            ("EUR".into(), "4540.5837374453".into(), "multi-collateral wallet".into(), true),
            ("USD".into(), "5000".into(), "multi-collateral wallet".into(), true),
            ("XBT".into(), "0.1185308247".into(), "multi-collateral wallet".into(), false),
        ]
    );
    // The summary is the same answer, said in one line.
    encode_once(&m, &mut s, &cache, &query(4, AccountKind::Summary, None), ts(0)).unwrap();
    let evs = decode_once(&m, &mut s, &cache, &http_frame(4, "accounts", 200, &json(ACCOUNTS)), ts(0)).unwrap();
    let Event::AccountSummary(a) = &evs[0].event else { panic!() };
    assert_eq!(
        a.detail.as_deref(),
        Some("key accepted, 6 wallet balance(s), multi-collateral portfolio value 34995.52 USD, available margin 34122.66 USD")
    );
}

#[test]
fn positions_are_coins_marked_by_the_tickers_and_matured_contracts_come_from_the_expired_list() {
    let m = m();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let reqs = encode_once(&m, &mut s, &cache, &query(5, AccountKind::Positions, None), ts(0)).unwrap();
    let h = http(&reqs);
    let shape: Vec<(&str, &str, bool)> = h.iter().map(|h| (h.endpoint_id, h.path.as_str(), h.signed)).collect();
    assert_eq!(
        shape,
        [
            ("contracts", "/derivatives/api/v3/instruments", false),
            ("tickers", "/derivatives/api/v3/tickers", false),
            ("openpositions", "/derivatives/api/v3/openpositions", true)
        ]
    );
    assert!(decode_once(&m, &mut s, &cache, &http_frame(5, "contracts", 200, &json(INSTRUMENTS)), ts(0)).unwrap().is_empty());
    assert!(decode_once(&m, &mut s, &cache, &http_frame(5, "tickers", 200, &json(TICKERS)), ts(0)).unwrap().is_empty());
    // FI_XBTUSD_201225 and PF_DEFIUSD are not in the current list: the answer waits for the
    // expired one, asked next.
    assert!(decode_once(&m, &mut s, &cache, &http_frame(5, "openpositions", 200, &json(POSITIONS)), ts(0)).unwrap().is_empty());
    let next = m.follow_ups(CorrId(5), &mut s);
    let h = http(&next);
    assert_eq!((h[0].endpoint_id, h[0].query.clone()), ("expired", vec![("expired".to_string(), "true".to_string())]));
    let evs = decode_once(&m, &mut s, &cache, &http_frame(5, "expired", 200, &json(EXPIRED)), ts(0)).unwrap();
    let (AccountItems::Positions(p), None) = items(&evs) else { panic!() };
    assert_eq!(p.len(), 3);
    // Short 10000 USD contracts at 9392.749993345933: coins = 10000 / price.
    let pi = &p[0];
    let coins = Decimal::from(10000).checked_div("9392.749993345933".parse::<Decimal>().unwrap()).unwrap();
    assert_eq!((pi.instrument.native.as_str(), pi.qty.0), ("PI_XBTUSD", -coins));
    assert_eq!((pi.unrealized_pnl, pi.mark_price.map(|m| m.0.to_string())), (None, Some("30209.9".into())));
    assert_eq!(pi.name.as_deref(), Some("inverse, 10000 USD contracts, profit counted in the coin"));
    assert_eq!((pi.class, pi.multiplier), (Some(AssetClass::Perp), Some(Decimal::ONE)));
    assert_eq!(p[1].class, Some(AssetClass::Future));
    // Linear: one DEFI. The profit is in BTC, not the contract's USD: not carried.
    let defi = &p[2];
    assert_eq!((defi.qty.0.to_string(), defi.unrealized_pnl, defi.currency.as_deref()), ("1".into(), None, Some("USD")));
    assert_eq!(defi.name.as_deref(), Some("isolated, max leverage 5"));
}

/// A multi-collateral position with neither leverage field is cross with no cap
/// (`crossMaxLeverage` "Absent when the position is uncapped or isolated").
#[test]
fn a_linear_position_with_no_leverage_field_is_uncapped_cross() {
    let m = m();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    encode_once(&m, &mut s, &cache, &query(12, AccountKind::Positions, None), ts(0)).unwrap();
    decode_once(&m, &mut s, &cache, &http_frame(12, "contracts", 200, &json(INSTRUMENTS)), ts(0)).unwrap();
    decode_once(&m, &mut s, &cache, &http_frame(12, "tickers", 200, &json(TICKERS)), ts(0)).unwrap();
    // Synthetic: the positions example's linear row on a listed contract, its leverage removed,
    // and one with `crossMaxLeverage`.
    let body = r#"{"result":"success","openPositions":[{"side":"long","symbol":"PF_XBTUSD","price":30000,"size":0.5,"unrealizedPnl":1},{"side":"short","symbol":"PF_XBTUSD","price":30000,"size":0.5,"unrealizedPnl":1,"crossMaxLeverage":10}],"serverTime":"2026-11-12T10:00:00Z"}"#;
    let evs = decode_once(&m, &mut s, &cache, &http_frame(12, "openpositions", 200, body), ts(0)).unwrap();
    let (AccountItems::Positions(p), None) = items(&evs) else { panic!() };
    assert_eq!(p.iter().map(|p| p.name.as_deref()).collect::<Vec<_>>(), [Some("cross, leverage uncapped"), Some("cross, max leverage 10")]);
}

#[test]
fn a_contract_neither_list_names_is_an_error_naming_it() {
    let m = m();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    encode_once(&m, &mut s, &cache, &query(6, AccountKind::OpenOrders, None), ts(0)).unwrap();
    decode_once(&m, &mut s, &cache, &http_frame(6, "contracts", 200, &json(INSTRUMENTS)), ts(0)).unwrap();
    // Synthetic row on a contract nobody lists.
    let body = r#"{"result":"success","openOrders":[{"order_id":"a","symbol":"PF_NOPEUSD","side":"buy","orderType":"lmt","limitPrice":1,"unfilledSize":1,"filledSize":0,"status":"untouched","reduceOnly":false,"receivedTime":"2019-09-05T17:01:17.410Z","lastUpdateTime":"2019-09-05T17:01:17.410Z"}],"serverTime":"2019-09-05T17:08:18.138Z"}"#;
    assert!(decode_once(&m, &mut s, &cache, &http_frame(6, "openorders", 200, body), ts(0)).unwrap().is_empty());
    m.follow_ups(CorrId(6), &mut s);
    let e = decode_once(&m, &mut s, &cache, &http_frame(6, "expired", 200, &json(EXPIRED)), ts(0)).unwrap_err();
    assert!(e.kind == ErrorKind::NotFound && e.message.contains("PF_NOPEUSD"), "{e}");
}

#[test]
fn open_orders_and_trigger_orders_share_one_id_space_in_coins() {
    let m = m();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let reqs = encode_once(&m, &mut s, &cache, &query(7, AccountKind::OpenOrders, None), ts(0)).unwrap();
    assert_eq!(http(&reqs)[1].path, "/derivatives/api/v3/openorders");
    decode_once(&m, &mut s, &cache, &http_frame(7, "contracts", 200, &json(INSTRUMENTS)), ts(0)).unwrap();
    let evs = decode_once(&m, &mut s, &cache, &http_frame(7, "openorders", 200, &json(ORDERS)), ts(0)).unwrap();
    let (AccountItems::Orders(o), None) = items(&evs) else { panic!() };
    assert_eq!(o.len(), 7);
    let first = &o[0];
    assert_eq!(first.id, "59302619-41d2-4f0b-941f-7e7914760ad3", "an id is the venue's, untagged");
    let coins = Decimal::from(304).checked_div(Decimal::from(10640)).unwrap();
    assert_eq!((first.qty.0, first.filled_qty.0, first.order_type.as_str()), (coins, Decimal::ZERO, "lmt"));
    assert_eq!(first.placed_at.unwrap().millis(), 1_567_702_877_410);
    assert_eq!(first.quote_ccy.as_deref(), Some("USD"));
    // Synthetic: a stop on a linear contract, with no limit (a market on trigger).
    let body = r#"{"result":"success","openOrders":[{"order_id":"723ba95f-13b7-418b-8fcf-ab7ba6620555","symbol":"PF_XBTUSD","side":"sell","orderType":"stop","stopPrice":13789,"unfilledSize":0.5,"filledSize":0.25,"status":"partiallyFilled","reduceOnly":true,"triggerSignal":"last","receivedTime":"2021-02-02T14:10:24.153Z","lastUpdateTime":"2021-02-02T14:10:24.153Z"}],"serverTime":"2021-02-02T14:10:30.000Z"}"#;
    encode_once(&m, &mut s, &cache, &query(8, AccountKind::OpenOrders, None), ts(0)).unwrap();
    let evs = decode_once(&m, &mut s, &cache, &http_frame(8, "openorders", 200, body), ts(0)).unwrap();
    let (AccountItems::Orders(o), None) = items(&evs) else { panic!() };
    assert_eq!(o[0].id, "723ba95f-13b7-418b-8fcf-ab7ba6620555");
    assert_eq!((o[0].qty.0.to_string(), o[0].filled_qty.0.to_string()), ("0.75".into(), "0.25".into()));
    assert_eq!((o[0].limit_price, o[0].stop_price.map(|p| p.0.to_string())), (None, Some("13789".into())));
}

/// Synthetic: one execution page as the history OpenAPI describes it (no example exists),
/// a linear fill and an inverse one.
fn executions(token: Option<&str>) -> String {
    let exec = |uid: &str, tradeable: &str, qty: &str, fee: &str| {
        format!(
            r#"{{"uid":"e-{uid}","timestamp":1700000100000,"event":{{"execution":{{"execution":{{"uid":"{uid}","order":{{"uid":"o-{uid}","accountUid":"a","tradeable":"{tradeable}","direction":"Buy","quantity":"{qty}","filled":"{qty}","timestamp":1700000100000,"limitPrice":"30000","orderType":"Limit","clientId":"","reduceOnly":false,"lastUpdateTimestamp":1700000100000}},"timestamp":1700000100123,"quantity":"{qty}","price":"30000.5","markPrice":"30001","executionType":"taker","limitFilled":true,"usdValue":"15000","orderData":{{"fee":"{fee}","positionSize":"{qty}","feeCalculationInfo":{{"percentageFee":"0.0005","userFeeDiscountApplied":null,"marketShareRebateCredited":null}}}}}},"takerReducedQuantity":""}}}}}}"#
        )
    };
    let token = token.map(|t| format!(r#","continuationToken":"{t}""#)).unwrap_or_default();
    format!(
        r#"{{"accountUid":"a","len":2,"elements":[{},{}]{token}}}"#,
        exec("f1", "PF_XBTUSD", "0.5", "7.5"),
        exec("f2", "PI_XBTUSD", "1000", "0.00001")
    )
}

#[test]
fn fills_walk_the_history_oldest_first_by_its_token_and_leave_inverse_fills_out() {
    let m = m();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let reqs = encode_once(&m, &mut s, &cache, &query(9, AccountKind::Fills, None), ts(0)).unwrap();
    let h = http(&reqs);
    assert_eq!((h[1].path.as_str(), h[1].signed), ("/api/history/v3/executions", true));
    let q: Vec<(&str, &str)> = h[1].query.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    assert_eq!(q, [("since", "1700000000000"), ("before", "1700086400000"), ("sort", "asc"), ("count", "1000")]);
    decode_once(&m, &mut s, &cache, &http_frame(9, "contracts", 200, &json(INSTRUMENTS)), ts(0)).unwrap();
    let evs = decode_once(&m, &mut s, &cache, &http_frame(9, "executions", 200, &json(&executions(Some("dG9rZW4=")))), ts(0)).unwrap();
    let (AccountItems::Fills(f), next) = items(&evs) else { panic!() };
    assert_eq!(next.as_deref(), Some("1:dG9rZW4="));
    assert_eq!(f.len(), 1, "the inverse fill is not imported");
    let x = &f[0];
    assert_eq!((x.id.as_str(), x.order_id.as_deref(), x.instrument.native.as_str()), ("f1", Some("o-f1"), "PF_XBTUSD"));
    assert_eq!((x.qty.0.to_string(), x.price.0.to_string(), x.fee.0.to_string()), ("0.5".into(), "30000.5".into(), "7.5".into()));
    assert_eq!((x.ts.millis(), x.fee_asset.as_deref(), x.quote_ccy.as_deref()), (1_700_000_100_123, None, Some("USD")));
    assert_eq!(x.side, otw_connector_model::Mapped::Known(otw_connector_model::Side::Buy));
    // The next page carries the token; the list is still fresh, so it is not read again.
    let reqs = encode_once(&m, &mut s, &cache, &query(9, AccountKind::Fills, next), ts(0)).unwrap();
    let h = http(&reqs);
    assert_eq!(h.len(), 1);
    assert_eq!(h[0].query.last().unwrap(), &("continuation_token".to_string(), "dG9rZW4=".to_string()));
    let evs = decode_once(&m, &mut s, &cache, &http_frame(9, "executions", 200, r#"{"accountUid":"a","len":0,"elements":[]}"#), ts(0)).unwrap();
    assert!(matches!(&evs[0].event, Event::AccountChunk { next_cursor: None, done: true, .. }));
    // A pull that never ends is stopped with a sentence.
    let e = encode_once(&m, &mut s, &cache, &query(9, AccountKind::Fills, Some("500:x".into())), ts(0)).unwrap_err();
    assert!(e.message.contains("shorter period"));
}

/// The history API hands the next page's token in the `Next-Continuation-Token` header too
/// (the OpenAPI): a page whose body carries none pages on by the header's.
#[test]
fn fills_page_on_by_the_header_token_when_the_body_has_none() {
    let m = m();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    encode_once(&m, &mut s, &cache, &query(10, AccountKind::Fills, None), ts(0)).unwrap();
    decode_once(&m, &mut s, &cache, &http_frame(10, "contracts", 200, &json(INSTRUMENTS)), ts(0)).unwrap();
    let frame = otw_connector_sdk::RawFrame::Http {
        corr_id: CorrId(10),
        endpoint_id: "executions",
        status: 200,
        headers: vec![("next-continuation-token".into(), "aGRy".into())],
        body: bytes::Bytes::from(json(&executions(None))),
    };
    let evs = decode_once(&m, &mut s, &cache, &frame, ts(0)).unwrap();
    assert_eq!(items(&evs).1.as_deref(), Some("1:aGRy"));
    // The body's token wins where both are sent.
    encode_once(&m, &mut s, &cache, &query(11, AccountKind::Fills, None), ts(0)).unwrap();
    let frame = otw_connector_sdk::RawFrame::Http {
        corr_id: CorrId(11),
        endpoint_id: "executions",
        status: 200,
        headers: vec![("Next-Continuation-Token".into(), "aGRy".into())],
        body: bytes::Bytes::from(json(&executions(Some("Ym9keQ==")))),
    };
    let evs = decode_once(&m, &mut s, &cache, &frame, ts(0)).unwrap();
    assert_eq!(items(&evs).1.as_deref(), Some("1:Ym9keQ=="));
}

/// Each signed read weighs its documented cost: `accounts`, `openpositions`, `openorders` 2,
/// a history page 1.
#[test]
fn signed_reads_weigh_their_documented_cost() {
    let m = m();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let mut seen = Vec::new();
    for (i, kind) in [AccountKind::Summary, AccountKind::Balances, AccountKind::Positions, AccountKind::OpenOrders, AccountKind::Fills].into_iter().enumerate() {
        let reqs = encode_once(&m, &mut s, &cache, &query(20 + i as u64, kind, None), ts(0)).unwrap();
        seen.extend(http(&reqs).into_iter().filter(|h| h.signed).map(|h| (h.endpoint_id, h.weight)));
    }
    assert_eq!(seen, [("accounts", 2), ("accounts", 2), ("openpositions", 2), ("openorders", 2), ("executions", 1)]);
}

#[test]
fn a_refused_key_reads_as_auth_and_missing_marks_are_not_an_error() {
    let m = m();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    encode_once(&m, &mut s, &cache, &query(10, AccountKind::Positions, None), ts(0)).unwrap();
    decode_once(&m, &mut s, &cache, &http_frame(10, "contracts", 200, &json(INSTRUMENTS)), ts(0)).unwrap();
    assert!(decode_once(&m, &mut s, &cache, &http_frame(10, "tickers", 502, "<html>"), ts(0)).unwrap().is_empty());
    let e = decode_once(
        &m,
        &mut s,
        &cache,
        &http_frame(10, "openpositions", 200, r#"{"result":"error","error":"authenticationError","serverTime":"2026-11-12T10:18:41.833Z"}"#),
        ts(0),
    )
    .unwrap_err();
    assert_eq!(e.kind, ErrorKind::Auth);
    let secrets: HashMap<String, String> = [("api_key".to_string(), "k".to_string())].into_iter().collect();
    let Err(e) = m.signer(&secrets) else { panic!() };
    assert!(e.message.contains("api_secret"));
}

// ── Public conformance (no key): `OTW_NET_TEST=1` ──

/// The live contract list decodes, and an account read signed with a key Kraken never issued
/// is answered with the authentication refusal on both APIs (derivatives and history), which
/// says the path, headers and signed string reached the key check. Run with:
/// `OTW_NET_TEST=1 cargo test -p otw-connector-providers kraken_futures::tests::public -- --nocapture`.
#[tokio::test]
async fn public_conformance_against_the_live_venue() {
    use otw_connector_sdk::{Driver, DriverConfig};
    if std::env::var("OTW_NET_TEST").as_deref() != Ok("1") {
        return;
    }
    let d = Driver::new(m(), DriverConfig::new("kraken_futures"), reqwest::Client::new());
    let specs = d.load_instruments(CorrId(1)).await.unwrap();
    assert!(specs.len() > 10, "{} contracts", specs.len());
    let btc = specs.iter().find(|s| s.id.native == "PF_XBTUSD").expect("PF_XBTUSD listed");
    assert!(btc.tick_size.is_some() && btc.lot_size.is_some() && !btc.inverse, "{btc:?}");
    println!("kraken_futures: {} contracts, PF_XBTUSD tick {:?} lot {:?}", specs.len(), btc.tick_size, btc.lot_size);
    let secrets: HashMap<String, String> =
        [("api_key", "0123456789abcdef0123456789abcdef"), ("api_secret", "aGVsbG8gd29ybGQgdGhpcyBpcyBub3QgYSBrZXkgYXQgYWxs")] // gitleaks:allow
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
    let signer = m().signer(&secrets).unwrap().unwrap();
    let range = Some(TimeRange::new(ts(1_700_000_000), ts(1_700_086_400)));
    for kind in [AccountKind::Summary, AccountKind::Fills] {
        let e = d.account(CorrId(2), kind, &[], range, None, Some(signer.as_ref())).await.unwrap_err();
        println!("kraken_futures {kind:?} with a junk key: {e}");
        assert_eq!(e.kind, ErrorKind::Auth, "{kind:?}: {e}");
    }
}
