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

// The trade on alert sends through this path; the strategy on a broker lands after it, and
// until then only the tests reach `Source::Strategy`.
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
    /// A strategy executing on a broker account, by its paper session's id.
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
/// trade on alert, `backtest` for a strategy), which must be granted the account.
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
    brokers_api::require_live(state, account, connector.as_ref(), &mut settings)
        .await
        .map_err(|api| Failure { api, unknown: None })?;
    let request = serde_json::to_value(&ticket).unwrap_or(Value::Null);
    let keys: Vec<String> = ticket.legs.iter().map(|l| l.key.clone()).collect();
    let mut unknown: Option<Vec<String>> = None;
    let call = async {
        super::risk::gate(state, account, connector.as_ref(), &settings, &ticket).await?;
        super::reconcile::claim(state, account, &ticket).await?;
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
    /// Strategies executing on the account (section 4: `paper_sessions.execution` disarmed).
    pub strategies: u64,
}

/// Disarm every row that may send in the background on one account: called when background
/// trading is turned off for it, or trading is.
///
/// Each kind clears `armed_at` and `armed_by` in one statement and counts what it cleared:
/// the trades on alert (`alert_trades`) and the strategies executing on a broker (`paper_exec`).
/// A disarm never cancels a working order: that is the user's call (the kill switch), and a
/// cancel is never gated anyway.
pub async fn disarm_account(state: &AppState, account_id: Uuid) -> anyhow::Result<Disarmed> {
    let alert_trades = otw_store::alert_trades::disarm_account(&state.pool, account_id).await?;
    let strategies = crate::paper_exec::disarm_where(state, Some(account_id)).await?;
    Ok(Disarmed { alert_trades, strategies })
}

/// [`disarm_account`] on every account: the second factor turned off, the trading lock turned
/// on, the kill switch.
pub async fn disarm_all(state: &AppState) -> anyhow::Result<Disarmed> {
    let alert_trades = otw_store::alert_trades::disarm_all(&state.pool).await?;
    let strategies = crate::paper_exec::disarm_where(state, None).await?;
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
}
