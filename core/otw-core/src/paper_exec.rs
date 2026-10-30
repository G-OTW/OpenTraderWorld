//! Strategies on a broker account: a paper session whose fills happen at a broker.
//!
//! **The engine decides, the broker executes, a sync converges.** A session that executes
//! keeps ticking exactly like a simulated one ([`crate::paper`]): the ordinary backtest re-run
//! over the moving window, its trades diffed into the paper book. After each tick, after a level
//! the live watch ([`crate::paper_live`]) saw crossed, and on a cycle, [`sync`] reads what the
//! engine intends (the position it holds, the market entry its last close decided, the limits it
//! rests) and what the account holds (the session's own orders looked up by client id, the
//! venue's position on the instrument), and sends the difference through the server-side order
//! sender (`brokers::dispatch::send_internal`, `Source::Strategy`), with every gate the chart's
//! orders meet and the account's background trading permission in place of the browser unlock:
//!
//! - an entry the engine decided at this close is sent at once, as a market order, sized from the
//!   engine's own size and cut down to the venue's step (never up), refused when over the
//!   session's maximum: never resized to fit;
//! - a resting limit the engine holds and the broker does not have is placed, one the broker has
//!   and the engine dropped is cancelled;
//! - the stop and the target become native exits (attached to the entry, one-cancels-other)
//!   where the broker's order capabilities hold them (`exits = native`); otherwise the live
//!   watch keeps checking the levels on the live price and a crossing makes the engine flat,
//!   which the sync answers with a market close (`exits = watch`: an emulation, said so);
//! - a trailing stop, or any level the engine moved at a close, is a modify of the working leg;
//! - a position the engine no longer holds is closed at the market.
//!
//! **The venue is the truth.** The session's position is the sum of what its own orders filled,
//! as the broker reports them by client id; the account's position on the instrument is read
//! beside it, less what it held when the session was armed (`exec_baseline`). Any difference
//! (an external order, a partial fill left over) puts the session in `degraded`: it adds no
//! exposure, keeps managing its exits, and says so on its channels until the user flattens or
//! adopts the difference.
//!
//! **Never resend.** Each decision is an intent (`entry|<ticker>|<side>|<signal bar>`, the
//! exits of a position, a close) written before anything leaves (`paper_exec_orders`, unique
//! per session), its client id derived from it, so a restart, a second sync or a tick that
//! re-runs the same close never sends it twice. An order whose answer was lost (`Unknown`)
//! blocks the instrument (`broker_orders`), cancels the session's own resting entries there and
//! tells the user to check at the broker.
//!
//! `simulate` sessions never reach any of this: [`sync`] answers at once for a session with no
//! broker target.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use otw_connector_model::{Decimal, OrderType, Side, TifKind};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::Digest;
use time::OffsetDateTime;
use tokio::sync::Notify;
use uuid::Uuid;

use otw_store::paper as store;
use otw_store::paper_exec as exec_store;

use crate::backtest::{self, Settings};
use crate::brokers::dispatch::{self, SendRefusal, Source};
use crate::brokers::{self, Broker, Lookup, OrderLeg, OrderTicket};
use crate::AppState;

/// The module a strategy sends as: the account must be granted to it.
pub const MODULE: &str = "backtest";
/// The sync cycle, on top of the poke every tick and every live fill sends.
const CYCLE: Duration = Duration::from_secs(15);
/// Before the first cycle: the gateway sessions and the live hub come up first.
const WARMUP: Duration = Duration::from_secs(25);
/// Tries at placing a position's exits, and at closing it, before the user is asked to act.
const MAX_EXIT_TRIES: usize = 2;
const MAX_CLOSE_TRIES: usize = 3;
/// Fills are read from a little before the last one, so none falls between two reads.
const FILL_OVERLAP: time::Duration = time::Duration::minutes(10);

static POKE: Notify = Notify::const_new();
/// One sync at a time, process-wide: a tick, a live fill and the cycle never interleave.
static SYNC: LazyLock<tokio::sync::Mutex<()>> = LazyLock::new(|| tokio::sync::Mutex::new(()));
/// Changes a venue refused, so a refused modify is not asked again every cycle.
static REFUSED_MOVES: LazyLock<Mutex<HashMap<String, Decimal>>> = LazyLock::new(Default::default);

/// Something changed for a session (a tick, a live fill): sync now.
pub fn poke() {
    POKE.notify_one();
}

// ── The target ───────────────────────────────────────────────────────────────

/// Where a session's fills happen. Stored on the session at creation, never edited.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Execution {
    /// `simulate` | `broker`
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub account_id: Option<Uuid>,
    /// The account's name when the session was created, for a page to show.
    #[serde(default)]
    pub account: String,
    #[serde(default)]
    pub broker: String,
    /// `<provider>:<ticker>` of each dataset, to the broker's spelling of that instrument, as
    /// the account's own lookup answered it. Never matched by name.
    #[serde(default)]
    pub symbols: BTreeMap<String, String>,
    /// `native` | `watch`
    #[serde(default)]
    pub exits: String,
    /// The largest entry the session may send, in the instrument's units. Over it, an entry is
    /// refused, never cut down.
    #[serde(default)]
    pub max_position_qty: Option<Decimal>,
}

impl Execution {
    pub fn of(session: &store::Session) -> Option<Execution> {
        let e: Execution = serde_json::from_value(session.execution.clone()?).ok()?;
        (e.target == "broker" && e.account_id.is_some()).then_some(e)
    }
}

/// The dataset key the symbol map is keyed on.
pub fn key_of(provider: &str, ticker: &str) -> String {
    format!("{provider}:{ticker}")
}

// ── What the engine intends, read from its last tick ────────────────────────

/// A position the engine holds, or enters at this close, on one instrument.
#[derive(Debug, Clone, PartialEq)]
pub struct Hold {
    pub long: bool,
    pub stop: Option<f64>,
    pub target: Option<f64>,
}

/// A market entry the last close decided.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub long: bool,
    pub qty: f64,
    /// The close it was decided on: the reference price of the order.
    pub price: f64,
    pub signal_ts: String,
    /// When that close happened: an entry decided before the session was armed is not sent.
    pub decided_at: OffsetDateTime,
    pub stop: Option<f64>,
    pub target: Option<f64>,
}

/// A limit entry the engine rests.
#[derive(Debug, Clone, PartialEq)]
pub struct Limit {
    pub long: bool,
    pub qty: f64,
    pub price: f64,
    /// Set on the close that decided it, and only then.
    pub signal_ts: Option<String>,
    pub decided_at: Option<OffsetDateTime>,
    pub stop: Option<f64>,
    pub target: Option<f64>,
}

/// The engine's book on one instrument.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Intended {
    pub hold: Option<Hold>,
    pub entry: Option<Entry>,
    pub limits: Vec<Limit>,
}

fn rfc(ts: &str) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(ts, &time::format_description::well_known::Rfc3339).ok()
}

/// What the last tick left for `ticker`, the way the live watch reads it: the open positions
/// (minus those a live exit already closed), the market entry the last close decided (a
/// position from the moment it was decided, with the levels the engine gives that price), and
/// the resting limit entries. `tf_secs`: the dataset's bar, to tell when a close happened.
pub fn intended(stats: Option<&Value>, ticker: &str, live: &[store::LiveFill], settings: &Settings, tf_secs: i64) -> Intended {
    let mut out = Intended::default();
    let Some(stats) = stats else { return out };
    let num = |v: &Value, k: &str| v.get(k).and_then(Value::as_f64);
    let text = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    let mine = |v: &Value| v.get("ticker").and_then(Value::as_str).is_none_or(|t| t == ticker);
    let exited = |key: &str| live.iter().any(|f| f.kind == "exit" && f.trade_key == key);
    for p in stats.get("open_positions").and_then(Value::as_array).into_iter().flatten().filter(|p| mine(p)) {
        let direction = text(p, "direction");
        if exited(&format!("{ticker}|{direction}|{}", text(p, "entry_ts"))) {
            continue;
        }
        out.hold = Some(Hold { long: direction == "long", stop: num(p, "stop"), target: num(p, "take_profit").or_else(|| num(p, "exit_limit")) });
        break;
    }
    for o in stats.get("pending_orders").and_then(Value::as_array).into_iter().flatten().filter(|o| mine(o)) {
        if text(o, "kind") != "entry" {
            continue;
        }
        let long = text(o, "direction") == "long";
        let (qty, price, atr) = (num(o, "qty").unwrap_or(0.0), num(o, "price").unwrap_or(0.0), num(o, "atr"));
        let signal = o.get("signal_ts").and_then(Value::as_str).map(String::from);
        let decided_at = signal.as_deref().and_then(rfc).map(|t| t + time::Duration::seconds(tf_secs));
        if text(o, "order") == "market" {
            let (Some(signal_ts), Some(decided_at)) = (signal, decided_at) else { continue };
            // Filled on the live price and closed there already: nothing is held from it.
            let direction = if long { "long" } else { "short" };
            let filled = live.iter().find(|f| f.kind == "entry" && f.reason == "market" && f.ticker == ticker && f.direction == direction && f.at >= decided_at);
            if filled.is_some_and(|f| exited(&f.trade_key)) {
                continue;
            }
            let at = filled.map_or(price, |f| f.price);
            let (stop, target) = backtest::fresh_levels(settings, long, at, atr);
            let (stop, target) = (stop.map(|(s, _)| s), target);
            out.entry = Some(Entry { long, qty, price, signal_ts, decided_at, stop, target });
            if out.hold.is_none() {
                out.hold = Some(Hold { long, stop, target });
            }
            continue;
        }
        if o.get("provisional").and_then(Value::as_bool) == Some(true) || price <= 0.0 {
            continue;
        }
        let (stop, target) = backtest::fresh_levels(settings, long, price, atr);
        out.limits.push(Limit { long, qty, price, signal_ts: signal, decided_at, stop: stop.map(|(s, _)| s), target });
    }
    out
}

