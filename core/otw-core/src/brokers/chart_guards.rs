//! Guards on manual trading from the charts.
//!
//! A guard is a rule over the chart session's own trading and what happens when it is
//! reached: an `alert` (a notice on the chart), `acknowledge` (a window that stays on top of
//! the chart until the user acknowledges it, nothing blocked), `stop` (new chart entries
//! refused on the account until the user resets the session, optionally with the chart's
//! working orders cancelled and its positions closed), or `kill_switch` (the existing kill
//! switch, as its button fires it).
//!
//! - **Chart trading only.** What a rule reads is the legs sent from a chart
//!   (`broker_orders.source = 'session'`) and the fills of those legs. Robots and trades on
//!   alert are neither counted nor stopped.
//! - **A session** starts at the start of the day in the user's timezone (`day`, the
//!   default), or only at the user's reset (`manual`); a reset always starts a new one. A stop
//!   holds for the session it fired in.
//! - **Trades** are round trips rebuilt from the session's chart fills, per instrument, at
//!   weighted average cost: a trade ends when its instrument is flat again. A position held
//!   before the session (read from what is held now, minus the session's fills) has no known
//!   cost: the round trip that closes it is not counted.
//! - **Each occurrence fires once.** A firing is stored under a key (the trade, the count,
//!   the window); reading the rules again finds it and does nothing.
//! - **Pure rules.** [`trades`] and [`evaluate`] take the facts and answer what fires, so
//!   every rule is unit-tested; [`check_account`] gathers the facts and acts.

use std::collections::{HashMap, HashSet};

use anyhow::Result;
use otw_connector_model::Decimal;
use otw_store::brokers::BrokerRow;
use otw_store::chart_guards::{self as store, Guard};
use serde::{Deserialize, Serialize};
use serde_json::json;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::AppState;

/// What the charts module is called in a trading grant.
const CHARTS: &str = "histviz";

/// How often the rules are read for every account a guard applies to.
const TICK: std::time::Duration = std::time::Duration::from_secs(20);

/// Guards the user may hold.
pub const MAX_GUARDS: usize = 30;

pub const ACTIONS: &[&str] = &["alert", "acknowledge", "stop", "kill_switch"];

/// The reason a stopped chart session takes among the halts the gate is given.
pub const CHART_STOP: &str = "chart_session_stopped";

// ── Rules ─────────────────────────────────────────────────────────────────────

/// What a percentage of a trade's loss is measured against.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Of {
    /// What the trade's entries were worth.
    #[default]
    Notional,
    /// The account's equity when the session started.
    Equity,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Rule {
    /// `n` losing trades: in a row, or over the session.
    LosingTrades {
        n: u32,
        #[serde(default)]
        consecutive: bool,
    },
    /// `n` entries within `minutes`: a sliding window, or windows of `minutes` from the
    /// session's start.
    Trades {
        n: u32,
        minutes: u32,
        #[serde(default)]
        rolling: bool,
    },
    /// The account's equity down by `value` (an amount, or a percentage) from the session's start.
    Drawdown {
        #[serde(deserialize_with = "crate::robots::guards::dec")]
        value: Decimal,
        #[serde(default)]
        pct: bool,
    },
    /// One trade lost more than `value` (an amount, or a percentage of `of`).
    TradeLoss {
        #[serde(deserialize_with = "crate::robots::guards::dec")]
        value: Decimal,
        #[serde(default)]
        pct: bool,
        #[serde(default)]
        of: Of,
    },
    /// The session's fees reached `value`.
    Fees {
        #[serde(deserialize_with = "crate::robots::guards::dec")]
        value: Decimal,
    },
}

impl Rule {
    pub fn parse(v: &serde_json::Value) -> Result<Rule, String> {
        serde_json::from_value(v.clone()).map_err(|e| format!("this guard's rule cannot be read: {e}"))
    }

