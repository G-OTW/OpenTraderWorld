//! Numbers and time. Prices, quantities and money are exact decimals in distinct types, so a
//! price added to a quantity does not compile; time is one integer, Unix nanoseconds UTC.

use std::fmt;
use std::ops::{Add, Mul, Neg, Sub};

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

macro_rules! decimal_newtype {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub Decimal);

        impl $name {
            pub const ZERO: $name = $name(Decimal::ZERO);
            pub fn new(d: Decimal) -> Self {
                $name(d)
            }
            pub fn value(self) -> Decimal {
                self.0
            }
            pub fn is_zero(self) -> bool {
                self.0.is_zero()
            }
            pub fn is_sign_negative(self) -> bool {
                self.0.is_sign_negative() && !self.0.is_zero()
            }
        }

        impl Add for $name {
            type Output = $name;
            fn add(self, rhs: $name) -> $name {
                $name(self.0 + rhs.0)
            }
        }

        impl Sub for $name {
            type Output = $name;
            fn sub(self, rhs: $name) -> $name {
                $name(self.0 - rhs.0)
            }
        }

        impl Neg for $name {
            type Output = $name;
            fn neg(self) -> $name {
                $name(-self.0)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

decimal_newtype!(Price, "A price in the instrument's quote currency.");
decimal_newtype!(Qty, "A quantity in base units (contracts already converted by `multiplier`).");
decimal_newtype!(Money, "An amount of a currency: a notional, a fee (positive = cost), a balance.");

impl Mul<Qty> for Price {
    type Output = Money;
    fn mul(self, rhs: Qty) -> Money {
        Money(self.0 * rhs.0)
    }
}

/// Unix nanoseconds, UTC. Every timestamp in the model is one of these: no zones, no floats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Timestamp(pub i64);

const NS_PER_US: i64 = 1_000;
const NS_PER_MS: i64 = 1_000_000;
const NS_PER_S: i64 = 1_000_000_000;

impl Timestamp {
    /// `None` on overflow: a timestamp past year 2262 is a decode error, not a wrap.
    pub fn from_secs(s: i64) -> Option<Self> {
        s.checked_mul(NS_PER_S).map(Timestamp)
    }
    pub fn from_millis(ms: i64) -> Option<Self> {
        ms.checked_mul(NS_PER_MS).map(Timestamp)
    }
    pub fn from_micros(us: i64) -> Option<Self> {
        us.checked_mul(NS_PER_US).map(Timestamp)
    }
    pub fn from_nanos(ns: i64) -> Self {
        Timestamp(ns)
    }
    pub fn nanos(self) -> i64 {
        self.0
    }
    /// Floor division: a pre-1970 instant still lands on the second that contains it.
    pub fn secs(self) -> i64 {
        self.0.div_euclid(NS_PER_S)
    }
    pub fn millis(self) -> i64 {
        self.0.div_euclid(NS_PER_MS)
    }
    pub fn plus_nanos(self, ns: i64) -> Self {
        Timestamp(self.0.saturating_add(ns))
    }
    pub fn plus_secs(self, s: i64) -> Self {
        Timestamp(self.0.saturating_add(s.saturating_mul(NS_PER_S)))
    }
    pub fn to_offset_datetime(self) -> Option<time::OffsetDateTime> {
        time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(self.0)).ok()
    }
    pub fn from_offset_datetime(t: time::OffsetDateTime) -> Option<Self> {
        i64::try_from(t.unix_timestamp_nanos()).ok().map(Timestamp)
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.to_offset_datetime() {
            Some(t) => write!(f, "{t}"),
            None => write!(f, "{}ns", self.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn d(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap_or_default()
    }

    #[test]
    fn price_times_qty_is_money_and_exact() {
        let m = Price(d("0.1")) * Qty(d("3"));
        assert_eq!(m, Money(d("0.3")));
    }

    #[test]
    fn timestamp_units_round_trip_and_refuse_overflow() {
        assert_eq!(Timestamp::from_millis(1_700_000_000_123).map(|t| t.millis()), Some(1_700_000_000_123));
        assert_eq!(Timestamp::from_secs(-1).map(|t| t.secs()), Some(-1));
        assert_eq!(Timestamp::from_secs(i64::MAX), None);
        assert_eq!(Timestamp(-1).secs(), -1, "floor, not truncation, before 1970");
    }
}
