//! Guards: rules over a robot's own closed trades that change how it behaves for a while.
//!
//! A rule is `{ when, then }`. `when` reads the robot's closed trades (its own size, see
//! `robot_trades.robot_qty`) and the equity they make from its starting capital; `then` pauses
//! new entries for a while, halts them until the user resumes, or scales the robot's size down
//! until a restore condition is met. Exits are always managed: a guard never leaves a position
//! unwatched.
//!
//! **Pure.** [`evaluate`] takes the rules, every closed trade, the starting equity, the state
//! the guards hold and the time, and answers the next state and what happened. The caller runs
//! it after a tick that closed a trade and on the cycle (a pause that ran out, a scale restored
//! after some days), writes the state and records the events.
//!
//! **Each trade is read once.** `history_cursor` is the last trade evaluated; a trade closed
//! after it is processed in order, oldest first. Counting rules (`consecutive_*`,
//! `losses_in_window`) count from the rule's last firing, so a pause after three losses fires
//! again after three more, not on every loss that follows. Level rules (`drawdown_pct`,
//! `loss_pct`, `loss_amount`, `gain_pct`) fire when the level is crossed and re-arm once it is
//! no longer met (or, for a window, when the window rolls).
//!
//! **Precedence when several fire at once: halt > pause > scale.** A halt replaces a pause, a
//! pause never shortens a halt, and while either holds no entry is taken whatever the scale. A
//! scale fired at the same moment still applies when entries resume. A scale on top of a scale
//! multiplies, never under [`SCALE_FLOOR`], and the latest trigger's restore brings it back to
//! 1 in one step.
//!
//! **Money is Decimal.** A trade's P&L reaches the guards as the robot trade row stores it
//! (`robot_pnl`, rounded to the instrument currency's precision where the engine's trade became
//! the robot's); every threshold, sum and ratio here is exact. No float arithmetic in this file.
//!
//! **Windows are the robot's days.** `day`, `week` (ISO, Monday first) and `month` are read in
//! the robot's own timezone, the one its schedule runs in.
#![deny(clippy::float_arithmetic)]

use otw_connector_model::Decimal;
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{json, Value};
use time::{Duration, OffsetDateTime};
use time_tz::{OffsetDateTimeExt, Tz};

/// A decimal written as a JSON number or a string. A number is read from its own text, never
/// through a float.
fn dec<'de, D: Deserializer<'de>>(d: D) -> Result<Decimal, D::Error> {
    match Value::deserialize(d)? {
        Value::Number(n) => n.to_string().parse().map_err(serde::de::Error::custom),
        Value::String(s) => s.trim().parse().map_err(serde::de::Error::custom),
        other => Err(serde::de::Error::custom(format!("expected a number, got {other}"))),
    }
}

fn opt_dec<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Decimal>, D::Error> {
    match Option::<Value>::deserialize(d)? {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if s.trim().is_empty() => Ok(None),
        Some(Value::Number(n)) => n.to_string().parse().map(Some).map_err(serde::de::Error::custom),
        Some(Value::String(s)) => s.trim().parse().map(Some).map_err(serde::de::Error::custom),
        Some(other) => Err(serde::de::Error::custom(format!("expected a number, got {other}"))),
    }
}

const HUNDRED: Decimal = Decimal::ONE_HUNDRED;

/// The smallest share of its size a scaled robot keeps.
pub const SCALE_FLOOR: Decimal = Decimal::from_parts(1, 0, 0, false, 1);
/// Rules one robot may hold.
pub const MAX_RULES: usize = 10;

// ── Rules ─────────────────────────────────────────────────────────────────────

/// A period a window rule sums over, in calendar terms of the robot's timezone (UTC when it
/// names none).
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Window {
    #[default]
    Day,
    Week,
    Month,
    All,
}

