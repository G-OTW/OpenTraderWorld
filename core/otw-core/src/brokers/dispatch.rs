//! The server-side order sender: one path for an order nobody is watching leave, and the same
//! path the chart's own orders take.
//!
//! The chart sends through `POST /api/brokers/{id}/orders`, whose trading lock check needs a
//! browser session that unlocked trading. A trade on alert or a strategy on a broker account
//! has no browser, so [`send_internal`] replaces that one check by a standing, explicit
//! authorization: the account's background trading permission ([`super::background`]),
//! read again at every send. Every other gate is the route's, in the route's order, because
//! it is the same code ([`send_resolved`]):
//!
//! 1. the account carries the `trading` grant and is granted to the sending module;
//! 2. (route only) the trading lock; (background only) the background trading permission;
//! 3. the live gate (`brokers::live`): live trading on and the limits set, on an account that
//!    moves real money;
//! 4. the risk gate (`brokers::risk`), the price collar measuring from the reference the
//!    sender passed (`reference`, `reference_at`: the price that fired, and when);
//! 5. the legs stored under their client ids before anything leaves (`reconcile::claim`);
//! 6. the broker's place, its answer stored (`reconcile::record`);
//! 7. all of it written to the audit log with who sent it (`broker_order_log.source`).
//!
//! **Unknown is not a refusal.** An order whose answer was lost may be working at the broker.
//! [`SendRefusal::Unknown`] says so apart from every refusal, so a caller does the only safe
//! thing: cancel what it owns, add no exposure, tell the user to check at the broker. The
//! instrument stays blocked until reconciliation settles the order, as for the chart.
//!
//! **Disarming.** Rows that may send in the background (an armed trade on alert, a strategy
//! executing on an account) are disarmed when the
//! permission is taken back, when trading is turned off for the account, when the second factor
//! goes, when the trading lock is turned on, and on the kill switch. [`disarm_account`] and
//! [`disarm_all`] are where that happens; the sends are refused anyway by the checks above, the
//! disarm is so the user re-arms knowingly instead of finding an order armed again later.

// The trade on alert and a trading robot on a broker account (`Source::Strategy`) both send
// through this path.
#![cfg_attr(not(test), allow(dead_code))]

use std::collections::HashMap;

use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use super::{Broker, OrderTicket, OrderUnknown, Placed};
use crate::{brokers_api, ApiError, AppState};
use otw_store::brokers::BrokerRow;

/// Who sends an order. Written to the audit log, so the Logs tab and the audit answer "who
/// sent this".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Someone at the chart, through the order routes.
    Session,
    /// A trade on alert, by the alert's id.
    Alert(Uuid),
    /// A robot executing its strategy on a broker account, by the robot's id.
    Strategy(Uuid),
}

impl Source {
    /// The value stored in `broker_order_log.source`.
    pub fn tag(&self) -> String {
        match self {
            Source::Session => "session".into(),
            Source::Alert(id) => format!("alert:{id}"),
            Source::Strategy(id) => format!("strategy:{id}"),
        }
    }

    /// Whether nobody is watching this send: it then needs background trading.
    pub fn unattended(&self) -> bool {
        !matches!(self, Source::Session)
    }
}

/// Why [`send_internal`] placed nothing, or may have placed something it cannot confirm.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum SendRefusal {
    /// Nothing was placed: a gate refused, or the broker answered no. `code` is the route's
    /// code when it has one (`background_trading_required`, `risk_limit`, `trading_halted`,
    /// `live_off`, `live_limits_missing`, `demo_disabled`...), `refused` otherwise.
    Refused { code: &'static str, message: String },
    /// The broker's answer was lost: the legs named may be working. Never send them again;
    /// cancel what the caller owns, add no exposure, tell the user to check at the broker.
    Unknown { keys: Vec<String>, message: String },
}

impl SendRefusal {
    pub fn message(&self) -> &str {
        match self {
            SendRefusal::Refused { message, .. } | SendRefusal::Unknown { message, .. } => message,
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            SendRefusal::Refused { code, .. } => code,
            SendRefusal::Unknown { .. } => "order_unknown",
        }
    }

    fn refused(code: &'static str, message: String) -> Self {
        SendRefusal::Refused { code, message }
    }

    fn from_api(e: ApiError) -> Self {
        SendRefusal::Refused { code: e.code.unwrap_or("refused"), message: e.message }
    }
}

impl std::fmt::Display for SendRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for SendRefusal {}

