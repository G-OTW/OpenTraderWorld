//! Best bid and ask through the native bridge: a venue that pushes a quote channel, and one
//! whose streamed candles carry both sides. Frames recorded from the venues (Binance) or
//! written from their documented schema (Capital.com).

use std::collections::HashMap;
use std::sync::Arc;

use otw_connector_providers::binance::Binance;
use otw_connector_sdk::{Driver, DriverConfig};

use super::super::{Feed, FeedUpdate, Instrument, LiveQuote, Payload, StreamConnector};
use super::{NativeFeed, NativeStream};

fn binance() -> Arc<Driver<Binance>> {
    Arc::new(Driver::new(Binance::spot().expect("caps"), DriverConfig::new("binance"), reqwest::Client::new()))
}

fn quotes(updates: &[FeedUpdate]) -> Vec<(String, LiveQuote)> {
    updates
        .iter()
        .filter_map(|u| match u.payload {
            Payload::Quote(q) => Some((u.channel.clone(), q)),
            _ => None,
        })
        .collect()
}

fn params(frames: &[String]) -> Vec<String> {
    frames
        .iter()
        .flat_map(|f| {
            let v: serde_json::Value = serde_json::from_str(f).expect("json");
            v["params"].as_array().cloned().unwrap_or_default()
        })
        .map(|p| p.as_str().unwrap_or_default().to_string())
        .collect()
}

#[test]
fn a_book_ticker_reaches_the_quote_channel_and_only_it() {
    let bars = Instrument::new("BTCUSDT", "crypto", "1m");
    let quote = Instrument::quote("BTCUSDT", "crypto");
    let stream = NativeStream::new(binance(), 2);
    assert_eq!(stream.quote_source(&quote), Some("native"));
    let qc = stream.channel_of(&quote).unwrap();
    assert_ne!(qc, stream.channel_of(&bars).unwrap());

    let mut feed = NativeFeed::new(binance(), 2, std::slice::from_ref(&bars)).unwrap();
    let mut first = feed.subscribe(std::slice::from_ref(&bars));
    first.extend(feed.drain_sends());
    assert_eq!(params(&first), vec!["btcusdt@kline_1m"]);
    // A pane turning bid/ask on is one more stream on the socket already open.
    assert_eq!(params(&feed.subscribe(std::slice::from_ref(&quote))), vec!["btcusdt@bookTicker"]);

    let tick = r#"{"u":101628173415,"s":"BTCUSDT","b":"82280.00000000","B":"0.40541000","a":"82280.01000000","A":"3.91138000"}"#;
    let got = quotes(&feed.on_message(tick).unwrap());
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].0, qc);
    assert_eq!((got[0].1.bid, got[0].1.ask), (82280.0, 82280.01));
    // A crossed book is not drawn.
    let crossed = tick.replace("82280.01000000", "82279.00000000");
    assert!(quotes(&feed.on_message(&crossed).unwrap()).is_empty());

    // Turning it off drops the book ticker and nothing else.
    assert_eq!(params(&feed.unsubscribe(&[quote])), vec!["btcusdt@bookTicker"]);
    assert!(feed.on_message(tick).unwrap().is_empty());
}

#[test]
fn a_venue_with_no_quote_stream_refuses_one() {
    let driver = Arc::new(Driver::new(
        otw_connector_providers::yahoo::Yahoo::new().expect("caps"),
        DriverConfig::new("yahoo"),
        reqwest::Client::new(),
    ));
    let stream = NativeStream::new(driver, 2);
    let quote = Instrument::quote("AAPL", "equity");
    assert_eq!(stream.quote_source(&quote), None);
    assert!(stream.channel_of(&quote).is_err());
}

#[tokio::test]
async fn capitalcom_quotes_ride_the_candles_without_a_subscription() {
    use otw_connector_sdk::Mapper as _;
    use wiremock::{matchers::any, Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(200).set_body_string("{}").insert_header("CST", "c").insert_header("X-SECURITY-TOKEN", "s"))
        .mount(&server)
        .await;
    let driver = Arc::new(Driver::new(
        otw_connector_providers::capitalcom::CapitalCom::new().expect("caps"),
        DriverConfig::new("capitalcom").base("rest", &server.uri()),
        reqwest::Client::new(),
    ));
    let secrets: HashMap<String, String> =
        [("api_key", "k"), ("identifier", "quote-test"), ("api_password", "p")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    let signer = driver.mapper().signer(&secrets).unwrap().unwrap();
    driver.ensure_token(signer.as_ref()).await.unwrap();

    let bars = Instrument::new("GOLD", "fx", "5m");
    let quote = Instrument::quote("GOLD", "fx");
    let stream = NativeStream::new(driver.clone(), 2);
    assert_eq!(stream.quote_source(&quote), Some("candle_sides"));
    assert_eq!(stream.socket_class(&quote).unwrap(), stream.socket_class(&bars).unwrap(), "same socket");
    let qc = stream.channel_of(&quote).unwrap();

    let mut feed = NativeFeed::with_session(driver, 2, std::slice::from_ref(&bars), Some(signer), Default::default()).unwrap();
    feed.subscribe(std::slice::from_ref(&bars));
    feed.drain_sends();
    let ev = |side: &str, c: f64| {
        format!(r#"{{"status":"OK","destination":"ohlc.event","payload":{{"resolution":"MINUTE_5","epic":"GOLD","type":"classic","priceType":"{side}","t":1671714000000,"h":{},"l":{},"o":{c},"c":{c}}}}}"#, c + 1.0, c - 1.0)
    };
    // Nobody holds the quote yet: candles only.
    feed.on_message(&ev("bid", 1800.0)).unwrap();
    assert!(quotes(&feed.on_message(&ev("ask", 1800.5)).unwrap()).is_empty());

    // Holding it costs no frame upstream.
    assert!(feed.subscribe(std::slice::from_ref(&quote)).is_empty());
    let all = feed.on_message(&ev("bid", 1800.2)).unwrap();
    let got = quotes(&all);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].0, qc);
    assert_eq!((got[0].1.bid, got[0].1.ask), (1800.2, 1800.5));
    let got = quotes(&feed.on_message(&ev("offer", 1801.4)).unwrap());
    assert_eq!((got[0].1.bid, got[0].1.ask), (1800.2, 1801.4));
    // One side moving past the other's stale close reads crossed for an instant: dropped.
    assert!(quotes(&feed.on_message(&ev("bid", 1802.0)).unwrap()).is_empty());

    // Releasing it sends nothing either, and the quotes stop.
    assert!(feed.unsubscribe(&[quote]).is_empty());
    assert!(quotes(&feed.on_message(&ev("bid", 1801.1)).unwrap()).is_empty());
}