/// Where a drawdown is measured from.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum From {
    #[default]
    Peak,
    Start,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum When {
    ConsecutiveLosses { n: u32 },
    ConsecutiveWins { n: u32 },
    DrawdownPct { #[serde(deserialize_with = "dec")] pct: Decimal, #[serde(default)] from: From },
    LossPct { #[serde(deserialize_with = "dec")] pct: Decimal, #[serde(default)] window: Window },
    LossAmount { #[serde(deserialize_with = "dec")] amount: Decimal, #[serde(default)] window: Window },
    LossesInWindow { n: u32, #[serde(default)] window: Window },
    GainPct { #[serde(deserialize_with = "dec")] pct: Decimal, #[serde(default)] window: Window },
}

/// How a scale comes back to 1.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Restore {
    AfterDays { n: u32 },
    AfterWins { n: u32 },
    AfterTrades { n: u32 },
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Then {
    /// No new entries for this long, exits still managed, then back on its own.
    Pause {
        #[serde(default, deserialize_with = "opt_dec", skip_serializing_if = "Option::is_none")]
        days: Option<Decimal>,
        #[serde(default, deserialize_with = "opt_dec", skip_serializing_if = "Option::is_none")]
        hours: Option<Decimal>,
    },
    /// No new entries until the user resumes.
    Halt,
    /// Size divided by `divisor` until `restore`.
    Scale { #[serde(deserialize_with = "dec")] divisor: Decimal, restore: Restore },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub when: When,
    pub then: Then,
}

impl Then {
    fn pause_for(&self) -> Option<Duration> {
        match self {
            Then::Pause { days, hours } => {
                let secs = days.unwrap_or_default() * Decimal::from(86_400) + hours.unwrap_or_default() * Decimal::from(3_600);
                Some(Duration::seconds(secs.round().to_i64().unwrap_or(i64::MAX)))
            }
            _ => None,
        }
    }
}

/// One sentence for a rule, as the timeline and the notes say it.
pub fn describe(r: &Rule) -> String {
    let w = |w: &Window| match w {
        Window::Day => "today",
        Window::Week => "this week",
        Window::Month => "this month",
        Window::All => "since the start",
    };
    let when = match &r.when {
        When::ConsecutiveLosses { n } => format!("{n} consecutive losses"),
        When::ConsecutiveWins { n } => format!("{n} consecutive wins"),
        When::DrawdownPct { pct, from } => format!("a {}% drawdown from the {}", pct.normalize(), if *from == From::Peak { "peak" } else { "start" }),
        When::LossPct { pct, window } => format!("a {}% loss {}", pct.normalize(), w(window)),
        When::LossAmount { amount, window } => format!("a loss of {} {}", amount.normalize(), w(window)),
        When::LossesInWindow { n, window } => format!("{n} losses {}", w(window)),
        When::GainPct { pct, window } => format!("a {}% gain {}", pct.normalize(), w(window)),
    };
    let then = match &r.then {
        Then::Pause { .. } => {
            let d = r.then.pause_for().unwrap_or_default();
            if d.whole_hours() >= 24 && d.whole_hours() % 24 == 0 {
                format!("pause {} day(s)", d.whole_days())
            } else if d.whole_minutes() % 60 == 0 {
                format!("pause {} hour(s)", d.whole_hours())
            } else {
                format!("pause {} minute(s)", d.whole_minutes())
            }
        }
        Then::Halt => "halt until resumed".to_string(),
        Then::Scale { divisor, restore } => {
            let back = match restore {
                Restore::AfterDays { n } => format!("restore after {n} day(s)"),
                Restore::AfterWins { n } => format!("restore after {n} win(s)"),
                Restore::AfterTrades { n } => format!("restore after {n} trade(s)"),
                Restore::Manual => "restore by hand".to_string(),
            };
            format!("divide the size by {}, {back}", divisor.normalize())
        }
    };
    format!("{then} after {when}")
}

/// Parse and check the rules a robot is saved with. Each refusal names the fix.
pub fn validate(raw: &Value) -> Result<Vec<Rule>, String> {
    let raw = if raw.is_null() { json!([]) } else { raw.clone() };
    let rules: Vec<Rule> = serde_json::from_value(raw).map_err(|e| format!("invalid guard: {e}"))?;
    if rules.len() > MAX_RULES {
        return Err(format!("a bot holds at most {MAX_RULES} guards: remove {}", rules.len() - MAX_RULES));
    }
    for (i, r) in rules.iter().enumerate() {
        let at = format!("guard {}", i + 1);
        let count = |n: u32| -> Result<(), String> {
            if (1..=1000).contains(&n) { Ok(()) } else { Err(format!("{at}: a count is between 1 and 1000")) }
        };
        let pct = |p: Decimal| -> Result<(), String> {
            if p > Decimal::ZERO && p <= HUNDRED { Ok(()) } else { Err(format!("{at}: a percentage is above 0 and at most 100")) }
        };
        match &r.when {
            When::ConsecutiveLosses { n } | When::ConsecutiveWins { n } | When::LossesInWindow { n, .. } => count(*n)?,
            When::DrawdownPct { pct: p, .. } | When::LossPct { pct: p, .. } | When::GainPct { pct: p, .. } => pct(*p)?,
            When::LossAmount { amount, .. } => {
                if *amount <= Decimal::ZERO {
                    return Err(format!("{at}: the loss amount is more than zero"));
                }
            }
        }
        match &r.then {
            Then::Pause { days, hours } => {
                if days.is_some_and(|d| d < Decimal::ZERO) || hours.is_some_and(|h| h < Decimal::ZERO) {
                    return Err(format!("{at}: a pause lasts a positive number of days or hours"));
                }
                let d = r.then.pause_for().unwrap_or_default();
                if d < Duration::minutes(1) || d > Duration::days(365) {
                    return Err(format!("{at}: a pause lasts between one minute and 365 days: set days or hours"));
                }
            }
            Then::Halt => {}
            Then::Scale { divisor, restore } => {
                if !(*divisor > Decimal::ONE && *divisor <= Decimal::TEN) {
                    return Err(format!("{at}: a scale divides the size by more than 1 and at most 10"));
                }
                match restore {
                    Restore::AfterDays { n } | Restore::AfterWins { n } | Restore::AfterTrades { n } => count(*n)?,
                    Restore::Manual => {}
                }
            }
        }
    }
    Ok(rules)
}

/// The guard set the wizard offers: pause a day after three consecutive losses, halve after a
/// 10% drawdown and come back after five trades.
pub fn defaults() -> Value {
    json!([
        { "when": { "kind": "consecutive_losses", "n": 3 }, "then": { "kind": "pause", "days": 1 } },
        { "when": { "kind": "drawdown_pct", "pct": 10, "from": "peak" },
          "then": { "kind": "scale", "divisor": 2, "restore": { "kind": "after_trades", "n": 5 } } }
    ])
}

// ── State ─────────────────────────────────────────────────────────────────────

/// A trade's place in the history: when it closed, then its key.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct Cursor {
    #[serde(with = "time::serde::rfc3339")]
    pub at: OffsetDateTime,
    pub key: String,
}

/// What one rule remembers between evaluations.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Mark {
    /// Counting rules count the trades closed after this one (their last firing).
    #[serde(default)]
    pub after: Option<Cursor>,
    /// Level rules: the level was met at the last trade (fires again only once it was not).
    #[serde(default)]
    pub on: bool,
    /// Window rules: the window `on` belongs to.
    #[serde(default)]
    pub window: String,
}

fn one() -> Decimal {
    Decimal::ONE
}

/// What the guards hold, stored on the robot (`guard_state`). `{}` is a robot at rest.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct State {
    /// A pause runs until then; `None` with `halt_kind` = `halt` holds until resumed.
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub halted_until: Option<OffsetDateTime>,
    /// The rule that holds the entries, as a sentence.
    #[serde(default)]
    pub halted_by: Option<String>,
    /// `pause` | `halt` while entries are held.
    #[serde(default)]
    pub halt_kind: Option<String>,
    #[serde(default = "one")]
    pub scale: Decimal,
    #[serde(default)]
    pub scale_restore: Option<Restore>,
    /// The rule that scaled the robot, as a sentence.
    #[serde(default)]
    pub scale_by: Option<String>,
    /// When the scale in force fired: restores count from here.
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub triggered_at: Option<OffsetDateTime>,
    /// Trades and wins closed since the scale in force fired.
    #[serde(default)]
    pub scale_trades: u32,
    #[serde(default)]
    pub scale_wins: u32,
    #[serde(default)]
    pub history_cursor: Option<Cursor>,
    #[serde(default)]
    pub marks: Vec<Mark>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            halted_until: None,
            halted_by: None,
            halt_kind: None,
            scale: Decimal::ONE,
            scale_restore: None,
            scale_by: None,
            triggered_at: None,
            scale_trades: 0,
            scale_wins: 0,
            history_cursor: None,
            marks: Vec::new(),
        }
    }
}

impl State {
    pub fn of(v: &Value) -> State {
        serde_json::from_value(v.clone()).unwrap_or_default()
    }

    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or_else(|_| json!({}))
    }

    /// Why no new entry is taken now, when one is not.
    pub fn holds(&self, now: OffsetDateTime) -> Option<String> {
        let by = self.halted_by.clone().unwrap_or_else(|| "a guard".into());
        match self.halt_kind.as_deref() {
            Some("halt") => Some(format!("halted by its guard ({by}): resume it from its page")),
            Some("pause") => match self.halted_until {
                Some(until) if until > now => Some(format!(
                    "paused by its guard ({by}) until {}",
                    until.format(&time::format_description::well_known::Rfc3339).unwrap_or_default()
                )),
                _ => None,
            },
            _ => None,
        }
    }

    /// The scale in force, never under the floor nor over 1.
    pub fn scale(&self) -> Decimal {
        self.scale.clamp(SCALE_FLOOR, Decimal::ONE)
    }

    /// The badge the fleet shows: `halt`, `pause`, `x0.5`, or nothing.
    pub fn badge(&self, now: OffsetDateTime) -> Option<String> {
        if self.holds(now).is_some() {
            return self.halt_kind.clone();
        }
        (self.scale() < Decimal::ONE).then(|| format!("x{}", self.scale().normalize()))
    }
}

