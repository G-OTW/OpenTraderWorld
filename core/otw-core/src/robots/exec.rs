//! Strategies on a broker account: a paper session whose fills happen at a broker.
//!
//! **The engine decides, the broker executes, a sync converges.** A session that executes
//! keeps ticking exactly like a simulated one ([`crate::robots`]): the ordinary backtest re-run
//! over the moving window, its trades diffed into the paper book. After each tick, after a level
//! the live watch ([`crate::robots::live`]) saw crossed, and on a cycle, [`sync`] reads what the
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
//! beside it, less what it held when the session was armed (`exec_baseline`). Every sync reads
//! the account before anything leaves, so a restart reconciles before it sends. A difference
//! is the user acting at the broker: a position opened while the robot is flat, the robot's
//! position closed, or a size added to or taken from it stops the robot (disarmed, `stopped`,
//! armed again by hand, which measures the account anew; a closed position is written closed
//! outside). The robot's instruments are its own to check: the account's reconciliation does
//! not judge them ([`crate::brokers::reconcile`]). A stop or a target the user moved at the
//! broker is left where it is: the robot no longer moves it. A partial fill left over puts the
//! session in `degraded` until the user flattens or adopts it. An account that cannot be read
//! sends nothing; [`UNREACHABLE_AFTER`] syncs in a row, the robot says it (`unreachable`).
//!
//! **Its money is what the broker did.** An entry the account cannot pay for is written refused
//! with what it needs and what the account has; every refused opening order is a `refused`
//! line in the log. A closed trade is priced from its own fills, fees included
//! ([`trade_money`]), over the engine's figure.
//!
//! **Never resend.** Each decision is an intent (`entry|<ticker>|<side>|<signal bar>`, the
//! exits of a position, a close) written before anything leaves (`robot_exec_orders`, unique
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

use otw_store::robots as store;
use otw_store::robots_exec as exec_store;

use super::{guards, sizing};
use crate::backtest::{self, Settings};
use crate::brokers::dispatch::{self, SendRefusal, Source};
use crate::brokers::{self, Broker, Lookup, OrderLeg, OrderTicket};
use crate::AppState;

/// The module a robot sends as: the account must be granted to it.
pub const MODULE: &str = super::MODULE;
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
/// Syncs in a row whose account read failed, per robot: a passing outage is not news.
static UNANSWERED: LazyLock<Mutex<HashMap<Uuid, u32>>> = LazyLock::new(Default::default);
/// Syncs in a row the broker may leave unanswered before the robot says it is unreachable
/// (45 s at the cycle). Nothing is sent meanwhile: the account must be read first.
pub const UNREACHABLE_AFTER: u32 = 3;

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
        // Gone at the next open at the market, decided by the close that just passed: the close
        // goes now, not once the next bar has printed.
        if exited(&format!("{ticker}|{direction}|{}", text(p, "entry_ts"))) || p.get("leaves_at_open").and_then(Value::as_bool) == Some(true) {
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
            // The engine only decides an entry on the other side of a position it holds when it
            // reverses at that open (stop and reverse): the new side is what it holds from now.
            if out.hold.as_ref().is_none_or(|h| h.long != long) {
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
    /// What the venue kept of the instrument itself as fees on the robot's fills (a spot buy's
    /// commission taken in the base asset): the account never held it.
    pub withheld: Decimal,
    /// The robot's working exits the user moved at the broker: client id, the price the broker
    /// holds now. Left where the user put them.
    pub moved: HashMap<String, Decimal>,
    /// The broker's working orders could not be read: a move by hand cannot be told from the
    /// robot's own level, so no exit is moved this pass.
    pub orders_unread: bool,
}

impl Actual {
    /// The session's own position: what its legs filled, buys up and sells down, less what the
    /// venue kept of it as fees.
    pub fn own(&self) -> Decimal {
        self.legs.iter().map(|l| if l.buy { l.filled } else { -l.filled }).sum::<Decimal>() - self.withheld
    }
}

/// The fees the venue took in the instrument itself on the robot's fills of `symbol`: a fee
/// named in the pair's base asset (`BTC` on `BTCUSDT`). Pure.
pub fn withheld(fills: &[exec_store::Fill], symbol: &str) -> Decimal {
    fills
        .iter()
        .filter(|f| f.symbol == symbol && !f.fee.is_zero())
        .filter(|f| {
            let c = f.fee_currency.to_uppercase();
            !c.is_empty() && c.len() < symbol.len() && symbol.to_uppercase().starts_with(&c)
        })
        .map(|f| f.fee)
        .sum()
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
    /// The venue's smallest order, when it states one.
    pub min_qty: Option<Decimal>,
    /// The robot's capital policy and the guard scale in force (0 reads as 1).
    pub capital: sizing::Capital,
    pub scale: Decimal,
    /// Why the robot's guards hold new entries now (a pause, a halt).
    pub guard_hold: Option<String>,
    /// Positions the robot holds and opening orders it sent today, for its caps.
    pub open_positions: i64,
    pub orders_today: i64,
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
    /// An entry the robot does not take (its guards hold it, its caps are reached): written
    /// refused with the reason, so no later sync sends it.
    Refuse { leg: LegSpec, reason: String },
}

/// The sync's decision on one instrument.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Plan {
    pub actions: Vec<Action>,
    /// Why the session adds no exposure here: a partial fill left over.
    pub degraded: Option<String>,
    /// The user acted on the robot's instrument at the broker (opened a position while the robot
    /// was flat, closed the robot's position, added to it or took from it): the robot is
    /// disarmed and must be armed again. The sentence names what happened.
    pub stop: Option<String>,
    /// The robot's position the user closed at the broker: written as closed outside, so the
    /// robot holds nothing when it is armed again.
    pub closed_outside: Option<Decimal>,
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
/// (never rounded up). `None` when nothing is left. The sync sizes through [`sizing::size`],
/// which cuts the same way; this is the bare step rule its tests pin.
#[cfg(test)]
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
            // What the account holds of the instrument beside its baseline, against what the
            // robot's own fills hold.
            let outside = held - act.baseline;
            if own.is_zero() {
                p.stop = Some(format!(
                    "a {} {} {ticker} position was opened at the broker outside the bot: the bot was disarmed. Arm it again to go on (what is held then stays outside the bot).",
                    dir(drift > Decimal::ZERO),
                    drift.abs().normalize()
                ));
            } else if outside.is_zero() || outside.is_sign_positive() != own.is_sign_positive() {
                p.stop = Some(format!(
                    "the bot's {} {} {ticker} position was closed at the broker outside the bot: the bot was disarmed. Arm it again to go on.",
                    dir(own > Decimal::ZERO),
                    own.abs().normalize()
                ));
                p.closed_outside = Some(own);
            } else {
                p.stop = Some(format!(
                    "the bot's {} {} {ticker} position was changed at the broker outside the bot ({} held now): the bot was disarmed. Arm it again to go on (what is held then beside the bot's own size stays outside the bot).",
                    dir(own > Decimal::ZERO),
                    own.abs().normalize(),
                    outside.abs().normalize()
                ));
            }
        }
    }
    if p.stop.is_some() {
        return p;
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
            // Stop and reverse: the entry on the other side, decided at this close. The position
            // the close takes off frees its place under the robot's maximum: a reverse is not an add.
            if let Some(e) = eng.entry.as_ref().filter(|e| Some(e.long) == engine_side) {
                let freed = Rules { open_positions: (r.open_positions - 1).max(0), ..r.clone() };
                enter(&mut p, session, ticker, e, act, &freed, why_not.as_deref());
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
        if let Some(why) = why_not.as_ref().or(r.guard_hold.as_ref()) {
            p.notes.push(format!("the {} limit on {ticker} at {} was not placed: {why}", dir(x.long), x.price));
            continue;
        }
        let qty = match sized(&mut p, ticker, x.qty, r) {
            Size::Send(q) => q,
            Size::Refused(why) => {
                p.actions.push(refusal(session, &intent, Role::Limit, x.long, x.qty, x.price, why));
                continue;
            }
        };
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

/// What sizing an entry came to.
enum Size {
    Send(Decimal),
    /// Under the venue's step or minimum, over the maximum, at the robot's caps: written
    /// refused with the reason, never asked again.
    Refused(String),
}

/// The size to send, or why none is: the robot's capital layer ([`sizing::size`]), its guard
/// scale first, the venue's step, its caps, the session's maximum.
fn sized(p: &mut Plan, ticker: &str, engine: f64, r: &Rules) -> Size {
    let ask = sizing::Ask {
        ticker: ticker.to_string(),
        engine_qty: engine,
        scale: r.scale,
        step: r.step,
        min_qty: r.min_qty,
        open_positions: r.open_positions,
        orders_today: r.orders_today,
        max_qty: r.max_qty,
    };
    match sizing::size(&r.capital, &ask) {
        sizing::Sized::Take { qty, .. } => Size::Send(qty),
        sizing::Sized::Refuse(_, why) => {
            p.notes.push(why.clone());
            Size::Refused(why)
        }
    }
}

/// An entry written refused: the leg it would have been, nothing sent.
fn refusal(session: Uuid, intent: &str, role: Role, buy: bool, engine_qty: f64, engine_price: f64, reason: String) -> Action {
    Action::Refuse {
        leg: LegSpec {
            client_id: client_id(session, intent, role),
            intent: intent.to_string(),
            role,
            buy,
            kind: if role == Role::Limit { OrderType::Limit } else { OrderType::Market },
            qty: Decimal::ZERO,
            limit: None,
            stop: None,
            parent: None,
            group: None,
            reduce_only: false,
            engine_qty: Some(engine_qty),
            engine_price: Some(engine_price),
            position: String::new(),
        },
        reason,
    }
}

/// The opening order that decided a trade of the robot's book: the session's last entry or
/// limit on the trade's ticker and side whose deciding close came before the trade entered (a
/// market entry fills at the open after its close, a limit later still). Pure.
pub fn deciding<'a>(orders: &'a [exec_store::Order], ticker: &str, direction: &str, entry_ts: &str) -> Option<&'a exec_store::Order> {
    let entered = rfc(entry_ts)?;
    let side = if direction == "short" { "sell" } else { "buy" };
    orders
        .iter()
        .filter(|o| matches!(o.role.as_str(), "entry" | "limit") && o.ticker == ticker && o.side == side)
        .filter_map(|o| Some((rfc(o.intent.rsplit('|').next()?)?, o)))
        .filter(|(signal, _)| *signal < entered)
        .max_by_key(|(signal, _)| *signal)
        .map(|(_, o)| o)
}

