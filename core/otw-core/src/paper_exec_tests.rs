//! The sync's decisions on scripted books (pure), then the whole path on a mock venue over a
//! scratch database (`OTW_TEST_DATABASE_URL`, skipped without it).

#![allow(clippy::unwrap_used)]

use super::*;

fn d(v: &str) -> Decimal {
    v.parse().unwrap()
}

fn at(t: &str) -> OffsetDateTime {
    OffsetDateTime::parse(t, &time::format_description::well_known::Rfc3339).unwrap()
}

const SESSION: Uuid = Uuid::from_u128(0x5e55);
const T: &str = "BTCUSDT";

fn rules() -> Rules {
    Rules {
        armed_at: Some(at("2026-10-28T10:00:00Z")),
        active: true,
        native: true,
        brackets: true,
        oca: true,
        amend: true,
        reduce_only: true,
        step: Some(d("0.001")),
        ..Default::default()
    }
}

fn entry(long: bool, signal: &str) -> Entry {
    Entry { long, qty: 0.12345, price: 100.0, signal_ts: signal.into(), decided_at: at(signal) + time::Duration::minutes(1), stop: Some(95.0), target: Some(110.0) }
}

fn leg(role: Role, intent: &str, buy: bool, qty: &str, filled: &str, working: bool) -> Leg {
    Leg {
        client_id: client_id(SESSION, intent, role),
        intent: intent.into(),
        role,
        buy,
        qty: d(qty),
        limit: None,
        stop: None,
        engine_price: None,
        position: String::new(),
        working,
        filled: d(filled),
        done: !working,
        refused: false,
    }
}

/// The session's long position of 0.123 from one filled market entry, with its exits working.
fn long_position(stop: &str, target: &str) -> Actual {
    let e = leg(Role::Entry, "entry|BTCUSDT|long|2026-10-28T10:05:00Z", true, "0.123", "0.123", false);
    let mut s = leg(Role::Stop, "entry|BTCUSDT|long|2026-10-28T10:05:00Z|stop", false, "0.123", "0", true);
    s.stop = Some(d(stop));
    s.position = e.client_id.clone();
    let mut t = leg(Role::Target, "entry|BTCUSDT|long|2026-10-28T10:05:00Z|target", false, "0.123", "0", true);
    t.limit = Some(d(target));
    t.position = e.client_id.clone();
    Actual { held: Some(d("0.123")), baseline: Decimal::ZERO, legs: vec![e, s, t], adopted: vec![] }
}

fn placed(p: &Plan) -> Vec<&Vec<LegSpec>> {
    p.actions.iter().filter_map(|a| if let Action::Place(l) = a { Some(l) } else { None }).collect()
}

fn cancelled(p: &Plan) -> Vec<&str> {
    p.actions.iter().filter_map(|a| if let Action::Cancel { client_id } = a { Some(client_id.as_str()) } else { None }).collect()
}

// ── Entries ──────────────────────────────────────────────────────────────────

/// An entry the close decided after arming leaves at once as a market order, cut down to the
/// venue's step, its stop and target attached one-cancels-other. The same close read again
/// sends nothing: the decision is already written.
#[test]
fn an_entry_decided_at_the_close_is_sent_once_at_market_with_its_exits() {
    let eng = Intended { entry: Some(entry(true, "2026-10-28T10:05:00Z")), hold: Some(Hold { long: true, stop: Some(95.0), target: Some(110.0) }), limits: vec![] };
    let flat = Actual { held: Some(Decimal::ZERO), ..Default::default() };
    let p = plan(SESSION, T, &eng, &flat, &rules());
    assert_eq!(p.degraded, None);
    let tickets = placed(&p);
    assert_eq!(tickets.len(), 1, "{p:?}");
    let legs = tickets[0];
    assert_eq!((legs[0].role, legs[0].kind, legs[0].buy, legs[0].qty), (Role::Entry, OrderType::Market, true, d("0.123")));
    assert_eq!(legs[0].engine_qty, Some(0.12345), "the engine's size is kept beside what was sent");
    let (stop, target) = (&legs[1], &legs[2]);
    assert_eq!((stop.role, stop.kind, stop.buy, stop.stop), (Role::Stop, OrderType::Stop, false, Some(d("95"))));
    assert_eq!((target.role, target.kind, target.limit), (Role::Target, OrderType::Limit, Some(d("110"))));
    assert_eq!(stop.parent.as_deref(), Some(legs[0].client_id.as_str()));
    assert!(stop.group.is_some() && stop.group == target.group, "one-cancels-other");

    // Sent: the legs are on record, the same close sends nothing again.
    let sent = Actual {
        held: Some(Decimal::ZERO),
        legs: legs.iter().map(|l| Leg { client_id: l.client_id.clone(), intent: l.intent.clone(), role: l.role, buy: l.buy, qty: l.qty, limit: l.limit, stop: l.stop, engine_price: l.engine_price, position: l.position.clone(), working: true, filled: Decimal::ZERO, done: false, refused: false }).collect(),
        ..Default::default()
    };
    assert!(placed(&plan(SESSION, T, &eng, &sent, &rules())).is_empty(), "never sent twice");
    // Refused the first time: still never sent again for the same close.
    let mut refused = sent.clone();
    for l in &mut refused.legs {
        (l.working, l.refused, l.done) = (false, true, true);
    }
    assert!(placed(&plan(SESSION, T, &eng, &refused, &rules())).is_empty(), "a refused decision is not retried");
}

/// The close before arming decided an entry the user never armed for: it is not sent.
#[test]
fn an_entry_decided_before_arming_is_not_sent() {
    let eng = Intended { entry: Some(entry(true, "2026-10-28T09:58:00Z")), ..Default::default() };
    let p = plan(SESSION, T, &eng, &Actual::default(), &rules());
    assert!(p.actions.is_empty(), "{p:?}");
}

/// Watched exits: the entry goes alone, no order rests for the stop or the target.
#[test]
fn watched_exits_send_the_entry_alone() {
    let eng = Intended { entry: Some(entry(false, "2026-10-28T10:05:00Z")), ..Default::default() };
    let r = Rules { native: false, ..rules() };
    let p = plan(SESSION, T, &eng, &Actual::default(), &r);
    let tickets = placed(&p);
    assert_eq!(tickets.len(), 1);
    assert_eq!(tickets[0].len(), 1);
    assert!(!tickets[0][0].buy, "a short entry sells");
}