/// One closed robot trade, as the guards read it: its P&L at the robot's own size.
#[derive(Debug, Clone, PartialEq)]
pub struct Closed {
    pub key: String,
    pub closed_at: OffsetDateTime,
    pub pnl: Decimal,
}

/// What happened: a trigger or a restoration, for the timeline and the log.
#[derive(Debug, Clone, PartialEq)]
pub struct Fired {
    pub at: OffsetDateTime,
    /// The rule (`{ when, then }`), `Null` for a restoration by time or by hand.
    pub rule: Value,
    /// `{ kind: pause|halt|scale|restore|resume, ... }`
    pub action: Value,
    pub trigger: Value,
    /// One sentence for the log and the channels.
    pub message: String,
}

// ── Evaluation ────────────────────────────────────────────────────────────────

/// The window an instant falls in, in the robot's timezone (UTC when it names none).
fn window_key(w: Window, at: OffsetDateTime, tz: Option<&Tz>) -> String {
    let at = match tz {
        Some(tz) => at.to_timezone(tz),
        None => at,
    };
    match w {
        Window::Day => at.date().to_string(),
        Window::Week => {
            let (y, wk, _) = at.date().to_iso_week_date();
            format!("{y}-W{wk}")
        }
        Window::Month => format!("{}-{:02}", at.year(), at.month() as u8),
        Window::All => "all".into(),
    }
}

fn rfc(t: OffsetDateTime) -> String {
    t.format(&time::format_description::well_known::Rfc3339).unwrap_or_default()
}

fn cursor(t: &Closed) -> Cursor {
    Cursor { at: t.closed_at, key: t.key.clone() }
}

/// Run the guards over what closed since the last evaluation, then over the time. `closed`
/// is every closed robot trade (any order); `start_equity` the robot's starting capital; `tz`
/// the robot's timezone, which its windows are read in.
pub fn evaluate(rules: &[Rule], closed: &[Closed], start_equity: Decimal, state: &State, now: OffsetDateTime, tz: Option<&Tz>) -> (State, Vec<Fired>) {
    let mut s = state.clone();
    let mut out = Vec::new();
    s.marks.resize(rules.len(), Mark::default());
    let mut trades: Vec<&Closed> = closed.iter().collect();
    trades.sort_by(|a, b| (a.closed_at, &a.key).cmp(&(b.closed_at, &b.key)));

    for (i, t) in trades.iter().enumerate() {
        let c = cursor(t);
        if s.history_cursor.as_ref().is_some_and(|h| c <= *h) {
            continue;
        }
        s.history_cursor = Some(c.clone());
        let history = &trades[..=i];

        // A scale counts the trades after its trigger first: the trade that restores it may
        // also be the one that scales it again.
        if s.scale < Decimal::ONE {
            s.scale_trades += 1;
            if t.pnl > Decimal::ZERO {
                s.scale_wins += 1;
            }
            let back = match &s.scale_restore {
                Some(Restore::AfterTrades { n }) => s.scale_trades >= *n,
                Some(Restore::AfterWins { n }) => s.scale_wins >= *n,
                _ => false,
            };
            if back {
                let trigger = json!({ "trades": s.scale_trades, "wins": s.scale_wins, "trade": t.key });
                out.push(restore_scale(&mut s, t.closed_at, trigger));
            }
        }

        // Which rules this trade makes fire.
        let mut fired: Vec<(usize, Value)> = Vec::new();
        for (ri, r) in rules.iter().enumerate() {
            if let Some(trigger) = check(r, &mut s.marks[ri], history, t, start_equity, tz) {
                fired.push((ri, trigger));
            }
        }
        if fired.is_empty() {
            continue;
        }
        let at = t.closed_at;
        let halt = fired.iter().find(|(ri, _)| matches!(rules[*ri].then, Then::Halt));
        let pause = fired
            .iter()
            .filter(|(ri, _)| matches!(rules[*ri].then, Then::Pause { .. }))
            .max_by_key(|(ri, _)| rules[*ri].then.pause_for().unwrap_or_default());
        if let Some((ri, trigger)) = halt {
            if s.halt_kind.as_deref() != Some("halt") {
                let by = describe(&rules[*ri]);
                s.halt_kind = Some("halt".into());
                s.halted_until = None;
                s.halted_by = Some(by.clone());
                out.push(Fired {
                    at,
                    rule: serde_json::to_value(&rules[*ri]).unwrap_or(Value::Null),
                    action: json!({ "kind": "halt" }),
                    trigger: trigger.clone(),
                    message: format!("Guard: {by}. No new entry until you resume the bot; exits are still managed."),
                });
            }
        } else if let Some((ri, trigger)) = pause {
            let until = at + rules[*ri].then.pause_for().unwrap_or_default();
            let held = s.halt_kind.as_deref() == Some("halt") || s.halted_until.is_some_and(|u| u >= until);
            if !held {
                let by = describe(&rules[*ri]);
                s.halt_kind = Some("pause".into());
                s.halted_until = Some(until);
                s.halted_by = Some(by.clone());
                out.push(Fired {
                    at,
                    rule: serde_json::to_value(&rules[*ri]).unwrap_or(Value::Null),
                    action: json!({ "kind": "pause", "until": rfc(until) }),
                    trigger: trigger.clone(),
                    message: format!("Guard: {by}. No new entry until {}; exits are still managed.", rfc(until)),
                });
            }
        }
        for (ri, trigger) in fired.iter().filter(|(ri, _)| matches!(rules[*ri].then, Then::Scale { .. })) {
            let Then::Scale { divisor, restore } = &rules[*ri].then else { continue };
            let next = (s.scale / *divisor).round_dp(6).max(SCALE_FLOOR).normalize();
            let by = describe(&rules[*ri]);
            s.scale = next;
            s.scale_restore = Some(restore.clone());
            s.scale_by = Some(by.clone());
            s.triggered_at = Some(at);
            s.scale_trades = 0;
            s.scale_wins = 0;
            out.push(Fired {
                at,
                rule: serde_json::to_value(&rules[*ri]).unwrap_or(Value::Null),
                action: json!({ "kind": "scale", "scale": next.normalize().to_string(), "divisor": divisor.normalize().to_string(), "restore": restore }),
                trigger: trigger.clone(),
                message: format!("Guard: {by}. New entries are sized x{}.", next.normalize()),
            });
        }
    }

    // The time: a pause that ran out, a scale restored after its days.
    if s.halt_kind.as_deref() == Some("pause") && s.halted_until.is_none_or(|u| u <= now) {
        let until = s.halted_until.unwrap_or(now);
        let by = s.halted_by.take().unwrap_or_default();
        s.halt_kind = None;
        s.halted_until = None;
        out.push(Fired {
            at: until.min(now),
            rule: Value::Null,
            action: json!({ "kind": "restore", "what": "pause" }),
            trigger: json!({ "until": rfc(until) }),
            message: format!("Guard: the pause ({by}) is over. New entries are taken again."),
        });
    }
    if s.scale < Decimal::ONE {
        if let (Some(Restore::AfterDays { n }), Some(at)) = (s.scale_restore.clone(), s.triggered_at) {
            let due = at + Duration::days(n as i64);
            if due <= now {
                out.push(restore_scale(&mut s, due, json!({ "days": n })));
            }
        }
    }
    (s, out)
}