/// The opening order of one trade of the robot's book: [`deciding`], decided after the trade
/// before it on the instrument entered. An order older than that opened an earlier trade, never
/// this one: a trade whose own entry never left has no opening order. `trades`: the robot's
/// book (open and closed). Pure.
pub fn opening_order<'a>(orders: &'a [exec_store::Order], trades: &[store::Trade], t: &store::Trade) -> Option<&'a exec_store::Order> {
    let entered = rfc(&t.entry_ts)?;
    let since = trades
        .iter()
        .filter(|x| x.ticker == t.ticker && x.trade_key != t.trade_key && !x.trade_key.contains("|pending:"))
        .filter_map(|x| rfc(&x.entry_ts))
        .filter(|e| *e < entered)
        .max();
    let o = deciding(orders, &t.ticker, &t.direction, &t.entry_ts)?;
    let signal = rfc(o.intent.rsplit('|').next()?)?;
    since.is_none_or(|s| signal >= s).then_some(o)
}

/// A close of the robot's position the engine did not decide: Stop and flatten, or the user's
/// own close at the broker.
pub fn outside_close(o: &exec_store::Order) -> bool {
    o.role == "close" && (o.intent.contains("|flatten") || o.reason.as_deref() == Some(CLOSED_OUTSIDE))
}

/// Why a trade of a broker robot's book holds nothing it took at the broker: its opening order
/// was refused, ended unfilled, or never left. `None` while it is taken, or not known yet (an
/// order in flight). A trade with no opening order at all is judged only open and once its
/// row is a minute old: the entry leaves seconds after its close, the row a bar later. Pure.
pub fn not_taken(orders: &[exec_store::Order], trades: &[store::Trade], t: &store::Trade, now: OffsetDateTime) -> Option<String> {
    if t.trade_key.contains("|pending:") || t.robot_qty.is_none_or(|q| q.is_zero()) {
        return None;
    }
    match opening_order(orders, trades, t) {
        Some(o) if o.outcome == "refused" => Some(format!("not taken: {}", o.reason.as_deref().unwrap_or("the opening order was refused"))),
        Some(o) if o.outcome == "sent" && o.filled_qty.is_zero() && matches!(o.status.as_deref(), Some("cancelled" | "rejected" | "expired")) => {
            Some(format!("not taken: the entry was {} at the broker without a fill", o.status.as_deref().unwrap_or_default()))
        }
        Some(_) => None,
        None if t.open && now - t.opened_at > time::Duration::minutes(1) && !orders.is_empty() => {
            Some("not taken: no order was sent for this entry".to_string())
        }
        None => None,
    }
}

/// The trades of the robot's book it holds nothing of at the broker ([`not_taken`]): not taken
/// after all. Their size goes to 0 with the reason, so the robot's positions, P&L, guards and
/// the fleet read what it did at the broker, not what the engine decided.
pub(crate) async fn untake_unsent(state: &AppState, session: Uuid) -> anyhow::Result<()> {
    let orders = exec_store::orders(&state.pool, session).await?;
    let trades = store::list_trades(&state.pool, session, false).await?;
    let now = OffsetDateTime::now_utc();
    let mut days = HashSet::new();
    for t in &trades {
        let Some(note) = not_taken(&orders, &trades, t, now) else { continue };
        if let Some(closed) = store::untake_trade(&state.pool, session, &t.trade_key, &note).await? {
            days.insert(closed.date());
        }
    }
    for day in days {
        otw_store::robots_guards::refresh_realized(&state.pool, session, day).await?;
    }
    Ok(())
}

/// A held trade of the robot closed at the broker by an exit the engine did not decide: its
/// native stop or target, Stop and flatten, or the user's own close there.
#[derive(Debug, Clone, PartialEq)]
pub struct BrokerExit {
    pub trade_key: String,
    pub ticker: String,
    pub direction: String,
    pub entry_ts: String,
    /// `take_profit`, the stop's own reason (`stop_loss`, `trailing_stop`), `flatten`, or
    /// `closed_outside`.
    pub reason: String,
    pub qty: Decimal,
    pub entry_price: Option<Decimal>,
    pub price: Decimal,
    pub fees: Decimal,
    pub at: OffsetDateTime,
}

/// The robot's held trades the broker closed whole by an exit the engine did not decide (its
/// native stop or target, Stop and flatten, the user's close at the broker), priced as the
/// venue filled them: those legs' fills cover all the entry filled, and no close the engine
/// decided took part (that exit is the engine's own). `trades`: the robot's book, to find each
/// trade's own opening order. `stop_reasons`: the stop's reason per trade key, from the
/// engine. Pure.
pub fn broker_exits(held: &[store::Trade], trades: &[store::Trade], orders: &[exec_store::Order], fills: &[exec_store::Fill], stop_reasons: &HashMap<String, String>) -> Vec<BrokerExit> {
    let mut out = Vec::new();
    for t in held.iter().filter(|t| !t.trade_key.contains("|pending:")) {
        let Some(entry) = opening_order(orders, trades, t).filter(|o| o.outcome == "sent" && o.filled_qty > Decimal::ZERO) else { continue };
        let legs: Vec<&exec_store::Order> = orders.iter().filter(|o| o.position == entry.client_id).collect();
        if legs.iter().any(|o| o.role == "close" && !outside_close(o) && o.filled_qty > Decimal::ZERO) {
            continue;
        }
        let exits: Vec<&&exec_store::Order> = legs.iter().filter(|o| (matches!(o.role.as_str(), "stop" | "target") || outside_close(o)) && o.filled_qty > Decimal::ZERO).collect();
        if exits.is_empty() {
            continue;
        }
        let of = |ids: &[&str]| fills.iter().filter(|f| ids.contains(&f.client_id.as_str())).collect::<Vec<_>>();
        let ins = of(&[entry.client_id.as_str()]);
        let in_qty: Decimal = ins.iter().map(|f| f.qty).sum();
        let exit_ids: Vec<&str> = exits.iter().map(|o| o.client_id.as_str()).collect();
        let outs = of(&exit_ids);
        let qty: Decimal = outs.iter().map(|f| f.qty).sum();
        // Out whole, by the venue's own fills: until they cover the entry, it waits.
        if qty.is_zero() || qty < entry.filled_qty.max(in_qty) {
            continue;
        }
        let Some(at) = outs.iter().map(|f| f.at).max() else { continue };
        let price = outs.iter().map(|f| f.qty * f.price).sum::<Decimal>() / qty;
        let entry_price = (!in_qty.is_zero()).then(|| ins.iter().map(|f| f.qty * f.price).sum::<Decimal>() / in_qty);
        let fees = ins.iter().chain(outs.iter()).map(|f| f.fee).sum();
        // The leg that took most of the position names how it left.
        let filled_by = |o: &exec_store::Order| outs.iter().filter(|f| f.client_id == o.client_id).map(|f| f.qty).sum::<Decimal>();
        let main = exits.iter().max_by_key(|o| filled_by(o)).copied().copied();
        let reason = match main {
            Some(o) if o.role == "target" => "take_profit".to_string(),
            Some(o) if o.reason.as_deref() == Some(CLOSED_OUTSIDE) => "closed_outside".to_string(),
            Some(o) if o.role == "close" => "flatten".to_string(),
            _ => stop_reasons.get(&t.trade_key).cloned().unwrap_or_else(|| "stop_loss".into()),
        };
        out.push(BrokerExit {
            trade_key: t.trade_key.clone(),
            ticker: t.ticker.clone(),
            direction: t.direction.clone(),
            entry_ts: t.entry_ts.clone(),
            reason,
            qty: entry.filled_qty,
            entry_price,
            price: price.normalize(),
            fees,
            at,
        });
    }
    out
}

