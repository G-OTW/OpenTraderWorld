//! The capital layer: the robot's money. Its allocation, its share in one position, its
//! leverage and its entries per position size the engine itself ([`super::own`]): the engine
//! decides on the robot's capital, not the strategy's. This layer then takes each decision
//! where it becomes an order (a broker robot) or a recorded robot trade (a simulated one).
//!
//! In order, each step pure:
//!
//! 1. `qty = engine_qty * scale`, the guard scale in force;
//! 2. cut down to the venue's step (never up), refused under the step or the venue's minimum;
//! 3. refused when the robot already holds `max_positions` or sent `max_orders_per_day` today;
//! 4. refused over `max_position_qty` (the execution's maximum), never cut to fit it.
//!
//! A refusal is a sentence that names the fix.

use otw_connector_model::Decimal;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

/// A decimal written as a number or as a string, empty meaning none.
fn lenient<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Decimal>, D::Error> {
    let v = Option::<Value>::deserialize(d)?;
    match v {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if s.trim().is_empty() => Ok(None),
        Some(Value::String(s)) => s.trim().parse().map(Some).map_err(serde::de::Error::custom),
        Some(Value::Number(n)) => n.to_string().parse().map(Some).map_err(serde::de::Error::custom),
        Some(other) => Err(serde::de::Error::custom(format!("expected a number, got {other}"))),
    }
}

fn lenient_count<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u32>, D::Error> {
    let v = Option::<Value>::deserialize(d)?;
    match v {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if s.trim().is_empty() => Ok(None),
        Some(Value::String(s)) => s.trim().parse().map(Some).map_err(serde::de::Error::custom),
        Some(Value::Number(n)) => n.as_u64().and_then(|n| u32::try_from(n).ok()).map(Some).ok_or_else(|| serde::de::Error::custom("expected a whole number")),
        Some(other) => Err(serde::de::Error::custom(format!("expected a whole number, got {other}"))),
    }
}

/// The robot's capital policy (`robots.capital`). Every field optional.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Capital {
    /// The most equity the robot may put to work, in the account's currency.
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub allocation: Option<Decimal>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub currency: String,
    /// One position's share of the allocation, in percent.
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub percent_in: Option<Decimal>,
    #[serde(default, deserialize_with = "lenient_count", skip_serializing_if = "Option::is_none")]
    pub max_positions: Option<u32>,
    #[serde(default, deserialize_with = "lenient_count", skip_serializing_if = "Option::is_none")]
    pub max_orders_per_day: Option<u32>,
    /// The leverage the robot sizes with. Empty: 1.
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub leverage: Option<Decimal>,
    /// Entries one position may take. Empty: 1. A broker robot sends one entry per position.
    #[serde(default, deserialize_with = "lenient_count", skip_serializing_if = "Option::is_none")]
    pub pyramiding: Option<u32>,
}

impl Capital {
    pub fn of(v: &Value) -> Capital {
        serde_json::from_value(v.clone()).unwrap_or_default()
    }

    /// Parse and check the policy a robot is saved with.
    pub fn validate(v: &Value) -> Result<Capital, String> {
        let v = if v.is_null() { Value::Object(Default::default()) } else { v.clone() };
        let c: Capital = serde_json::from_value(v).map_err(|e| format!("invalid capital: {e}"))?;
        if c.allocation.is_some_and(|a| a <= Decimal::ZERO) {
            return Err("the allocation must be more than zero, or left empty for the whole account".into());
        }
        if c.percent_in.is_some_and(|p| p <= Decimal::ZERO || p > Decimal::ONE_HUNDRED) {
            return Err("the share in one position is above 0 and at most 100 percent".into());
        }
        if c.percent_in.is_some() && c.allocation.is_none() {
            return Err("a share in one position is a share of the allocation: set the allocation too".into());
        }
        if c.max_positions == Some(0) || c.max_orders_per_day == Some(0) {
            return Err("a maximum of positions or of orders a day is at least 1, or left empty".into());
        }
        if c.leverage.is_some_and(|l| l <= Decimal::ZERO || l > Decimal::from(MAX_LEVERAGE)) {
            return Err(format!("the leverage is above 0 and at most {MAX_LEVERAGE}, or left empty for 1"));
        }
        if c.pyramiding == Some(0) || c.pyramiding.is_some_and(|n| n > MAX_PYRAMIDING) {
            return Err(format!("the entries per position are between 1 and {MAX_PYRAMIDING}, or left empty for 1"));
        }
        Ok(c)
    }
}

