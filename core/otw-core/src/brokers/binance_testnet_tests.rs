//! Trading checks against Binance's **test** environments (Spot Demo Mode, USDⓈ-M futures
//! testnet), on broker accounts stored in the local database and set to `env = testnet`.
//! Every order goes through the `Broker` trait, as the chart's tickets do. An account that is
//! not set to testnet is refused before anything is sent, so nothing here reaches real money.
//!
//! Run with:
//! `OTW_BINANCE_TESTNET=<spot account id>,<futures account id> OTW_SECRET_KEY=... cargo test -p otw-core binance_testnet -- --nocapture`.
//! The keys are read from the running stack's database (`docker exec` on the postgres
//! container, `OTW_PG_CONTAINER`, default `opentraderworld-postgres-1`) and opened with
//! `OTW_SECRET_KEY`; they are never printed.

use std::collections::HashMap;
use std::time::Instant;

use otw_connector_model::{Decimal, OrderAmend, OrderType, Price, Qty, Side, TifKind, TimeInForce};

use super::{Broker, OrderLeg, OrderTicket};

/// One account's settings and secrets, as `brokers_api::resolve` hands them to a broker.
fn account(id: &str) -> (String, HashMap<String, String>) {
    let container = std::env::var("OTW_PG_CONTAINER").unwrap_or_else(|_| "opentraderworld-postgres-1".into());
    let psql = |sql: &str| -> String {
        let out = std::process::Command::new("docker")
            .args(["exec", &container, "psql", "-U", "otw", "-d", "opentraderworld", "-At", "-F", "\t", "-c", sql])
            .output()
            .unwrap_or_else(|e| panic!("docker exec: {e}"));
        assert!(out.status.success(), "psql: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    assert!(id.chars().all(|c| c.is_ascii_hexdigit() || c == '-'), "{id} is not an account id");
    let row = psql(&format!("SELECT broker, config::text FROM broker_accounts WHERE id = '{id}'"));
    let (broker, config) = row.trim().split_once('\t').unwrap_or_else(|| panic!("no broker account {id}"));
    let mut map = HashMap::new();
    if let Ok(serde_json::Value::Object(obj)) = serde_json::from_str::<serde_json::Value>(config) {
        for (k, v) in obj {
            map.insert(k, v.as_str().map(String::from).unwrap_or_else(|| v.to_string()));
        }
    }
    let cipher = otw_store::crypto::SecretCipher::from_master(&std::env::var("OTW_SECRET_KEY").expect("OTW_SECRET_KEY")).unwrap();
    let rows = psql(&format!(
        "SELECT c.name, encode(COALESCE(vi.nonce, c.nonce), 'hex'), encode(COALESCE(vi.ciphertext, c.ciphertext), 'hex') \
         FROM broker_account_creds c LEFT JOIN vault_items vi ON vi.id = c.vault_item_id WHERE c.account_id = '{id}'"
    ));
    for line in rows.lines() {
        let mut p = line.split('\t');
        let (Some(name), Some(n), Some(ct)) = (p.next(), p.next(), p.next()) else { continue };
        let open = cipher.open(&hex::decode(n).unwrap(), &hex::decode(ct).unwrap()).unwrap();
        map.insert(name.to_string(), open);
    }
    (broker.to_string(), map)
}

fn accounts() -> Option<Vec<(String, HashMap<String, String>)>> {
    let ids = std::env::var("OTW_BINANCE_TESTNET").ok()?;
    let out: Vec<_> = ids.split(',').map(|s| account(s.trim())).collect();
    for (broker, s) in &out {
        assert_eq!(s.get("env").map(String::as_str), Some("testnet"), "{broker}: the account must be set to env = testnet");
    }
    Some(out)
}


#[derive(Default)]
struct Report {
    rows: Vec<(String, &'static str, String)>,
}

impl Report {
    fn note(&mut self, id: &str, verdict: &'static str, detail: impl Into<String>) {
        let detail = detail.into();
        println!("| {id} | {verdict} | {} |", detail.replace('\n', " "));
        self.rows.push((id.into(), verdict, detail));
    }
    fn check(&mut self, id: &str, ok: bool, detail: impl Into<String>) {
        self.note(id, if ok { "PASS" } else { "FAIL" }, detail);
    }
}

fn key(tag: &str) -> String {
    let n = time::OffsetDateTime::now_utc().unix_timestamp_nanos() % 1_000_000_000_000;
    format!("otwt-{tag}-{n}")
}

fn dec(s: &str) -> Decimal {
    Decimal::from_str_exact(s).unwrap_or_default()
}

/// `v` on the `tick` grid, rounded down.
fn on_tick(v: Decimal, tick: Decimal) -> Decimal {
    ((v / tick).floor() * tick).normalize()
}

fn leg(key: &str, side: Side, t: OrderType, qty: Decimal, limit: Option<Decimal>, stop: Option<Decimal>, tif: TifKind) -> OrderLeg {
    OrderLeg { limit, stop, tif: Some(tif), ..OrderLeg::new(key, side, t, qty) }
}

fn ticket(symbol: &str, legs: Vec<OrderLeg>) -> OrderTicket {
    OrderTicket { ticker: symbol.into(), asset_type: "crypto".into(), legs, provider: None, connector: None, reference: None, reference_at: None, confirm_duplicate: false }
}

async fn working(b: &dyn Broker, s: &HashMap<String, String>) -> Vec<super::OpenOrder> {
    let http = super::client().unwrap();
    match b.orders(&http, s).await {
        Ok(l) => l,
        Err(e) => {
            println!("orders read failed: {e:#}");
            vec![]
        }
    }
}

fn err_of<T>(r: anyhow::Result<T>) -> String {
    match r {
        Ok(_) => "accepted".into(),
        Err(e) => format!("{e:#}"),
    }
}

/// Best bid and ask on the environment's own book.
async fn book(futures: bool, symbol: &str) -> (Decimal, Decimal) {
    let url = if futures {
        format!("https://testnet.binancefuture.com/fapi/v1/ticker/bookTicker?symbol={symbol}")
    } else {
        format!("https://demo-api.binance.com/api/v3/ticker/bookTicker?symbol={symbol}")
    };
    let v: serde_json::Value = reqwest::get(url).await.unwrap().json().await.unwrap();
    (dec(v["bidPrice"].as_str().unwrap()), dec(v["askPrice"].as_str().unwrap()))
}

async fn catalog(r: &mut Report, broker: &str, s: &HashMap<String, String>) {
    let futures = broker == "binance_futures";
    let tag = if futures { "F" } else { "S" };
    let id = |t: &str| format!("{tag}-{t}");
    let symbol = std::env::var("OTW_BINANCE_TESTNET_SYMBOL").unwrap_or_else(|_| "BTCUSDT".into());
    let b = super::broker_for(broker).unwrap();
    let b = b.as_ref();
    let http = super::client().unwrap();
    let (tick, step, qty) = if futures { (dec("0.1"), dec("0.0001"), dec("0.002")) } else { (dec("0.01"), dec("0.00001"), dec("0.0002")) };

    match b.test(&http, s).await {
        Ok(m) => r.check(&id("C01 connect"), m.contains("Testnet"), m),
        Err(e) => r.check(&id("C01 connect"), false, format!("{e:#}")),
    }
    let mut live = s.clone();
    live.remove("env");
    let e = err_of(b.place(&live, &ticket(&symbol, vec![leg(&key("live"), Side::Buy, OrderType::Market, qty, None, None, TifKind::Gtc)])).await);
    r.check(&id("C02 live account refused"), e.contains("testnet only"), e);
    let t = Instant::now();
    let warm = b.symbol_for(s, &symbol, "crypto").await;
    r.check(&id("C03 chart opens the instrument (clock, terms)"), warm.as_ref().is_ok_and(|w| w.as_deref() == Some(symbol.as_str())), format!("{} ms {}", t.elapsed().as_millis(), err_of(warm)));
    let before = working(b, s).await;
    let mine = |l: &[super::OpenOrder]| l.iter().filter(|o| o.symbol == symbol).count();
    let (bid, ask) = book(futures, &symbol).await;
    let low = on_tick(bid * dec("0.9"), tick);
    let low2 = on_tick(bid * dec("0.88"), tick);
    let high = on_tick(ask * dec("1.04"), tick);
    let high2 = on_tick(ask * dec("1.045"), tick);

    // T01 limit rests, amend, cancel.
    let k = key("lmt");
    let t0 = Instant::now();
    match b.place(s, &ticket(&symbol, vec![leg(&k, Side::Buy, OrderType::Limit, qty, Some(low), None, TifKind::Gtc)])).await {
        Ok(p) => {
            let oid = p[0].id.clone();
            r.check(&id("T01 limit rests"), p[0].key == k && p[0].tick == Some(tick), format!("ack {} ms, id {oid}, tick {:?}", t0.elapsed().as_millis(), p[0].tick));
            let l = working(b, s).await;
            r.check(&id("T01 listed under its id"), l.iter().any(|o| o.id == oid && o.limit_price == Some(low.to_string().parse().unwrap())), format!("{} working", l.len()));
            let t = Instant::now();
            let res = b.modify(s, &oid, &OrderAmend { limit: Some(Price::new(low2)), qty: Some(Qty::new(qty * dec("2"))), ..Default::default() }).await;
            let l = working(b, s).await;
            let moved = l.iter().find(|o| o.id == oid);
            r.check(
                &id("T06 amend price + qty, same id"),
                res.is_ok() && moved.is_some_and(|o| o.limit_price == Some(low2.to_string().parse().unwrap()) && (o.qty - (qty * dec("2")).to_string().parse::<f64>().unwrap()).abs() < 1e-12),
                format!("{} ms, {}", t.elapsed().as_millis(), match &res { Ok(_) => format!("{moved:?}").chars().take(160).collect(), Err(e) => format!("{e:#}") }),
            );
            let e = err_of(b.modify(s, &oid, &OrderAmend { limit: Some(Price::new(low2 + tick / dec("2"))), ..Default::default() }).await);
            r.check(&id("T06 amend off tick refused"), e.contains("off that step"), e);
            if !futures {
                // A time in force change rides the replace on spot.
                let res = b.modify(s, &oid, &OrderAmend { tif: Some(TimeInForce::Gtc), limit: Some(Price::new(low)), ..Default::default() }).await;
                r.check(&id("T06 amend keeps tif"), res.is_ok(), err_of(res));
            } else {
                let e = err_of(b.modify(s, &oid, &OrderAmend { tif: Some(TimeInForce::Ioc), ..Default::default() }).await);
                r.check(&id("T06 futures tif change refused"), e.contains("new time in force"), e);
            }
            let t = Instant::now();
            let res = b.cancel(s, &oid).await;
            let gone = !working(b, s).await.iter().any(|o| o.id == oid);
            r.check(&id("T01 cancel"), res.is_ok() && gone, format!("{} ms {}", t.elapsed().as_millis(), err_of(res)));
            let res = b.cancel(s, &oid).await;
            r.check(&id("T22a cancel an already cancelled order"), res.is_ok(), err_of(res));
            let e = err_of(b.modify(s, &oid, &OrderAmend { limit: Some(Price::new(low)), ..Default::default() }).await);
            r.check(&id("T23a amend a cancelled order"), e.contains("not working"), e);
            let e = err_of(b.place(s, &ticket(&symbol, vec![leg(&k, Side::Buy, OrderType::Limit, qty, Some(low), None, TifKind::Gtc)])).await);
            r.check(&id("T19z same client id twice"), e.contains("already sent"), e);
        }
        Err(e) => r.check(&id("T01 limit rests"), false, format!("{e:#}")),
    }

    // T11/T12 refusals before anything is sent.
    let cases: Vec<(&str, OrderTicket, &str)> = vec![
        ("T11 off tick", ticket(&symbol, vec![leg(&key("x"), Side::Buy, OrderType::Limit, qty, Some(low + tick / dec("2")), None, TifKind::Gtc)]), "off that step"),
        ("T11 off lot", ticket(&symbol, vec![leg(&key("x"), Side::Buy, OrderType::Limit, qty + step / dec("2"), Some(low), None, TifKind::Gtc)]), "off that step"),
        ("T11 qty 0", ticket(&symbol, vec![leg(&key("x"), Side::Buy, OrderType::Limit, Decimal::ZERO, Some(low), None, TifKind::Gtc)]), "above zero"),
        ("T11 limit without price", ticket(&symbol, vec![leg(&key("x"), Side::Buy, OrderType::Limit, qty, None, None, TifKind::Gtc)]), "limit price"),
        ("T11 day order", ticket(&symbol, vec![leg(&key("x"), Side::Buy, OrderType::Limit, qty, Some(low), None, TifKind::Day)]), "Day"),
        ("T11 client id too long", ticket(&symbol, vec![leg(&"k".repeat(40), Side::Buy, OrderType::Limit, qty, Some(low), None, TifKind::Gtc)]), "1 to 36"),
        ("T12 unknown symbol", ticket("NOPEUSDT", vec![leg(&key("x"), Side::Buy, OrderType::Limit, qty, Some(low), None, TifKind::Gtc)]), "does not list"),
        ("T12 display spelling", ticket("BTC/USDT", vec![leg(&key("x"), Side::Buy, OrderType::Limit, qty, Some(low), None, TifKind::Gtc)]), "bad symbol"),
    ];
    for (name, t, want) in cases {
        let e = err_of(b.place(s, &t).await);
        r.check(&id(name), e.contains(want), e);
    }
    if !futures {
        let e = err_of(b.place(s, &ticket(&symbol, vec![leg(&key("x"), Side::Buy, OrderType::Limit, qty, Some(low), None, TifKind::Gtd)])).await);
        r.check(&id("T11 gtd on spot"), e.contains("Gtd") || e.contains("expiry"), e);
    }
    let l = working(b, s).await;
    r.check(&id("T11z nothing reached the venue"), mine(&l) == mine(&before), format!("{} working on {symbol}", mine(&l)));

    // T05 stop and stop-limit rest, the stop moves, both cancel.
    let ks = key("stp");
    let kl = key("stl");
    let stop = b.place(s, &ticket(&symbol, vec![leg(&ks, Side::Buy, OrderType::Stop, qty, None, Some(high), TifKind::Gtc)])).await;
    let stop_limit = b.place(s, &ticket(&symbol, vec![leg(&kl, Side::Buy, OrderType::StopLimit, qty, Some(high2), Some(high), TifKind::Gtc)])).await;
    match (&stop, &stop_limit) {
        (Ok(a), Ok(c)) => {
            let l = working(b, s).await;
            let listed = l.iter().any(|o| o.id == a[0].id && o.stop_price.is_some()) && l.iter().any(|o| o.id == c[0].id && o.limit_price.is_some());
            r.check(&id("T05 stop, stop-limit rest and are listed"), listed, format!("{} / {}", a[0].id, c[0].id));
            let t = Instant::now();
            let res = b.modify(s, &a[0].id, &OrderAmend { stop: Some(Price::new(high2)), ..Default::default() }).await;
            // Spot changes the order under its id; a futures stop is placed anew (`k.r1`) before
            // the old one goes, and the answer names it.
            let now_id = res.as_ref().ok().and_then(|p| p.as_ref().map(|p| p.id.clone())).unwrap_or_else(|| a[0].id.clone());
            let l = working(b, s).await;
            let moved = l.iter().any(|o| o.id == now_id && o.stop_price == Some(high2.to_string().parse().unwrap()));
            let old_gone = now_id == a[0].id || !l.iter().any(|o| o.id == a[0].id);
            r.check(&id("T06 move a stop, one stop working"), res.is_ok() && moved && old_gone, format!("{} ms {now_id} {}", t.elapsed().as_millis(), err_of(res.map(|_| ()))));
            let ok = b.cancel(s, &now_id).await.is_ok() && b.cancel(s, &c[0].id).await.is_ok();
            r.check(&id("T05 cancel both"), ok && !working(b, s).await.iter().any(|o| o.id == now_id || o.id == c[0].id), "");
        }
        _ => r.check(&id("T05 stop, stop-limit rest"), false, format!("{} / {}", err_of(stop.as_ref().map(|_| ()).map_err(|e| anyhow::anyhow!("{e:#}"))), err_of(stop_limit.as_ref().map(|_| ()).map_err(|e| anyhow::anyhow!("{e:#}"))))),
    }
    let e = err_of(b.place(s, &ticket(&symbol, vec![leg(&key("wrong"), Side::Buy, OrderType::Stop, qty, None, Some(low), TifKind::Gtc)])).await);
    r.check(&id("T05 buy stop below the market refused"), e != "accepted", e);

    // T09 IOC far from the market expires at once.
    let k = key("ioc");
    let res = b.place(s, &ticket(&symbol, vec![leg(&k, Side::Buy, OrderType::Limit, qty, Some(low), None, TifKind::Ioc)])).await;
    let now = working(b, s).await;
    let left = res.as_ref().is_ok_and(|p| now.iter().any(|o| o.id == p[0].id));
    r.check(&id("T09 IOC unfilled is not working"), res.is_ok() && !left, err_of(res));
    if futures {
        let mut g = leg(&key("gtd"), Side::Buy, OrderType::Limit, qty, Some(low), None, TifKind::Gtd);
        g.expires = Some((time::OffsetDateTime::now_utc().unix_timestamp() + 900) * 1000);
        match b.place(s, &ticket(&symbol, vec![g])).await {
            Ok(p) => {
                r.check(&id("T09 GTD +15 min rests"), working(b, s).await.iter().any(|o| o.id == p[0].id), "");
                let _ = b.cancel(s, &p[0].id).await;
            }
            Err(e) => r.check(&id("T09 GTD +15 min rests"), false, format!("{e:#}")),
        }
    }

    // T15 more than the account can carry: refused with the venue's reason, at once.
    let t = Instant::now();
    let e = err_of(b.place(s, &ticket(&symbol, vec![leg(&key("big"), Side::Buy, OrderType::Limit, dec("50"), Some(on_tick(bid * dec("0.98"), tick)), None, TifKind::Gtc)])).await);
    r.check(&id("T15 beyond balance / margin refused"), e != "accepted" && (e.contains("insufficient") || e.contains("Margin")), format!("{} ms: {e}", t.elapsed().as_millis()));

    // T34 kill switch.
    let ka = key("ka");
    let kb = key("kb");
    let a = b.place(s, &ticket(&symbol, vec![leg(&ka, Side::Buy, OrderType::Limit, qty, Some(low), None, TifKind::Gtc)])).await;
    let c = b.place(s, &ticket(&symbol, vec![leg(&kb, Side::Buy, OrderType::Stop, qty, None, Some(high), TifKind::Gtc)])).await;
    if a.is_ok() && c.is_ok() && mine(&before) == 0 {
        let t = Instant::now();
        let n = b.cancel_all(s).await;
        let l = working(b, s).await;
        r.check(&id("T34 kill switch"), n.as_ref().is_ok_and(|n| *n >= 2) && l.is_empty(), format!("{:?} cancelled in {} ms, {} left", n.as_ref().map_err(|e| format!("{e:#}")), t.elapsed().as_millis(), l.len()));
    } else {
        r.note(&id("T34 kill switch"), "N/A", format!("orders already working on the account, or placing failed: {} / {}", err_of(a), err_of(c)));
    }

    // T04 market round trip, T10 close.
    let k = key("mkt");
    let t = Instant::now();
    match b.place(s, &ticket(&symbol, vec![leg(&k, Side::Buy, OrderType::Market, qty, None, None, TifKind::Gtc)])).await {
        Ok(p) => {
            r.check(&id("T04 market buy"), true, format!("ack {} ms, {}", t.elapsed().as_millis(), p[0].id));
            if futures {
                let pos = b.positions(&http, s).await.unwrap_or_default();
                let held = pos.iter().find(|p| p.symbol == symbol);
                r.check(&id("T04 position long"), held.is_some_and(|p| p.side == "long" && p.native_id.as_deref() == Some(symbol.as_str())), format!("{held:?}").chars().take(200).collect::<String>());
                let t = Instant::now();
                let res = b.close_position(s, &symbol, "", &key("close")).await;
                let flat = !b.positions(&http, s).await.unwrap_or_default().iter().any(|p| p.symbol == symbol);
                r.check(&id("T10 close position"), res.is_ok() && flat, format!("{} ms {}", t.elapsed().as_millis(), err_of(res)));
                let e = err_of(b.close_position(s, &symbol, "", &key("close2")).await);
                r.check(&id("T10 close when flat refused"), e.contains("no position"), e);
                match b.account(s, None).await {
                    Ok(a) => r.check(&id("account figures"), a.first().is_some_and(|a| a.equity.is_some() && a.available.is_some()), format!("{:?}", a.first().map(|a| (a.equity, a.available, a.margin_used)))),
                    Err(e) => r.check(&id("account figures"), false, format!("{e:#}")),
                }
            } else {
                // The buy's fee is taken in the coin: what is left to sell is a step less.
                let res = b.place(s, &ticket(&symbol, vec![leg(&key("mks"), Side::Sell, OrderType::Market, qty - step * dec("10"), None, None, TifKind::Gtc)])).await;
                r.check(&id("T04 market sell back"), res.is_ok(), err_of(res));
                let e = err_of(b.close_position(s, &symbol, "", &key("close")).await);
                r.check(&id("T10 spot has no position to close"), e.contains("not a position"), e);
            }
        }
        Err(e) => r.check(&id("T04 market buy"), false, format!("{e:#}")),
    }

    // T20 / T21 USDⓈ-M exits: sent reduce-only beside the entry, linked by OTW.
    if futures {
        let exit = |k: &str, parent: &str, t: OrderType, limit: Option<Decimal>, stop: Option<Decimal>| OrderLeg {
            parent: Some(parent.into()),
            group: Some(format!("g{parent}")),
            ..leg(k, Side::Sell, t, qty, limit, stop, TifKind::Gtc)
        };
        let settle = std::time::Duration::from_secs(12);
        let (e, tp, sl) = (key("be"), key("btp"), key("bsl"));
        let legs = vec![
            leg(&e, Side::Buy, OrderType::Limit, qty, Some(low), None, TifKind::Gtc),
            exit(&tp, &e, OrderType::Limit, Some(high), None),
            exit(&sl, &e, OrderType::Stop, None, Some(low2)),
        ];
        match b.place(s, &ticket(&symbol, legs)).await {
            Ok(p) => {
                let l = working(b, s).await;
                r.check(&id("T20 entry and both exits rest"), p.iter().all(|x| l.iter().any(|o| o.id == x.id)), format!("{} placed, {} working", p.len(), l.len()));
                let _ = b.cancel(s, &p[0].id).await;
                tokio::time::sleep(settle).await;
                let l = working(b, s).await;
                r.check(&id("T20 entry cancelled unfilled takes its exits"), !p.iter().any(|x| l.iter().any(|o| o.id == x.id)), format!("{} working on {symbol}", mine(&l)));
            }
            Err(e) => r.check(&id("T20 entry and both exits rest"), false, format!("{e:#}")),
        }
        let (e, tp, sl) = (key("me"), key("mtp"), key("msl"));
        let legs = vec![
            leg(&e, Side::Buy, OrderType::Market, qty, None, None, TifKind::Gtc),
            // Below the bid: fills the moment it reaches the book, as a hit target would.
            exit(&tp, &e, OrderType::Limit, Some(on_tick(bid * dec("0.99"), tick)), None),
            exit(&sl, &e, OrderType::Stop, None, Some(low2)),
        ];
        match b.place(s, &ticket(&symbol, legs)).await {
            Ok(p) => {
                tokio::time::sleep(settle).await;
                let l = working(b, s).await;
                let flat = !b.positions(&http, s).await.unwrap_or_default().iter().any(|x| x.symbol == symbol);
                r.check(&id("T21 filled exit cancels the other"), flat && !l.iter().any(|o| o.id == p[2].id), format!("flat {flat}, {} working on {symbol}", mine(&l)));
            }
            Err(e) => r.check(&id("T21 filled exit cancels the other"), false, format!("{e:#}")),
        }
    }

    let l = working(b, s).await;
    r.check(&id("Z01 nothing left working"), mine(&l) == mine(&before), format!("{} working on {symbol}", mine(&l)));
}

#[tokio::test(flavor = "multi_thread")]
async fn binance_testnet_trading_catalog() {
    let Some(list) = accounts() else { return };
    let mut r = Report::default();
    println!("| Test | Result | Detail |\n| --- | --- | --- |");
    for (broker, s) in &list {
        catalog(&mut r, broker, s).await;
    }
    let count = |v: &str| r.rows.iter().filter(|x| x.1 == v).count();
    println!("{} PASS, {} FAIL, {} GAP, {} N/A", count("PASS"), count("FAIL"), count("GAP"), count("N/A"));
    assert_eq!(count("FAIL"), 0);
}

/// The first event `hit` takes, within 10 s.
async fn until(events: &mut futures::stream::BoxStream<'static, super::AccountEvent>, hit: impl Fn(&super::AccountEvent) -> bool) -> Option<super::AccountEvent> {
    use futures::StreamExt;
    let first = async {
        while let Some(ev) = events.next().await {
            if hit(&ev) {
                return Some(ev);
            }
        }
        None
    };
    tokio::time::timeout(std::time::Duration::from_secs(10), first).await.ok().flatten()
}

/// The account stream: a limit far from the market shows as an `order` event under its id,
/// and its cancel as a `done` one.
async fn stream(r: &mut Report, broker: &str, s: &HashMap<String, String>) {
    use super::AccountEvent;
    let futures = broker == "binance_futures";
    let tag = if futures { "F" } else { "S" };
    let id = |t: &str| format!("{tag}-{t}");
    let symbol = std::env::var("OTW_BINANCE_TESTNET_SYMBOL").unwrap_or_else(|_| "BTCUSDT".into());
    let b = super::broker_for(broker).unwrap();
    let b = b.as_ref();
    let (tick, qty) = if futures { (dec("0.1"), dec("0.002")) } else { (dec("0.01"), dec("0.0002")) };
    let t = Instant::now();
    let mut events = match b.events(s).await {
        Ok(Some(e)) => {
            r.check(&id("E01 stream opens"), true, format!("{} ms", t.elapsed().as_millis()));
            e
        }
        Ok(None) => return r.check(&id("E01 stream opens"), false, "no stream"),
        Err(e) => return r.check(&id("E01 stream opens"), false, format!("{e:#}")),
    };
    let (bid, _) = book(futures, &symbol).await;
    let low = on_tick(bid * dec("0.9"), tick);
    let k = key("evt");
    let t = Instant::now();
    let oid = match b.place(s, &ticket(&symbol, vec![leg(&k, Side::Buy, OrderType::Limit, qty, Some(low), None, TifKind::Gtc)])).await {
        Ok(p) => p[0].id.clone(),
        Err(e) => return r.check(&id("E02 order event"), false, format!("{e:#}")),
    };
    let want: f64 = low.to_string().parse().unwrap();
    let ev = until(&mut events, |e| matches!(e, AccountEvent::Order { order, .. } if order.id == oid)).await;
    let ok = matches!(&ev, Some(AccountEvent::Order { order, key }) if order.limit_price == Some(want) && key.as_deref() == Some(k.as_str()));
    r.check(&id("E02 order event"), ok, format!("{} ms, {}", t.elapsed().as_millis(), format!("{ev:?}").chars().take(200).collect::<String>()));
    let t = Instant::now();
    let res = b.cancel(s, &oid).await;
    let ev = until(&mut events, |e| matches!(e, AccountEvent::Done { id, .. } if *id == oid)).await;
    let ok = res.is_ok() && matches!(&ev, Some(AccountEvent::Done { status, .. }) if status == "cancelled");
    r.check(&id("E03 done event on cancel"), ok, format!("{} ms, {} {ev:?}", t.elapsed().as_millis(), err_of(res)));
}

#[tokio::test(flavor = "multi_thread")]
async fn binance_testnet_account_stream() {
    let Some(list) = accounts() else { return };
    let mut r = Report::default();
    println!("| Test | Result | Detail |\n| --- | --- | --- |");
    for (broker, s) in &list {
        stream(&mut r, broker, s).await;
    }
    let count = |v: &str| r.rows.iter().filter(|x| x.1 == v).count();
    println!("{} PASS, {} FAIL", count("PASS"), count("FAIL"));
    assert_eq!(count("FAIL"), 0);
}