/// Refused, never resized: under the step, or over the session's maximum.
#[test]
fn a_size_the_venue_or_the_session_cannot_take_is_refused_not_resized() {
    let mut e = entry(true, "2026-10-28T10:05:00Z");
    e.qty = 0.0004;
    let p = plan(SESSION, T, &Intended { entry: Some(e), ..Default::default() }, &Actual::default(), &rules());
    assert!(p.actions.is_empty());
    assert!(p.notes[0].contains("under the venue's step"), "{:?}", p.notes);
    let r = Rules { max_qty: Some(d("0.1")), ..rules() };
    let p = plan(SESSION, T, &Intended { entry: Some(entry(true, "2026-10-28T10:05:00Z")), ..Default::default() }, &Actual::default(), &r);
    assert!(p.actions.is_empty());
    assert!(p.notes[0].contains("over this session's maximum"), "{:?}", p.notes);
}

// ── Resting limits ───────────────────────────────────────────────────────────

/// A limit the engine rests and the broker does not have is placed; when the signal leaves
/// and the engine drops it, the session's order is cancelled.
#[test]
fn a_resting_limit_is_placed_then_cancelled_when_the_engine_drops_it() {
    let limit = Limit { long: true, qty: 1.0, price: 98.5, signal_ts: Some("2026-10-28T10:05:00Z".into()), decided_at: Some(at("2026-10-28T10:06:00Z")), stop: None, target: None };
    let eng = Intended { limits: vec![limit.clone()], ..Default::default() };
    let p = plan(SESSION, T, &eng, &Actual::default(), &rules());
    let tickets = placed(&p);
    assert_eq!(tickets.len(), 1);
    assert_eq!((tickets[0][0].role, tickets[0][0].kind, tickets[0][0].limit), (Role::Limit, OrderType::Limit, Some(d("98.5"))));

    // Resting at the broker, the engine still holds it (its signal_ts is gone a close later).
    let mut resting = leg(Role::Limit, &tickets[0][0].intent, true, "1", "0", true);
    resting.engine_price = Some(98.5);
    let still = Intended { limits: vec![Limit { signal_ts: None, decided_at: None, ..limit }], ..Default::default() };
    let act = Actual { legs: vec![resting.clone()], ..Default::default() };
    assert!(plan(SESSION, T, &still, &act, &rules()).actions.is_empty());
    // Dropped by the engine: cancelled.
    let p = plan(SESSION, T, &Intended::default(), &act, &rules());
    assert_eq!(cancelled(&p), vec![resting.client_id.as_str()]);
}

// ── Exits ────────────────────────────────────────────────────────────────────

/// A trailing stop is the engine moving its stop at each close: the working leg is modified,
/// the target left where it is.
#[test]
fn a_trailing_stop_is_a_modify_of_the_stop_leg() {
    let eng = Intended { hold: Some(Hold { long: true, stop: Some(97.0), target: Some(110.0) }), ..Default::default() };
    let act = long_position("95", "110");
    let p = plan(SESSION, T, &eng, &act, &rules());
    assert_eq!(p.actions, vec![Action::Modify { client_id: act.legs[1].client_id.clone(), limit: None, stop: Some(d("97")), engine_price: 97.0 }]);
    // On the engine's level already: nothing.
    let eng = Intended { hold: Some(Hold { long: true, stop: Some(95.0), target: Some(110.0) }), ..Default::default() };
    assert!(plan(SESSION, T, &eng, &act, &rules()).actions.is_empty());
}

/// A position whose exits are not at the broker (after a restart, a cancel by hand) gets them
/// back, standalone and linked; a broker that refused them leaves them to the live watch.
#[test]
fn missing_exits_are_placed_again_and_refused_ones_are_watched() {
    let eng = Intended { hold: Some(Hold { long: true, stop: Some(95.0), target: Some(110.0) }), ..Default::default() };
    let mut act = long_position("95", "110");
    act.legs.truncate(1);
    let p = plan(SESSION, T, &eng, &act, &rules());
    let tickets = placed(&p);
    assert_eq!(tickets.len(), 1);
    assert!(tickets[0].iter().all(|l| l.parent.is_none() && l.group.is_some() && l.reduce_only && l.qty == d("0.123")), "{:?}", tickets[0]);
    // Refused by the broker: not asked again every cycle.
    let mut refused = act.clone();
    refused.legs.extend(tickets[0].iter().map(|l| Leg { client_id: l.client_id.clone(), intent: l.intent.clone(), role: l.role, buy: l.buy, qty: l.qty, limit: l.limit, stop: l.stop, engine_price: l.engine_price, position: l.position.clone(), working: false, filled: Decimal::ZERO, done: true, refused: true }));
    let p = plan(SESSION, T, &eng, &refused, &rules());
    assert!(p.actions.is_empty());
    assert!(p.notes[0].contains("watched on the live price"), "{:?}", p.notes);
}

/// Watched exits: nothing rests; the live watch makes the engine flat when a level is crossed
/// and the sync answers with a market close, reduce-only, for what the session holds.
#[test]
fn a_watched_level_crossed_closes_at_the_market() {
    let r = Rules { native: false, ..rules() };
    let mut act = long_position("95", "110");
    act.legs.truncate(1);
    let holding = Intended { hold: Some(Hold { long: true, stop: Some(95.0), target: None }), ..Default::default() };
    assert!(plan(SESSION, T, &holding, &act, &r).actions.is_empty());
    let p = plan(SESSION, T, &Intended::default(), &act, &r);
    let tickets = placed(&p);
    assert_eq!(tickets.len(), 1);
    let close = &tickets[0][0];
    assert_eq!((close.role, close.kind, close.buy, close.qty, close.reduce_only), (Role::Close, OrderType::Market, false, d("0.123"), true));
    assert_eq!(close.position, act.legs[0].client_id);
}

/// The engine closed on a signal while native exits rest: they are cancelled and the position
/// closed. A stop that filled at the broker leaves nothing to do but tidy the other leg.
#[test]
fn a_signal_exit_cancels_the_exits_and_closes() {
    let act = long_position("95", "110");
    let p = plan(SESSION, T, &Intended::default(), &act, &rules());
    assert_eq!(cancelled(&p).len(), 2);
    assert_eq!(placed(&p)[0][0].role, Role::Close);
    assert!(matches!(p.actions.last(), Some(Action::Place(_))), "cancels first, then the close");

    // The stop filled at the broker: flat, the target left over is tidied.
    let mut stopped = long_position("95", "110");
    stopped.held = Some(Decimal::ZERO);
    (stopped.legs[1].working, stopped.legs[1].done, stopped.legs[1].filled) = (false, true, d("0.123"));
    let eng = Intended { hold: Some(Hold { long: true, stop: Some(95.0), target: Some(110.0) }), ..Default::default() };
    let p = plan(SESSION, T, &eng, &stopped, &rules());
    assert_eq!(p.own, Decimal::ZERO);
    assert_eq!(p.degraded, None);
    assert_eq!(cancelled(&p), vec![stopped.legs[2].client_id.as_str()]);
    assert!(placed(&p).is_empty(), "the engine's replay catches up; nothing is re-entered");
}