/// The most leverage a robot sizes with.
const MAX_LEVERAGE: u32 = 125;
/// The most entries one position may take.
const MAX_PYRAMIDING: u32 = 50;

/// What a sizing reads besides the policy.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Ask {
    pub ticker: String,
    pub engine_qty: f64,
    pub scale: Decimal,
    /// The venue's quantity step, when it states one (a simulated robot has none).
    pub step: Option<Decimal>,
    /// The venue's minimum quantity, when it states one.
    pub min_qty: Option<Decimal>,
    /// Positions the robot holds now.
    pub open_positions: i64,
    /// Opening orders it sent today.
    pub orders_today: i64,
    /// The execution's maximum (`max_position_qty`).
    pub max_qty: Option<Decimal>,
}

/// Which step refused a size. A robot on a broker records a refusal of its own caps
/// (`Positions`, `OrdersToday`) as a refused order, so it is not asked again; the venue's
/// limits and the execution's maximum are said and asked again at the next close, as before.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    Zero,
    Step,
    Minimum,
    Positions,
    OrdersToday,
    Maximum,
}

/// The size to take, or why none is.
#[derive(Debug, Clone, PartialEq)]
pub enum Sized {
    Take { qty: Decimal },
    Refuse(Why, String),
}

#[cfg(test)]
impl Sized {
    pub fn qty(&self) -> Option<Decimal> {
        match self {
            Sized::Take { qty } => Some(*qty),
            Sized::Refuse(..) => None,
        }
    }
}

fn dec(v: f64) -> Option<Decimal> {
    otw_store::paper_broker::decimal_of(v).ok()
}

