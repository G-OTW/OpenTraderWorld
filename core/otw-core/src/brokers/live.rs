//! Live trading per account (onboarding spec 9): what an order on an account that moves real
//! money needs before it leaves, on top of the trading grant and the lock.
//!
//! - **Opt in per account.** Rating a broker E4 opens no account by itself: each live account
//!   takes orders only once the user turned live trading on for it (with the lock code if the
//!   trading lock is on).
//! - **Limits first.** A live account without a size (or value) limit per order, a size (or
//!   value) limit per position and a loss limit is refused, and so is turning live trading on for it.
//! - **E4, or the canary.** Below E4 the adapter itself refuses a live account. The one account
//!   a maintainer names in `OTW_LIVE_CANARY` (the live canary, E3 to E4) passes that check; it
//!   still needs the opt-in and the limits.
//! - **E2D: built from the documentation, never tested.** A broker rated E2D takes live orders
//!   on an account only when its owner turned live trading on with an explicit acknowledgement
//!   that the broker was never tested against the venue (refused without it,
//!   `live_untested_ack_required`), with the same limits as E4. Its adapter asks the venue's own
//!   check before the first live order of the session where the venue has one. An anomaly on
//!   such a broker (an unmapped answer, an unknown order left unresolved, a position drift) is
//!   handled as on every broker: logged, notified, reconciled.
//! - **Taking risk off passes.** Cancels and the kill switch are never gated here.

use std::collections::HashMap;

use otw_connector_model::ExecLevel;
use otw_store::broker_risk::Limits;
use otw_store::brokers::BrokerRow;
use uuid::Uuid;

use super::{Broker, LIVE_CANARY, LIVE_UNTESTED};

/// The environment variable naming the live canary's account id.
pub const CANARY_ENV: &str = "OTW_LIVE_CANARY";

/// A refusal of the live gate, with the code the chart answers.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveRefusal {
    pub code: &'static str,
    pub message: String,
}

/// How far the broker's order path is proven; E0 when it states no order capabilities.
pub fn level(connector: &dyn Broker) -> ExecLevel {
    connector.order_caps().map(|c| c.level).unwrap_or_default()
}

/// Whether `account` is the live canary named in [`CANARY_ENV`].
pub fn is_canary(account: Uuid) -> bool {
    std::env::var(CANARY_ENV).ok().and_then(|v| v.trim().parse::<Uuid>().ok()) == Some(account)
}

/// The limits a live account must have set, in the words the refusal uses.
pub fn missing_limits(l: &Limits) -> Vec<&'static str> {
    let mut out = Vec::new();
    if l.max_order_qty.is_none() && l.max_order_value.is_none() {
        out.push("a size or value limit per order");
    }
    if l.max_position_qty.is_none() && l.max_position_value.is_none() {
        out.push("a size or value limit per position");
    }
    if l.max_loss.is_none() {
        out.push("a loss limit");
    }
    out
}

fn limits_refusal(account: &str, missing: &[&str]) -> LiveRefusal {
    LiveRefusal {
        code: "live_limits_missing",
        message: format!("“{account}” trades real money and has no {} set: set them on the account in Settings, Brokers", missing.join(", ")),
    }
}

/// What the owner of an account on an E2D broker acknowledges to turn live trading on, stored
/// with the acknowledgement (`broker_live_untested.statement`).
pub const UNTESTED_STATEMENT: &str = "This broker's order path was built from the venue's documentation and has never been tested against the venue. Orders on this account move real money.";

/// The code a live order or opt-in is refused with on an E2D broker without the acknowledgement.
pub const UNTESTED_ACK_REQUIRED: &str = "live_untested_ack_required";
/// The E2D half of the live gate: the owner's acknowledgement.
pub fn check_untested(level: ExecLevel, acknowledged: bool) -> Result<(), LiveRefusal> {
    if level != ExecLevel::E2D {
        return Ok(());
    }
    if !acknowledged {
        return Err(LiveRefusal {
            code: UNTESTED_ACK_REQUIRED,
            message: "this broker's order path was built from the venue's documentation and never tested against the venue: turn live trading on again and acknowledge that".into(),
        });
    }
    Ok(())
}

