//! Trading checks against a real IB Gateway or TWS signed in to a **paper** account
//! (`ProjectSpecs/OTW_Trading_Broker_Onboarding_Spec.md`, catalog 10.3). Every order goes
//! through the `Broker` trait, as the chart's tickets do. The adapter refuses a live login
//! below M4, so nothing here can reach real money.
//!
//! Run with: `OTW_IBKR_PAPER=127.0.0.1:4002 cargo test -p otw-core ibkr_paper -- --nocapture`.
//! The future is `OTW_IBKR_PAPER_FUTURE` (`MNQZ6` by default) and must be trading. Orders rest
//! far from the market except the market round trip, which runs only when the account holds
//! none of that contract. The kill switch runs only when no order of the user's is working.
//!
//! Recovery (T22 race, T26/T27 dropped session, T32 restart) is driven from here: the session
//! is dropped by hand and a second broker instance stands in for a restart. A partial fill
//! (T03) cannot be forced on a paper account: IB's simulator fills a marketable order whole
//! against the quoted size, so it is covered by the mapper tests, not here.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::StreamExt;
use otw_connector_model::{Decimal, OrderAmend, OrderStatus, OrderType, Price, Qty, Side, TifKind};

use super::{AccountEvent, Broker, Lookup, OrderLeg, OrderTicket};
use crate::histdata::Connector;

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

fn d(v: f64) -> Decimal {
    Decimal::from_str_exact(&format!("{v:.2}")).unwrap_or_default()
}

/// `v` on the 0.25 grid, rounded down.
fn on_tick(v: f64) -> f64 {
    (v * 4.0).floor() / 4.0
}

fn key(tag: &str) -> String {
    let n = time::OffsetDateTime::now_utc().unix_timestamp_nanos() % 1_000_000_000_000;
    format!("otwt-{tag}-{n}")
}

fn leg(key: &str, side: Side, t: OrderType, qty: &str, limit: Option<f64>, stop: Option<f64>) -> OrderLeg {
    OrderLeg { limit: limit.map(d), stop: stop.map(d), tif: Some(TifKind::Day), ..OrderLeg::new(key, side, t, Decimal::from_str_exact(qty).unwrap_or_default()) }
}