/// Write the exits the broker made that the engine did not decide ([`broker_exits`]): a live
/// exit fill (the next run replays it on its bar, at its price, and the robot holds nothing
/// more), and the close said once, now.
async fn record_broker_exits(state: &AppState, session: &store::Session, rows: &[exec_store::Order], fills: &[exec_store::Fill], mult: f64) -> anyhow::Result<()> {
    let held = store::held(&state.pool, session.id).await?;
    if held.is_empty() || fills.is_empty() {
        return Ok(());
    }
    let trades = store::list_trades(&state.pool, session.id, false).await?;
    let text = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    let stop_reasons: HashMap<String, String> = session
        .stats
        .as_ref()
        .and_then(|s| s.get("open_positions"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|p| (format!("{}|{}|{}", text(p, "ticker"), text(p, "direction"), text(p, "entry_ts")), text(p, "stop_reason")))
        .filter(|(_, r)| !r.is_empty())
        .collect();
    let mut any = false;
    for x in broker_exits(&held, &trades, rows, fills, &stop_reasons) {
        let price = x.price.to_string().parse::<f64>().unwrap_or_default();
        let fill = store::LiveFill {
            id: Uuid::new_v4(),
            session_id: session.id,
            trade_key: x.trade_key.clone(),
            ticker: x.ticker.clone(),
            kind: "exit".into(),
            direction: x.direction.clone(),
            reason: x.reason.clone(),
            bar_ts: String::new(),
            price,
            maker: x.reason == "take_profit",
            quoted: false,
            at: x.at,
        };
        if !store::add_live_fill(&state.pool, &fill).await? {
            continue;
        }
        any = true;
        let f = |d: Decimal| d.to_string().parse::<f64>().unwrap_or_default();
        let (qty, fees) = (f(x.qty), f(x.fees));
        let sign = if x.direction == "long" { 1.0 } else { -1.0 };
        let entry = x.entry_price.map(f);
        let pnl = entry.map(|e| sign * (price - e) * qty * mult - fees);
        let notional = entry.map(|e| e * qty * mult).filter(|n| *n != 0.0);
        let trade = json!({
            "ticker": x.ticker,
            "entry_ts": x.entry_ts,
            "exit_ts": x.at.format(&time::format_description::well_known::Rfc3339).unwrap_or_default(),
            "entry_price": entry,
            "exit_price": price,
            "qty": qty,
            "direction": x.direction,
            "exit_reason": x.reason,
            "pnl": pnl,
            "fees": fees,
            "return_pct": pnl.zip(notional).map(|(p, n)| p / n * 100.0),
            "live": true,
            "at_broker": true,
        });
        let tick = super::Tick { skipped: false, bars: 0, inputs_hash: String::new(), stats: None, opened: Vec::new(), closed: vec![trade], error: String::new() };
        let book = super::book(state, session).await;
        super::record(state, session, &tick, &book).await?;
        super::deliver(state, session, OffsetDateTime::now_utc(), &book).await;
    }
    if any {
        super::live::poke();
    }
    Ok(())
}

/// How long after its close a trade priced from its fills is read again: a venue may publish
/// a late fill or its commission after the fact (IB's commission report).
const REPRICE_FOR: time::Duration = time::Duration::days(1);

/// A broker robot's closed trades priced from what its own orders filled at the venue, fees
/// included: its money is what the broker did, not what the engine simulated. A trade is
/// priced once its exits filled its whole entry and every fee reads in the instrument's
/// currency; until then the engine's figure stands. A position the user closed at the broker
/// is priced from the venue's executions of that close ([`outside_fills`]) once they are in.
async fn price_from_fills(state: &AppState, session: &store::Session, mult: Decimal) -> anyhow::Result<()> {
    let fills = exec_store::fills(&state.pool, session.id).await?;
    if fills.is_empty() {
        return Ok(());
    }
    let orders = exec_store::orders(&state.pool, session.id).await?;
    let now = OffsetDateTime::now_utc();
    let book = store::list_trades(&state.pool, session.id, false).await?;
    let trades: Vec<&store::Trade> = book
        .iter()
        .filter(|t| !t.open && !t.trade_key.contains("|pending:") && t.robot_qty.is_none_or(|q| !q.is_zero()))
        .filter(|t| !t.money_at_broker || t.closed_at.is_some_and(|c| now - c < REPRICE_FOR))
        .collect();
    if trades.is_empty() {
        return Ok(());
    }
    // Per ticker: the instrument's currency, its base (a pair's first asset) and decimals.
    let mut facts: HashMap<String, (String, String, u32)> = HashMap::new();
    for &id in &session.dataset_ids {
        let Some(ds) = otw_store::histdata::get_dataset(&state.pool, id).await? else { continue };
        let meta = crate::instrument_meta_api::resolved(&state.pool, &ds.provider, &ds.asset_type, &ds.ticker).await.ok().flatten();
        // A crypto pair with no stated currency is quoted in the asset its symbol ends with.
        let ccy = meta
            .and_then(|m| m.currency)
            .filter(|c| !c.trim().is_empty())
            .or_else(|| (ds.asset_type == "crypto").then(|| super::pair_quote(&ds.ticker)).flatten().map(String::from))
            .unwrap_or_default()
            .to_uppercase();
        let base = ds.ticker.to_uppercase().strip_suffix(ccy.as_str()).filter(|b| !b.is_empty() && !ccy.is_empty()).unwrap_or_default().to_string();
        let dp = super::money_dp(Some(ccy.as_str()).filter(|c| !c.is_empty()), &ds.asset_type);
        facts.insert(ds.ticker.clone(), (ccy, base, dp));
    }
    let mut days = HashSet::new();
    for t in trades {
        let Some(entry) = opening_order(&orders, &book, t).filter(|o| o.outcome == "sent") else { continue };
        let exits: Vec<&exec_store::Order> = orders.iter().filter(|o| o.position == entry.client_id && matches!(o.role.as_str(), "stop" | "target" | "close")).collect();
        let Some((ccy, base, dp)) = facts.get(&t.ticker) else { continue };
        // A fee in a third currency (IB's in the account's, BNB at Binance) at the day's FX rate.
        let mut rates: HashMap<String, Decimal> = HashMap::new();
        let ids: HashSet<&str> = exits.iter().map(|o| o.client_id.as_str()).chain([entry.client_id.as_str()]).collect();
        for f in fills.iter().filter(|f| ids.contains(f.client_id.as_str()) && !f.fee.is_zero()) {
            let c = f.fee_currency.to_uppercase();
            if c.is_empty() || c == *ccy || c == *base || rates.contains_key(&c) || ccy.is_empty() {
                continue;
            }
            if let Ok(Some(r)) = otw_store::journal_fx::convert(&state.pool, 1.0, &c, ccy, f.at.date()).await {
                if let Ok(r) = otw_store::paper_broker::decimal_of(r) {
                    rates.insert(c, r);
                }
            }
        }
        let Some((pnl, fees)) = trade_money(entry, &exits, &fills, mult, ccy, base, &rates) else { continue };
        // The prices shown are the venue's fills, and an exit outside the engine is its own.
        if let Some(b) = broker_fills(entry, &exits, &fills) {
            store::set_broker_exit(&state.pool, session.id, &t.trade_key, &b).await?;
        }
        if store::set_broker_money(&state.pool, session.id, &t.trade_key, pnl.round_dp(*dp), fees.round_dp(*dp)).await? {
            if let Some(c) = t.closed_at {
                days.insert(c.date());
            }
        }
    }
    for day in days {
        otw_store::robots_guards::refresh_realized(&state.pool, session.id, day).await?;
    }
    Ok(())
}

/// A round trip as the venue filled it: the average entry and exit prices of its fills and,
/// when a close outside the engine took part (Stop and flatten, the user's close at the
/// broker), that exit's time and reason too, since the engine's are not what happened.
/// `None` until the exits filled the whole entry. Pure.
pub fn broker_fills(entry: &exec_store::Order, exits: &[&exec_store::Order], fills: &[exec_store::Fill]) -> Option<Value> {
    let ins: Vec<&exec_store::Fill> = fills.iter().filter(|f| f.client_id == entry.client_id).collect();
    let outs: Vec<&exec_store::Fill> = fills.iter().filter(|f| exits.iter().any(|o| o.client_id == f.client_id)).collect();
    let qty_in: Decimal = ins.iter().map(|f| f.qty).sum();
    let qty_out: Decimal = outs.iter().map(|f| f.qty).sum();
    if qty_in.is_zero() || qty_out != qty_in {
        return None;
    }
    let avg = |fs: &[&exec_store::Fill], q: Decimal| (fs.iter().map(|f| f.qty * f.price).sum::<Decimal>() / q).normalize().to_string().parse::<f64>().ok();
    let mut out = json!({ "entry_price": avg(&ins, qty_in)?, "exit_price": avg(&outs, qty_out)? });
    if let Some(o) = exits.iter().find(|o| outside_close(o) && o.filled_qty > Decimal::ZERO) {
        let at = outs.iter().map(|f| f.at).max()?;
        out["exit_ts"] = json!(at.format(&time::format_description::well_known::Rfc3339).ok()?);
        out["exit_reason"] = json!(if o.reason.as_deref() == Some(CLOSED_OUTSIDE) { "closed_outside" } else { "flatten" });
    }
    Some(out)
}

/// One round trip's money from its fills: what the exits received less what the entry paid
/// (signed by side, times the contract multiplier), less every fee, and the fees alone. A fee
/// in the instrument's currency (or unnamed) is taken as is, one in the pair's base at the
/// fill's price, another through `rates` (one unit in the instrument's currency). `None` until
/// the exits filled the whole entry, or while a fee reads in no known currency. Pure.
pub fn trade_money(
    entry: &exec_store::Order,
    exits: &[&exec_store::Order],
    fills: &[exec_store::Fill],
    mult: Decimal,
    ccy: &str,
    base: &str,
    rates: &HashMap<String, Decimal>,
) -> Option<(Decimal, Decimal)> {
    let ins: Vec<&exec_store::Fill> = fills.iter().filter(|f| f.client_id == entry.client_id).collect();
    let outs: Vec<&exec_store::Fill> = fills.iter().filter(|f| exits.iter().any(|o| o.client_id == f.client_id)).collect();
    let qty_in: Decimal = ins.iter().map(|f| f.qty).sum();
    let qty_out: Decimal = outs.iter().map(|f| f.qty).sum();
    if qty_in.is_zero() || qty_out != qty_in {
        return None;
    }
    let mut gross = Decimal::ZERO;
    let mut fees = Decimal::ZERO;
    for f in ins.iter().chain(outs.iter()) {
        let cash = f.qty * f.price * mult;
        gross += if f.side == "sell" { cash } else { -cash };
        if f.fee.is_zero() {
            continue;
        }
        let c = f.fee_currency.to_uppercase();
        fees += if c.is_empty() || c == ccy.to_uppercase() {
            f.fee
        } else if !base.is_empty() && c == base.to_uppercase() {
            f.fee * f.price
        } else {
            f.fee * *rates.get(&c)?
        };
    }
    Some((gross - fees, fees))
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
    // Not sent is not taken: written refused, so the robot's book holds nothing of it.
    if let Some(why) = why_not {
        let reason = format!("the {} entry on {ticker} decided at the close of {} was not sent: {why}", dir(e.long), e.signal_ts);
        p.notes.push(reason.clone());
        p.actions.push(refusal(session, &intent, Role::Entry, e.long, e.qty, e.price, reason));
        return;
    }
    if let Some(why) = &r.guard_hold {
        let reason = format!("the {} entry on {ticker} decided at the close of {} was not taken: the bot is {why}", dir(e.long), e.signal_ts);
        p.notes.push(reason.clone());
        p.actions.push(refusal(session, &intent, Role::Entry, e.long, e.qty, e.price, reason));
        return;
    }
    let qty = match sized(p, ticker, e.qty, r) {
        Size::Send(q) => q,
        Size::Refused(why) => {
            p.actions.push(refusal(session, &intent, Role::Entry, e.long, e.qty, e.price, why));
            return;
        }
    };
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
            // Moved by the user at the broker: theirs from now on, never touched.
            if let Some(at) = act.moved.get(&l.client_id) {
                p.notes.push(format!("the {} of the {ticker} position was moved at the broker to {}: it stays there", if stop_leg { "stop" } else { "target" }, at.normalize()));
                continue;
            }
            if act.orders_unread {
                continue;
            }
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
            match store::broker_robots(&state.pool).await {
                Ok(robots) => pass(&state, &robots).await,
                Err(e) => tracing::error!("robots exec: listing broker robots: {e:#}"),
            }
            tokio::select! {
                _ = tokio::time::sleep(CYCLE) => {}
                _ = POKE.notified() => {}
            }
        }
    });
}