    /// Checked on save: each refusal names the fix.
    pub fn validate(&self) -> Result<(), String> {
        let positive = |v: &Decimal, what: &str| if *v > Decimal::ZERO { Ok(()) } else { Err(format!("{what} must be above 0")) };
        match self {
            Rule::LosingTrades { n, .. } => (1..=1000).contains(n).then_some(()).ok_or_else(|| "the number of losing trades is 1 to 1000".to_string()),
            Rule::Trades { n, minutes, .. } => {
                if !(1..=10_000).contains(n) {
                    return Err("the number of trades is 1 to 10000".into());
                }
                (1..=10_080).contains(minutes).then_some(()).ok_or_else(|| "the window is 1 minute to 7 days".to_string())
            }
            Rule::Drawdown { value, pct } | Rule::TradeLoss { value, pct, .. } => {
                positive(value, "the loss")?;
                if *pct && *value > Decimal::ONE_HUNDRED {
                    return Err("a percentage is at most 100".into());
                }
                Ok(())
            }
            Rule::Fees { value } => positive(value, "the fees"),
        }
    }

    fn needs_trades(&self) -> bool {
        matches!(self, Rule::LosingTrades { .. } | Rule::TradeLoss { .. } | Rule::Fees { .. })
    }

    fn needs_equity(&self) -> bool {
        matches!(self, Rule::Drawdown { .. } | Rule::TradeLoss { pct: true, of: Of::Equity, .. })
    }
}

// ── Trades from fills ─────────────────────────────────────────────────────────

/// One chart fill, its fee in the instrument's currency.
#[derive(Debug, Clone)]
pub struct Fill {
    pub id: String,
    pub at: OffsetDateTime,
    pub symbol: String,
    pub buy: bool,
    pub qty: Decimal,
    pub price: Decimal,
    pub fee: Decimal,
    pub multiplier: Decimal,
}

/// A round trip: from flat to flat again on one instrument.
#[derive(Debug, Clone, PartialEq)]
pub struct Trade {
    /// The instrument and the fill that opened it.
    pub key: String,
    pub closed_at: OffsetDateTime,
    /// Net of every fee paid along the way.
    pub pnl: Decimal,
    /// What its entries were worth.
    pub notional: Decimal,
}

#[derive(Default)]
struct Open {
    pos: Decimal,
    avg: Decimal,
    realized: Decimal,
    fees: Decimal,
    notional: Decimal,
    first: String,
    /// Holds a part bought before the session, at no known cost.
    unknown: bool,
}

/// The session's round trips, in the order they closed. `seed`: what was held on each
/// instrument when the session started (signed, long positive), at no known cost.
pub fn trades(fills: &[Fill], seed: &HashMap<String, Decimal>) -> Vec<Trade> {
    let mut fills: Vec<&Fill> = fills.iter().filter(|f| f.qty > Decimal::ZERO).collect();
    fills.sort_by(|a, b| a.at.cmp(&b.at).then_with(|| a.id.cmp(&b.id)));
    let mut book: HashMap<String, Open> = HashMap::new();
    let mut out = Vec::new();
    for f in fills {
        let st = book.entry(f.symbol.clone()).or_insert_with(|| {
            let pos = seed.get(&f.symbol).copied().unwrap_or_default();
            Open { pos, unknown: !pos.is_zero(), ..Default::default() }
        });
        let mult = if f.multiplier > Decimal::ZERO { f.multiplier } else { Decimal::ONE };
        let dir = if f.buy { Decimal::ONE } else { -Decimal::ONE };
        let mut left = f.qty;
        let mut fee = f.fee;
        if !st.pos.is_zero() && st.pos.is_sign_positive() != f.buy {
            let closing = left.min(st.pos.abs());
            let share = if left.is_zero() { Decimal::ZERO } else { fee * closing / left };
            let long = if st.pos.is_sign_positive() { Decimal::ONE } else { -Decimal::ONE };
            st.realized += closing * (f.price - st.avg) * long * mult;
            st.fees += share;
            st.pos += dir * closing;
            left -= closing;
            fee -= share;
            if st.pos.is_zero() {
                if !st.unknown {
                    out.push(Trade { key: format!("{}:{}", f.symbol, st.first), closed_at: f.at, pnl: st.realized - st.fees, notional: st.notional });
                }
                *st = Open::default();
            }
        }
        if left > Decimal::ZERO {
            if st.pos.is_zero() {
                *st = Open { first: f.id.clone(), ..Default::default() };
            }
            let held = st.pos.abs();
            st.avg = (st.avg * held + f.price * left) / (held + left);
            st.pos += dir * left;
            st.fees += fee;
            st.notional += left * f.price * mult;
        }
    }
    out
}