fn ticket(ticker: &str, legs: Vec<OrderLeg>) -> OrderTicket {
    OrderTicket { ticker: ticker.into(), asset_type: "future".into(), legs, provider: None, connector: None, reference: None, reference_at: None, confirm_duplicate: false }
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

/// Poll the working orders until `pred` holds, up to `secs`.
async fn wait_orders(b: &dyn Broker, s: &HashMap<String, String>, secs: u64, pred: impl Fn(&[super::OpenOrder]) -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(secs);
    loop {
        if pred(&working(b, s).await) {
            return true;
        }
        if Instant::now() >= end {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

async fn held(b: &dyn Broker, s: &HashMap<String, String>, ticker: &str) -> Option<super::Position> {
    let http = super::client().unwrap();
    b.positions(&http, s).await.unwrap_or_default().into_iter().find(|p| p.symbol.eq_ignore_ascii_case(ticker) && p.qty != 0.0)
}

fn has(list: &[super::OpenOrder], id: &str) -> bool {
    list.iter().any(|o| o.id == id)
}

fn err_text<T>(r: &anyhow::Result<T>) -> String {
    match r {
        Ok(_) => "accepted".into(),
        Err(e) => format!("{e:#}"),
    }
}

#[tokio::test]
async fn ibkr_paper_trading_catalog() {
    let Ok(addr) = std::env::var("OTW_IBKR_PAPER") else { return };
    let (host, port) = addr.rsplit_once(':').expect("OTW_IBKR_PAPER is host:port");
    let s: HashMap<String, String> = [("host", host), ("port", port)].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    let fut = std::env::var("OTW_IBKR_PAPER_FUTURE").unwrap_or_else(|_| "MNQZ6".into());
    let b = super::ibkr::ibkr().expect("ibkr broker");
    let b: &dyn Broker = &b;
    let http = super::client().unwrap();
    let mut r = Report::default();
    println!("| Test | Result | Detail |\n| --- | --- | --- |");

    // Connection and account.
    let t = b.test(&http, &s).await;
    r.check("C01 connect", t.is_ok(), err_text(&t).replace("accepted", "connected"));
    let acc = b.account(&s, None).await;
    let paper = acc.as_ref().map(|a| !a.is_empty() && a.iter().all(|x| x.account.starts_with('D'))).unwrap_or(false);
    r.check("C02 paper account only", paper, format!("{} account line(s), all D…: {paper}", acc.as_ref().map(|a| a.len()).unwrap_or(0)));
    if !paper {
        panic!("not a paper login, stopping");
    }

    let quote = crate::histdata::native::ibkr().expect("native ibkr").best_quote(&http, &s, &fut, "future").await;
    let Ok((bid, ask)) = quote else {
        r.note("C03 quote", "FAIL", format!("{:#}", quote.err().unwrap()));
        panic!("no quote on {fut}");
    };
    r.check("C03 quote", bid > 0.0 && ask >= bid, format!("{fut} bid {bid} ask {ask}"));
    let low = on_tick(bid * 0.8);
    let high = on_tick(ask * 1.2);

    let before = working(b, &s).await;
    let user_orders = before.len();
    let user_pos = held(b, &s, &fut).await;
    r.note("C04 state before", "INFO", format!("{user_orders} working order(s), {fut} position {:?}", user_pos.as_ref().map(|p| p.qty)));

    // Account stream, collected for the whole run.
    let events: Arc<Mutex<Vec<AccountEvent>>> = Arc::default();
    match b.events(&s).await {
        Ok(Some(mut st)) => {
            let ev = events.clone();
            tokio::spawn(async move {
                while let Some(e) = st.next().await {
                    ev.lock().unwrap().push(e);
                }
            });
            r.note("C05 account stream", "PASS", "opened");
        }
        other => r.note("C05 account stream", "FAIL", format!("{:?}", other.err())),
    }

    // T01 limit rests, then cancel.
    let k = key("t01");
    let t0 = Instant::now();
    let p = b.place(&s, &ticket(&fut, vec![leg(&k, Side::Buy, OrderType::Limit, "1", Some(low), None)])).await;
    let ms = t0.elapsed().as_millis();
    match &p {
        Ok(v) if v.len() == 1 && v[0].key == k => {
            let id = v[0].id.clone();
            let tick = v[0].tick;
            let listed = wait_orders(b, &s, 5, |l| has(l, &id)).await;
            r.check("T01a limit buy rests", listed, format!("id {id}, ack {ms} ms, tick {tick:?}, listed {listed}"));
            let t0 = Instant::now();
            let c = b.cancel(&s, &id).await;
            let gone = wait_orders(b, &s, 5, |l| !has(l, &id)).await;
            r.check("T01b cancel", c.is_ok() && gone, format!("{} in {} ms, gone {gone}", err_text(&c), t0.elapsed().as_millis()));
            // Cancel again, and amend, an order already cancelled.
            let c2 = b.cancel(&s, &id).await;
            r.check("T22a cancel a cancelled order answers done", c2.is_ok(), err_text(&c2));
            let m2 = b.modify(&s, &id, &OrderAmend { limit: Some(Price::new(d(low - 1.0))), ..Default::default() }).await;
            r.check("T23a amend a cancelled order refused", m2.is_err(), err_text(&m2));
        }
        _ => r.note("T01a limit buy rests", "FAIL", err_text(&p)),
    }

    // T06 amend price and qty, amend off tick.
    let k = key("t06");
    let p = b.place(&s, &ticket(&fut, vec![leg(&k, Side::Buy, OrderType::Limit, "1", Some(low), None)])).await;
    if let Ok(v) = &p {
        let id = v[0].id.clone();
        let new_px = low - 1.0;
        let t0 = Instant::now();
        let m = b.modify(&s, &id, &OrderAmend { limit: Some(Price::new(d(new_px))), qty: Some(Qty::new(d(2.0))), ..Default::default() }).await;
        let ms = t0.elapsed().as_millis();
        let seen = wait_orders(b, &s, 5, |l| l.iter().any(|o| o.id == id && o.qty == 2.0 && o.limit_price == Some(new_px))).await;
        r.check("T06a amend price and qty", m.is_ok() && seen, format!("{} in {ms} ms, book shows qty 2 at {new_px}: {seen}", err_text(&m)));
        let m = b.modify(&s, &id, &OrderAmend { limit: Some(Price::new(d(new_px + 0.1))), ..Default::default() }).await;
        r.check("T06b amend off tick refused", m.as_ref().is_err_and(|e| format!("{e:#}").contains("tick")), err_text(&m));
        let m = b.modify(&s, &id, &OrderAmend { qty: Some(Qty::new(Decimal::ZERO)), ..Default::default() }).await;
        r.check("T06c amend qty 0 refused", m.is_err(), err_text(&m));
        let _ = b.cancel(&s, &id).await;
    } else {
        r.note("T06a amend price and qty", "FAIL", err_text(&p));
    }

    // T11 refusals before anything is sent (once the cancels above have landed).
    wait_orders(b, &s, 10, |l| l.len() == user_orders).await;
    let n0 = working(b, &s).await.len();
    for (id, l) in [
        ("T11a price off tick", leg(&key("t11a"), Side::Buy, OrderType::Limit, "1", Some(low + 0.1), None)),
        ("T11b fractional contract", leg(&key("t11b"), Side::Buy, OrderType::Limit, "0.5", Some(low), None)),
        ("T11c qty 0", leg(&key("t11c"), Side::Buy, OrderType::Limit, "0", Some(low), None)),
        ("T11d negative price", leg(&key("t11d"), Side::Buy, OrderType::Limit, "1", Some(-5.0), None)),
        ("T11e limit without price", leg(&key("t11e"), Side::Buy, OrderType::Limit, "1", None, None)),
    ] {
        let p = b.place(&s, &ticket(&fut, vec![l])).await;
        if let Ok(v) = &p {
            for x in v {
                let _ = b.cancel(&s, &x.id).await;
            }
        }
        // Refused, but by IB after the order went out: the spec wants it caught before.
        let by_ib = p.as_ref().is_err_and(|e| format!("{e:#}").contains("Interactive Brokers refused"));
        r.note(id, if p.is_ok() { "FAIL" } else if by_ib { "GAP" } else { "PASS" }, err_text(&p));
    }
    let mut gtd = leg(&key("t12"), Side::Buy, OrderType::Limit, "1", Some(low), None);
    gtd.tif = Some(TifKind::Gtd);
    let p = b.place(&s, &ticket(&fut, vec![gtd])).await;
    r.check("T12a GTD without expiry", p.is_err(), err_text(&p));
    let p = b.place(&s, &ticket(&format!("{fut}X"), vec![leg(&key("t12b"), Side::Buy, OrderType::Limit, "1", Some(low), None)])).await;
    r.check("T12b unknown contract refused, not guessed", p.is_err(), err_text(&p));
    let p = b.place(&s, &ticket("MNQ", vec![leg(&key("t12c"), Side::Buy, OrderType::Limit, "1", Some(low), None)])).await;
    if let Ok(v) = &p {
        for x in v {
            let _ = b.cancel(&s, &x.id).await;
        }
    }
    r.check("T12c continuous root refused", p.is_err(), err_text(&p));
    let n1 = working(b, &s).await.len();
    r.check("T11z nothing reached IB", n0 == n1, format!("working before {n0}, after {n1}"));

    // T05 stop and stop-limit rest above the market.
    let (k1, k2) = (key("t05a"), key("t05b"));
    let p = b
        .place(&s, &ticket(&fut, vec![leg(&k1, Side::Buy, OrderType::Stop, "1", None, Some(high)), leg(&k2, Side::Buy, OrderType::StopLimit, "1", Some(high + 1.0), Some(high))]))
        .await;
    match &p {
        Ok(v) if v.len() == 2 => {
            let ids: Vec<String> = v.iter().map(|x| x.id.clone()).collect();
            let listed = wait_orders(b, &s, 5, |l| ids.iter().all(|i| has(l, i))).await;
            let l = working(b, &s).await;
            let stop_px = l.iter().find(|o| o.id == ids[0]).and_then(|o| o.stop_price);
            r.check("T05 stop and stop-limit rest", listed && stop_px == Some(high), format!("ids {ids:?}, stop {stop_px:?}"));
            for i in &ids {
                let _ = b.cancel(&s, i).await;
            }
        }
        _ => r.note("T05 stop and stop-limit rest", "FAIL", err_text(&p)),
    }

    // T07 bracket: cancelling the entry takes its exits.
    let (ke, kt, ks) = (key("t07e"), key("t07t"), key("t07s"));
    let mut tp = leg(&kt, Side::Sell, OrderType::Limit, "1", Some(low + 20.0), None);
    tp.parent = Some(ke.clone());
    let mut sl = leg(&ks, Side::Sell, OrderType::Stop, "1", None, Some(low - 20.0));
    sl.parent = Some(ke.clone());
    let p = b.place(&s, &ticket(&fut, vec![leg(&ke, Side::Buy, OrderType::Limit, "1", Some(low), None), tp, sl])).await;
    match &p {
        Ok(v) if v.len() == 3 => {
            let ids: Vec<String> = v.iter().map(|x| x.id.clone()).collect();
            let listed = wait_orders(b, &s, 5, |l| ids.iter().all(|i| has(l, i))).await;
            let l = working(b, &s).await;
            let linked = ids[1..].iter().all(|i| l.iter().any(|o| &o.id == i && o.parent_id.as_deref() == Some(ids[0].as_str())));
            r.check("T07a bracket placed and linked", listed && linked, format!("ids {ids:?}, exits name the entry: {linked}"));
            let _ = b.cancel(&s, &ids[0]).await;
            let gone = wait_orders(b, &s, 8, |l| ids.iter().all(|i| !has(l, i))).await;
            r.check("T07b cancel entry cancels exits", gone, format!("all three gone: {gone}"));
        }
        _ => r.note("T07a bracket placed and linked", "FAIL", err_text(&p)),
    }

    // T09 IOC far from the market does not fill.
    let p = b.place(&s, &ticket(&fut, vec![{
        let mut l = leg(&key("t09"), Side::Buy, OrderType::Limit, "1", Some(low), None);
        l.tif = Some(TifKind::Ioc);
        l
    }])).await;
    r.note(
        "T09a IOC unfilled",
        match &p {
            Ok(_) => "PASS",
            Err(_) => "GAP",
        },
        format!("{} (spec: an IOC that does not fill is Cancelled, not a refusal)", err_text(&p)),
    );
    // T09 GTD expires on its own.
    let mut g = leg(&key("t09g"), Side::Buy, OrderType::Limit, "1", Some(low), None);
    g.tif = Some(TifKind::Gtd);
    g.expires = Some((time::OffsetDateTime::now_utc().unix_timestamp() + 70) * 1000);
    let p = b.place(&s, &ticket(&fut, vec![g])).await;
    if let Ok(v) = &p {
        let id = v[0].id.clone();
        let t0 = Instant::now();
        let gone = wait_orders(b, &s, 150, |l| !has(l, &id)).await;
        r.check("T09b GTD +70 s expires", gone, format!("gone after {} s", t0.elapsed().as_secs()));
        if !gone {
            let _ = b.cancel(&s, &id).await;
        }
    } else {
        r.note("T09b GTD +70 s expires", "FAIL", err_text(&p));
    }

    // T15a margin known ahead: the chart opened the instrument, a ticket beyond the account
    // is refused before it is sent.
    let _ = b.symbol_for(&s, &fut, "future").await;
    tokio::time::sleep(Duration::from_secs(4)).await;
    let n0 = working(b, &s).await.len();
    let t0 = Instant::now();
    let p = b.place(&s, &ticket(&fut, vec![leg(&key("t15a"), Side::Buy, OrderType::Limit, "20000", Some(low), None)])).await;
    let ms = t0.elapsed().as_millis();
    if let Ok(v) = &p {
        for x in v {
            let _ = b.cancel(&s, &x.id).await;
        }
    }
    let n1 = working(b, &s).await.len();
    r.check("T15a margin refused before sending", p.as_ref().is_err_and(|e| format!("{e:#}").contains("not enough margin")) && n0 == n1, format!("{} in {ms} ms", err_text(&p)));
    // A ticket that fits pays no margin call: the figures are at hand.
    let k = key("t15f");
    let t0 = Instant::now();
    let p = b.place(&s, &ticket(&fut, vec![leg(&k, Side::Buy, OrderType::Limit, "1", Some(low), None)])).await;
    let ms = t0.elapsed().as_millis();
    if let Ok(v) = &p {
        let _ = b.cancel(&s, &v[0].id).await;
    }
    r.check("T15c ticket that fits, no extra wait", p.is_ok(), format!("{} in {ms} ms", err_text(&p)));

    // T15b margin not known yet (another contract, never opened): IB turns it down, and its
    // reason reaches the user.
    let other = std::env::var("OTW_IBKR_PAPER_OTHER").unwrap_or_else(|_| "MESZ6".into());
    let p = b.place(&s, &OrderTicket { ticker: other.clone(), ..ticket(&fut, vec![leg(&key("t15b"), Side::Buy, OrderType::Limit, "20000", Some(on_tick(bid * 0.2)), None)]) }).await;
    match &p {
        Ok(v) => {
            let id = v[0].id.clone();
            let gone = wait_orders(b, &s, 8, |l| !has(l, &id)).await;
            tokio::time::sleep(Duration::from_secs(2)).await;
            let said: Vec<String> = events
                .lock()
                .unwrap()
                .iter()
                .filter_map(|e| match e {
                    AccountEvent::Done { id: i, status, reason } if *i == id => Some(format!("{status}: {}", reason.clone().unwrap_or_default())),
                    _ => None,
                })
                .collect();
            if !gone {
                let _ = b.cancel(&s, &id).await;
            }
            let reasoned = said.iter().any(|x| x.starts_with("rejected: ") && x.len() > "rejected: ".len());
            r.check("T15b IB rejection reaches the stream with its reason", gone && reasoned, format!("{other} id {id}; stream: {:?}", said.first()));
        }
        Err(_) => r.check("T15b IB rejection reaches the user with its reason", err_text(&p).contains("(201)"), format!("{other}: {}", err_text(&p))),
    }

    // Same client id twice.
    let k = key("dup");
    let p1 = b.place(&s, &ticket(&fut, vec![leg(&k, Side::Buy, OrderType::Limit, "1", Some(low), None)])).await;
    let p2 = b.place(&s, &ticket(&fut, vec![leg(&k, Side::Buy, OrderType::Limit, "1", Some(low), None)])).await;
    for v in [&p1, &p2].into_iter().flatten() {
        for x in v {
            let _ = b.cancel(&s, &x.id).await;
        }
    }
    r.check("T19z same client id twice refused", p1.is_ok() && p2.is_err(), format!("first {}, second {}", err_text(&p1), err_text(&p2)));

    // T08 OCA group, T31 external order, T34 kill switch.
    if user_orders == 0 {
        let (ka, kb) = (key("t08a"), key("t08b"));
        let mut a = leg(&ka, Side::Buy, OrderType::Limit, "1", Some(low), None);
        a.group = Some("g1".into());
        let mut c = leg(&kb, Side::Buy, OrderType::Limit, "1", Some(low - 5.0), None);
        c.group = Some("g1".into());
        let p = b.place(&s, &ticket(&fut, vec![a, c])).await;
        r.check("T08 OCA pair placed", p.as_ref().is_ok_and(|v| v.len() == 2), err_text(&p));

        // An order placed by another API client, as TWS or a script would.
        let ext = external(&addr, &fut, low - 10.0).await;
        let ext_seen = match &ext {
            Ok(id) => wait_orders(b, &s, 8, |l| has(l, &id.to_string())).await,
            Err(_) => false,
        };
        r.check("T31 external order listed", ext_seen, format!("{:?}", ext.as_ref().map_err(|e| e.to_string())));

        let n = working(b, &s).await.len();
        let t0 = Instant::now();
        let c = b.cancel_all(&s).await;
        let ms = t0.elapsed().as_millis();
        let empty = wait_orders(b, &s, 10, |l| l.is_empty()).await;
        r.check("T34 kill switch", c.is_ok() && empty, format!("{n} working, answered {:?} in {ms} ms, none left: {empty}", c.map_err(|e| e.to_string())));
    } else {
        r.note("T08/T31/T34", "N/A", format!("{user_orders} order(s) of the user working; global cancel would hit them"));
    }

    // T02/T04/T10 market round trip.
    if user_pos.is_none() {
        let k = key("t04");
        let t0 = Instant::now();
        let p = b.place(&s, &ticket(&fut, vec![leg(&k, Side::Buy, OrderType::Market, "1", None, None)])).await;
        let ms = t0.elapsed().as_millis();
        let mut pos = None;
        let end = Instant::now() + Duration::from_secs(20);
        while pos.is_none() && Instant::now() < end {
            pos = held(b, &s, &fut).await;
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        match (&p, &pos) {
            (Ok(_), Some(x)) => {
                let slip = x.avg_price.map(|a| (a - ask).abs());
                r.check(
                    "T04a market buy fills",
                    x.qty == 1.0 && x.side == "long",
                    format!("ack {ms} ms, {} {} at {:?} (ask {ask}, diff {slip:?}), mult {}, fee {:?} {}", x.side, x.qty, x.avg_price, x.multiplier, x.entry_fee, x.fee_currency),
                );
                let native = x.native_id.clone().unwrap_or_default();
                let t0 = Instant::now();
                let c = b.close_position(&s, &native, &x.account, &key("t10")).await;
                let ms = t0.elapsed().as_millis();
                let mut flat = false;
                let end = Instant::now() + Duration::from_secs(20);
                while !flat && Instant::now() < end {
                    flat = held(b, &s, &fut).await.is_none();
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
                r.check("T10 close position", c.is_ok() && flat, format!("{} in {ms} ms, flat {flat}", err_text(&c)));
            }
            _ => r.note("T04a market buy fills", "FAIL", format!("{} / position {:?}", err_text(&p), pos.map(|x| x.qty))),
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
        let ev = events.lock().unwrap();
        let fills = ev.iter().filter(|e| matches!(e, AccountEvent::Filled { .. } | AccountEvent::Done { .. })).count();
        let positions = ev.iter().filter(|e| matches!(e, AccountEvent::Position { .. })).count();
        let done_filled = ev.iter().filter(|e| matches!(e, AccountEvent::Done { status, .. } if status == "filled")).count();
        r.check("T04b stream reports fills and positions", done_filled >= 2 && positions >= 2, format!("{} events: {fills} fill/done, {done_filled} done filled, {positions} position", ev.len()));
    } else {
        r.note("T04/T10", "N/A", format!("the account holds {fut}; a close would touch it"));
    }

    // T22 a cancel that races a fill: a market buy cancelled at once ends filled, and the
    // cancel is no error to the user.
    if user_pos.is_none() && held(b, &s, &fut).await.is_none() {
        let k = key("t22");
        let p = b.place(&s, &ticket(&fut, vec![leg(&k, Side::Buy, OrderType::Market, "1", None, None)])).await;
        match &p {
            Ok(v) => {
                let c = b.cancel(&s, &v[0].id).await;
                let mut found = None;
                let end = Instant::now() + Duration::from_secs(10);
                while Instant::now() < end {
                    found = b.lookup(&s, &fut, "future", &k).await.ok();
                    if matches!(&found, Some(Lookup::Found { status, .. }) if status.is_final()) {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
                let detail = format!("cancel {}, lookup {found:?}", err_text(&c));
                match &found {
                    Some(Lookup::Found { status: OrderStatus::Filled, .. }) => r.check("T22b cancel races a fill: filled, no error", c.is_ok(), detail),
                    // The cancel landed before the fill: the race was not run, nothing wrong.
                    Some(Lookup::Found { status: OrderStatus::Cancelled, .. }) if c.is_ok() => r.note("T22b cancel races a fill", "N/A", detail),
                    _ => r.note("T22b cancel races a fill: filled, no error", "FAIL", detail),
                }
            }
            Err(_) => r.note("T22b cancel races a fill: filled, no error", "FAIL", err_text(&p)),
        }
        if let Some(x) = held(b, &s, &fut).await {
            let _ = b.close_position(&s, &x.native_id.clone().unwrap_or_default(), &x.account, &key("t22z")).await;
            let end = Instant::now() + Duration::from_secs(20);
            while held(b, &s, &fut).await.is_some() && Instant::now() < end {
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
    } else {
        r.note("T22b", "N/A", format!("the account holds {fut}; a fill would touch it"));
    }

    // T26/T27/T32 with an order working: the session drops and comes back, then a new broker
    // stands in for a restart. The book is read again from IB each time; nothing is resent.
    let k = key("t26");
    let p = b.place(&s, &ticket(&fut, vec![leg(&k, Side::Buy, OrderType::Limit, "1", Some(low), None)])).await;
    match &p {
        Ok(v) => {
            let id = v[0].id.clone();
            wait_orders(b, &s, 5, |l| has(l, &id)).await;
            let resyncs = || events.lock().unwrap().iter().filter(|e| matches!(e, AccountEvent::Resync)).count();
            let before = resyncs();
            let n0 = working(b, &s).await.len();
            let session = otw_connector_providers::ibkr::gateway::session::get(&s).await.expect("ibkr session");
            session.reset().await;
            let t0 = Instant::now();
            let back = wait_orders(b, &s, 10, |l| has(l, &id)).await;
            let found = b.lookup(&s, &fut, "future", &k).await;
            let same = matches!(&found, Ok(Lookup::Found { id: i, status: OrderStatus::New, .. }) if *i == id);
            r.check("T26 session dropped, book read again", back && same, format!("listed again after {} ms: {back}, lookup {:?}", t0.elapsed().as_millis(), found.map_err(|e| e.to_string())));
            let end = Instant::now() + Duration::from_secs(15);
            while resyncs() == before && Instant::now() < end {
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            r.check("T27 stream resubscribes and says Resync", resyncs() > before, format!("{} Resync after the drop (IB's 1101 itself cannot be sent from here)", resyncs() - before));

            // T32: a broker built afresh reads the same order from IB and sends nothing again.
            // In one process the sent-id list survives; a real restart seeds it from IB.
            let fresh = super::ibkr::ibkr().expect("ibkr broker");
            let fresh: &dyn Broker = &fresh;
            let listed = has(&working(fresh, &s).await, &id);
            let found = fresh.lookup(&s, &fut, "future", &k).await;
            let same = matches!(&found, Ok(Lookup::Found { id: i, .. }) if *i == id);
            let again = fresh.place(&s, &ticket(&fut, vec![leg(&k, Side::Buy, OrderType::Limit, "1", Some(low), None)])).await;
            if let Ok(v) = &again {
                for x in v {
                    let _ = fresh.cancel(&s, &x.id).await;
                }
            }
            let n1 = working(fresh, &s).await.len();
            r.check(
                "T32 restart: state from IB, nothing resent",
                listed && same && again.is_err() && n0 == n1,
                format!("listed {listed}, lookup {:?}, same key again: {}, working {n0} then {n1}", found.map_err(|e| e.to_string()), err_text(&again)),
            );
            let _ = b.cancel(&s, &id).await;
            let gone = wait_orders(b, &s, 5, |l| !has(l, &id)).await;
            let after = b.lookup(&s, &fut, "future", &k).await;
            r.check("T20z lookup of a cancelled order", gone && matches!(&after, Ok(Lookup::Found { status: OrderStatus::Cancelled, .. })), format!("{:?}", after.map_err(|e| e.to_string())));
        }
        Err(_) => r.note("T26 session dropped, book read again", "FAIL", err_text(&p)),
    }
    let never = b.lookup(&s, &fut, "future", &key("never-sent")).await;
    r.check("T20 a client id IB never saw is not found", matches!(never, Ok(Lookup::NotFound)), format!("{:?}", never.map_err(|e| e.to_string())));

    // Leftovers.
    let left: Vec<String> = working(b, &s).await.into_iter().filter(|o| !before.iter().any(|x| x.id == o.id)).map(|o| o.id).collect();
    for id in &left {
        let _ = b.cancel(&s, id).await;
    }
    r.check("Z01 nothing left working", left.is_empty(), format!("left {left:?}"));
    let after = held(b, &s, &fut).await.map(|p| p.qty);
    r.check("Z02 position as before", after == user_pos.map(|p| p.qty), format!("{fut} {after:?}"));

    let fails: Vec<&String> = r.rows.iter().filter(|x| x.1 == "FAIL").map(|x| &x.0).collect();
    println!("\nPASS {}  FAIL {}  GAP {}  N/A {}", r.rows.iter().filter(|x| x.1 == "PASS").count(), fails.len(), r.rows.iter().filter(|x| x.1 == "GAP").count(), r.rows.iter().filter(|x| x.1 == "N/A").count());
    assert!(fails.is_empty(), "failed: {fails:?}");
}

/// A resting buy placed by a second API client (client id 77), as TWS or another script would:
/// an order OTW did not send.
async fn external(addr: &str, local: &str, price: f64) -> anyhow::Result<i32> {
    use ibapi::contracts::{Contract, SecurityType};
    use ibapi::orders::{Action, Order};
    let client = ibapi::Client::connect(addr, 77).await?;
    let probe = Contract { security_type: SecurityType::Future, local_symbol: local.into(), exchange: "CME".into(), currency: "USD".into(), ..Default::default() };
    let c = client.contract_details(&probe).await?.into_iter().next().map(|d| d.contract).ok_or_else(|| anyhow::anyhow!("no contract {local}"))?;
    let id = client.next_order_id();
    let order = Order { action: Action::Buy, order_type: "LMT".into(), total_quantity: 1.0, limit_price: Some(price), transmit: true, ..Order::default() };
    let mut sub = client.place_order(id, &c, &order).await?;
    let _ = tokio::time::timeout(Duration::from_secs(5), sub.next()).await;
    // Keep the client alive long enough for IB to register the order, then let it go: the
    // order stays working at IB.
    tokio::time::sleep(Duration::from_secs(1)).await;
    Ok(id)
}

