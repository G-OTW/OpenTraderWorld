//! Coinbase, offline. Synthetic fixtures written from the documented schema.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use otw_connector_model::{AccountItems, AccountKind, Command, CorrId, DataType, ErrorKind, Event, HistoryItems, InstrumentId, TimeRange, Timeframe};
use otw_connector_sdk::{ConnKey, InstrumentCache, MapCtx, NativeRequest, RawFrame};
use otw_connector_testkit::{assert_history_sound, assert_undeclared_is_unsupported, chunks, decode_once, encode_once, http_frame, json, ts};

use super::*;

fn btc() -> InstrumentId {
    InstrumentId::new(VENUE, "BTC-USD")
}

fn hist(cursor: Option<&str>, tf: Timeframe, from: i64, to: i64) -> Command {
    Command::RequestHistory { req_id: CorrId(1), instrument: btc(), data_type: DataType::Candles { tf }, range: TimeRange::new(ts(from), ts(to)), cursor: cursor.map(str::to_string) }
}

fn q<'a>(h: &'a HttpRequest, k: &str) -> Option<&'a str> {
    h.query.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
}

#[test]
fn capabilities_parse_and_undeclared_is_refused() {
    let m = Coinbase::new().unwrap();
    assert_eq!(m.capabilities().history.candles.as_ref().map(|h| h.max_items_per_call), Some(300));
    assert_undeclared_is_unsupported(&m, &btc());
    // Candles are never streamed: a candle subscription is refused, trades are the source.
    let sub = Command::Subscribe { sub_id: CorrId(1), instrument: btc(), data_type: DataType::Candles { tf: Timeframe::M1 } };
    assert_eq!(encode_once(&m, &mut Session::default(), &InstrumentCache::new(), &sub, ts(0)).unwrap_err().kind, ErrorKind::Unsupported);
    assert!(m.instrument_id("BTC-USD/../x").is_err(), "a product id lands in a path");
}

#[test]
fn history_pages_300_steps_at_a_time_with_both_ends_counted() {
    let m = Coinbase::new().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let from = 1_704_067_200; // 2024-01-01
    let to = from + 450 * 60;
    let reqs = encode_once(&m, &mut s, &cache, &hist(None, Timeframe::M1, from, to), ts(2_000_000_000)).unwrap();
    let [NativeRequest::Http(h)] = reqs.as_slice() else { panic!() };
    assert_eq!(h.path, "/products/BTC-USD/candles");
    assert_eq!((q(h, "granularity"), q(h, "start"), q(h, "end")), (Some("60"), Some("2024-01-01T00:00:00Z"), Some("2024-01-01T05:00:00Z")));
    // 301 rows (both ends), newest first.
    let rows: Vec<String> = (0..=300).rev().map(|i| format!("[{},42000.5,42100,42050.25,42080.75,1.25]", from + i * 60)).collect();
    let evs = decode_once(&m, &mut s, &cache, &http_frame(1, "candles", 200, &format!("[{}]", rows.join(","))), ts(2_000_000_000)).unwrap();
    let c = chunks(&evs);
    let (HistoryItems::Candles(cs), next, done) = &c[0] else { panic!() };
    assert_eq!(cs.len(), 300, "the end bar opens the next page");
    assert_eq!((cs[0].open.to_string(), cs[0].high.to_string(), cs[0].low.to_string(), cs[0].close.to_string()), ("42050.25".into(), "42100".into(), "42000.5".into(), "42080.75".into()));
    assert_eq!((next.as_deref(), *done), (Some("1704085200"), false));
    assert_history_sound(&[c[0].0.clone()]);
    let reqs = encode_once(&m, &mut s, &cache, &hist(Some("1704085200"), Timeframe::M1, from, to), ts(2_000_000_000)).unwrap();
    let [NativeRequest::Http(h)] = reqs.as_slice() else { panic!() };
    assert_eq!((q(h, "start"), q(h, "end")), (Some("2024-01-01T05:00:00Z"), Some("2024-01-01T07:30:00Z")));
}