/// Alpaca's option socket: MessagePack, and prints rather than bars, so every timeframe
/// folds from the trades.
#[test]
fn an_alpaca_option_folds_from_prints_on_a_msgpack_socket() {
    use super::super::SourceGrain;
    let driver = Arc::new(Driver::new(
        otw_connector_providers::alpaca::Alpaca::new(otw_connector_providers::alpaca::Feed::Iex).expect("caps"),
        DriverConfig::new("alpaca"),
        reqwest::Client::new(),
    ));
    let stream = NativeStream::new(driver.clone(), 2);
    let opt = Instrument::new("SPY251219C00650000", "option", "5m");
    assert_eq!(stream.grain(&opt).unwrap(), SourceGrain::Trades);
    assert_ne!(stream.channel_of(&opt).unwrap(), stream.channel_of(&Instrument::quote("SPY251219C00650000", "option")).unwrap());
    assert!(matches!(stream.grain(&Instrument::new("AAPL", "equity", "5m")).unwrap(), SourceGrain::Bars { secs: 60, .. }));
    let feed = NativeFeed::new(driver.clone(), 2, &[opt]).unwrap();
    assert!(feed.msgpack(), "the option socket asks for MessagePack");
    assert!(feed.url().ends_with("/v1beta1/indicative"));
    let equity = NativeFeed::new(driver, 2, &[Instrument::new("AAPL", "equity", "1m")]).unwrap();
    assert!(!equity.msgpack());
}

/// An Alpaca option pane, end to end through the feed: after the login the prints and the
/// quote are subscribed, a print reaches the trades channel the hub folds, a quote the quote
/// channel. Messages as the transport hands them over once transcoded to JSON.
#[test]
fn an_alpaca_option_pane_takes_prints_and_quotes() {
    use otw_connector_sdk::Mapper as _;
    let driver = Arc::new(Driver::new(
        otw_connector_providers::alpaca::Alpaca::new(otw_connector_providers::alpaca::Feed::Sip).expect("caps"),
        DriverConfig::new("alpaca"),
        reqwest::Client::new(),
    ));
    let secrets: HashMap<String, String> = [("api_key".to_string(), "k".to_string()), ("api_secret".to_string(), "s".to_string())].into();
    let signer = driver.mapper().signer(&secrets).unwrap();
    let sym = "SPY251219C00650000";
    let bars = Instrument::new(sym, "option", "1m");
    let quote = Instrument::quote(sym, "option");
    let stream = NativeStream::new(driver.clone(), 4);
    let (trades_ch, quote_ch) = (stream.channel_of(&bars).unwrap(), stream.channel_of(&quote).unwrap());
    let mut feed = NativeFeed::new(driver, 4, &[bars.clone(), quote.clone()]).unwrap().with_signer(signer);
    assert!(feed.msgpack() && feed.url().ends_with("/v1beta1/opra"));
    let mut sent = feed.subscribe(&[bars, quote]);
    sent.extend(feed.drain_sends());
    assert!(sent.is_empty(), "nothing before the login");
    for m in [r#"[{"T":"success","msg":"connected"}]"#, r#"[{"T":"success","msg":"authenticated"}]"#] {
        feed.on_message(m).unwrap();
        sent.extend(feed.drain_sends());
    }
    let sent: Vec<serde_json::Value> = sent.iter().map(|f| serde_json::from_str(f).unwrap()).collect();
    assert!(sent.contains(&serde_json::json!({"action":"subscribe","trades":[sym]})), "{sent:?}");
    assert!(sent.contains(&serde_json::json!({"action":"subscribe","quotes":[sym]})), "{sent:?}");
    let print = format!(r#"[{{"T":"t","S":"{sym}","t":"2026-10-19T14:30:01.5Z","p":9.5,"s":2,"x":"C","c":"I"}}]"#);
    let up = feed.on_message(&print).unwrap();
    assert!(matches!(&up[..], [u] if u.channel == trades_ch && matches!(u.payload, Payload::Trade { price, size, .. } if price == 9.5 && size == 2.0)));
    let q = format!(r#"[{{"T":"q","S":"{sym}","t":"2026-10-19T14:30:01.6Z","bp":9.46,"bs":53,"ap":9.66,"as":38,"c":"A"}}]"#);
    let got = quotes(&feed.on_message(&q).unwrap());
    assert_eq!(got.len(), 1);
    assert_eq!((got[0].0.as_str(), got[0].1.bid, got[0].1.ask), (quote_ch.as_str(), 9.46, 9.66));
}