fn restore_scale(s: &mut State, at: OffsetDateTime, trigger: Value) -> Fired {
    let by = s.scale_by.take().unwrap_or_default();
    s.scale = Decimal::ONE;
    s.scale_restore = None;
    s.triggered_at = None;
    s.scale_trades = 0;
    s.scale_wins = 0;
    Fired {
        at,
        rule: Value::Null,
        action: json!({ "kind": "restore", "what": "scale" }),
        trigger,
        message: format!("Guard: the size is back to normal ({by})."),
    }
}

/// What a resume clears: a halt, a pause, a scale, or all of them.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Clear {
    Halt,
    Pause,
    Scale,
    #[default]
    All,
}

impl Clear {
    pub fn word(self) -> &'static str {
        match self {
            Clear::Halt => "halt",
            Clear::Pause => "pause",
            Clear::Scale => "scale",
            Clear::All => "all",
        }
    }

    /// The timeline actions a clear closes.
    pub fn actions(self) -> &'static [&'static str] {
        match self {
            Clear::Halt => &["halt"],
            Clear::Pause => &["pause"],
            Clear::Scale => &["scale"],
            Clear::All => &["pause", "halt", "scale"],
        }
    }
}

/// Clear what the guards hold, by the user's hand: one thing or all of them. A clear of
/// something the guards do not hold is refused with the sentence the page shows.
pub fn resume(state: &State, what: Clear, now: OffsetDateTime) -> Result<(State, Fired), String> {
    let mut s = state.clone();
    let halted = s.halt_kind.as_deref() == Some("halt");
    let paused = s.halt_kind.as_deref() == Some("pause") && s.halted_until.is_some_and(|u| u > now);
    let scaled = s.scale < Decimal::ONE;
    let before = json!({ "halt": s.halt_kind, "until": s.halted_until.map(rfc), "scale": s.scale.normalize().to_string() });
    let clear_halt = |s: &mut State| {
        s.halt_kind = None;
        s.halted_until = None;
        s.halted_by = None;
    };
    let clear_scale = |s: &mut State| {
        s.scale = Decimal::ONE;
        s.scale_restore = None;
        s.scale_by = None;
        s.triggered_at = None;
        s.scale_trades = 0;
        s.scale_wins = 0;
    };
    let message = match what {
        Clear::Halt if !halted => return Err("the bot is not halted: there is no halt to clear".into()),
        Clear::Pause if !paused => return Err("the bot is not paused: there is no pause to clear".into()),
        Clear::Scale if !scaled => return Err("the bot is at full size: there is no scale to clear".into()),
        Clear::All if !(halted || paused || scaled || s.halt_kind.is_some()) => {
            return Err("the bot's guards hold nothing: there is nothing to resume".into())
        }
        Clear::Halt | Clear::Pause => {
            clear_halt(&mut s);
            format!("Resumed by hand: the {} is cleared, new entries are taken again.", what.word())
        }
        Clear::Scale => {
            clear_scale(&mut s);
            "Resumed by hand: the scale is cleared, new entries are at full size.".to_string()
        }
        Clear::All => {
            clear_halt(&mut s);
            clear_scale(&mut s);
            "Resumed by hand: new entries are taken again, at full size.".to_string()
        }
    };
    Ok((
        s,
        Fired { at: now, rule: Value::Null, action: json!({ "kind": "resume", "what": what.word() }), trigger: before, message },
    ))
}

