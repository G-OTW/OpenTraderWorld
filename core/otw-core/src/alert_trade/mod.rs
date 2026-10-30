//! Trade on alert: a chart alert that sends a ticket to a broker account when it fires.
//!
//! The alert keeps its condition, mode and channels; the trade is one row beside it
//! (`alert_trades`, [`otw_store::alert_trades`]). It fires once and disarms itself. Many may be
//! armed on one instrument, each armed on its own; the account's position limit stops the one
//! too many.
//!
//! **Fire path.** Both evaluators (closed bars and live feed) call [`on_fire`] once the alert's
//! own notification is written. A trade not armed says so on the alert's channels and stops.
//! An armed one is taken ([`otw_store::alert_trades::take`]: disarmed and marked `sending` in
//! one statement, so it is taken once whatever races it and a restart never sends it again),
//! then:
//!
//! 1. its template is made an order at the price that fired ([`template::materialize`]), on the
//!    broker's grid; a value that cannot be worked out is refused with a sentence;
//! 2. `enter` goes through [`dispatch::send_internal`] with [`Source::Alert`]: every gate the
//!    chart's orders meet, the background trading permission in place of the session lock;
//!    `exit` cancels the instrument's working orders and closes the position at the market;
//! 3. placed: `sent`, followed until it fills ([`follow`]), which is notified once with price,
//!    size and fees; refused: `refused`, notified, the fallback run; unknown: `unknown`,
//!    notified with "check at the broker", and the fallback cut down to cancelling
//!    ([`template::fallback_for`]), since nothing may add exposure next to an order that may
//!    be working.
//!
//! **Fallbacks** (`cancel_symbol`, `close_symbol`, `flatten_account`) run through the same
//! audited internals as the order routes, each call written to the audit log with the alert as
//! its source. Cancelling is never gated; closing needs what any background send needs.
//!
//! Notifications go to the alert's own channels, as module `histviz`, linked to the alert.

pub mod template;

#[cfg(test)]
mod mock_tests;

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Duration;

use anyhow::Result;
use otw_connector_model::{Decimal, Side, TifKind};
use serde::Serialize;
use serde_json::{json, Value};
use time::OffsetDateTime;
use uuid::Uuid;

use otw_store::alert_trades::{self as store, AlertTrade};
use otw_store::brokers::BrokerRow;
use otw_store::histviz::Alert;

use crate::brokers::dispatch::{self, SendRefusal, Source};
use crate::brokers::{self, Broker, Grid, OrderUnknown, Placed, Position};
use crate::histviz_alerts::{self as alerts, AlertCx};
use crate::{brokers_api, AppState};
use template::{Intent, Materialized, TicketTemplate};

/// The state the evaluators reach the order sender through. Set once from `main`, after the
/// state is built (the evaluators start before it).
static STATE: OnceLock<AppState> = OnceLock::new();

/// How often placed orders are looked at for a fill. The reconciliation cycle is what reads
/// the broker; this only reads what it stored.
const FOLLOW_EVERY: Duration = Duration::from_secs(15);
/// Wait before the first look, so a restart does not race the migrations.
const WARMUP: Duration = Duration::from_secs(25);

/// The price that fired an alert, and when it was seen.
#[derive(Debug, Clone, Copy)]
pub struct Trigger {
    pub price: f64,
    pub at: OffsetDateTime,
}

/// What a fire did with the trade.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Fired {
    /// The alert has a trade, not armed: nothing was sent (reported to the tests).
    #[cfg_attr(not(test), allow(dead_code))]
    NotArmed,
    /// Placed at the broker under these client ids.
    Sent { keys: Vec<String>, placed: Vec<Placed>, order: String },
    /// Nothing was placed.
    Refused { code: String, message: String, fallback: Option<FallbackReport> },
    /// The broker's answer was lost: the order may be working.
    Unknown { keys: Vec<String>, message: String, fallback: Option<FallbackReport> },
}

/// What a fallback did, and what it left open.
#[derive(Debug, Clone, Default, Serialize)]
pub struct FallbackReport {
    pub kind: String,
    /// Broker ids of the orders cancelled.
    pub cancelled: Vec<String>,
    /// Positions closed: `0.5 BTCUSDT`.
    pub closed: Vec<String>,
    /// What is still open, one sentence each.
    pub failed: Vec<String>,
}

impl FallbackReport {
    pub fn ok(&self) -> bool {
        self.failed.is_empty()
    }

    fn sentence(&self) -> String {
        let mut parts = Vec::new();
        if !self.cancelled.is_empty() {
            parts.push(format!("{} working order(s) cancelled", self.cancelled.len()));
        }
        if !self.closed.is_empty() {
            parts.push(format!("closed at the market: {}", self.closed.join(", ")));
        }
        if parts.is_empty() && self.failed.is_empty() {
            parts.push("nothing was working or held".into());
        }
        let mut s = format!("Fallback ({}): {}.", self.kind.replace('_', " "), parts.join("; "));
        if !self.failed.is_empty() {
            s.push_str(&format!(" Still open: {}", self.failed.join(" ")));
        }
        s
    }
}