/// Whether turning live trading on for this account may go ahead: limits set, and an order
/// path rated E4 (or this account the canary), or rated E2D with the acknowledgement asked
/// on the opt-in.
pub fn can_opt_in(account: &BrokerRow, level: ExecLevel, canary: bool, limits: &Limits, untested_ack: bool) -> Result<(), LiveRefusal> {
    if !account.trades() {
        return Err(LiveRefusal { code: "trading_off", message: format!("trading is off for “{}”: turn it on first", account.name) });
    }
    if level == ExecLevel::E2D && !canary {
        check_untested(level, untested_ack)?;
    } else if level < ExecLevel::E4 && !canary {
        return Err(LiveRefusal {
            code: "live_level",
            message: format!("this broker's order path is rated {level:?}: live orders open at E4, once its live canary passed"),
        });
    }
    let missing = missing_limits(limits);
    if !missing.is_empty() {
        return Err(limits_refusal(&account.name, &missing));
    }
    Ok(())
}

/// The check before an order that opens or changes exposure on a live account. `live` is
/// the broker's answer to [`Broker::is_live`].
pub fn check(account: &BrokerRow, live: bool, limits: &Limits) -> Result<(), LiveRefusal> {
    if !live {
        return Ok(());
    }
    if account.live_since.is_none() {
        return Err(LiveRefusal {
            code: "live_off",
            message: format!("“{}” trades real money and live trading is off for it: turn it on in Settings, Brokers", account.name),
        });
    }
    let missing = missing_limits(limits);
    if !missing.is_empty() {
        return Err(limits_refusal(&account.name, &missing));
    }
    Ok(())
}

/// Mark the canary's settings so its adapter lets a live order through below E4.
pub fn mark_canary(account: Uuid, settings: &mut HashMap<String, String>) {
    settings.remove(LIVE_CANARY);
    if is_canary(account) {
        settings.insert(LIVE_CANARY.into(), "1".into());
    }
}