// ── Evaluation ────────────────────────────────────────────────────────────────

/// What the rules read for one account's session.
#[derive(Debug, Clone, Default)]
pub struct Facts {
    pub session_start: Option<OffsetDateTime>,
    pub trades: Vec<Trade>,
    /// When each chart entry was sent.
    pub entries: Vec<OffsetDateTime>,
    pub fees: Decimal,
    /// The equity the session started with, and now.
    pub equity_start: Option<Decimal>,
    pub equity_now: Option<Decimal>,
    pub currency: String,
}

/// A firing: the occurrence's key and the measure that reached the rule, in figures only (the
/// chart words it).
#[derive(Debug, Clone, PartialEq)]
pub struct Fire {
    pub key: String,
    pub detail: String,
}

fn ms(t: OffsetDateTime) -> i128 {
    t.unix_timestamp_nanos() / 1_000_000
}

fn money(v: Decimal, currency: &str) -> String {
    let v = v.round_dp(2).normalize();
    if currency.is_empty() { v.to_string() } else { format!("{v} {currency}") }
}

/// What `rule` fires on `f`, from the session's start: every occurrence, already fired or not.
/// `Err` when the rule cannot be read (no equity).
pub fn evaluate(rule: &Rule, f: &Facts) -> Result<Vec<Fire>, String> {
    let mut out = Vec::new();
    match rule {
        Rule::LosingTrades { n, consecutive } => {
            let n = (*n).max(1);
            let mut run = 0u32;
            let mut total = 0u32;
            for t in &f.trades {
                if t.pnl < Decimal::ZERO {
                    run += 1;
                    total += 1;
                } else {
                    run = 0;
                }
                if *consecutive {
                    if run == n {
                        out.push(Fire { key: format!("c:{}", t.key), detail: n.to_string() });
                        run = 0;
                    }
                } else if t.pnl < Decimal::ZERO && total.is_multiple_of(n) {
                    out.push(Fire { key: format!("l:{total}"), detail: total.to_string() });
                }
            }
        }
        Rule::Trades { n, minutes, rolling } => {
            let n = (*n).max(1) as usize;
            let span = Duration::minutes(i64::from((*minutes).max(1)));
            let mut at = f.entries.clone();
            at.sort();
            if *rolling {
                let mut last: Option<OffsetDateTime> = None;
                for (i, t) in at.iter().enumerate() {
                    let inside = at[..=i].iter().filter(|e| **e > *t - span).count();
                    if inside >= n && last.is_none_or(|l| *t - l >= span) {
                        out.push(Fire { key: format!("r:{}", ms(*t)), detail: inside.to_string() });
                        last = Some(*t);
                    }
                }
            } else if let Some(start) = f.session_start {
                let mut counts: HashMap<i64, usize> = HashMap::new();
                for t in &at {
                    let w = ((*t - start).whole_seconds().max(0)) / span.whole_seconds();
                    let c = counts.entry(w).or_default();
                    *c += 1;
                    if *c == n {
                        out.push(Fire { key: format!("w:{w}"), detail: n.to_string() });
                    }
                }
            }
        }
        Rule::Drawdown { value, pct } => {
            let (Some(start), Some(now)) = (f.equity_start, f.equity_now) else {
                return Err("the account's equity could not be read".into());
            };
            let loss = start - now;
            let cap = if *pct { start * *value / Decimal::ONE_HUNDRED } else { *value };
            if loss > Decimal::ZERO && loss >= cap {
                let detail = if *pct && start > Decimal::ZERO {
                    format!("{}% ({})", (loss / start * Decimal::ONE_HUNDRED).round_dp(2).normalize(), money(-loss, &f.currency))
                } else {
                    money(-loss, &f.currency)
                };
                out.push(Fire { key: "dd".into(), detail });
            }
        }
        Rule::TradeLoss { value, pct, of } => {
            if *pct && *of == Of::Equity && f.equity_start.is_none() {
                return Err("the account's equity could not be read".into());
            }
            for t in f.trades.iter().filter(|t| t.pnl < Decimal::ZERO) {
                let loss = -t.pnl;
                let base = match of {
                    Of::Notional => t.notional,
                    Of::Equity => f.equity_start.unwrap_or_default(),
                };
                let reached = if *pct { base > Decimal::ZERO && loss / base * Decimal::ONE_HUNDRED > *value } else { loss > *value };
                if reached {
                    let detail = if *pct && base > Decimal::ZERO {
                        format!("{}% ({})", (loss / base * Decimal::ONE_HUNDRED).round_dp(2).normalize(), money(t.pnl, &f.currency))
                    } else {
                        money(t.pnl, &f.currency)
                    };
                    out.push(Fire { key: format!("t:{}", t.key), detail });
                }
            }
        }
        Rule::Fees { value } => {
            if f.fees >= *value {
                out.push(Fire { key: "fees".into(), detail: money(f.fees, &f.currency) });
            }
        }
    }
    Ok(out)
}

