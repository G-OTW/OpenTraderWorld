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
    Actual { held: Some(d("0.123")), legs: vec![e, s, t], ..Default::default() }
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

/// Refused, never resized: under the step, or over the session's maximum. Written refused
/// with the reason (the log says it, no later sync asks again), nothing placed.
#[test]
fn a_size_the_venue_or_the_session_cannot_take_is_refused_not_resized() {
    let refused = |p: &Plan| p.actions.iter().filter_map(|a| if let Action::Refuse { reason, .. } = a { Some(reason.clone()) } else { None }).collect::<Vec<_>>();
    let mut e = entry(true, "2026-10-28T10:05:00Z");
    e.qty = 0.0004;
    let p = plan(SESSION, T, &Intended { entry: Some(e), ..Default::default() }, &Actual::default(), &rules());
    assert!(placed(&p).is_empty());
    assert!(refused(&p)[0].contains("under the venue's step"), "{:?}", p.actions);
    let r = Rules { max_qty: Some(d("0.1")), ..rules() };
    let p = plan(SESSION, T, &Intended { entry: Some(entry(true, "2026-10-28T10:05:00Z")), ..Default::default() }, &Actual::default(), &r);
    assert!(placed(&p).is_empty());
    assert!(refused(&p)[0].contains("over this session's maximum"), "{:?}", p.actions);
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

    // At its maximum of one position: the long the close takes off frees the place, the short goes.
    let capped = Rules { native: false, capital: sizing::Capital::validate(&json!({ "max_positions": 1 })).unwrap(), open_positions: 1, ..rules() };
    let p = plan(SESSION, T, &eng, &act, &capped);
    assert_eq!(placed(&p).iter().map(|t| t[0].role).collect::<Vec<_>>(), vec![Role::Close, Role::Entry]);
    assert!(!p.actions.iter().any(|a| matches!(a, Action::Refuse { .. })), "{:?}", p.actions);

    // Two positions held on other instruments still count: the reverse is refused.
    let full = Rules { open_positions: 2, ..capped };
    let p = plan(SESSION, T, &eng, &act, &full);
    assert_eq!(placed(&p).iter().map(|t| t[0].role).collect::<Vec<_>>(), vec![Role::Close]);
    assert!(p.actions.iter().any(|a| matches!(a, Action::Refuse { reason, .. } if reason.contains("its maximum of 1"))), "{:?}", p.actions);
}

// ── Degraded, halted, blocked ────────────────────────────────────────────────

/// The user added to or took from the robot's position at the broker: the robot stops, sends
/// nothing, and must be armed again. Its position is not written closed: it still holds it.
#[test]
fn a_position_the_user_resized_stops_the_robot() {
    let mut act = long_position("95", "110");
    let eng = Intended { hold: Some(Hold { long: true, stop: Some(97.0), target: Some(110.0) }), entry: None, limits: vec![] };
    for held in ["0.5", "0.05"] {
        act.held = Some(d(held));
        let p = plan(SESSION, T, &eng, &act, &rules());
        let why = p.stop.as_deref().unwrap();
        assert!(why.contains("long 0.123 BTCUSDT position was changed at the broker") && why.contains(&format!("{held} held now")), "{why}");
        assert_eq!(p.closed_outside, None);
        assert!(p.actions.is_empty(), "{:?}", p.actions);
    }
    // What the account held when it was armed is not a drift (a spot account's own asset).
    let spot = Actual { held: Some(d("0.2")), baseline: d("0.2"), ..Default::default() };
    let p = plan(SESSION, T, &Intended::default(), &spot, &rules());
    assert_eq!(p.stop, None);
    // Nor is the robot's own fill the account already shows.
    let p = plan(SESSION, T, &eng, &long_position("95", "110"), &rules());
    assert_eq!((p.drift, p.stop), (Some(Decimal::ZERO), None));
}

/// A stop the user moved at the broker stays where they put it: the robot's trailing level no
/// longer moves it, and the target it did not touch is still the robot's. With the broker's
/// working orders unread, no exit is moved at all.
#[test]
fn an_exit_the_user_moved_is_left_where_it_is() {
    let eng = Intended { hold: Some(Hold { long: true, stop: Some(97.0), target: Some(112.0) }), ..Default::default() };
    let mut act = long_position("95", "110");
    act.moved.insert(act.legs[1].client_id.clone(), d("93"));
    let p = plan(SESSION, T, &eng, &act, &rules());
    assert_eq!(p.actions, vec![Action::Modify { client_id: act.legs[2].client_id.clone(), limit: Some(d("112")), stop: None, engine_price: 112.0 }]);
    assert!(p.notes.iter().any(|n| n.contains("stop of the BTCUSDT position was moved at the broker to 93")), "{:?}", p.notes);
    assert_eq!(p.stop, None, "a moved order is not an act on the position");

    let unread = Actual { orders_unread: true, ..long_position("95", "110") };
    assert!(plan(SESSION, T, &eng, &unread, &rules()).actions.is_empty());
}

/// The broker's working orders against the levels the robot sent: a stop or a target listed
/// at another price was moved there by the user; the same level, an unlisted order, or one
/// listed without that price is not.
#[test]
fn moved_exits_are_read_from_the_broker_orders() {
    let row = |client: &str, role: &str, limit: Option<&str>, stop: Option<&str>| exec_store::Order {
        role: role.into(),
        side: "sell".into(),
        status: Some("working".into()),
        filled_qty: Decimal::ZERO,
        limit_price: limit.map(d),
        stop_price: stop.map(d),
        ..sent(client, Some(&format!("BTCUSDT:{client}")), None)
    };
    let listed = |client: &str, limit: Option<f64>, stop: Option<f64>| brokers::OpenOrder {
        id: format!("BTCUSDT:{client}"),
        symbol: T.into(),
        side: "sell".into(),
        order_type: "x".into(),
        qty: 0.123,
        filled_qty: 0.0,
        limit_price: limit,
        stop_price: stop,
        trail_start: None,
        trail_rate: None,
        currency: "USDT".into(),
        asset_class: "crypto".into(),
        placed_at: None,
        venue: String::new(),
        account: String::new(),
        parent_id: None,
        reduce_only: true,
    };
    let rows = vec![
        row("s1", "stop", None, Some("95")),
        row("t1", "target", Some("110"), None),
        row("s2", "stop", None, Some("95")),
        row("t2", "target", Some("110"), None),
        row("e1", "entry", Some("100"), None),
    ];
    let broker = vec![
        listed("s1", None, Some(93.5)),
        listed("t1", Some(110.0000000001), None),
        listed("t2", None, None),
        listed("e1", Some(99.0), None),
    ];
    let moved = moved_exits(&rows, T, &broker);
    assert_eq!(moved, HashMap::from([("s1".to_string(), d("93.5"))]));
}

/// A spot buy's commission taken in the base asset never reached the account: the robot's
/// own position is what filled less it, so the account agrees and no act by hand is read.
#[test]
fn a_fee_taken_in_the_base_asset_is_not_a_drift() {
    let fee = |ccy: &str, symbol: &str| exec_store::Fill { fee: d("0.000123"), fee_currency: ccy.into(), symbol: symbol.into(), ..fill("e", "buy", "0.123", "100", "0", "") };
    let fills = [fee("BTC", T), fee("USDT", T), fee("BTC", "ETHBTC"), fee("BNB", T)];
    assert_eq!(withheld(&fills, T), d("0.000123"));
    let mut act = long_position("95", "110");
    act.held = Some(d("0.122877"));
    act.withheld = withheld(&fills, T);
    let eng = Intended { hold: Some(Hold { long: true, stop: Some(95.0), target: Some(110.0) }), ..Default::default() };
    let p = plan(SESSION, T, &eng, &act, &rules());
    assert_eq!((p.drift, p.stop), (Some(Decimal::ZERO), None));
    // Closed by the engine: the close sells what the account holds.
    let p = plan(SESSION, T, &Intended::default(), &act, &Rules { native: false, ..rules() });
    assert_eq!(placed(&p)[0][0].qty, d("0.122877"));
}