/// Make the order sender reachable from the evaluators, and follow placed orders until they
/// fill. Called once from `main`.
pub fn install(state: AppState) {
    if STATE.set(state.clone()).is_err() {
        return;
    }
    tokio::spawn(async move {
        tokio::time::sleep(WARMUP).await;
        if let Err(e) = restart(&state).await {
            tracing::error!("trades on alert: {e:#}");
        }
        loop {
            if let Err(e) = follow(&state).await {
                tracing::warn!("trades on alert: following placed orders: {e:#}");
            }
            tokio::time::sleep(FOLLOW_EVERY).await;
        }
    });
}

fn cx(state: &AppState) -> AlertCx {
    AlertCx { cipher: state.cipher.clone(), http: state.http.clone() }
}

async fn notify(state: &AppState, a: &Alert, title: String, details: String) {
    if let Err(e) = alerts::notify(&state.pool, &cx(state), a, title, details).await {
        tracing::warn!("trade on alert {}: notification failed: {e:#}", a.id);
    }
}

/// The alert fired: send its trade, when it has one. Called by both evaluators once the
/// alert's own notification is written; never fails the evaluator. The trade is taken here,
/// before anything else, and sent on its own task so a slow broker holds no other alert up.
pub async fn on_fire(a: &Alert, trigger: Option<Trigger>) {
    let Some(state) = STATE.get().cloned() else { return };
    match begin(&state, a).await {
        Ok(Begin::Taken(row)) => {
            let a = a.clone();
            tokio::spawn(async move {
                run_taken(&state, &a, *row, trigger).await;
            });
        }
        Ok(_) => {}
        Err(e) => tracing::error!("trade on alert {}: {e:#}", a.id),
    }
}

/// [`on_fire`] in place, for one alert: what the tests drive.
#[cfg(test)]
pub async fn execute(state: &AppState, a: &Alert, trigger: Option<Trigger>) -> Result<Option<Fired>> {
    Ok(match begin(state, a).await? {
        Begin::NoTrade => None,
        Begin::NotArmed => Some(Fired::NotArmed),
        Begin::Taken(row) => Some(run_taken(state, a, *row, trigger).await),
    })
}

enum Begin {
    NoTrade,
    NotArmed,
    Taken(Box<AlertTrade>),
}

async fn begin(state: &AppState, a: &Alert) -> Result<Begin> {
    let Some(row) = store::get(&state.pool, a.id).await? else { return Ok(Begin::NoTrade) };
    if !row.armed() {
        notify(
            state,
            a,
            format!("Trade not armed: {}", alerts::title_of(a)),
            format!("The alert on {} fired and its trade is not armed, so no order was sent. Arm it again from the alert to trade the next one.", a.ticker),
        )
        .await;
        return Ok(Begin::NotArmed);
    }
    Ok(match store::take(&state.pool, a.id).await? {
        Some(row) => Begin::Taken(Box::new(row)),
        // Taken by another fire a moment ago.
        None => Begin::NoTrade,
    })
}

/// A client id for an order a trade on alert sends: unique per account forever, short enough
/// for every venue (Binance takes 36 characters).
pub fn mint_key() -> String {
    let ms = (OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as u64;
    let mut n = ms;
    let mut b36 = Vec::new();
    while n > 0 {
        let d = (n % 36) as u8;
        b36.push(if d < 10 { b'0' + d } else { b'a' + d - 10 });
        n /= 36;
    }
    b36.reverse();
    format!("at{}{}", String::from_utf8_lossy(&b36), &Uuid::new_v4().simple().to_string()[..6])
}

/// The trigger as a decimal, or the price read now for an alert that has none (a time alert):
/// its newest closed bar, with the time that bar closed.
async fn trigger_price(state: &AppState, a: &Alert, trigger: Option<Trigger>) -> Result<(Decimal, i64), String> {
    let t = match trigger {
        Some(t) => t,
        None => {
            let tf = crate::histdata::timeframe_secs(&a.timeframe).map_err(|e| format!("{e}"))?;
            let bars = alerts::load_bars(&state.pool, &cx(state), a, tf, 3).await.map_err(|e| format!("the price of {} could not be read ({e:#}), so nothing was sent", a.ticker))?;
            let closed = alerts::closed_bars(&bars, tf, OffsetDateTime::now_utc());
            let b = closed.last().ok_or_else(|| format!("no closed bar of {} could be read, so nothing was sent", a.ticker))?;
            Trigger { price: b.close, at: b.ts + time::Duration::seconds(tf) }
        }
    };
    let price = brokers::paper::exact(t.price).filter(|p| *p > Decimal::ZERO).ok_or_else(|| format!("the price that fired ({}) is not a price, so nothing was sent", t.price))?;
    Ok((price, (t.at.unix_timestamp_nanos() / 1_000_000) as i64))
}

/// What materializing a template on one account needs, read once.
pub(crate) struct Prepared {
    pub connector: Box<dyn Broker>,
    pub settings: HashMap<String, String>,
    pub account: BrokerRow,
    pub grid: Option<Grid>,
    pub point_value: Option<Decimal>,
    pub atr: Result<Decimal, String>,
    pub reduce_only_ok: bool,
    pub exit_tif: TifKind,
}

impl Prepared {
    pub(crate) fn cx<'a>(&'a self, a: &'a Alert, trigger: Decimal, at_ms: i64, key: &'a str) -> template::Cx<'a> {
        template::Cx {
            ticker: &a.ticker,
            asset_type: &a.asset_type,
            provider: Some(a.provider.clone()),
            connector: a.connector_id,
            trigger,
            at_ms,
            grid: self.grid.as_ref(),
            point_value: self.point_value,
            atr: self.atr.clone(),
            reduce_only_ok: self.reduce_only_ok,
            exit_tif: self.exit_tif,
            key,
        }
    }
}