/// Orders one pass may send to one provider. A burst spreads over several cycles instead of
/// tripping a ban; exits and closes are never held back by it (they take risk off), entries
/// wait for the next pass.
pub const SENDS_PER_PASS: usize = 10;

/// What one pass may still send to one provider.
#[derive(Debug, Clone)]
pub struct Budget {
    pub left: usize,
}

impl Budget {
    pub fn new(n: usize) -> Self {
        Self { left: n }
    }

    /// No limit: one robot synced alone.
    #[cfg(test)]
    pub fn unlimited() -> Self {
        Self { left: usize::MAX }
    }

    /// Whether this send may go: an exit always, an entry while the budget lasts.
    pub fn spend(&mut self, opens: bool) -> bool {
        if opens && self.left == 0 {
            return false;
        }
        self.left = self.left.saturating_sub(1);
        true
    }
}

/// The robots of one pass in the order they are served: by priority, grouped by the provider
/// whose limits they share, each group in the order its first robot comes. Pure.
pub fn groups(robots: &[store::BrokerRobot]) -> Vec<(String, Vec<Uuid>)> {
    let mut sorted: Vec<&store::BrokerRobot> = robots.iter().collect();
    sorted.sort_by_key(|r| r.priority);
    let mut out: Vec<(String, Vec<Uuid>)> = Vec::new();
    for r in sorted {
        match out.iter_mut().find(|(p, _)| *p == r.broker) {
            Some((_, ids)) => ids.push(r.id),
            None => out.push((r.broker.clone(), vec![r.id])),
        }
    }
    out
}

/// What a provider on hold means for a robot, said once. The venue's limit is the account's
/// plan: nothing here can raise it, so the sentence names what the user can change.
pub fn rate_note(provider: &str, until: OffsetDateTime) -> String {
    format!(
        "waiting for {provider} rate limit until {}: the broker refused more requests. If this keeps happening, the account's plan allows fewer requests than its bots need: put fewer bots or instruments on it, or raise the plan's limit.",
        until.format(&time::format_description::well_known::Rfc3339).unwrap_or_default()
    )
}

/// How long a refusal without a Retry-After holds a provider's robots: one sync pass.
pub const UNTIMED_HOLD: Duration = Duration::from_secs(15);

/// How long `provider`'s robots wait: the Retry-After it sent, else what is left of
/// [`UNTIMED_HOLD`] since it last refused a request for its rate limit.
pub fn provider_hold(retry_after: Option<Duration>, limited_since: Option<Duration>) -> Option<Duration> {
    retry_after.or_else(|| limited_since.and_then(|ago| UNTIMED_HOLD.checked_sub(ago)).filter(|d| !d.is_zero()))
}

/// One pass over every broker robot: provider by provider, highest priority first. A provider
/// that asked to be left alone (a Retry-After, or a refusal in the last pass) skips its whole group, each robot told
/// once, and the pass goes on with the others: no robot waits on another venue's limit.
pub(crate) async fn pass(state: &AppState, robots: &[store::BrokerRobot]) {
    for step in plan_pass(robots, |p| provider_hold(crate::rate::retry_after(p), crate::rate::limited_since(p))) {
        match step {
            PassStep::Hold { provider, ids, wait } => {
                let until = OffsetDateTime::now_utc() + time::Duration::seconds(wait.as_secs() as i64 + 1);
                for id in ids {
                    if let Err(e) = hold_for_rate(state, id, &provider, until).await {
                        tracing::warn!("robots exec: {id}: {e:#}");
                    }
                }
            }
            PassStep::Sync { ids, .. } => {
                let mut budget = Budget::new(SENDS_PER_PASS);
                for id in ids {
                    if let Err(e) = sync_with(state, id, &mut budget).await {
                        tracing::warn!("robots exec: robot {id}: {e:#}");
                    }
                }
            }
        }
    }
}

/// What a pass does with one provider's robots.
#[derive(Debug, Clone, PartialEq)]
pub enum PassStep {
    /// Synced in this order, sharing one send budget.
    Sync { provider: String, ids: Vec<Uuid> },
    /// The provider asked to be left alone for `wait`: nothing is read or sent.
    Hold { provider: String, ids: Vec<Uuid>, wait: Duration },
}

/// The pass, planned: the provider groups in priority order, each synced or held by the
/// provider's rate limit as `retry_after` reads it. Pure.
pub fn plan_pass(robots: &[store::BrokerRobot], retry_after: impl Fn(&str) -> Option<Duration>) -> Vec<PassStep> {
    groups(robots)
        .into_iter()
        .map(|(provider, ids)| match retry_after(&provider) {
            Some(wait) => PassStep::Hold { provider, ids, wait },
            None => PassStep::Sync { provider, ids },
        })
        .collect()
}

/// A robot whose provider is on hold: its note says until when, once; nothing is read or sent.
async fn hold_for_rate(state: &AppState, id: Uuid, provider: &str, until: OffsetDateTime) -> anyhow::Result<()> {
    let Some(session) = store::get_session(&state.pool, id).await? else { return Ok(()) };
    if session.exec_note.starts_with(&format!("waiting for {provider} rate limit")) {
        return Ok(());
    }
    let note = rate_note(provider, until);
    exec_store::set_state(&state.pool, id, &session.exec_state, &note, None).await?;
    if session.armed_at.is_some() {
        say(state, &session, "waiting for the broker", &note).await;
    }
    Ok(())
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
    use otw_store::histdata as hd;
    let mut out = Vec::new();
    for &id in &session.dataset_ids {
        let ds = hd::get_dataset(&state.pool, id).await?.ok_or_else(|| anyhow::anyhow!("a dataset of this session no longer exists"))?;
        let key = key_of(&ds.provider, &ds.ticker);
        let symbol = ex.symbols.get(&key).cloned().ok_or_else(|| anyhow::anyhow!("{key} has no broker symbol: start the session again"))?;
        let tf_secs = crate::histdata::timeframe_secs(&ds.timeframe)?;
        let conn = super::data_connector_for(&state.pool, session, &ds).await?;
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

/// The robot's working stops and targets on one instrument whose price at the broker is not the
/// one the robot last sent: the user moved them there. Client id, the broker's price. Matched by
/// the venue's id; an exit the broker does not list, or lists without that price, is not judged.
/// Pure.
pub fn moved_exits(rows: &[exec_store::Order], symbol: &str, at_broker: &[brokers::OpenOrder]) -> HashMap<String, Decimal> {
    let mut out = HashMap::new();
    for o in rows.iter().filter(|o| o.symbol.eq_ignore_ascii_case(symbol) && o.outcome == "sent" && status_of(o).0 == "working") {
        let (sent, stop_leg) = match o.role.as_str() {
            "stop" => (o.stop_price, true),
            "target" => (o.limit_price, false),
            _ => continue,
        };
        let (Some(sent), Some(venue)) = (sent, o.venue_id.as_deref()) else { continue };
        let Some(b) = at_broker.iter().find(|b| b.id == venue) else { continue };
        let Some(now) = (if stop_leg { b.stop_price } else { b.limit_price }) else { continue };
        let Ok(now) = otw_store::paper_broker::decimal_of(now) else { continue };
        // The venue echoes a price on its own grid: a difference under a millionth is the same level.
        if (now - sent).abs() > sent.abs() * Decimal::new(1, 6) {
            out.insert(o.client_id.clone(), now);
        }
    }
    out
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
                if let Err(e) = exec_store::set_status(&state.pool, account, &o.client_id, word, filled_qty.normalize(), Some(id.as_str()).filter(|i| !i.is_empty()), fills_as.as_deref()).await {
                    problems.push(format!("{e:#}"));
                }
            }
            Ok(Lookup::NotFound) => {}
            Err(e) => problems.push(format!("order {} could not be looked up: {e:#}", o.client_id)),
        }
    }
    problems
}