// ── Sessions ──────────────────────────────────────────────────────────────────

/// When the account's chart session started.
pub fn session_start(settings: &store::Settings, reset_at: Option<OffsetDateTime>, now: OffsetDateTime) -> OffsetDateTime {
    let base = if settings.reset == "manual" {
        settings.updated_at.unwrap_or(now)
    } else {
        super::risk::period("day", &settings.timezone, now).0
    };
    reset_at.map_or(base, |r| r.max(base))
}

async fn current_start(state: &AppState, account_id: Uuid, now: OffsetDateTime) -> Result<OffsetDateTime> {
    let settings = store::settings(&state.pool).await?;
    let session = store::session(&state.pool, account_id).await?;
    Ok(session_start(&settings, session.reset_at, now))
}

/// The stop in force on the account's chart session, if one fired.
pub async fn stopped(state: &AppState, account_id: Uuid) -> Result<Option<store::Event>> {
    let start = current_start(state, account_id, OffsetDateTime::now_utc()).await?;
    store::stopped(&state.pool, account_id, start).await
}

// ── Reading one account ───────────────────────────────────────────────────────

fn dec(v: f64) -> Decimal {
    Decimal::try_from(v).unwrap_or_default()
}

fn bare(s: &str) -> String {
    s.chars().filter(char::is_ascii_alphanumeric).collect::<String>().to_ascii_uppercase()
}

/// A fill's fee in the instrument's currency: billed in it, or in the coin bought (worth its
/// price). A fee in anything else is not counted.
fn fee_in_quote(e: &super::Execution) -> Decimal {
    let fee = dec(e.fee.abs());
    let cur = e.fee_currency.trim().to_ascii_uppercase();
    if cur.is_empty() || cur == e.currency.trim().to_ascii_uppercase() {
        fee
    } else if bare(&e.symbol).starts_with(&cur) {
        fee * dec(e.price)
    } else {
        Decimal::ZERO
    }
}

/// The guards that apply to the account, parsed.
fn applying(guards: &[Guard], account_id: Uuid) -> Vec<(&Guard, Rule)> {
    guards
        .iter()
        .filter(|g| g.enabled && g.account_id.is_none_or(|a| a == account_id))
        .filter_map(|g| match Rule::parse(&g.rule) {
            Ok(r) => Some((g, r)),
            Err(e) => {
                tracing::warn!("chart guard {}: {e}", g.id);
                None
            }
        })
        .collect()
}