/// Size one entry. See the module header for the order of the steps.
pub fn size(c: &Capital, a: &Ask) -> Sized {
    let t = &a.ticker;
    let Some(engine) = dec(a.engine_qty.abs()).filter(|q| *q > Decimal::ZERO) else {
        return Sized::Refuse(Why::Zero, format!("the strategy's size on {t} is zero: nothing was taken"));
    };
    let scale = if a.scale > Decimal::ZERO { a.scale.min(Decimal::ONE) } else { Decimal::ONE };
    let qty = engine * scale;
    let qty = match a.step.filter(|s| *s > Decimal::ZERO) {
        Some(s) => (qty / s).floor() * s,
        None => qty.trunc_with_scale(8),
    };
    if qty <= Decimal::ZERO {
        return Sized::Refuse(Why::Step, format!(
            "the bot's size {} on {t} is under the venue's step: nothing was sent. Raise the allocation, the share in one position or the leverage",
            (engine * scale).normalize()
        ));
    }
    if let Some(min) = a.min_qty.filter(|m| qty < *m) {
        return Sized::Refuse(Why::Minimum, format!(
            "the bot's size {} on {t} is under the venue's minimum of {}: nothing was sent. Raise the allocation, the share in one position or the strategy's size",
            qty.normalize(),
            min.normalize()
        ));
    }
    if let Some(n) = c.max_positions.filter(|n| a.open_positions >= *n as i64) {
        return Sized::Refuse(Why::Positions, format!("the bot already holds {} position(s), its maximum of {n}: the entry on {t} was not taken. Raise the maximum of positions", a.open_positions));
    }
    if let Some(n) = c.max_orders_per_day.filter(|n| a.orders_today >= *n as i64) {
        return Sized::Refuse(Why::OrdersToday, format!("the bot sent {} order(s) today, its maximum of {n}: the entry on {t} was not taken. Raise the maximum of orders a day", a.orders_today));
    }
    if let Some(max) = a.max_qty.filter(|m| qty > *m) {
        return Sized::Refuse(Why::Maximum, format!(
            "the bot's size {} on {t} is over this session's maximum of {}: nothing was sent. Raise the maximum, or lower the allocation or the share in one position",
            qty.normalize(),
            max.normalize()
        ));
    }
    Sized::Take { qty: qty.normalize() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn d(v: &str) -> Decimal {
        v.parse().unwrap()
    }

    fn ask(qty: f64) -> Ask {
        Ask { ticker: "BTCUSDT".into(), engine_qty: qty, scale: Decimal::ONE, ..Default::default() }
    }

    #[test]
    fn the_scale_then_the_step_cut_down_never_up() {
        let c = Capital::default();
        let a = Ask { scale: d("0.5"), step: Some(d("0.001")), ..ask(0.12345) };
        assert_eq!(size(&c, &a), Sized::Take { qty: d("0.061") });
        // No step: eight decimals, truncated.
        let a = Ask { scale: d("0.5"), ..ask(0.123456789) };
        assert_eq!(size(&c, &a).qty(), Some(d("0.06172839")));
    }

    /// The allocation sizes the engine (`own`), not this layer: a policy with one takes the
    /// engine's size as it comes, whatever the price.
    #[test]
    fn the_allocation_no_longer_cuts_the_engine_size() {
        let c = Capital::validate(&json!({ "allocation": 1000, "currency": "USDT", "percent_in": "50" })).unwrap();
        assert_eq!(size(&c, &ask(1.0)), Sized::Take { qty: d("1") });
    }

    #[test]
    fn each_refusal_names_its_fix() {
        let Sized::Refuse(_, why) = size(&Capital::default(), &Ask { step: Some(d("1")), ..ask(0.0035) }) else { panic!() };
        assert!(why.contains("under the venue's step") && why.contains("Raise the allocation"), "{why}");

        let Sized::Refuse(_, why) = size(&Capital::default(), &Ask { min_qty: Some(d("0.01")), ..ask(0.005) }) else { panic!() };
        assert!(why.contains("under the venue's minimum of 0.01"), "{why}");

        let c = Capital::validate(&json!({ "max_positions": 2, "max_orders_per_day": 3 })).unwrap();
        let Sized::Refuse(_, why) = size(&c, &Ask { open_positions: 2, ..ask(1.0) }) else { panic!() };
        assert!(why.contains("its maximum of 2") && why.contains("Raise the maximum of positions"), "{why}");
        let Sized::Refuse(_, why) = size(&c, &Ask { orders_today: 3, ..ask(1.0) }) else { panic!() };
        assert!(why.contains("3 order(s) today") && why.contains("maximum of orders a day"), "{why}");
        assert!(size(&c, &Ask { open_positions: 1, orders_today: 2, ..ask(1.0) }).qty().is_some());

        let Sized::Refuse(_, why) = size(&Capital::default(), &Ask { max_qty: Some(d("0.1")), ..ask(0.5) }) else { panic!() };
        assert!(why.contains("over this session's maximum of 0.1"), "{why}");
        let Sized::Refuse(_, why) = size(&Capital::default(), &ask(0.0)) else { panic!() };
        assert!(why.contains("zero"), "{why}");
    }

    #[test]
    fn a_policy_is_checked_when_saved() {
        let bad = |v: Value| Capital::validate(&v).unwrap_err();
        assert!(bad(json!({ "allocation": 0 })).contains("more than zero"));
        assert!(bad(json!({ "allocation": 100, "percent_in": 120 })).contains("at most 100"));
        assert!(bad(json!({ "percent_in": 50 })).contains("set the allocation too"));
        assert!(bad(json!({ "max_positions": 0 })).contains("at least 1"));
        assert!(bad(json!({ "budget": 3 })).contains("invalid capital"));
        assert!(bad(json!({ "leverage": 0 })).contains("leverage"));
        assert!(bad(json!({ "leverage": 200 })).contains("at most 125"));
        assert!(bad(json!({ "pyramiding": 0 })).contains("entries per position"));
        let ok = Capital::validate(&json!({ "leverage": "3", "pyramiding": 2 })).unwrap();
        assert_eq!((ok.leverage, ok.pyramiding), (Some(d("3")), Some(2)));
        assert_eq!(Capital::validate(&json!({ "allocation": "" })).unwrap(), Capital::default());
        assert_eq!(Capital::validate(&Value::Null).unwrap(), Capital::default());
    }
}