/// A close row written for the user's own close at the broker whose fills are not all in yet:
/// a day at most, the venue lists its executions by then or never.
fn awaits_fills(o: &exec_store::Order, fills: &[exec_store::Fill]) -> bool {
    o.reason.as_deref() == Some(CLOSED_OUTSIDE)
        && OffsetDateTime::now_utc() - o.created_at < time::Duration::days(1)
        && fills.iter().filter(|f| f.client_id == o.client_id).map(|f| f.qty).sum::<Decimal>() < o.qty
}

/// The robot's book set to what it did at the broker, from the store alone: the exits the
/// engine did not decide, the entries it did not take, the closed trades priced from their
/// fills. Run by every sync, the disarmed robot's too.
async fn reconcile(state: &AppState, session: &store::Session, rows: &[exec_store::Order], fills: &[exec_store::Fill]) {
    let settings_bt: Option<Settings> = super::own::settings_of(state, session).await.ok().map(|(s, _)| s);
    let mult = settings_bt.as_ref().map(|s| s.instrument.multiplier).filter(|m| *m > 0.0).unwrap_or(1.0);
    if let Err(e) = record_broker_exits(state, session, rows, fills, mult).await {
        tracing::warn!("robots exec: writing the broker exits of {}: {e:#}", session.name);
    }
    if let Err(e) = untake_unsent(state, session.id).await {
        tracing::warn!("robots exec: writing the trades {} did not take: {e:#}", session.name);
    }
    let mult = otw_store::paper_broker::decimal_of(mult).unwrap_or(Decimal::ONE);
    if let Err(e) = price_from_fills(state, session, mult).await {
        tracing::warn!("robots exec: pricing the closed trades of {} from their fills: {e:#}", session.name);
    }
}

/// The fills of the user's own close at the broker, written to the close row the robot wrote
/// for it ([`close_outside`]): the venue's executions on the instrument, on the close's side,
/// after the robot's last own fill there and before the close was seen, that are none of the
/// robot's orders, up to the row's size. Until they are in, the trade it closed stays open:
/// its exit is the venue's price, never a guess.
async fn outside_fills(state: &AppState, connector: &dyn Broker, settings: &HashMap<String, String>, session: Uuid, account: Uuid, rows: &[exec_store::Order]) -> anyhow::Result<()> {
    let known = exec_store::fills(&state.pool, session).await?;
    let waiting: Vec<&exec_store::Order> = rows.iter().filter(|o| awaits_fills(o, &known)).collect();
    if waiting.is_empty() || !connector.capability().executions {
        return Ok(());
    }
    let same = |a: &str, b: &str| {
        let bare = |s: &str| s.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_uppercase();
        bare(a) == bare(b)
    };
    let mut taken: HashSet<String> = known.iter().map(|f| f.exec_id.clone()).collect();
    for o in waiting {
        let since = known.iter().filter(|f| f.symbol == o.symbol && f.client_id != o.client_id).map(|f| f.at).max().unwrap_or(o.created_at - time::Duration::days(1));
        let seen = o.created_at + time::Duration::minutes(1);
        let q = brokers::ExecQuery { from: since - FILL_OVERLAP, to: seen, symbols: vec![o.symbol.clone()] };
        let mut execs = connector.executions(&brokers::client()?, settings, &q).await?;
        execs.sort_by_key(|f| f.at);
        let mut left = o.qty - known.iter().filter(|f| f.client_id == o.client_id).map(|f| f.qty).sum::<Decimal>();
        for f in execs {
            if left <= Decimal::ZERO {
                break;
            }
            if f.side != o.side || f.at < since || f.at > seen || !same(&f.symbol, &o.symbol) || taken.contains(&f.id) || order_of(rows, &f.order_id, &f.client_id).is_some() {
                continue;
            }
            let qty = otw_store::paper_broker::decimal_of(f.qty)?;
            if qty <= Decimal::ZERO {
                continue;
            }
            let part = qty.min(left);
            let fee = otw_store::paper_broker::decimal_of(f.fee).unwrap_or_default() * part / qty;
            let fill = exec_store::Fill {
                session_id: session,
                account_id: account,
                exec_id: f.id.clone(),
                client_id: o.client_id.clone(),
                symbol: o.symbol.clone(),
                side: o.side.clone(),
                qty: part,
                price: otw_store::paper_broker::decimal_of(f.price)?,
                fee,
                fee_currency: f.fee_currency.clone(),
                at: f.at,
            };
            exec_store::add_fill(&state.pool, &fill).await?;
            taken.insert(f.id);
            left -= part;
        }
    }
    Ok(())
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
        let Some(o) = order_of(rows, &f.order_id, &f.client_id) else { continue };
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

/// The session's order a broker fill names: by the client id the broker echoes with the fill
/// (IB's `orderRef`), else by the broker's order id: the id it is followed by (whole, or past
/// `SYMBOL:`), the number the lookup read, or its client id.
fn order_of<'a>(rows: &'a [exec_store::Order], order_id: &str, client_id: &str) -> Option<&'a exec_store::Order> {
    if let Some(o) = rows.iter().find(|o| !client_id.is_empty() && o.client_id == client_id) {
        return Some(o);
    }
    if order_id.is_empty() {
        return None;
    }
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
    crate::robots::deliver_events(state, session).await;
}

/// Sync one session: read both books, converge, record what was found. A session with no
/// broker target answers at once. The cycle goes through [`pass`]; this is one robot alone,
/// with no send budget.
#[cfg(test)]
pub async fn sync(state: &AppState, id: Uuid) -> anyhow::Result<()> {
    sync_with(state, id, &mut Budget::unlimited()).await
}

