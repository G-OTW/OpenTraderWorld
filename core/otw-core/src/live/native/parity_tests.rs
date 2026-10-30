//! Non-regression for the live feeds: each venue's feed writes its frames and turns the
//! venue's messages into updates, checked against the reference recorded when the legacy
//! feeds still ran beside them and answered the same (see `golden`). Synthetic messages,
//! written from the venue's documented schema.

use std::collections::HashMap;
use std::sync::Arc;

use futures::{SinkExt, StreamExt};
use otw_connector_providers::binance::Binance;
use otw_connector_sdk::{Driver, DriverConfig};
use tokio_tungstenite::tungstenite::Message;

use super::super::{run_feed, Feed, FeedUpdate, Instrument, LiveEvent, Payload, StreamConnector};
use super::{NativeFeed, NativeStream};

fn driver(ws_base: Option<&str>) -> Arc<Driver<Binance>> {
    let mut cfg = DriverConfig::new("binance");
    if let Some(b) = ws_base {
        cfg = cfg.base("ws", b);
    }
    Arc::new(Driver::new(Binance::spot().expect("caps"), cfg, reqwest::Client::new()))
}

fn usdm_driver() -> Arc<Driver<Binance>> {
    Arc::new(Driver::new(Binance::usdm().expect("caps"), DriverConfig::new("binance_futures"), reqwest::Client::new()))
}

#[test]
fn binance_futures_frames_and_updates_match_the_legacy_feed() {
    let mut g = crate::histdata::native::golden::Golden::new("live/binance_futures_frames_and_updates_match_the_legacy_feed");
    let opening = insts();
    let native_stream = NativeStream::new(usdm_driver(), 2);
    let mut native = NativeFeed::new(usdm_driver(), 2, &opening).unwrap();
    g.check("native.url()", &native.url());
    let mut first = native.subscribe(&opening);
    first.extend(native.drain_sends());
    g.check(format!("opening frames"), &json(&first));
    let sol = [Instrument::new("SOLUSDT", "crypto", "5m")];
    g.check("json(&native.subscribe(&sol))", &json(&native.subscribe(&sol)));
    g.check("json(&native.unsubscribe(&sol))", &json(&native.unsubscribe(&sol)));
    let mut native_map: HashMap<String, Vec<String>> = HashMap::new();
    for i in &opening {
        native_map.entry(native_stream.channel_of(i).unwrap()).or_default().push(i.key());
    }
    let mut b = Vec::new();
    for i in 0..30 {
        let open = 1_700_000_040_000 + (i / 3) * 60_000;
        for m in [
            kline("BTCUSDT", "1m", open, Some(open + 999 + i * 17), i % 3 == 2, i),
            kline("ETHUSDT", "1h", 1_699_999_200_000, Some(1_700_000_000_000 + i * 1_000), false, i),
        ] {
            b.extend(native.on_message(&m).unwrap());
        }
    }
    g.check("binance futures klines", &events(&by_instrument(b, &native_map)));
    // Futures stream no 1s klines: refused rather than approximated, as before.
    assert!(native_stream.channel_of(&Instrument::new("BTCUSDT", "crypto", "1s")).is_err() || native_stream.grain(&Instrument::new("BTCUSDT", "crypto", "1s")).is_err());
}

fn insts() -> Vec<Instrument> {
    vec![
        Instrument::new("BTCUSDT", "crypto", "1m"),
        Instrument::new("ETHUSDT", "crypto", "1h"),
        Instrument::new("BTCUSDT", "crypto", "1h"),
    ]
}

fn json(frames: &[String]) -> Vec<serde_json::Value> {
    frames.iter().map(|f| serde_json::from_str(f).expect("frame is json")).collect()
}

/// Updates keyed by instrument instead of by each side's own channel spelling.
fn by_instrument(updates: Vec<FeedUpdate>, channel_to_inst: &HashMap<String, Vec<String>>) -> Vec<(String, LiveEvent)> {
    let mut out = Vec::new();
    for u in updates {
        let Payload::Bar(e) = u.payload else { continue };
        // Only channels a pane holds: the legacy feed also passes bars of streams nobody
        // asked for, which the hub then drops.
        for inst in channel_to_inst.get(&u.channel).into_iter().flatten() {
            out.push((inst.clone(), e.clone()));
        }
    }
    out.sort_by(|a, b| (a.0.as_str(), a.1.bar_ts).cmp(&(b.0.as_str(), b.1.bar_ts)));
    out
}

/// An update as the reference keeps it: its instrument, its stamps, its closed flag and its
/// prices and volume bit for bit.
fn events(v: &[(String, LiveEvent)]) -> Vec<String> {
    v.iter()
        .map(|(i, x)| format!("{i}|{}|{}|{}|{}|{}|{}|{}|{}", x.bar_ts, x.provider_ts, x.closed, x.open.to_bits(), x.high.to_bits(), x.low.to_bits(), x.close.to_bits(), x.volume.to_bits()))
        .collect()
}