/// Stop and reverse: the long goes at the market and the short the close decided leaves too.
#[test]
fn a_reversal_closes_then_enters_the_other_side() {
    let eng = Intended { hold: Some(Hold { long: false, stop: None, target: None }), entry: Some(entry(false, "2026-10-28T10:20:00Z")), limits: vec![] };
    let act = long_position("95", "110");
    let p = plan(SESSION, T, &eng, &act, &Rules { native: false, ..rules() });
    let tickets = placed(&p);
    assert_eq!(tickets.iter().map(|t| t[0].role).collect::<Vec<_>>(), vec![Role::Close, Role::Entry]);
    assert!(!tickets[1][0].buy);
}

// ── Degraded, halted, blocked ────────────────────────────────────────────────

/// The account holds more than the session's fills explain (an order placed elsewhere): the
/// session is degraded, adds nothing, and keeps managing its exits.
#[test]
fn a_position_the_strategy_did_not_take_degrades_the_session() {
    let mut act = long_position("95", "110");
    act.held = Some(d("0.5"));
    let eng = Intended { hold: Some(Hold { long: true, stop: Some(97.0), target: Some(110.0) }), entry: None, limits: vec![] };
    let p = plan(SESSION, T, &eng, &act, &rules());
    assert_eq!(p.drift, Some(d("0.377")));
    assert!(p.degraded.as_deref().unwrap().contains("0.377"), "{:?}", p.degraded);
    assert!(matches!(p.actions[..], [Action::Modify { .. }]), "exits are still managed: {:?}", p.actions);

    // Flat and degraded: the entry the close decided is not sent, and the note says why.
    let flat = Actual { held: Some(d("0.2")), ..Default::default() };
    let p = plan(SESSION, T, &Intended { entry: Some(entry(true, "2026-10-28T10:05:00Z")), ..Default::default() }, &flat, &rules());
    assert!(placed(&p).is_empty());
    assert!(p.notes[0].contains("was not sent"), "{:?}", p.notes);
    // What the account held when it was armed is not a drift (a spot account's own asset).
    let spot = Actual { held: Some(d("0.2")), baseline: d("0.2"), ..Default::default() };
    assert_eq!(plan(SESSION, T, &Intended::default(), &spot, &rules()).degraded, None);
}

/// A partial fill left over (the rest cancelled or expired) degrades until the user adopts it.
#[test]
fn a_partial_fill_left_over_degrades_until_adopted() {
    let e = leg(Role::Entry, "entry|BTCUSDT|long|2026-10-28T10:05:00Z", true, "1", "0.4", false);
    let mut act = Actual { held: Some(d("0.4")), legs: vec![e.clone()], ..Default::default() };
    let eng = Intended { hold: Some(Hold { long: true, stop: None, target: None }), ..Default::default() };
    let p = plan(SESSION, T, &eng, &act, &rules());
    assert!(p.degraded.as_deref().unwrap().contains("filled 0.4 of 1"), "{:?}", p.degraded);
    act.adopted.push(e.client_id);
    assert_eq!(plan(SESSION, T, &eng, &act, &rules()).degraded, None);
}

/// A halt on the account (kill switch, loss limit): no entry leaves, a close still does.
#[test]
fn a_halt_pauses_entries_and_lets_a_close_out() {
    let r = Rules { halted: Some("trading is halted on the account (the kill switch is on)".into()), ..rules() };
    let p = plan(SESSION, T, &Intended { entry: Some(entry(true, "2026-10-28T10:05:00Z")), ..Default::default() }, &Actual::default(), &r);
    assert!(placed(&p).is_empty());
    assert!(p.notes[0].contains("kill switch"), "{:?}", p.notes);
    let p = plan(SESSION, T, &Intended::default(), &long_position("95", "110"), &r);
    assert_eq!(placed(&p)[0][0].role, Role::Close);
}

/// An order with no known outcome on the instrument: nothing at all is sent on it.
#[test]
fn an_order_with_no_answer_blocks_the_instrument() {
    let r = Rules { blocked: true, ..rules() };
    let p = plan(SESSION, T, &Intended::default(), &long_position("95", "110"), &r);
    assert!(p.actions.is_empty());
    assert!(p.notes[0].contains("Check it at the broker"));
}

/// Disarmed or paused: nothing is sent, and the drift is still measured and shown.
#[test]
fn a_disarmed_or_paused_session_sends_nothing_but_still_reads_the_account() {
    let mut act = long_position("95", "110");
    act.held = Some(d("1"));
    for r in [Rules { armed_at: None, ..rules() }, Rules { active: false, ..rules() }] {
        let p = plan(SESSION, T, &Intended::default(), &act, &r);
        assert!(p.actions.is_empty());
        assert!(p.degraded.is_some());
    }
}

// ── Identity, sizes, prices ─────────────────────────────────────────────────

/// One decision, one client id, inside every venue's rules (Binance: 1 to 36 of
/// `.A-Z:/a-z0-9_-`); two decisions never share one.
#[test]
fn a_client_id_is_the_decision_and_fits_every_venue() {
    let a = client_id(SESSION, "entry|BTCUSDT|long|2026-10-28T10:05:00Z", Role::Entry);
    assert_eq!(a, client_id(SESSION, "entry|BTCUSDT|long|2026-10-28T10:05:00Z", Role::Entry));
    assert_ne!(a, client_id(SESSION, "entry|BTCUSDT|long|2026-10-28T10:06:00Z", Role::Entry));
    assert_ne!(a, client_id(Uuid::from_u128(1), "entry|BTCUSDT|long|2026-10-28T10:05:00Z", Role::Entry));
    assert!(a.len() <= 36 && a.chars().all(|c| c.is_ascii_alphanumeric()), "{a}");
    assert!(format!("{a}g").len() <= 36, "the group id derived from it fits too");
}

#[test]
fn sizes_are_cut_down_and_levels_put_on_the_tick() {
    assert_eq!(qty_of(0.12345, Some(d("0.001"))), Some(d("0.123")));
    assert_eq!(qty_of(-2.9, Some(d("1"))), Some(d("2")));
    assert_eq!(qty_of(0.0004, Some(d("0.001"))), None);
    assert_eq!(qty_of(0.1, None), Some(d("0.1")), "no step stated: the engine's decimal, never more");
    assert_eq!(px_of(101.237, Some(d("0.25"))), Some(d("101.25")));
    assert_eq!(px_of(101.237, None), Some(d("101.237")));
    assert_eq!(px_of(f64::NAN, None), None);
    assert_eq!(px_of(0.0, None), None);
}