// ── What the account holds ──────────────────────────────────────────────────

/// What one of the session's legs is, for the sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    Entry,
    Limit,
    Stop,
    Target,
    Close,
}

impl Role {
    pub fn word(self) -> &'static str {
        match self {
            Role::Entry => "entry",
            Role::Limit => "limit",
            Role::Stop => "stop",
            Role::Target => "target",
            Role::Close => "close",
        }
    }

    fn of(word: &str) -> Option<Role> {
        Some(match word {
            "entry" => Role::Entry,
            "limit" => Role::Limit,
            "stop" => Role::Stop,
            "target" => Role::Target,
            "close" => Role::Close,
            _ => return None,
        })
    }

    fn letter(self) -> char {
        match self {
            Role::Entry => 'e',
            Role::Limit => 'l',
            Role::Stop => 's',
            Role::Target => 't',
            Role::Close => 'c',
        }
    }

    fn opens(self) -> bool {
        matches!(self, Role::Entry | Role::Limit)
    }
}

/// One leg the session sent, as the broker last answered for it.
#[derive(Debug, Clone, PartialEq)]
pub struct Leg {
    pub client_id: String,
    pub intent: String,
    pub role: Role,
    pub buy: bool,
    pub qty: Decimal,
    pub limit: Option<Decimal>,
    pub stop: Option<Decimal>,
    pub engine_price: Option<f64>,
    /// The entry (its client id) an exit or a close belongs to.
    pub position: String,
    /// Still working at the broker (accepted, not filled, cancelled, rejected nor expired).
    pub working: bool,
    /// Filled so far, as the broker says.
    pub filled: Decimal,
    /// The broker said it stopped working.
    pub done: bool,
    /// The sender refused it: nothing left.
    pub refused: bool,
}

/// The account's side of one instrument.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Actual {
    /// Signed size the account holds now; `None` when it could not be read steadily.
    pub held: Option<Decimal>,
    /// What it held outside the strategy when the session was armed.
    pub baseline: Decimal,
    pub legs: Vec<Leg>,
    /// Entries whose partial fill the user adopted as the position.
    pub adopted: Vec<String>,
}

impl Actual {
    /// The session's own position: what its legs filled, buys up and sells down.
    pub fn own(&self) -> Decimal {
        self.legs.iter().map(|l| if l.buy { l.filled } else { -l.filled }).sum()
    }
}

/// What the sync may do on one instrument, and with what.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Rules {
    pub armed_at: Option<OffsetDateTime>,
    /// The session is active (a paused session sends nothing).
    pub active: bool,
    /// A halt on the account: only orders that take risk off go out.
    pub halted: Option<String>,
    /// An order on the instrument has no known outcome: nothing goes out on it.
    pub blocked: bool,
    pub native: bool,
    pub brackets: bool,
    pub oca: bool,
    pub amend: bool,
    pub reduce_only: bool,
    pub step: Option<Decimal>,
    pub tick: Option<Decimal>,
    pub max_qty: Option<Decimal>,
}

// ── The sync, pure ──────────────────────────────────────────────────────────

/// One leg to send.
#[derive(Debug, Clone, PartialEq)]
pub struct LegSpec {
    pub client_id: String,
    pub intent: String,
    pub role: Role,
    pub buy: bool,
    pub kind: OrderType,
    pub qty: Decimal,
    pub limit: Option<Decimal>,
    pub stop: Option<Decimal>,
    pub parent: Option<String>,
    pub group: Option<String>,
    pub reduce_only: bool,
    pub engine_qty: Option<f64>,
    pub engine_price: Option<f64>,
    pub position: String,
}

/// What the sync does on one instrument.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// One ticket: an entry with its attached exits, a limit, a position's exits, a close.
    Place(Vec<LegSpec>),
    Cancel { client_id: String },
    /// Move a working leg to the engine's level.
    Modify { client_id: String, limit: Option<Decimal>, stop: Option<Decimal>, engine_price: f64 },
}

/// The sync's decision on one instrument.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Plan {
    pub actions: Vec<Action>,
    /// Why the session adds no exposure here, when the account holds what its fills do not
    /// explain.
    pub degraded: Option<String>,
    /// What was not done and why, in one sentence each.
    pub notes: Vec<String>,
    pub own: Decimal,
    pub drift: Option<Decimal>,
}

/// The client id of an intent: derived from the session and the decision, so the same
/// decision always carries the same id and the broker's own duplicate guard holds too.
/// 24 characters of `[a-z0-9]`, inside every venue's client id rules.
pub fn client_id(session: Uuid, intent: &str, role: Role) -> String {
    let mut h = sha2::Sha256::new();
    h.update(session.as_bytes());
    h.update(intent.as_bytes());
    let hex: String = h.finalize().iter().take(10).map(|b| format!("{b:02x}")).collect();
    format!("ots{}{hex}", role.letter())
}

/// The engine's size as an order quantity: its decimal reading, cut down to the venue's step
/// (never rounded up). `None` when nothing is left.
pub fn qty_of(engine: f64, step: Option<Decimal>) -> Option<Decimal> {
    let q = otw_store::paper_broker::decimal_of(engine.abs()).ok()?;
    let q = match step.filter(|s| *s > Decimal::ZERO) {
        Some(s) => (q / s).floor() * s,
        None => q.trunc_with_scale(8),
    };
    (q > Decimal::ZERO).then(|| q.normalize())
}

/// A level as an order price: its decimal reading, on the instrument's tick when it states one.
pub fn px_of(level: f64, tick: Option<Decimal>) -> Option<Decimal> {
    if !(level.is_finite() && level > 0.0) {
        return None;
    }
    let p = otw_store::paper_broker::decimal_of(level).ok()?;
    let p = match tick.filter(|t| *t > Decimal::ZERO) {
        Some(t) => (p / t).round() * t,
        None => p.round_dp(10),
    };
    (p > Decimal::ZERO).then(|| p.normalize())
}

fn same_price(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
}

fn dir(long: bool) -> &'static str {
    if long { "long" } else { "short" }
}