/// [`sync`], spending the pass's send budget for the robot's provider.
pub async fn sync_with(state: &AppState, id: Uuid, budget: &mut Budget) -> anyhow::Result<()> {
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
    // A robot stopped by an act at the broker it cannot follow keeps saying so until it is
    // armed again.
    let stopped = !armed && session.exec_state == "stopped";
    // The fills of a close the user made at the broker are read until they are all in.
    let known = exec_store::fills(&state.pool, id).await.unwrap_or_default();
    let awaiting = rows.iter().any(|o| awaits_fills(o, &known));
    if !armed && quiet && own.values().all(|q| q.is_zero()) && !awaiting {
        if !stopped {
            exec_store::set_state(&state.pool, id, "idle", "", None).await?;
        }
        // What the broker did is the robot's book, armed or not: a flatten, a native exit, an
        // entry that never left.
        reconcile(state, &session, &rows, &known).await;
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
            Err(e) => return unanswered(state, &session, &format!("{e:#}")).await,
        }
    }
    if let Ok(mut m) = UNANSWERED.lock() {
        m.remove(&id);
    }
    let problems = look_up(state, connector.as_ref(), &settings, account_id, &rows, &inst).await;
    // Read again before the fills: the lookups just wrote the number a fill may be named by.
    let rows = exec_store::orders(&state.pool, id).await?;
    if let Err(e) = read_fills(state, connector.as_ref(), &settings, id, account_id, &rows, &inst).await {
        tracing::warn!("paper exec: reading fills of {}: {e:#}", session.name);
    }
    if let Err(e) = outside_fills(state, connector.as_ref(), &settings, id, account_id, &rows).await {
        tracing::warn!("robots exec: reading the fills of a close made outside {}: {e:#}", session.name);
    }
    let fills = exec_store::fills(&state.pool, id).await.unwrap_or_default();
    let settings_bt: Option<Settings> = super::own::settings_of(state, &session).await.ok().map(|(s, _)| s);
    // A position the broker took out by an exit the engine did not decide (its native stop or
    // target, Stop and flatten, the user's close) is out: the robot writes that exit as the
    // venue filled it, before it reads what the engine holds.
    {
        let mult = settings_bt.as_ref().map(|s| s.instrument.multiplier).filter(|m| *m > 0.0).unwrap_or(1.0);
        if let Err(e) = record_broker_exits(state, &session, &rows, &fills, mult).await {
            tracing::warn!("robots exec: writing the broker exits of {}: {e:#}", session.name);
        }
    }
    // The broker's working orders: an exit the user moved there is told from the robot's own
    // level by its price. Unread, no exit is moved this pass.
    let at_broker: Option<Vec<brokers::OpenOrder>> = if !connector.capability().orders {
        Some(Vec::new())
    } else {
        match brokers::client() {
            Ok(http) => match connector.orders(&http, &settings).await {
                Ok(o) => Some(o),
                Err(e) => {
                    tracing::warn!("robots exec: {} could not read the broker's working orders: {e:#}", session.name);
                    None
                }
            },
            Err(_) => None,
        }
    };
    let live = store::list_live_fills(&state.pool, id).await.unwrap_or_default();
    let halts =otw_store::broker_risk::halts(&state.pool, Some(account_id), OffsetDateTime::now_utc()).await.unwrap_or_default();
    let caps = connector.order_caps();
    let tick = settings_bt.as_ref().map(|s| s.instrument.tick_size).filter(|t| *t > 0.0).and_then(|t| otw_store::paper_broker::decimal_of(t).ok());
    // The robot's layers: its capital policy, its guards, what it holds and sent today.
    let now = OffsetDateTime::now_utc();
    let guard = guards::State::of(&session.guard_state);
    // An exclusion period holds entries the way a guard does: exits are still managed.
    let guard_hold = guard.holds(now).or_else(|| super::excluded(&session, now));
    let capital = sizing::Capital::of(&session.capital);
    let open_positions = own.values().filter(|q| !q.is_zero()).count() as i64;
    let orders_today = otw_store::robots_guards::day(&state.pool, id, now.date()).await.map(|d| d.orders_sent as i64).unwrap_or_default();

    let mult = settings_bt.as_ref().map(|s| s.instrument.multiplier).filter(|m| *m > 0.0).and_then(|m| otw_store::paper_broker::decimal_of(m).ok()).unwrap_or(Decimal::ONE);
    let leverage = capital.leverage.filter(|l| *l > Decimal::ZERO).unwrap_or(Decimal::ONE);
    let mut stops: Vec<String> = Vec::new();
    let mut notes: Vec<String> = problems;
    let mut degraded: Vec<String> = Vec::new();
    let mut blocked_any = false;
    let mut book = Vec::new();
    for i in &inst {
        let again = held_qty(connector.as_ref(), &settings, i).await.ok();
        let held = first.get(&i.symbol).copied().filter(|q| again == Some(*q));
        let baseline = session.exec_baseline.get(&i.symbol).and_then(|v| v.as_str()).and_then(|s| s.parse().ok()).unwrap_or_default();
        let adopted = session.exec_baseline.get("adopted").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
        let act = Actual {
            held,
            baseline,
            legs: legs_of(&rows, &i.symbol),
            adopted,
            withheld: withheld(&fills, &i.symbol),
            moved: at_broker.as_deref().map(|b| moved_exits(&rows, &i.symbol, b)).unwrap_or_default(),
            orders_unread: at_broker.is_none(),
        };
        let eng = match &settings_bt {
            Some(s) => intended(session.stats.as_ref(), &i.ticker, &live, s, i.tf_secs),
            None => Intended::default(),
        };
        let blocked = !otw_store::broker_orders::unknown_on(&state.pool, account_id, &i.ticker).await.unwrap_or_default().is_empty();
        blocked_any |= blocked;
        let wants_size = eng.entry.is_some() || !eng.limits.is_empty();
        let step = if armed && wants_size { connector.qty_step(&settings, &i.ticker).await.ok().flatten() } else { None };
        // The venue's grid: its smallest order, and its price tick where the strategy states
        // none (a stop or a target off the tick is refused by the venue, whole ticket with it).
        let grid = if armed { connector.grid(&settings, Some(&i.provider), &i.ticker, &i.asset_type).await.ok().flatten() } else { None };
        let min_qty = grid.as_ref().filter(|_| wants_size).and_then(|g| g.market_min_qty.or(g.min_qty));
        let tick = tick.or_else(|| grid.as_ref().and_then(|g| g.tick).filter(|t| *t > Decimal::ZERO));
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
            min_qty,
            capital: capital.clone(),
            scale: guard.scale(),
            guard_hold: guard_hold.clone(),
            open_positions,
            orders_today,
        };
        let plan = plan(id, &i.ticker, &eng, &act, &rules);
        if let Some(d) = &plan.degraded {
            degraded.push(d.clone());
        }
        if let Some(q) = plan.closed_outside {
            if let Err(e) = close_outside(state, &session, account_id, connector.as_ref(), &settings, i, &act, &rows, q).await {
                tracing::warn!("robots exec: writing the position {} closed outside: {e:#}", session.name);
            }
        }
        if armed {
            if let Some(why) = &plan.stop {
                stops.push(why.clone());
            }
        }
        notes.extend(plan.notes.iter().cloned());
        // Once the robot stops, nothing more leaves for it in this pass.
        let actions = if stops.is_empty() { afford(state, connector.as_ref(), &settings, i, mult, leverage, plan.actions.clone()).await } else { Vec::new() };
        let out = act_on(state, &session, account_id, connector.as_ref(), &settings, i, caps.as_ref(), &actions, &rows, budget).await;
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
                    "moved_to": act.moved.get(&l.client_id),
                })).collect::<Vec<_>>(),
            },
            "halted": rules.halted,
            "blocked": blocked,
        }));
    }
    if !stops.is_empty() {
        return stop(state, &session, account_id, connector.as_ref(), &settings, &rows, &stops.join(" "), &json!({ "instruments": book })).await;
    }
    let (state_word, mut note) = if blocked_any {
        ("blocked", "an order got no answer from the broker: nothing is sent on its instrument until it is settled. Check it at the broker.".to_string())
    } else if !degraded.is_empty() {
        ("degraded", degraded.join(" "))
    } else if !halts.is_empty() && armed {
        ("paused", "trading is halted on the account: the session sends only what takes risk off until the halt is lifted".to_string())
    } else if let Some(why) = guard_hold.as_ref().filter(|_| armed) {
        ("paused", format!("the bot is {why}: it takes no new entry, its exits are still managed"))
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
    if let Err(e) = untake_unsent(state, id).await {
        tracing::warn!("robots exec: writing the refused trades of {}: {e:#}", session.name);
    }
    if let Err(e) = price_from_fills(state, &session, mult).await {
        tracing::warn!("robots exec: pricing the closed trades of {} from their fills: {e:#}", session.name);
    }
    if stopped {
        return note_state(state, &session, "stopped", &session.exec_note, Some(&json!({ "instruments": book }))).await;
    }
    note_state(state, &session, state_word, &note, Some(&json!({ "instruments": book }))).await
}

/// The account could not be read: nothing is sent, since nothing may leave before it is. A
/// passing outage is not news; [`UNREACHABLE_AFTER`] syncs in a row, the robot says it once.
async fn unanswered(state: &AppState, session: &store::Session, err: &str) -> anyhow::Result<()> {
    let n = UNANSWERED
        .lock()
        .map(|mut m| {
            let n = m.entry(session.id).or_default();
            *n += 1;
            *n
        })
        .unwrap_or(UNREACHABLE_AFTER);
    tracing::warn!("robots exec: {} could not read its account ({n} in a row): {err}", session.name);
    if n < UNREACHABLE_AFTER || session.armed_at.is_none() || session.exec_state == "unreachable" {
        return Ok(());
    }
    let note = format!(
        "the broker did not answer {n} syncs in a row ({err}): nothing is sent until it answers again, and the account is read before anything is"
    );
    note_state(state, session, "unreachable", &note, None).await
}

/// The user acted at the broker in a way the robot cannot follow: it is disarmed, its resting
/// entries are cancelled (nobody would manage what they fill), and it says why. What it holds
/// keeps its exits at the broker. Arming it again measures the account anew.
#[allow(clippy::too_many_arguments)]
async fn stop(
    state: &AppState,
    session: &store::Session,
    account: Uuid,
    connector: &dyn Broker,
    settings: &HashMap<String, String>,
    rows: &[exec_store::Order],
    why: &str,
    book: &Value,
) -> anyhow::Result<()> {
    exec_store::disarm(&state.pool, session.id).await?;
    if let Ok(Some(account_row)) = otw_store::brokers::get(&state.pool, account).await {
        let source = Source::Strategy(session.id);
        for o in rows.iter().filter(|o| matches!(o.role.as_str(), "entry" | "limit") && o.outcome == "sent" && status_of(o).0 == "working") {
            if let Some(v) = &o.venue_id {
                if let Err(e) = cancel(state, &account_row, connector, settings, source, v).await {
                    tracing::warn!("robots exec: cancelling a resting entry of the stopped {}: {}", session.name, e.message());
                }
            }
        }
    }
    crate::security_events::log("strategy_stopped", &format!("strategy “{}” stopped: {why}", session.name));
    note_state(state, session, "stopped", why, Some(book)).await
}