/// The user opened a position on the instrument while the robot was flat, or closed the
/// robot's position: the robot stops, sends nothing, and must be armed again.
#[test]
fn a_position_opened_or_closed_outside_the_robot_stops_it() {
    let opened = Actual { held: Some(d("0.2")), ..Default::default() };
    let p = plan(SESSION, T, &Intended { entry: Some(entry(true, "2026-10-28T10:05:00Z")), ..Default::default() }, &opened, &rules());
    assert!(p.stop.as_deref().unwrap().contains("long 0.2 BTCUSDT position was opened"), "{:?}", p.stop);
    assert!(p.actions.is_empty(), "{:?}", p.actions);
    assert_eq!(p.closed_outside, None);

    let eng = Intended { hold: Some(Hold { long: true, stop: Some(97.0), target: Some(110.0) }), ..Default::default() };
    for held in ["0", "-0.3"] {
        let mut act = long_position("95", "110");
        act.held = Some(d(held));
        let p = plan(SESSION, T, &eng, &act, &rules());
        assert!(p.stop.as_deref().unwrap().contains("long 0.123 BTCUSDT position was closed"), "{:?}", p.stop);
        assert_eq!(p.closed_outside, Some(d("0.123")));
        assert!(p.actions.is_empty(), "{:?}", p.actions);
    }
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
    // Not sent is not taken: the entry is written refused with why, so the book holds nothing.
    let refused: Vec<&str> = p.actions.iter().filter_map(|a| if let Action::Refuse { reason, .. } = a { Some(reason.as_str()) } else { None }).collect();
    assert_eq!(refused.len(), 1);
    assert!(refused[0].contains("was not sent: trading is halted"), "{}", refused[0]);
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
        assert_eq!(p.drift, Some(d("0.877")));
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

/// A trade of the book is decided by the last entry or limit on its side whose close came
/// before it entered: a refused one makes the trade not taken, a later sent one does not.
#[test]
fn a_trade_is_decided_by_the_last_opening_order_before_it_entered() {
    let order = |role: &str, side: &str, signal: &str, outcome: &str| exec_store::Order {
        intent: format!("{role}|{T}|{}|{signal}", if side == "buy" { "long" } else { "short" }),
        role: role.into(),
        side: side.into(),
        outcome: outcome.into(),
        ..sent(&format!("{role}{side}{signal}"), None, None)
    };
    let rows = [
        order("entry", "buy", "2026-10-28T10:00:00Z", "refused"),
        order("entry", "sell", "2026-10-28T10:05:00Z", "sent"),
        order("entry", "buy", "2026-10-28T10:10:00Z", "sent"),
        order("limit", "sell", "2026-10-28T10:20:00Z", "refused"),
    ];
    // The long the 10:00 close decided enters at the next open: its order was refused.
    assert_eq!(deciding(&rows, T, "long", "2026-10-28T10:01:00Z").map(|o| o.outcome.as_str()), Some("refused"));
    // The long of 10:10 was sent; the stop and target legs never decide.
    assert_eq!(deciding(&rows, T, "long", "2026-10-28T10:11:00Z").map(|o| o.outcome.as_str()), Some("sent"));
    // A short filled by the limit rested at 10:20, later on.
    assert_eq!(deciding(&rows, T, "short", "2026-10-28T10:45:00Z").map(|o| o.outcome.as_str()), Some("refused"));
    assert_eq!(deciding(&rows, T, "short", "2026-10-28T10:06:00Z").map(|o| o.outcome.as_str()), Some("sent"));
    // Nothing decided before it, or another instrument: no order.
    assert!(deciding(&rows, T, "long", "2026-10-28T10:00:00Z").is_none());
    assert!(deciding(&rows, "ETHUSDT", "long", "2026-10-28T10:11:00Z").is_none());
}

// ── Funds, money from fills ─────────────────────────────────────────────────

fn margin(available: f64, currency: &str) -> brokers::AccountSummary {
    brokers::AccountSummary { currency: currency.into(), equity: Some(available * 2.0), available: Some(available), ..Default::default() }
}

fn spot(quote_free: f64) -> brokers::AccountSummary {
    let a = |asset: &str, free: f64| brokers::AssetBalance { asset: asset.into(), free, locked: 0.0 };
    brokers::AccountSummary { assets: vec![a("BTC", 0.5), a("USDT", quote_free)], ..Default::default() }
}

/// An entry the account cannot pay for is refused with what it needs and what it has: a margin
/// order its notional over the leverage, a spot buy its whole cost in the quote asset; what an
/// earlier entry of the same sync took is spent.
#[test]
fn an_entry_the_account_cannot_pay_for_says_what_it_needs() {
    let one = Decimal::ONE;
    assert_eq!(shortfall(&margin(1000.0, "USDT"), true, d("0.1"), d("50000"), one, d("5"), one, Decimal::ZERO), Ok(d("1000")));
    let why = shortfall(&margin(1000.0, "USDT"), true, d("0.1"), d("50000"), one, d("2"), one, Decimal::ZERO).unwrap_err();
    assert!(why.contains("has 1000 USDT available and the entry needs 2500 USDT (0.1 at 50000, leverage 2)"), "{why}");
    // Spent by an earlier entry of the same sync.
    assert!(shortfall(&margin(1000.0, "USDT"), false, d("0.01"), d("50000"), one, one, one, d("600")).is_err());
    // An EUR account trading a USD instrument: the notional at the rate.
    assert!(shortfall(&margin(1000.0, "EUR"), true, d("10"), d("100"), one, one, d("0.9"), Decimal::ZERO).is_ok());
    assert!(shortfall(&margin(1000.0, "EUR"), true, d("12"), d("100"), one, one, d("0.9"), Decimal::ZERO).is_err());
    // Spot: the cost in the quote asset; a sell is the broker's to judge.
    assert!(shortfall(&spot(400.0), true, d("0.01"), d("50000"), one, d("10"), one, Decimal::ZERO).unwrap_err().contains("400 USDT available"));
    assert!(shortfall(&spot(0.0), false, d("0.01"), d("50000"), one, one, one, Decimal::ZERO).is_ok());
    // No figure stated: the broker decides.
    assert!(shortfall(&brokers::AccountSummary::default(), true, d("1"), d("1"), one, one, one, Decimal::ZERO).is_ok());
}

/// Arming reads the account's equity (a spot pair's quote asset), and refuses a robot whose
/// money is more than the account holds, naming both figures.
#[test]
fn a_robot_is_armed_on_an_account_that_holds_its_money() {
    assert_eq!(funds_of(&margin(1000.0, "USD")), (Some(d("2000")), "USD".to_string()));
    assert_eq!(funds_of(&spot(250.0)), (Some(d("250")), "USDT".to_string()));
    assert_eq!(funds_of(&brokers::AccountSummary::default()).0, None);
    assert_eq!(funds_short(d("500"), d("1000"), "USD"), None);
    let why = funds_short(d("1500"), d("1000"), "USD").unwrap();
    assert!(why.contains("holds 1000 USD and the bot works with 1500 USD"), "{why}");
}

fn fill(client_id: &str, side: &str, qty: &str, price: &str, fee: &str, ccy: &str) -> exec_store::Fill {
    exec_store::Fill {
        session_id: SESSION,
        account_id: Uuid::nil(),
        exec_id: format!("{client_id}{qty}{price}"),
        client_id: client_id.into(),
        symbol: T.into(),
        side: side.into(),
        qty: d(qty),
        price: d(price),
        fee: d(fee),
        fee_currency: ccy.into(),
        at: at("2026-10-28T10:00:00Z"),
    }
}

/// A round trip's money is what its fills did: the exits against the entry, by side, less
/// every fee read in the instrument's currency (the base at the fill's price, a third one at
/// its rate). Not priced until the exits filled the whole entry, nor with a fee it cannot read.
#[test]
fn a_round_trip_is_priced_from_its_fills_net_of_fees() {
    let entry = sent("in", None, None);
    let mut stop = sent("out", None, None);
    stop.role = "stop".into();
    stop.side = "sell".into();
    let none = HashMap::new();
    let fills = vec![
        fill("in", "buy", "0.6", "100", "0.0006", "BTC"),
        fill("in", "buy", "0.4", "101", "0.04", "USDT"),
        fill("out", "sell", "1", "110", "0.11", "USDT"),
    ];
    // Gross 110 - 60 - 40.4 = 9.6; fees 0.0006 x 100 + 0.04 + 0.11 = 0.21.
    assert_eq!(trade_money(&entry, &[&stop], &fills, Decimal::ONE, "USDT", "BTC", &none), Some((d("9.39"), d("0.21"))));
    // A short, and a contract multiplier.
    let mut short_in = entry.clone();
    short_in.side = "sell".into();
    let f = vec![fill("in", "sell", "2", "50", "0", ""), fill("out", "buy", "2", "45", "1", "")];
    assert_eq!(trade_money(&short_in, &[&stop], &f, d("10"), "USD", "", &none), Some((d("99"), d("1"))));
    // Not closed whole yet.
    assert_eq!(trade_money(&entry, &[&stop], &fills[..2], Decimal::ONE, "USDT", "BTC", &none), None);
    // A fee in BNB: priced at its rate, unknown without one.
    let bnb = vec![fill("in", "buy", "1", "100", "0.01", "BNB"), fill("out", "sell", "1", "110", "0", "USDT")];
    assert_eq!(trade_money(&entry, &[&stop], &bnb, Decimal::ONE, "USDT", "BTC", &none), None);
    let rates = HashMap::from([("BNB".to_string(), d("600"))]);
    assert_eq!(trade_money(&entry, &[&stop], &bnb, Decimal::ONE, "USDT", "BTC", &rates), Some((d("4"), d("6"))));
}

/// The testnet case of 2026-11-17: a short entered at 09:27, its native target filled at the
/// broker at 09:46 while the engine's data never reached it. The robot holds nothing more: the
/// exit is the broker's, at its fill price and time, and named by the leg that filled.
#[test]
fn a_position_the_broker_took_out_through_its_native_exits_is_out() {
    let order = |client: &str, role: &str, side: &str, intent: &str, position: &str, filled: &str| exec_store::Order {
        intent: intent.into(),
        role: role.into(),
        side: side.into(),
        position: position.into(),
        ticker: "SOLUSDT".into(),
        symbol: "SOLUSDT".into(),
        qty: d("0.83"),
        filled_qty: d(filled),
        status: Some(if filled == "0" { "cancelled" } else { "filled" }.into()),
        ..sent(client, None, None)
    };
    let entry = order("otse1", "entry", "sell", "entry|SOLUSDT|short|2026-11-17T09:26:00Z", "", "0.83");
    let stop = order("otss1", "stop", "buy", "entry|SOLUSDT|short|2026-11-17T09:26:00Z|stop", "otse1", "0");
    let target = order("otst1", "target", "buy", "entry|SOLUSDT|short|2026-11-17T09:26:00Z|target", "otse1", "0.83");
    let at_ = |f: exec_store::Fill, t: &str| exec_store::Fill { at: at(t), symbol: "SOLUSDT".into(), ..f };
    let fills = vec![
        at_(fill("otse1", "sell", "0.83", "72.06", "0.03", "USDT"), "2026-11-17T09:27:05Z"),
        at_(fill("otst1", "buy", "0.5", "69.36", "0.01", "USDT"), "2026-11-17T09:46:26Z"),
        at_(fill("otst1", "buy", "0.33", "69.36", "0.01", "USDT"), "2026-11-17T09:46:27Z"),
    ];
    let held = store::Trade {
        session_id: SESSION,
        trade_key: "SOLUSDT|short|2026-11-17T09:27:00Z".into(),
        ticker: "SOLUSDT".into(),
        direction: "short".into(),
        entry_ts: "2026-11-17T09:27:00Z".into(),
        exit_ts: String::new(),
        open: true,
        pnl: 0.0,
        fees: 0.0,
        trade: json!({ "entry_price": 72.26, "qty": 0.83 }),
        opened_at: OffsetDateTime::UNIX_EPOCH,
        closed_at: None,
        robot_qty: Some(d("0.83")),
        scale: Decimal::ONE,
        robot_note: String::new(),
        robot_pnl: None,
        robot_fees: None,
        money_at_broker: false,
    };
    let rows = vec![entry.clone(), stop.clone(), target.clone()];
    let out = broker_exits(std::slice::from_ref(&held), std::slice::from_ref(&held), &rows, &fills, &HashMap::new());
    assert_eq!(out.len(), 1);
    let x = &out[0];
    assert_eq!((x.trade_key.as_str(), x.reason.as_str()), ("SOLUSDT|short|2026-11-17T09:27:00Z", "take_profit"));
    assert_eq!((x.qty, x.price, x.entry_price, x.fees), (d("0.83"), d("69.36"), Some(d("72.06")), d("0.05")));
    assert_eq!(x.at, at("2026-11-17T09:46:27Z"));

    // Not whole yet: the target filled part of the position.
    let partial = vec![entry.clone(), stop.clone(), exec_store::Order { filled_qty: d("0.5"), ..target.clone() }];
    assert!(broker_exits(std::slice::from_ref(&held), std::slice::from_ref(&held), &partial, &fills[..2], &HashMap::new()).is_empty());
    // The robot's own close took part: the engine decided that exit, not the broker.
    let closed = vec![entry.clone(), stop.clone(), target.clone(), order("otsc1", "close", "buy", "close|otse1|0", "otse1", "0.83")];
    assert!(broker_exits(std::slice::from_ref(&held), std::slice::from_ref(&held), &closed, &fills, &HashMap::new()).is_empty());
    // A stop names the engine's own reason for it.
    let stopped = vec![entry, exec_store::Order { filled_qty: d("0.83"), ..stop }, exec_store::Order { filled_qty: d("0"), ..target }];
    let stop_fills = vec![fills[0].clone(), exec_store::Fill { client_id: "otss1".into(), qty: d("0.83"), ..fills[1].clone() }];
    let reasons = HashMap::from([(held.trade_key.clone(), "trailing_stop".to_string())]);
    let out = broker_exits(std::slice::from_ref(&held), std::slice::from_ref(&held), &stopped, &stop_fills, &reasons);
    assert_eq!(out[0].reason, "trailing_stop");
}

fn sol_order(client: &str, role: &str, side: &str, intent: &str, position: &str, qty: &str, filled: &str) -> exec_store::Order {
    exec_store::Order {
        intent: intent.into(),
        role: role.into(),
        side: side.into(),
        position: position.into(),
        ticker: "SOLUSDT".into(),
        symbol: "SOLUSDT".into(),
        qty: d(qty),
        filled_qty: d(filled),
        status: Some(if filled == "0" { "cancelled" } else { "filled" }.into()),
        ..sent(client, None, None)
    }
}

fn sol_fill(client: &str, side: &str, qty: &str, price: &str, t: &str) -> exec_store::Fill {
    exec_store::Fill { at: at(t), symbol: "SOLUSDT".into(), ..fill(client, side, qty, price, "0.01", "USDT") }
}

fn sol_trade(direction: &str, entry_ts: &str, open: bool, qty: &str) -> store::Trade {
    store::Trade {
        session_id: SESSION,
        trade_key: format!("SOLUSDT|{direction}|{entry_ts}"),
        ticker: "SOLUSDT".into(),
        direction: direction.into(),
        entry_ts: entry_ts.into(),
        exit_ts: String::new(),
        open,
        pnl: 0.0,
        fees: 0.0,
        trade: json!({ "qty": qty.parse::<f64>().unwrap() }),
        opened_at: at(entry_ts) + time::Duration::minutes(1),
        closed_at: None,
        robot_qty: Some(d(qty)),
        scale: Decimal::ONE,
        robot_note: String::new(),
        robot_pnl: None,
        robot_fees: None,
        money_at_broker: false,
    }
}

fn on_broker() -> store::Session {
    store::Session { execution: Some(json!({ "target": "broker" })), ..super::super::tests::session() }
}

/// BinanceLive, 2026-11-17: the long entered at 12:03 (7.8 at 73.34) was closed by Stop and
/// flatten at 12:07 (7.8 at 73.26). That close is the trade's exit, at the venue's price: the
/// robot holds nothing more, and its P&L is the fills'.
#[test]
fn a_flattened_position_is_out_at_its_fill() {
    let entry = sol_order("otsecd", "entry", "buy", "entry|SOLUSDT|long|2026-11-17T12:02:00Z", "", "7.8", "7.8");
    let flat = sol_order("otsce2", "close", "sell", "close|otsecd|flatten21", "otsecd", "7.8", "7.8");
    let fills = vec![sol_fill("otsecd", "buy", "7.8", "73.34", "2026-11-17T12:03:15Z"), sol_fill("otsce2", "sell", "7.8", "73.26", "2026-11-17T12:07:08Z")];
    let held = sol_trade("long", "2026-11-17T12:03:00Z", true, "7.8");
    let rows = [entry, flat];
    let out = broker_exits(std::slice::from_ref(&held), std::slice::from_ref(&held), &rows, &fills, &HashMap::new());
    assert_eq!(out.len(), 1);
    assert_eq!((out[0].reason.as_str(), out[0].price, out[0].entry_price, out[0].qty), ("flatten", d("73.26"), Some(d("73.34")), d("7.8")));
    assert_eq!(out[0].at, at("2026-11-17T12:07:08Z"));
    // Its fill not read yet: it waits, nothing is guessed.
    assert!(broker_exits(std::slice::from_ref(&held), std::slice::from_ref(&held), &rows, &fills[..1], &HashMap::new()).is_empty());
    // The page shows no position for it from the moment the close filled.
    assert!(super::super::dashboard::not_at_broker(&on_broker(), std::slice::from_ref(&held), &rows).contains(&held.trade_key));
    // The P&L is the fills': (73.26 - 73.34) x 7.8 less 0.02 of fees.
    let exits: Vec<&exec_store::Order> = vec![&rows[1]];
    assert_eq!(trade_money(&rows[0], &exits, &fills, Decimal::ONE, "USDT", "SOL", &HashMap::new()), Some((d("-0.644"), d("0.02"))));
    // Shown as filled: the entry's and the flatten's prices, the flatten's time and reason, not
    // the engine's live price nor its later reverse.
    assert_eq!(
        broker_fills(&rows[0], &exits, &fills),
        Some(json!({ "entry_price": 73.34, "exit_price": 73.26, "exit_ts": "2026-11-17T12:07:08Z", "exit_reason": "flatten" }))
    );
    // An exit the engine decided keeps its time and reason; the prices are still the fills'.
    let engine_close = sol_order("otsce2", "close", "sell", "close|otsecd|0", "otsecd", "7.8", "7.8");
    assert_eq!(broker_fills(&rows[0], &[&engine_close], &fills), Some(json!({ "entry_price": 73.34, "exit_price": 73.26 })));
    // Not out whole yet: nothing.
    assert_eq!(broker_fills(&rows[0], &exits, &fills[..1]), None);
}

/// The user's own close at the broker: the venue's executions of it close the trade, at their
/// price. Until they are read, the trade waits.
#[test]
fn a_position_closed_at_the_broker_is_out_at_the_venues_price() {
    let entry = sol_order("e1", "entry", "buy", "entry|SOLUSDT|long|2026-11-17T12:02:00Z", "", "7.8", "7.8");
    let mut outside = sol_order("c1", "close", "sell", "close|e1|outside|2026-11-17T12:10:00Z", "e1", "7.8", "7.8");
    outside.reason = Some(CLOSED_OUTSIDE.into());
    let held = sol_trade("long", "2026-11-17T12:03:00Z", true, "7.8");
    let rows = [entry, outside.clone()];
    let entry_fill = sol_fill("e1", "buy", "7.8", "73.34", "2026-11-17T12:03:15Z");
    assert!(broker_exits(std::slice::from_ref(&held), std::slice::from_ref(&held), &rows, std::slice::from_ref(&entry_fill), &HashMap::new()).is_empty());
    let fills = vec![entry_fill, sol_fill("c1", "sell", "5", "73.50", "2026-11-17T12:09:00Z"), sol_fill("c1", "sell", "2.8", "73.40", "2026-11-17T12:09:01Z")];
    let out = broker_exits(std::slice::from_ref(&held), std::slice::from_ref(&held), &rows, &fills, &HashMap::new());
    assert_eq!(out[0].reason, "closed_outside");
    // (5 x 73.50 + 2.8 x 73.40) / 7.8
    assert_eq!(out[0].price.round_dp(6), d("73.464103"));
    let now = OffsetDateTime::now_utc();
    assert!(awaits_fills(&exec_store::Order { created_at: now, ..outside.clone() }, &fills[..1]));
    assert!(!awaits_fills(&exec_store::Order { created_at: now, ..outside }, &fills));
}

/// BinancePaperCrossSMA, 2026-11-17: the reverse of 12:03 closed the short, then its long entry
/// was held back by a halt and never left. The robot's book held that long anyway, matched to
/// the long of 11:02 (an older trade's order). A trade's opening order comes after the trade
/// before it: this long has none, so it is not taken, and the page shows nothing.
#[test]
fn an_entry_that_never_left_is_not_taken() {
    let rows = [
        sol_order("l1", "entry", "buy", "entry|SOLUSDT|long|2026-11-17T11:02:00Z", "", "0.83", "0.83"),
        sol_order("c1", "close", "sell", "close|l1|0", "l1", "0.83", "0.83"),
        sol_order("s1", "entry", "sell", "entry|SOLUSDT|short|2026-11-17T11:05:00Z", "", "0.83", "0.83"),
        sol_order("c2", "close", "buy", "close|s1|0", "s1", "0.83", "0.83"),
    ];
    let long_before = sol_trade("long", "2026-11-17T11:03:00Z", false, "0.83");
    let short = sol_trade("short", "2026-11-17T11:06:00Z", false, "0.83");
    let long = sol_trade("long", "2026-11-17T12:03:00Z", true, "0.81");
    let book = [long_before.clone(), short.clone(), long.clone()];
    // Unbounded, the older long's order would claim it.
    assert_eq!(deciding(&rows, "SOLUSDT", "long", &long.entry_ts).map(|o| o.client_id.as_str()), Some("l1"));
    assert!(opening_order(&rows, &book, &long).is_none());
    let now = at("2026-11-17T12:05:30Z");
    assert_eq!(not_taken(&rows, &book, &long, now).as_deref(), Some("not taken: no order was sent for this entry"));
    // A minute after its row is written at the soonest: the entry may still be in flight.
    assert_eq!(not_taken(&rows, &book, &long, at("2026-11-17T12:04:30Z")), None);
    // The trades that were taken keep their orders.
    assert_eq!(opening_order(&rows, &book, &short).map(|o| o.client_id.as_str()), Some("s1"));
    assert_eq!(opening_order(&rows, &book, &long_before).map(|o| o.client_id.as_str()), Some("l1"));
    assert_eq!(not_taken(&rows, &book, &short, now), None);
    // Its entry refused, or ended unfilled at the broker: not taken either, with why.
    let mut refused = rows.to_vec();
    refused.push(exec_store::Order {
        outcome: "refused".into(),
        reason: Some("the long entry on SOLUSDT decided at the close of 2026-11-17T12:02:00Z was not sent: trading is halted on the account (position_drift)".into()),
        ..sol_order("l2", "entry", "buy", "entry|SOLUSDT|long|2026-11-17T12:02:00Z", "", "0", "0")
    });
    assert!(not_taken(&refused, &book, &long, now).unwrap().contains("halted on the account (position_drift)"));
    let mut expired = rows.to_vec();
    expired.push(exec_store::Order { status: Some("expired".into()), ..sol_order("l2", "entry", "buy", "entry|SOLUSDT|long|2026-11-17T12:02:00Z", "", "0.81", "0") });
    assert_eq!(not_taken(&expired, &book, &long, now).as_deref(), Some("not taken: the entry was expired at the broker without a fill"));
    // Filled: taken.
    let mut filled = rows.to_vec();
    filled.push(sol_order("l2", "entry", "buy", "entry|SOLUSDT|long|2026-11-17T12:02:00Z", "", "0.81", "0.81"));
    assert_eq!(not_taken(&filled, &book, &long, now), None);
    // The page shows no position for it, and shows the taken one.
    assert!(super::super::dashboard::not_at_broker(&on_broker(), &book, &rows).contains(&long.trade_key));
    assert!(!super::super::dashboard::not_at_broker(&on_broker(), &book, &filled).contains(&long.trade_key));
}

#[test]
fn a_binance_fill_is_matched_by_the_number_the_lookup_read() {
    // Binance follows an order by `SYMBOL:client id`; its trade history names its own number.
    let rows = [sent("k1", Some("BTCUSDT:k1"), None), sent("k2", Some("BTCUSDT:k2"), Some("4419087"))];
    assert_eq!(order_of(&rows, "4419087", "").map(|o| o.client_id.as_str()), Some("k2"));
    assert!(order_of(&rows, "28", "").is_none(), "a number no lookup read matches no order");
    // A venue whose history names the id the order is followed by still matches on it.
    let ibkr = [sent("k3", Some("42"), None)];
    assert_eq!(order_of(&ibkr, "42", "").map(|o| o.client_id.as_str()), Some("k3"));
}

/// IB names a fill by its permanent id, never the id the order is cancelled by, and lists the
/// day's completed orders under no id at all: the `orderRef` echoed with the fill is the match.
#[test]
fn an_ibkr_fill_is_matched_by_its_order_ref() {
    let rows = [sent("k1", Some("-1"), None), sent("k2", Some("17"), None)];
    assert_eq!(order_of(&rows, "1538653993", "k2").map(|o| o.client_id.as_str()), Some("k2"));
    assert!(order_of(&rows, "1538653993", "").is_none(), "a permanent id alone names no order");
    assert!(order_of(&rows, "", "other").is_none(), "another client's order is not the bot's");
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

    // The last close decided a market exit at the next open: not held from that close on, so
    // the sync closes it now instead of one bar later.
    let leaving = json!({ "open_positions": [{ "ticker": T, "direction": "long", "entry_ts": "2026-10-28T10:00:00Z",
        "avg_price": 100.0, "qty": 1.0, "fees": 0.0, "stop_reason": "stop_loss", "leaves_at_open": true }] });
    assert_eq!(intended(Some(&leaving), T, &[], &settings, 60).hold, None);

    // Stop and reverse: a market entry on the other side of the held position is the side
    // held from that close.
    let reverse = json!({
        "open_positions": [{ "ticker": T, "direction": "long", "entry_ts": "2026-10-28T10:00:00Z", "avg_price": 100.0,
            "qty": 1.0, "fees": 0.0, "stop_reason": "stop_loss" }],
        "pending_orders": [{ "ticker": T, "kind": "entry", "direction": "short", "order": "market",
            "price": 100.0, "bars_left": 1, "signal_ts": "2026-10-28T10:05:00Z", "qty": 1.0 }]
    });
    let eng = intended(Some(&reverse), T, &[], &settings, 60);
    assert_eq!((eng.hold.map(|h| h.long), eng.entry.map(|e| e.long)), (Some(false), Some(false)));
}

/// A simulated session is never a broker one: no target, the sync answers at once.
#[test]
fn a_simulated_session_has_no_broker_target() {
    let input: store::SessionInput = serde_json::from_value(json!({ "name": "sim", "settings": {}, "kind": "interval", "timezone": "UTC" })).unwrap();
    let mut s = crate::robots::api::probe_session(&input);
    assert!(Execution::of(&s).is_none() && !s.on_broker());
    s.execution = Some(json!({ "target": "simulate" }));
    assert!(Execution::of(&s).is_none() && !s.on_broker());
    s.execution = Some(json!({ "target": "broker", "account_id": Uuid::nil() }));
    assert!(Execution::of(&s).is_some() && s.on_broker());
}

// ── Priority, providers and rate holds ──────────────────────────────────────

fn robot(id: u128, priority: i32, broker: &str) -> store::BrokerRobot {
    store::BrokerRobot { id: Uuid::from_u128(id), priority, broker: broker.into() }
}

/// The pass serves the important robots first: by priority, grouped by the provider whose
/// limits they share, each group where its first robot comes.
#[test]
fn a_pass_serves_robots_by_priority_grouped_by_provider() {
    let robots = vec![robot(1, 100, "binance"), robot(2, 5, "ibkr"), robot(3, 50, "binance"), robot(4, 5, "paper"), robot(5, 200, "ibkr")];
    let g = groups(&robots);
    let order: Vec<(&str, Vec<u128>)> = g.iter().map(|(p, ids)| (p.as_str(), ids.iter().map(|i| i.as_u128()).collect())).collect();
    assert_eq!(order, vec![("ibkr", vec![2, 5]), ("paper", vec![4]), ("binance", vec![3, 1])]);
}

/// A provider on hold skips its whole group, and only it: no robot waits on another venue's
/// limit.
#[test]
fn a_rate_hold_skips_its_provider_and_only_it() {
    let robots = vec![robot(1, 10, "binance"), robot(2, 20, "ibkr"), robot(3, 30, "binance")];
    let wait = Duration::from_secs(30);
    let steps = plan_pass(&robots, |p| (p == "binance").then_some(wait));
    assert_eq!(steps, vec![
        PassStep::Hold { provider: "binance".into(), ids: vec![Uuid::from_u128(1), Uuid::from_u128(3)], wait },
        PassStep::Sync { provider: "ibkr".into(), ids: vec![Uuid::from_u128(2)] },
    ]);
    assert!(plan_pass(&robots, |_| None).iter().all(|s| matches!(s, PassStep::Sync { .. })));
    assert!(rate_note("binance", at("2026-11-08T10:00:30Z")).starts_with("waiting for binance rate limit until 2026-11-08T10:00:30Z"));
    // A Retry-After wins; without one, a refusal holds one pass and then lets go.
    assert_eq!(provider_hold(Some(Duration::from_secs(40)), Some(Duration::from_secs(1))), Some(Duration::from_secs(40)));
    assert_eq!(provider_hold(None, Some(Duration::from_secs(5))), Some(Duration::from_secs(10)));
    assert_eq!(provider_hold(None, Some(UNTIMED_HOLD)), None);
    assert_eq!(provider_hold(None, None), None);
}

/// The send budget holds entries back once spent; exits and closes always go.
#[test]
fn the_send_budget_holds_entries_and_never_exits() {
    let mut b = Budget::new(2);
    assert!(b.spend(true));
    assert!(b.spend(false));
    assert!(!b.spend(true), "the third entry waits for the next pass");
    assert!(b.spend(false), "an exit goes whatever the budget");
    assert_eq!(b.left, 0);
}

/// The robot's own caps and its guards: a refused entry is written, never asked again; the
/// guard scale sizes the entry; a guard hold refuses a market entry and notes a limit.
#[test]
fn the_robot_layers_size_refuse_and_hold() {
    let e = entry(true, "2026-10-28T10:05:00Z");
    let scaled = Rules { scale: d("0.5"), ..rules() };
    let p = plan(SESSION, T, &Intended { entry: Some(e.clone()), ..Default::default() }, &Actual::default(), &scaled);
    assert_eq!(placed(&p)[0][0].qty, d("0.061"), "0.12345 x 0.5 cut down to the step");

    let capped = Rules { capital: sizing::Capital::validate(&json!({ "max_positions": 1 })).unwrap(), open_positions: 1, ..rules() };
    let p = plan(SESSION, T, &Intended { entry: Some(e.clone()), ..Default::default() }, &Actual::default(), &capped);
    assert!(placed(&p).is_empty());
    let refused: Vec<&Action> = p.actions.iter().filter(|a| matches!(a, Action::Refuse { .. })).collect();
    assert_eq!(refused.len(), 1, "{:?}", p.actions);
    let Action::Refuse { leg: refused_leg, reason } = refused[0] else { unreachable!() };
    assert_eq!(refused_leg.intent, "entry|BTCUSDT|long|2026-10-28T10:05:00Z");
    assert!(reason.contains("its maximum of 1"), "{reason}");

    let held = Rules { guard_hold: Some("paused by its guard (x) until later".into()), ..rules() };
    let p = plan(SESSION, T, &Intended { entry: Some(e), ..Default::default() }, &Actual::default(), &held);
    assert!(placed(&p).is_empty());
    assert!(p.actions.iter().any(|a| matches!(a, Action::Refuse { reason, .. } if reason.contains("paused by its guard"))), "{:?}", p.actions);
    // The same refusal, once written, is not asked again.
    let written = leg(Role::Entry, "entry|BTCUSDT|long|2026-10-28T10:05:00Z", true, "0", "0", false);
    let act = Actual { legs: vec![Leg { refused: true, ..written }], ..Default::default() };
    let p = plan(SESSION, T, &Intended { entry: Some(entry(true, "2026-10-28T10:05:00Z")), ..Default::default() }, &act, &held);
    assert!(p.actions.is_empty(), "{:?}", p.actions);
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
            notifications: crate::notifications_stream::NotifHub::new(),
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
        /// The venue does not answer at all: not even the account.
        down: std::sync::Arc<std::sync::atomic::AtomicBool>,
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
            if self.0.down.load(std::sync::atomic::Ordering::SeqCst) {
                return ResponseTemplate::new(503).set_body_string("upstream timed out");
            }
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
        let ex = vet(state, Some(&raw), &s, &[dataset]).await?.unwrap();
        let input: store::SessionInput = serde_json::from_value(json!({
            "name": name, "settings": settings(), "dataset_ids": [dataset], "kind": "interval", "timezone": "UTC",
            "every_minutes": 1, "status": "active", "feed": "hist", "notify_when": "on_change", "cadence": "each"
        }))
        .unwrap();
        let created = store::create_session(&state.pool, &input, None, None).await.unwrap();
        let target = serde_json::to_value(&ex).unwrap();
        exec_store::attach(&state.pool, created.id, account.id, &target).await.unwrap();
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
    /// the fill makes the session's position, the user's change beside it is theirs, the
    /// position closed by hand stops the robot, and the account cannot take a second session
    /// on the same instrument.
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

        // A second robot on the same instrument and account is created: only one is armed.
        let second = session(&state, &acct, "second").await.unwrap_or_else(|e| panic!("{}", e.message));

        // Not armed: the decision stays with the engine.
        decide_entry(&state, s.id).await;
        sync(&state, s.id).await.unwrap();
        assert_eq!(v.posts(), 0, "a disarmed session sends nothing");

        let baseline = baseline(&state, &store::get_session(&state.pool, s.id).await.unwrap().unwrap()).await.unwrap();
        // Armed after that close: still not sent (decided before arming). Re-decide after.
        assert_eq!(exec_store::arm(&state.pool, s.id, Uuid::nil(), &baseline).await.unwrap(), None);
        // Two armed robots on one position: the second is refused until the first is disarmed.
        assert_eq!(exec_store::arm(&state.pool, second.id, Uuid::nil(), &baseline).await.unwrap().as_deref(), Some("BTCUSDT"));
        assert!(store::get_session(&state.pool, second.id).await.unwrap().unwrap().armed_at.is_none());
        assert_eq!(exec_store::taken(&state.pool, acct.id, &["BTCUSDT".into()], Some(second.id)).await.unwrap(), vec![("BTCUSDT".to_string(), "e2e".to_string())]);
        assert!(exec_store::disarm(&state.pool, s.id).await.unwrap());
        assert_eq!(exec_store::arm(&state.pool, second.id, Uuid::nil(), &baseline).await.unwrap(), None);
        assert!(exec_store::disarm(&state.pool, second.id).await.unwrap());
        assert_eq!(exec_store::arm(&state.pool, s.id, Uuid::nil(), &baseline).await.unwrap(), None);
        sync(&state, s.id).await.unwrap();
        assert_eq!(v.posts(), 0, "an entry decided before arming is not sent");
        let armed_at = OffsetDateTime::now_utc() - time::Duration::minutes(5);
        sqlx::query("UPDATE robots SET armed_at = $2 WHERE id = $1").bind(s.id).bind(armed_at).execute(&state.pool).await.unwrap();
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

        // Bought beside the robot's position, outside it: the user's, folded into the baseline,
        // and the robot goes on.
        *v.btc.lock().unwrap() = "0.5".into();
        sync(&state, s.id).await.unwrap();
        let s3 = store::get_session(&state.pool, s.id).await.unwrap().unwrap();
        assert_eq!(s3.exec_state, "synced", "{}", s3.exec_note);
        assert_eq!(s3.exec_baseline.get("BTCUSDT").and_then(Value::as_str), Some("0.377"));
        assert!(s3.armed_at.is_some());

        // The robot's position sold at the broker by hand: the robot stops, disarmed, said once;
        // its position is written closed outside, so it holds nothing when armed again.
        *v.btc.lock().unwrap() = "0.377".into();
        sync(&state, s.id).await.unwrap();
        sync(&state, s.id).await.unwrap();
        let s4 = store::get_session(&state.pool, s.id).await.unwrap().unwrap();
        assert_eq!((s4.exec_state.as_str(), s4.armed_at), ("stopped", None));
        assert!(s4.exec_note.contains("position was closed at the broker outside the bot"), "{}", s4.exec_note);
        let notices = store::list_events(&state.pool, Some(s.id), false, 50).await.unwrap().into_iter().filter(|e| e.title.contains("stopped")).count();
        assert_eq!(notices, 1, "one notice per distinct problem");
        let rows = exec_store::orders(&state.pool, s.id).await.unwrap();
        let outside = rows.iter().find(|o| o.reason.as_deref() == Some(CLOSED_OUTSIDE)).unwrap();
        assert_eq!((outside.role.as_str(), outside.side.as_str(), outside.filled_qty), ("close", "sell", d("0.123")));

        // Armed again: the account is measured anew, the robot is flat and in step with it.
        let again = super::baseline(&state, &s4).await.unwrap();
        assert_eq!(again.get("BTCUSDT").and_then(Value::as_str), Some("0.377"));
        assert_eq!(exec_store::arm(&state.pool, s.id, Uuid::nil(), &again).await.unwrap(), None);
        sync(&state, s.id).await.unwrap();
        let s5 = store::get_session(&state.pool, s.id).await.unwrap().unwrap();
        assert_eq!(s5.exec_state, "synced", "{}", s5.exec_note);
        assert_eq!(v.posts(), 1, "nothing sent to close what the user already closed");

        // Disarming the account (background trading off) disarms the session.
        let d = crate::brokers::dispatch::disarm_account(&state, acct.id).await.unwrap();
        assert_eq!(d.strategies, 1);
        assert!(store::get_session(&state.pool, s.id).await.unwrap().unwrap().armed_at.is_none());
    }

    /// A broker that stops answering: a passing outage is not news, three syncs in a row the
    /// robot says it once, sends nothing meanwhile, and says it is back when the account reads.
    #[tokio::test]
    async fn a_broker_that_stops_answering_is_said_once() {
        let Some(state) = scratch_state().await else { return };
        let server = MockServer::start().await;
        let v = Venue::default();
        mock_venue(&server, &v).await;
        crate::brokers::serve_from(&server.uri());
        let acct = account(&state, "down").await;
        let s = session(&state, &acct, "down").await.unwrap_or_else(|e| panic!("{}", e.message));
        let base = super::baseline(&state, &s).await.unwrap();
        assert_eq!(exec_store::arm(&state.pool, s.id, Uuid::nil(), &base).await.unwrap(), None);
        sync(&state, s.id).await.unwrap();
        let said = |t: &'static str| {
            let state = state.clone();
            async move { store::list_events(&state.pool, Some(s.id), false, 50).await.unwrap().into_iter().filter(|e| e.title.contains(t)).count() }
        };

        v.down.store(true, std::sync::atomic::Ordering::SeqCst);
        for _ in 1..UNREACHABLE_AFTER {
            sync(&state, s.id).await.unwrap();
        }
        assert_eq!(store::get_session(&state.pool, s.id).await.unwrap().unwrap().exec_state, "synced");
        assert_eq!(said("cannot reach").await, 0, "a passing outage is not news");
        sync(&state, s.id).await.unwrap();
        sync(&state, s.id).await.unwrap();
        let down = store::get_session(&state.pool, s.id).await.unwrap().unwrap();
        assert_eq!(down.exec_state, "unreachable");
        assert!(down.exec_note.contains("nothing is sent until it answers again"), "{}", down.exec_note);
        assert_eq!(said("cannot reach").await, 1, "said once");

        v.down.store(false, std::sync::atomic::Ordering::SeqCst);
        sync(&state, s.id).await.unwrap();
        assert_eq!(store::get_session(&state.pool, s.id).await.unwrap().unwrap().exec_state, "synced");
        assert_eq!(said("back to normal").await, 1);
        assert_eq!(v.posts(), 0);
    }

    /// A venue that asked to be left alone holds its robots for the pass: nothing is read or
    /// sent there, the robot says until when once, and the same robot syncs again once the
    /// provider is no longer on hold.
    #[tokio::test]
    async fn a_rate_hold_skips_the_robot_and_says_so_once() {
        let Some(state) = scratch_state().await else { return };
        let server = MockServer::start().await;
        let v = Venue::default();
        mock_venue(&server, &v).await;
        crate::brokers::serve_from(&server.uri());
        let acct = account(&state, "rate").await;
        let s = session(&state, &acct, "rate").await.unwrap_or_else(|e| panic!("{}", e.message));
        exec_store::arm(&state.pool, s.id, Uuid::nil(), &json!({ "BTCUSDT": "0" })).await.unwrap();
        sqlx::query("UPDATE robots SET armed_at = now() - interval '5 minutes' WHERE id = $1").bind(s.id).execute(&state.pool).await.unwrap();
        decide_entry(&state, s.id).await;
        let mine = [store::BrokerRobot { id: s.id, priority: 1, broker: "binance".into() }];
        // A pass planned with binance on hold: the robot is held, nothing leaves.
        let held = plan_pass(&mine, |p| (p == "binance").then_some(Duration::from_secs(30)));
        assert!(matches!(&held[0], PassStep::Hold { ids, .. } if ids == &vec![s.id]));
        let until = OffsetDateTime::now_utc() + time::Duration::seconds(31);
        hold_for_rate(&state, s.id, "binance", until).await.unwrap();
        hold_for_rate(&state, s.id, "binance", until).await.unwrap();
        assert_eq!(v.posts(), 0, "a held provider is sent nothing");
        let s1 = store::get_session(&state.pool, s.id).await.unwrap().unwrap();
        assert!(s1.exec_note.starts_with("waiting for binance rate limit until"), "{}", s1.exec_note);
        let notices = store::list_events(&state.pool, Some(s.id), false, 50).await.unwrap().into_iter().filter(|e| e.message.starts_with("waiting for binance")).count();
        assert_eq!(notices, 1, "said once");
        // The hold is over: the next pass syncs it and the entry leaves once.
        let mut budget = Budget::new(SENDS_PER_PASS);
        sync_with(&state, s.id, &mut budget).await.unwrap();
        assert_eq!(v.posts(), 1);
        assert_eq!(budget.left, SENDS_PER_PASS - 1, "the entry spent the provider's budget");
    }

    /// A strategy that states no tick: its stop and target go out on the venue's own tick
    /// (the paper broker reads it from the instrument's facts), never refused off it.
    #[tokio::test]
    async fn exits_go_out_on_the_venue_tick_when_the_strategy_states_none() {
        let Some(state) = scratch_state().await else { return };
        let acct = paper_account(&state).await;
        let meta: otw_store::instrument_meta::InstrumentMeta = serde_json::from_value(json!({ "provider": "binance", "asset_type": "crypto", "ticker": "ETHUSDT", "currency": "USDT", "mintick": 0.01 })).unwrap();
        otw_store::instrument_meta::put(&state.pool, &meta).await.unwrap();
        let dataset = otw_store::histdata::upsert_dataset(&state.pool, "binance", "crypto", "ETHUSDT", "1m").await.unwrap();
        let bars: Vec<otw_store::histdata::Bar> = (0..3)
            .map(|i| otw_store::histdata::Bar { ts: OffsetDateTime::now_utc() - time::Duration::minutes(3 - i), open: 3012.0, high: 3013.0, low: 3011.0, close: 3012.345, volume: 1.0, adj_open: None, adj_high: None, adj_low: None, adj_close: None })
            .collect();
        otw_store::histdata::write_bars(&state.pool, dataset, &bars).await.unwrap();
        let strategy = json!({ "mode": "long", "long": { "stop_loss": { "kind": "pct", "value": 0.001 }, "take_profit": { "kind": "pct", "value": 0.001 } }, "sizing": { "mode": "fixed_qty", "qty": 0.004 } });
        let s: Settings = serde_json::from_value(strategy.clone()).unwrap();
        let raw = json!({ "target": "broker", "account_id": acct.id });
        let ex = vet(&state, Some(&raw), &s, &[dataset]).await.unwrap_or_else(|e| panic!("{}", e.message)).unwrap();
        let input: store::SessionInput = serde_json::from_value(json!({
            "name": "tick", "settings": strategy, "dataset_ids": [dataset], "kind": "interval", "timezone": "UTC",
            "every_minutes": 1, "status": "active", "feed": "hist", "notify_when": "on_change", "cadence": "each"
        }))
        .unwrap();
        let created = store::create_session(&state.pool, &input, None, None).await.unwrap();
        // ETHUSDT: no other test writes its facts, so the tick read is this test's own.
        exec_store::attach(&state.pool, created.id, acct.id, &serde_json::to_value(&ex).unwrap()).await.unwrap();
        let id = created.id;
        exec_store::arm(&state.pool, id, Uuid::nil(), &json!({ "ETHUSDT": "0" })).await.unwrap();
        sqlx::query("UPDATE robots SET armed_at = now() - interval '5 minutes' WHERE id = $1").bind(id).execute(&state.pool).await.unwrap();
        let signal = OffsetDateTime::now_utc() - time::Duration::minutes(3);
        let signal = (signal - time::Duration::seconds(signal.second() as i64)).replace_nanosecond(0).unwrap();
        let ts = signal.format(&time::format_description::well_known::Rfc3339).unwrap();
        let stats = json!({ "pending_orders": [{ "ticker": "ETHUSDT", "kind": "entry", "direction": "long", "order": "market",
            "price": 3012.345, "bars_left": 1, "signal_ts": ts, "qty": 0.004 }] });
        store::finish_tick(&state.pool, id, None, Some(&stats), 3, "h", 1, "").await.unwrap();
        sync(&state, id).await.unwrap();
        let rows = exec_store::orders(&state.pool, id).await.unwrap();
        assert!(rows.len() == 3 && rows.iter().all(|o| o.outcome == "sent"), "{rows:?}");
        let stop = rows.iter().find(|o| o.role == "stop").and_then(|o| o.stop_price).unwrap();
        let target = rows.iter().find(|o| o.role == "target").and_then(|o| o.limit_price).unwrap();
        assert_eq!((stop, target), (d("3009.33"), d("3015.36")), "3012.345 less and plus 0.1%, on the 0.01 tick");
    }

    /// A robot on a broker counts only what it sent: an engine trade whose entry came before
    /// the robot was armed is in its book as not sent, one after is taken at the robot's size.
    #[tokio::test]
    async fn a_broker_robot_book_counts_only_what_it_sent() {
        let Some(state) = scratch_state().await else { return };
        let acct = paper_account(&state).await;
        let input: store::SessionInput = serde_json::from_value(json!({
            "name": "armed book", "settings": { "mode": "long" }, "dataset_ids": [], "kind": "interval", "timezone": "UTC",
            "every_minutes": 1, "status": "active", "feed": "hist", "notify_when": "never", "cadence": "each"
        }))
        .unwrap();
        let created = store::create_session(&state.pool, &input, None, None).await.unwrap();
        let target = json!({ "target": "broker", "account_id": acct.id, "account": acct.name, "broker": "paper", "symbols": {}, "exits": "watch" });
        exec_store::attach(&state.pool, created.id, acct.id, &target).await.unwrap();
        exec_store::arm(&state.pool, created.id, Uuid::nil(), &json!({})).await.unwrap();
        sqlx::query("UPDATE robots SET armed_at = '2026-11-08T10:00:30Z', inputs_hash = 'seeded' WHERE id = $1").bind(created.id).execute(&state.pool).await.unwrap();
        let session = store::get_session(&state.pool, created.id).await.unwrap().unwrap();
        let trade = |entry: &str| crate::backtest::Trade {
            ticker: "BTCUSDT".into(), entry_ts: entry.into(), exit_ts: "2026-11-08T10:05:00Z".into(), entry_price: 100.0, exit_price: 99.0,
            qty: 0.004, entries: 1, direction: "long".into(), exit_reason: "exit_signal".into(), pnl: -0.004, fees: 0.0, return_pct: -1.0, bars_held: 2, mae: 0.0, mfe: 0.0,
        };
        let (before, after) = (trade("2026-11-08T10:00:00Z"), trade("2026-11-08T10:01:00Z"));
        let mut fresh = Vec::new();
        for t in [&before, &after] {
            let key = format!("BTCUSDT|long|{}", t.entry_ts);
            store::upsert_trade(&state.pool, created.id, &key, "BTCUSDT", "long", &t.entry_ts, &t.exit_ts, false, t.pnl, 0.0, &serde_json::to_value(t).unwrap()).await.unwrap();
            fresh.push((key, t));
        }
        crate::robots::take(&state, &session, &fresh).await.unwrap();
        let mut rows = store::list_trades(&state.pool, created.id, false).await.unwrap();
        rows.sort_by(|a, b| a.entry_ts.cmp(&b.entry_ts));
        assert_eq!(rows[0].robot_qty, Some(Decimal::ZERO));
        assert!(rows[0].robot_note.contains("not armed"), "{}", rows[0].robot_note);
        assert_eq!(rows[1].robot_qty, Some(d("0.004")));
        assert_eq!(rows[1].robot_note, "");
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
        sqlx::query("UPDATE robots SET armed_at = now() - interval '5 minutes' WHERE id = $1").bind(s.id).execute(&state.pool).await.unwrap();
        otw_store::broker_risk::add_halt(&state.pool, Some(acct.id), "kill_switch", "", None).await.unwrap();
        decide_entry(&state, s.id).await;
        sync(&state, s.id).await.unwrap();
        assert_eq!(v.posts(), 0);
        let s = store::get_session(&state.pool, s.id).await.unwrap().unwrap();
        assert_eq!(s.exec_state, "paused");
        assert!(s.exec_note.contains("kill switch"), "{}", s.exec_note);
    }

    /// An exclusion period holds the entry the close decided, like a guard: nothing is sent,
    /// the robot says why. Once the period is gone the same entry leaves. The robot's next
    /// tick is the close of the bar in progress on its own timeframe (1m here).
    #[tokio::test]
    async fn an_exclusion_holds_the_entry_and_the_robot_ticks_on_its_close() {
        let Some(state) = scratch_state().await else { return };
        let server = MockServer::start().await;
        let v = Venue::default();
        mock_venue(&server, &v).await;
        crate::brokers::serve_from(&server.uri());
        let acct = account(&state, "excl").await;
        let s = session(&state, &acct, "excl").await.unwrap_or_else(|e| panic!("{}", e.message));

        let next = crate::robots::next_tick(&state, &s).await.unwrap();
        let now = OffsetDateTime::now_utc();
        assert_eq!(next.unix_timestamp() % 60, 0, "on the minute close");
        assert!(next > now && next - now <= time::Duration::minutes(1), "the bar in progress closes next");

        exec_store::arm(&state.pool, s.id, Uuid::nil(), &json!({ "BTCUSDT": "0" })).await.unwrap();
        sqlx::query("UPDATE robots SET armed_at = now() - interval '5 minutes', exclusions = '{\"weekdays\": 127}' WHERE id = $1")
            .bind(s.id)
            .execute(&state.pool)
            .await
            .unwrap();
        decide_entry(&state, s.id).await;
        sync(&state, s.id).await.unwrap();
        assert_eq!(v.posts(), 0, "no entry inside an exclusion");
        let held = store::get_session(&state.pool, s.id).await.unwrap().unwrap();
        assert_eq!(held.exec_state, "paused");
        assert!(held.exec_note.contains("excluded on"), "{}", held.exec_note);

        sqlx::query("UPDATE robots SET exclusions = '{}' WHERE id = $1").bind(s.id).execute(&state.pool).await.unwrap();
        // The refusal was recorded for that close: a new decision after the period goes out.
        sqlx::query("DELETE FROM robot_exec_orders WHERE session_id = $1").bind(s.id).execute(&state.pool).await.unwrap();
        sync(&state, s.id).await.unwrap();
        assert_eq!(v.posts(), 1, "the entry leaves once the period is over");
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
        sqlx::query("UPDATE robots SET armed_at = now() - interval '5 minutes' WHERE id = $1").bind(s.id).execute(&state.pool).await.unwrap();
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
        let ex = vet(&state, Some(&raw), &s, &[dataset]).await.unwrap_or_else(|e| panic!("{}", e.message)).unwrap();
        assert_eq!(ex.exits, "native");
        let input: store::SessionInput = serde_json::from_value(json!({
            "name": "paper e2e", "settings": strategy, "dataset_ids": [dataset], "kind": "interval", "timezone": "UTC",
            "every_minutes": 1, "status": "active", "feed": "hist", "notify_when": "on_change", "cadence": "each"
        }))
        .unwrap();
        let created = store::create_session(&state.pool, &input, None, None).await.unwrap();
        exec_store::attach(&state.pool, created.id, acct.id, &serde_json::to_value(&ex).unwrap()).await.unwrap();
        let id = created.id;
        let session = store::get_session(&state.pool, id).await.unwrap().unwrap();
        exec_store::arm(&state.pool, id, Uuid::nil(), &baseline(&state, &session).await.unwrap()).await.unwrap();
        sqlx::query("UPDATE robots SET armed_at = now() - interval '5 minutes' WHERE id = $1").bind(id).execute(&state.pool).await.unwrap();
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

/// NQ, 2026-11-17/18: entries refused the day before held no trade of their own, and each
/// showed the one of 18:52 as its simulated price. A row reads its own trade's frozen prices,
/// else its engine price; a same-side signal while the position is open does not move them.
#[test]
fn each_entry_reads_the_simulated_prices_of_its_own_trade() {
    let mut refused = sol_order("r1", "entry", "buy", "entry|SOLUSDT|long|2026-11-17T12:01:00Z", "", "0", "0");
    refused.engine_price = Some(73.10);
    let mut taken = sol_order("e1", "entry", "buy", "entry|SOLUSDT|long|2026-11-18T18:51:00Z", "", "1", "1");
    taken.engine_price = Some(72.50);
    let mut again = sol_order("e2", "entry", "buy", "entry|SOLUSDT|long|2026-11-18T18:53:00Z", "", "0", "0");
    again.engine_price = Some(72.90);
    let orders = [refused, taken, again];
    let mut t = sol_trade("long", "2026-11-18T18:52:00Z", true, "1");
    t.trade = json!({ "qty": 1.0, "entry_price": 72.80, "sim_entry": 72.75 });
    let fills = [sol_fill("e1", "buy", "1", "72.70", "2026-11-18T18:52:01Z")];
    let rows = compare(&orders, &fills, std::slice::from_ref(&t));
    let sim = |id: &str| rows.iter().find(|r| r["client_id"] == id).and_then(|r| r["sim_entry"].as_f64());
    assert_eq!((sim("r1"), sim("e1"), sim("e2")), (Some(73.10), Some(72.75), Some(72.90)));
    assert!(rows.iter().find(|r| r["client_id"] == "e1").unwrap()["sim_exit"].is_null(), "no exit while open");
}