/// Converge one instrument: what the engine intends against what the account holds. Pure: the
/// caller reads both sides and sends what comes back, in order.
pub fn plan(session: Uuid, ticker: &str, eng: &Intended, act: &Actual, r: &Rules) -> Plan {
    let mut p = Plan { own: act.own(), ..Default::default() };
    let own = p.own;
    if let Some(held) = act.held {
        let drift = held - act.baseline - own;
        p.drift = Some(drift);
        if !drift.is_zero() {
            p.degraded = Some(format!(
                "the account holds {} {ticker} that this strategy's orders do not explain: it adds no exposure until you flatten or adopt it",
                drift.normalize()
            ));
        }
    }
    let entry_working = act.legs.iter().any(|l| l.working && l.role.opens());
    if p.degraded.is_none() && !own.is_zero() && !entry_working {
        let last = act.legs.iter().rev().find(|l| l.role.opens() && l.filled > Decimal::ZERO);
        if let Some(l) = last.filter(|l| l.done && l.filled < l.qty && !act.adopted.contains(&l.client_id)) {
            p.degraded = Some(format!(
                "the entry filled {} of {} {ticker}: it adds no exposure until you flatten or adopt the remainder",
                l.filled.normalize(),
                l.qty.normalize()
            ));
        }
    }
    if r.armed_at.is_none() || !r.active {
        return p;
    }
    if r.blocked {
        p.notes.push(format!("an order on {ticker} got no answer from the broker: nothing is sent on it until it is settled. Check it at the broker."));
        return p;
    }
    let why_not = p.degraded.clone().or_else(|| r.halted.clone());
    let position = act.legs.iter().rev().find(|l| l.role.opens() && l.filled > Decimal::ZERO).map(|l| l.client_id.clone()).unwrap_or_default();
    let working = |role: Role| act.legs.iter().filter(move |l| l.working && l.role == role);

    if !own.is_zero() {
        let own_long = own > Decimal::ZERO;
        let engine_side = eng.hold.as_ref().map(|h| h.long);
        if engine_side != Some(own_long) {
            // The engine is flat, or on the other side: what the session holds goes.
            for l in act.legs.iter().filter(|l| l.working && l.role != Role::Close) {
                p.actions.push(Action::Cancel { client_id: l.client_id.clone() });
            }
            if working(Role::Close).next().is_none() {
                let tries = act.legs.iter().filter(|l| l.role == Role::Close && l.position == position).count();
                if tries < MAX_CLOSE_TRIES {
                    let intent = format!("close|{position}|{tries}");
                    p.actions.push(Action::Place(vec![LegSpec {
                        client_id: client_id(session, &intent, Role::Close),
                        intent,
                        role: Role::Close,
                        buy: !own_long,
                        kind: OrderType::Market,
                        qty: own.abs(),
                        limit: None,
                        stop: None,
                        parent: None,
                        group: None,
                        reduce_only: r.reduce_only,
                        engine_qty: None,
                        engine_price: None,
                        position: position.clone(),
                    }]));
                } else {
                    p.notes.push(format!("the strategy closed its {} {ticker} position and the broker refused the close {tries} times: close it at the broker", dir(own_long)));
                }
            }
            // Stop and reverse: the entry on the other side, decided at this close.
            if let Some(e) = eng.entry.as_ref().filter(|e| Some(e.long) == engine_side) {
                enter(&mut p, session, ticker, e, act, r, why_not.as_deref());
            }
        } else {
            // A resting entry while the position is held would be an add: none is sent.
            for l in working(Role::Limit) {
                p.actions.push(Action::Cancel { client_id: l.client_id.clone() });
            }
            if r.native {
                if let Some(h) = &eng.hold {
                    exits(&mut p, session, ticker, h, own, &position, act, r);
                }
            }
        }
        return p;
    }

    // Flat: exits left over from a position that is gone, then the entries.
    if !entry_working {
        for l in act.legs.iter().filter(|l| l.working && matches!(l.role, Role::Stop | Role::Target | Role::Close)) {
            p.actions.push(Action::Cancel { client_id: l.client_id.clone() });
        }
    }
    if let Some(e) = &eng.entry {
        enter(&mut p, session, ticker, e, act, r, why_not.as_deref());
    }
    // Resting limits: the engine's against the session's.
    for l in working(Role::Limit) {
        let kept = eng.limits.iter().any(|x| x.long == l.buy && l.engine_price.is_some_and(|p| same_price(p, x.price)));
        if !kept {
            p.actions.push(Action::Cancel { client_id: l.client_id.clone() });
        }
    }
    for x in &eng.limits {
        let resting = act.legs.iter().any(|l| l.working && l.role == Role::Limit && l.buy == x.long && l.engine_price.is_some_and(|p| same_price(p, x.price)));
        let (Some(signal), Some(at)) = (&x.signal_ts, x.decided_at) else { continue };
        let intent = format!("limit|{ticker}|{}|{signal}", dir(x.long));
        if resting || act.legs.iter().any(|l| l.intent == intent) || r.armed_at.is_none_or(|a| at < a) {
            continue;
        }
        if let Some(why) = &why_not {
            p.notes.push(format!("the {} limit on {ticker} at {} was not placed: {why}", dir(x.long), x.price));
            continue;
        }
        let Some(qty) = sized(&mut p, ticker, x.qty, r) else { continue };
        let Some(price) = px_of(x.price, r.tick) else { continue };
        let id = client_id(session, &intent, Role::Limit);
        let mut legs = vec![LegSpec {
            client_id: id.clone(),
            intent: intent.clone(),
            role: Role::Limit,
            buy: x.long,
            kind: OrderType::Limit,
            qty,
            limit: Some(price),
            stop: None,
            parent: None,
            group: None,
            reduce_only: false,
            engine_qty: Some(x.qty),
            engine_price: Some(x.price),
            position: String::new(),
        }];
        if r.native && r.brackets {
            legs.extend(bracket(session, &intent, &id, x.long, qty, x.stop, x.target, r));
        }
        p.actions.push(Action::Place(legs));
    }
    p
}

/// The size to send, or why none is: under the venue's step, or over the session's maximum.
fn sized(p: &mut Plan, ticker: &str, engine: f64, r: &Rules) -> Option<Decimal> {
    let Some(qty) = qty_of(engine, r.step) else {
        p.notes.push(format!("the strategy's size {engine} on {ticker} is under the venue's step: nothing was sent"));
        return None;
    };
    if let Some(max) = r.max_qty.filter(|m| qty > *m) {
        p.notes.push(format!("the strategy's size {qty} on {ticker} is over this session's maximum of {max}: nothing was sent. Raise the maximum or size the strategy down"));
        return None;
    }
    Some(qty)
}

/// The exits attached to an entry: a stop and a target on the other side, one-cancels-other.
#[allow(clippy::too_many_arguments)]
fn bracket(session: Uuid, intent: &str, parent: &str, long: bool, qty: Decimal, stop: Option<f64>, target: Option<f64>, r: &Rules) -> Vec<LegSpec> {
    let stop = stop.and_then(|s| px_of(s, r.tick)).map(|px| (px, stop.unwrap_or_default()));
    let target = target.and_then(|t| px_of(t, r.tick)).map(|px| (px, target.unwrap_or_default()));
    let group = (stop.is_some() && target.is_some()).then(|| format!("{parent}g"));
    let mut out = Vec::new();
    if let Some((px, level)) = stop {
        let intent = format!("{intent}|stop");
        out.push(LegSpec {
            client_id: client_id(session, &intent, Role::Stop),
            intent,
            role: Role::Stop,
            buy: !long,
            kind: OrderType::Stop,
            qty,
            limit: None,
            stop: Some(px),
            parent: Some(parent.to_string()),
            group: group.clone(),
            reduce_only: r.reduce_only,
            engine_qty: None,
            engine_price: Some(level),
            position: parent.to_string(),
        });
    }
    if let Some((px, level)) = target {
        let intent = format!("{intent}|target");
        out.push(LegSpec {
            client_id: client_id(session, &intent, Role::Target),
            intent,
            role: Role::Target,
            buy: !long,
            kind: OrderType::Limit,
            qty,
            limit: Some(px),
            stop: None,
            parent: Some(parent.to_string()),
            group: group.clone(),
            reduce_only: r.reduce_only,
            engine_qty: None,
            engine_price: Some(level),
            position: parent.to_string(),
        });
    }
    out
}

/// A market entry decided at the last close, once, if the session may add exposure.
fn enter(p: &mut Plan, session: Uuid, ticker: &str, e: &Entry, act: &Actual, r: &Rules, why_not: Option<&str>) {
    let intent = format!("entry|{ticker}|{}|{}", dir(e.long), e.signal_ts);
    if act.legs.iter().any(|l| l.intent == intent) || r.armed_at.is_none_or(|a| e.decided_at < a) {
        return;
    }
    if let Some(why) = why_not {
        p.notes.push(format!("the {} entry on {ticker} decided at the close of {} was not sent: {why}", dir(e.long), e.signal_ts));
        return;
    }
    let Some(qty) = sized(p, ticker, e.qty, r) else { return };
    let id = client_id(session, &intent, Role::Entry);
    let mut legs = vec![LegSpec {
        client_id: id.clone(),
        intent: intent.clone(),
        role: Role::Entry,
        buy: e.long,
        kind: OrderType::Market,
        qty,
        limit: None,
        stop: None,
        parent: None,
        group: None,
        reduce_only: false,
        engine_qty: Some(e.qty),
        engine_price: Some(e.price),
        position: String::new(),
    }];
    if r.native && r.brackets {
        legs.extend(bracket(session, &intent, &id, e.long, qty, e.stop, e.target, r));
    }
    p.actions.push(Action::Place(legs));
}

/// The exits of the position the session holds: placed when missing, moved to the engine's
/// levels at each close (a trailing stop is this modify), cancelled when the engine drops one.
#[allow(clippy::too_many_arguments)]
fn exits(p: &mut Plan, session: Uuid, ticker: &str, h: &Hold, own: Decimal, position: &str, act: &Actual, r: &Rules) {
    let want_stop = h.stop.and_then(|s| px_of(s, r.tick));
    let want_target = h.target.and_then(|t| px_of(t, r.tick));
    let mine = |role: Role| act.legs.iter().filter(move |l| l.role == role && l.position == position);
    let stops: Vec<&Leg> = mine(Role::Stop).filter(|l| l.working).collect();
    let targets: Vec<&Leg> = mine(Role::Target).filter(|l| l.working).collect();
    if stops.is_empty() && targets.is_empty() {
        if want_stop.is_none() && want_target.is_none() {
            return;
        }
        let refused = mine(Role::Stop).chain(mine(Role::Target)).any(|l| l.refused);
        let tries: HashSet<&str> = mine(Role::Stop).chain(mine(Role::Target)).map(|l| l.intent.rsplit_once('|').map_or(l.intent.as_str(), |(head, _)| head)).collect();
        if refused || tries.len() >= MAX_EXIT_TRIES {
            p.notes.push(format!("the broker did not take the exits of the {ticker} position: its stop and target are watched on the live price and closed at the market"));
            return;
        }
        if want_stop.is_some() && want_target.is_some() && !r.oca {
            p.notes.push(format!("the broker cannot link a stop and a target on {ticker}: they are watched on the live price and closed at the market"));
            return;
        }
        let intent = format!("exits|{position}|{}", tries.len());
        let long = own > Decimal::ZERO;
        let mut legs = bracket(session, &intent, position, long, own.abs(), h.stop, h.target, r);
        for l in &mut legs {
            // Standalone: the entry has filled, nothing to hang on.
            l.parent = None;
            l.group = l.group.as_ref().map(|_| format!("{}g", client_id(session, &intent, Role::Stop)));
        }
        p.actions.push(Action::Place(legs));
        return;
    }
    for (legs, want, level, stop_leg) in [(stops, want_stop, h.stop, true), (targets, want_target, h.target, false)] {
        for l in legs {
            let now = if stop_leg { l.stop } else { l.limit };
            match want {
                Some(w) if now != Some(w) => {
                    if !r.amend {
                        p.notes.push(format!("the broker cannot move a working order on {ticker}: the {} stays at {}", if stop_leg { "stop" } else { "target" }, now.map(|d| d.to_string()).unwrap_or_default()));
                        continue;
                    }
                    let (limit, stop) = if stop_leg { (None, Some(w)) } else { (Some(w), None) };
                    p.actions.push(Action::Modify { client_id: l.client_id.clone(), limit, stop, engine_price: level.unwrap_or_default() });
                }
                Some(_) => {}
                None => p.actions.push(Action::Cancel { client_id: l.client_id.clone() }),
            }
        }
    }
}