/// The robot's position the user closed at the broker: written closed outside the robot (a
/// close row filled at its size), so the robot holds nothing, and its exits, which have
/// nothing left to protect, are cancelled.
#[allow(clippy::too_many_arguments)]
async fn close_outside(
    state: &AppState,
    session: &store::Session,
    account: Uuid,
    connector: &dyn Broker,
    settings: &HashMap<String, String>,
    inst: &Instrument,
    act: &Actual,
    rows: &[exec_store::Order],
    own: Decimal,
) -> anyhow::Result<()> {
    let position = act.legs.iter().rev().find(|l| l.role.opens() && l.filled > Decimal::ZERO).map(|l| l.client_id.clone()).unwrap_or_default();
    if let Some(account_row) = otw_store::brokers::get(&state.pool, account).await? {
        let source = Source::Strategy(session.id);
        let exits = rows.iter().filter(|o| o.symbol == inst.symbol && matches!(o.role.as_str(), "stop" | "target" | "close") && o.outcome == "sent" && status_of(o).0 == "working");
        for o in exits {
            if let Some(v) = &o.venue_id {
                if let Err(e) = cancel(state, &account_row, connector, settings, source, v).await {
                    tracing::warn!("robots exec: cancelling an exit of a position closed outside {}: {}", session.name, e.message());
                }
            }
        }
    }
    let at = OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339).unwrap_or_default();
    let intent = format!("close|{position}|outside|{at}");
    let id = client_id(session.id, &intent, Role::Close);
    let new = exec_store::NewOrder {
        client_id: id.clone(),
        intent,
        role: Role::Close.word().into(),
        ticker: inst.ticker.clone(),
        symbol: inst.symbol.clone(),
        side: if own > Decimal::ZERO { "sell".into() } else { "buy".into() },
        qty: own.abs(),
        limit_price: None,
        stop_price: None,
        engine_qty: None,
        engine_price: None,
        position,
    };
    if exec_store::claim(&state.pool, session.id, account, std::slice::from_ref(&new)).await? {
        exec_store::set_outcome(&state.pool, account, &id, "sent", Some(CLOSED_OUTSIDE), None).await?;
        exec_store::set_status(&state.pool, account, &id, "filled", own.abs(), None, None).await?;
    }
    Ok(())
}

/// The reason a close row written for a position the user closed at the broker carries.
pub const CLOSED_OUTSIDE: &str = "closed at the broker outside the bot";

/// Whether an action opens exposure.
fn opening(a: &Action) -> bool {
    matches!(a, Action::Place(legs) if legs.first().is_some_and(|l| l.role.opens()))
}

/// The opening orders the account can pay for go; one it cannot is written refused with what
/// it needs and what the account has, so the log says it and the robot's book does not take
/// it. The account is read only when an entry is about to leave; unread, the broker decides.
async fn afford(state: &AppState, connector: &dyn Broker, settings: &HashMap<String, String>, inst: &Instrument, mult: Decimal, leverage: Decimal, actions: Vec<Action>) -> Vec<Action> {
    if !actions.iter().any(opening) {
        return actions;
    }
    let summary = match connector.account(settings, Some(&inst.symbol)).await {
        Ok(v) => v.into_iter().next(),
        Err(e) => {
            tracing::warn!("robots exec: reading what the account can pay for {}: {e:#}", inst.ticker);
            None
        }
    };
    let Some(summary) = summary else { return actions };
    // A margin account states its funds in its own currency: the instrument's is read once.
    let mut fx: Option<Option<Decimal>> = None;
    let mut spent = Decimal::ZERO;
    let mut out = Vec::with_capacity(actions.len());
    for a in actions {
        let Action::Place(legs) = &a else {
            out.push(a);
            continue;
        };
        if !opening(&a) {
            out.push(a);
            continue;
        }
        let l = &legs[0];
        let px = l.limit.or_else(|| l.engine_price.and_then(|p| otw_store::paper_broker::decimal_of(p).ok())).or_else(|| inst.last.and_then(|(p, _)| otw_store::paper_broker::decimal_of(p).ok()));
        let Some(px) = px.filter(|p| *p > Decimal::ZERO) else {
            out.push(a);
            continue;
        };
        let rate = if summary.assets.is_empty() {
            if fx.is_none() {
                fx = Some(funds_rate(state, inst, &summary.currency).await);
            }
            fx.flatten()
        } else {
            Some(Decimal::ONE)
        };
        let Some(rate) = rate else {
            out.push(a);
            continue;
        };
        match shortfall(&summary, l.buy, l.qty, px, mult, leverage, rate, spent) {
            Ok(need) => {
                spent += need;
                out.push(a);
            }
            Err(reason) => {
                let reason = format!("the {} entry on {} was not sent: {reason}", dir(l.buy), inst.ticker);
                out.push(Action::Refuse { leg: l.clone(), reason });
            }
        }
    }
    out
}

/// One unit of the instrument's currency in the account's funds currency: 1 when they are the
/// same, the stored FX rate of today otherwise, `None` when it is unknown (the broker decides).
async fn funds_rate(state: &AppState, inst: &Instrument, funds: &str) -> Option<Decimal> {
    let facts = crate::instrument_meta_api::resolved(&state.pool, &inst.provider, &inst.asset_type, &inst.ticker).await.ok().flatten();
    let ccy = facts.and_then(|f| f.currency)?;
    if funds.is_empty() || ccy.eq_ignore_ascii_case(funds) {
        return Some(Decimal::ONE);
    }
    let today = OffsetDateTime::now_utc().date();
    let r = otw_store::journal_fx::convert(&state.pool, 1.0, &ccy.to_uppercase(), &funds.to_uppercase(), today).await.ok().flatten()?;
    otw_store::paper_broker::decimal_of(r).ok().filter(|r| *r > Decimal::ZERO)
}