/// The code a background send is refused with when the account has no background trading.
pub const BACKGROUND_REQUIRED: &str = "background_trading_required";

/// A failure of the shared path: the route answers `api` as it always did; the internal
/// sender also needs to know whether the outcome is unknown.
pub(crate) struct Failure {
    pub api: ApiError,
    pub unknown: Option<Vec<String>>,
}

impl From<Failure> for SendRefusal {
    fn from(f: Failure) -> Self {
        match f.unknown {
            Some(keys) => SendRefusal::Unknown { keys, message: f.api.message },
            None => SendRefusal::from_api(f.api),
        }
    }
}

/// Demo mode sends nothing, before any work.
fn refuse_demo(demo: bool) -> Result<(), SendRefusal> {
    if demo {
        return Err(SendRefusal::refused("demo_disabled", "orders are never sent from the public demo".into()));
    }
    Ok(())
}

/// The background half of the gates, over what was read: the permission for a sender nobody
/// watches, and the price that fired, which the collar measures from and the log keeps.
pub fn admit(source: Source, account: &BrokerRow, ticket: &OrderTicket) -> Result<(), SendRefusal> {
    if !source.unattended() {
        return Ok(());
    }
    if account.background_trading_ack.is_none() {
        return Err(SendRefusal::refused(
            BACKGROUND_REQUIRED,
            format!("background trading is off for “{}”: sign it on the account in Settings, Brokers", account.name),
        ));
    }
    let priced = ticket.reference.is_some_and(|r| r > otw_connector_model::Decimal::ZERO) && ticket.reference_at.is_some();
    if !priced {
        return Err(SendRefusal::refused(
            "reference_missing",
            format!("the order on {} came without the price that fired it and its time, so nothing was sent", ticket.ticker),
        ));
    }
    Ok(())
}

/// Send a ticket with no browser attached. `module` is the sending module (`histviz` for a
/// trade on alert, `robots` for a trading robot), which must be granted the account.
///
/// Placed legs come back as the route returns them. A refusal is one sentence naming the fix;
/// an unknown outcome is [`SendRefusal::Unknown`], never a refusal.
pub async fn send_internal(
    state: &AppState,
    account_id: Uuid,
    module: &str,
    source: Source,
    ticket: OrderTicket,
) -> Result<Vec<Placed>, SendRefusal> {
    refuse_demo(crate::demo::enabled())?;
    let (connector, settings, account) =
        brokers_api::resolve_trading(state, account_id, Some(module)).await.map_err(SendRefusal::from_api)?;
    admit(source, &account, &ticket)?;
    send_resolved(state, connector, settings, &account, source, ticket).await.map_err(SendRefusal::from)
}

/// Option orders stay off for now, on every account, paper included: the pricing is a European
/// model on American contracts and no gate here counts their multiplier or margin. Every sender
/// passes [`send_resolved`], so this is the one place to lift it. Taking risk off stays open:
/// closing a position, changing and cancelling an order do not come through here.
fn refuse_options(ticket: &OrderTicket) -> Result<(), ApiError> {
    if matches!(ticket.asset_type.trim().to_ascii_lowercase().as_str(), "option" | "options") {
        return Err(ApiError::bad_request(
            "OpenTraderWorld does not send option orders yet. Place this one at your broker; an open position can still be closed from here",
        ));
    }
    Ok(())
}

/// The ticket as the risk gate values it: a leg sized in the quote currency of a book that
/// counts in it (Deribit inverse contracts, `qty_unit` quote: USD) is worth its size, so it is
/// handed over as the coins that size buys at the leg's price (its limit, else its stop, else
/// the chart's reference, else `fallback`): the gate's size times price is then the USD amount
/// itself. A leg with no price to convert at, and every other leg, goes as it is.
pub(crate) fn valued_in_coin(ticket: &OrderTicket) -> std::borrow::Cow<'_, OrderTicket> {
    valued_in_coin_at(ticket, None)
}

fn valued_in_coin_at(ticket: &OrderTicket, fallback: Option<otw_connector_model::Decimal>) -> std::borrow::Cow<'_, OrderTicket> {
    use otw_connector_model::SizeUnit;
    let quote = |l: &super::OrderLeg| l.qty_unit == Some(SizeUnit::Quote);
    if !ticket.legs.iter().any(quote) {
        return std::borrow::Cow::Borrowed(ticket);
    }
    let mut t = ticket.clone();
    for l in t.legs.iter_mut().filter(|l| quote(l)) {
        let Some(px) = l.limit.or(l.stop).or(ticket.reference).or(fallback).filter(|p| *p > otw_connector_model::Decimal::ZERO) else { continue };
        if let Some(coins) = l.qty.checked_div(px) {
            l.qty = coins;
            l.qty_unit = Some(SizeUnit::Base);
        }
    }
    std::borrow::Cow::Owned(t)
}