/// A refusal as `(code, sentence)`.
type Refusal = (String, String);

fn refusal(code: &str, message: impl Into<String>) -> Refusal {
    (code.to_string(), message.into())
}

/// Read what the templates need: the account and its broker (granted to the chart), the grid,
/// the point value, the ATR. Only what one of `templates` uses is asked for.
pub(crate) async fn prepare(state: &AppState, a: &Alert, account_id: Uuid, templates: &[&TicketTemplate]) -> Result<Prepared, Refusal> {
    let (connector, settings, account) =
        brokers_api::resolve(state, account_id, Some("histviz")).await.map_err(|e| refusal(e.code.unwrap_or("refused"), e.message().to_string()))?;
    let grid = if templates.iter().any(|t| t.needs_grid()) {
        match connector.grid(&settings, Some(&a.provider), &a.ticker, &a.asset_type).await {
            Ok(g) => g,
            Err(e) => return Err(refusal("grid_unread", format!("the tick and size grid of {} could not be read from {} ({e:#}), so nothing was sent", a.ticker, account.name))),
        }
    } else {
        None
    };
    let point_value = if templates.iter().any(|t| t.uses_point_value(&a.asset_type)) {
        crate::instrument_meta_api::resolved(&state.pool, &a.provider, &a.asset_type, &a.ticker)
            .await
            .ok()
            .flatten()
            .and_then(|m| m.pointvalue)
            .and_then(brokers::paper::exact)
            .filter(|v| *v > Decimal::ZERO)
    } else {
        None
    };
    let atr = if templates.iter().any(|t| t.uses_atr()) { atr_of(state, a).await } else { Err("no ATR distance".into()) };
    let caps = connector.order_caps();
    let reduce_only_ok = caps.as_ref().is_some_and(|c| c.reduce_only);
    let exit_tif = match &caps {
        Some(c) if !c.tif.is_empty() && !c.tif.contains(&TifKind::Gtc) && c.tif.contains(&TifKind::Day) => TifKind::Day,
        _ => TifKind::Gtc,
    };
    Ok(Prepared { connector, settings, account, grid, point_value, atr, reduce_only_ok, exit_tif })
}

/// ATR 14 over the alert's stored closed bars. Never read from a provider: a distance worked
/// out from bars nobody stored is refused rather than guessed.
async fn atr_of(state: &AppState, a: &Alert) -> Result<Decimal, String> {
    let none = || {
        format!(
            "ATR distances read the stored bars of {} {}, and fewer than 15 are stored: download its history in Historical Data, or use another distance",
            a.ticker, a.timeframe
        )
    };
    let tf = crate::histdata::timeframe_secs(&a.timeframe).map_err(|e| format!("{e}"))?;
    let ds = otw_store::histdata::find_dataset(&state.pool, &a.provider, &a.asset_type, &a.ticker, &a.timeframe)
        .await
        .map_err(|e| format!("{e:#}"))?
        .ok_or_else(none)?;
    let bars = otw_store::histdata::read_bars(&state.pool, ds.id, None, None, 200).await.map_err(|e| format!("{e:#}"))?;
    let bars = alerts::closed_bars(&bars, tf, OffsetDateTime::now_utc());
    if bars.len() < 15 {
        return Err(none());
    }
    let (h, l, c): (Vec<f64>, Vec<f64>, Vec<f64>) = (bars.iter().map(|b| b.high).collect(), bars.iter().map(|b| b.low).collect(), bars.iter().map(|b| b.close).collect());
    crate::backtest::indicators::atr(&h, &l, &c, 14)
        .last()
        .copied()
        .flatten()
        .and_then(brokers::paper::exact)
        .filter(|v| *v > Decimal::ZERO)
        .ok_or_else(none)
}

