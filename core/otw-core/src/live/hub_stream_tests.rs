//! The hub as a stream processor: what reaches a viewer when the socket repeats itself,
//! answers late, goes quiet and comes back, says something the hub cannot use, or talks
//! faster than a viewer reads. Every update here is handed to the hub's own routing, never
//! to a real venue, and nothing can reach the network: the REST paths are held busy.

use super::*;
use std::sync::atomic::Ordering;
use time::macros::datetime;

const T0: OffsetDateTime = datetime!(2026-03-02 10:00:00 UTC);

fn minute(n: i64) -> OffsetDateTime {
    T0 + TimeDuration::minutes(n)
}

fn inner() -> Arc<Inner> {
    Arc::new(Inner {
        pool: PgPool::connect_lazy("postgres://unused@127.0.0.1:1/unused").unwrap(),
        http: Client::new(),
        cipher: SecretCipher::from_master("t").unwrap(),
        sessions: Mutex::new(HashMap::new()),
        next_gen: Mutex::new(0),
        anchors: Mutex::new(HashMap::new()),
        points: broadcast::channel(4).0,
    })
}

/// One subscription on `provider`, broadcast-only, with the real channel capacity. Its REST
/// side is marked busy, so a gap it finds is kept as owed instead of being fetched.
fn run(provider: &str, ticker: &str, tf: &str) -> SubRun {
    let connector = stream_connector_for(provider).unwrap();
    let (tx, _rx) = broadcast::channel(CHANNEL_CAP);
    let (anchor_tx, _arx) = mpsc::unbounded_channel();
    let target = LiveTarget {
        dataset_id: None,
        connector_id: Uuid::new_v4(),
        connector: "test".into(),
        provider: provider.into(),
        asset_type: "crypto".into(),
        ticker: ticker.into(),
        timeframe: tf.into(),
        source: SourcePref::Candles,
    };
    let spec = SubSpec {
        target,
        tx,
        current: Arc::new(Mutex::new(None)),
        quote: Arc::new(Mutex::new(None)),
        status: Arc::new(Mutex::new(LiveStatus::connecting())),
    };
    let run = build_run(connector.as_ref(), spec, anchor_tx).unwrap();
    run.backfilling.store(true, Ordering::SeqCst);
    run.settling.store(true, Ordering::SeqCst);
    run
}

fn ev(ts: OffsetDateTime, close: f64, closed: bool) -> LiveEvent {
    LiveEvent { bar_ts: ts, open: close, high: close, low: close, close, volume: 1.0, closed, provider_ts: ts }
}

struct Rig {
    inner: Arc<Inner>,
    subs: HashMap<String, SubRun>,
    meta: LiveTarget,
    key: String,
    channel: String,
    handle: super::super::FeedHandle,
    _feed_cmds: mpsc::Receiver<super::super::FeedCmd>,
}

impl Rig {
    fn new(run: SubRun) -> (Self, broadcast::Receiver<LiveMsg>) {
        let rx = run.tx.subscribe();
        let (cmd_tx, cmd_rx) = mpsc::channel(8);
        let rig = Rig {
            inner: inner(),
            meta: run.target.clone(),
            key: run.inst.key(),
            channel: run.channel.clone(),
            subs: HashMap::from([(run.inst.key(), run)]),
            handle: super::super::FeedHandle::new(cmd_tx),
            _feed_cmds: cmd_rx,
        };
        (rig, rx)
    }

    async fn send(&mut self, payload: Payload) {
        let update = FeedUpdate { channel: self.channel.clone(), payload };
        route(&self.inner, &HashMap::new(), &self.meta, &mut self.subs, &self.handle, update).await.unwrap();
    }

    async fn bar(&mut self, e: LiveEvent) {
        self.send(Payload::Bar(e)).await;
    }

    fn run(&self) -> &SubRun {
        &self.subs[&self.key]
    }