/// The unit the gate reads what is held in, for the unit its legs are in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum HeldUnit {
    /// As the broker reports it: the legs are in that unit.
    AsIs,
    /// A book that sizes in the quote currency reports its positions in it (USD): the legs went
    /// to coins, so do the positions on the ticket's instrument, at this price (the first
    /// entry's, as the leg was valued at).
    InCoinAt(otw_connector_model::Decimal),
    /// The legs are in coins and the positions in USD, with no price to convert at: what is
    /// held is not read, rather than compared across units.
    Unknown,
}

/// The ticket the gate is given and the unit it must read what is held in, so both are in one
/// unit: on a book that sizes in the quote currency (`quote_book`), positions come in USD
/// while the gate's legs are coins (sized in the coin, or valued in it). Everywhere else the
/// legs are [`valued_in_coin`] and the positions read as they are.
pub(crate) fn for_gate(ticket: &OrderTicket, quote_book: bool) -> (std::borrow::Cow<'_, OrderTicket>, HeldUnit) {
    use otw_connector_model::{Decimal, SizeUnit};
    if !quote_book {
        return (valued_in_coin(ticket), HeldUnit::AsIs);
    }
    let entry = |l: &&super::OrderLeg| l.parent.as_deref().is_none_or(str::is_empty);
    let px = ticket.legs.iter().find(entry).and_then(|l| l.limit.or(l.stop)).or(ticket.reference).filter(|p| *p > Decimal::ZERO);
    match px {
        Some(px) => (valued_in_coin_at(ticket, Some(px)), HeldUnit::InCoinAt(px)),
        // No price: entries all in USD compare with USD positions as they are.
        None if ticket.legs.iter().filter(entry).all(|l| l.qty_unit == Some(SizeUnit::Quote)) => (std::borrow::Cow::Borrowed(ticket), HeldUnit::AsIs),
        None => (valued_in_coin(ticket), HeldUnit::Unknown),
    }
}

/// The account as the risk gate reads it: the broker itself, with what is held on the
/// ticket's instrument in the unit of the gate's legs ([`HeldUnit`]). The gate asks it for
/// the account's equity, the asset held, the positions and the instrument's spelling only.
pub(crate) struct GateView<'a> {
    pub inner: &'a dyn Broker,
    pub ticker: &'a str,
    pub asset_type: &'a str,
    pub held: HeldUnit,
}

#[async_trait::async_trait]
impl Broker for GateView<'_> {
    fn capability(&self) -> &'static super::BrokerCapability {
        self.inner.capability()
    }

    async fn account(&self, settings: &HashMap<String, String>, symbol: Option<&str>) -> anyhow::Result<Vec<super::AccountSummary>> {
        self.inner.account(settings, symbol).await
    }

    async fn held_asset(&self, settings: &HashMap<String, String>, ticker: &str) -> anyhow::Result<Option<otw_connector_model::Decimal>> {
        self.inner.held_asset(settings, ticker).await
    }

    async fn symbol_for(&self, settings: &HashMap<String, String>, ticker: &str, asset_type: &str) -> anyhow::Result<Option<String>> {
        self.inner.symbol_for(settings, ticker, asset_type).await
    }

    async fn positions(&self, client: &reqwest::Client, settings: &HashMap<String, String>) -> anyhow::Result<Vec<super::Position>> {
        let px = match self.held {
            HeldUnit::AsIs => return self.inner.positions(client, settings).await,
            HeldUnit::Unknown => anyhow::bail!("{} counts in USD and the order in the coin, with no price to compare them at", self.ticker),
            HeldUnit::InCoinAt(px) => px,
        };
        let mut rows = self.inner.positions(client, settings).await?;
        let spelled = self.inner.symbol_for(settings, self.ticker, self.asset_type).await.ok().flatten();
        for p in rows.iter_mut().filter(|p| p.symbol.eq_ignore_ascii_case(self.ticker) || spelled.as_deref().is_some_and(|s| p.symbol.eq_ignore_ascii_case(s))) {
            let usd: otw_connector_model::Decimal = format!("{:.10}", p.qty).parse().unwrap_or_default();
            p.qty = usd.checked_div(px).and_then(|c| c.to_string().parse().ok()).unwrap_or_default();
        }
        Ok(rows)
    }
}