/// What an opening order needs from the account, against what it has left after the entries
/// already let through in this sync (`spent`). A spot buy needs its whole cost in the quote
/// asset; a margin order its notional over the robot's leverage, in the account's currency
/// (`rate` converts the instrument's). `Err` names what is missing and the fix. A spot sell, or
/// an account that states no funds, is left to the broker. Pure.
#[allow(clippy::too_many_arguments)]
pub fn shortfall(acct: &brokers::AccountSummary, buy: bool, qty: Decimal, price: Decimal, mult: Decimal, leverage: Decimal, rate: Decimal, spent: Decimal) -> Result<Decimal, String> {
    let dec = |v: f64| otw_store::paper_broker::decimal_of(v).ok();
    let notional = qty * price * mult;
    let (need, has, ccy) = if !acct.assets.is_empty() {
        let Some(quote) = acct.assets.get(1).filter(|_| buy) else { return Ok(Decimal::ZERO) };
        (notional, dec(quote.free), quote.asset.clone())
    } else {
        let lev = if leverage > Decimal::ZERO { leverage } else { Decimal::ONE };
        (notional * rate / lev, acct.available.and_then(dec), acct.currency.clone())
    };
    let Some(has) = has else { return Ok(Decimal::ZERO) };
    let left = has - spent;
    if need <= left {
        return Ok(need);
    }
    Err(format!(
        "the account has {} {ccy} available and the entry needs {} {ccy} ({} at {}{}): add funds to the account or lower the bot's allocation",
        left.max(Decimal::ZERO).round_dp(2).normalize(),
        need.round_dp(2).normalize(),
        qty.normalize(),
        price.normalize(),
        if leverage > Decimal::ONE { format!(", leverage {}", leverage.normalize()) } else { String::new() }
    ))
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
                "error" => "cannot use the broker account",
                "unreachable" => "cannot reach the broker",
                "stopped" => "stopped: arm it again",
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
    budget: &mut Budget,
) -> Done {
    let mut done = Done::default();
    let source = Source::Strategy(session.id);
    let account_row = match otw_store::brokers::get(&state.pool, account).await {
        Ok(Some(a)) => a,
        _ => return done,
    };
    // What takes risk off goes first: cancels, moves, exits and closes, then the entries, in
    // the order the plan gave each.
    let opens = |a: &Action| matches!(a, Action::Place(legs) if legs.first().is_some_and(|l| l.role.opens()));
    let ordered = actions.iter().filter(|a| !opens(a)).chain(actions.iter().filter(|a| opens(a)));
    for a in ordered {
        match a {
            Action::Refuse { leg, reason } => {
                let new = exec_store::NewOrder {
                    client_id: leg.client_id.clone(),
                    intent: leg.intent.clone(),
                    role: leg.role.word().into(),
                    ticker: inst.ticker.clone(),
                    symbol: inst.symbol.clone(),
                    side: if leg.buy { "buy".into() } else { "sell".into() },
                    qty: Decimal::ZERO,
                    limit_price: None,
                    stop_price: None,
                    engine_qty: leg.engine_qty,
                    engine_price: leg.engine_price,
                    position: String::new(),
                };
                if let Ok(true) = exec_store::claim(&state.pool, session.id, account, std::slice::from_ref(&new)).await {
                    let _ = exec_store::set_outcome(&state.pool, account, &leg.client_id, "refused", Some(reason), None).await;
                    refused(state, session, &inst.ticker, leg, reason).await;
                }
            }
            Action::Place(legs) => {
                if done.unknown {
                    break;
                }
                let opening = legs.first().is_some_and(|l| l.role.opens());
                if !budget.spend(opening) {
                    done.notes.push(format!("the entry on {} waits for the next pass: this pass already sent {SENDS_PER_PASS} orders to the broker", inst.ticker));
                    continue;
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
                        if opening {
                            let today = OffsetDateTime::now_utc().date();
                            if let Err(e) = otw_store::robots_guards::bump_orders(&state.pool, session.id, today).await {
                                tracing::error!("robots exec: counting an order: {e:#}");
                            }
                        }
                    }
                    Err(SendRefusal::Refused { message, .. }) => {
                        for l in legs {
                            let _ = exec_store::set_outcome(&state.pool, account, &l.client_id, "refused", Some(&message), None).await;
                        }
                        if opening {
                            refused(state, session, &inst.ticker, &legs[0], &format!("the broker refused it: {message}")).await;
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

/// An opening order that did not leave, or that the broker refused: one line in the robot's
/// log, the exact reason with it, said on its channels.
async fn refused(state: &AppState, session: &store::Session, ticker: &str, leg: &LegSpec, reason: &str) {
    let side = dir(leg.buy);
    let title = format!("{}: {side} {ticker} not sent", session.name);
    let payload = json!({ "ticker": ticker, "direction": side, "intent": leg.intent, "refused": reason });
    if let Err(e) = store::add_event(&state.pool, session.id, "refused", &payload, &title, reason, true).await {
        tracing::error!("robots exec: recording a refusal: {e:#}");
        return;
    }
    crate::robots::deliver_events(state, session).await;
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
/// own lookup (never by name), and the exits mode is one the broker's capabilities hold. Another
/// robot on the same instrument and account is no refusal here: only one is armed at a time.
/// Answers the normalized target; `None` for a simulated session.
pub async fn vet(state: &AppState, raw: Option<&Value>, settings: &Settings, dataset_ids: &[Uuid]) -> Result<Option<Execution>, crate::ApiError> {
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
    ex.target = "broker".into();
    ex.account = account.name.clone();
    ex.broker = account.broker.clone();
    ex.symbols = symbols;
    Ok(Some(ex))
}

/// Before arming: the account must be reachable and hold the robot's money (its allocation,
/// else the strategy's capital). Each robot is read alone: several robots may share one
/// account's equity, and an entry the account cannot pay for later is refused then, said in
/// the log. Refused with the fix when the account cannot be read or holds less.
pub async fn check_funds(state: &AppState, session: &store::Session, connector: &dyn Broker, settings: &HashMap<String, String>) -> Result<(), crate::ApiError> {
    use crate::ApiError;
    let ex = Execution::of(session).ok_or_else(|| ApiError::bad_request("this session simulates"))?;
    let symbol = ex.symbols.values().next().cloned();
    let (bt, _) = super::own::settings_of(state, session).await.map_err(|e| ApiError::bad_request(&format!("{e:#}")))?;
    let money = otw_store::paper_broker::decimal_of(bt.starting_capital).unwrap_or_default();
    let summary = connector
        .account(settings, symbol.as_deref())
        .await
        .map_err(|e| ApiError::bad_request(&format!("the broker could not be reached to read the account, so the bot was not armed: {e:#}")))?
        .into_iter()
        .next()
        .ok_or_else(|| ApiError::bad_request("the broker answered no account figures, so the bot was not armed: check the account at the broker"))?;
    let (has, ccy) = funds_of(&summary);
    let Some(has) = has else {
        return Err(ApiError::bad_request("the broker states neither the account's equity nor its cash, so the bot's capital cannot be checked: it was not armed"));
    };
    // The allocation is in the account's currency unless the policy names another.
    let want = sizing::Capital::of(&session.capital).currency;
    let rate = if want.is_empty() || ccy.is_empty() || want.eq_ignore_ascii_case(&ccy) {
        Decimal::ONE
    } else {
        let today = OffsetDateTime::now_utc().date();
        let r = otw_store::journal_fx::convert(&state.pool, 1.0, &want.to_uppercase(), &ccy.to_uppercase(), today).await.ok().flatten();
        r.and_then(|r| otw_store::paper_broker::decimal_of(r).ok()).ok_or_else(|| {
            ApiError::bad_request(&format!("no {want} to {ccy} rate is stored, so the bot's capital cannot be checked against the account: add the rate in Settings, FX"))
        })?
    };
    match funds_short(money * rate, has, &ccy) {
        Some(why) => Err(ApiError::bad_request(&why)),
        None => Ok(()),
    }
}

/// What the account holds for a robot to work with, and in what: its equity (net liquidation),
/// else what is available, else its cash; on a spot account the pair's quote asset, free and
/// locked. Pure.
pub fn funds_of(a: &brokers::AccountSummary) -> (Option<Decimal>, String) {
    let dec = |v: f64| otw_store::paper_broker::decimal_of(v).ok();
    if let Some(q) = a.assets.get(1) {
        return (dec(q.free + q.locked), q.asset.clone());
    }
    (a.equity.or(a.available).or(a.cash).and_then(dec), a.currency.clone())
}

/// Why the account cannot hold the robot's money, or `None` when it can. Pure.
pub fn funds_short(money: Decimal, has: Decimal, ccy: &str) -> Option<String> {
    (money > has).then(|| {
        format!(
            "the account holds {} {ccy} and the bot works with {} {ccy}: add funds to the account or lower the bot's allocation, then arm it",
            has.round_dp(2).normalize(),
            money.round_dp(2).normalize()
        )
    })
}

/// What the account holds of each instrument that the session's own fills do not explain:
/// measured when the session is armed, so a spot account's holding is not a drift.
pub async fn baseline(state: &AppState, session: &store::Session) -> anyhow::Result<Value> {
    let ex = Execution::of(session).ok_or_else(|| anyhow::anyhow!("this session simulates"))?;
    let account_id = ex.account_id.ok_or_else(|| anyhow::anyhow!("this session names no account"))?;
    let (connector, settings, _) = crate::brokers_api::resolve_for_job(&state.pool, &state.cipher, account_id, MODULE).await?;
    let inst = instruments(state, session, &ex).await?;
    let rows = exec_store::orders(&state.pool, session.id).await?;
    let fills = exec_store::fills(&state.pool, session.id).await?;
    let mut out = serde_json::Map::new();
    let mut adopted = Vec::new();
    for i in &inst {
        let held = held_qty(connector.as_ref(), &settings, i).await?;
        let legs = legs_of(&rows, &i.symbol);
        let own: Decimal = legs.iter().map(|l| if l.buy { l.filled } else { -l.filled }).sum::<Decimal>() - withheld(&fills, &i.symbol);
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
        let mut legs = legs_of(&rows, &i.symbol);
        // Exits first would leave a moment unprotected; entries first, then exits, then close.
        let mut working: Vec<&Leg> = legs.iter().filter(|l| l.working).collect();
        working.sort_by_key(|l| !l.role.opens());
        // An order that stopped working between the lookup and its cancel (an entry that just
        // filled) is not a failure: the account is read again before the close is sized.
        let mut moved = false;
        for l in working {
            let Some(venue) = rows.iter().find(|o| o.client_id == l.client_id).and_then(|o| o.venue_id.clone()) else { continue };
            match cancel(state, &account, connector.as_ref(), &settings, source, &venue).await {
                Ok(()) => cancelled += 1,
                Err(e) if e.message().contains("no longer working") => moved = true,
                Err(e) => problems.push(e.message().to_string()),
            }
        }
        if moved {
            let fresh = exec_store::orders(&state.pool, session.id).await?;
            let _ = look_up(state, connector.as_ref(), &settings, account_id, &fresh, std::slice::from_ref(i)).await;
            legs = legs_of(&exec_store::orders(&state.pool, session.id).await?, &i.symbol);
        }
        let fills = exec_store::fills(&state.pool, session.id).await.unwrap_or_default();
        let own: Decimal = legs.iter().map(|l| if l.buy { l.filled } else { -l.filled }).sum::<Decimal>() - withheld(&fills, &i.symbol);
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
        dispatch::gate_in_one_unit(state, account, connector, &settings, &ticket, false).await?;
        brokers::reconcile::claim(state, account, &ticket, &source.tag()).await?;
        brokers::reconcile::mint(state, account, connector, &settings, &ticket.ticker, &keys).await?;
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
        let direction = parts.get(2).copied().unwrap_or("");
        // The engine's trade this entry opened, none when its own never entered.
        let sim = trades.iter().find(|t| !t.trade_key.contains("|pending:") && opening_order(orders, trades, t).is_some_and(|o| o.client_id == e.client_id));
        let exit_ids: Vec<&str> = orders.iter().filter(|o| o.position == e.client_id && matches!(o.role.as_str(), "stop" | "target" | "close")).map(|o| o.client_id.as_str()).collect();
        let real_in = avg(&[e.client_id.as_str()]);
        let real_out = avg(&exit_ids);
        // The prices frozen when the position opened and closed, the live ones for older rows.
        let f = |t: Option<&store::Trade>, k: &str, live: &str| t.and_then(|t| t.trade.get(k).or_else(|| t.trade.get(live))).and_then(Value::as_f64);
        out.push(json!({
            "client_id": e.client_id,
            "ticker": e.ticker,
            "side": direction,
            "engine_qty": e.engine_qty,
            "qty": e.qty,
            "filled": real_in.map(|(_, q)| q),
            "sim_entry": f(sim, "sim_entry", "entry_price").or(e.engine_price),
            "real_entry": real_in.map(|(p, _)| p),
            "sim_exit": f(sim.filter(|t| !t.open), "sim_exit", "exit_price"),
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
#[path = "exec_tests.rs"]
mod tests;