fn kline(symbol: &str, interval: &str, open_ms: i64, event_ms: Option<i64>, closed: bool, i: i64) -> String {
    let e = event_ms.map(|e| format!(r#""E":{e},"#)).unwrap_or_default();
    format!(
        r#"{{"e":"kline",{e}"s":"{symbol}","k":{{"t":{open_ms},"T":{},"s":"{symbol}","i":"{interval}","o":"{}.10","c":"{}.25000000","h":"{}.9","l":"{}.00000001","v":"{}.{:08}","n":7,"x":{closed},"q":"1.5"}}}}"#,
        open_ms + 59_999,
        30_000 + i,
        30_000 + i,
        30_001 + i,
        29_999 + i,
        i % 13,
        (i * 7_919) % 100_000_000
    )
}

#[test]
fn binance_frames_and_updates_match_the_legacy_feed() {
    let mut g = crate::histdata::native::golden::Golden::new("live/binance_frames_and_updates_match_the_legacy_feed");
    let opening = insts();
    let native_stream = NativeStream::new(driver(None), 2);
    let mut native = NativeFeed::new(driver(None), 2, &opening).unwrap();

    // The same socket, opened with the same streams.
    g.check("native.url()", &native.url());
    let mut first = native.subscribe(&opening);
    first.extend(native.drain_sends());
    g.check(format!("opening frames"), &json(&first));

    // Joining and leaving: the same frames, and nothing for a timeframe already carried.
    let sol = [Instrument::new("SOLUSDT", "crypto", "5m")];
    g.check("json(&native.subscribe(&sol))", &json(&native.subscribe(&sol)));
    g.check("json(&native.unsubscribe(&sol))", &json(&native.unsubscribe(&sol)));

    let mut native_map: HashMap<String, Vec<String>> = HashMap::new();
    for i in &opening {
        native_map.entry(native_stream.channel_of(i).unwrap()).or_default().push(i.key());
    }

    let mut messages = vec![
        r#"{"result":null,"id":2}"#.to_string(),
        r#"{"result":null,"id":3}"#.to_string(),
        kline("SOLUSDT", "5m", 1_700_000_100_000, Some(1_700_000_101_000), false, 1),
    ];
    for i in 0..60 {
        let open = 1_700_000_040_000 + (i / 3) * 60_000;
        messages.push(kline("BTCUSDT", "1m", open, Some(open + 999 + i * 17), i % 3 == 2, i));
        messages.push(kline("ETHUSDT", "1h", 1_699_999_200_000, Some(1_700_000_000_000 + i * 1_000), false, i));
        messages.push(kline("BTCUSDT", "1h", 1_699_999_200_000, if i % 5 == 0 { None } else { Some(1_700_000_000_500 + i) }, i == 59, i));
    }
    let mut b = Vec::new();
    for m in &messages {
        b.extend(native.on_message(m).unwrap());
    }
    g.check("binance klines", &events(&by_instrument(b, &native_map)));

    // A frame that is not JSON ends the session on both sides (the supervisor reconnects).
    assert!(native.on_message("{garbage").is_err());
}

/// The one deliberate difference: a stream the venue refuses is reported on that instrument
/// (the pane's banner), where the legacy feed swallowed the refusal and left it waiting.
#[test]
fn binance_refused_stream_is_reported_on_its_instrument() {
    let opening = [Instrument::new("NOPEUSDT", "crypto", "1m")];
    let mut native = NativeFeed::new(driver(None), 2, &opening).unwrap();
    let ups = native.on_message(r#"{"code":2,"msg":"Invalid request: unknown stream","id":2}"#).unwrap();
    assert_eq!(ups.len(), 1);
    assert!(matches!(ups[0].payload, Payload::Fault { .. }));
}

#[test]
fn binance_grain_and_channels_follow_the_hub_contract() {
    let s = NativeStream::new(driver(None), 2);
    for tf in ["1s", "1m", "5m", "15m", "1h", "4h", "1d", "1w"] {
        let i = Instrument::new("BTCUSDT", "crypto", tf);
        assert_eq!(s.grain(&i).unwrap(), super::super::SourceGrain::Native, "{tf}");
    }
    let a = Instrument::new("BTCUSDT", "crypto", "1m");
    let b = Instrument::new("BTCUSDT", "crypto", "1h");
    assert_ne!(s.channel_of(&a).unwrap(), s.channel_of(&b).unwrap(), "one channel per interval");
    // A resolution Binance does not stream natively folds from the largest one that tiles it.
    assert_eq!(
        s.grain(&Instrument::new("BTCUSDT", "crypto", "3m")).unwrap(),
        super::super::SourceGrain::Bars { secs: 60, grace: 2 }
    );
    assert!(s.channel_of(&Instrument::new("BTC/USDT", "crypto", "1m")).is_err());
}

/// End to end through the real socket loop, against a local server answering like Binance.
#[tokio::test]
async fn binance_native_feed_runs_through_the_socket_loop() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (path_tx, path_rx) = tokio::sync::oneshot::channel::<(String, String)>();
    tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut path = String::new();
        let cb = |req: &tokio_tungstenite::tungstenite::handshake::server::Request, resp| {
            path = req.uri().path().to_string();
            Ok(resp)
        };
        let mut ws = tokio_tungstenite::accept_hdr_async(tcp, cb).await.unwrap();
        let first = match ws.next().await {
            Some(Ok(Message::Text(t))) => t.to_string(),
            other => panic!("expected the subscribe frame, got {other:?}"),
        };
        let _ = path_tx.send((path, first));
        ws.send(Message::Text(r#"{"result":null,"id":2}"#.into())).await.unwrap();
        ws.send(Message::Text(kline("BTCUSDT", "1m", 1_700_000_040_000, Some(1_700_000_041_000), false, 1).into())).await.unwrap();
        ws.send(Message::Text(kline("BTCUSDT", "1m", 1_700_000_040_000, Some(1_700_000_099_999), true, 2).into())).await.unwrap();
        let _ = ws.close(None).await;
    });
    let base = format!("ws://{addr}");
    let opening = [Instrument::new("BTCUSDT", "crypto", "1m")];
    let feed = NativeFeed::new(driver(Some(&base)), 2, &opening).unwrap();
    let session = run_feed(feed, &opening).await.unwrap();
    let (path, first) = path_rx.await.unwrap();
    assert_eq!(path, "/ws/btcusdt@kline_1m");
    assert_eq!(serde_json::from_str::<serde_json::Value>(&first).unwrap(), serde_json::json!({"method":"SUBSCRIBE","params":["btcusdt@kline_1m"],"id":2}));
    let updates: Vec<_> = session.stream.collect().await;
    let bars: Vec<LiveEvent> = updates
        .into_iter()
        .filter_map(|u| match u.unwrap().payload {
            Payload::Bar(e) => Some(e),
            _ => None,
        })
        .collect();
    assert_eq!(bars.len(), 2);
    assert!(!bars[0].closed && bars[1].closed);
    assert_eq!(bars[1].bar_ts.unix_timestamp(), 1_700_000_040);
}

/// Shadow against the real venue: every frame of one real socket is fed to both feeds and
/// their updates compared. Nothing is stored. Skipped unless `OTW_LIVE_TEST=1` or
/// `OTW_NET_TEST=1`.
#[tokio::test]
async fn shadow_binance_live_against_the_real_venue() {
    let on = |k: &str| std::env::var(k).as_deref() == Ok("1");
    if !on("OTW_LIVE_TEST") && !on("OTW_NET_TEST") {
        return;
    }
    let opening = vec![
        Instrument::new("BTCUSDT", "crypto", "1s"),
        Instrument::new("ETHUSDT", "crypto", "1s"),
        Instrument::new("BTCUSDT", "crypto", "1m"),
    ];
    let native_stream = NativeStream::new(driver(None), 2);
    let mut native = NativeFeed::new(driver(None), 2, &opening).unwrap();
    let mut first = native.subscribe(&opening);
    first.extend(native.drain_sends());
    let (mut ws, _) = tokio_tungstenite::connect_async(native.url()).await.unwrap();
    for f in first {
        ws.send(Message::Text(f.into())).await.unwrap();
    }
    let mut native_map: HashMap<String, Vec<String>> = HashMap::new();
    for i in &opening {
        native_map.entry(native_stream.channel_of(i).unwrap()).or_default().push(i.key());
    }
    let mut b = Vec::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(15);
    let mut frames = 0;
    while let Ok(Some(Ok(msg))) = tokio::time::timeout_at(deadline, ws.next()).await {
        if let Message::Text(t) = msg {
            frames += 1;
            b.extend(native.on_message(t.as_str()).unwrap());
        }
    }
    let b = by_instrument(b, &native_map);
    assert!(b.len() > 10, "too few real updates: {}", b.len());
    let _ = &b;
    assert!(b.iter().any(|(_, e)| e.closed), "a closed 1s bar in 15 s");
    eprintln!("shadow live: {frames} frames, {} updates", b.len());
}

// ── OKX ──────────────────────────────────────────────────────────────────────

fn okx_driver() -> Arc<Driver<otw_connector_providers::okx::Okx>> {
    Arc::new(Driver::new(otw_connector_providers::okx::Okx::new().expect("caps"), DriverConfig::new("okx"), reqwest::Client::new()))
}

fn okx_row(open_ms: i64, confirm: &str, i: i64) -> String {
    format!(r#"["{open_ms}","{}.1","{}.9","{}.01","{}.5","{}.{:04}","1","2","{confirm}"]"#, 50_000 + i, 50_001 + i, 49_999 + i, 50_000 + i, i % 7, (i * 13) % 10_000)
}

#[test]
fn okx_frames_and_updates_match_the_legacy_feed() {
    let mut g = crate::histdata::native::golden::Golden::new("live/okx_frames_and_updates_match_the_legacy_feed");
    let opening = vec![
        Instrument::new("BTC-USDT", "crypto", "1m"),
        Instrument::new("ETH-USDT-SWAP", "crypto", "1d"),
        Instrument::new("BTC-USDT", "crypto", "1h"),
    ];
    let native_stream = NativeStream::new(okx_driver(), 2);
    let mut native = NativeFeed::new(okx_driver(), 2, &opening).unwrap();
    g.check("native.url()", &native.url());
    let mut first = native.subscribe(&opening);
    first.extend(native.drain_sends());
    g.check(format!("opening frames"), &json(&first));
    let sol = [Instrument::new("SOL-USDT", "crypto", "5m")];
    g.check("json(&native.subscribe(&sol))", &json(&native.subscribe(&sol)));
    g.check("json(&native.unsubscribe(&sol))", &json(&native.unsubscribe(&sol)));

    let mut native_map: HashMap<String, Vec<String>> = HashMap::new();
    for i in &opening {
        native_map.entry(native_stream.channel_of(i).unwrap()).or_default().push(i.key());
    }
    let frame = |ch: &str, inst: &str, rows: &[String]| format!(r#"{{"arg":{{"channel":"{ch}","instId":"{inst}"}},"data":[{}]}}"#, rows.join(","));
    let mut messages = vec![
        r#"{"event":"subscribe","arg":{"channel":"candle1m","instId":"BTC-USDT"},"connId":"a"}"#.to_string(),
        // A subscription replays recent candles, several per frame.
        frame("candle1m", "BTC-USDT", &[okx_row(1_700_000_040_000, "1", 1), okx_row(1_700_000_100_000, "0", 2)]),
    ];
    for i in 0..40 {
        let open = 1_700_000_100_000 + (i / 4) * 60_000;
        messages.push(frame("candle1m", "BTC-USDT", &[okx_row(open, if i % 4 == 3 { "1" } else { "0" }, i)]));
        messages.push(frame("candle1Dutc", "ETH-USDT-SWAP", &[okx_row(1_699_920_000_000, "0", i)]));
        messages.push(frame("candle1H", "BTC-USDT", &[okx_row(1_699_999_200_000 + (i / 20) * 3_600_000, "0", i)]));
        // A replayed older row: not news on either side.
        messages.push(frame("candle1m", "BTC-USDT", &[okx_row(1_700_000_040_000, "1", 99)]));
    }
    messages.push("pong".into());
    let mut b = Vec::new();
    for m in &messages {
        b.extend(native.on_message(m).unwrap());
    }
    // OKX rows carry no event time: both stamp them when heard, so provider_ts is compared
    // to the second it was read, not bit for bit.
    let strip = |v: Vec<(String, LiveEvent)>| v.into_iter().map(|(k, mut e)| { e.provider_ts = time::OffsetDateTime::UNIX_EPOCH; (k, e) }).collect::<Vec<_>>();
    g.check("okx candles", &events(&strip(by_instrument(b, &native_map))));

    // Refusals: routed to the pane named in the sentence, the socket's otherwise.
    let refused = r#"{"event":"error","msg":"Subscribe failed, wrong URL or channel:candle1Dutc,instId:ETH-USDT-SWAP doesn't exist.","code":"60018"}"#;
    let rb = native.on_message(refused).unwrap();
    g.check("(1, 1)", &(1, 1));
    assert!(matches!((&rb[0].payload, &rb[0].payload), (Payload::Fault { fault: x, .. }, Payload::Fault { fault: y, .. }) if x == y));
    assert!(native.on_message(r#"{"event":"error","msg":"Wrong URL","code":"60012"}"#).is_err());
    assert!(native.on_message("{garbage").is_err());
}

#[tokio::test]
async fn shadow_okx_live_against_the_real_venue() {
    let on = |k: &str| std::env::var(k).as_deref() == Ok("1");
    if !on("OTW_LIVE_TEST") && !on("OTW_NET_TEST") {
        return;
    }
    let opening = vec![Instrument::new("BTC-USDT", "crypto", "1m"), Instrument::new("ETH-USDT-SWAP", "crypto", "1m")];
    let native_stream = NativeStream::new(okx_driver(), 2);
    let mut native = NativeFeed::new(okx_driver(), 2, &opening).unwrap();
    let mut first = native.subscribe(&opening);
    first.extend(native.drain_sends());
    let (mut ws, _) = tokio_tungstenite::connect_async(native.url()).await.unwrap();
    for f in first {
        ws.send(Message::Text(f.into())).await.unwrap();
    }
    let mut native_map: HashMap<String, Vec<String>> = HashMap::new();
    for i in &opening {
        native_map.entry(native_stream.channel_of(i).unwrap()).or_default().push(i.key());
    }
    let mut b = Vec::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(15);
    while let Ok(Some(Ok(msg))) = tokio::time::timeout_at(deadline, ws.next()).await {
        if let Message::Text(t) = msg {
            b.extend(native.on_message(t.as_str()).unwrap());
        }
    }
    let strip = |v: Vec<(String, LiveEvent)>| v.into_iter().map(|(k, mut e)| { e.provider_ts = time::OffsetDateTime::UNIX_EPOCH; (k, e) }).collect::<Vec<_>>();
    let b = strip(by_instrument(b, &native_map));
    assert!(!b.is_empty(), "no real OKX candle in 15 s");
    let _ = &b;
    eprintln!("shadow okx live: {} updates", b.len());
}

// ── Bitget ───────────────────────────────────────────────────────────────────

#[test]
fn bitget_frames_and_updates_match_the_legacy_feed_on_both_books() {
    let mut g = crate::histdata::native::golden::Golden::new("live/bitget_frames_and_updates_match_the_legacy_feed_on_both_books");
    use otw_connector_providers::bitget::Bitget;
    for (book, inst_type) in [(Bitget::spot().unwrap(), "SPOT"), (Bitget::usdt_futures().unwrap(), "USDT-FUTURES")] {
        let d = Arc::new(Driver::new(book, DriverConfig::new("bitget"), reqwest::Client::new()));
        let opening = vec![Instrument::new("BTCUSDT", "crypto", "1m"), Instrument::new("ETHUSDT", "crypto", "1d")];
        let native_stream = NativeStream::new(d.clone(), 2);
        let mut native = NativeFeed::new(d.clone(), 2, &opening).unwrap();
        g.check("native.url()", &native.url());
        let mut first = native.subscribe(&opening);
        first.extend(native.drain_sends());
        g.check(format!("{inst_type} opening frames"), &json(&first));
        let mut native_map: HashMap<String, Vec<String>> = HashMap::new();
        for i in &opening {
            native_map.entry(native_stream.channel_of(i).unwrap()).or_default().push(i.key());
        }
        let frame = |ch: &str, inst: &str, open: i64, i: i64| {
            format!(
                r#"{{"action":"update","arg":{{"instType":"{inst_type}","channel":"{ch}","instId":"{inst}"}},"data":[["{open}","{}.1","{}.9","{}.01","{}.5","{}.{:03}","1","1"]],"ts":"{}"}}"#,
                30_000 + i, 30_001 + i, 29_999 + i, 30_000 + i, i % 9, (i * 7) % 1000, open + 500 + i
            )
        };
        let mut messages = Vec::new();
        for i in 0..40 {
            messages.push(frame("candle1m", "BTCUSDT", 1_790_014_500_000 + (i / 4) * 60_000, i));
            messages.push(frame("candle1Dutc", "ETHUSDT", 1_789_948_800_000, i));
            // A snapshot row older than the forming candle: history, not news.
            messages.push(frame("candle1m", "BTCUSDT", 1_790_014_440_000, 99));
        }
        let mut b = Vec::new();
        for m in &messages {
            b.extend(native.on_message(m).unwrap());
        }
        g.check(format!("bitget {inst_type}"), &events(&by_instrument(b, &native_map)));
        let refused = format!(r#"{{"event":"error","arg":{{"instType":"{inst_type}","channel":"candle1m","instId":"BTCUSDT"}},"code":30001,"msg":"instId:BTCUSDT doesn't exist"}}"#);
        let rb = native.on_message(&refused).unwrap();
        assert!(matches!((&rb[0].payload, &rb[0].payload), (Payload::Fault { fault: x, .. }, Payload::Fault { fault: y, .. }) if x == y));
        assert!(native.on_message(r#"{"event":"error","msg":"bad op","code":30002}"#).is_err());
    }
}

#[tokio::test]
async fn shadow_bitget_live_against_the_real_venue() {
    let on = |k: &str| std::env::var(k).as_deref() == Ok("1");
    if !on("OTW_LIVE_TEST") && !on("OTW_NET_TEST") {
        return;
    }
    let d = Arc::new(Driver::new(otw_connector_providers::bitget::Bitget::spot().unwrap(), DriverConfig::new("bitget"), reqwest::Client::new()));
    let opening = vec![Instrument::new("BTCUSDT", "crypto", "1m"), Instrument::new("ETHUSDT", "crypto", "1m")];
    let native_stream = NativeStream::new(d.clone(), 2);
    let mut native = NativeFeed::new(d, 2, &opening).unwrap();
    let mut first = native.subscribe(&opening);
    first.extend(native.drain_sends());
    let (mut ws, _) = tokio_tungstenite::connect_async(native.url()).await.unwrap();
    for f in first {
        ws.send(Message::Text(f.into())).await.unwrap();
    }
    let mut native_map: HashMap<String, Vec<String>> = HashMap::new();
    for i in &opening {
        native_map.entry(native_stream.channel_of(i).unwrap()).or_default().push(i.key());
    }
    let mut b = Vec::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(15);
    while let Ok(Some(Ok(msg))) = tokio::time::timeout_at(deadline, ws.next()).await {
        if let Message::Text(t) = msg {
            b.extend(native.on_message(t.as_str()).unwrap());
        }
    }
    // A snapshot frame carries no `ts`: each side stamps it with its own clock read, which
    // only feeds the lag gauge. Frames that carry one are compared exactly offline.
    let secs = |v: Vec<(String, LiveEvent)>| v.into_iter().map(|(k, mut e)| { e.provider_ts = time::OffsetDateTime::UNIX_EPOCH; (k, e) }).collect::<Vec<_>>();
    let b = secs(by_instrument(b, &native_map));
    assert!(!b.is_empty(), "no real Bitget candle in 15 s");
    let _ = &b;
    eprintln!("shadow bitget live: {} updates", b.len());
}

// ── Kraken ───────────────────────────────────────────────────────────────────

fn kraken_driver() -> Arc<Driver<otw_connector_providers::kraken::Kraken>> {
    Arc::new(Driver::new(otw_connector_providers::kraken::Kraken::new().expect("caps"), DriverConfig::new("kraken"), reqwest::Client::new()))
}

#[test]
fn kraken_live_compact_altname_resolves_from_loaded_catalog() {
    use otw_connector_model::InstrumentSpec;
    use otw_connector_sdk::InstrumentCache;

    let driver = kraken_driver();
    let mut catalog = InstrumentCache::new();
    let id = otw_connector_sdk::Mapper::instrument_id(driver.mapper(), "SOLUSD").unwrap();
    catalog.insert(InstrumentSpec::spot(id, "SOL", "USD"));
    driver.set_instruments(catalog).unwrap();

    let opening = [Instrument::new("SOLUSD", "crypto", "1m")];
    let mut feed = NativeFeed::new(driver, 2, &opening).unwrap();
    let frames = feed.drain_sends();
    assert_eq!(frames.len(), 1, "one subscription frame");
    let subscribe: serde_json::Value = serde_json::from_str(&frames[0]).unwrap();
    assert_eq!(subscribe["params"]["symbol"], serde_json::json!(["SOL/USD"]));
}

fn kraken_row(symbol: &str, begin_s: i64, at: &str, i: i64) -> String {
    let begin = time::OffsetDateTime::from_unix_timestamp(begin_s).unwrap().format(&time::format_description::well_known::Rfc3339).unwrap();
    format!(
        r#"{{"symbol":"{symbol}","open":{}.1,"high":{}.9,"low":{}.01,"close":{}.5,"trades":{i},"volume":{}.{:05},"vwap":1.5,"interval_begin":"{begin}","interval":1,"timestamp":"{at}"}}"#,
        50_000 + i, 50_001 + i, 49_999 + i, 50_000 + i, i % 7, (i * 13) % 100_000
    )
}

#[test]
fn kraken_frames_and_updates_match_the_legacy_feed() {
    let mut g = crate::histdata::native::golden::Golden::new("live/kraken_frames_and_updates_match_the_legacy_feed");
    let opening = vec![Instrument::new("BTC/USD", "crypto", "1m"), Instrument::new("ETH/USD", "crypto", "1m")];
    let native_stream = NativeStream::new(kraken_driver(), 2);
    // The same socket per interval: two timeframes are two sockets on both sides.
    for tf in ["1m", "5m", "15m", "1h", "4h", "1d", "1w"] {
        let i = Instrument::new("BTC/USD", "crypto", tf);
        g.check(format!("{tf}"), &native_stream.socket_class(&i).unwrap());
        g.check(format!("{tf}"), &native_stream.grain(&i).unwrap());
    }
    let mut native = NativeFeed::new(kraken_driver(), 2, &opening).unwrap();
    g.check("native.url()", &native.url());
    let mut first = native.subscribe(&opening);
    first.extend(native.drain_sends());
    g.check(format!("opening frames"), &json(&first));
    let sol = [Instrument::new("SOL/USD", "crypto", "1m")];
    g.check("json(&native.subscribe(&sol))", &json(&native.subscribe(&sol)));
    g.check("json(&native.unsubscribe(&sol))", &json(&native.unsubscribe(&sol)));

    let mut native_map: HashMap<String, Vec<String>> = HashMap::new();
    for i in &opening {
        native_map.entry(native_stream.channel_of(i).unwrap()).or_default().push(i.key());
    }
    let frame = |kind: &str, rows: &[String]| format!(r#"{{"channel":"ohlc","type":"{kind}","data":[{}]}}"#, rows.join(","));
    let t0 = 1_788_602_400;
    let mut messages = vec![
        r#"{"method":"subscribe","result":{"channel":"ohlc","symbol":"BTC/USD","interval":1,"snapshot":true},"success":true,"time_in":"a","time_out":"b"}"#.to_string(),
        // The snapshot: recent candles, several per frame, then updates.
        frame("snapshot", &[kraken_row("BTC/USD", t0 - 120, "2026-09-05T09:58:59Z", 1), kraken_row("BTC/USD", t0 - 60, "2026-09-05T09:59:59Z", 2), kraken_row("BTC/USD", t0, "2026-09-05T10:00:01Z", 3)]),
        frame("snapshot", &[kraken_row("ETH/USD", t0, "2026-09-05T10:00:02Z", 4)]),
        r#"{"channel":"heartbeat"}"#.to_string(),
    ];
    for i in 0..40 {
        let begin = t0 + (i / 4) * 60;
        let at = format!("2026-09-05T10:{:02}:{:02}Z", (i / 4) % 60, 10 + i % 40);
        messages.push(frame("update", &[kraken_row("BTC/USD", begin, &at, i)]));
        messages.push(frame("update", &[kraken_row("ETH/USD", t0 + (i / 20) * 60, &at, i)]));
        // An older row: dropped on both sides.
        messages.push(frame("update", &[kraken_row("BTC/USD", t0 - 120, &at, 99)]));
    }
    let mut b = Vec::new();
    for m in &messages {
        b.extend(native.on_message(m).unwrap());
    }
    g.check("kraken candles", &events(&by_instrument(b, &native_map)));

    // The venue's time keeps its fraction, as legacy did, so the lag reads to the millisecond.
    let fraction = frame("update", &[kraken_row("BTC/USD", t0 + 3_600, "2026-09-05T11:00:00.75Z", 7)]);
    let rb = native.on_message(&fraction).unwrap();
    let ts = |u: &[FeedUpdate]| match &u.last().unwrap().payload {
        Payload::Bar(e) => e.provider_ts,
        _ => panic!(),
    };
    g.check("ts(&rb).unix_timestamp()", &ts(&rb).unix_timestamp());
    assert_eq!(ts(&rb).nanosecond(), 750_000_000);

    // Deliberate difference: a refused pair is reported on its pane (legacy ignored it and
    // the chart waited for data that never came); a refusal naming nothing is the socket's.
    let refused = r#"{"method":"subscribe","success":false,"error":"Currency pair not supported ETH/USD","symbol":"ETH/USD","time_in":"a","time_out":"b"}"#;
    let rb = native.on_message(refused).unwrap();
    assert!(matches!(&rb[..], [u] if matches!(&u.payload, Payload::Fault { .. }) && native_map.contains_key(&u.channel)));
    assert!(native.on_message(r#"{"method":"subscribe","success":false,"error":"Malformed request"}"#).is_err());
    assert!(native.on_message("{garbage").is_err());
}

#[tokio::test]
async fn shadow_kraken_live_against_the_real_venue() {
    let on = |k: &str| std::env::var(k).as_deref() == Ok("1");
    if !on("OTW_LIVE_TEST") && !on("OTW_NET_TEST") {
        return;
    }
    let opening = vec![Instrument::new("BTC/USD", "crypto", "1m"), Instrument::new("ETH/USD", "crypto", "1m")];
    let native_stream = NativeStream::new(kraken_driver(), 2);
    let mut native = NativeFeed::new(kraken_driver(), 2, &opening).unwrap();
    let mut first = native.subscribe(&opening);
    first.extend(native.drain_sends());
    let (mut ws, _) = tokio_tungstenite::connect_async(native.url()).await.unwrap();
    for f in first {
        ws.send(Message::Text(f.into())).await.unwrap();
    }
    let mut native_map: HashMap<String, Vec<String>> = HashMap::new();
    for i in &opening {
        native_map.entry(native_stream.channel_of(i).unwrap()).or_default().push(i.key());
    }
    let mut b = Vec::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(15);
    while let Ok(Some(Ok(msg))) = tokio::time::timeout_at(deadline, ws.next()).await {
        if let Message::Text(t) = msg {
            b.extend(native.on_message(t.as_str()).unwrap());
        }
    }
    let b = by_instrument(b, &native_map);
    assert!(!b.is_empty(), "no real Kraken candle in 15 s");
    let _ = &b;
    eprintln!("shadow kraken live: {} updates", b.len());
}

// ── Coinbase ─────────────────────────────────────────────────────────────────

fn coinbase_driver() -> Arc<Driver<otw_connector_providers::coinbase::Coinbase>> {
    Arc::new(Driver::new(otw_connector_providers::coinbase::Coinbase::new().expect("caps"), DriverConfig::new("coinbase"), reqwest::Client::new()))
}

/// Trades keyed by instrument instead of by each side's own channel spelling.
fn trades_by_instrument(updates: Vec<FeedUpdate>, channel_to_inst: &HashMap<String, Vec<String>>) -> Vec<(String, u64, u64, time::OffsetDateTime)> {
    let mut out = Vec::new();
    for u in updates {
        let Payload::Trade { price, size, ts } = u.payload else { continue };
        for inst in channel_to_inst.get(&u.channel).into_iter().flatten() {
            out.push((inst.clone(), price.to_bits(), size.to_bits(), ts));
        }
    }
    out
}

#[test]
fn coinbase_frames_and_trades_match_the_legacy_feed() {
    let mut g = crate::histdata::native::golden::Golden::new("live/coinbase_frames_and_trades_match_the_legacy_feed");
    let opening = vec![
        Instrument::new("BTC-USD", "crypto", "1m"),
        Instrument::new("ETH-EUR", "crypto", "1h"),
        Instrument::new("BTC-USD", "crypto", "4h"),
    ];
    let native_stream = NativeStream::new(coinbase_driver(), 2);
    // Every timeframe folds from the trades, on both sides, on one socket.
    for tf in ["1m", "5m", "15m", "1h", "4h", "1d", "1w"] {
        let i = Instrument::new("BTC-USD", "crypto", tf);
        g.check(format!("{tf}"), &native_stream.grain(&i).unwrap());
        g.check(format!("{tf}"), &native_stream.socket_class(&i).unwrap());
    }
    let mut native = NativeFeed::new(coinbase_driver(), 2, &opening).unwrap();
    g.check("native.url()", &native.url());
    let mut first = native.subscribe(&opening);
    first.extend(native.drain_sends());
    g.check(format!("opening frames"), &json(&first));
    let sol = [Instrument::new("SOL-USD", "crypto", "5m")];
    g.check("json(&native.subscribe(&sol))", &json(&native.subscribe(&sol)));
    g.check("json(&native.unsubscribe(&sol))", &json(&native.unsubscribe(&sol)));

    let mut native_map: HashMap<String, Vec<String>> = HashMap::new();
    for i in &opening {
        native_map.entry(native_stream.channel_of(i).unwrap()).or_default().push(i.key());
    }
    let mut messages = vec![
        r#"{"type":"subscriptions","channels":[{"name":"matches","product_ids":["BTC-USD","ETH-EUR"]}]}"#.to_string(),
        r#"{"type":"heartbeat","sequence":1}"#.to_string(),
    ];
    for i in 0..60 {
        let product = if i % 3 == 0 { "ETH-EUR" } else { "BTC-USD" };
        let kind = if i == 0 { "last_match" } else { "match" };
        messages.push(format!(
            r#"{{"type":"{kind}","trade_id":{i},"maker_order_id":"m","taker_order_id":"t","side":"{}","size":"0.{:08}","price":"{}.{:02}","product_id":"{product}","sequence":{i},"time":"2026-09-05T10:{:02}:{:02}.{:03}Z"}}"#,
            if i % 2 == 0 { "buy" } else { "sell" },
            1 + i * 7_919 % 99_999_999,
            40_000 + i,
            i % 100,
            i / 30,
            i % 60,
            (i * 37) % 1000
        ));
    }
    let mut b = Vec::new();
    for m in &messages {
        b.extend(native.on_message(m).unwrap());
    }
    let b = trades_by_instrument(b, &native_map);
    assert_eq!(b.len(), 100, "40 BTC trades on two panes, 20 ETH trades on one");
    g.check(format!("coinbase trades"), &b);
    assert!(native.on_message("{garbage").is_err());
}

#[tokio::test]
async fn shadow_coinbase_live_against_the_real_venue() {
    let on = |k: &str| std::env::var(k).as_deref() == Ok("1");
    if !on("OTW_LIVE_TEST") && !on("OTW_NET_TEST") {
        return;
    }
    let opening = vec![Instrument::new("BTC-USD", "crypto", "1m"), Instrument::new("ETH-USD", "crypto", "1m")];
    let native_stream = NativeStream::new(coinbase_driver(), 2);
    let mut native = NativeFeed::new(coinbase_driver(), 2, &opening).unwrap();
    let mut first = native.subscribe(&opening);
    first.extend(native.drain_sends());
    let (mut ws, _) = tokio_tungstenite::connect_async(native.url()).await.unwrap();
    for f in first {
        ws.send(Message::Text(f.into())).await.unwrap();
    }
    let mut native_map: HashMap<String, Vec<String>> = HashMap::new();
    for i in &opening {
        native_map.entry(native_stream.channel_of(i).unwrap()).or_default().push(i.key());
    }
    let mut b = Vec::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(15);
    while let Ok(Some(Ok(msg))) = tokio::time::timeout_at(deadline, ws.next()).await {
        if let Message::Text(t) = msg {
            b.extend(native.on_message(t.as_str()).unwrap());
        }
    }
    // The venue stamps trades to the microsecond; the native side keeps the millisecond.
    let ms = |v: Vec<(String, u64, u64, time::OffsetDateTime)>| {
        v.into_iter().map(|(k, p, s, t)| (k, p, s, t.unix_timestamp_nanos() / 1_000_000)).collect::<Vec<_>>()
    };
    let b = ms(trades_by_instrument(b, &native_map));
    assert!(!b.is_empty(), "no real Coinbase trade in 15 s");
    let _ = b;
    eprintln!("shadow coinbase live: {} trades identical", b.len());
}

// ── Alpaca ───────────────────────────────────────────────────────────────────

fn alpaca_driver(feed: otw_connector_providers::alpaca::Feed) -> Arc<Driver<otw_connector_providers::alpaca::Alpaca>> {
    Arc::new(Driver::new(otw_connector_providers::alpaca::Alpaca::new(feed).expect("caps"), DriverConfig::new("alpaca"), reqwest::Client::new()))
}

#[test]
fn alpaca_handshake_frames_and_bars_match_the_legacy_feed() {
    let mut g = crate::histdata::native::golden::Golden::new("live/alpaca_handshake_frames_and_bars_match_the_legacy_feed");
    use otw_connector_providers::alpaca::Feed;
    use otw_connector_sdk::Mapper as _;
    let secrets: HashMap<String, String> = [("api_key".to_string(), "k".to_string()), ("api_secret".to_string(), "s".to_string())].into();
    for (asset, symbols, feed, ent) in [
        ("equity", ["AAPL", "MSFT"], Feed::Iex, "Iex"),
        ("equity", ["AAPL", "MSFT"], Feed::Sip, "Sip"),
        ("crypto", ["BTC/USD", "ETH/USD"], Feed::Iex, "Iex"),
        // Deliberate difference: no option row. The option socket has no `bars` channel (410)
        // and speaks MessagePack only (412), so the legacy feed's option bars never streamed;
        // options now fold from prints (`quote_tests::an_alpaca_option_*`).
    ] {
        let opening = vec![Instrument::new(symbols[0], asset, "1m"), Instrument::new(symbols[1], asset, "5m"), Instrument::new(symbols[0], asset, "1d")];
        let native_stream = NativeStream::new(alpaca_driver(feed), 4);
        for i in &opening {
            g.check(format!("{asset} grain"), &native_stream.grain(i).unwrap());
            g.check(format!("{asset} socket"), &native_stream.socket_class(i).unwrap());
        }
        let driver = alpaca_driver(feed);
        let signer = driver.mapper().signer(&secrets).unwrap();
        let mut native = NativeFeed::new(driver, 4, &opening).unwrap().with_signer(signer);
        g.check(format!("{asset} {ent} url"), &native.url());
        // Nothing before the handshake on either side.
        let mut first = native.subscribe(&opening);
        first.extend(native.drain_sends());
        assert!(first.is_empty());
        let mut b = Vec::new();
        let mut frames_b = Vec::new();
        let mut native_map: HashMap<String, Vec<String>> = HashMap::new();
        for i in &opening {
            native_map.entry(native_stream.channel_of(i).unwrap()).or_default().push(i.key());
        }
        let mut messages = vec![r#"[{"T":"success","msg":"connected"}]"#.to_string(), r#"[{"T":"success","msg":"authenticated"}]"#.to_string(), format!(r#"[{{"T":"subscription","bars":["{}","{}"]}}]"#, symbols[0], symbols[1])];
        for i in 0..30 {
            let t = time::OffsetDateTime::from_unix_timestamp(1_788_602_400 + i * 60).unwrap().format(&time::format_description::well_known::Rfc3339).unwrap();
            messages.push(format!(
                r#"[{{"T":"b","S":"{}","t":"{t}","o":{}.1,"h":{}.9,"l":{}.05,"c":{}.5,"v":{},"n":3,"vw":1}},{{"T":"u","S":"{}","t":"{t}"}},{{"T":"d","S":"{}"}}]"#,
                symbols[(i % 2) as usize], 150 + i, 151 + i, 149 + i, 150 + i, 100 + i, symbols[0], symbols[1]
            ));
        }
        for m in &messages {
            b.extend(native.on_message(m).unwrap());
            frames_b.extend(native.drain_sends());
        }
        g.check(format!("{asset}: login then the waiting subscription"), &json(&frames_b));
        // Alpaca bars carry no event time: both stamp them when heard.
        let strip = |v: Vec<(String, LiveEvent)>| v.into_iter().map(|(k, mut e)| { e.provider_ts = time::OffsetDateTime::UNIX_EPOCH; (k, e) }).collect::<Vec<_>>();
        let b = strip(by_instrument(b, &native_map));
        assert_eq!(b.len(), 45, "{asset}: each symbol's bars reach every pane on it");
        g.check(format!("alpaca {asset} bars"), &events(&b));
        // Joining after the handshake goes straight out; leaving too.
        let more = [Instrument::new(symbols[1], asset, "15m")];
        assert_eq!(json(&native.subscribe(&more)), Vec::<serde_json::Value>::new(), "already carried");
        // Refusals end the session on both sides, with the same fault.
        for code in [402, 406, 409] {
            let m = format!(r#"[{{"T":"error","code":{code},"msg":"x"}}]"#);
            let Err(eb) = native.on_message(&m) else { panic!("{asset} code {code}: both refuse") };
            let fault = |e: &anyhow::Error| e.downcast_ref::<super::super::LiveError>().map(|l| format!("{:?}", l.fault));
            if code == 409 {
                // Deliberate difference: a plan refusal reads as an entitlement fault (the
                // framework's Permission), legacy said Plan; both name the fix.
                assert!(format!("{eb:#}").contains("upgrade the plan"));
            } else {
                g.check(format!("{asset} code {code}"), &fault(&eb));
            }
        }
    }
}

// ── Massive ──────────────────────────────────────────────────────────────────

fn massive_driver() -> Arc<Driver<otw_connector_providers::massive::Massive>> {
    Arc::new(Driver::new(otw_connector_providers::massive::Massive::new().expect("caps"), DriverConfig::new("massive"), reqwest::Client::new()))
}

#[test]
fn massive_handshake_frames_and_aggregates_match_the_legacy_feed() {
    let mut g = crate::histdata::native::golden::Golden::new("live/massive_handshake_frames_and_aggregates_match_the_legacy_feed");
    use otw_connector_sdk::Mapper as _;
    let secrets: HashMap<String, String> = [("api_key".to_string(), "k".to_string())].into();
    for (asset, symbols, ev, names) in [
        ("equity", ["AAPL", "MSFT"], "AM", ["AAPL", "MSFT"]),
        ("option", ["SPY251219C00650000", "AAPL251219P00150000"], "AM", ["O:SPY251219C00650000", "O:AAPL251219P00150000"]),
        ("index", ["SPX", "I:NDX"], "AM", ["I:SPX", "I:NDX"]),
        ("crypto", ["BTCUSD", "ETHUSDT"], "XA", ["BTC-USD", "ETH-USDT"]),
        ("fx", ["EURUSD", "GBPJPY"], "CA", ["EUR/USD", "GBP/JPY"]),
        ("future", ["ESZ5", "6CH6"], "AM", ["ESZ5", "6CH6"]),
    ] {
        let opening = vec![Instrument::new(symbols[0], asset, "1m"), Instrument::new(symbols[1], asset, "5m"), Instrument::new(symbols[0], asset, "1d")];
        let native_stream = NativeStream::new(massive_driver(), 8);
        for i in &opening {
            g.check(format!("{asset} grain"), &native_stream.grain(i).unwrap());
            g.check(format!("{asset} socket"), &native_stream.socket_class(i).unwrap());
        }
        let driver = massive_driver();
        let signer = driver.mapper().signer(&secrets).unwrap();
        let mut native = NativeFeed::new(driver, 8, &opening).unwrap().with_signer(signer);
        g.check(format!("{asset} url"), &native.url());
        // Nothing before the handshake on either side.
        let mut first = native.subscribe(&opening);
        first.extend(native.drain_sends());
        assert!(first.is_empty());
        let mut native_map: HashMap<String, Vec<String>> = HashMap::new();
        for i in &opening {
            native_map.entry(native_stream.channel_of(i).unwrap()).or_default().push(i.key());
        }
        let key = if ev == "AM" { "sym" } else { "pair" };
        let mut messages = vec![
            r#"[{"ev":"status","status":"connected","message":"Connected Successfully"}]"#.to_string(),
            r#"[{"ev":"status","status":"auth_success","message":"authenticated"}]"#.to_string(),
            format!(r#"[{{"ev":"status","status":"success","message":"subscribed to: {ev}.{}"}}]"#, names[0]),
        ];
        for i in 0..30 {
            let s = (1_788_602_400_i64 + i * 60) * 1000;
            messages.push(format!(
                r#"[{{"ev":"{ev}","{key}":"{}","v":{},"av":9,"op":1,"vw":1,"o":{}.1,"c":{}.5,"h":{}.9,"l":{}.05,"a":1,"z":1,"s":{s},"e":{}}},{{"ev":"T","sym":"{}"}}]"#,
                names[(i % 2) as usize], 100 + i, 150 + i, 150 + i, 151 + i, 149 + i, s + 60_000, names[0]
            ));
        }
        let (mut b, mut frames_b) = (Vec::new(), Vec::new());
        for m in &messages {
            b.extend(native.on_message(m).unwrap());
            frames_b.extend(native.drain_sends());
        }
        g.check(format!("{asset}: login then the waiting subscription"), &json(&frames_b));
        // Both stamp an aggregate when heard.
        let strip = |v: Vec<(String, LiveEvent)>| v.into_iter().map(|(k, mut e)| { e.provider_ts = time::OffsetDateTime::UNIX_EPOCH; (k, e) }).collect::<Vec<_>>();
        let b = strip(by_instrument(b, &native_map));
        assert_eq!(b.len(), 45, "{asset}: each symbol's aggregates reach every pane on it");
        g.check(format!("massive {asset} aggregates"), &events(&b));
        // Leaving goes straight out on both sides.
        let gone = [opening[1].clone()];
        g.check(format!("{asset} unsubscribe"), &json(&native.unsubscribe(&gone)));
        // Refusals end the session on both sides, with the same fault.
        let fault = |e: &anyhow::Error| e.downcast_ref::<super::super::LiveError>().map(|l| format!("{:?}", l.fault));
        for (status, msg) in [
            ("auth_timeout", "x"),
            ("error", "Maximum number of websocket connections exceeded"),
            ("error", "unknown ticker ZZZ"),
            ("error", "something else"),
        ] {
            let m = format!(r#"[{{"ev":"status","status":"{status}","message":"{msg}"}}]"#);
            let Err(eb) = native.on_message(&m) else { panic!("{asset} {status} {msg}: both refuse") };
            g.check(format!("{asset} {status} {msg}"), &fault(&eb));
        }
        // Deliberate difference: a refused login reads as an entitlement fault (legacy: Plan),
        // both terminal and both naming the paid plan; `max_connections` ends the session
        // (legacy let it pass silently).
        let m = r#"[{"ev":"status","status":"auth_failed","message":"Your plan doesn't include websocket access."}]"#;
        let Err(eb) = native.on_message(m) else { panic!("auth_failed") };
        assert!(format!("{eb:#}").contains("paid") && format!("{eb:#}").contains("paid"));
        assert!(native.on_message(r#"[{"ev":"status","status":"max_connections","message":"Maximum number of connections exceeded."}]"#).is_err());
    }
}

#[tokio::test]
async fn capitalcom_frames_and_mid_candles_match_the_legacy_feed() {
    let mut g = crate::histdata::native::golden::Golden::new("live/capitalcom_frames_and_mid_candles_match_the_legacy_feed");
    use otw_connector_sdk::Mapper as _;
    use wiremock::{matchers::any, Mock, MockServer, ResponseTemplate};
    // The REST sign-in the socket rides on.
    let server = MockServer::start().await;
    Mock::given(any()).respond_with(ResponseTemplate::new(200).set_body_string("{}").insert_header("CST", "c").insert_header("X-SECURITY-TOKEN", "s")).mount(&server).await;
    let driver = Arc::new(Driver::new(otw_connector_providers::capitalcom::CapitalCom::new().expect("caps"), DriverConfig::new("capitalcom").base("rest", &server.uri()), reqwest::Client::new()));
    let secrets: HashMap<String, String> = [("api_key", "k"), ("identifier", "live-parity"), ("api_password", "p")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    let signer = driver.mapper().signer(&secrets).unwrap().unwrap();
    driver.ensure_token(signer.as_ref()).await.unwrap();

    let opening = vec![Instrument::new("gold", "fx", "5m"), Instrument::new("US500", "index", "5m"), Instrument::new("GOLD", "fx", "1h"), Instrument::new("EURUSD", "fx", "1d")];
    let native_stream = NativeStream::new(driver.clone(), 2);
    for i in &opening {
        g.check("native_stream.grain(i).unwrap()", &native_stream.grain(i).unwrap());
    }
    let mut native = NativeFeed::with_session(driver, 2, &opening, Some(signer), Default::default()).unwrap();
    g.check("native.url()", &native.url());
    // The same frames, credentials on each; the order of resolutions and the correlation ids
    // are the sides' own counters.
    let norm = |frames: &[String]| {
        let mut v: Vec<serde_json::Value> = json(frames)
            .into_iter()
            .map(|mut f| {
                f.as_object_mut().unwrap().remove("correlationId");
                f
            })
            .collect();
        v.sort_by_key(|f| f.to_string());
        v
    };
    let mut first = native.subscribe(&opening);
    first.extend(native.drain_sends());
    g.check(format!("opening frames"), &norm(&first));
    let more = [Instrument::new("OIL_CRUDE", "fx", "15m")];
    g.check("norm(&native.subscribe(&more))", &norm(&native.subscribe(&more)));
    g.check("norm(&native.unsubscribe(&more))", &norm(&native.unsubscribe(&more)));

    let mut native_map: HashMap<String, Vec<String>> = HashMap::new();
    for i in &opening {
        native_map.entry(native_stream.channel_of(i).unwrap()).or_default().push(i.key());
    }
    let ev = |epic: &str, res: &str, side: &str, t: i64, c: f64| {
        format!(r#"{{"status":"OK","destination":"ohlc.event","payload":{{"resolution":"{res}","epic":"{epic}","type":"classic","priceType":"{side}","t":{t},"h":{},"l":{},"o":{c},"c":{}}}}}"#, c + 1.5, c - 0.75, c + 0.25)
    };
    let mut messages = vec![r#"{"status":"OK","destination":"OHLCMarketData.subscribe","correlationId":"2","payload":{"subscriptions":{"GOLD:MINUTE_5:classic":"PROCESSED","US500:MINUTE_5:classic":"PROCESSED"}}}"#.to_string()];
    for i in 0..40_i64 {
        let t5 = 1_671_714_000_000 + (i / 4) * 300_000;
        let px = 1_800.0 + i as f64 * 0.25;
        // Bid then ask, sometimes the ask first, sometimes a side alone.
        let (first, second) = if i % 5 == 0 { ("ask", "bid") } else { ("bid", "ask") };
        messages.push(ev("GOLD", "MINUTE_5", first, t5, px));
        if i % 7 != 3 {
            messages.push(ev("GOLD", "MINUTE_5", second, t5, px + 0.5));
        }
        messages.push(ev("US500", "MINUTE_5", "bid", t5, 4_000.0 + i as f64));
        messages.push(ev("US500", "MINUTE_5", "offer", t5, 4_000.5 + i as f64));
        messages.push(ev("GOLD", "HOUR", "bid", 1_671_714_000_000, px));
        messages.push(ev("GOLD", "HOUR", "ask", 1_671_714_000_000, px + 0.5));
        // A stream nobody holds is ignored.
        messages.push(ev("SILVER", "MINUTE_5", "bid", t5, 23.0));
    }
    let mut b = Vec::new();
    for m in &messages {
        b.extend(native.on_message(m).unwrap());
    }
    let strip = |v: Vec<(String, LiveEvent)>| v.into_iter().map(|(k, mut e)| { e.provider_ts = time::OffsetDateTime::UNIX_EPOCH; (k, e) }).collect::<Vec<_>>();
    let b = strip(by_instrument(b, &native_map));
    assert!(b.len() > 100, "{}", b.len());
    g.check("capitalcom mid candles", &events(&b));
    // A refused epic faults its own pane on both sides; a refused token ends the socket.
    let ack = r#"{"status":"OK","destination":"OHLCMarketData.subscribe","correlationId":"3","payload":{"subscriptions":{"GOLD:HOUR:classic":"PROCESSED","EURUSD:DAY:classic":"ERROR"}}}"#;
    let faults = |v: Vec<FeedUpdate>, map: &HashMap<String, Vec<String>>| {
        v.into_iter().filter_map(|u| match u.payload { Payload::Fault { fault, .. } => Some((map.get(&u.channel).cloned().unwrap_or_default(), format!("{fault:?}"))), _ => None }).collect::<Vec<_>>()
    };
    g.check("faults(native.on_message(ack).unwrap(), &native_map)", &faults(native.on_message(ack).unwrap(), &native_map));
    let bad = r#"{"status":"FAILED","destination":"OHLCMarketData.subscribe.error","payload":{"errorCode":"error.invalid.session.token"}}"#;
    let fault = |e: &anyhow::Error| e.downcast_ref::<super::super::LiveError>().map(|l| format!("{:?}", l.fault));
    let Err(eb) = native.on_message(bad) else { panic!("both refuse") };
    g.check("fault(&eb)", &fault(&eb));
}

/// Shadow against a real IB Gateway or TWS: `OTW_IBKR_GATEWAY=host:port`. Opens the December
/// MNQ's live line through the adapter and reads it for twenty seconds (nothing is ordered).
/// Outside trading hours the line stays quiet; what must not happen is a refusal.
#[tokio::test]
async fn shadow_ibkr_live_against_the_real_venue() {
    use futures::StreamExt;
    let Ok(addr) = std::env::var("OTW_IBKR_GATEWAY") else { return };
    let (host, port) = addr.rsplit_once(':').expect("OTW_IBKR_GATEWAY is host:port");
    let secrets: HashMap<String, String> = [("host", host), ("port", port)].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    let ticker = std::env::var("OTW_IBKR_FUTURE").unwrap_or_else(|_| "MNQ.202612@CME".into());
    // The process-wide session may hold a client opened on another test's runtime, gone now.
    otw_connector_providers::ibkr::gateway::session::get(&secrets).await.unwrap().reset().await;
    let native = super::ibkr().expect("native ibkr");
    let inst = Instrument::new(&ticker, "future", "1m");
    assert!(matches!(native.grain(&inst).unwrap(), super::super::SourceGrain::Trades));
    let mut session = native.open(super::super::SessionRequest { secrets: &secrets, connector: "ib", instruments: std::slice::from_ref(&inst) }).await.unwrap();
    let mut trades = 0;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(20);
    while let Ok(Some(item)) = tokio::time::timeout_at(deadline, session.stream.next()).await {
        match item.expect("the session itself failed") {
            FeedUpdate { payload: Payload::Trade { price, size, .. }, .. } => {
                assert!(price > 0.0 && size >= 0.0);
                trades += 1;
            }
            FeedUpdate { payload: Payload::Fault { fault, message }, .. } => panic!("{fault:?}: {message}"),
            _ => {}
        }
    }
    eprintln!("shadow ibkr live: {trades} print(s) in 20 s on {ticker}");
}