/// What is held on the ticket's instrument as the gate reads it (the same reading as the
/// gate's), signed, long positive.
async fn held_on(view: &GateView<'_>, settings: &HashMap<String, String>) -> Option<otw_connector_model::Decimal> {
    if let Some(q) = view.held_asset(settings, view.ticker).await.ok()? {
        return Some(q);
    }
    let rows = view.positions(&super::client().ok()?, settings).await.ok()?;
    let spelled = view.symbol_for(settings, view.ticker, view.asset_type).await.ok().flatten().unwrap_or_else(|| view.ticker.to_string());
    let mut total = otw_connector_model::Decimal::ZERO;
    for p in rows.iter().filter(|p| p.symbol.eq_ignore_ascii_case(&spelled) || p.symbol.eq_ignore_ascii_case(view.ticker)) {
        let q: otw_connector_model::Decimal = format!("{:.10}", p.qty).parse().unwrap_or_default();
        total += if p.side == "short" { -q.abs() } else { q.abs() };
    }
    Some(total)
}

/// The duplicate guard on a ticket the gate saw in another unit: the stored requests keep the
/// legs as they were sent (USD), so they are compared with the ticket as sent. Skipped when
/// the ticket only takes risk off, as the gate skips it.
async fn duplicate_as_sent(state: &AppState, account: &BrokerRow, view: &GateView<'_>, settings: &HashMap<String, String>, sent: &OrderTicket, gated: &OrderTicket) -> anyhow::Result<()> {
    use otw_store::broker_risk as store;
    let l = store::limits(&state.pool, account.id).await?;
    let Some(secs) = l.duplicate_secs.filter(|_| !sent.confirm_duplicate) else { return Ok(()) };
    if super::risk::reduces(gated, held_on(view, settings).await) {
        return Ok(());
    }
    let since = time::OffsetDateTime::now_utc() - time::Duration::seconds(i64::from(secs));
    let recent = store::recent_requests(&state.pool, account.id, &sent.ticker, since).await?;
    let only = store::Limits { duplicate_secs: Some(secs), ..Default::default() };
    let facts = super::risk::Facts { recent, ..Default::default() };
    super::risk::check(&only, &facts, sent).map_err(anyhow::Error::new)
}

/// The risk gate for a ticket, its legs and what is held in one unit ([`for_gate`]).
pub(crate) async fn gate_in_one_unit(state: &AppState, account: &BrokerRow, connector: &dyn Broker, settings: &HashMap<String, String>, ticket: &OrderTicket, chart: bool) -> anyhow::Result<()> {
    let quote_book = connector.quote_sized(settings, &ticket.ticker).await.ok().flatten().is_some();
    let (gated, held) = for_gate(ticket, quote_book);
    let view = GateView { inner: connector, ticker: &ticket.ticker, asset_type: &ticket.asset_type, held };
    super::risk::gate(state, account, &view, settings, &gated, chart).await?;
    if matches!(gated, std::borrow::Cow::Owned(_)) {
        duplicate_as_sent(state, account, &view, settings, ticket, &gated).await?;
    }
    Ok(())
}

/// A change of size on a working order of a book that sizes in the quote currency, as the
/// size limit (in coins) reads it: the new USD amount as the coins it buys at the order's
/// price (the new one, else the working one). Every other change goes as it is.
pub(crate) async fn amend_in_coin(state: &AppState, account: &BrokerRow, connector: &dyn Broker, settings: &HashMap<String, String>, order: &str, change: &otw_connector_model::OrderAmend) -> otw_connector_model::OrderAmend {
    use otw_connector_model::Decimal;
    if change.qty.is_none() {
        return *change;
    }
    let limited = otw_store::broker_risk::limits(&state.pool, account.id).await.is_ok_and(|l| l.max_order_qty.is_some());
    if !limited {
        return *change;
    }
    let Ok(client) = super::client() else { return *change };
    let Some(o) = connector.orders(&client, settings).await.ok().and_then(|rows| rows.into_iter().find(|o| o.id == order)) else { return *change };
    if connector.quote_sized(settings, &o.symbol).await.ok().flatten().is_none() {
        return *change;
    }
    let f = |x: f64| format!("{x:.10}").parse::<Decimal>().ok();
    amend_valued(change, o.limit_price.and_then(f).or(o.stop_price.and_then(f)))
}