// ── The sync, against the broker ────────────────────────────────────────────

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        tokio::time::sleep(WARMUP).await;
        loop {
            match exec_store::broker_sessions(&state.pool).await {
                Ok(ids) => {
                    for id in ids {
                        if let Err(e) = sync(&state, id).await {
                            tracing::warn!("paper exec: session {id}: {e:#}");
                        }
                    }
                }
                Err(e) => tracing::error!("paper exec: listing broker sessions: {e:#}"),
            }
            tokio::select! {
                _ = tokio::time::sleep(CYCLE) => {}
                _ = POKE.notified() => {}
            }
        }
    });
}

/// One instrument of a session, as the sync sees it.
struct Instrument {
    key: String,
    ticker: String,
    asset_type: String,
    provider: String,
    symbol: String,
    tf_secs: i64,
    connector: Option<Uuid>,
    last: Option<(f64, OffsetDateTime)>,
}

async fn instruments(state: &AppState, session: &store::Session, ex: &Execution) -> anyhow::Result<Vec<Instrument>> {
    use otw_store::{connectors, histdata as hd};
    let mut out = Vec::new();
    for &id in &session.dataset_ids {
        let ds = hd::get_dataset(&state.pool, id).await?.ok_or_else(|| anyhow::anyhow!("a dataset of this session no longer exists"))?;
        let key = key_of(&ds.provider, &ds.ticker);
        let symbol = ex.symbols.get(&key).cloned().ok_or_else(|| anyhow::anyhow!("{key} has no broker symbol: start the session again"))?;
        let tf_secs = crate::histdata::timeframe_secs(&ds.timeframe)?;
        let conn = match connectors::default_for_module(&state.pool, &ds.provider, MODULE).await? {
            Some(c) => Some(c),
            None => connectors::default_for(&state.pool, &ds.provider).await?,
        };
        let bars = hd::read_bars(&state.pool, id, None, None, 1).await.unwrap_or_default();
        let last = bars.last().map(|b| (b.close, b.ts + time::Duration::seconds(tf_secs)));
        out.push(Instrument { key, ticker: ds.ticker, asset_type: ds.asset_type, provider: ds.provider, symbol, tf_secs, connector: conn.map(|c| c.id), last });
    }
    Ok(out)
}

/// What the account holds of `symbol`, signed: the pair's base asset on a spot account, else
/// its positions on that symbol. Read from the venue, never derived.
async fn held_qty(connector: &dyn Broker, settings: &HashMap<String, String>, inst: &Instrument) -> anyhow::Result<Decimal> {
    if let Some(q) = connector.held_asset(settings, &inst.ticker).await? {
        return Ok(q);
    }
    let rows = connector.positions(&brokers::client()?, settings).await?;
    let mut total = Decimal::ZERO;
    for p in rows.iter().filter(|p| p.symbol.eq_ignore_ascii_case(&inst.symbol)) {
        let q = otw_store::paper_broker::decimal_of(p.qty.abs())?;
        total += if p.side == "short" { -q } else { q };
    }
    Ok(total)
}

/// The legs of one instrument as the sync reads them.
fn legs_of(rows: &[exec_store::Order], symbol: &str) -> Vec<Leg> {
    rows.iter()
        .filter(|o| o.symbol.eq_ignore_ascii_case(symbol))
        .filter_map(|o| {
            let role = Role::of(&o.role)?;
            let (status, filled) = status_of(o);
            Some(Leg {
                client_id: o.client_id.clone(),
                intent: o.intent.clone(),
                role,
                buy: o.side == "buy",
                qty: o.qty,
                limit: o.limit_price,
                stop: o.stop_price,
                engine_price: o.engine_price,
                position: o.position.clone(),
                working: matches!(o.outcome.as_str(), "sent" | "unknown" | "sending") && status == "working",
                filled,
                done: matches!(status.as_str(), "filled" | "cancelled" | "rejected" | "expired"),
                refused: o.outcome == "refused",
            })
        })
        .collect()
}

/// Where a leg stands as the broker last said, and how much of it filled. A leg not looked up
/// yet is taken as working; one the sender refused never left.
fn status_of(o: &exec_store::Order) -> (String, Decimal) {
    if o.outcome == "refused" {
        return ("rejected".into(), Decimal::ZERO);
    }
    (o.status.clone().unwrap_or_else(|| "working".into()), o.filled_qty)
}

/// Ask the broker where each live leg stands, by its client id (spec 6.3: the venue's answer
/// by the id the order was sent under, whatever the venue calls it).
async fn look_up(state: &AppState, connector: &dyn Broker, settings: &HashMap<String, String>, account: Uuid, rows: &[exec_store::Order], inst: &[Instrument]) -> Vec<String> {
    let mut problems = Vec::new();
    for o in rows.iter().filter(|o| matches!(o.outcome.as_str(), "sent" | "unknown")) {
        let (status, _) = status_of(o);
        if status != "working" {
            continue;
        }
        let Some(i) = inst.iter().find(|i| i.symbol.eq_ignore_ascii_case(&o.symbol)) else { continue };
        match connector.lookup(settings, &i.ticker, &i.asset_type, &o.client_id).await {
            Ok(Lookup::Found { id, status, filled_qty, fills_as }) => {
                let word = brokers::reconcile::word(status);
                if let Err(e) = exec_store::set_status(&state.pool, account, &o.client_id, word, filled_qty.normalize(), Some(&id), fills_as.as_deref()).await {
                    problems.push(format!("{e:#}"));
                }
            }
            Ok(Lookup::NotFound) => {}
            Err(e) => problems.push(format!("order {} could not be looked up: {e:#}", o.client_id)),
        }
    }
    problems
}

/// Record the broker's fills for the session's orders: the price and the fee each one dealt
/// at, beside the engine's. A fill is matched to an order by the broker's order id (the number
/// the lookup read where the history names that one), never by its content; a venue whose
/// history does not name the order leaves the real price unread.
async fn read_fills(state: &AppState, connector: &dyn Broker, settings: &HashMap<String, String>, session: Uuid, account: Uuid, rows: &[exec_store::Order], inst: &[Instrument]) -> anyhow::Result<()> {
    if rows.is_empty() || !connector.capability().executions && !connector.chart_fills() {
        return Ok(());
    }
    let known = exec_store::fills(&state.pool, session).await?;
    let first = rows.iter().map(|o| o.created_at).min().unwrap_or_else(OffsetDateTime::now_utc);
    let from = known.iter().map(|f| f.at).max().unwrap_or(first).max(first) - FILL_OVERLAP;
    let q = brokers::ExecQuery { from, to: OffsetDateTime::now_utc() + time::Duration::minutes(1), symbols: inst.iter().map(|i| i.symbol.clone()).collect() };
    let fills = connector.executions(&brokers::client()?, settings, &q).await?;
    for f in fills {
        let Some(o) = order_of(rows, &f.order_id) else { continue };
        let fill = exec_store::Fill {
            session_id: session,
            account_id: account,
            exec_id: f.id.clone(),
            client_id: o.client_id.clone(),
            symbol: o.symbol.clone(),
            side: if f.side == "sell" { "sell".into() } else { "buy".into() },
            qty: otw_store::paper_broker::decimal_of(f.qty)?,
            price: otw_store::paper_broker::decimal_of(f.price)?,
            fee: otw_store::paper_broker::decimal_of(f.fee).unwrap_or_default(),
            fee_currency: f.fee_currency.clone(),
            at: f.at,
        };
        exec_store::add_fill(&state.pool, &fill).await?;
    }
    Ok(())
}

/// The session's order a broker fill names, by the broker's order id: the id it is followed
/// by (whole, or past `SYMBOL:`), the number the lookup read, or its client id.
fn order_of<'a>(rows: &'a [exec_store::Order], order_id: &str) -> Option<&'a exec_store::Order> {
    let bare = |v: &str| v.rsplit(':').next().unwrap_or(v).to_string();
    rows.iter().find(|o| {
        o.venue_id.as_deref().is_some_and(|v| v == order_id || bare(v) == order_id) || o.fills_as.as_deref() == Some(order_id) || o.client_id == order_id
    })
}