fn parse_template(row: &AlertTrade) -> Result<(Intent, TicketTemplate), Refusal> {
    let intent = Intent::parse(&row.intent).map_err(|m| refusal("ticket_refused", m))?;
    let t: TicketTemplate = serde_json::from_value(row.ticket.clone()).map_err(|e| refusal("ticket_refused", format!("the ticket cannot be read ({e}): open the alert and save its trade again")))?;
    let t = t.validate(intent).map_err(|m| refusal("ticket_refused", m))?;
    Ok((intent, t))
}

/// Run a taken trade: send it, record how it went, notify, run the fallback.
async fn run_taken(state: &AppState, a: &Alert, row: AlertTrade, trigger: Option<Trigger>) -> Fired {
    let source = Source::Alert(a.id);
    let (fired, trig, order) = match trigger_price(state, a, trigger).await {
        Err(m) => (Fired::Refused { code: "price_unread".into(), message: m, fallback: None }, None, String::new()),
        Ok((price, at_ms)) => {
            let (fired, order) = send(state, a, &row, price, at_ms, source).await;
            (fired, Some((price, at_ms)), order)
        }
    };
    // The fallback, when it applies: never after a refusal that had nothing to act on.
    let fired = match fired {
        Fired::Refused { code, message, .. } if code != "nothing_held" && row.fallback != "none" => {
            let fb = fallback(state, a, row.account_id, &row.fallback, source).await;
            Fired::Refused { code, message, fallback: fb }
        }
        Fired::Unknown { keys, message, .. } => {
            let fb = fallback(state, a, row.account_id, template::fallback_for(&row.fallback, true), source).await;
            Fired::Unknown { keys, message, fallback: fb }
        }
        other => other,
    };
    let account = otw_store::brokers::get(&state.pool, row.account_id).await.ok().flatten().map(|r| r.name).unwrap_or_default();
    let status = match &fired {
        Fired::NotArmed => "idle",
        Fired::Sent { .. } => "sent",
        Fired::Refused { fallback: Some(f), .. } => {
            if f.ok() {
                "fallback_done"
            } else {
                "fallback_failed"
            }
        }
        Fired::Refused { .. } => "refused",
        Fired::Unknown { .. } => "unknown",
    };
    let mut result = serde_json::to_value(&fired).unwrap_or(Value::Null);
    if let Value::Object(m) = &mut result {
        m.insert("account".into(), json!(account));
        if let Some((price, at)) = trig {
            m.insert("trigger".into(), json!({ "price": price.normalize().to_string(), "at": at }));
        }
    }
    if let Err(e) = store::finish(&state.pool, a.id, status, &result).await {
        tracing::error!("trade on alert {}: could not record how it went ({status}): {e:#}", a.id);
    }
    tell(state, a, &fired, &account, &order).await;
    fired
}

/// The notification for a fire's outcome, and a second one when a fallback left something open.
async fn tell(state: &AppState, a: &Alert, fired: &Fired, account: &str, order: &str) {
    let name = alerts::title_of(a);
    match fired {
        Fired::NotArmed => {}
        Fired::Sent { placed, .. } => {
            let ids: Vec<String> = placed.iter().map(|p| p.id.clone()).collect();
            notify(state, a, format!("Trade sent: {name}"), format!("Sent to {account}: {order}. Broker order ids: {}.", ids.join(", "))).await;
        }
        Fired::Refused { message, fallback, .. } => {
            let fb = fallback.as_ref().map(|f| format!(" {}", f.sentence())).unwrap_or_default();
            notify(state, a, format!("Trade refused: {name}"), format!("The order to {account} was not placed: {message}.{fb}")).await;
        }
        Fired::Unknown { message, fallback, .. } => {
            let fb = fallback.as_ref().map(|f| format!(" {}", f.sentence())).unwrap_or_default();
            notify(
                state,
                a,
                format!("Trade outcome unknown: {name}"),
                format!("The order to {account} got no answer ({message}). It may be working: check it at the broker. It is never sent again, and nothing new goes out on {} until it is settled.{fb}", a.ticker),
            )
            .await;
        }
    }
    let failed = match fired {
        Fired::Refused { fallback: Some(f), .. } | Fired::Unknown { fallback: Some(f), .. } if !f.ok() => Some(f),
        _ => None,
    };
    if let Some(f) = failed {
        notify(state, a, format!("Fallback failed: {name}"), format!("The fallback ({}) on {account} could not finish. Still open: {}", f.kind.replace('_', " "), f.failed.join(" "))).await;
    }
}