/// A quote-sized change of size as the coins it buys at the change's price, else `working`
/// (the working order's); with no price, as it is.
fn amend_valued(change: &otw_connector_model::OrderAmend, working: Option<otw_connector_model::Decimal>) -> otw_connector_model::OrderAmend {
    use otw_connector_model::{Decimal, Qty};
    let mut out = *change;
    let px = change.limit.map(|p| p.value()).or(change.stop.map(|p| p.value())).or(working).filter(|p| *p > Decimal::ZERO);
    if let Some(coins) = change.qty.zip(px).and_then(|(q, px)| q.value().checked_div(px)) {
        out.qty = Some(Qty::new(coins));
    }
    out
}

/// Everything after the session or background check, shared by the route and
/// [`send_internal`]: the live gate, the risk gate, the legs stored before anything leaves,
/// the broker's place and its answer stored, the audit log.
pub(crate) async fn send_resolved(
    state: &AppState,
    connector: Box<dyn Broker>,
    mut settings: HashMap<String, String>,
    account: &BrokerRow,
    source: Source,
    ticket: OrderTicket,
) -> Result<Vec<Placed>, Failure> {
    refuse_options(&ticket).map_err(|api| Failure { api, unknown: None })?;
    super::mark_instrument_book(&state.pool, account, ticket.connector, &mut settings).await;
    brokers_api::require_live(state, account, connector.as_ref(), &mut settings)
        .await
        .map_err(|api| Failure { api, unknown: None })?;
    let request = serde_json::to_value(&ticket).unwrap_or(Value::Null);
    let keys: Vec<String> = ticket.legs.iter().map(|l| l.key.clone()).collect();
    let mut unknown: Option<Vec<String>> = None;
    // An order from a chart is held by the charts' freeze; one sent in the background is not.
    let chart = matches!(source, Source::Session);
    let call = async {
        gate_in_one_unit(state, account, connector.as_ref(), &settings, &ticket, chart).await?;
        super::reconcile::claim(state, account, &ticket).await?;
        super::reconcile::mint(state, account, connector.as_ref(), &settings, &ticket.ticker, &keys).await?;
        let out = connector.place(&settings, &ticket).await;
        super::reconcile::record(state, account, &keys, &out).await;
        if let Err(e) = &out {
            unknown = e.downcast_ref::<OrderUnknown>().map(|u| u.keys.clone());
        }
        out
    };
    let res = brokers_api::audited_from(state, account, "place", source, request, call).await;
    res.map_err(|api| Failure { api, unknown })
}

/// What a disarm stopped, per kind of armed row.
#[derive(Debug, Default, Clone, Serialize)]
pub struct Disarmed {
    /// Armed trades on alert (section 3: `alert_trades.armed_at` cleared).
    pub alert_trades: u64,
    /// Strategies executing on the account (section 4: `robots.execution` disarmed).
    pub strategies: u64,
}

/// Disarm every row that may send in the background on one account: called when background
/// trading is turned off for it, or trading is.
///
/// Each kind clears `armed_at` and `armed_by` in one statement and counts what it cleared:
/// the trades on alert (`alert_trades`) and the strategies executing on a broker (`robots::exec`).
/// A disarm never cancels a working order: that is the user's call (the kill switch), and a
/// cancel is never gated anyway.
pub async fn disarm_account(state: &AppState, account_id: Uuid) -> anyhow::Result<Disarmed> {
    let alert_trades = otw_store::alert_trades::disarm_account(&state.pool, account_id).await?;
    let strategies = crate::robots::exec::disarm_where(state, Some(account_id)).await?;
    Ok(Disarmed { alert_trades, strategies })
}

/// [`disarm_account`] on every account: the second factor turned off, the trading lock turned
/// on, the kill switch.
pub async fn disarm_all(state: &AppState) -> anyhow::Result<Disarmed> {
    let alert_trades = otw_store::alert_trades::disarm_all(&state.pool).await?;
    let strategies = crate::robots::exec::disarm_where(state, None).await?;
    Ok(Disarmed { alert_trades, strategies })
}