/// Legs left `sending` by a process that stopped mid-call: those the order store has went out
/// and are followed from now on; the others never left.
async fn settle_sending(state: &AppState, session: Uuid, account: Uuid) -> anyhow::Result<()> {
    for o in exec_store::sending(&state.pool, session).await? {
        match otw_store::broker_orders::get(&state.pool, account, &o.client_id).await? {
            Some(leg) => {
                let outcome = if leg.status == "unknown" || leg.status == "pending" { "unknown" } else { "sent" };
                exec_store::set_outcome(&state.pool, account, &o.client_id, outcome, None, leg.venue_id.as_deref()).await?;
            }
            None => exec_store::set_outcome(&state.pool, account, &o.client_id, "refused", Some("the app stopped before it was sent"), None).await?,
        }
    }
    Ok(())
}

/// The time in force a leg asks for: Day where the venue takes it, else good till cancelled,
/// for a market order; good till cancelled for anything that rests.
fn tif_for(caps: Option<&otw_connector_model::OrderCaps>, rests: bool) -> Option<TifKind> {
    let c = caps?;
    let order = if rests { [TifKind::Gtc, TifKind::Day] } else { [TifKind::Day, TifKind::Gtc] };
    order.into_iter().find(|t| c.tif.contains(t)).or_else(|| c.tif.iter().next().copied())
}

fn ticket_of(inst: &Instrument, legs: &[LegSpec], caps: Option<&otw_connector_model::OrderCaps>, reference: (Decimal, i64)) -> OrderTicket {
    let legs = legs
        .iter()
        .map(|l| OrderLeg {
            tif: tif_for(caps, l.kind != OrderType::Market),
            limit: l.limit,
            stop: l.stop,
            parent: l.parent.clone(),
            group: l.group.clone(),
            reduce_only: l.reduce_only,
            ..OrderLeg::new(l.client_id.clone(), if l.buy { Side::Buy } else { Side::Sell }, l.kind, l.qty)
        })
        .collect();
    OrderTicket {
        ticker: inst.ticker.clone(),
        asset_type: inst.asset_type.clone(),
        legs,
        provider: Some(inst.provider.clone()),
        connector: inst.connector,
        reference: Some(reference.0),
        reference_at: Some(reference.1),
        confirm_duplicate: false,
    }
}

/// The price an order is sent against: the engine's for an entry (the close it decided on, and
/// when that close happened), else the last stored close.
fn reference_for(legs: &[LegSpec], inst: &Instrument) -> Option<(Decimal, i64)> {
    let ms = |t: OffsetDateTime| (t.unix_timestamp_nanos() / 1_000_000) as i64;
    let (px, at) = inst.last?;
    let px = legs.iter().find(|l| l.role.opens()).and_then(|l| l.engine_price).filter(|p| *p > 0.0).unwrap_or(px);
    Some((otw_store::paper_broker::decimal_of(px).ok()?, ms(at)))
}

/// Say something about the session on its log and its channels, once per distinct note.
async fn say(state: &AppState, session: &store::Session, title: &str, message: &str) {
    let payload = json!({ "execution": true });
    let title = format!("{}: {title}", session.name);
    if let Err(e) = store::add_event(&state.pool, session.id, "broker", &payload, &title, message, true).await {
        tracing::error!("paper exec: recording a notice: {e:#}");
        return;
    }
    crate::paper::deliver_events(state, session).await;
}

/// Sync one session: read both books, converge, record what was found. A session with no
/// broker target answers at once.
pub async fn sync(state: &AppState, id: Uuid) -> anyhow::Result<()> {
    let _one = SYNC.lock().await;
    let Some(session) = store::get_session(&state.pool, id).await? else { return Ok(()) };
    let Some(ex) = Execution::of(&session) else { return Ok(()) };
    let Some(account_id) = ex.account_id else { return Ok(()) };
    let rows = exec_store::orders(&state.pool, id).await?;
    let armed = session.armed_at.is_some();
    // A disarmed session with nothing working and nothing held has nothing at the broker to
    // read: it is not asked again every cycle.
    let quiet = rows.iter().all(|o| {
        let (status, _) = status_of(o);
        o.outcome == "refused" || (o.outcome == "sent" && status != "working")
    });
    let mut own: HashMap<&str, Decimal> = HashMap::new();
    for o in &rows {
        *own.entry(o.symbol.as_str()).or_default() += if o.side == "buy" { o.filled_qty } else { -o.filled_qty };
    }
    if !armed && quiet && own.values().all(|q| q.is_zero()) {
        exec_store::set_state(&state.pool, id, "idle", "", None).await?;
        return Ok(());
    }
    let resolved = crate::brokers_api::resolve_for_job(&state.pool, &state.cipher, account_id, MODULE).await;
    let (connector, settings, _account) = match resolved {
        Ok(r) => r,
        Err(e) => return note_state(state, &session, "error", &format!("the broker account cannot be used: {e:#}"), None).await,
    };
    settle_sending(state, id, account_id).await?;
    let inst = match instruments(state, &session, &ex).await {
        Ok(i) => i,
        Err(e) => return note_state(state, &session, "error", &format!("{e:#}"), None).await,
    };
    let rows = exec_store::orders(&state.pool, id).await?;
    // The account, read around the lookups: a position that moved between the two reads is
    // in motion, and no drift is measured on it this time.
    let mut first = HashMap::new();
    for i in &inst {
        match held_qty(connector.as_ref(), &settings, i).await {
            Ok(q) => {
                first.insert(i.symbol.clone(), q);
            }
            Err(e) => return note_state(state, &session, "error", &format!("the account could not be read ({e:#}): nothing is sent until it can"), None).await,
        }
    }
    let problems = look_up(state, connector.as_ref(), &settings, account_id, &rows, &inst).await;
    // Read again before the fills: the lookups just wrote the number a fill may be named by.
    let rows = exec_store::orders(&state.pool, id).await?;
    if let Err(e) = read_fills(state, connector.as_ref(), &settings, id, account_id, &rows, &inst).await {
        tracing::warn!("paper exec: reading fills of {}: {e:#}", session.name);
    }
    let settings_bt: Option<Settings> = serde_json::from_value(session.settings.clone()).ok();
    let live = store::list_live_fills(&state.pool, id).await.unwrap_or_default();
    let halts = otw_store::broker_risk::halts(&state.pool, Some(account_id), OffsetDateTime::now_utc()).await.unwrap_or_default();
    let caps = connector.order_caps();
    let tick = settings_bt.as_ref().map(|s| s.instrument.tick_size).filter(|t| *t > 0.0).and_then(|t| otw_store::paper_broker::decimal_of(t).ok());

    let mut notes: Vec<String> = problems;
    let mut degraded: Vec<String> = Vec::new();
    let mut blocked_any = false;
    let mut book = Vec::new();
    for i in &inst {
        let again = held_qty(connector.as_ref(), &settings, i).await.ok();
        let held = first.get(&i.symbol).copied().filter(|q| again == Some(*q));
        let baseline = session.exec_baseline.get(&i.symbol).and_then(|v| v.as_str()).and_then(|s| s.parse().ok()).unwrap_or_default();
        let adopted = session.exec_baseline.get("adopted").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
        let act = Actual { held, baseline, legs: legs_of(&rows, &i.symbol), adopted };
        let eng = match &settings_bt {
            Some(s) => intended(session.stats.as_ref(), &i.ticker, &live, s, i.tf_secs),
            None => Intended::default(),
        };
        let blocked = !otw_store::broker_orders::unknown_on(&state.pool, account_id, &i.ticker).await.unwrap_or_default().is_empty();
        blocked_any |= blocked;
        let wants_size = eng.entry.is_some() || !eng.limits.is_empty();
        let step = if armed && wants_size { connector.qty_step(&settings, &i.ticker).await.ok().flatten() } else { None };
        let rules = Rules {
            armed_at: session.armed_at,
            active: session.status == "active",
            halted: halts.first().map(|h| format!("trading is halted on the account ({})", if h.reason == "kill_switch" { "the kill switch is on" } else { h.reason.as_str() })),
            blocked,
            native: ex.exits == "native",
            brackets: caps.as_ref().is_some_and(|c| c.brackets),
            oca: caps.as_ref().is_some_and(|c| c.oca),
            amend: caps.as_ref().is_some_and(|c| c.amend),
            reduce_only: caps.as_ref().is_some_and(|c| c.reduce_only),
            step,
            tick,
            max_qty: ex.max_position_qty,
        };
        let plan = plan(id, &i.ticker, &eng, &act, &rules);
        if let Some(d) = &plan.degraded {
            degraded.push(d.clone());
        }
        notes.extend(plan.notes.iter().cloned());
        let out = act_on(state, &session, account_id, connector.as_ref(), &settings, i, caps.as_ref(), &plan.actions, &rows).await;
        notes.extend(out.notes);
        blocked_any |= out.unknown;
        book.push(json!({
            "key": i.key,
            "ticker": i.ticker,
            "symbol": i.symbol,
            "intended": {
                "side": eng.hold.as_ref().map(|h| dir(h.long)),
                "stop": eng.hold.as_ref().and_then(|h| h.stop),
                "target": eng.hold.as_ref().and_then(|h| h.target),
                "entry": eng.entry.as_ref().map(|e| json!({ "side": dir(e.long), "qty": e.qty, "price": e.price, "signal_ts": e.signal_ts })),
                "limits": eng.limits.iter().map(|l| json!({ "side": dir(l.long), "qty": l.qty, "price": l.price })).collect::<Vec<_>>(),
            },
            "broker": {
                "held": held.or(again),
                "baseline": baseline,
                "own": plan.own,
                "drift": plan.drift,
                "working": act.legs.iter().filter(|l| l.working).map(|l| json!({
                    "client_id": l.client_id, "role": l.role.word(), "side": if l.buy { "buy" } else { "sell" },
                    "qty": l.qty, "limit": l.limit, "stop": l.stop,
                })).collect::<Vec<_>>(),
            },
            "halted": rules.halted,
            "blocked": blocked,
        }));
    }
    let (state_word, mut note) = if blocked_any {
        ("blocked", "an order got no answer from the broker: nothing is sent on its instrument until it is settled. Check it at the broker.".to_string())
    } else if !degraded.is_empty() {
        ("degraded", degraded.join(" "))
    } else if !halts.is_empty() && armed {
        ("paused", "trading is halted on the account: the session sends only what takes risk off until the halt is lifted".to_string())
    } else {
        ("synced", String::new())
    };
    notes.sort();
    notes.dedup();
    if !notes.is_empty() {
        if !note.is_empty() {
            note.push(' ');
        }
        note.push_str(&notes.join(" "));
    }
    note_state(state, &session, state_word, &note, Some(&json!({ "instruments": book }))).await
}