/// Read the account's chart session, fire what its guards reach and act on each new firing.
/// Answers the new firings.
pub async fn check_account(state: &AppState, account: &BrokerRow, guards: &[Guard]) -> Result<Vec<store::Event>> {
    let rules = applying(guards, account.id);
    if rules.is_empty() {
        return Ok(Vec::new());
    }
    let now = OffsetDateTime::now_utc();
    let settings = store::settings(&state.pool).await?;
    let session = store::session(&state.pool, account.id).await?;
    let start = session_start(&settings, session.reset_at, now);
    let legs = store::chart_legs(&state.pool, account.id, start).await?;
    let mut facts = Facts { session_start: Some(start), ..Default::default() };
    facts.entries = legs.iter().filter(|l| l.entry && l.status != "rejected").map(|l| l.created_at).collect();

    let need_trades = rules.iter().any(|(_, r)| r.needs_trades()) && !legs.is_empty();
    let need_equity = rules.iter().any(|(_, r)| r.needs_equity());
    if need_trades || need_equity {
        let (connector, cfg, _) = crate::brokers_api::resolve(state, account.id, None).await.map_err(|e| anyhow::anyhow!(e.message().to_string()))?;
        if need_equity {
            match super::risk::equity(connector.as_ref(), &cfg).await {
                Ok((eq, currency)) => {
                    if session.session_start == Some(start) && session.equity.is_some() {
                        facts.equity_start = session.equity;
                    } else {
                        store::set_mark(&state.pool, account.id, start, eq, &currency).await?;
                        facts.equity_start = Some(eq);
                    }
                    facts.equity_now = Some(eq);
                    facts.currency = currency;
                }
                Err(e) => tracing::warn!("chart guards on “{}”: {e}", account.name),
            }
        }
        if need_trades {
            let (fills, seed, currency) = chart_fills(account, connector.as_ref(), &cfg, &legs, start, now).await?;
            facts.fees = fills.iter().map(|f| f.fee).sum();
            facts.trades = trades(&fills, &seed);
            if facts.currency.is_empty() {
                facts.currency = currency;
            }
        }
    }

    let mut fired = Vec::new();
    for (g, rule) in rules {
        let fires = match evaluate(&rule, &facts) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("chart guard {} on “{}”: {e}", g.id, account.name);
                continue;
            }
        };
        for f in fires {
            let Some(ev) = store::fire(&state.pool, g.id, account.id, start, &f.key, &g.action, &f.detail).await? else { continue };
            act(state, account, g, &legs, &ev).await;
            fired.push(ev);
        }
    }
    Ok(fired)
}

/// The fills of the session's chart legs, what was held on their instruments when it
/// started, and their currency.
async fn chart_fills(
    account: &BrokerRow,
    connector: &dyn super::Broker,
    cfg: &HashMap<String, String>,
    legs: &[store::ChartLeg],
    start: OffsetDateTime,
    now: OffsetDateTime,
) -> Result<(Vec<Fill>, HashMap<String, Decimal>, String)> {
    if !connector.chart_fills() {
        return Ok((Vec::new(), HashMap::new(), String::new()));
    }
    let mut ids: HashSet<String> = HashSet::new();
    for l in legs {
        ids.insert(l.client_id.clone());
        ids.extend(l.venue_id.clone());
        ids.extend(l.venue_client_id.clone());
    }
    let mut instruments: Vec<(String, String)> = legs.iter().filter(|l| !l.asset_type.is_empty()).map(|l| (l.ticker.clone(), l.asset_type.clone())).collect();
    instruments.sort();
    instruments.dedup();
    let mut symbols = Vec::new();
    for (ticker, asset_type) in &instruments {
        symbols.push(crate::brokers_api::ChartSymbol::of(connector, cfg, ticker, asset_type).await.symbol);
    }
    let q = super::ExecQuery { from: start, to: now, symbols };
    let rows = super::cache::executions(account.id, connector, cfg, &q).await?;
    let mine: Vec<&super::Execution> = rows.iter().filter(|e| ids.contains(&e.order_id) || (!e.client_id.is_empty() && ids.contains(&e.client_id))).collect();
    let currency = mine.first().map(|e| e.currency.clone()).unwrap_or_default();
    let fills: Vec<Fill> = mine
        .iter()
        .map(|e| Fill {
            id: e.id.clone(),
            at: e.at,
            symbol: bare(&e.symbol),
            buy: e.side.eq_ignore_ascii_case("buy"),
            qty: dec(e.qty.abs()),
            price: dec(e.price),
            fee: fee_in_quote(e),
            multiplier: dec(e.multiplier),
        })
        .collect();
    // What was held when the session started: what is held now, less the session's fills.
    let mut seed: HashMap<String, Decimal> = HashMap::new();
    let http = super::client()?;
    if let Ok(rows) = connector.positions(&http, cfg).await {
        for p in rows {
            let q = dec(p.qty.abs());
            *seed.entry(bare(&p.symbol)).or_default() += if p.side == "short" { -q } else { q };
        }
        for f in &fills {
            *seed.entry(f.symbol.clone()).or_default() -= if f.buy { f.qty } else { -f.qty };
        }
    } else {
        seed.clear();
    }
    seed.retain(|_, v| !v.is_zero());
    Ok((fills, seed, currency))
}

// ── Acting ────────────────────────────────────────────────────────────────────