/// Send the trade at `price`. The order as a sentence comes back with the outcome.
async fn send(state: &AppState, a: &Alert, row: &AlertTrade, price: Decimal, at_ms: i64, source: Source) -> (Fired, String) {
    if crate::demo::enabled() {
        return (Fired::Refused { code: "demo_disabled".into(), message: "orders are never sent from the public demo".into(), fallback: None }, String::new());
    }
    let (intent, t) = match parse_template(row) {
        Ok(v) => v,
        Err((code, message)) => return (Fired::Refused { code, message, fallback: None }, String::new()),
    };
    if intent == Intent::Exit {
        return (exit(state, a, row.account_id, source).await, format!("close {} at the market", a.ticker));
    }
    let p = match prepare(state, a, row.account_id, &[&t]).await {
        Ok(p) => p,
        Err((code, message)) => return (Fired::Refused { code, message, fallback: None }, String::new()),
    };
    let key = mint_key();
    let m: Materialized = match template::materialize(&t, &p.cx(a, price, at_ms, &key)) {
        Ok(m) => m,
        Err(message) => return (Fired::Refused { code: "ticket_refused".into(), message, fallback: None }, String::new()),
    };
    let order = template::describe(&m, &a.ticker, t.order_type);
    let keys: Vec<String> = m.ticket.legs.iter().map(|l| l.key.clone()).collect();
    let fired = match dispatch::send_internal(state, row.account_id, "histviz", source, m.ticket).await {
        Ok(placed) => Fired::Sent { keys, placed, order: order.clone() },
        Err(SendRefusal::Refused { code, message }) => Fired::Refused { code: code.into(), message, fallback: None },
        Err(SendRefusal::Unknown { keys, message }) => Fired::Unknown { keys, message, fallback: None },
    };
    (fired, order)
}

/// The account behind a background order that only takes risk off (an exit, a fallback's
/// close): the trading grant, the chart's grant, the background trading permission. No live
/// gate: closing never needs it.
async fn closing_account(state: &AppState, account_id: Uuid) -> Result<(Box<dyn Broker>, HashMap<String, String>, BrokerRow), Refusal> {
    let (connector, mut settings, account) =
        brokers_api::resolve_trading(state, account_id, Some("histviz")).await.map_err(|e| refusal(e.code.unwrap_or("refused"), e.message().to_string()))?;
    if account.background_trading_ack.is_none() {
        return Err(refusal(
            dispatch::BACKGROUND_REQUIRED,
            format!("background trading is off for “{}”: sign it on the account in Settings, Brokers", account.name),
        ));
    }
    brokers::live::mark_canary(account.id, &mut settings);
    Ok((connector, settings, account))
}

/// How the account spells the alert's instrument, asked of the broker, else the chart's own.
async fn spelling(connector: &dyn Broker, settings: &HashMap<String, String>, a: &Alert) -> String {
    connector.symbol_for(settings, &a.ticker, &a.asset_type).await.ok().flatten().unwrap_or_else(|| a.ticker.clone())
}

/// `exit`: cancel the instrument's working orders, then close what is held at the market.
async fn exit(state: &AppState, a: &Alert, account_id: Uuid, source: Source) -> Fired {
    let (connector, settings, account) = match closing_account(state, account_id).await {
        Ok(v) => v,
        Err((code, message)) => return Fired::Refused { code, message, fallback: None },
    };
    let sym = spelling(connector.as_ref(), &settings, a).await;
    let mut report = FallbackReport::default();
    cancel_on(state, &account, connector.as_ref(), &settings, &sym, source, &mut report).await;
    let held = match held_on(connector.as_ref(), &settings, Some(&sym)).await {
        Ok(h) => h,
        Err(e) => return Fired::Refused { code: "positions_unread".into(), message: format!("the positions of {} could not be read ({e}), so nothing was closed", account.name), fallback: None },
    };
    if held.is_empty() {
        let cancelled = if report.cancelled.is_empty() { String::new() } else { format!("; {} working order(s) were cancelled", report.cancelled.len()) };
        return Fired::Refused { code: "nothing_held".into(), message: format!("nothing was held on {} when the alert fired{cancelled}", a.ticker), fallback: None };
    }
    let (mut keys, mut placed) = (Vec::new(), Vec::new());
    for p in &held {
        match close_one(state, &account, connector.as_ref(), &settings, p, source).await {
            Ok(pl) => {
                keys.push(pl.key.clone());
                placed.push(pl);
            }
            Err(Closing::Unknown(k, message)) => return Fired::Unknown { keys: vec![k], message, fallback: None },
            Err(Closing::Refused(message)) => return Fired::Refused { code: "refused".into(), message, fallback: None },
        }
    }
    Fired::Sent { keys, placed, order: format!("close {} at the market", a.ticker) }
}

/// Positions held, on one instrument (`sym`) or all.
async fn held_on(connector: &dyn Broker, settings: &HashMap<String, String>, sym: Option<&str>) -> Result<Vec<Position>, String> {
    if !connector.capability().positions {
        return Err(format!("{} does not report positions", connector.capability().label));
    }
    let http = brokers::client().map_err(|e| format!("{e:#}"))?;
    Ok(connector
        .positions(&http, settings)
        .await
        .map_err(|e| format!("{e:#}"))?
        .into_iter()
        .filter(|p| p.qty.abs() > 0.0 && sym.is_none_or(|s| p.symbol.eq_ignore_ascii_case(s)))
        .collect())
}