/// Whether one rule fires on trade `t`, given the history up to and including it. Updates
/// the rule's mark. Answers what made it fire.
fn check(r: &Rule, m: &mut Mark, history: &[&Closed], t: &Closed, start: Decimal, tz: Option<&Tz>) -> Option<Value> {
    let after = |c: &&&Closed| m.after.as_ref().is_none_or(|a| cursor(c) > *a);
    let key_of = |w: Window, at: OffsetDateTime| window_key(w, at, tz);
    match &r.when {
        When::ConsecutiveLosses { n } | When::ConsecutiveWins { n } => {
            let losing = matches!(r.when, When::ConsecutiveLosses { .. });
            let streak = history.iter().rev().filter(after).take_while(|c| if losing { c.pnl < Decimal::ZERO } else { c.pnl > Decimal::ZERO }).count() as u32;
            (streak >= *n).then(|| {
                m.after = Some(cursor(t));
                json!({ "streak": streak, "trade": t.key })
            })
        }
        When::LossesInWindow { n, window } => {
            let key = key_of(*window, t.closed_at);
            let losses = history.iter().filter(after).filter(|c| key_of(*window, c.closed_at) == key && c.pnl < Decimal::ZERO).count() as u32;
            (losses >= *n).then(|| {
                m.after = Some(cursor(t));
                json!({ "losses": losses, "window": key, "trade": t.key })
            })
        }
        When::DrawdownPct { pct, from } => {
            let mut equity = start;
            let mut peak = start;
            for c in history {
                equity += c.pnl;
                peak = peak.max(equity);
            }
            let base = if *from == From::Peak { peak } else { start };
            let dd = if base > Decimal::ZERO { (base - equity) / base * HUNDRED } else { Decimal::ZERO };
            level(m, "all", dd >= *pct, json!({ "drawdown_pct": round2(dd), "equity": round2(equity), "from": round2(base), "trade": t.key }))
        }
        When::LossPct { pct, window } | When::GainPct { pct, window } => {
            let key = key_of(*window, t.closed_at);
            let before: Decimal = history.iter().filter(|c| key_of(*window, c.closed_at) != key && c.closed_at < t.closed_at).map(|c| c.pnl).sum();
            let net: Decimal = history.iter().filter(|c| key_of(*window, c.closed_at) == key).map(|c| c.pnl).sum();
            let base = if *window == Window::All { start } else { start + before };
            let share = if base > Decimal::ZERO { net / base * HUNDRED } else { Decimal::ZERO };
            let met = if matches!(r.when, When::LossPct { .. }) { share <= -*pct } else { share >= *pct };
            level(m, &key, met, json!({ "pnl": round2(net), "pct": round2(share), "window": key, "trade": t.key }))
        }
        When::LossAmount { amount, window } => {
            let key = key_of(*window, t.closed_at);
            let net: Decimal = history.iter().filter(|c| key_of(*window, c.closed_at) == key).map(|c| c.pnl).sum();
            level(m, &key, net <= -*amount, json!({ "pnl": round2(net), "window": key, "trade": t.key }))
        }
    }
}

/// A level rule: fires on the crossing, re-arms once the level is no longer met or the window
/// rolled.
fn level(m: &mut Mark, window: &str, met: bool, trigger: Value) -> Option<Value> {
    if m.window != window {
        m.window = window.to_string();
        m.on = false;
    }
    let fire = met && !m.on;
    m.on = met;
    fire.then_some(trigger)
}

/// A figure as the timeline shows it: two decimals, as a decimal string.
fn round2(v: Decimal) -> String {
    v.round_dp(2).normalize().to_string()
}