/// [`disarm_account`] or [`disarm_all`] from a request that already did its job: a disarm
/// that fails is logged, never turned into the request's error, since the sends it would stop
/// are refused at the gate anyway.
pub async fn disarm_logged(state: &AppState, account_id: Option<Uuid>, why: &str) {
    let out = match account_id {
        Some(id) => disarm_account(state, id).await,
        None => disarm_all(state).await,
    };
    match out {
        Ok(d) if d.alert_trades + d.strategies > 0 => {
            tracing::info!("background trading: {why}: disarmed {} trade(s) on alert, {} strateg(ies)", d.alert_trades, d.strategies)
        }
        Ok(_) => {}
        Err(e) => tracing::error!("background trading: {why}: could not disarm: {e:#}"),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use otw_connector_model::{Decimal, OrderType, Side};
    use time::OffsetDateTime;

    fn account(ack: Option<i64>) -> BrokerRow {
        BrokerRow {
            id: Uuid::nil(),
            broker: "paper".into(),
            name: "Paper".into(),
            modules: vec!["*".into(), "trading".into()],
            config: Value::Null,
            created_at: OffsetDateTime::UNIX_EPOCH,
            live_since: None,
            background_trading_ack: ack,
            background_trading_since: ack.map(|_| OffsetDateTime::UNIX_EPOCH),
        }
    }

    fn ticket(reference: Option<Decimal>, at: Option<i64>) -> OrderTicket {
        let leg = super::super::OrderLeg::new("k1", Side::Buy, OrderType::Market, Decimal::ONE);
        OrderTicket { ticker: "BTCUSDT".into(), asset_type: "crypto".into(), legs: vec![leg], provider: None, connector: None, reference, reference_at: at, confirm_duplicate: false }
    }

    #[test]
    fn an_option_order_is_refused_and_nothing_else_is() {
        let of = |kind: &str| OrderTicket { asset_type: kind.into(), ..ticket(None, None) };
        for kind in ["option", "options", " Option "] {
            let e = refuse_options(&of(kind)).unwrap_err();
            assert!(e.message.contains("does not send option orders"), "{}", e.message);
        }
        for kind in ["crypto", "equity", "etf", "future", "fx", "index"] {
            assert!(refuse_options(&of(kind)).is_ok(), "{kind}");
        }
    }

    #[test]
    fn the_source_is_written_as_a_tag() {
        let id = Uuid::from_u128(7);
        assert_eq!(Source::Session.tag(), "session");
        assert_eq!(Source::Alert(id).tag(), format!("alert:{id}"));
        assert_eq!(Source::Strategy(id).tag(), format!("strategy:{id}"));
        assert!(!Source::Session.unattended());
        assert!(Source::Alert(id).unattended() && Source::Strategy(id).unattended());
    }

    #[test]
    fn a_background_send_needs_the_permission() {
        let t = ticket(Some(Decimal::ONE_HUNDRED), Some(1));
        let e = admit(Source::Alert(Uuid::nil()), &account(None), &t).unwrap_err();
        assert_eq!(e.code(), BACKGROUND_REQUIRED);
        assert!(e.message().contains("Settings, Brokers"), "{e}");
        assert_eq!(admit(Source::Strategy(Uuid::nil()), &account(None), &t).unwrap_err().code(), BACKGROUND_REQUIRED);
        // The chart's own orders never ask for it: the session lock is their check.
        assert_eq!(admit(Source::Session, &account(None), &ticket(None, None)), Ok(()));
        assert_eq!(admit(Source::Alert(Uuid::nil()), &account(Some(1)), &t), Ok(()));
    }

    #[test]
    fn a_background_send_carries_the_price_that_fired() {
        let a = account(Some(1));
        for t in [ticket(None, Some(1)), ticket(Some(Decimal::ONE), None), ticket(Some(Decimal::ZERO), Some(1))] {
            assert_eq!(admit(Source::Alert(Uuid::nil()), &a, &t).unwrap_err().code(), "reference_missing");
        }
    }

    #[test]
    fn demo_mode_sends_nothing() {
        assert_eq!(refuse_demo(true).unwrap_err().code(), "demo_disabled");
        assert_eq!(refuse_demo(false), Ok(()));
    }

    #[test]
    fn an_unknown_outcome_is_not_a_refusal() {
        let f = Failure { api: ApiError::bad_request("no answer: check at the broker"), unknown: Some(vec!["k1".into()]) };
        let r = SendRefusal::from(f);
        assert_eq!(r, SendRefusal::Unknown { keys: vec!["k1".into()], message: "no answer: check at the broker".into() });
        assert_eq!(r.code(), "order_unknown");
        let f = Failure { api: ApiError::bad_request_code("risk_limit", "over your size limit"), unknown: None };
        assert_eq!(SendRefusal::from(f), SendRefusal::Refused { code: "risk_limit", message: "over your size limit".into() });
        let f = Failure { api: ApiError::bad_request("venue said no"), unknown: None };
        assert_eq!(SendRefusal::from(f).code(), "refused");
    }

    /// A leg sized in USD on a book that counts in it is valued at its USD size: the gate gets
    /// the coins it buys at the leg's price, so size times price is the USD amount. Coin legs,
    /// and a USD leg with no price at hand, go as they are.
    #[test]
    fn a_quote_sized_leg_is_valued_at_its_quote_size() {
        use otw_connector_model::SizeUnit;
        let mut t = ticket(Some(Decimal::from(50_000)), Some(1));
        t.legs[0].qty = Decimal::from(1_000);
        t.legs[0].qty_unit = Some(SizeUnit::Quote);
        let v = valued_in_coin(&t);
        assert_eq!((v.legs[0].qty, v.legs[0].qty_unit), (Decimal::new(2, 2), Some(SizeUnit::Base)));
        assert_eq!(v.legs[0].qty * t.reference.unwrap(), Decimal::from(1_000));
        let mut limit = t.clone();
        limit.legs[0].limit = Some(Decimal::from(40_000));
        assert_eq!(valued_in_coin(&limit).legs[0].qty, Decimal::new(25, 3));
        let coin = ticket(Some(Decimal::from(50_000)), Some(1));
        assert!(matches!(valued_in_coin(&coin), std::borrow::Cow::Borrowed(_)));
        let mut unpriced = t.clone();
        unpriced.reference = None;
        assert_eq!(valued_in_coin(&unpriced).legs[0].qty, Decimal::from(1_000));
    }

    fn inverse_ticket(side: Side, usd: i64, limit: Option<i64>) -> OrderTicket {
        use otw_connector_model::SizeUnit;
        let mut leg = super::super::OrderLeg::new("k1", side, OrderType::Limit, Decimal::from(usd));
        leg.qty_unit = Some(SizeUnit::Quote);
        leg.limit = limit.map(Decimal::from);
        OrderTicket { ticker: "BTC-PERPETUAL".into(), asset_type: "crypto".into(), legs: vec![leg], provider: None, connector: None, reference: Some(Decimal::from(100_000)), reference_at: Some(1), confirm_duplicate: false }
    }

    struct Held(Vec<super::super::Position>);

    #[async_trait::async_trait]
    impl Broker for Held {
        fn capability(&self) -> &'static super::super::BrokerCapability {
            &crate::brokers::paper::PAPER
        }
        async fn positions(&self, _: &reqwest::Client, _: &HashMap<String, String>) -> anyhow::Result<Vec<super::super::Position>> {
            Ok(self.0.clone())
        }
    }

    fn position(symbol: &str, side: &str, qty: f64) -> super::super::Position {
        super::super::Position {
            symbol: symbol.into(),
            side: side.into(),
            qty,
            avg_price: None,
            fill_price: None,
            currency: "USD".into(),
            asset_class: "crypto".into(),
            multiplier: 1.0,
            venue: String::new(),
            account: String::new(),
            native_id: None,
            market_price: None,
            unrealized_pnl: None,
            opened_at: None,
            entry_fee: None,
            fee_currency: String::new(),
            leverage: None,
            margin_mode: None,
        }
    }

    /// The review's case: long 10,000 USD on an inverse book, sell 30,000 USD at 100,000. In one
    /// unit the order flips the position to short 20,000 USD: it does not reduce, so the gate
    /// runs every check. A sale of 5,000 USD does reduce.
    #[tokio::test]
    async fn an_inverse_flip_is_not_taken_for_a_reduction() {
        let book = Held(vec![position("BTC-PERPETUAL", "long", 10_000.0), position("ETH-PERPETUAL", "long", 10_000.0)]);
        let s = HashMap::new();
        for (usd, reduces) in [(30_000, false), (5_000, true), (10_000, true), (10_001, false)] {
            let t = inverse_ticket(Side::Sell, usd, None);
            let (gated, unit) = for_gate(&t, true);
            assert_eq!(unit, HeldUnit::InCoinAt(Decimal::from(100_000)));
            let view = GateView { inner: &book, ticker: &t.ticker, asset_type: &t.asset_type, held: unit };
            let held = held_on(&view, &s).await;
            assert_eq!(held, Some(Decimal::new(1, 1)), "10,000 USD at 100,000 is 0.1 BTC");
            assert_eq!(super::super::risk::reduces(&gated, held), reduces, "sell {usd} USD");
            // The old mix: the coin leg against the USD position took the flip for a shrink.
            if usd == 30_000 {
                assert!(super::super::risk::reduces(&gated, Some(Decimal::from(10_000))));
            }
        }
        // Another instrument's row is left in its own unit.
        let view = GateView { inner: &book, ticker: "BTC-PERPETUAL", asset_type: "crypto", held: HeldUnit::InCoinAt(Decimal::from(100_000)) };
        let rows = view.positions(&reqwest::Client::new(), &s).await.unwrap();
        assert_eq!(rows[1].qty, 10_000.0);
    }

    /// A coin-sized leg on an inverse book compares with the position in coins too; with no
    /// price at all, a USD ticket compares with USD positions as they are, and a coin one reads
    /// nothing held rather than mixing units.
    #[tokio::test]
    async fn every_leg_unit_meets_the_position_in_its_own_unit() {
        let book = Held(vec![position("BTC-PERPETUAL", "long", 10_000.0)]);
        let s = HashMap::new();
        let mut coin = inverse_ticket(Side::Sell, 0, None);
        coin.legs[0].qty = Decimal::new(3, 1);
        coin.legs[0].qty_unit = None;
        let (gated, unit) = for_gate(&coin, true);
        let view = GateView { inner: &book, ticker: &coin.ticker, asset_type: &coin.asset_type, held: unit };
        assert!(!super::super::risk::reduces(&gated, held_on(&view, &s).await));
        coin.reference = None;
        let (_, unit) = for_gate(&coin, true);
        assert_eq!(unit, HeldUnit::Unknown);
        let view = GateView { inner: &book, ticker: &coin.ticker, asset_type: &coin.asset_type, held: unit };
        assert_eq!(held_on(&view, &s).await, None);
        let mut usd = inverse_ticket(Side::Sell, 5_000, None);
        usd.reference = None;
        let (gated, unit) = for_gate(&usd, true);
        assert_eq!((unit, gated.legs[0].qty), (HeldUnit::AsIs, Decimal::from(5_000)));
        let view = GateView { inner: &book, ticker: &usd.ticker, asset_type: &usd.asset_type, held: unit };
        assert!(super::super::risk::reduces(&gated, held_on(&view, &s).await));
        // A book that sizes in the coin is read as it is.
        assert_eq!(for_gate(&inverse_ticket(Side::Sell, 5_000, None), false).1, HeldUnit::AsIs);
    }

    /// The duplicate guard compares the ticket as sent with the requests as stored: both in USD.
    #[test]
    fn the_duplicate_guard_compares_the_ticket_as_sent() {
        let t = inverse_ticket(Side::Buy, 1_000, Some(100_000));
        let stored = vec![serde_json::to_value(&t.legs[0]).unwrap()];
        let only = otw_store::broker_risk::Limits { duplicate_secs: Some(30), ..Default::default() };
        let facts = super::super::risk::Facts { recent: stored, ..Default::default() };
        let e = super::super::risk::check(&only, &facts, &t).unwrap_err();
        assert_eq!(e.code, "duplicate_order");
        // The gate's coin legs never match the stored USD request: why the guard runs on the
        // ticket as sent.
        let (gated, _) = for_gate(&t, true);
        assert!(super::super::risk::check(&only, &facts, &gated).is_ok());
    }

    /// A new size on a working inverse order, in USD, meets the size limit (in coins) as the
    /// coins it buys at the new price, else the working one: 30,000 USD at 100,000 is 0.3 BTC,
    /// under a limit of 1. With no price at all the change goes as it is.
    #[test]
    fn an_inverse_amend_meets_the_size_limit_in_coins() {
        use otw_connector_model::{OrderAmend, Price, Qty};
        let change = OrderAmend { limit: None, stop: None, qty: Some(Qty::new(Decimal::from(30_000))), tif: None };
        let max = Decimal::ONE;
        let at_working = amend_valued(&change, Some(Decimal::from(100_000)));
        assert_eq!(at_working.qty.map(|q| q.value()), Some(Decimal::new(3, 1)));
        assert!(at_working.qty.unwrap().value() <= max);
        let repriced = OrderAmend { limit: Some(Price::new(Decimal::from(50_000))), ..change };
        assert_eq!(amend_valued(&repriced, Some(Decimal::from(100_000))).qty.map(|q| q.value()), Some(Decimal::new(6, 1)));
        assert_eq!(amend_valued(&change, None).qty, change.qty);
        let price_only = OrderAmend { qty: None, ..repriced };
        assert_eq!(amend_valued(&price_only, Some(Decimal::from(100_000))).qty, None);
    }
}