// ── Matching broker fills ────────────────────────────────────────────────────

fn sent(client_id: &str, venue_id: Option<&str>, fills_as: Option<&str>) -> exec_store::Order {
    exec_store::Order {
        session_id: SESSION,
        account_id: Uuid::nil(),
        client_id: client_id.into(),
        intent: client_id.into(),
        role: "entry".into(),
        ticker: T.into(),
        symbol: T.into(),
        side: "buy".into(),
        qty: d("0.001"),
        limit_price: None,
        stop_price: None,
        engine_qty: None,
        engine_price: None,
        position: String::new(),
        outcome: "sent".into(),
        reason: None,
        venue_id: venue_id.map(Into::into),
        fills_as: fills_as.map(Into::into),
        status: Some("filled".into()),
        filled_qty: d("0.001"),
        created_at: at("2026-10-28T10:00:00Z"),
        updated_at: at("2026-10-28T10:00:00Z"),
    }
}

#[test]
fn a_binance_fill_is_matched_by_the_number_the_lookup_read() {
    // Binance follows an order by `SYMBOL:client id`; its trade history names its own number.
    let rows = [sent("k1", Some("BTCUSDT:k1"), None), sent("k2", Some("BTCUSDT:k2"), Some("4419087"))];
    assert_eq!(order_of(&rows, "4419087").map(|o| o.client_id.as_str()), Some("k2"));
    assert!(order_of(&rows, "28").is_none(), "a number no lookup read matches no order");
    // A venue whose history names the id the order is followed by still matches on it.
    let ibkr = [sent("k3", Some("42"), None)];
    assert_eq!(order_of(&ibkr, "42").map(|o| o.client_id.as_str()), Some("k3"));
}

// ── Reading the engine ───────────────────────────────────────────────────────

fn live_fill(kind: &str, key: &str, direction: &str, reason: &str, t: &str) -> store::LiveFill {
    store::LiveFill {
        id: Uuid::nil(),
        session_id: Uuid::nil(),
        trade_key: key.into(),
        ticker: T.into(),
        kind: kind.into(),
        direction: direction.into(),
        reason: reason.into(),
        bar_ts: String::new(),
        price: 100.0,
        maker: false,
        quoted: false,
        at: at(t),
    }
}

/// The tick's stats read as the engine's book: the open position with its levels, the market
/// entry its last close decided (a position from then on), the resting limits; a position the
/// live watch already closed is not held.
#[test]
fn the_engine_book_is_read_from_the_last_tick() {
    let settings: Settings = serde_json::from_value(json!({
        "mode": "both",
        "long": { "stop_loss_pct": 0.05, "take_profit_pct": 0.1 },
        "short": { "stop_loss_pct": 0.05, "take_profit_pct": 0.1 },
        "sizing": { "mode": "fixed_qty", "qty": 1 }
    }))
    .unwrap();
    let stats = json!({
        "open_positions": [{ "ticker": T, "direction": "long", "entry_ts": "2026-10-28T10:00:00Z", "avg_price": 100.0,
            "qty": 1.0, "fees": 0.0, "stop": 96.0, "stop_reason": "trailing_stop", "take_profit": 110.0 }],
        "pending_orders": [
            { "ticker": T, "kind": "entry", "direction": "short", "order": "limit", "price": 120.0, "bars_left": 3,
              "signal_ts": "2026-10-28T10:05:00Z", "qty": 1.0 },
            { "ticker": T, "kind": "entry", "direction": "long", "order": "limit", "price": 90.0, "bars_left": 2, "provisional": true }
        ]
    });
    let eng = intended(Some(&stats), T, &[], &settings, 60);
    assert_eq!(eng.hold, Some(Hold { long: true, stop: Some(96.0), target: Some(110.0) }));
    assert_eq!(eng.entry, None);
    assert_eq!(eng.limits.len(), 1, "a provisional limit has no price to send yet");
    assert_eq!(eng.limits[0].decided_at, Some(at("2026-10-28T10:06:00Z")));

    // The live watch closed that long: the engine holds nothing there any more.
    let exited = [live_fill("exit", "BTCUSDT|long|2026-10-28T10:00:00Z", "long", "stop_loss", "2026-10-28T10:07:30Z")];
    assert_eq!(intended(Some(&stats), T, &exited, &settings, 60).hold, None);

    // A market entry decided at the close: held from then, on the levels of its price.
    let stats = json!({ "pending_orders": [{ "ticker": T, "kind": "entry", "direction": "long", "order": "market",
        "price": 100.0, "bars_left": 1, "signal_ts": "2026-10-28T10:05:00Z", "qty": 0.5 }] });
    let eng = intended(Some(&stats), T, &[], &settings, 60);
    let e = eng.entry.clone().unwrap();
    assert_eq!((e.long, e.qty, e.decided_at), (true, 0.5, at("2026-10-28T10:06:00Z")));
    let h = eng.hold.unwrap();
    assert!((h.stop.unwrap() - 95.0).abs() < 1e-9 && (h.target.unwrap() - 110.0).abs() < 1e-9, "{h:?}");
    // Filled live then exited live before the next tick: nothing held.
    let live = [
        live_fill("entry", "BTCUSDT|long|live:x", "long", "market", "2026-10-28T10:06:01Z"),
        live_fill("exit", "BTCUSDT|long|live:x", "long", "stop_loss", "2026-10-28T10:06:40Z"),
    ];
    let eng = intended(Some(&stats), T, &live, &settings, 60);
    assert_eq!((eng.hold, eng.entry), (None, None));
}

/// A simulated session is never a broker one: no target, the sync answers at once.
#[test]
fn a_simulated_session_has_no_broker_target() {
    let input: store::SessionInput = serde_json::from_value(json!({ "name": "sim", "settings": {}, "kind": "interval", "timezone": "UTC" })).unwrap();
    let mut s = crate::paper_api::probe_session(&input);
    assert!(Execution::of(&s).is_none() && !s.on_broker());
    s.execution = Some(json!({ "target": "simulate" }));
    assert!(Execution::of(&s).is_none() && !s.on_broker());
    s.execution = Some(json!({ "target": "broker", "account_id": Uuid::nil() }));
    assert!(Execution::of(&s).is_some() && s.on_broker());
}

// ── On a mock venue, over a scratch database ────────────────────────────────

mod venue {
    use super::*;
    use std::sync::Mutex as StdMutex;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