    fn owed(&self) -> Option<(OffsetDateTime, OffsetDateTime)> {
        *self.run().gap_pending.lock().unwrap()
    }
}

/// Every bar a viewer has been sent so far, as (start, close, closed).
fn bars(rx: &mut broadcast::Receiver<LiveMsg>) -> Vec<(OffsetDateTime, f64, bool)> {
    let mut out = Vec::new();
    loop {
        match rx.try_recv() {
            Ok(LiveMsg::Bar(e)) => out.push((e.bar_ts, e.close, e.closed)),
            Ok(_) => {}
            Err(broadcast::error::TryRecvError::Empty) => return out,
            Err(e) => panic!("viewer channel: {e:?}"),
        }
    }
}

// ── Duplicates ──────────────────────────────────────────────────────────────

/// A venue that repeats its closed candle (Binance resends the final kline on a reconnect)
/// makes the viewer replace the bar in place: it is sent again, the series does not move,
/// and no hole is invented.
#[tokio::test]
async fn a_repeated_closed_bar_is_resent_in_place_and_opens_no_gap() {
    let (mut rig, mut rx) = Rig::new(run("binance", "BTCUSDT", "1m"));
    rig.bar(ev(minute(0), 1.0, true)).await;
    rig.bar(ev(minute(1), 2.0, true)).await;
    rig.bar(ev(minute(1), 2.0, true)).await;
    rig.bar(ev(minute(2), 3.0, true)).await;
    assert_eq!(
        bars(&mut rx),
        vec![(minute(0), 1.0, true), (minute(1), 2.0, true), (minute(1), 2.0, true), (minute(2), 3.0, true)]
    );
    assert_eq!(rig.run().last_closed, Some(minute(2)));
    assert_eq!(rig.owed(), None, "no bucket is missing");
}

/// Forming updates of one bar replace each other: the snapshot a late viewer gets is the
/// newest, and the bar's close clears it.
#[tokio::test]
async fn forming_updates_replace_each_other_and_the_close_clears_the_snapshot() {
    let (mut rig, _rx) = Rig::new(run("binance", "BTCUSDT", "1m"));
    for close in [1.0, 1.5, 1.25] {
        rig.bar(ev(minute(0), close, false)).await;
    }
    assert_eq!(rig.run().current.lock().unwrap().as_ref().map(|e| e.close), Some(1.25));
    rig.bar(ev(minute(0), 1.3, true)).await;
    assert!(rig.run().current.lock().unwrap().is_none());
}

// ── Out of order ────────────────────────────────────────────────────────────

/// A closed bar older than the newest one already closed is a straggler. It must not pull
/// the series back (which makes the next close look like a gap and fires a REST backfill
/// for a bar that was received) nor clear the bar forming now.
#[tokio::test]
async fn a_late_closed_bar_does_not_move_the_series_back() {
    let (mut rig, _rx) = Rig::new(run("binance", "BTCUSDT", "1m"));
    for m in 0..3 {
        rig.bar(ev(minute(m), m as f64, true)).await;
    }
    rig.bar(ev(minute(3), 3.0, false)).await;
    // The venue answers late for a minute already closed.
    rig.bar(ev(minute(1), 1.0, true)).await;
    assert_eq!(rig.run().last_closed, Some(minute(2)), "the series stays where it was");
    assert_eq!(rig.run().current.lock().unwrap().as_ref().map(|e| e.bar_ts), Some(minute(3)), "the forming bar survives");
    rig.bar(ev(minute(3), 3.0, true)).await;
    assert_eq!(rig.owed(), None, "minute 2 was received: nothing is missing");
}