/// Mark an E2D account's settings so its adapter lets a live order through: the gate passed
/// ([`check_untested`]). A stored mark is dropped first.
pub fn mark_untested(settings: &mut HashMap<String, String>, allowed: bool) {
    settings.remove(LIVE_UNTESTED);
    if allowed {
        settings.insert(LIVE_UNTESTED.into(), "1".into());
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use otw_connector_model::Decimal;
    use time::OffsetDateTime;

    fn account(trading: bool, live: bool) -> BrokerRow {
        BrokerRow {
            id: Uuid::nil(),
            broker: "binance".into(),
            name: "Main".into(),
            modules: if trading { vec!["*".into(), "trading".into()] } else { vec!["*".into()] },
            config: serde_json::Value::Null,
            created_at: OffsetDateTime::UNIX_EPOCH,
            live_since: live.then_some(OffsetDateTime::UNIX_EPOCH),
            background_trading_ack: None,
            background_trading_since: None,
        }
    }

    fn full() -> Limits {
        Limits { max_order_qty: Some(Decimal::ONE), max_position_qty: Some(Decimal::ONE), max_loss: Some(Decimal::ONE_HUNDRED), ..Default::default() }
    }

    #[test]
    fn a_sandbox_or_paper_account_needs_nothing() {
        assert!(check(&account(true, false), false, &Limits::default()).is_ok());
    }

    #[test]
    fn a_live_account_needs_the_opt_in_then_every_limit() {
        // T14 (live trading gate, spec 9)
        assert_eq!(check(&account(true, false), true, &full()).unwrap_err().code, "live_off");
        let e = check(&account(true, true), true, &Limits::default()).unwrap_err();
        assert_eq!(e.code, "live_limits_missing");
        assert!(e.message.contains("a size or value limit per order, a size or value limit per position, a loss limit"), "{}", e.message);
        let value_only = Limits { max_order_qty: None, max_order_value: Some(Decimal::ONE_HUNDRED), ..full() };
        assert!(check(&account(true, true), true, &value_only).is_ok());
        let position_value = Limits { max_position_qty: None, max_position_value: Some(Decimal::ONE_HUNDRED), ..full() };
        assert!(check(&account(true, true), true, &position_value).is_ok());
        assert!(check(&account(true, true), true, &full()).is_ok());
    }

    #[test]
    fn opting_in_needs_trading_e4_or_the_canary_and_the_limits() {
        // T14 (live trading gate, spec 9)
        assert_eq!(can_opt_in(&account(false, false), ExecLevel::E4, false, &full(), false).unwrap_err().code, "trading_off");
        assert_eq!(can_opt_in(&account(true, false), ExecLevel::E3, false, &full(), false).unwrap_err().code, "live_level");
        assert!(can_opt_in(&account(true, false), ExecLevel::E3, true, &full(), false).is_ok());
        assert_eq!(can_opt_in(&account(true, false), ExecLevel::E4, false, &Limits::default(), false).unwrap_err().code, "live_limits_missing");
        assert!(can_opt_in(&account(true, false), ExecLevel::E4, false, &full(), false).is_ok());
    }

    #[test]
    fn e2d_opens_live_only_with_the_acknowledgement_and_the_limits() {
        // Refused without the acknowledgement, with its own code.
        let e = can_opt_in(&account(true, false), ExecLevel::E2D, false, &full(), false).unwrap_err();
        assert_eq!(e.code, UNTESTED_ACK_REQUIRED);
        assert!(e.message.contains("never tested against the venue"), "{}", e.message);
        // The same limits as E4.
        assert_eq!(can_opt_in(&account(true, false), ExecLevel::E2D, false, &Limits::default(), true).unwrap_err().code, "live_limits_missing");
        assert!(can_opt_in(&account(true, false), ExecLevel::E2D, false, &full(), true).is_ok());
        // E2 and E3 stay closed whatever is acknowledged.
        assert_eq!(can_opt_in(&account(true, false), ExecLevel::E2, false, &full(), true).unwrap_err().code, "live_level");
        assert_eq!(can_opt_in(&account(true, false), ExecLevel::E3, false, &full(), true).unwrap_err().code, "live_level");
        // E4 never asks for it.
        assert!(can_opt_in(&account(true, false), ExecLevel::E4, false, &full(), false).is_ok());
    }

    #[test]
    fn e2d_orders_need_the_acknowledgement() {
        assert_eq!(check_untested(ExecLevel::E2D, false).unwrap_err().code, UNTESTED_ACK_REQUIRED);
        assert!(check_untested(ExecLevel::E2D, true).is_ok());
        // Other levels are not this check's business.
        assert!(check_untested(ExecLevel::E4, false).is_ok());
        assert!(check_untested(ExecLevel::E2, false).is_ok());
        assert!(ExecLevel::E2 < ExecLevel::E2D && ExecLevel::E2D < ExecLevel::E3);
    }

    #[test]
    fn the_untested_mark_is_the_cores_alone() {
        let mut s = HashMap::from([(LIVE_UNTESTED.to_string(), "1".to_string())]);
        mark_untested(&mut s, false);
        assert!(!s.contains_key(LIVE_UNTESTED));
        mark_untested(&mut s, true);
        assert_eq!(s.get(LIVE_UNTESTED).map(String::as_str), Some("1"));
    }

    #[test]
    fn only_the_named_account_is_marked_and_a_stored_mark_is_dropped() {
        let mut s = HashMap::from([(LIVE_CANARY.to_string(), "1".to_string())]);
        mark_canary(Uuid::new_v4(), &mut s);
        assert!(!s.contains_key(LIVE_CANARY));
    }
}