    async fn scratch_state() -> Option<crate::AppState> {
        let url = std::env::var("OTW_TEST_DATABASE_URL").ok()?;
        let pool = otw_store::connect_and_migrate(&url).await.unwrap();
        let cipher = otw_store::crypto::SecretCipher::from_master("paper-exec-tests").unwrap();
        let http = reqwest::Client::new();
        let live = crate::live::hub::LiveHub::new(pool.clone(), http.clone(), cipher.clone());
        Some(crate::AppState {
            pool: pool.clone(),
            upload_dir: std::env::temp_dir(),
            cipher: cipher.clone(),
            scheduler: otw_scheduler::Scheduler::new(pool.clone(), cipher.clone()),
            findb_importing: Default::default(),
            mportfolios_refresh: crate::mportfolios_job::new_lock(),
            watchlists_refresh: crate::watchlists::new_lock(),
            http: http.clone(),
            live: live.clone(),
            live_alerts: crate::histviz_live_alerts::spawn(pool.clone(), live, crate::histviz_alerts::AlertCx { cipher: cipher.clone(), http }),
            exchanges: crate::exchanges::load(&pool).await,
            submit_limiter: crate::community_docs_api::SubmitLimiter::new(),
            optimizer: std::sync::Arc::new(crate::backtest::optimize::Jobs::new()),
        })
    }

    /// A Binance spot testnet that takes orders and answers lookups by client id: every order
    /// it was sent is `NEW` until `fill` marks it filled, and the account holds `btc` BTC.
    #[derive(Clone, Default)]
    struct Venue {
        sent: std::sync::Arc<StdMutex<Vec<String>>>,
        filled: std::sync::Arc<StdMutex<Vec<String>>>,
        btc: std::sync::Arc<StdMutex<String>>,
        /// The gateway in front of the venue falls over: no answer to a place, nor to a lookup.
        lost: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }

    impl Venue {
        fn fill(&self, key: &str, btc: &str) {
            self.filled.lock().unwrap().push(key.into());
            *self.btc.lock().unwrap() = btc.into();
        }
        fn posts(&self) -> usize {
            self.sent.lock().unwrap().len()
        }
    }

    /// A parameter of a signed call, in its query or its form body.
    fn param(req: &Request, key: &str) -> Option<String> {
        let body = reqwest::Url::parse(&format!("http://body/?{}", String::from_utf8_lossy(&req.body))).ok()?;
        req.url.query_pairs().chain(body.query_pairs()).find(|(k, _)| k == key).map(|(_, v)| v.into_owned())
    }

    fn order(key: &str, status: &str, qty: &str) -> Value {
        json!({
            "symbol": "BTCUSDT", "orderId": 4242, "clientOrderId": key, "price": "0",
            "origQty": qty, "executedQty": if status == "FILLED" { qty } else { "0" }, "status": status, "timeInForce": "GTC",
            "type": "MARKET", "side": "BUY", "time": 1, "updateTime": 1, "transactTime": 1, "fills": []
        })
    }

    struct Place(Venue);
    impl Respond for Place {
        fn respond(&self, req: &Request) -> ResponseTemplate {
            let key = param(req, "newClientOrderId").unwrap_or_default();
            let qty = param(req, "quantity").unwrap_or_else(|| "0".into());
            self.0.sent.lock().unwrap().push(key.clone());
            if self.0.lost.load(std::sync::atomic::Ordering::SeqCst) {
                return ResponseTemplate::new(503).set_body_string("upstream timed out");
            }
            ResponseTemplate::new(200).set_body_json(order(&key, "NEW", &qty))
        }
    }

    struct Query(Venue);
    impl Respond for Query {
        fn respond(&self, req: &Request) -> ResponseTemplate {
            let key = param(req, "origClientOrderId").unwrap_or_default();
            if self.0.lost.load(std::sync::atomic::Ordering::SeqCst) {
                return ResponseTemplate::new(503).set_body_string("upstream timed out");
            }
            if !self.0.sent.lock().unwrap().contains(&key) {
                return ResponseTemplate::new(400).set_body_json(json!({ "code": -2013, "msg": "Order does not exist." }));
            }
            let status = if self.0.filled.lock().unwrap().contains(&key) { "FILLED" } else { "NEW" };
            ResponseTemplate::new(200).set_body_json(order(&key, status, "0.123"))
        }
    }

    struct Account(Venue);
    impl Respond for Account {
        fn respond(&self, _: &Request) -> ResponseTemplate {
            let btc = self.0.btc.lock().unwrap().clone();
            ResponseTemplate::new(200).set_body_json(json!({ "balances": [
                { "asset": "BTC", "free": btc, "locked": "0" }, { "asset": "USDT", "free": "100000", "locked": "0" }
            ] }))
        }
    }