/// On a fold from trades, a print stamped inside a bucket already closed is dropped: the
/// closed candle the viewer has is never rewritten, and the forming one is not touched.
#[tokio::test]
async fn a_late_trade_for_a_closed_bucket_is_dropped() {
    let (mut rig, mut rx) = Rig::new(run("coinbase", "BTC-USD", "1m"));
    assert!(rig.run().agg.is_some(), "coinbase minutes are folded from trades");
    let trade = |price: f64, ts: OffsetDateTime| Payload::Trade { price, size: 1.0, ts };
    rig.send(trade(10.0, minute(0) + TimeDuration::seconds(10))).await;
    rig.send(trade(11.0, minute(1) + TimeDuration::seconds(5))).await;
    rig.send(trade(99.0, minute(0) + TimeDuration::seconds(30))).await;
    assert_eq!(
        bars(&mut rx),
        vec![(minute(0), 10.0, false), (minute(0), 10.0, true), (minute(1), 11.0, false)],
        "the straggler at 99 reaches nobody"
    );
    let cur = rig.run().current.lock().unwrap().clone().unwrap();
    assert_eq!((cur.bar_ts, cur.high, cur.volume), (minute(1), 11.0, 1.0));
}

// ── Disconnect and reconnect ────────────────────────────────────────────────

/// A reconnect resyncs from the REST seed. The updates that arrived while it was in flight
/// are held, then merged: history the seed covers is not sent twice (only its last bar is
/// resent, in place), nothing between the seed and the live bars is missing, and the series
/// continues from the seed's last bar.
#[tokio::test]
async fn a_reconnect_resyncs_from_the_seed_without_gap_or_replayed_history() {
    let (mut rig, mut rx) = Rig::new(run("binance", "BTCUSDT", "1m"));
    // The socket is back and already talking while the seed is out.
    begin_sync(&rig.inner, &HashMap::new(), rig.key.clone(), rig.subs.get_mut(&rig.key).unwrap(), &mut JoinSet::new());
    rig.subs.get_mut(&rig.key).unwrap().seeding = true;
    rig.bar(ev(minute(1), 1.0, true)).await;
    rig.bar(ev(minute(2), 2.0, true)).await;
    rig.bar(ev(minute(3), 3.0, false)).await;
    assert!(bars(&mut rx).is_empty(), "held while the seed is in flight");

    let seed: Vec<Bar> = (0..3).map(|m| ev(minute(m), m as f64, true).as_bar()).collect();
    finish_seed(&rig.inner, &HashMap::new(), &rig.meta, &mut rig.subs, rig.key.clone(), Ok(seed), None).await;

    let got = bars(&mut rx);
    assert_eq!(
        got,
        vec![
            // The seed, oldest first.
            (minute(0), 0.0, true),
            (minute(1), 1.0, true),
            (minute(2), 2.0, true),
            // The held updates from the seed's last bar on: minute 1 is history and dropped,
            // minute 2 is the seed's own bar sent again in place, then the forming minute.
            (minute(2), 2.0, true),
            (minute(3), 3.0, false),
        ]
    );
    let closed: Vec<OffsetDateTime> = got.iter().filter(|b| b.2).map(|b| b.0).collect();
    assert_eq!(closed.iter().filter(|t| **t == minute(1)).count(), 1, "minute 1 reaches the viewer once");
    assert!(closed.windows(2).all(|w| w[1] >= w[0]), "never backwards: {closed:?}");
    assert_eq!(rig.run().last_closed, Some(minute(2)));
    assert_eq!(rig.owed(), None);

    // The live bars carry on from there with no hole.
    rig.bar(ev(minute(3), 3.0, true)).await;
    assert_eq!(rig.owed(), None);
    assert_eq!(rig.run().last_closed, Some(minute(3)));
}