async fn act(state: &AppState, account: &BrokerRow, g: &Guard, legs: &[store::ChartLeg], ev: &store::Event) {
    crate::security_events::log("chart_guard", &format!("chart guard {} on “{}”: {} ({})", g.id, account.name, ev.action, ev.detail));
    match g.action.as_str() {
        "stop" if g.close_positions => flatten(state, account, legs).await,
        "kill_switch" => {
            if let Err(e) = crate::brokers_risk_api::kill_all(state).await {
                tracing::error!("chart guard {}: the kill switch failed: {}", g.id, e.message());
            }
        }
        _ => {}
    }
}

/// The chart's working orders on the account cancelled, then its positions on the instruments
/// the session traded closed at the market, each call audited like the chart's own.
async fn flatten(state: &AppState, account: &BrokerRow, legs: &[store::ChartLeg]) {
    let (connector, cfg, account) = match crate::brokers_api::resolve(state, account.id, None).await {
        Ok(v) => v,
        Err(e) => return tracing::error!("chart guard stop on “{}”: {}", account.name, e.message()),
    };
    for l in legs.iter().filter(|l| otw_store::broker_orders::OPEN.contains(&l.status.as_str())) {
        let Some(id) = l.venue_id.as_deref() else { continue };
        let request = json!({ "order": id, "chart_guard": true });
        if let Err(e) = crate::brokers_api::audited(state, &account, "cancel", request, connector.cancel(&cfg, id)).await {
            tracing::warn!("chart guard stop on “{}”: cancel {id}: {}", account.name, e.message());
        }
    }
    let mut wanted = Vec::new();
    for l in legs.iter().filter(|l| !l.asset_type.is_empty()) {
        wanted.push(crate::brokers_api::ChartSymbol::of(connector.as_ref(), &cfg, &l.ticker, &l.asset_type).await);
    }
    let Ok(http) = super::client() else { return };
    let positions = match connector.positions(&http, &cfg).await {
        Ok(p) => p,
        Err(e) => return tracing::error!("chart guard stop on “{}”: positions: {e:#}", account.name),
    };
    for p in positions.iter().filter(|p| p.qty.abs() > 0.0 && wanted.iter().any(|w| w.matches(&p.symbol))) {
        let Some(native) = p.native_id.clone() else {
            tracing::warn!("chart guard stop on “{}”: the {} position carries no id to close it by", account.name, p.symbol);
            continue;
        };
        let key = crate::alert_trade::mint_key();
        let keys = vec![key.clone()];
        let request = json!({ "native_id": native, "account": p.account, "key": key, "chart_guard": true });
        let call = async {
            super::reconcile::claim_close(state, &account, &native, &key, "session").await?;
            super::reconcile::mint(state, &account, connector.as_ref(), &cfg, &native, &keys).await?;
            let out = connector.close_position(&cfg, &native, &p.account, &key).await.map(|p| vec![p]);
            super::reconcile::record(state, &account, &keys, &out).await;
            out.map(|mut v| v.remove(0))
        };
        if let Err(e) = crate::brokers_api::audited(state, &account, "close", request, call).await {
            tracing::warn!("chart guard stop on “{}”: close {}: {}", account.name, p.symbol, e.message());
        }
    }
}

/// Every account a guard applies to, read once.
pub async fn check_all(state: &AppState) -> Result<Vec<store::Event>> {
    let guards = store::list(&state.pool).await?;
    if !guards.iter().any(|g| g.enabled) {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for account in otw_store::brokers::list(&state.pool).await?.into_iter().filter(|a| a.trades() && a.allows(CHARTS)) {
        match check_account(state, &account, &guards).await {
            Ok(v) => out.extend(v),
            Err(e) => tracing::warn!("chart guards on “{}”: {e:#}", account.name),
        }
    }
    Ok(out)
}

static RUNNING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// One read of every account, never two at once: the loop's and the chart's ask.
pub async fn check_now(state: &AppState) -> Result<Vec<store::Event>> {
    let _one = RUNNING.lock().await;
    check_all(state).await
}

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        loop {
            if let Err(e) = check_now(&state).await {
                tracing::warn!("chart guards: {e:#}");
            }
            tokio::time::sleep(TICK).await;
        }
    });
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
#[path = "chart_guards_tests.rs"]
mod tests;