    async fn mock_venue(server: &MockServer, v: &Venue) {
        *v.btc.lock().unwrap() = "0".into();
        Mock::given(method("GET")).and(path("/api/v3/time")).respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "serverTime": (OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as i64
        }))).mount(server).await;
        Mock::given(method("GET")).and(path("/api/v3/exchangeInfo")).respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "timezone": "UTC",
            "symbols": [{
                "symbol": "BTCUSDT", "status": "TRADING", "baseAsset": "BTC", "quoteAsset": "USDT",
                "orderTypes": ["LIMIT", "MARKET", "STOP_LOSS", "STOP_LOSS_LIMIT", "TAKE_PROFIT", "TAKE_PROFIT_LIMIT", "LIMIT_MAKER"],
                "filters": [
                    { "filterType": "PRICE_FILTER", "minPrice": "0.01", "maxPrice": "1000000", "tickSize": "0.01" },
                    { "filterType": "LOT_SIZE", "minQty": "0.00001", "maxQty": "9000", "stepSize": "0.00001" }
                ]
            }]
        }))).mount(server).await;
        Mock::given(method("GET")).and(path("/api/v3/rateLimit/order")).respond_with(ResponseTemplate::new(200).set_body_json(json!([]))).mount(server).await;
        Mock::given(method("GET")).and(path("/api/v3/myTrades")).respond_with(ResponseTemplate::new(200).set_body_json(json!([]))).mount(server).await;
        Mock::given(method("GET")).and(path("/api/v3/account")).respond_with(Account(v.clone())).mount(server).await;
        Mock::given(method("POST")).and(path("/api/v3/order")).respond_with(Place(v.clone())).mount(server).await;
        Mock::given(method("GET")).and(path("/api/v3/order")).respond_with(Query(v.clone())).mount(server).await;
    }

    /// A testnet spot account trading, granted to every module, background trading signed.
    async fn account(state: &crate::AppState, tag: &str) -> otw_store::brokers::BrokerRow {
        let run = Uuid::new_v4();
        let modules = vec!["*".to_string(), "trading".to_string()];
        let row = otw_store::brokers::create(&state.pool, "binance", &format!("exec {tag} {run}"), &modules, &json!({ "env": "testnet" })).await.unwrap();
        otw_store::brokers::set_cred(&state.pool, &state.cipher, row.id, "binance", "api_key", &format!("exec-key-{tag}-{run}")).await.unwrap();
        otw_store::brokers::set_cred(&state.pool, &state.cipher, row.id, "binance", "api_secret", "mock-secret").await.unwrap();
        otw_store::trading_ack::sign(
            &state.pool,
            &otw_store::trading_ack::NewAck {
                user_id: Uuid::nil(),
                account_id: row.id,
                account_name: &row.name,
                broker: &row.broker,
                environment: "sandbox",
                terms_version: "Last updated: 28 October 2026",
                terms: &json!([]),
                statements: &json!([{ "key": "free", "text": "x", "accepted": true }]),
                locale: "en",
                typed: &row.name,
                second_factor: false,
            },
        )
        .await
        .unwrap();
        otw_store::brokers::get(&state.pool, row.id).await.unwrap().unwrap()
    }

    fn settings() -> Value {
        json!({ "mode": "long", "long": {}, "sizing": { "mode": "fixed_qty", "qty": 0.123 } })
    }

    /// A broker session on BTCUSDT 1m, created through the same checks as the route.
    async fn session(state: &crate::AppState, account: &otw_store::brokers::BrokerRow, name: &str) -> Result<store::Session, crate::ApiError> {
        let dataset = otw_store::histdata::upsert_dataset(&state.pool, "binance", "crypto", "BTCUSDT", "1m").await.unwrap();
        let bars: Vec<otw_store::histdata::Bar> = (0..3)
            .map(|i| otw_store::histdata::Bar { ts: OffsetDateTime::now_utc() - time::Duration::minutes(3 - i), open: 100.0, high: 101.0, low: 99.0, close: 100.0, volume: 1.0, adj_open: None, adj_high: None, adj_low: None, adj_close: None })
            .collect();
        otw_store::histdata::write_bars(&state.pool, dataset, &bars).await.unwrap();
        let s: Settings = serde_json::from_value(settings()).unwrap();
        let raw = json!({ "target": "broker", "account_id": account.id, "exits": "watch" });
        let (ex, symbols) = vet(state, Some(&raw), &s, &[dataset]).await?.unwrap();
        let input: store::SessionInput = serde_json::from_value(json!({
            "name": name, "settings": settings(), "dataset_ids": [dataset], "kind": "interval", "timezone": "UTC",
            "every_minutes": 1, "status": "active", "feed": "hist", "notify_when": "on_change", "cadence": "each"
        }))
        .unwrap();
        let created = store::create_session(&state.pool, &input, None).await.unwrap();
        let target = serde_json::to_value(&ex).unwrap();
        if let Some(symbol) = exec_store::attach(&state.pool, created.id, account.id, &target, &symbols).await.unwrap() {
            return Err(crate::ApiError::bad_request(&format!("taken: {symbol}")));
        }
        Ok(store::get_session(&state.pool, created.id).await.unwrap().unwrap())
    }

    /// What the last tick left: a long market entry decided at the close that just happened.
    async fn decide_entry(state: &crate::AppState, id: Uuid) {
        // Two minutes ago: before an arming now, after one five minutes ago.
        let signal = OffsetDateTime::now_utc() - time::Duration::minutes(3);
        let signal = (signal - time::Duration::seconds(signal.second() as i64)).replace_nanosecond(0).unwrap();
        let ts = signal.format(&time::format_description::well_known::Rfc3339).unwrap();
        let stats = json!({ "pending_orders": [{ "ticker": "BTCUSDT", "kind": "entry", "direction": "long", "order": "market",
            "price": 100.0, "bars_left": 1, "signal_ts": ts, "qty": 0.123 }] });
        store::finish_tick(&state.pool, id, None, Some(&stats), 3, "h", 1, "").await.unwrap();
    }

    /// Paper session on a broker account end to end: the entry the close decided goes out once
    /// at market under the strategy's name, a second sync and a restart send nothing again,
    /// the fill makes the session's position, an external change degrades it, and the account
    /// cannot take a second session on the same instrument.
    #[tokio::test]
    async fn a_strategy_executes_once_on_the_account_and_rebuilds_from_it() {
        let Some(state) = scratch_state().await else { return };
        let server = MockServer::start().await;
        let v = Venue::default();
        mock_venue(&server, &v).await;
        crate::brokers::serve_from(&server.uri());
        let acct = account(&state, "e2e").await;
        let s = session(&state, &acct, "e2e").await.unwrap_or_else(|e| panic!("{}", e.message));
        assert!(s.on_broker());
        assert_eq!(Execution::of(&s).unwrap().symbols.get("binance:BTCUSDT").map(String::as_str), Some("BTCUSDT"));

        // Two strategies on one position: refused at creation.
        let e = session(&state, &acct, "second").await.unwrap_err();
        assert!(e.message.contains("one strategy per instrument and account"), "{}", e.message);

        // Not armed: the decision stays with the engine.
        decide_entry(&state, s.id).await;
        sync(&state, s.id).await.unwrap();
        assert_eq!(v.posts(), 0, "a disarmed session sends nothing");

        let baseline = baseline(&state, &store::get_session(&state.pool, s.id).await.unwrap().unwrap()).await.unwrap();
        // Armed after that close: still not sent (decided before arming). Re-decide after.
        exec_store::arm(&state.pool, s.id, Uuid::nil(), &baseline).await.unwrap();
        sync(&state, s.id).await.unwrap();
        assert_eq!(v.posts(), 0, "an entry decided before arming is not sent");
        let armed_at = OffsetDateTime::now_utc() - time::Duration::minutes(5);
        sqlx::query("UPDATE paper_sessions SET armed_at = $2 WHERE id = $1").bind(s.id).bind(armed_at).execute(&state.pool).await.unwrap();
        sync(&state, s.id).await.unwrap();
        assert_eq!(v.posts(), 1, "the entry leaves once");
        let rows = exec_store::orders(&state.pool, s.id).await.unwrap();
        assert_eq!((rows[0].role.as_str(), rows[0].outcome.as_str(), rows[0].qty), ("entry", "sent", d("0.123")));
        let log = otw_store::order_log::list(&state.pool, Some(acct.id), 10).await.unwrap();
        assert_eq!(log.iter().find(|e| e.action == "place").and_then(|e| e.source.clone()), Some(format!("strategy:{}", s.id)));

        // The same close read again, and a restart (nothing in memory): nothing resent.
        sync(&state, s.id).await.unwrap();
        sync(&state, s.id).await.unwrap();
        assert_eq!(v.posts(), 1, "never sent twice");

        // Filled at the venue: the session's position is read from it, and agrees.
        v.fill(&rows[0].client_id, "0.123");
        sync(&state, s.id).await.unwrap();
        let s2 = store::get_session(&state.pool, s.id).await.unwrap().unwrap();
        assert_eq!(s2.exec_state, "synced", "{}", s2.exec_note);
        let rows = exec_store::orders(&state.pool, s.id).await.unwrap();
        assert_eq!((rows[0].status.as_deref(), rows[0].filled_qty), (Some("filled"), d("0.123")));

        // Something bought outside the strategy: degraded, and said once.
        *v.btc.lock().unwrap() = "0.5".into();
        sync(&state, s.id).await.unwrap();
        let s3 = store::get_session(&state.pool, s.id).await.unwrap().unwrap();
        assert_eq!(s3.exec_state, "degraded");
        assert!(s3.exec_note.contains("0.377"), "{}", s3.exec_note);
        sync(&state, s.id).await.unwrap();
        let notices = store::list_events(&state.pool, Some(s.id), false, 50).await.unwrap().into_iter().filter(|e| e.title.contains("degraded")).count();
        assert_eq!(notices, 1, "one notice per distinct problem");

        // Disarming the account (background trading off) disarms the session.
        let d = crate::brokers::dispatch::disarm_account(&state, acct.id).await.unwrap();
        assert_eq!(d.strategies, 1);
        assert!(store::get_session(&state.pool, s.id).await.unwrap().unwrap().armed_at.is_none());
    }

    /// A kill switch on the account: the session pauses its entries and says why.
    #[tokio::test]
    async fn a_kill_switch_pauses_the_strategy() {
        let Some(state) = scratch_state().await else { return };
        let server = MockServer::start().await;
        let v = Venue::default();
        mock_venue(&server, &v).await;
        crate::brokers::serve_from(&server.uri());
        let acct = account(&state, "halt").await;
        let s = session(&state, &acct, "halt").await.unwrap_or_else(|e| panic!("{}", e.message));
        exec_store::arm(&state.pool, s.id, Uuid::nil(), &json!({ "BTCUSDT": "0" })).await.unwrap();
        sqlx::query("UPDATE paper_sessions SET armed_at = now() - interval '5 minutes' WHERE id = $1").bind(s.id).execute(&state.pool).await.unwrap();
        otw_store::broker_risk::add_halt(&state.pool, Some(acct.id), "kill_switch", "", None).await.unwrap();
        decide_entry(&state, s.id).await;
        sync(&state, s.id).await.unwrap();
        assert_eq!(v.posts(), 0);
        let s = store::get_session(&state.pool, s.id).await.unwrap().unwrap();
        assert_eq!(s.exec_state, "paused");
        assert!(s.exec_note.contains("kill switch"), "{}", s.exec_note);
    }

    /// The entry's answer is lost and the lookup fails too: the session says so, blocks the
    /// instrument, and sends nothing more there, whatever the next syncs read.
    #[tokio::test]
    async fn an_entry_whose_answer_was_lost_is_never_sent_again() {
        let Some(state) = scratch_state().await else { return };
        let server = MockServer::start().await;
        let v = Venue::default();
        mock_venue(&server, &v).await;
        v.lost.store(true, std::sync::atomic::Ordering::SeqCst);
        crate::brokers::serve_from(&server.uri());
        let acct = account(&state, "lost").await;
        let s = session(&state, &acct, "lost").await.unwrap_or_else(|e| panic!("{}", e.message));
        exec_store::arm(&state.pool, s.id, Uuid::nil(), &json!({ "BTCUSDT": "0" })).await.unwrap();
        sqlx::query("UPDATE paper_sessions SET armed_at = now() - interval '5 minutes' WHERE id = $1").bind(s.id).execute(&state.pool).await.unwrap();
        decide_entry(&state, s.id).await;
        sync(&state, s.id).await.unwrap();
        assert_eq!(v.posts(), 1);
        let s1 = store::get_session(&state.pool, s.id).await.unwrap().unwrap();
        assert!(s1.exec_note.contains("Check it at the broker"), "{}", s1.exec_note);
        let rows = exec_store::orders(&state.pool, s.id).await.unwrap();
        assert_eq!(rows[0].outcome, "unknown");
        // The venue answers again; the leg is still unknown until reconciliation settles it,
        // and nothing is sent meanwhile.
        v.lost.store(false, std::sync::atomic::Ordering::SeqCst);
        sync(&state, s.id).await.unwrap();
        sync(&state, s.id).await.unwrap();
        assert_eq!(v.posts(), 1, "an order whose answer was lost is never resent");
    }

    /// A paper broker account in USDT, trading, granted to every module, background trading
    /// signed; BTCUSDT's currency stated, and a Binance data connector to price it on.
    async fn paper_account(state: &crate::AppState) -> otw_store::brokers::BrokerRow {
        let run = Uuid::new_v4();
        let modules = vec!["*".to_string(), "trading".to_string()];
        let row = otw_store::brokers::create(&state.pool, "paper", &format!("paper {run}"), &modules, &json!({ "currency": "USDT", "start_balance": "100000" })).await.unwrap();
        crate::brokers::paper::open(&state.pool, &row).await.unwrap();
        otw_store::trading_ack::sign(
            &state.pool,
            &otw_store::trading_ack::NewAck {
                user_id: Uuid::nil(),
                account_id: row.id,
                account_name: &row.name,
                broker: &row.broker,
                environment: "paper",
                terms_version: "Last updated: 28 October 2026",
                terms: &json!([]),
                statements: &json!([{ "key": "free", "text": "x", "accepted": true }]),
                locale: "en",
                typed: &row.name,
                second_factor: false,
            },
        )
        .await
        .unwrap();
        let meta: otw_store::instrument_meta::InstrumentMeta = serde_json::from_value(json!({ "provider": "binance", "asset_type": "crypto", "ticker": "BTCUSDT", "currency": "USDT" })).unwrap();
        otw_store::instrument_meta::put(&state.pool, &meta).await.unwrap();
        if otw_store::connectors::default_for(&state.pool, "binance").await.unwrap().is_none() {
            otw_store::connectors::create(&state.pool, "binance", &format!("binance {run}"), &["*".to_string()], &json!({})).await.unwrap();
        }
        otw_store::brokers::get(&state.pool, row.id).await.unwrap().unwrap()
    }

    /// The paper broker account, end to end: the entry leaves with its stop and target attached
    /// (native exits), its fill at the broker is the session's position, a trailing stop moves
    /// the working stop, the engine going flat cancels the exits and closes at the market, and
    /// Stop and flatten leaves nothing working. Fills are the paper broker's own, written as its
    /// engine writes them on the live price.
    #[tokio::test]
    async fn a_strategy_trades_the_paper_broker_account() {
        let Some(state) = scratch_state().await else { return };
        let acct = paper_account(&state).await;
        let dataset = otw_store::histdata::upsert_dataset(&state.pool, "binance", "crypto", "BTCUSDT", "1m").await.unwrap();
        let bars: Vec<otw_store::histdata::Bar> = (0..3)
            .map(|i| otw_store::histdata::Bar { ts: OffsetDateTime::now_utc() - time::Duration::minutes(3 - i), open: 100.0, high: 101.0, low: 99.0, close: 100.0, volume: 1.0, adj_open: None, adj_high: None, adj_low: None, adj_close: None })
            .collect();
        otw_store::histdata::write_bars(&state.pool, dataset, &bars).await.unwrap();
        let strategy = json!({ "mode": "long", "long": { "stop_loss_pct": 0.05, "take_profit_pct": 0.1 }, "sizing": { "mode": "fixed_qty", "qty": 0.123 } });
        let s: Settings = serde_json::from_value(strategy.clone()).unwrap();
        // Native exits by default: the paper broker attaches and moves them.
        let raw = json!({ "target": "broker", "account_id": acct.id });
        let (ex, symbols) = vet(&state, Some(&raw), &s, &[dataset]).await.unwrap_or_else(|e| panic!("{}", e.message)).unwrap();
        assert_eq!(ex.exits, "native");
        let input: store::SessionInput = serde_json::from_value(json!({
            "name": "paper e2e", "settings": strategy, "dataset_ids": [dataset], "kind": "interval", "timezone": "UTC",
            "every_minutes": 1, "status": "active", "feed": "hist", "notify_when": "on_change", "cadence": "each"
        }))
        .unwrap();
        let created = store::create_session(&state.pool, &input, None).await.unwrap();
        assert_eq!(exec_store::attach(&state.pool, created.id, acct.id, &serde_json::to_value(&ex).unwrap(), &symbols).await.unwrap(), None);
        let id = created.id;
        let session = store::get_session(&state.pool, id).await.unwrap().unwrap();
        exec_store::arm(&state.pool, id, Uuid::nil(), &baseline(&state, &session).await.unwrap()).await.unwrap();
        sqlx::query("UPDATE paper_sessions SET armed_at = now() - interval '5 minutes' WHERE id = $1").bind(id).execute(&state.pool).await.unwrap();
        decide_entry(&state, id).await;

        sync(&state, id).await.unwrap();
        let rows = exec_store::orders(&state.pool, id).await.unwrap();
        assert_eq!(rows.iter().map(|o| (o.role.as_str(), o.outcome.as_str())).collect::<Vec<_>>(), vec![("entry", "sent"), ("stop", "sent"), ("target", "sent")], "{rows:?}");
        let working = otw_store::paper_broker::open_orders(&state.pool, acct.id).await.unwrap();
        assert_eq!(working.len(), 3, "the entry working, its exits held for it");
        let stop = working.iter().find(|o| o.order_type == "stop").unwrap();
        assert_eq!(stop.stop_price, Some(d("95")));

        // The paper broker fills the entry at 100.4: the session holds what filled.
        let entry = working.iter().find(|o| o.order_type == "market").unwrap();
        otw_store::paper_broker::fill(&state.pool, entry.id, d("100.4"), Decimal::ZERO, Decimal::ONE, OffsetDateTime::now_utc()).await.unwrap();
        // The engine's next close: the position with a stop that trailed up to 97.
        let stats = json!({ "open_positions": [{ "ticker": "BTCUSDT", "direction": "long", "entry_ts": "2026-10-28T10:00:00Z",
            "avg_price": 100.0, "qty": 0.123, "fees": 0.0, "stop": 97.0, "stop_reason": "trailing_stop", "take_profit": 110.0 }] });
        store::finish_tick(&state.pool, id, None, Some(&stats), 3, "h2", 1, "").await.unwrap();
        sync(&state, id).await.unwrap();
        let after = store::get_session(&state.pool, id).await.unwrap().unwrap();
        assert_eq!(after.exec_state, "synced", "{}", after.exec_note);
        let rows = exec_store::orders(&state.pool, id).await.unwrap();
        assert_eq!((rows[0].status.as_deref(), rows[0].filled_qty), (Some("filled"), d("0.123")));
        let fills = exec_store::fills(&state.pool, id).await.unwrap();
        assert_eq!(fills.iter().map(|f| f.price).collect::<Vec<_>>(), vec![d("100.4")], "the real price, beside the engine's");
        let stop = otw_store::paper_broker::open_orders(&state.pool, acct.id).await.unwrap().into_iter().find(|o| o.order_type == "stop").unwrap();
        assert_eq!(stop.stop_price, Some(d("97")), "the trailing stop moved at the broker");

        // The engine closed on a signal: exits cancelled, the position closed at the market.
        store::finish_tick(&state.pool, id, None, Some(&json!({ "trades": 1 })), 3, "h3", 1, "").await.unwrap();
        sync(&state, id).await.unwrap();
        let working = otw_store::paper_broker::open_orders(&state.pool, acct.id).await.unwrap();
        assert_eq!(working.len(), 1, "{working:?}");
        assert_eq!((working[0].order_type.as_str(), working[0].side.as_str(), working[0].qty, working[0].reduce_only), ("market", "sell", d("0.123"), true));
        sync(&state, id).await.unwrap();
        assert_eq!(otw_store::paper_broker::open_orders(&state.pool, acct.id).await.unwrap().len(), 1, "the close is not sent twice");

        // Stop and flatten: disarmed, the close still working cancelled, nothing left out. The
        // position (not filled yet) is closed again by the flatten, reduce-only.
        let session = store::get_session(&state.pool, id).await.unwrap().unwrap();
        let out = flatten(&state, &session).await.unwrap_or_else(|e| panic!("{}", e.message));
        assert_eq!(out["cancelled"], json!(1), "{out}");
        assert!(store::get_session(&state.pool, id).await.unwrap().unwrap().armed_at.is_none());
        let working = otw_store::paper_broker::open_orders(&state.pool, acct.id).await.unwrap();
        assert_eq!(working.len(), 1);
        assert_eq!((working[0].side.as_str(), working[0].qty), ("sell", d("0.123")));
        let log = otw_store::order_log::list(&state.pool, Some(acct.id), 20).await.unwrap();
        assert!(log.iter().filter(|e| e.action == "place").all(|e| e.source.as_deref() == Some(format!("strategy:{id}").as_str())));
    }
}