/// The scale as a plain number, for the JSON a page reads.
pub fn scale_f64(s: &State) -> f64 {
    s.scale().to_f64().unwrap_or(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn at(min: i64) -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(1_790_000_000 + min * 60).unwrap()
    }

    fn dd(v: &str) -> Decimal {
        v.parse().unwrap()
    }

    fn trades(pnls: &[i64]) -> Vec<Closed> {
        pnls.iter().enumerate().map(|(i, p)| Closed { key: format!("T|long|{i:04}"), closed_at: at(i as i64 * 10), pnl: Decimal::from(*p) }).collect()
    }

    fn rules(v: Value) -> Vec<Rule> {
        validate(&v).unwrap()
    }

    fn run(r: &[Rule], pnls: &[i64]) -> (State, Vec<Fired>) {
        evaluate(r, &trades(pnls), Decimal::from(1000), &State::default(), at(100_000), None)
    }

    fn kinds(f: &[Fired]) -> Vec<String> {
        f.iter().map(|x| x.action["kind"].as_str().unwrap_or("").to_string()).collect()
    }

    /// Three losses in a row pause the robot; the count starts again after the pause fired, so a
    /// fourth loss does not fire it again but a sixth does.
    #[test]
    fn consecutive_losses_pause_and_count_again_from_the_firing() {
        let r = rules(json!([{ "when": { "kind": "consecutive_losses", "n": 3 }, "then": { "kind": "pause", "hours": 1 } }]));
        let now = at(25);
        let (s, f) = evaluate(&r, &trades(&[-1, -1, -1]), Decimal::from(1000), &State::default(), now, None);
        assert_eq!(kinds(&f), ["pause"]);
        assert_eq!(s.halt_kind.as_deref(), Some("pause"));
        assert_eq!(s.halted_until, Some(at(20) + Duration::hours(1)));
        assert!(s.holds(now).unwrap().contains("paused"), "{:?}", s.holds(now));
        // Same history read again: nothing new.
        let (s2, f2) = evaluate(&r, &trades(&[-1, -1, -1]), Decimal::from(1000), &s, now, None);
        assert!(f2.is_empty() && s2 == s);
        // A fourth loss after the pause ran out: no new pause; then two more make three.
        let (s3, f3) = evaluate(&r, &trades(&[-1, -1, -1, -1]), Decimal::from(1000), &s, at(30) + Duration::hours(2), None);
        assert_eq!(kinds(&f3), ["restore"]);
        assert!(s3.holds(at(30) + Duration::hours(2)).is_none());
        let (_, f4) = evaluate(&r, &trades(&[-1, -1, -1, -1, -1, -1]), Decimal::from(1000), &s3, at(55), None);
        assert_eq!(kinds(&f4), ["pause"]);
    }

    #[test]
    fn consecutive_wins_fire_on_a_winning_streak_only() {
        let r = rules(json!([{ "when": { "kind": "consecutive_wins", "n": 2 }, "then": { "kind": "halt" } }]));
        let (_, f) = run(&r, &[1, -1, 1]);
        assert!(f.is_empty());
        let (s, f) = run(&r, &[1, -1, 1, 1]);
        assert_eq!(kinds(&f), ["halt"]);
        assert!(s.holds(at(1_000_000)).unwrap().contains("resume"));
    }

    /// From the peak: a 10% fall from the best equity. From the start: from the capital.
    #[test]
    fn drawdown_from_peak_and_from_start() {
        let peak = rules(json!([{ "when": { "kind": "drawdown_pct", "pct": 10 }, "then": { "kind": "halt" } }]));
        let start = rules(json!([{ "when": { "kind": "drawdown_pct", "pct": 10, "from": "start" }, "then": { "kind": "halt" } }]));
        // 1000 -> 1200 -> 1070: 10.8% from the peak, a gain from the start.
        let (_, f) = run(&peak, &[200, -130]);
        assert_eq!(kinds(&f), ["halt"]);
        assert_eq!(f[0].trigger["drawdown_pct"], "10.83");
        let (_, f) = run(&start, &[200, -130]);
        assert!(f.is_empty());
        let (_, f) = run(&start, &[-50, -60]);
        assert_eq!(kinds(&f), ["halt"]);
    }

    /// A level fires on the crossing, not on every trade under it, and re-arms once it is met
    /// no more.
    #[test]
    fn a_level_fires_once_until_it_recovers() {
        let r = rules(json!([{ "when": { "kind": "drawdown_pct", "pct": 5 }, "then": { "kind": "scale", "divisor": 2, "restore": { "kind": "manual" } } }]));
        let (s, f) = run(&r, &[-60, -10, -10]);
        assert_eq!(kinds(&f), ["scale"], "one firing under the level");
        assert_eq!(s.scale(), Decimal::new(5, 1));
        let (s, f) = run(&r, &[-60, -10, 200, -200]);
        assert_eq!(kinds(&f), ["scale", "scale"], "recovered, then crossed again");
        assert_eq!(s.scale(), Decimal::new(25, 2), "a scale on a scale multiplies");
    }

    #[test]
    fn window_rules_sum_their_window_and_rearm_when_it_rolls() {
        let day = rules(json!([{ "when": { "kind": "loss_pct", "pct": 5, "window": "day" }, "then": { "kind": "pause", "days": 1 } }]));
        let mut t = trades(&[-30, -30]);
        let (_, f) = evaluate(&day, &t, Decimal::from(1000), &State::default(), at(5), None);
        assert_eq!(kinds(&f), ["pause"], "6% lost today");
        // The second loss a day later: a new window, 3% only.
        t[1].closed_at += Duration::days(1);
        let (_, f) = evaluate(&day, &t, Decimal::from(1000), &State::default(), at(5) + Duration::days(1), None);
        assert!(f.is_empty(), "{f:?}");

        let amount = rules(json!([{ "when": { "kind": "loss_amount", "amount": 50, "window": "week" }, "then": { "kind": "halt" } }]));
        let (_, f) = run(&amount, &[-20, 10, -45]);
        assert_eq!(kinds(&f), ["halt"], "net -55 this week");

        let count = rules(json!([{ "when": { "kind": "losses_in_window", "n": 2, "window": "month" }, "then": { "kind": "halt" } }]));
        let (_, f) = run(&count, &[-1, 5, -1]);
        assert_eq!(kinds(&f), ["halt"], "two losses this month, not in a row");

        let gain = rules(json!([{ "when": { "kind": "gain_pct", "pct": 3, "window": "all" }, "then": { "kind": "pause", "hours": 4 } }]));
        let (_, f) = evaluate(&gain, &trades(&[10, 25]), Decimal::from(1000), &State::default(), at(15), None);
        assert_eq!(kinds(&f), ["pause"], "3.5% since the start");
    }

    /// Halt beats pause, pause beats scale, and a scale fired with them still applies.
    #[test]
    fn precedence_halt_over_pause_over_scale() {
        let r = rules(json!([
            { "when": { "kind": "consecutive_losses", "n": 1 }, "then": { "kind": "scale", "divisor": 2, "restore": { "kind": "after_trades", "n": 2 } } },
            { "when": { "kind": "consecutive_losses", "n": 1 }, "then": { "kind": "pause", "hours": 2 } },
            { "when": { "kind": "consecutive_losses", "n": 1 }, "then": { "kind": "halt" } }
        ]));
        let (s, f) = run(&r, &[-1]);
        assert_eq!(kinds(&f), ["halt", "scale"]);
        assert_eq!(s.halt_kind.as_deref(), Some("halt"));
        assert_eq!(s.scale(), Decimal::new(5, 1));
        let pause_then_scale = rules(json!([
            { "when": { "kind": "consecutive_losses", "n": 1 }, "then": { "kind": "pause", "hours": 2 } },
            { "when": { "kind": "consecutive_losses", "n": 1 }, "then": { "kind": "scale", "divisor": 4, "restore": { "kind": "manual" } } }
        ]));
        let (s, f) = evaluate(&pause_then_scale, &trades(&[-1]), Decimal::from(1000), &State::default(), at(1), None);
        assert_eq!(kinds(&f), ["pause", "scale"]);
        assert_eq!(s.scale(), Decimal::new(25, 2));
        // A pause never shortens a halt.
        let mut halted = State { halt_kind: Some("halt".into()), halted_by: Some("x".into()), ..Default::default() };
        halted.marks = vec![Mark::default(); 2];
        let (s, f) = evaluate(&pause_then_scale, &trades(&[-1]), Decimal::from(1000), &halted, at(1), None);
        assert_eq!(kinds(&f), ["scale"]);
        assert_eq!(s.halt_kind.as_deref(), Some("halt"));
    }

    /// "Scale by 2 after 1 loss, restore after 2 trades": the trade that restores may scale
    /// again.
    #[test]
    fn restore_after_trades_wins_and_days() {
        let r = rules(json!([{ "when": { "kind": "consecutive_losses", "n": 1 }, "then": { "kind": "scale", "divisor": 2, "restore": { "kind": "after_trades", "n": 2 } } }]));
        let (s, _) = run(&r, &[-1]);
        assert_eq!(s.scale(), Decimal::new(5, 1));
        let (s, f) = run(&r, &[-1, 1, 1]);
        assert_eq!(kinds(&f), ["scale", "restore"]);
        assert_eq!(s.scale(), Decimal::ONE);
        let (s, f) = run(&r, &[-1, 1, -1]);
        assert_eq!(kinds(&f), ["scale", "restore", "scale"]);
        assert_eq!(s.scale(), Decimal::new(5, 1));

        let wins = rules(json!([{ "when": { "kind": "consecutive_losses", "n": 2 }, "then": { "kind": "scale", "divisor": 2, "restore": { "kind": "after_wins", "n": 2 } } }]));
        let (s, _) = run(&wins, &[-1, -1, 1, 0, -1]);
        assert_eq!(s.scale(), Decimal::new(5, 1), "one win only");
        let (s, _) = run(&wins, &[-1, -1, 1, 0, -1, 2]);
        assert_eq!(s.scale(), Decimal::ONE);

        let days = rules(json!([{ "when": { "kind": "consecutive_losses", "n": 1 }, "then": { "kind": "scale", "divisor": 3, "restore": { "kind": "after_days", "n": 2 } } }]));
        let (s, _) = evaluate(&days, &trades(&[-1]), Decimal::from(1000), &State::default(), at(1) + Duration::days(1), None);
        assert!(s.scale() < Decimal::ONE);
        let (s, f) = evaluate(&days, &trades(&[-1]), Decimal::from(1000), &s, at(1) + Duration::days(2), None);
        assert_eq!(kinds(&f), ["restore"]);
        assert_eq!(s.scale(), Decimal::ONE);

        let manual = rules(json!([{ "when": { "kind": "consecutive_losses", "n": 1 }, "then": { "kind": "scale", "divisor": 2, "restore": { "kind": "manual" } } }]));
        let (s, _) = run(&manual, &[-1, 1, 1, 1, 1]);
        assert_eq!(s.scale(), Decimal::new(5, 1), "only a resume restores it");
        let (s, f) = resume(&s, Clear::All, at(9)).unwrap();
        assert_eq!((f.action["kind"].as_str(), f.action["what"].as_str()), (Some("resume"), Some("all")));
        assert_eq!(s.scale(), Decimal::ONE);
        assert!(resume(&s, Clear::All, at(9)).unwrap_err().contains("nothing to resume"), "nothing held: nothing to resume");
    }

    #[test]
    fn the_scale_never_falls_under_the_floor() {
        let r = rules(json!([{ "when": { "kind": "consecutive_losses", "n": 1 }, "then": { "kind": "scale", "divisor": 10, "restore": { "kind": "manual" } } }]));
        let (s, _) = run(&r, &[-1, -1, -1]);
        assert_eq!(s.scale(), SCALE_FLOOR);
        assert_eq!(s.badge(at(0)).as_deref(), Some("x0.1"));
    }

    #[test]
    fn validation_names_the_fix() {
        let bad = |v: Value| validate(&v).unwrap_err();
        assert!(bad(json!([{ "when": { "kind": "consecutive_losses", "n": 0 }, "then": { "kind": "halt" } }])).contains("between 1 and 1000"));
        assert!(bad(json!([{ "when": { "kind": "drawdown_pct", "pct": 150 }, "then": { "kind": "halt" } }])).contains("at most 100"));
        assert!(bad(json!([{ "when": { "kind": "loss_amount", "amount": -3 }, "then": { "kind": "halt" } }])).contains("more than zero"));
        assert!(bad(json!([{ "when": { "kind": "consecutive_losses", "n": 2 }, "then": { "kind": "pause" } }])).contains("between one minute and 365 days"));
        assert!(bad(json!([{ "when": { "kind": "consecutive_losses", "n": 2 }, "then": { "kind": "scale", "divisor": 1, "restore": { "kind": "manual" } } }])).contains("more than 1"));
        assert!(bad(json!([{ "when": { "kind": "sideways", "n": 2 }, "then": { "kind": "halt" } }])).contains("invalid guard"));
        assert!(validate(&defaults()).is_ok());
        assert!(validate(&Value::Null).unwrap().is_empty());
        let many: Vec<Value> = (0..11).map(|_| json!({ "when": { "kind": "consecutive_losses", "n": 2 }, "then": { "kind": "halt" } })).collect();
        assert!(bad(json!(many)).contains("at most 10"));
    }

    #[test]
    fn a_state_reads_back_from_its_json() {
        assert_eq!(State::of(&json!({})), State::default());
        let r = rules(defaults());
        let (s, _) = run(&r, &[-1, -1, -1]);
        assert_eq!(State::of(&s.to_value()), s);
        assert_eq!(describe(&r[0]), "pause 1 day(s) after 3 consecutive losses");
    }

    /// The threshold is exact: a drawdown of 10.00% fires a 10% rule, 9.99% does not.
    #[test]
    fn a_drawdown_on_the_threshold_fires_and_one_cent_under_does_not() {
        let r = rules(json!([{ "when": { "kind": "drawdown_pct", "pct": 10 }, "then": { "kind": "halt" } }]));
        let one = |pnl: &str| vec![Closed { key: "T|long|0".into(), closed_at: at(0), pnl: dd(pnl) }];
        let (_, f) = evaluate(&r, &one("-100.00"), Decimal::from(1000), &State::default(), at(1), None);
        assert_eq!(kinds(&f), ["halt"], "exactly 10.00%");
        assert_eq!(f[0].trigger["drawdown_pct"], "10");
        let (_, f) = evaluate(&r, &one("-99.90"), Decimal::from(1000), &State::default(), at(1), None);
        assert!(f.is_empty(), "9.99% stays under: {f:?}");
        // A threshold written as a string reads the same.
        let r = rules(json!([{ "when": { "kind": "drawdown_pct", "pct": "10.00" }, "then": { "kind": "halt" } }]));
        let (_, f) = evaluate(&r, &one("-100"), Decimal::from(1000), &State::default(), at(1), None);
        assert_eq!(kinds(&f), ["halt"]);
    }

    /// A day is the robot's day: 23:30 in Paris on 9 November and 00:30 on the 10th are the
    /// same UTC day (22:30 and 23:30) but two Paris days, so a daily loss rule does not add them.
    #[test]
    fn a_day_window_is_read_in_the_robot_timezone() {
        let paris = time_tz::timezones::get_by_name("Europe/Paris").unwrap();
        let r = rules(json!([{ "when": { "kind": "loss_amount", "amount": 50, "window": "day" }, "then": { "kind": "halt" } }]));
        let utc = |t: &str| OffsetDateTime::parse(t, &time::format_description::well_known::Rfc3339).unwrap();
        let book = vec![
            Closed { key: "a".into(), closed_at: utc("2026-11-09T22:30:00Z"), pnl: dd("-30") },
            Closed { key: "b".into(), closed_at: utc("2026-11-09T23:30:00Z"), pnl: dd("-30") },
        ];
        let now = utc("2026-11-09T23:45:00Z");
        let (_, f) = evaluate(&r, &book, Decimal::from(1000), &State::default(), now, None);
        assert_eq!(kinds(&f), ["halt"], "one UTC day: -60 on 9 November");
        let (_, f) = evaluate(&r, &book, Decimal::from(1000), &State::default(), now, Some(paris));
        assert!(f.is_empty(), "two Paris days, -30 each: {f:?}");
        let book3 = vec![book[0].clone(), book[1].clone(), Closed { key: "c".into(), closed_at: utc("2026-11-10T08:00:00Z"), pnl: dd("-25") }];
        let (_, f) = evaluate(&r, &book3, Decimal::from(1000), &State::default(), utc("2026-11-10T09:00:00Z"), Some(paris));
        assert_eq!(kinds(&f), ["halt"], "the 00:30 and the 09:00 Paris trades make -55 on 10 November");
        assert_eq!(f[0].trigger["window"], "2026-11-10");
    }

    /// Each hold is cleared on its own, and a clear of nothing says so.
    #[test]
    fn a_resume_clears_one_hold_or_all() {
        let held = State {
            halt_kind: Some("pause".into()),
            halted_until: Some(at(500)),
            halted_by: Some("x".into()),
            scale: dd("0.5"),
            scale_restore: Some(Restore::Manual),
            ..Default::default()
        };
        let (s, f) = resume(&held, Clear::Pause, at(10)).unwrap();
        assert!(s.halt_kind.is_none() && s.scale == dd("0.5"), "the pause goes, the scale stays");
        assert_eq!(f.action["what"], "pause");
        assert!(resume(&s, Clear::Halt, at(10)).unwrap_err().contains("not halted"));
        assert!(resume(&s, Clear::Pause, at(10)).unwrap_err().contains("not paused"));
        let (s, _) = resume(&s, Clear::Scale, at(10)).unwrap();
        assert_eq!(s.scale(), Decimal::ONE);
        assert!(resume(&s, Clear::Scale, at(10)).unwrap_err().contains("full size"));
        let halted = State { halt_kind: Some("halt".into()), ..Default::default() };
        let (s, f) = resume(&halted, Clear::Halt, at(10)).unwrap();
        assert!(s.halt_kind.is_none());
        assert_eq!(f.action["what"], "halt");
        assert_eq!(serde_json::from_value::<Clear>(json!("scale")).unwrap(), Clear::Scale);
        assert_eq!(Clear::default(), Clear::All);
    }

    fn arb_scale_rule() -> impl Strategy<Value = Value> {
        let when = prop_oneof![
            (1u32..4).prop_map(|n| json!({ "kind": "consecutive_losses", "n": n })),
            (1u32..4).prop_map(|n| json!({ "kind": "losses_in_window", "n": n, "window": "day" })),
            (1u32..20).prop_map(|p| json!({ "kind": "drawdown_pct", "pct": p })),
            (1u32..20).prop_map(|p| json!({ "kind": "loss_pct", "pct": p, "window": "week" })),
            (1u32..200).prop_map(|a| json!({ "kind": "loss_amount", "amount": a, "window": "all" })),
        ];
        let restore = prop_oneof![
            (1u32..5).prop_map(|n| json!({ "kind": "after_trades", "n": n })),
            (1u32..5).prop_map(|n| json!({ "kind": "after_wins", "n": n })),
            (1u32..5).prop_map(|n| json!({ "kind": "after_days", "n": n })),
        ];
        (when, 11u32..100, restore).prop_map(|(w, d, r)| json!({ "when": w, "then": { "kind": "scale", "divisor": Decimal::new(d as i64, 1).to_string(), "restore": r } }))
    }

    proptest! {
        /// Whatever losses come, the scale stays between the floor and 1 at every step, and a
        /// run of wins with the days going by brings it back to 1.
        #[test]
        fn a_scale_always_comes_back_and_never_falls_under_the_floor(
            raw in proptest::collection::vec(arb_scale_rule(), 1..4),
            cents in proptest::collection::vec(-10_000i64..10_000, 0..40),
        ) {
            let r = rules(json!(raw));
            let mut all: Vec<Closed> = trades(&vec![0; cents.len()]).into_iter().zip(&cents).map(|(mut c, p)| { c.pnl = Decimal::new(*p, 2); c }).collect();
            let mut s = State::default();
            for i in 0..all.len() {
                let (next, _) = evaluate(&r, &all[..=i], Decimal::from(1000), &s, all[i].closed_at, None);
                prop_assert!(next.scale >= SCALE_FLOOR && next.scale <= Decimal::ONE, "scale {}", next.scale);
                s = next;
            }
            let end = all.last().map_or(at(0), |t| t.closed_at);
            for k in 0..5 {
                all.push(Closed { key: format!("W|{k}"), closed_at: end + Duration::minutes(10 * (k as i64 + 1)), pnl: Decimal::ONE });
            }
            let (s, _) = evaluate(&r, &all, Decimal::from(1000), &s, end + Duration::days(10), None);
            prop_assert_eq!(s.scale, Decimal::ONE);
        }
    }
}