/// Record what a sync found, and say it on the session's channels when it changed: once per
/// distinct note, and once when it clears.
async fn note_state(state: &AppState, session: &store::Session, word: &str, note: &str, book: Option<&Value>) -> anyhow::Result<()> {
    exec_store::set_state(&state.pool, session.id, word, note, book).await?;
    if note != session.exec_note {
        if !note.is_empty() {
            let title = match word {
                "degraded" => "degraded at the broker",
                "blocked" => "check at the broker",
                "paused" => "sending paused",
                "error" => "cannot reach the broker",
                _ => "broker execution",
            };
            say(state, session, title, note).await;
        } else if !session.exec_note.is_empty() {
            say(state, session, "broker execution back to normal", "The session and the account agree again.").await;
        }
    }
    Ok(())
}

/// What carrying out a plan found.
#[derive(Default)]
struct Done {
    notes: Vec<String>,
    unknown: bool,
}

/// Carry out one instrument's plan, in order. Every order goes through the server-side sender
/// (every gate, the audit log); a cancel through the audited broker call. A refusal is noted, an
/// unknown outcome cancels the session's resting entries on the instrument and adds nothing.
#[allow(clippy::too_many_arguments)]
async fn act_on(
    state: &AppState,
    session: &store::Session,
    account: Uuid,
    connector: &dyn Broker,
    settings: &HashMap<String, String>,
    inst: &Instrument,
    caps: Option<&otw_connector_model::OrderCaps>,
    actions: &[Action],
    rows: &[exec_store::Order],
) -> Done {
    let mut done = Done::default();
    let source = Source::Strategy(session.id);
    let account_row = match otw_store::brokers::get(&state.pool, account).await {
        Ok(Some(a)) => a,
        _ => return done,
    };
    for a in actions {
        match a {
            Action::Place(legs) => {
                if done.unknown {
                    break;
                }
                let Some(reference) = reference_for(legs, inst) else {
                    done.notes.push(format!("{} has no stored price to send against yet: nothing was sent", inst.ticker));
                    continue;
                };
                let new: Vec<exec_store::NewOrder> = legs
                    .iter()
                    .map(|l| exec_store::NewOrder {
                        client_id: l.client_id.clone(),
                        intent: l.intent.clone(),
                        role: l.role.word().into(),
                        ticker: inst.ticker.clone(),
                        symbol: inst.symbol.clone(),
                        side: if l.buy { "buy".into() } else { "sell".into() },
                        qty: l.qty,
                        limit_price: l.limit,
                        stop_price: l.stop,
                        engine_qty: l.engine_qty,
                        engine_price: l.engine_price,
                        position: l.position.clone(),
                    })
                    .collect();
                // Written before it leaves: a decision already written is never sent again.
                match exec_store::claim(&state.pool, session.id, account, &new).await {
                    Ok(true) => {}
                    Ok(false) => continue,
                    Err(e) => {
                        done.notes.push(format!("the order could not be recorded, so it was not sent: {e:#}"));
                        continue;
                    }
                }
                let ticket = ticket_of(inst, legs, caps, reference);
                match dispatch::send_internal(state, account, MODULE, source, ticket).await {
                    Ok(placed) => {
                        for l in legs {
                            let venue = placed.iter().find(|p| p.key == l.client_id).map(|p| p.id.as_str());
                            let _ = exec_store::set_outcome(&state.pool, account, &l.client_id, "sent", None, venue).await;
                        }
                    }
                    Err(SendRefusal::Refused { message, .. }) => {
                        for l in legs {
                            let _ = exec_store::set_outcome(&state.pool, account, &l.client_id, "refused", Some(&message), None).await;
                        }
                        done.notes.push(format!("the {} order on {} was refused: {message}", legs[0].role.word(), inst.ticker));
                    }
                    Err(SendRefusal::Unknown { keys, message }) => {
                        for l in legs {
                            let outcome = if keys.contains(&l.client_id) { "unknown" } else { "refused" };
                            let _ = exec_store::set_outcome(&state.pool, account, &l.client_id, outcome, Some(&message), None).await;
                        }
                        done.unknown = true;
                        // Cancel what the session rests here; never add, never resend.
                        for o in rows.iter().filter(|o| o.symbol == inst.symbol && o.role == "limit" && o.outcome == "sent") {
                            if let Some(v) = &o.venue_id {
                                let _ = cancel(state, &account_row, connector, settings, source, v).await;
                            }
                        }
                        done.notes.push(format!("the {} order on {} got no answer from the broker ({message}): it is not sent again. Check it at the broker.", legs[0].role.word(), inst.ticker));
                    }
                }
            }
            Action::Cancel { client_id } => {
                let Some(o) = rows.iter().find(|o| &o.client_id == client_id) else { continue };
                let Some(venue) = &o.venue_id else { continue };
                if let Err(e) = cancel(state, &account_row, connector, settings, source, venue).await {
                    let msg = e.message().to_string();
                    // An order that stopped working on its own is not an error: the next lookup reads it.
                    if !msg.contains("no longer working") {
                        done.notes.push(format!("cancelling the {} order on {} failed: {msg}", o.role, inst.ticker));
                    }
                }
            }
            Action::Modify { client_id, limit, stop, engine_price } => {
                let Some(o) = rows.iter().find(|o| &o.client_id == client_id) else { continue };
                let Some(venue) = &o.venue_id else { continue };
                let want = stop.or(*limit).unwrap_or_default();
                let seen = REFUSED_MOVES.lock().map(|m| m.get(client_id) == Some(&want)).unwrap_or(false);
                if seen {
                    continue;
                }
                match modify(state, &account_row, connector, settings, source, venue, *limit, *stop).await {
                    Ok(replaced) => {
                        let _ = exec_store::set_prices(&state.pool, account, client_id, *limit, *stop, Some(*engine_price)).await;
                        if let Some(p) = replaced {
                            let _ = exec_store::set_outcome(&state.pool, account, client_id, "sent", None, Some(&p.id)).await;
                        }
                    }
                    Err(e) => {
                        if let Ok(mut m) = REFUSED_MOVES.lock() {
                            m.insert(client_id.clone(), want);
                        }
                        done.notes.push(format!("moving the {} on {} to {want} was refused: {}", o.role, inst.ticker, e.message()));
                    }
                }
            }
        }
    }
    done
}

/// A cancel, written to the audit log under the strategy's name. Never gated: cancelling is
/// the safe direction.
async fn cancel(state: &AppState, account: &otw_store::brokers::BrokerRow, connector: &dyn Broker, settings: &HashMap<String, String>, source: Source, venue: &str) -> Result<(), crate::ApiError> {
    crate::brokers_api::audited_from(state, account, "cancel", source, json!({ "order": venue }), connector.cancel(settings, venue)).await
}