#[test]
fn bars_go_out_forming_and_the_window_is_exact() {
    let m = Coinbase::new().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let from = 1_704_067_230; // unaligned
    encode_once(&m, &mut s, &cache, &hist(None, Timeframe::M1, from, from + 180), ts(0)).unwrap();
    let body = "[[1704067380,1,2,1,1,1],[1704067320,1,2,1,1,1],[1704067260,1,2,1,1,1],[1704067200,1,2,1,1,1]]";
    let evs = decode_once(&m, &mut s, &cache, &http_frame(1, "candles", 200, body), ts(1_704_067_370)).unwrap();
    let c = chunks(&evs);
    let (HistoryItems::Candles(cs), ..) = &c[0] else { panic!() };
    // No closed flag: every bar goes out forming, the engine finalizes at the period's end.
    assert_eq!(cs.iter().map(|c| (c.open_ts.secs(), c.is_final)).collect::<Vec<_>>(), [(1_704_067_260, false), (1_704_067_320, false), (1_704_067_380, false)]);
}

#[test]
fn an_unknown_product_is_a_symbol_problem() {
    let m = Coinbase::new().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    encode_once(&m, &mut s, &cache, &hist(None, Timeframe::M1, 1_700_000_000, 1_700_003_600), ts(0)).unwrap();
    let e = decode_once(&m, &mut s, &cache, &http_frame(1, "candles", 404, r#"{"message":"NotFound"}"#), ts(0)).unwrap_err();
    assert!(e.kind == ErrorKind::NotFound && e.message.contains("bad symbol"));
}

#[test]
fn products_keep_their_order_and_display_name() {
    let m = Coinbase::new().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    encode_once(&m, &mut s, &cache, &Command::LoadInstruments { req_id: CorrId(4) }, ts(0)).unwrap();
    let body = json(
        r#"[{"id":"NEON-USD","base_currency":"NEON","quote_currency":"USD","display_name":"NEON/USD","base_increment":"0.01","quote_increment":"0.00001","trading_disabled":false},
            {"id":"BTC-USD","base_currency":"BTC","quote_currency":"USD","display_name":"BTC-USD"},
            {"id":"OLD-USD","base_currency":"OLD","quote_currency":"USD","trading_disabled":true}]"#,
    );
    let evs = decode_once(&m, &mut s, &cache, &http_frame(4, "products", 200, &body), ts(0)).unwrap();
    let specs: Vec<_> = evs.iter().filter_map(|e| match &e.event { Event::Instrument { spec } => Some(spec.as_ref().clone()), _ => None }).collect();
    assert_eq!(specs.iter().map(|s| (s.id.native.as_str(), s.symbol.0.as_str(), s.active)).collect::<Vec<_>>(), [("NEON-USD", "NEON/USD", true), ("BTC-USD", "BTC-USD", true), ("OLD-USD", "OLD-USD", false)]);
    assert_eq!((specs[0].base.as_deref(), specs[0].quote.as_str(), specs[0].lot_size.map(|q| q.to_string())), (Some("NEON"), "USD", Some("0.01".into())));
}

#[test]
fn the_board_skips_unpriced_products_and_keeps_their_quotes() {
    let m = Coinbase::new().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let reqs = encode_once(&m, &mut s, &cache, &Command::RequestBoard { req_id: CorrId(5) }, ts(0)).unwrap();
    let [NativeRequest::Http(h)] = reqs.as_slice() else { panic!() };
    assert_eq!((h.base, h.path.as_str(), h.signed), (BROKERAGE, "/api/v3/brokerage/market/products", false));
    let body = json(r#"{"products":[{"product_id":"BTC-USD","price":"81531.5","quote_currency_id":"USD"},{"product_id":"DEAD-USD","price":"","quote_currency_id":"USD"},{"product_id":"ETH-EUR","price":"2000","quote_currency_id":"EUR"}]}"#);
    let evs = decode_once(&m, &mut s, &cache, &http_frame(5, "board", 200, &body), ts(0)).unwrap();
    let Event::Board(b) = &evs[0].event else { panic!() };
    assert_eq!(b.iter().map(|p| p.instrument.native.as_str()).collect::<Vec<_>>(), ["BTC-USD", "ETH-EUR"]);
    assert_eq!(s.quotes().get("DEAD-USD").map(String::as_str), Some("USD"));
}

fn fills(cursor: Option<String>, symbols: &[&str]) -> Command {
    Command::QueryAccount {
        req_id: CorrId(7),
        kind: AccountKind::Fills,
        symbols: symbols.iter().map(|s| InstrumentId::new(VENUE, s)).collect(),
        range: Some(TimeRange::new(ts(1_700_000_000), ts(1_700_086_400))),
        cursor,
        account: None,
    }
}

const QUOTES: &str = r#"{"products":[{"product_id":"BTC-USD","price":"30000","quote_currency_id":"USD"},{"product_id":"ETH-EUR","price":"2000","quote_currency_id":"EUR"}]}"#;

#[test]
fn fills_are_signed_paged_by_cursor_and_converted_from_quote_size() {
    let m = Coinbase::new().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let reqs = encode_once(&m, &mut s, &cache, &fills(None, &["BTC-USD"]), ts(0)).unwrap();
    let [NativeRequest::Http(qh), NativeRequest::Http(h)] = reqs.as_slice() else { panic!() };
    assert_eq!(qh.endpoint_id, "productQuotes");
    assert!(h.signed && h.base == BROKERAGE && h.path == "/api/v3/brokerage/orders/historical/fills");
    assert_eq!((q(h, "start_sequence_timestamp"), q(h, "product_ids"), q(h, "cursor")), (Some("2023-11-14T22:13:20Z"), Some("BTC-USD"), None));
    assert_eq!(q(h, "product_types"), Some("SPOT"), "spot fills only: the default is every product type");
    decode_once(&m, &mut s, &cache, &http_frame(7, "productQuotes", 200, &json(QUOTES)), ts(0)).unwrap();
    let rows: Vec<String> = (0..100)
        .map(|i| {
            let quote_sized = i == 0;
            format!(
                r#"{{"entry_id":"e{i}","trade_id":"t{i}","order_id":"o1","product_id":"BTC-USD","trade_time":"2023-11-15T10:00:00.123456Z","side":"BUY","price":"50000","size":"{}","size_in_quote":{quote_sized},"commission":"-0.5","retail_portfolio_id":"pf1"}}"#,
                if quote_sized { "100" } else { "0.5" }
            )
        })
        .collect();
    let body = format!(r#"{{"fills":[{}],"cursor":"abc"}}"#, rows.join(","));
    let evs = decode_once(&m, &mut s, &cache, &http_frame(7, "fills", 200, &body), ts(0)).unwrap();
    let Event::AccountChunk { items: AccountItems::Fills(f), next_cursor, .. } = &evs[0].event else { panic!() };
    assert_eq!(next_cursor.as_deref(), Some("1:abc"));
    assert_eq!(f[0].qty.to_string(), "0.002", "100 USD at 50 000, not 100 coins");
    // The commission as stated: its sign is doc silent, so a negative one stays a rebate.
    assert_eq!((f[0].fee.to_string(), f[0].fee_asset.as_deref(), f[0].quote_ccy.as_deref(), f[0].account.as_deref()), ("-0.5".into(), Some("USD"), Some("USD"), Some("pf1")));
    assert_eq!(f[1].ts.nanos() % 1_000_000_000, 123_456_000);
    let reqs = encode_once(&m, &mut s, &cache, &fills(Some("1:abc".into()), &[]), ts(0)).unwrap();
    let [NativeRequest::Http(h)] = reqs.as_slice() else { panic!() };
    assert_eq!((q(h, "cursor"), q(h, "product_ids")), (Some("abc"), None), "quotes kept, cursor sent");
    let evs = decode_once(&m, &mut s, &cache, &http_frame(7, "fills", 200, r#"{"fills":[],"cursor":""}"#), ts(0)).unwrap();
    assert!(matches!(&evs[0].event, Event::AccountChunk { next_cursor: None, .. }));
    assert!(encode_once(&m, &mut s, &cache, &fills(Some("500:x".into()), &[]), ts(0)).unwrap_err().message.contains("shorter period"));
    // A quote-sized fill at price 0 cannot be a quantity.
    encode_once(&m, &mut s, &cache, &fills(None, &[]), ts(0)).unwrap();
    let bad = r#"{"fills":[{"entry_id":"x","product_id":"BTC-USD","trade_time":"2023-11-15T10:00:00Z","side":"BUY","price":"0","size":"100","size_in_quote":true}],"cursor":""}"#;
    assert!(decode_once(&m, &mut s, &cache, &http_frame(7, "fills", 200, bad), ts(0)).unwrap_err().message.contains("price 0"));
    // An adjusted fill is never read as a buy or a sell of its own: it is left out, the other
    // fills of the page kept.
    encode_once(&m, &mut s, &cache, &fills(None, &[]), ts(0)).unwrap();
    let adjusted = r#"{"fills":[{"entry_id":"r1","trade_type":"REVERSAL","order_id":"o1","product_id":"BTC-USD","trade_time":"2023-11-15T10:00:00Z","side":"BUY","price":"50000","size":"0.5"},{"entry_id":"r2","trade_type":"FILL","order_id":"o2","product_id":"BTC-USD","trade_time":"2023-11-15T10:00:00Z","side":"BUY","price":"50000","size":"0.5"}],"cursor":""}"#;
    let evs = decode_once(&m, &mut s, &cache, &http_frame(7, "fills", 200, adjusted), ts(0)).unwrap();
    let Event::AccountChunk { items: AccountItems::Fills(kept), .. } = &evs[0].event else { panic!("{evs:?}") };
    assert_eq!(kept.iter().map(|f| f.id.as_str()).collect::<Vec<_>>(), vec!["r2"]);
}

#[test]
fn wallets_add_holds_and_say_which_are_fiat() {
    let m = Coinbase::new().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let qy = Command::QueryAccount { req_id: CorrId(8), kind: AccountKind::Balances, symbols: vec![], range: None, cursor: None, account: None };
    encode_once(&m, &mut s, &cache, &qy, ts(0)).unwrap();
    let body = json(
        r#"{"accounts":[
        {"currency":"BTC","name":"BTC Wallet","type":"ACCOUNT_TYPE_CRYPTO","available_balance":{"value":"0.5","currency":"BTC"},"hold":{"value":"0.25","currency":"BTC"},"retail_portfolio_id":"pf1"},
        {"currency":"USD","name":"Cash (USD)","type":"ACCOUNT_TYPE_FIAT","available_balance":{"value":"100.00"},"hold":{"value":"0"}},
        {"currency":"ETH","name":"ETH Wallet","type":"ACCOUNT_TYPE_CRYPTO","available_balance":{"value":"0"},"hold":{"value":"0"}}]}"#,
    );
    let evs = decode_once(&m, &mut s, &cache, &http_frame(8, "accounts", 200, &body), ts(0)).unwrap();
    let Event::AccountChunk { items: AccountItems::Balances(b), .. } = &evs[0].event else { panic!() };
    assert_eq!(b.len(), 2);
    assert_eq!((b[0].total.to_string(), b[0].name.as_deref(), b[0].fiat, b[0].account.as_deref()), ("0.75".into(), Some("BTC Wallet"), false, Some("pf1")));
    assert!(b[1].fiat && b[1].account.is_none());
    // The summary counts accounts and those with something available.
    let qy = Command::QueryAccount { req_id: CorrId(9), kind: AccountKind::Summary, symbols: vec![], range: None, cursor: None, account: None };
    encode_once(&m, &mut s, &cache, &qy, ts(0)).unwrap();
    let evs = decode_once(&m, &mut s, &cache, &http_frame(9, "accounts", 200, &body), ts(0)).unwrap();
    let Event::AccountSummary(a) = &evs[0].event else { panic!() };
    assert_eq!(a.detail.as_deref(), Some("key accepted, 3 account(s), 2 with a balance"));
}

#[test]
fn open_orders_read_their_prices_from_whatever_configuration() {
    let m = Coinbase::new().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let qy = Command::QueryAccount { req_id: CorrId(9), kind: AccountKind::OpenOrders, symbols: vec![], range: None, cursor: None, account: None };
    encode_once(&m, &mut s, &cache, &qy, ts(0)).unwrap();
    decode_once(&m, &mut s, &cache, &http_frame(9, "productQuotes", 200, &json(QUOTES)), ts(0)).unwrap();
    let body = json(
        r#"{"orders":[
        {"order_id":"A","product_id":"BTC-USD","side":"SELL","order_type":"STOP_LIMIT","order_configuration":{"stop_limit_stop_limit_gtc":{"base_size":"0.1","limit_price":"29000","stop_price":"29500","stop_direction":"STOP_DIRECTION_STOP_DOWN"}},"filled_size":"0","created_time":"2023-11-15T10:00:00.5Z"},
        {"order_id":"B","product_id":"NEW-USD","side":"BUY","order_type":"LIMIT","order_configuration":{"limit_limit_gtc":{"base_size":"2","limit_price":"1.5"}},"filled_size":"0.5"}]}"#,
    );
    let evs = decode_once(&m, &mut s, &cache, &http_frame(9, "ordersOpen", 200, &body), ts(0)).unwrap();
    let Event::AccountChunk { items: AccountItems::Orders(o), next_cursor, .. } = &evs[0].event else { panic!() };
    assert_eq!((o[0].qty.to_string(), o[0].limit_price.map(|p| p.to_string()), o[0].stop_price.map(|p| p.to_string()), o[0].quote_ccy.as_deref()), ("0.1".into(), Some("29000".into()), Some("29500".into()), Some("USD")));
    assert_eq!(o[0].placed_at.map(|t| t.millis()), Some(1_700_042_400_500));
    assert_eq!((o[1].quote_ccy.as_deref(), o[1].filled_qty.to_string()), (None, "0.5".into()));
    // The advanced source is done: the simple app's orders are listed next.
    assert_eq!(next_cursor.as_deref(), Some("1:"));
}

/// Every page of every placement source: the first page only would hide orders past it.
#[test]
fn open_orders_walk_every_page_of_each_placement_source() {
    let m = Coinbase::new().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let qy = |cursor: Option<&str>| Command::QueryAccount { req_id: CorrId(9), kind: AccountKind::OpenOrders, symbols: vec![], range: None, cursor: cursor.map(str::to_string), account: None };
    let reqs = encode_once(&m, &mut s, &cache, &qy(None), ts(0)).unwrap();
    let NativeRequest::Http(h) = reqs.last().unwrap() else { panic!() };
    assert_eq!((q(h, "order_placement_source"), q(h, "cursor")), (Some("RETAIL_ADVANCED"), None));
    // Every working status, not OPEN alone: a queued order may still fill.
    let statuses: Vec<&str> = h.query.iter().filter(|(k, _)| k == "order_status").map(|(_, v)| v.as_str()).collect();
    assert_eq!(statuses, ["OPEN", "PENDING", "QUEUED", "CANCEL_QUEUED", "EDIT_QUEUED"]);
    let page = r#"{"orders":[{"order_id":"A","product_id":"BTC-USD","side":"BUY","order_type":"LIMIT","order_configuration":{"limit_limit_gtc":{"base_size":"1","limit_price":"1"}},"filled_size":"0"}],"has_next":true,"cursor":"c2"}"#;
    let evs = decode_once(&m, &mut s, &cache, &http_frame(9, "ordersOpen", 200, page), ts(0)).unwrap();
    assert!(matches!(&evs[0].event, Event::AccountChunk { next_cursor: Some(c), .. } if c == "0:c2"));
    let reqs = encode_once(&m, &mut s, &cache, &qy(Some("0:c2")), ts(0)).unwrap();
    let NativeRequest::Http(h) = reqs.last().unwrap() else { panic!() };
    assert_eq!((q(h, "order_placement_source"), q(h, "cursor")), (Some("RETAIL_ADVANCED"), Some("c2")));
    decode_once(&m, &mut s, &cache, &http_frame(9, "ordersOpen", 200, r#"{"orders":[],"has_next":false,"cursor":""}"#), ts(0)).unwrap();
    let reqs = encode_once(&m, &mut s, &cache, &qy(Some("1:")), ts(0)).unwrap();
    let NativeRequest::Http(h) = reqs.last().unwrap() else { panic!() };
    assert_eq!((q(h, "order_placement_source"), q(h, "cursor")), (Some("RETAIL_SIMPLE"), None));
    // Every placement source of the enum, in turn; the last one ends the walk.
    for (i, src) in ["RETAIL_ADMIN", "RETAIL_RAISE", "UNKNOWN_PLACEMENT_SOURCE"].into_iter().enumerate() {
        let reqs = encode_once(&m, &mut s, &cache, &qy(Some(&format!("{}:", i + 2))), ts(0)).unwrap();
        let NativeRequest::Http(h) = reqs.last().unwrap() else { panic!() };
        assert_eq!(q(h, "order_placement_source"), Some(src));
    }
    // An order placed in the quote currency shows what it filled, and its type says the amount.
    let quoted = r#"{"orders":[{"order_id":"Q","product_id":"BTC-USD","side":"BUY","order_type":"LIMIT","size_in_quote":true,"order_configuration":{"limit_limit_gtc":{"quote_size":"50","limit_price":"60000"}},"filled_size":"0.0001"}],"has_next":false,"cursor":""}"#;
    let evs = decode_once(&m, &mut s, &cache, &http_frame(9, "ordersOpen", 200, quoted), ts(0)).unwrap();
    let Event::AccountChunk { items: AccountItems::Orders(o), next_cursor, .. } = &evs[0].event else { panic!() };
    assert_eq!((o[0].qty.to_string(), o[0].order_type.as_str(), next_cursor.as_deref()), ("0.0001".into(), "LIMIT (50 in the quote currency)", None));
    assert!(encode_once(&m, &mut s, &cache, &qy(Some("7:x")), ts(0)).is_err());
}

/// A bracket's stop is its `stop_trigger_price`; an entry with an attached take profit and stop
/// loss says them in its type words.
#[test]
fn open_orders_read_a_brackets_stop_and_an_entrys_attached_exits() {
    let m = Coinbase::new().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let qy = Command::QueryAccount { req_id: CorrId(9), kind: AccountKind::OpenOrders, symbols: vec![], range: None, cursor: None, account: None };
    encode_once(&m, &mut s, &cache, &qy, ts(0)).unwrap();
    let page = r#"{"orders":[
        {"order_id":"E","product_id":"BTC-USD","side":"BUY","order_type":"LIMIT","order_configuration":{"limit_limit_gtc":{"base_size":"0.05","limit_price":"99900.83"}},"attached_order_configuration":{"trigger_bracket_gtc":{"limit_price":"99990","stop_trigger_price":"99800"}},"filled_size":"0"},
        {"order_id":"B","product_id":"BTC-USD","side":"SELL","order_type":"TAKE_PROFIT_STOP_LOSS","order_configuration":{"trigger_bracket_gtc":{"base_size":"0.05","limit_price":"99990","stop_trigger_price":"99800"}},"filled_size":"0"}
    ],"has_next":false,"cursor":""}"#;
    let evs = decode_once(&m, &mut s, &cache, &http_frame(9, "ordersOpen", 200, page), ts(0)).unwrap();
    let Event::AccountChunk { items: AccountItems::Orders(o), .. } = &evs[0].event else { panic!() };
    assert_eq!((o[0].order_type.as_str(), o[0].stop_price.map(|p| p.0.to_string())), ("LIMIT with take profit 99990 and stop loss 99800 attached", None));
    assert_eq!((o[1].limit_price.map(|p| p.0.to_string()), o[1].stop_price.map(|p| p.0.to_string())), (Some("99990".into()), Some("99800".into())));
}

/// A list Coinbase withholds behind a proof token (EU SCA) is an error naming it, never empty.
#[test]
fn a_list_behind_a_proof_token_is_an_error_not_empty() {
    let m = Coinbase::new().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let qy = Command::QueryAccount { req_id: CorrId(9), kind: AccountKind::OpenOrders, symbols: vec![], range: None, cursor: None, account: None };
    encode_once(&m, &mut s, &cache, &qy, ts(0)).unwrap();
    let e = decode_once(&m, &mut s, &cache, &http_frame(9, "ordersOpen", 200, r#"{"orders":[],"has_next":false,"cursor":"","proof_token_required":true}"#), ts(0)).unwrap_err();
    assert!(e.message.contains("proof token"), "{e}");
    let qy = Command::QueryAccount { req_id: CorrId(10), kind: AccountKind::Fills, symbols: vec![], range: Some(TimeRange::new(ts(0), ts(1_000_000))), cursor: None, account: None };
    encode_once(&m, &mut s, &cache, &qy, ts(0)).unwrap();
    let e = decode_once(&m, &mut s, &cache, &http_frame(10, "fills", 200, r#"{"fills":[],"cursor":"","proof_token_required":true}"#), ts(0)).unwrap_err();
    assert!(e.message.contains("proof token"), "{e}");
}

#[test]
fn settings_name_the_whole_key() {
    let m = Coinbase::new().unwrap();
    let cfg = |v: &str| std::iter::once(("key_name".to_string(), v.to_string())).collect::<HashMap<_, _>>();
    assert!(m.check_settings(&cfg("organizations/o/apiKeys/k")).is_ok());
    // A CDP Ed25519 key's id is a bare UUID.
    assert!(m.check_settings(&cfg("8a0f2b9e-1c3d-4e5f-9a7b-6c5d4e3f2a1b")).is_ok());
    assert!(m.check_settings(&cfg("k")).is_err());
    assert!(m.check_settings(&cfg("8a0f2b9e1c3d-4e5f-9a7b-6c5d4e3f2a1b-")).is_err());
    assert!(m.check_settings(&HashMap::new()).is_ok());
}

fn ws(p: &str) -> RawFrame {
    RawFrame::Ws { conn_key: ConnKey("feed".into()), payload: p.to_string().into() }
}

#[test]
fn the_feed_routes_trades_by_product() {
    let m = Coinbase::new().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let c = otw_connector_model::ConnectorId("t".into());
    let ctx = MapCtx { received_ts: ts(0), instruments: &cache, connector: &c };
    let sub = |id: u64, p: &str| Command::Subscribe { sub_id: CorrId(id), instrument: InstrumentId::new(VENUE, p), data_type: DataType::Trades };
    let out = m.encode_batch(&[sub(1, "BTC-USD"), sub(2, "ETH-USD")], &mut s, &ctx).unwrap();
    assert!(matches!(&out[0], NativeRequest::WsOpen { path, .. } if path.is_empty()));
    let NativeRequest::WsSend { payload, .. } = &out[1] else { panic!() };
    let v: serde_json::Value = serde_json::from_slice(payload).unwrap();
    assert_eq!(v, serde_json::json!({"type":"subscribe","product_ids":["BTC-USD","ETH-USD"],"channels":["matches"]}));
    let evs = decode_once(&m, &mut s, &cache, &ws(r#"{"type":"match","trade_id":7,"product_id":"ETH-USD","price":"2500.5","size":"0.3","side":"sell","time":"2026-09-05T10:00:01.25Z"}"#), ts(0)).unwrap();
    let Event::Trade(t) = &evs[0].event else { panic!() };
    assert_eq!((evs[0].corr_id, t.price.to_string(), t.qty.to_string()), (Some(CorrId(2)), "2500.5".into(), "0.3".into()));
    assert_eq!(evs[0].exchange_ts.map(|t| t.millis()), Some(1_788_602_401_250));
    assert!(decode_once(&m, &mut s, &cache, &ws(r#"{"type":"subscriptions","channels":[]}"#), ts(0)).unwrap().is_empty());
    let evs = decode_once(&m, &mut s, &cache, &ws(r#"{"type":"error","message":"Failed to subscribe","reason":"ETH-USD is delisted"}"#), ts(0)).unwrap();
    assert!(matches!(&evs[0].event, Event::SubscriptionFail { .. }) && evs[0].corr_id == Some(CorrId(2)));
    let evs = decode_once(&m, &mut s, &cache, &ws(r#"{"type":"error","message":"Malformed"}"#), ts(0)).unwrap();
    assert!(matches!(&evs[0].event, Event::Error(_)) && evs[0].corr_id.is_none());
    let out = m.encode_batch(&[Command::Unsubscribe { sub_id: CorrId(2) }], &mut s, &ctx).unwrap();
    let [NativeRequest::WsSend { payload, .. }] = out.as_slice() else { panic!() };
    let v: serde_json::Value = serde_json::from_slice(payload).unwrap();
    assert_eq!(v["type"], "unsubscribe");
}

proptest::proptest! {
    #[test]
    fn decode_survives_arbitrary_bytes(bytes in proptest::collection::vec(proptest::num::u8::ANY, 0..512), which in 0u8..4) {
        let m = Coinbase::new().unwrap();
        let mut s = Session::default();
        let cache = InstrumentCache::new();
        let frame = match which {
            0 => {
                encode_once(&m, &mut s, &cache, &hist(None, Timeframe::M1, 1_700_000_000, 1_700_003_600), ts(0)).unwrap();
                RawFrame::Http { corr_id: CorrId(1), endpoint_id: "candles", status: 200, headers: vec![], body: bytes.into() }
            }
            1 => {
                encode_once(&m, &mut s, &cache, &fills(None, &[]), ts(0)).unwrap();
                RawFrame::Http { corr_id: CorrId(7), endpoint_id: "fills", status: 200, headers: vec![], body: bytes.into() }
            }
            2 => {
                encode_once(&m, &mut s, &cache, &Command::RequestBoard { req_id: CorrId(5) }, ts(0)).unwrap();
                RawFrame::Http { corr_id: CorrId(5), endpoint_id: "board", status: 200, headers: vec![], body: bytes.into() }
            }
            _ => RawFrame::Ws { conn_key: ConnKey("feed".into()), payload: bytes.into() },
        };
        let _ = decode_once(&m, &mut s, &cache, &frame, ts(0));
    }
}

#[test]
fn the_top_of_the_level2_book_is_the_quote() {
    let m = Coinbase::new().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let c = otw_connector_model::ConnectorId("t".into());
    let ctx = MapCtx { received_ts: ts(0), instruments: &cache, connector: &c };
    let q = Command::Subscribe { sub_id: CorrId(3), instrument: btc(), data_type: DataType::Quote };
    let out = m.encode_batch(&[q], &mut s, &ctx).unwrap();
    let NativeRequest::WsSend { payload, .. } = &out[1] else { panic!("{out:?}") };
    let v: serde_json::Value = serde_json::from_slice(payload).unwrap();
    assert_eq!(v, serde_json::json!({"type":"subscribe","product_ids":["BTC-USD"],"channels":["level2_batch"]}));
    let ws = |p: &str| RawFrame::Ws { conn_key: ConnKey("feed".into()), payload: p.to_string().into() };
    let quote = |s: &mut Session, f: &str| -> Option<(String, String)> {
        let evs = decode_once(&m, s, &cache, &ws(f), ts(1_792_410_300)).unwrap();
        evs.first().map(|e| {
            assert_eq!(e.corr_id, Some(CorrId(3)));
            let Event::Quote(q) = &e.event else { panic!("{evs:?}") };
            (q.bid.to_string(), q.ask.to_string())
        })
    };
    // Shapes recorded from the live feed, trimmed to a few levels.
    let snap = r#"{"type":"snapshot","product_id":"BTC-USD","asks":[["82226.50","0.01824265"],["82227.88","0.24324871"]],"bids":[["82226.49","0.5"],["82220.00","1.2"]]}"#;
    assert_eq!(quote(&mut s, snap), Some(("82226.49".into(), "82226.50".into())));
    let upd = r#"{"type":"l2update","product_id":"BTC-USD","changes":[["buy","82226.49","0.00000000"],["sell","82226.45","0.3"]],"time":"2026-10-19T11:45:00.123456Z"}"#;
    assert_eq!(quote(&mut s, upd), Some(("82220.00".into(), "82226.45".into())), "the top bid left, a better ask arrived");
    let evs = decode_once(&m, &mut s, &cache, &ws(upd), ts(1_792_410_300)).unwrap();
    assert_eq!(evs[0].exchange_ts.map(|t| t.millis()), Some(1_792_410_300_123));
    // Another product's book is not this pane's.
    assert_eq!(quote(&mut s, &snap.replace("BTC-USD", "ETH-USD")), None);
    let out = m.encode_batch(&[Command::Unsubscribe { sub_id: CorrId(3) }], &mut s, &ctx).unwrap();
    let NativeRequest::WsSend { payload, .. } = &out[0] else { panic!() };
    let v: serde_json::Value = serde_json::from_slice(payload).unwrap();
    assert_eq!((v["type"].as_str(), v["channels"][0].as_str()), (Some("unsubscribe"), Some("level2_batch")));
}