/// A seed that failed still knows where the recorded series stops: the first live close
/// after the outage owes exactly the hole between the two, to be fetched.
#[tokio::test]
async fn a_failed_seed_owes_the_hole_since_the_stored_series() {
    let (mut rig, _rx) = Rig::new(run("binance", "BTCUSDT", "1m"));
    rig.subs.get_mut(&rig.key).unwrap().seeding = true;
    let err = Err(anyhow!("venue unreachable"));
    finish_seed(&rig.inner, &HashMap::new(), &rig.meta, &mut rig.subs, rig.key.clone(), err, Some(minute(0))).await;
    assert_eq!(rig.run().last_closed, Some(minute(0)));
    rig.bar(ev(minute(5), 5.0, true)).await;
    assert_eq!(rig.owed(), Some((minute(1), minute(5))), "minutes 1 to 4 are owed, the new close is not");
}

/// A viewer that drops and comes back inside the teardown grace rejoins the running feed:
/// it is handed the bar forming right now, so its chart has no hole, and the updates that
/// follow reach it.
#[tokio::test]
async fn a_viewer_that_reconnects_gets_the_forming_bar_and_the_updates_after_it() {
    let hub = LiveHub::new(
        PgPool::connect_lazy("postgres://unused@127.0.0.1:1/unused").unwrap(),
        Client::new(),
        SecretCipher::from_master("t").unwrap(),
    );
    let provider = "push-0000c0de";
    crate::histdata::kinds::register(
        provider,
        "Script",
        &serde_json::json!({ "timeframes": "1h", "columns": "{\"ts\": 0, \"close\": 1}", "ts_unit": "s" }),
    );
    let connector = Uuid::new_v4();
    let target = LiveTarget {
        dataset_id: None,
        connector_id: connector,
        connector: "test".into(),
        provider: provider.into(),
        asset_type: "other".into(),
        ticker: "ACME".into(),
        timeframe: "1h".into(),
        source: SourcePref::Candles,
    };
    // A bucket far in the future, so the close timer cannot end it during the test.
    let hour = super::super::bar_start(OffsetDateTime::now_utc(), 3600) + TimeDuration::days(30);
    let tick = |price: f64, s: i64| Ingest::Tick { price, size: 1.0, ts: hour + TimeDuration::seconds(s) };

    let mut first = hub.subscribe(target.clone()).unwrap();
    assert_eq!(hub.ingest(connector, "ACME", "other", tick(10.0, 1)), 1);
    assert_eq!(hub.ingest(connector, "ACME", "other", tick(12.0, 2)), 1);
    async fn bar_closing_at(rx: &mut broadcast::Receiver<LiveMsg>, close: f64) -> LiveEvent {
        loop {
            match tokio::time::timeout(Duration::from_secs(5), rx.recv()).await {
                Ok(Ok(LiveMsg::Bar(e))) if e.close == close => return e,
                Ok(Ok(_)) => continue,
                other => panic!("no bar at {close}: {other:?}"),
            }
        }
    }
    let seen = bar_closing_at(&mut first.rx, 12.0).await;
    drop(first);

    let mut back = hub.subscribe(target).unwrap();
    let snap = back.snapshot.clone().expect("the forming bar is handed over on rejoin");
    assert_eq!((snap.bar_ts, snap.open, snap.close, snap.volume), (seen.bar_ts, 10.0, 12.0, 2.0));
    assert_eq!(back.status.state, "live");
    assert_eq!(hub.ingest(connector, "ACME", "other", tick(13.0, 3)), 1);
    let next = bar_closing_at(&mut back.rx, 13.0).await;
    assert_eq!((next.bar_ts, next.open, next.volume), (hour, 10.0, 3.0), "the same bar, extended");
    crate::histdata::kinds::forget(provider);
}

// ── Messages the hub cannot use ─────────────────────────────────────────────