enum Closing {
    Refused(String),
    Unknown(String, String),
}

/// Close one position at the market, the way the close route does: stored before it leaves,
/// the answer recorded, the call audited with its source.
async fn close_one(state: &AppState, account: &BrokerRow, connector: &dyn Broker, settings: &HashMap<String, String>, p: &Position, source: Source) -> Result<Placed, Closing> {
    let Some(native) = p.native_id.clone() else {
        return Err(Closing::Refused(format!("the {} position on {} carries no id to close it by: close it at the broker", p.side, p.symbol)));
    };
    let key = mint_key();
    let request = json!({ "native_id": native, "account": p.account, "key": key });
    let keys = vec![key.clone()];
    let mut unknown = false;
    let call = async {
        brokers::reconcile::claim_close(state, account, &native, &key).await?;
        let out = connector.close_position(settings, &native, &p.account, &key).await.map(|p| vec![p]);
        brokers::reconcile::record(state, account, &keys, &out).await;
        if let Err(e) = &out {
            unknown = e.downcast_ref::<OrderUnknown>().is_some();
        }
        out.map(|mut v| v.remove(0))
    };
    match brokers_api::audited_from(state, account, "close", source, request, call).await {
        Ok(placed) => Ok(placed),
        Err(e) if unknown => Err(Closing::Unknown(key, e.message().to_string())),
        Err(e) => Err(Closing::Refused(format!("the {} {} {} position is still open: the close was refused ({})", p.side, p.qty, p.symbol, e.message()))),
    }
}

/// Cancel the account's working orders on `sym`, each call audited. Never gated.
async fn cancel_on(state: &AppState, account: &BrokerRow, connector: &dyn Broker, settings: &HashMap<String, String>, sym: &str, source: Source, report: &mut FallbackReport) {
    if !connector.capability().orders {
        report.failed.push(format!("{} does not list working orders, so none on {sym} could be cancelled: check at the broker.", connector.capability().label));
        return;
    }
    let working = match brokers::client() {
        Ok(http) => connector.orders(&http, settings).await,
        Err(e) => Err(e),
    };
    let working = match working {
        Ok(w) => w,
        Err(e) => {
            report.failed.push(format!("the working orders on {sym} could not be read ({e:#}): check at the broker."));
            return;
        }
    };
    for o in working.iter().filter(|o| o.symbol.eq_ignore_ascii_case(sym)) {
        let call = connector.cancel(settings, &o.id);
        match brokers_api::audited_from(state, account, "cancel", source, json!({ "order": o.id }), call).await {
            Ok(()) => report.cancelled.push(o.id.clone()),
            Err(e) => report.failed.push(format!("order {} on {sym} is still working: the cancel was refused ({}).", o.id, e.message())),
        }
    }
}

/// Run a fallback. `None` for `none`.
async fn fallback(state: &AppState, a: &Alert, account_id: Uuid, kind: &str, source: Source) -> Option<FallbackReport> {
    if kind == "none" {
        return None;
    }
    let mut report = FallbackReport { kind: kind.to_string(), ..Default::default() };
    // Cancelling needs the account and the chart's grant, nothing more.
    let (connector, settings, account) = match brokers_api::resolve(state, account_id, Some("histviz")).await {
        Ok(v) => v,
        Err(e) => {
            report.failed.push(format!("the account could not be reached ({}): check at the broker.", e.message()));
            return Some(report);
        }
    };
    let sym = spelling(connector.as_ref(), &settings, a).await;
    if kind == "flatten_account" {
        match brokers_api::audited_from(state, &account, "cancel_all", source, json!({}), connector.cancel_all(&settings)).await {
            Ok(n) => report.cancelled.extend((0..n).map(|i| format!("#{}", i + 1))),
            Err(e) => report.failed.push(format!("the account's working orders could not be cancelled ({}).", e.message())),
        }
    } else {
        cancel_on(state, &account, connector.as_ref(), &settings, &sym, source, &mut report).await;
    }
    if kind == "cancel_symbol" {
        return Some(report);
    }
    let (connector, settings, account) = match closing_account(state, account_id).await {
        Ok(v) => v,
        Err((_, m)) => {
            report.failed.push(format!("no position was closed: {m}."));
            return Some(report);
        }
    };
    let only = (kind == "close_symbol").then_some(sym.as_str());
    let held = match held_on(connector.as_ref(), &settings, only).await {
        Ok(h) => h,
        Err(e) => {
            report.failed.push(format!("the positions could not be read ({e}): check at the broker."));
            return Some(report);
        }
    };
    for p in &held {
        match close_one(state, &account, connector.as_ref(), &settings, p, source).await {
            Ok(_) => report.closed.push(format!("{} {}", p.qty, p.symbol)),
            Err(Closing::Refused(m)) => report.failed.push(format!("{m}.")),
            Err(Closing::Unknown(k, m)) => report.failed.push(format!("the close {k} of {} {} got no answer ({m}): check at the broker.", p.qty, p.symbol)),
        }
    }
    Some(report)
}