/// A level moved at the broker: the route's own gates (the live gate, the risk gate on a
/// change), written to the audit log under the strategy's name.
#[allow(clippy::too_many_arguments)]
async fn modify(
    state: &AppState,
    account: &otw_store::brokers::BrokerRow,
    connector: &dyn Broker,
    settings: &HashMap<String, String>,
    source: Source,
    venue: &str,
    limit: Option<Decimal>,
    stop: Option<Decimal>,
) -> Result<Option<brokers::Placed>, crate::ApiError> {
    use otw_connector_model::{OrderAmend, Price};
    let mut settings = settings.clone();
    crate::brokers_api::require_live(state, account, connector, &mut settings).await?;
    let change = OrderAmend { limit: limit.map(Price::new), stop: stop.map(Price::new), qty: None, tif: None };
    let request = json!({ "order": venue, "limit": limit, "stop": stop });
    let call = async {
        brokers::risk::gate_modify(state, account, &change).await?;
        connector.modify(&settings, venue, &change).await
    };
    let moved = crate::brokers_api::audited_from(state, account, "modify", source, request, call).await?;
    if let Some(p) = &moved {
        if let Err(e) = otw_store::broker_orders::replaced(&state.pool, account.id, venue, &p.key, &p.id).await {
            tracing::error!("paper exec: could not follow {venue} to {}: {e:#}", p.id);
        }
    }
    Ok(moved)
}

// ── Creating, arming, flattening ────────────────────────────────────────────

/// A broker target checked at creation: the account trades and is granted to the backtest,
/// the strategy is one a broker can execute, each instrument is mapped through the account's
/// own lookup (never by name), the exits mode is one the broker's capabilities hold, and no
/// other session executes one of these instruments on the account. Answers the normalized
/// target and the symbols to claim; `None` for a simulated session.
pub async fn vet(state: &AppState, raw: Option<&Value>, settings: &Settings, dataset_ids: &[Uuid]) -> Result<Option<(Execution, Vec<String>)>, crate::ApiError> {
    use crate::ApiError;
    let Some(raw) = raw.filter(|v| !v.is_null()) else { return Ok(None) };
    let mut ex: Execution = serde_json::from_value(raw.clone()).map_err(|e| ApiError::bad_request(&format!("invalid execution: {e}")))?;
    match ex.target.as_str() {
        "" | "simulate" => return Ok(None),
        "broker" => {}
        _ => return Err(ApiError::bad_request("a session simulates or executes on a broker account")),
    }
    let account_id = ex.account_id.ok_or_else(|| ApiError::bad_request("pick the broker account the session executes on"))?;
    let (connector, bsettings, account) = crate::brokers_api::resolve_trading(state, account_id, Some(MODULE)).await?;
    if settings.kind != "signals" {
        return Err(ApiError::bad_request("only a signal strategy executes on a broker account: grid and savings-plan sessions simulate"));
    }
    if settings.pyramiding > 1 {
        return Err(ApiError::bad_request("a broker session sends one entry per position: set pyramiding to 1 in the strategy"));
    }
    let caps = connector.order_caps();
    let native_ok = caps.as_ref().is_some_and(|c| c.brackets && c.amend);
    ex.exits = match ex.exits.as_str() {
        "" => if native_ok { "native".into() } else { "watch".into() },
        "native" if !native_ok => {
            return Err(ApiError::bad_request(&format!(
                "{} cannot hold a stop and a target attached to an entry: choose watched exits",
                connector.capability().label
            )))
        }
        "native" | "watch" => ex.exits.clone(),
        _ => return Err(ApiError::bad_request("exits are native or watched")),
    };
    if let Some(m) = ex.max_position_qty {
        if m <= Decimal::ZERO {
            return Err(ApiError::bad_request("the maximum position must be more than zero, or left empty"));
        }
    }
    let mut symbols = BTreeMap::new();
    for &id in dataset_ids {
        let ds = otw_store::histdata::get_dataset(&state.pool, id).await?.ok_or_else(|| ApiError::bad_request("unknown dataset"))?;
        let looked = connector
            .symbol_for(&bsettings, &ds.ticker, &ds.asset_type)
            .await
            .map_err(|e| ApiError::bad_request(&format!("“{}” could not look {} up: {e:#}", account.name, ds.ticker)))?;
        let Some(symbol) = looked.filter(|s| !s.trim().is_empty()) else {
            return Err(ApiError::bad_request(&format!(
                "“{}” cannot say which of its instruments {} is: pick an account whose broker lists it, or a dataset of the broker's own market",
                account.name, ds.ticker
            )));
        };
        symbols.insert(key_of(&ds.provider, &ds.ticker), symbol);
    }
    let wanted: Vec<String> = symbols.values().cloned().collect();
    if let Some((symbol, name)) = exec_store::taken(&state.pool, account.id, &wanted, None).await?.into_iter().next() {
        return Err(ApiError::bad_request(&format!(
            "the session “{name}” already executes {symbol} on “{}”: one strategy per instrument and account, or the two would trade one position against each other",
            account.name
        )));
    }
    ex.target = "broker".into();
    ex.account = account.name.clone();
    ex.broker = account.broker.clone();
    ex.symbols = symbols;
    Ok(Some((ex, wanted)))
}

/// What the account holds of each instrument that the session's own fills do not explain:
/// measured when the session is armed, so a spot account's holding is not a drift.
pub async fn baseline(state: &AppState, session: &store::Session) -> anyhow::Result<Value> {
    let ex = Execution::of(session).ok_or_else(|| anyhow::anyhow!("this session simulates"))?;
    let account_id = ex.account_id.ok_or_else(|| anyhow::anyhow!("this session names no account"))?;
    let (connector, settings, _) = crate::brokers_api::resolve_for_job(&state.pool, &state.cipher, account_id, MODULE).await?;
    let inst = instruments(state, session, &ex).await?;
    let rows = exec_store::orders(&state.pool, session.id).await?;
    let mut out = serde_json::Map::new();
    let mut adopted = Vec::new();
    for i in &inst {
        let held = held_qty(connector.as_ref(), &settings, i).await?;
        let legs = legs_of(&rows, &i.symbol);
        let own: Decimal = legs.iter().map(|l| if l.buy { l.filled } else { -l.filled }).sum();
        out.insert(i.symbol.clone(), json!((held - own).normalize().to_string()));
        // A partial fill left over is the position from now on: the user said so.
        adopted.extend(legs.iter().filter(|l| l.role.opens() && l.done && l.filled > Decimal::ZERO && l.filled < l.qty).map(|l| json!(l.client_id)));
    }
    out.insert("adopted".into(), Value::Array(adopted));
    Ok(Value::Object(out))
}