/// An update on a channel nobody holds, or a payload that does not fit the subscription
/// (a lone trade on a candle feed), is dropped without a word to the viewer and without
/// disturbing what it was showing.
#[tokio::test]
async fn an_update_the_hub_cannot_use_is_ignored() {
    let (mut rig, mut rx) = Rig::new(run("binance", "BTCUSDT", "1m"));
    rig.bar(ev(minute(0), 1.0, false)).await;
    assert_eq!(bars(&mut rx), vec![(minute(0), 1.0, false)]);

    let stray = FeedUpdate { channel: "nobody|listens".into(), payload: Payload::Bar(ev(minute(5), 9.0, true)) };
    route(&rig.inner, &HashMap::new(), &rig.meta, &mut rig.subs, &rig.handle, stray).await.unwrap();
    rig.send(Payload::Trade { price: 9.0, size: 1.0, ts: minute(0) }).await;

    assert!(bars(&mut rx).is_empty());
    assert_eq!(rig.run().current.lock().unwrap().as_ref().map(|e| e.close), Some(1.0));
    assert_eq!(rig.run().last_closed, None);
    rig.bar(ev(minute(0), 1.5, false)).await;
    assert_eq!(bars(&mut rx), vec![(minute(0), 1.5, false)], "the feed carries on");
}

/// A passing fault on the instrument says `retrying` and keeps the subscription; the next
/// bar says `live` again. A terminal one stops this instrument and empties its snapshot,
/// and later bars on its channel are not applied.
#[tokio::test]
async fn a_fault_on_the_instrument_is_reported_and_only_a_terminal_one_stops_it() {
    let (mut rig, mut rx) = Rig::new(run("binance", "BTCUSDT", "1m"));
    let status = |rx: &mut broadcast::Receiver<LiveMsg>| {
        let mut out = Vec::new();
        while let Ok(m) = rx.try_recv() {
            if let LiveMsg::Status(s) = m {
                out.push(s.state);
            }
        }
        out
    };
    rig.send(Payload::Fault { fault: LiveFault::Transport, message: "blip".into() }).await;
    assert!(!rig.run().stopped);
    rig.bar(ev(minute(0), 1.0, false)).await;
    assert_eq!(status(&mut rx), vec!["retrying", "live"]);

    rig.send(Payload::Fault { fault: LiveFault::Symbol, message: "unknown symbol".into() }).await;
    assert!(rig.run().stopped);
    assert!(rig.run().current.lock().unwrap().is_none());
    assert_eq!(status(&mut rx), vec!["stopped"]);
    rig.bar(ev(minute(1), 2.0, true)).await;
    assert!(bars(&mut rx).is_empty(), "a stopped subscription takes no more bars");
    assert_eq!(rig.run().last_closed, None);
}

// ── A fast stream ───────────────────────────────────────────────────────────

/// A burst far larger than the viewer channel never grows it past its capacity: a viewer
/// that did not read is told how much it missed, then reads the newest updates, ending on
/// the last one, and the snapshot holds only the latest bar.
#[tokio::test]
async fn a_burst_is_bounded_by_the_channel_and_a_slow_viewer_skips_to_the_newest() {
    let (mut rig, mut rx) = Rig::new(run("binance", "BTCUSDT", "1m"));
    const BURST: usize = 10 * CHANNEL_CAP;
    for i in 0..BURST {
        rig.bar(ev(minute(0), i as f64, false)).await;
        assert!(rig.run().tx.len() <= CHANNEL_CAP, "the backlog stays bounded");
    }
    // One status message (`connecting` to `live`) went out ahead of the bars.
    let sent = BURST + 1;
    match rx.try_recv() {
        Err(broadcast::error::TryRecvError::Lagged(n)) => assert_eq!(n as usize, sent - CHANNEL_CAP),
        other => panic!("expected the viewer to be told it lagged, got {other:?}"),
    }
    let rest = bars(&mut rx);
    assert_eq!(rest.len(), CHANNEL_CAP);
    assert_eq!(rest.last().map(|b| b.1), Some((BURST - 1) as f64), "ends on the newest update");
    assert!(rest.windows(2).all(|w| w[1].1 > w[0].1), "in order");
    assert_eq!(rig.run().current.lock().unwrap().as_ref().map(|e| e.close), Some((BURST - 1) as f64));
}