// ── Following placed orders ──────────────────────────────────────────────────

/// After a restart: a trade caught mid-send is unknown, never sent again, and said so.
async fn restart(state: &AppState) -> Result<()> {
    for row in store::sending_to_unknown(&state.pool).await? {
        if let Some(a) = otw_store::histviz::get_alert(&state.pool, row.alert_id).await? {
            notify(
                state,
                &a,
                format!("Trade outcome unknown: {}", alerts::title_of(&a)),
                format!("The app stopped while the order of this alert was being sent. It is never sent again: check at the broker whether it went out on {}.", a.ticker),
            )
            .await;
        }
    }
    Ok(())
}

/// The entry's client id of a fire's result: the first key it sent.
fn entry_key(row: &AlertTrade) -> Option<String> {
    row.last_result.get("keys").and_then(|k| k.get(0)).and_then(Value::as_str).map(str::to_string)
}

/// One look at every followed trade: a fill is notified once, a placed order that ended
/// without one is recorded, an unknown one the reconciliation settled is followed again.
pub async fn follow(state: &AppState) -> Result<()> {
    for row in store::followed(&state.pool).await? {
        let Some(key) = entry_key(&row) else { continue };
        let Some(leg) = otw_store::broker_orders::get(&state.pool, row.account_id, &key).await? else { continue };
        let filled: Decimal = leg.filled_qty.as_deref().and_then(|q| q.parse().ok()).unwrap_or_default();
        let ended = matches!(leg.status.as_str(), "cancelled" | "expired" | "rejected" | "not_received" | "dismissed" | "resent");
        if leg.status == "filled" || (ended && filled > Decimal::ZERO) {
            let patch = json!({ "filled_qty": filled.normalize().to_string() });
            if store::advance(&state.pool, row.alert_id, store::FOLLOWED, "filled", &patch).await? {
                filled_notice(state, &row, &leg, filled).await;
            }
        } else if ended {
            let patch = json!({ "ended": leg.status, "reason": leg.reason });
            let to = if row.status == "unknown" { "refused" } else { "cancelled" };
            if store::advance(&state.pool, row.alert_id, store::FOLLOWED, to, &patch).await? && matches!(leg.status.as_str(), "expired" | "rejected") {
                if let Some(a) = otw_store::histviz::get_alert(&state.pool, row.alert_id).await? {
                    let why = leg.reason.clone().unwrap_or_default();
                    notify(state, &a, format!("Trade ended unfilled: {}", alerts::title_of(&a)), format!("The order of this alert on {} {} without a fill. {why}", a.ticker, leg.status)).await;
                }
            }
        } else if row.status == "unknown" && leg.status == "working" {
            store::advance(&state.pool, row.alert_id, &["unknown"], "sent", &json!({ "settled": true })).await?;
        }
    }
    Ok(())
}

/// The fill, notified once with the price and fees the broker reports.
async fn filled_notice(state: &AppState, row: &AlertTrade, leg: &otw_store::broker_orders::Leg, filled: Decimal) {
    let Ok(Some(a)) = otw_store::histviz::get_alert(&state.pool, row.alert_id).await else { return };
    let account = otw_store::brokers::get(&state.pool, row.account_id).await.ok().flatten();
    let name = account.as_ref().map(|r| r.name.clone()).unwrap_or_default();
    let detail = match (&account, &leg.venue_id) {
        (Some(acc), Some(id)) => fill_figures(state, acc, &a, id, row.fired_at).await,
        _ => None,
    };
    let details = match detail {
        Some((avg, fee, ccy)) => format!("Filled on {name}: {} {} at {avg}, fees {fee} {ccy}.", filled.normalize(), a.ticker),
        None => format!("Filled on {name}: {} {}. The broker has not stated the price and fees yet: see the account's fills.", filled.normalize(), a.ticker),
    };
    notify(state, &a, format!("Trade filled: {}", alerts::title_of(&a)), details).await;
}

/// The average price and the fees of an order's fills, as the broker reports them.
async fn fill_figures(state: &AppState, account: &BrokerRow, a: &Alert, venue_id: &str, since: Option<OffsetDateTime>) -> Option<(String, String, String)> {
    let connector = brokers::for_account(&state.pool, account).ok()?;
    if !connector.chart_fills() {
        return None;
    }
    let settings = otw_store::brokers::load_creds(&state.pool, &state.cipher, account.id).await.ok()?;
    let sym = spelling(connector.as_ref(), &settings, a).await;
    let from = since.unwrap_or_else(OffsetDateTime::now_utc) - time::Duration::minutes(5);
    let q = brokers::ExecQuery { from, to: OffsetDateTime::now_utc(), symbols: vec![sym] };
    let http = brokers::client().ok()?;
    let fills: Vec<_> = connector.executions(&http, &settings, &q).await.ok()?.into_iter().filter(|f| f.order_id == venue_id).collect();
    let qty: f64 = fills.iter().map(|f| f.qty).sum();
    if fills.is_empty() || qty <= 0.0 {
        return None;
    }
    let avg = fills.iter().map(|f| f.qty * f.price).sum::<f64>() / qty;
    let fee: f64 = fills.iter().map(|f| f.fee).sum();
    let ccy = fills.first().map(|f| f.fee_currency.clone()).unwrap_or_default();
    Some((short(avg), short(fee), ccy))
}