/// Stop and flatten: disarm, cancel every order the session has working, and close what its
/// own fills hold, at the market, with a reduce-only order where the broker has one. Sent by
/// the user from the page (the caller checked the trading lock), so the background permission
/// is not asked; written to the audit log under the strategy's name. Answers what was done.
pub async fn flatten(state: &AppState, session: &store::Session) -> Result<Value, crate::ApiError> {
    use crate::ApiError;
    let _one = SYNC.lock().await;
    let ex = Execution::of(session).ok_or_else(|| ApiError::bad_request("this session simulates: it holds nothing at a broker"))?;
    let account_id = ex.account_id.ok_or_else(|| ApiError::bad_request("this session names no account"))?;
    exec_store::disarm(&state.pool, session.id).await?;
    let (connector, settings, account) = crate::brokers_api::resolve(state, account_id, None).await?;
    let source = Source::Strategy(session.id);
    let inst = instruments(state, session, &ex).await.map_err(|e| ApiError::bad_request(&format!("{e:#}")))?;
    let rows = exec_store::orders(&state.pool, session.id).await?;
    let _ = look_up(state, connector.as_ref(), &settings, account_id, &rows, &inst).await;
    let rows = exec_store::orders(&state.pool, session.id).await?;
    let mut cancelled = 0;
    let mut closed = Vec::new();
    let mut problems = Vec::new();
    for i in &inst {
        let legs = legs_of(&rows, &i.symbol);
        // Exits first would leave a moment unprotected; entries first, then exits, then close.
        let mut working: Vec<&Leg> = legs.iter().filter(|l| l.working).collect();
        working.sort_by_key(|l| !l.role.opens());
        for l in working {
            let Some(venue) = rows.iter().find(|o| o.client_id == l.client_id).and_then(|o| o.venue_id.clone()) else { continue };
            match cancel(state, &account, connector.as_ref(), &settings, source, &venue).await {
                Ok(()) => cancelled += 1,
                Err(e) => problems.push(e.message().to_string()),
            }
        }
        let own: Decimal = legs.iter().map(|l| if l.buy { l.filled } else { -l.filled }).sum();
        if own.is_zero() {
            continue;
        }
        let position = legs.iter().rev().find(|l| l.role.opens() && l.filled > Decimal::ZERO).map(|l| l.client_id.clone()).unwrap_or_default();
        let tries = legs.iter().filter(|l| l.role == Role::Close).count();
        let intent = format!("close|{position}|flatten{tries}");
        let caps = connector.order_caps();
        let leg = LegSpec {
            client_id: client_id(session.id, &intent, Role::Close),
            intent,
            role: Role::Close,
            buy: own < Decimal::ZERO,
            kind: OrderType::Market,
            qty: own.abs(),
            limit: None,
            stop: None,
            parent: None,
            group: None,
            reduce_only: caps.as_ref().is_some_and(|c| c.reduce_only),
            engine_qty: None,
            engine_price: None,
            position,
        };
        let new = exec_store::NewOrder {
            client_id: leg.client_id.clone(),
            intent: leg.intent.clone(),
            role: "close".into(),
            ticker: i.ticker.clone(),
            symbol: i.symbol.clone(),
            side: if leg.buy { "buy".into() } else { "sell".into() },
            qty: leg.qty,
            limit_price: None,
            stop_price: None,
            engine_qty: None,
            engine_price: None,
            position: leg.position.clone(),
        };
        if !exec_store::claim(&state.pool, session.id, account_id, std::slice::from_ref(&new)).await? {
            continue;
        }
        let reference = reference_for(std::slice::from_ref(&leg), i).unwrap_or((Decimal::ONE, (OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as i64));
        let ticket = ticket_of(i, std::slice::from_ref(&leg), caps.as_ref(), reference);
        match send_reducing(state, &account, connector.as_ref(), &settings, source, ticket).await {
            Ok(placed) => {
                let venue = placed.first().map(|p| p.id.clone());
                exec_store::set_outcome(&state.pool, account_id, &leg.client_id, "sent", None, venue.as_deref()).await?;
                closed.push(json!({ "ticker": i.ticker, "qty": leg.qty, "side": if leg.buy { "buy" } else { "sell" } }));
            }
            Err(f) => {
                let outcome = if f.unknown.is_some() { "unknown" } else { "refused" };
                exec_store::set_outcome(&state.pool, account_id, &leg.client_id, outcome, Some(&f.api.message), None).await?;
                problems.push(f.api.message.clone());
            }
        }
    }
    let message = if problems.is_empty() {
        format!("Stopped and flattened: {cancelled} order(s) cancelled, {} position(s) closed at the market.", closed.len())
    } else {
        format!("Stopped. Some of it did not go through: {}. Check the account at the broker.", problems.join("; "))
    };
    say(state, session, "stopped and flattened", &message).await;
    crate::security_events::log("strategy_flatten", &format!("strategy “{}” stopped and flattened on “{}”", session.name, account.name));
    poke();
    Ok(json!({ "cancelled": cancelled, "closed": closed, "problems": problems }))
}

/// An order that only takes risk off, sent from the user's page: the risk gate (which lets risk
/// reduction through every limit and halt), the legs stored before anything leaves, the
/// broker's place and its answer stored, the audit log. No live gate: closing takes risk off,
/// like the chart's close.
async fn send_reducing(
    state: &AppState,
    account: &otw_store::brokers::BrokerRow,
    connector: &dyn Broker,
    settings: &HashMap<String, String>,
    source: Source,
    ticket: OrderTicket,
) -> Result<Vec<brokers::Placed>, dispatch::Failure> {
    let mut settings = settings.clone();
    brokers::live::mark_canary(account.id, &mut settings);
    let request = serde_json::to_value(&ticket).unwrap_or(Value::Null);
    let keys: Vec<String> = ticket.legs.iter().map(|l| l.key.clone()).collect();
    let mut unknown = None;
    let call = async {
        brokers::risk::gate(state, account, connector, &settings, &ticket).await?;
        brokers::reconcile::claim(state, account, &ticket).await?;
        let out = connector.place(&settings, &ticket).await;
        brokers::reconcile::record(state, account, &keys, &out).await;
        if let Err(e) = &out {
            unknown = e.downcast_ref::<brokers::OrderUnknown>().map(|u| u.keys.clone());
        }
        out
    };
    crate::brokers_api::audited_from(state, account, "place", source, request, call).await.map_err(|api| dispatch::Failure { api, unknown })
}

/// Disarm the sessions armed on one account, or on every account, each told on its log and
/// channels. The count is what `dispatch::disarm_*` reports.
pub async fn disarm_where(state: &AppState, account: Option<Uuid>) -> anyhow::Result<u64> {
    let ids = exec_store::disarm_where(&state.pool, account).await?;
    for id in &ids {
        if let Ok(Some(s)) = store::get_session(&state.pool, *id).await {
            say(state, &s, "disarmed", "The account's background trading, trading or second factor changed, or a kill switch was engaged: the session sends nothing until you arm it again.").await;
        }
    }
    Ok(ids.len() as u64)
}

// ── What the dashboard reads ────────────────────────────────────────────────

/// What a session's orders move: `paper` (the paper broker), `live` (live trading is on for
/// the account) or `sandbox` (a venue's test account, or a live one not opened for trading).
pub fn level(a: &otw_store::brokers::BrokerRow) -> &'static str {
    if a.broker == brokers::paper::ID {
        "paper"
    } else if a.live_since.is_some() {
        "live"
    } else {
        "sandbox"
    }
}

/// The execution panel in one read: the target, whether it is armed, what the last sync found
/// (intended and broker book, drift), the account's level and limits, the session's orders,
/// and per position the engine's price beside the broker's.
pub async fn view(state: &AppState, session: &store::Session) -> anyhow::Result<Value> {
    let Some(ex) = Execution::of(session) else { return Ok(json!({ "target": "simulate" })) };
    let account_id = ex.account_id.unwrap_or_default();
    let account = otw_store::brokers::get(&state.pool, account_id).await?;
    let limits = otw_store::broker_risk::limits(&state.pool, account_id).await.ok();
    let halts = otw_store::broker_risk::halts(&state.pool, Some(account_id), OffsetDateTime::now_utc()).await.unwrap_or_default();
    let orders = exec_store::orders(&state.pool, session.id).await?;
    let fills = exec_store::fills(&state.pool, session.id).await?;
    let trades = store::list_trades(&state.pool, session.id, false).await.unwrap_or_default();
    let level = account.as_ref().map_or("missing", level);
    let positions = compare(&orders, &fills, &trades);
    Ok(json!({
        "target": "broker",
        "execution": ex,
        "armed_at": session.armed_at.and_then(|t| t.format(&time::format_description::well_known::Rfc3339).ok()),
        "state": session.exec_state,
        "note": session.exec_note,
        "synced_at": session.exec_synced_at.and_then(|t| t.format(&time::format_description::well_known::Rfc3339).ok()),
        "book": session.exec_book,
        "baseline": session.exec_baseline,
        "account": account.as_ref().map(|a| json!({
            "id": a.id, "name": a.name, "broker": a.broker, "level": level,
            "background_trading": a.background_trading_ack.is_some(),
            "background_trading_since": a.background_trading_since.and_then(|t| t.format(&time::format_description::well_known::Rfc3339).ok()),
            "trading": a.trades(),
        })),
        "limits": limits,
        "halted": halts.first().map(|h| h.reason.clone()),
        "orders": orders,
        "fills": fills,
        "positions": positions,
    }))
}

/// Per entry the session sent: the engine's entry and exit beside the broker's (the size
/// weighted price of the fills), so simulated and real read side by side.
fn compare(orders: &[exec_store::Order], fills: &[exec_store::Fill], trades: &[store::Trade]) -> Vec<Value> {
    let avg = |ids: &[&str]| -> Option<(Decimal, Decimal)> {
        let mine: Vec<&exec_store::Fill> = fills.iter().filter(|f| ids.contains(&f.client_id.as_str())).collect();
        let qty: Decimal = mine.iter().map(|f| f.qty).sum();
        if qty.is_zero() {
            return None;
        }
        let notional: Decimal = mine.iter().map(|f| f.qty * f.price).sum();
        Some(((notional / qty).round_dp(10).normalize(), qty.normalize()))
    };
    let mut out = Vec::new();
    for e in orders.iter().filter(|o| o.role == "entry" || o.role == "limit") {
        let parts: Vec<&str> = e.intent.split('|').collect();
        let (direction, signal) = (parts.get(2).copied().unwrap_or(""), parts.get(3).copied().unwrap_or(""));
        // The engine's trade: the first one on that side that entered after the deciding close.
        let sim = trades.iter().filter(|t| t.ticker == e.ticker && t.direction == direction && t.entry_ts.as_str() > signal).min_by(|a, b| a.entry_ts.cmp(&b.entry_ts));
        let exit_ids: Vec<&str> = orders.iter().filter(|o| o.position == e.client_id && matches!(o.role.as_str(), "stop" | "target" | "close")).map(|o| o.client_id.as_str()).collect();
        let real_in = avg(&[e.client_id.as_str()]);
        let real_out = avg(&exit_ids);
        let f = |t: Option<&store::Trade>, k: &str| t.and_then(|t| t.trade.get(k)).and_then(Value::as_f64);
        out.push(json!({
            "client_id": e.client_id,
            "ticker": e.ticker,
            "side": direction,
            "engine_qty": e.engine_qty,
            "qty": e.qty,
            "filled": real_in.map(|(_, q)| q),
            "sim_entry": f(sim, "entry_price").or(e.engine_price),
            "real_entry": real_in.map(|(p, _)| p),
            "sim_exit": sim.filter(|t| !t.open).and_then(|t| t.trade.get("exit_price")).and_then(Value::as_f64),
            "real_exit": real_out.map(|(p, _)| p),
            "outcome": e.outcome,
            "reason": e.reason,
            "at": e.created_at.format(&time::format_description::well_known::Rfc3339).ok(),
        }));
    }
    out.reverse();
    out
}

#[cfg(test)]
#[path = "paper_exec_tests.rs"]
mod tests;