/// A figure for a notification: a fill's price as the broker reported it, trimmed.
fn short(v: f64) -> String {
    let s = format!("{v:.8}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

// ── Preview ──────────────────────────────────────────────────────────────────

/// What is already armed on the account and instrument, at `price`: the line the dialog shows
/// before the user arms one more.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Exposure {
    pub count: usize,
    /// Units bought and sold by the armed entries, materialized at the same price.
    pub buy: String,
    pub sell: String,
    /// Armed exits (close what is held).
    pub exits: usize,
    /// Armed entries whose size cannot be worked out at this price.
    pub unpriced: usize,
}

pub(crate) async fn exposure(state: &AppState, a: &Alert, p: &mut Prepared, price: Decimal, at_ms: i64) -> Result<Exposure> {
    let rows = store::armed_on(&state.pool, p.account.id, &a.asset_type, &a.ticker, a.id).await?;
    let mut out = Exposure { count: rows.len(), ..Default::default() };
    let (mut buy, mut sell) = (Decimal::ZERO, Decimal::ZERO);
    for r in rows {
        if r.intent == "exit" {
            out.exits += 1;
            continue;
        }
        let Some(t) = serde_json::from_value::<TicketTemplate>(r.ticket).ok().and_then(|t| t.validate(Intent::Enter).ok()) else {
            out.unpriced += 1;
            continue;
        };
        if t.uses_atr() && p.atr.is_err() {
            p.atr = atr_of(state, a).await;
        }
        match template::materialize(&t, &p.cx(a, price, at_ms, "x")) {
            Ok(m) if m.side == Side::Buy => buy += m.qty,
            Ok(m) => sell += m.qty,
            Err(_) => out.unpriced += 1,
        }
    }
    out.buy = buy.normalize().to_string();
    out.sell = sell.normalize().to_string();
    Ok(out)
}

/// Working orders and positions on the alert's instrument: what an exit would act on now.
pub(crate) async fn exit_preview(p: &Prepared, a: &Alert) -> Value {
    let sym = spelling(p.connector.as_ref(), &p.settings, a).await;
    let positions = held_on(p.connector.as_ref(), &p.settings, Some(&sym)).await;
    let orders = match (p.connector.capability().orders, brokers::client()) {
        (true, Ok(http)) => p.connector.orders(&http, &p.settings).await.map(|o| o.iter().filter(|o| o.symbol.eq_ignore_ascii_case(&sym)).count()).map_err(|e| format!("{e:#}")),
        _ => Err("this broker does not list working orders".into()),
    };
    json!({
        "positions": positions.as_ref().map(|v| v.iter().map(|p| json!({ "side": p.side, "qty": p.qty, "symbol": p.symbol })).collect::<Vec<_>>()).unwrap_or_default(),
        "positions_error": positions.err(),
        "orders": orders.as_ref().ok(),
        "orders_error": orders.err(),
    })
}

/// The order a template makes at `price`, for the dialog: or the sentence it is refused with.
pub(crate) fn preview_order(p: &Prepared, a: &Alert, t: &TicketTemplate, price: Decimal, at_ms: i64, key: &str) -> Result<(Materialized, String), String> {
    let m = template::materialize(t, &p.cx(a, price, at_ms, key))?;
    let order = template::describe(&m, &a.ticker, t.order_type);
    Ok((m, order))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn a_minted_key_fits_every_venue() {
        let k = mint_key();
        assert!(k.len() <= 20, "{k}");
        assert!(k.chars().all(|c| c.is_ascii_alphanumeric()), "{k}");
        assert_ne!(k, mint_key());
    }

    #[test]
    fn a_fallback_report_names_what_is_still_open() {
        let r = FallbackReport { kind: "close_symbol".into(), cancelled: vec!["1".into()], closed: vec![], failed: vec!["the long 0.5 BTCUSDT position is still open.".into()] };
        assert!(!r.ok());
        let s = r.sentence();
        assert!(s.contains("close symbol") && s.contains("1 working order(s) cancelled") && s.contains("Still open: the long 0.5"), "{s}");
        assert!(FallbackReport { kind: "cancel_symbol".into(), ..Default::default() }.sentence().contains("nothing was working"));
    }
}
